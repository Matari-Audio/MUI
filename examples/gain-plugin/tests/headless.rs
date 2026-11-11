//! The headless harness against the real gain editor: a scripted drag brackets
//! the gain parameter, and the same script twice is byte-identical.
use std::sync::Arc;

use mui_gain_plugin::{GainParams, SIZE, ui, view};
use mui_headless::Scene;

fn scene() -> Scene {
    let mut bridge = mui_truce::Bridge::new(Arc::new(GainParams::new()));
    Scene::new("gain", SIZE, ui(), move |ui| view(ui, &mut bridge))
}

fn run(args: &[&str]) -> mui_headless::Value {
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
    mui_headless::run(vec![scene()], &args).expect("run")
}

#[test]
fn a_scripted_drag_reports_its_gesture_bracket_and_repeats_exactly() {
    let dir = std::env::temp_dir().join("mui-headless-gain-test");
    std::fs::create_dir_all(&dir).unwrap();
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../scripts/gain-knob-drag.json"
    );
    let mut outputs = Vec::new();
    let mut report = None;
    for n in 0..2 {
        let (png, tree, trace) = (
            dir.join(format!("{n}.png")),
            dir.join(format!("{n}.json")),
            dir.join(format!("{n}.jsonl")),
        );
        let r = run(&[
            "--script",
            script,
            "--snap",
            png.to_str().unwrap(),
            "--tree",
            tree.to_str().unwrap(),
            "--trace",
            trace.to_str().unwrap(),
        ]);
        outputs.push([png, tree, trace].map(|p| std::fs::read(p).unwrap()));
        report = Some(r);
    }
    assert_eq!(outputs[0], outputs[1], "same script, different bytes");
    let r = report.unwrap();
    let edits = r["edits"].as_array().unwrap();
    let kinds: Vec<_> = edits
        .iter()
        .map(|e| (e["id"].as_str().unwrap(), e["edit"].as_str().unwrap()))
        .collect();
    // The drag is one bracket, the ArrowDown after it another.
    let bracket = [("param/0", "begin"), ("param/0", "end")];
    assert_eq!(kinds, [bracket, bracket].concat(), "{r}");
    assert!(
        !r["values"].as_array().unwrap().is_empty(),
        "the knob never moved: {r}"
    );
    let tree: mui_headless::serde_json::Value =
        mui_headless::serde_json::from_slice(&outputs[0][1]).unwrap();
    let knob = tree["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "param/0")
        .unwrap();
    assert_eq!(knob["role"], "slider");
    assert!(knob["value"].is_number());
}

#[test]
fn every_committed_script_runs() {
    for s in ["gain-toggle", "gain-knob-drag", "gain-hover"] {
        let path = format!("{}/../scripts/{s}.json", env!("CARGO_MANIFEST_DIR"));
        run(&["--script", &path, "--tree", "/dev/null"]);
    }
}
