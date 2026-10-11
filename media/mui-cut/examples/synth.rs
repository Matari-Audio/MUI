//! A small MUI plugin editor behind the mui-motion-bridge live protocol:
//! what `examples/plugin.cut.json` films and the tests drive. Real MUI
//! widgets (knobs, sliders, a toggle, a meter) on a parameter model the
//! host sets with `{"op": "set", "id": panel, "field": name, "value": v}`
//! and the pointer drags, in named panels (`head`, `osc`, `filter`, `env`,
//! `out`) that `discover_parts` splits out. Notes play a sine per key
//! (attack and release from `env`, gain from `out.level`) at the bridge's
//! sample rate; the meter shows the last block's peak and the snapshot
//! carries a `patch`.
//!
//! Its UI is a function of the model and the input alone (every tween is
//! settled each frame), so a state captures the same pixels every time.
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};

use mui::prelude::*;
use serde_json::{Value, json};

const W: f64 = 720.;
const H: f64 = 510.;
/// Captured at twice the UI's pixels, so a part filmed up close stays sharp.
const SCALE: f64 = 2.;

type Model = BTreeMap<(&'static str, &'static str), f64>;

fn main() -> Result<(), String> {
    let model = std::sync::Arc::new(std::sync::Mutex::new(Model::from([
        (("osc", "shape"), 0.35),
        (("osc", "detune"), 0.2),
        (("osc", "sync"), 0.),
        (("filter", "cutoff"), 0.6),
        (("filter", "res"), 0.25),
        (("filter", "drive"), 0.4),
        (("env", "attack"), 0.1),
        (("env", "decay"), 0.45),
        (("env", "sustain"), 0.7),
        (("env", "release"), 0.3),
        (("out", "level"), 0.8),
    ])));
    // The last block's peak, f32 bits: the meter reads it.
    let peak = std::sync::Arc::new(AtomicU32::new(0));
    let audio = voice(model.clone(), peak.clone());
    let patch_model = model.clone();
    let edits = model.clone();
    let edit = move |c: &Value| {
        if c["op"] != "set" {
            return Err("MUI Synth takes `set` only".into());
        }
        let v = c["value"]
            .as_f64()
            .filter(|v| (0.0..=1.0).contains(v))
            .ok_or("value must be 0..1")?;
        let mut m = edits.lock().map_err(|e| e.to_string())?;
        let slot = m
            .iter_mut()
            .find(|((id, f), _)| c["id"] == *id && c["field"] == *f)
            .ok_or("no such parameter")?;
        *slot.1 = v;
        Ok(())
    };
    let mut ui = Ui::default();
    ui.set_font(Some(
        Font::new(ttf_inter::REGULAR).map_err(|e| e.to_string())?,
    ));
    let mut editor = mui_motion_bridge::Editor::new(ui);
    let mut capture = mui_motion_bridge::CaptureStream::default();
    let frame = move |_rev: u64, clock: u64, inputs: &[Value]| {
        editor.advance(inputs, clock, |ui, input, _dt, sizes| {
            let mut m = model.lock().map_err(|e| e.to_string())?;
            let size = mui_motion_bridge::Editor::viewport(sizes, Size::new(W, H));
            let level = f64::from(f32::from_bits(peak.load(Ordering::Relaxed)));
            let tree = view(ui, &mut m, size, level);
            // Every tween settled: the pixels depend on the model alone.
            ui.frame(
                mui_motion_bridge::Editor::layout(tree, sizes)?,
                Some(size),
                input,
                1.,
            )
            .map_err(|e| format!("{e:?}"))?;
            Ok(())
        })?;
        let scene = editor.ui.scene().ok_or("no scene yet")?;
        let size = editor.view(Size::new(W, H));
        let roots = editor.roots(size.width, size.height);
        let mut v = capture.frame(
            scene,
            (size.width * SCALE) as u16,
            (size.height * SCALE) as u16,
            SCALE,
            &roots,
        )?;
        let m = patch_model.lock().map_err(|e| e.to_string())?;
        v["patch"] = json!({
            "plugin": "MUI Synth",
            "params": m.iter().map(|((id, f), v)| json!({"id": format!("{id}.{f}"), "name": f, "group": id, "value": v, "text": format!("{v:.2}"), "norm": v})).collect::<Vec<_>>(),
            "routes": [{"source": "ENV 01", "target": "Level", "depth": m[&("env", "sustain")], "live": f32::from_bits(peak.load(Ordering::Relaxed))}],
            "held": [],
        });
        Ok(v)
    };
    mui_motion_bridge::run_live(
        json!({"name": "MUI Synth", "notes": true, "patch": true, "addKinds": [], "move": false, "delete": false}),
        audio,
        edit,
        frame,
    )
}

