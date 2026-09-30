//! Optional development host for any MUI instrument: real audio, live scene
//! captures, plugin-owned edits, and a sample-clocked recording protocol.
//! This is not a DAW audio backend. Plugins retain ownership of DSP and models.
#![deny(unsafe_code)]
mod capture;
pub use capture::{CaptureStream, capture, capture_frame, discover_parts, discover_tree};
mod editor;
pub use editor::Editor;
mod headless;
pub use headless::{ParamSet, describe, param_set, run_headless};
/// The MUI this bridge is built on: a generated adapter names it here, so
/// Cargo loads it as the bridge's path dependency.
pub use mui;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

pub const SAMPLE_RATE: u32 = 48_000;
pub const BLOCK_FRAMES: usize = 480;

/// The clock a host asked for in `MUI_BRIDGE_CLOCK` when it started the
/// adapter: `manual:<rate>` hands the sample clock to the host, which moves
/// it with `{"op": "advance", "to": sample, "notes": [..]}` and gets every
/// sample up to there back before `{"type": "advanced"}`. No blocks run on
/// their own and the editor draws only when asked, so the same commands
/// give the same audio and frames every time. Unset (or anything else) is
/// the wall clock at [`SAMPLE_RATE`].
fn manual_clock() -> Option<u32> {
    static CLOCK: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    *CLOCK.get_or_init(|| {
        std::env::var("MUI_BRIDGE_CLOCK")
            .ok()?
            .strip_prefix("manual:")?
            .parse()
            .ok()
            .filter(|r| (8_000..=384_000).contains(r))
    })
}

/// The sample rate the audio callback renders at: the host's manual clock's
/// rate, else [`SAMPLE_RATE`]. Read it when setting up the DSP.
pub fn sample_rate() -> u32 {
    manual_clock().unwrap_or(SAMPLE_RATE)
}

