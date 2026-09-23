//! A beat-synced take of a gain-plugin editor, with its sound.
//!
//!     cargo run -p mui-reel --release --example gain_reel -- /tmp/gain-reel
//!
//! The editor is the gain plugin's tree (`examples/gain-plugin`) plus a tone
//! slider and a preset button, built from the stock widgets. The DSP is a
//! gated sine through the gain stage, run per video frame on the same model
//! the UI edits: the knob drag you see is the level you hear, sample-locked.
use mui::prelude::*;
use mui_reel::{beat, beats, Ease, Reel, ReelEvent, Script};

struct Gain {
    gain: f64,
    bypass: bool,
    tone: f64,
    /// DSP state: oscillator phase, gate envelope, and the meter's peak.
    phase: f64,
    env: f32,
    gate: bool,
    peak: f64,
}

fn editor(ui: &mut Ui, m: &mut Gain) -> El {
    let gain = knob(ui, "gain", "Gain", &mut m.gain, 0.0..=1.0)
        .0
        .size(L)
        .el();
    let bypass = toggle(ui, "bypass", &mut m.bypass).0.el();
    let tone = slider(ui, "tone", "Tone", &mut m.tone, 110.0..=880.0)
        .0
        .el();
    let (a4, clicked) = button(ui, "a4", "A4");
    if clicked {
        m.tone = 440.0;
    }
    let db = if m.gain > 0.0 {
        format!("{:+.1} dB", 40.0 * m.gain.log10())
    } else {
        "-inf dB".into()
    };
    let meter = row([leaf((220.0 * m.peak).clamp(8.0, 220.0), 8.0)
        .pill()
        .fill(Primary)])
    .size(220.0, 8.0)
    .pill()
    .fill(Field);
    let card = col![
        row![
            col![gain].id("gain-panel"),
            col![
                title(db),
                row![caption("Bypass"), bypass].gap(S).center(),
                row![tone, a4.el()].gap(S).center(),
            ]
            .gap(S),
        ]
        .gap(L)
        .center(),
        meter,
    ]
    .gap(M)
    .pad(L)
    .radius(20.0)
    .fill(Surface)
    .shadow(Shadow::soft(16.0))
    .anchor(Align::Center, Align::Center);
    // Square: a filled node takes the theme radius, and a take has no window corners.
    overlay([card]).fill(Background).radius(0.0)
}

/// One video frame of audio: a sine at `tone`, gated by the script's notes,
/// through `gain` squared (a fader law), or straight through when bypassed.
fn dsp(m: &mut Gain, events: &[ReelEvent], out: &mut [[f32; 2]]) {
    for e in events {
        match e {
            ReelEvent::NoteOn { .. } => m.gate = true,
            ReelEvent::NoteOff { .. } => m.gate = false,
            _ => {}
        }
    }
    let g = if m.bypass { 1.0 } else { m.gain * m.gain } as f32;
    let step = m.tone / 48_000.0;
    let mut peak = 0.0f32;
    for s in out.iter_mut() {
        let target = if m.gate { 1.0 } else { 0.0 };
        m.env += (target - m.env) * 0.002;
        let v = (m.phase * std::f64::consts::TAU).sin() as f32 * 0.4 * m.env * g;
        m.phase = (m.phase + step).fract();
        *s = [v, v];
        peak = peak.max(v.abs());
    }
    m.peak = f64::from(peak / 0.4);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or("gain-reel".into());
    let reel = Reel::new(Size::new(640.0, 360.0))
        .scale(3.0) // 1920x1080, rasterised at 3x: the punch-in stays vector-crisp
        .fps(60)
        .bpm(116.0)
        .cursor(true)
        .font(Font::new(epaint_default_fonts::HACK_REGULAR)?);
    let script = Script::new()
        .note_on(57, 100)
        .at(beat(0.25))
        .move_to("gain", beats(1.0))
        .at(beat(1.5))
        .camera_focus("gain-panel", 60.0, Spring::new(0.7, 0.9))
        .at(beat(2.0))
        .drag("gain", (0.0, -70.0), beats(2.0), Ease::IN_OUT)
        .at(beat(5.0))
        .camera_reset(Spring::new(0.6, 1.0))
        .move_to("bypass", beats(1.0))
        .at(beat(6.5))
        .click("bypass")
        .at(beat(7.0))
        .move_to("tone", beats(0.75))
        .at(beat(8.0))
        .drag("tone", (60.0, 0.0), beats(1.5), Ease::IN_OUT)
        .at(beat(10.0))
        .move_to("a4", beats(0.5))
        .at(beat(10.75))
        .click("a4")
        .at(beat(11.5))
        .note_off(57)
        .layers(&["gain-panel"])
        .end(beat(12.0));
    let mut model = Gain {
        gain: 0.3,
        bypass: false,
        tone: 220.0,
        phase: 0.0,
        env: 0.0,
        gate: false,
        peak: 0.0,
    };
    let manifest = reel.render(
        &script,
        std::path::Path::new(&out),
        &mut model,
        editor,
        Some(&mut dsp),
    )?;
    println!("{}", serde_json::to_string_pretty(&manifest)?);
    Ok(())
}
