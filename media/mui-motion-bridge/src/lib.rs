//! Optional development host for any MUI instrument: real audio, live scene
//! captures, plugin-owned edits, and a sample-clocked recording protocol.
//! This is not a DAW audio backend. Plugins retain ownership of DSP and models.
#![deny(unsafe_code)]
mod capture;
pub use capture::{CaptureStream, capture, capture_frame, discover_parts};
mod editor;
pub use editor::Editor;
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
        let mut frame = 0u64;
        let mut samples = vec![[0.; 2]; BLOCK_FRAMES];
        let mut active = std::collections::BTreeSet::new();
        while playing.load(Ordering::Acquire) {
            let mut events = Vec::new();
            for (command, event) in note_rx.try_iter() {
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
            }
            samples.fill([0.; 2]);
            audio(&events, &mut samples);
            // ponytail: local development host uses a bounded allocating wire;
            // keep allocation-free transport in the plugin's production host.
            let mut bytes = Vec::with_capacity(8 + BLOCK_FRAMES * 8);
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
            if audio_wire.send((b'A', bytes)).is_err() {
                break;
            }
            frame += BLOCK_FRAMES as u64;
            audio_clock.store(frame, Ordering::Release);
            if let Some(wait) = (started
                + Duration::from_secs_f64(frame as f64 / f64::from(SAMPLE_RATE)))
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
        json!({"type":"hello","version":1,"sampleRate":SAMPLE_RATE,"blockFrames":BLOCK_FRAMES,"plugin":describe}),
    )?;
    let mut revision = 0;
    let mut pending = Vec::new();
    let mut deadline = Instant::now();
    while running.load(Ordering::Acquire) {
        let timeout = if live {
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
        if dirty || (live && Instant::now() >= deadline) {
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
