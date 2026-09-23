//! A second, independent adapter proving the framework does not require Kurv.
use mui::prelude::*;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
fn main() -> Result<(), String> {
    let level = Arc::new(Mutex::new(0.2f32));
    let audio_level = Arc::clone(&level);
    let edit_level = Arc::clone(&level);
    let telemetry = Arc::new(AtomicU32::new(0));
    let audio_telemetry = Arc::clone(&telemetry);
    let mut lfo_phase = 0f32;
    let mut voices = std::collections::BTreeMap::<u8, (f32, f32)>::new();
    let audio = move |events: &[mui_motion_bridge::NoteEvent], out: &mut [[f32; 2]]| {
        for e in events {
            match *e {
                mui_motion_bridge::NoteEvent::On { note, velocity } => {
                    voices.insert(note, (0., f32::from(velocity) / 127.));
                }
                mui_motion_bridge::NoteEvent::Off { note } => {
                    voices.remove(&note);
                }
                mui_motion_bridge::NoteEvent::Panic => voices.clear(),
            }
        }
        let gain = *audio_level.lock().unwrap();
        for sample in out {
            let mut value = 0.;
            for (note, (phase, velocity)) in &mut voices {
                value += phase.sin() * *velocity * gain;
                *phase = (*phase
                    + std::f32::consts::TAU * 440. * 2f32.powf((f32::from(*note) - 69.) / 12.)
                        / 48000.)
                    % std::f32::consts::TAU;
            }
            lfo_phase = (lfo_phase + 1. / 48_000.).fract();
            let modulation = 0.5 + 0.5 * (lfo_phase * std::f32::consts::TAU).sin();
            *sample = [value * modulation, value * modulation];
        }
        audio_telemetry.store(lfo_phase.to_bits(), Ordering::Relaxed);
    };
    let edit = move |command: &Value| {
        if command["op"] != "set" || command["id"] != 1 || command["field"] != "level" {
            return Err("Tone supports its level parameter only".into());
        }
        let value = command["value"]
            .as_f64()
            .filter(|v| (0.0..=0.5).contains(v))
            .ok_or("level must be 0..0.5")?;
        *edit_level.lock().unwrap() = value as f32;
        Ok(())
    };
    let mut ui = Ui::new(Theme::DEFAULT);
    ui.font = Some(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    let mut editor = mui_motion_bridge::Editor::new(ui);
    let mut capture = mui_motion_bridge::CaptureStream::default();
    let snapshot = move |_revision: u64, frame: u64, commands: &[Value]| {
        let phase = f32::from_bits(telemetry.load(Ordering::Relaxed));
        editor.advance(commands, frame, |ui, input, dt, sizes| {
            let mut gain = f64::from(*level.lock().unwrap());
            let control = knob(ui, "gain", "Level", &mut gain, 0.0..=0.5)
                .0
                .el()
                .size(100., 100.);
            *level.lock().unwrap() = gain as f32;
            let tree = column([
                text("MUI TONE / LIVE").text_size(32.),
                column([
                    text("Sine + native 1 Hz tremolo"),
                    control,
                    row([leaf(f64::from(phase) * 400. + 1., 12.).fill(Primary)])
                        .size(410., 24.)
                        .id("phase"),
                    text(format!("LFO phase {phase:.3}")),
                ])
                .pad(24.)
                .gap(18.)
                .size(500., 310.)
                .fill(Surface)
                .radius(14.)
                .id("voice"),
            ])
            .pad(28.)
            .gap(20.)
            .size(640., 460.)
            .fill(Background)
            .id("root");
            ui.frame(
                mui_motion_bridge::Editor::layout(tree, sizes)?,
                Some(Size::new(640., 460.)),
                input,
                dt,
            )
            .map_err(|e| format!("{e:?}"))?;
            Ok(())
        })?;
        let scene = editor.ui.scene().ok_or("empty scene")?;
        let roots = if editor.selection.is_empty() {
            mui_motion_bridge::discover_parts(scene, 640., 460.)
        } else {
            editor.selection.clone()
        };
        let mut result = capture.frame(scene, 640, 460, 1., &roots)?;
        let value = *level.lock().unwrap();
        result["modules"] = json!([{"id":1,"part":"voice","kind":"Sine","parameters":[{"id":"level","label":"Level","min":0,"max":0.5,"step":0.01,"value":value}]}]);
        result["telemetry"] = json!({"phase":phase});
        Ok(result)
    };
    mui_motion_bridge::run_live(
        json!({"name":"MUI Tone","notes":true,"addKinds":[],"move":false,"delete":false}),
        audio,
        edit,
        snapshot,
    )
}