/// `{"op": "advance"}`'s notes, each at its absolute sample, in order.
fn advance_notes(v: &Value) -> Result<Vec<(u64, Value, NoteEvent)>, String> {
    let mut out = Vec::new();
    for n in v["notes"].as_array().map_or(&[][..], Vec::as_slice) {
        let at = n["at"].as_u64().ok_or("a note needs its sample `at`")?;
        out.push((at, n.clone(), note_event(n)?));
    }
    if out.len() > 4096 {
        return Err("at most 4096 notes an advance".into());
    }
    out.sort_by_key(|n| n.0);
    Ok(out)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoteEvent {
    On { note: u8, velocity: u8 },
    Off { note: u8 },
    Panic,
}

pub fn note_event(v: &Value) -> Result<NoteEvent, String> {
    if v["op"] == "panic" {
        return Ok(NoteEvent::Panic);
    }
    let note = v["note"]
        .as_u64()
        .filter(|n| *n <= 127)
        .ok_or("note must be 0..127")? as u8;
    match v["op"].as_str() {
        Some("note_on") => Ok(NoteEvent::On {
            note,
            velocity: v
                .get("velocity")
                .map_or(Some(96), Value::as_u64)
                .filter(|v| (1..=127).contains(v))
                .ok_or("velocity must be 1..127")? as u8,
        }),
        Some("note_off") => Ok(NoteEvent::Off { note }),
        _ => Err("unknown note operation".into()),
    }
}

/// A plugin adapter. Audio is moved to its own thread; edits and snapshots run
/// on the caller thread. Share the plugin's existing thread-safe parameter model.
/// `describe` returns plugin name/capabilities; `snapshot` returns the capture
/// manifest and optional editable modules. See tools/film/README.md.
///
/// The process's stdin/stdout are reserved for this versioned local protocol.
/// No graphics work occurs on the audio thread. The supplied audio callback must
/// render the given stereo slice at SAMPLE_RATE, consuming events at its start.
/// Compatibility entry point for command-driven, static adapters.
pub fn run(
    describe: &Value,
    audio: impl FnMut(&[NoteEvent], &mut [[f32; 2]]) + Send + 'static,
    edit: impl FnMut(&Value) -> Result<(), String>,
    mut snapshot: impl FnMut(u64) -> Result<Value, String>,
) -> Result<(), String> {
    host(
        describe,
        audio,
        edit,
        move |revision, _, _| snapshot(revision),
        false,
    )
}

/// Persistent editor callback. `sample_frame` is sampled before building the
/// UI; inputs remain ordered, including press/release edges in the same tick.
/// The callback owns the editor and returns an in-memory `capture_frame`.
pub fn run_live(
    mut describe: Value,
    audio: impl FnMut(&[NoteEvent], &mut [[f32; 2]]) + Send + 'static,
    edit: impl FnMut(&Value) -> Result<(), String>,
    frame: impl FnMut(u64, u64, &[Value]) -> Result<Value, String>,
) -> Result<(), String> {
    describe["liveEditor"] = json!(true);
    host(&describe, audio, edit, frame, true)
}

fn host(
    describe: &Value,
    mut audio: impl FnMut(&[NoteEvent], &mut [[f32; 2]]) + Send + 'static,
    mut edit: impl FnMut(&Value) -> Result<(), String>,
    mut snapshot: impl FnMut(u64, u64, &[Value]) -> Result<Value, String>,
    live: bool,
) -> Result<(), String> {
    let running = Arc::new(AtomicBool::new(true));
    let clock = Arc::new(AtomicU64::new(0));
    let (wire, incoming) = mpsc::sync_channel::<(u8, Vec<u8>)>(128);
    let writing = Arc::clone(&running);
    std::thread::spawn(move || {
        let mut output = std::io::stdout().lock();
        for (kind, bytes) in incoming {
            if output
                .write_all(&[kind])
                .and_then(|_| output.write_all(&(bytes.len() as u32).to_le_bytes()))
                .and_then(|_| output.write_all(&bytes))
                .and_then(|_| output.flush())
                .is_err()
            {
                writing.store(false, Ordering::Release);
                break;
            }
        }
    });
    let (notes, note_rx) = mpsc::channel::<(Value, NoteEvent)>();
    let (advances, advance_rx) = mpsc::channel::<(u64, Vec<(u64, Value, NoteEvent)>)>();
    let manual = manual_clock().is_some();
    let (commands, command_rx) = mpsc::channel::<Value>();
    let reader_wire = wire.clone();
    let reading = Arc::clone(&running);
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let result = line.map_err(|e| e.to_string()).and_then(|s| {
                if s.len() > 65536 {
                    Err("command too large".into())
                } else {
                    serde_json::from_str::<Value>(&s).map_err(|e| e.to_string())
                }
            });
            let result = result.and_then(|v| {
                if matches!(v["op"].as_str(), Some("note_on" | "note_off" | "panic")) {
                    let event = note_event(&v)?;
                    notes.send((v, event)).map_err(|e| e.to_string())
                } else if v["op"] == "advance" {
                    // Straight to the audio thread: a slow editor frame on
                    // the main thread never holds the sound up.
                    if !manual {
                        return Err("advance needs MUI_BRIDGE_CLOCK=manual:<rate>".into());
                    }
                    let to = v["to"].as_u64().ok_or("advance needs `to`, a sample")?;
                    advances
                        .send((to, advance_notes(&v)?))
                        .map_err(|e| e.to_string())
                } else {
                    commands.send(v).map_err(|e| e.to_string())
                }
            });
            if let Err(e) = result {
                let _ = reader_wire.send((
                    b'J',
                    json!({"type":"error","message":e}).to_string().into_bytes(),
                ));
            }
        }
        reading.store(false, Ordering::Release);
    });
    let audio_wire = wire.clone();
    let playing = Arc::clone(&running);
    let audio_clock = Arc::clone(&clock);
    std::thread::spawn(move || {
        let started = Instant::now();
        let rate = sample_rate();
        let mut frame = 0u64;
        let mut samples = vec![[0.; 2]; BLOCK_FRAMES];
        let mut active = std::collections::BTreeSet::new();
        let mut queue: Vec<(u64, Value, NoteEvent)> = Vec::new();
        // Renders `n` samples with `events` at their start, and sends them.
        let mut block = |frame: u64, n: usize, events: &[NoteEvent]| {
            let samples = &mut samples[..n];
            samples.fill([0.; 2]);
            audio(events, samples);
            // ponytail: local development host uses a bounded allocating wire;
            // keep allocation-free transport in the plugin's production host.
            let mut bytes = Vec::with_capacity(8 + n * 8);
            bytes.extend_from_slice(&frame.to_le_bytes());
            for sample in samples.iter().flatten() {
                bytes.extend_from_slice(
                    &(if sample.is_finite() {
                        sample.clamp(-1., 1.)
                    } else {
                        0.
                    })
                    .to_le_bytes(),
                );
            }
            audio_wire.send((b'A', bytes)).is_ok()
        };
        // Tracks held notes (a retrigger releases first) and tells the host.
        let note = |active: &mut std::collections::BTreeSet<u8>,
                    events: &mut Vec<NoteEvent>,
                    frame: u64,
                    command: Value,
                    event: NoteEvent| {
            match event {
                NoteEvent::On { note, .. } => {
                    if active.remove(&note) {
                        events.push(NoteEvent::Off { note });
                    }
                    active.insert(note);
                }
                NoteEvent::Off { note } => {
                    active.remove(&note);
                }
                NoteEvent::Panic => active.clear(),
            }
            events.push(event);
            let _ = audio_wire.send((
                b'J',
                json!({"type":"note","frame":frame,"command":command,"active":active})
                    .to_string()
                    .into_bytes(),
            ));
        };
        while playing.load(Ordering::Acquire) {
            if manual {
                // The host moves the clock: render exactly up to `to`,
                // splitting blocks where notes land.
                let Ok((to, timed)) = advance_rx.recv() else {
                    break;
                };
                // Played notes land now, timed ones at their sample; same
                // sample keeps the order sent. Past `to` waits.
                queue.extend(note_rx.try_iter().map(|(c, e)| (frame, c, e)));
                queue.extend(timed);
                queue.sort_by_key(|n| n.0);
                let mut events = Vec::new();
                while frame < to {
                    let due = queue.partition_point(|n| n.0 <= frame);
                    for (_, command, event) in queue.drain(..due) {
                        note(&mut active, &mut events, frame, command, event);
                    }
                    let next = queue.first().map_or(to, |n| n.0.min(to));
                    let n = (next - frame).min(BLOCK_FRAMES as u64) as usize;
                    if !block(frame, n, &events) {
                        return;
                    }
                    events.clear();
                    frame += n as u64;
                    audio_clock.store(frame, Ordering::Release);
                }
                let _ = audio_wire.send((
                    b'J',
                    json!({"type":"advanced","frame":frame})
                        .to_string()
                        .into_bytes(),
                ));
                continue;
            }
            let mut events = Vec::new();
            for (command, event) in note_rx.try_iter() {
                note(&mut active, &mut events, frame, command, event);
            }
            if !block(frame, BLOCK_FRAMES, &events) {
                break;
            }
            frame += BLOCK_FRAMES as u64;
            audio_clock.store(frame, Ordering::Release);
            if let Some(wait) = (started + Duration::from_secs_f64(frame as f64 / f64::from(rate)))
                .checked_duration_since(Instant::now())
            {
                std::thread::sleep(wait);
            }
        }
    });
    let send = |value: Value| {
        wire.send((b'J', value.to_string().into_bytes()))
            .map_err(|e| e.to_string())
    };
    send(
        json!({"type":"hello","version":1,"sampleRate":sample_rate(),"manualClock":manual,"blockFrames":BLOCK_FRAMES,"plugin":describe}),
    )?;
    let mut revision = 0;
    let mut pending = Vec::new();
    let mut deadline = Instant::now();
    while running.load(Ordering::Acquire) {
        let timeout = if live && !manual {
            deadline.saturating_duration_since(Instant::now())
        } else {
            Duration::from_millis(100)
        };
        let command = if revision == 0 {
            None
        } else {
            match command_rx.recv_timeout(timeout) {
                Ok(c) => Some(c),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        };
        let mut dirty = revision == 0;
        if let Some(command) = command {
            if command["op"] == "input" {
                if pending.len() < 256 {
                    pending.push(command);
                } else {
                    send(json!({"type":"error","message":"Input queue full"}))?;
                }
            } else {
                let result = if command["op"] == "snapshot" {
                    Ok(())
                } else {
                    edit(&command)
                };
                match result {
                    Ok(()) => {
                        dirty = true;
                        send(
                            json!({"type":"edit","revision":revision,"frame":clock.load(Ordering::Acquire),"command":command}),
                        )?;
                    }
                    Err(e) => send(json!({"type":"error","message":e}))?,
                }
            }
        }
        if dirty || (live && !manual && Instant::now() >= deadline) {
            let frame = clock.load(Ordering::Acquire);
            match snapshot(revision, frame, &pending) {
                Ok(mut value) => {
                    value["type"] = json!("scene");
                    value["revision"] = json!(revision);
                    value["frame"] = json!(frame);
                    send(value)?;
                }
                Err(e) => send(json!({"type":"error","message":e}))?,
            }
            pending.clear();
            revision += 1;
            // No catch-up bursts when a CPU render is slower than the target.
            deadline = (deadline + Duration::from_millis(33)).max(Instant::now());
        }
    }
    running.store(false, Ordering::Release);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_note_boundaries_before_the_audio_callback() {
        assert_eq!(
            note_event(&json!({"op":"note_on","note":69})).unwrap(),
            NoteEvent::On {
                note: 69,
                velocity: 96
            }
        );
        for v in [
            json!({"op":"note_on","note":128}),
            json!({"op":"note_on","note":-1}),
            json!({"op":"note_on","note":69,"velocity":0}),
            json!({"op":"note_off","note":60.5}),
            json!({"op":"whatever","note":1}),
        ] {
            assert!(note_event(&v).is_err());
        }
        assert_eq!(
            note_event(&json!({"op":"panic"})).unwrap(),
            NoteEvent::Panic
        );
    }
}
