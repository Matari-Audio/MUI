//! A plain MUI crate: `mui_editor()` hands over its editor, and
//! `mui_audio(sample_rate)`, when it has one, its sound: a closure called
//! a span at a time with the span's notes (`(key, velocity)`, velocity 0
//! a note off) at its start, filling stereo frames.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use mui_motion_bridge::NoteEvent;

fn main() {
    if let Err(e) = run() {
        eprintln!("mui-cut adapter: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let (ui, size, view) = plugin::ENTRY();
    mui_motion_bridge::mui::host::headless::claim();
    let shared = Arc::new(Mutex::new(mui_motion_bridge::mui::host::Shared { ui, view }));
    mui_motion_bridge::mui::host::headless::offer(&shared, size);
    let view = mui_motion_bridge::mui::host::headless::take().ok_or("no editor")?;
    let edit = |_: &serde_json::Value| Err("NAME has no parameters to set".to_owned());
    let rate = mui_motion_bridge::sample_rate() as f32;
    let Some(mut dsp) = SOUND_OR_NONE else {
        return mui_motion_bridge::run_headless(mui_motion_bridge::describe("NAME"), view, edit);
    };
    // The notes held, a bit each, for the patch.
    let held = Arc::new([AtomicU64::new(0), AtomicU64::new(0)]);
    let dsp_held = Arc::clone(&held);
    let mut events = Vec::with_capacity(256);
    let audio = move |notes: &[NoteEvent], samples: &mut [[f32; 2]]| {
        events.clear();
        let mut note = |key: u8, velocity: u8| {
            let (word, bit) = (&dsp_held[usize::from(key / 64 % 2)], 1u64 << (key % 64));
            if velocity > 0 {
                word.fetch_or(bit, Ordering::Relaxed);
            } else {
                word.fetch_and(!bit, Ordering::Relaxed);
            }
            events.push((key, velocity));
        };
        for n in notes {
            match *n {
                NoteEvent::On { note: k, velocity } => note(k, velocity.max(1)),
                NoteEvent::Off { note: k } => note(k, 0),
                NoteEvent::Panic => (0..128).for_each(|k| note(k, 0)),
            }
        }
        dsp(&events, samples);
    };
    let patch = move || {
        let notes: Vec<u8> =
            (0..128u8).filter(|&n| held[usize::from(n / 64)].load(Ordering::Relaxed) >> (n % 64) & 1 == 1).collect();
        serde_json::json!({"plugin": "NAME", "params": [], "routes": [], "held": notes})
    };
    let mut describe = mui_motion_bridge::describe("NAME");
    describe["notes"] = true.into();
    describe["patch"] = true.into();
    mui_motion_bridge::run_headless_with(describe, view, edit, audio, patch)
}