/// The DSP: a sine per held key, a linear attack and release.
fn voice(
    model: std::sync::Arc<std::sync::Mutex<Model>>,
    peak: std::sync::Arc<AtomicU32>,
) -> impl FnMut(&[mui_motion_bridge::NoteEvent], &mut [[f32; 2]]) + Send + 'static {
    use mui_motion_bridge::NoteEvent as N;
    let rate = f64::from(mui_motion_bridge::sample_rate());
    // (note, velocity 0..1, phase, gain, held)
    let mut voices: Vec<(u8, f64, f64, f64, bool)> = Vec::new();
    move |events, out| {
        for e in events {
            match *e {
                N::On { note, velocity } => {
                    voices.retain(|v| v.0 != note);
                    voices.push((note, f64::from(velocity) / 127., 0., 0., true));
                }
                N::Off { note } => voices
                    .iter_mut()
                    .filter(|v| v.0 == note)
                    .for_each(|v| v.4 = false),
                N::Panic => voices.clear(),
            }
        }
        let (attack, release, level) = model.lock().map_or((0.1, 0.3, 0.8), |m| {
            (
                m[&("env", "attack")],
                m[&("env", "release")],
                m[&("out", "level")],
            )
        });
        let (up, down) = (
            1. / (rate * (0.002 + attack * 0.2)),
            1. / (rate * (0.01 + release * 0.5)),
        );
        let mut max = 0f32;
        for s in out.iter_mut() {
            let mut x = 0.;
            for v in &mut voices {
                v.3 = if v.4 {
                    (v.3 + up).min(1.)
                } else {
                    (v.3 - down).max(0.)
                };
                let hz = 440. * 2f64.powf((f64::from(v.0) - 69.) / 12.);
                v.2 = (v.2 + hz / rate).fract();
                x += (v.2 * std::f64::consts::TAU).sin() * v.1 * v.3 * 0.25 * level;
            }
            let x = x as f32;
            *s = [x, x];
            max = max.max(x.abs());
        }
        voices.retain(|v| v.4 || v.3 > 0.);
        peak.store(max.to_bits(), Ordering::Relaxed);
    }
}

fn view(ui: &mut Ui, m: &mut Model, size: Size, peak: f64) -> El {
    let dial = |ui: &mut Ui, m: &mut Model, key: (&'static str, &'static str), label| {
        let v = m.get_mut(&key).expect("a known parameter");
        knob(ui, format!("{}-{}", key.0, key.1), label, v, 0.0..=1.0)
            .el
            .into_el()
    };
    let fader = |ui: &mut Ui, m: &mut Model, key: (&'static str, &'static str), label| {
        let v = m.get_mut(&key).expect("a known parameter");
        slider(ui, format!("{}-{}", key.0, key.1), label, v, 0.0..=1.0)
            .el
            .into_el()
            .w(Len::Pct(100.))
    };
    let panel = |name: &str, body: El| {
        col([text(name.to_uppercase()).fill(Role::Dim), body])
            .gap(12.)
            .pad(18.)
            .radius(14.)
            .fill(Role::Surface)
            .grow(1.)
            .id(name)
    };
    let mut sync = m[&("osc", "sync")] > 0.5;
    let sync_el = row([
        toggle(ui, "osc-sync", "Sync", &mut sync).el.into_el(),
        text("Sync"),
    ])
    .gap(8.)
    .align(Align::Center);
    m.insert(("osc", "sync"), f64::from(u8::from(sync)));
    let osc = panel(
        "osc",
        row([
            dial(ui, m, ("osc", "shape"), "Shape"),
            dial(ui, m, ("osc", "detune"), "Detune"),
            sync_el,
        ])
        .gap(18.)
        .align(Align::Center),
    );
    let filter = panel(
        "filter",
        row([
            dial(ui, m, ("filter", "cutoff"), "Cutoff"),
            dial(ui, m, ("filter", "res"), "Res"),
            dial(ui, m, ("filter", "drive"), "Drive"),
        ])
        .gap(18.),
    );
    let env = panel(
        "env",
        col([
            fader(ui, m, ("env", "attack"), "Attack"),
            fader(ui, m, ("env", "decay"), "Decay"),
            fader(ui, m, ("env", "sustain"), "Sustain"),
            fader(ui, m, ("env", "release"), "Release"),
        ])
        .gap(8.),
    );
    let out = panel(
        "out",
        col([
            dial(ui, m, ("out", "level"), "Level"),
            meter(ui, "out-meter", peak.min(1.)).w(Len::Pct(100.)),
        ])
        .gap(14.)
        .align(Align::Center),
    );
    let head = row([
        text("MUI SYNTH").text_size(22.),
        spacer(),
        text("plugin layer fixture").fill(Role::Dim),
    ])
    .align(Align::Center)
    .pad((18., 12.))
    .radius(12.)
    .fill(Role::Surface)
    .id("head");
    col([head, row([osc, filter]).gap(12.), row([env, out]).gap(12.)])
        .gap(12.)
        .pad(16.)
        .size(size.width, size.height)
        .fill(Role::Background)
        .id("root")
}
