//! A beat-synced take of a gain-plugin editor, with its sound.
//!
//!     cargo run --manifest-path media/Cargo.toml -p mui-reel --example gain_reel -- /tmp/gain-reel [--master]
//!
//! `--master` adds a ProRes 4444 `take.mov` and writes the layers as ProRes
//! with alpha instead of VP9 WebM. `--stage` shoots the card on the GPU as a
//! lit 3D slab over a shader background, with bloom and film grain
//! (`mui-stage`); the alpha layers and track stay the flat 2D ones.
//!
//! The editor is the gain plugin's tree (`examples/gain-plugin`) plus a tone
//! slider and a preset button, built from the stock widgets. The DSP is a
//! gated sine through the gain stage, run per video frame on the same model
//! the UI edits: the knob drag you see is the level you hear, sample-locked.
use mui::prelude::*;
use mui_reel::{Ease, Reel, ReelEvent, Script, beat, beats};

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
        .el
        .size(L)
        .el();
    let bypass = toggle(ui, "bypass", "Bypass", &mut m.bypass).el.into_el();
    let tone = slider(ui, "tone", "Tone", &mut m.tone, 110.0..=880.0)
        .el
        .into_el();
    let Response {
        el: a4,
        changed: clicked,
    } = button(ui, "a4", "A4");
    if clicked {
        m.tone = 440.0;
    }
    let db = if m.gain > 0.0 {
        format!("{:+.1} dB", 40.0 * m.gain.log10())
    } else {
        "-inf dB".into()
    };
    let meter = row([block((220.0 * m.peak).clamp(8.0, 220.0), 8.0)
        .pill()
        .fill(Role::Primary)])
    .size(220.0, 8.0)
    .pill()
    .fill(Role::Field);
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
    .fill(Role::Surface)
    .shadow(Shadow::soft(16.0))
    .id("card")
    .anchor(Align::Center, Align::Center);
    // Square: a filled node takes the theme radius, and a take has no window corners.
    stack([card]).fill(Role::Background).radius(0.0)
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
    let master = std::env::args().any(|a| a == "--master");
    let reel = Reel::new(Size::new(640.0, 360.0))
        .scale(3.0) // 1920x1080, rasterised at 3x: the punch-in stays vector-crisp
        .fps(60)
        .bpm(116.0)
        .cursor(true)
        .master(master)
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
    let mut stage = if std::env::args().any(|a| a == "--stage") {
        let (w, h) = reel.pixels();
        Some(mui_stage::Stage::new(w.into(), h.into())?)
    } else {
        None
    };
    let staged = stage.is_some();
    let bpm = 116.0;
    let font = Font::new(epaint_default_fonts::HACK_REGULAR)?;
    let mut title = None;
    let mut look = |take: &mui_reel::Take| {
        let stage = stage.as_mut().expect("look only runs with --stage");
        let title = match &title {
            Some(t) => t,
            None => title.insert(stage.text_layer(
                "title",
                std::slice::from_ref(&font),
                "GAIN",
                72.0,
                Color::srgb(0.75, 0.7, 1.0),
                6.0,
                2.0,
            )?),
        };
        // The pointer rides the card, painted into the same layer.
        let card = take.scene.isolate(&["card"])?;
        let card = match take.pointer {
            Some((p, down)) => mui_reel::with_cursor(&card, p, down),
            None => card,
        };
        stage.layer("card", &card, Size::new(640.0, 360.0), 3.0)?;
        let surface = take.scene.surface("card").ok_or("no card")?;
        let bottom = 180.0 - (surface.frame.y + surface.frame.size.height) as f32;
        let top = 180.0 - surface.frame.y as f32;
        let outline = surface.path.clone();
        let [cx, cy, zoom] = take.camera.map(|v| v as f32);
        Ok(stage
            .render(take.t, 0.0, 1, &|t| {
                let mut s = shot(t * bpm / 60.0, [cx, cy, zoom], outline.clone(), bottom);
                s.planes.insert(
                    0,
                    title
                        .clone()
                        .depth(24.0)
                        .edge([0.3, 0.2, 0.8])
                        .glow(2.2)
                        .at(0.0, top + 40.0, -220.0),
                );
                s
            })?
            .rgba8())
    };
    let look: Option<mui_reel::Look> = staged.then_some(&mut look as _);
    let manifest = reel.render_through(
        &script,
        std::path::Path::new(&out),
        &mut model,
        editor,
        Some(&mut dsp),
        look,
    )?;
    println!("{}", serde_json::to_string_pretty(&manifest)?);
    Ok(())
}

/// The 3D move, in beats: in from a low three-quarter angle, square to the
/// lens for the knob drag, then a slow drift while the tone plays.
/// The 3D move, in beats: in from a low three-quarter angle, square to the
/// lens for the knob drag (following the script's own punch-in), then a
/// slow drift while the tone plays. The card stands on a glossy floor, and
/// the lens focuses on it, so the title behind softens.
fn shot(
    beat: f64,
    [cx, cy, zoom]: [f32; 3],
    outline: std::sync::Arc<mui::geometry::Path>,
    floor: f32,
) -> mui_stage::Shot {
    use mui_stage::{Camera, Floor, Plane, Post, Shot};
    let arrive = Keys::new(0.0).to(2.0, 1.0, Ease::EMPHASIZED).at(beat) as f32;
    let drift = Keys::new(0.0)
        .hold(5.0)
        .to(12.0, 1.0, Ease::IN_OUT)
        .at(beat) as f32;
    let mut cam = Camera::front(360.0, 35.0)
        .punch([640.0, 360.0], [cx, cy], zoom)
        .orbit(
            -28.0 * (1.0 - arrive) + 10.0 * drift,
            8.0 + 6.0 * (1.0 - arrive) + 4.0 * drift,
        )
        .dolly(0.25 * (1.0 - arrive) - 0.08 * drift);
    cam.roll = -2.0 * (1.0 - arrive);
    let card = Plane::new("card", 640.0, 360.0)
        .depth(16.0)
        .edge([0.1, 0.07, 0.25])
        .glow(1.2)
        .outline(outline);
    Shot {
        planes: vec![card],
        floor: Some(Floor {
            reflect: 0.3,
            ..Floor::at(floor - 1.0)
        }),
        post: Post {
            bloom: 0.7,
            focus: cam.distance(),
            aperture: 30.0,
            max_blur: 14.0,
            ..Post::default()
        },
        ..Shot::new(cam)
    }
}
