//! Plugin layers that play notes and reflow, end to end on the synth
//! fixture (`examples/synth.rs`, built by `cargo test`): the soundtrack is
//! sample-accurate and the same every time, it is muxed into the render,
//! the UI is captured at a keyed size (reflowed, not scaled), and each
//! capture carries the plugin's patch.
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_mui-cut");
const RATE: u32 = 44_100;

fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn has(tool: &str) -> bool {
    Command::new(tool).arg("-version").output().is_ok()
}

fn synth() -> PathBuf {
    let s = Path::new(BIN).parent().unwrap().join("examples/synth");
    assert!(s.is_file(), "{} is missing: `cargo build -p mui-cut --example synth`", s.display());
    s
}

/// A one-scene project around one synth layer (`layer` merged in).
fn project(name: &str, duration: f64, layer: Value) -> PathBuf {
    let mut l = json!({"id": "synth", "kind": "plugin", "source": {"bin": synth()}});
    for (k, v) in layer.as_object().unwrap() {
        l[k] = v.clone();
    }
    let p = json!({
        "size": [320, 180], "fps": 30.0, "sample_rate": RATE,
        "scenes": [{"name": "s", "duration": duration, "background": "#0e0e10", "layers": [l]}],
    });
    let file = scratch(name).join("p.cut.json");
    std::fs::write(&file, p.to_string()).unwrap();
    file
}

fn run(project: &Path, args: &[&str]) -> String {
    let o = Command::new(BIN).args(args).arg(project).output().unwrap();
    let err = String::from_utf8_lossy(&o.stderr).into_owned();
    assert!(o.status.success(), "{err}");
    err
}

fn load(project: &Path) -> mui_cut::Project {
    mui_cut::Project::load(&std::fs::read_to_string(project).unwrap()).unwrap()
}

/// The layer's soundtrack as the capture wrote it.
fn soundtrack(project: &Path) -> Vec<f32> {
    let p = load(project);
    let (s, l) = (&p.scenes[0], &p.scenes[0].layers[0]);
    let last = mui_cut::plugin::frame_at(s.duration, p.fps);
    let track = l.plugin_track(p.fps, p.sample_rate, last);
    let (key, _) = l.plugin_audio(&track, p.fps, p.sample_rate, p.samples(s)).unwrap();
    let bytes = std::fs::read(
        project.parent().unwrap().join(".cut-cache/audio").join(format!("{key}.f32")),
    )
    .unwrap();
    bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
}

fn capture(project: &Path, t: f64) -> mui_cut::Capture {
    let p = load(project);
    let l = &p.scenes[0].layers[0];
    let key = &l.plugin_track(p.fps, p.sample_rate, mui_cut::plugin::frame_at(t, p.fps)).pop().unwrap().key;
    let file = project.parent().unwrap().join(".cut-cache").join(format!("{key}.json"));
    serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap()
}

/// A note sounds from its own sample, not the block or frame around it:
/// silence to the sample before, sound from it, and the whole scene long.
/// A second capture from nothing gives the same bytes.
#[test]
fn notes_play_from_their_sample_and_the_same_every_time() {
    // 0.3104 s is not on a frame (30 fps) or a 480-sample block.
    let notes = json!([{"t": 0.3104, "dur": 0.3, "pitch": 69}, {"t": 0.7, "dur": 0.2, "pitch": 76, "vel": 60}]);
    let project = project("sound-sample", 1.0, json!({"notes": notes}));
    run(&project, &["capture"]);
    let a = soundtrack(&project);
    assert_eq!(a.len(), 2 * RATE as usize, "a second of stereo");
    let on = mui_cut::plugin::sample_at(0.3104, RATE) as usize;
    let first = a.chunks(2).position(|s| s[0] != 0.).unwrap();
    // The sine starts at phase 0: its first sample after the onset is the
    // first non-zero one.
    assert!(first == on || first == on + 1, "sound from sample {first}, the note is at {on}");
    assert!(a[..2 * on].iter().all(|v| *v == 0.));
    let peak = a.iter().fold(0f32, |m, v| m.max(v.abs()));
    assert!(peak > 0.05, "peak {peak}");
    std::fs::remove_dir_all(project.parent().unwrap().join(".cut-cache")).unwrap();
    run(&project, &["capture"]);
    let b = soundtrack(&project);
    assert!(a == b, "a second render of the same notes sounds different");
}

/// The mix goes into the video as an audio stream the scene's length, and
/// it is not silence.
#[test]
fn render_muxes_the_soundtrack_into_the_video() {
    if !has("ffmpeg") || !has("ffprobe") {
        eprintln!("skipped: no ffmpeg/ffprobe");
        return;
    }
    let notes = json!([{"t": 0.1, "dur": 0.6, "pitch": 60}, {"t": 0.5, "dur": 0.8, "pitch": 67}]);
    let project = project("sound-mux", 1.5, json!({"notes": notes, "volume": 0.8}));
    let out = project.parent().unwrap().join("out.mp4");
    let o = Command::new(BIN)
        .args(["render"])
        .arg(&project)
        .args(["--renderer", "cpu", "-o"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let probe = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "a:0"])
        .args(["-show_entries", "stream=codec_name,sample_rate,channels,duration", "-of", "default=nw=1"])
        .arg(&out)
        .output()
        .unwrap();
    let text = String::from_utf8(probe.stdout).unwrap();
    assert!(text.contains("codec_name=aac") && text.contains("channels=2"), "{text}");
    assert!(text.contains(&format!("sample_rate={RATE}")), "{text}");
    let dur: f64 = text.lines().find_map(|l| l.strip_prefix("duration=")).unwrap().parse().unwrap();
    assert!((dur - 1.5).abs() < 0.06, "audio {dur} s");
    let vol = Command::new("ffmpeg")
        .args(["-v", "info", "-i"])
        .arg(&out)
        .args(["-af", "volumedetect", "-vn", "-f", "null", "-"])
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&vol.stderr);
    let max: f64 = log
        .lines()
        .find_map(|l| l.split("max_volume: ").nth(1))
        .and_then(|v| v.trim_end_matches(" dB").parse().ok())
        .unwrap();
    assert!(max > -30., "max volume {max} dB: {log}");
}

/// A keyed view size lays the UI out again at that size: the panels move
/// and change shape, where a stretched capture would scale them all alike.
/// And each capture carries the patch the plugin reports.
#[test]
fn a_keyed_view_size_reflows_the_ui_and_captures_carry_the_patch() {
    let project = project(
        "sound-view",
        1.0,
        json!({
            "view_width": [{"t": 0.0, "v": 0.0, "interp": "hold"}, {"t": 0.5, "v": 1000.0, "interp": "hold"}],
            "view_height": [{"t": 0.0, "v": 0.0, "interp": "hold"}, {"t": 0.5, "v": 510.0, "interp": "hold"}],
            "params": [{"id": "filter", "field": "cutoff", "value": 0.2}],
        }),
    );
    run(&project, &["capture"]);
    let (a, b) = (capture(&project, 0.), capture(&project, 0.8));
    assert_eq!([a.width, a.height], [720., 510.]);
    assert_eq!([b.width, b.height], [1000., 510.]);
    let frame = |c: &mui_cut::Capture, id: &str| c.tree().into_iter().find(|p| p.path == id).unwrap().frame;
    let (ha, hb) = (frame(&a, "head"), frame(&b, "head"));
    // The header spans the new width; its height stays: a reflow.
    assert!(hb[2] > ha[2] + 200., "{ha:?} -> {hb:?}");
    assert_eq!(ha[3], hb[3]);
    let (oa, ob) = (frame(&a, "filter"), frame(&b, "filter"));
    assert!(ob[0] > oa[0] && ob[2] > oa[2], "{oa:?} -> {ob:?}");
    // The patch: the parameter the layer set, as the plugin reports it.
    let cutoff = a.patch["params"].as_array().unwrap().iter().find(|p| p["id"] == "filter.cutoff").unwrap();
    assert_eq!(cutoff["value"], 0.2);
    assert_eq!(a.patch["plugin"], "MUI Synth");
}

/// A `bin` relative to a project named by a relative path, from another
/// directory: the adapter still starts (it runs in the project's directory).
#[test]
fn a_relative_adapter_path_resolves_from_the_project() {
    let dir = scratch("sound-relative");
    std::os::unix::fs::symlink(synth(), dir.join("adapter")).unwrap();
    std::fs::create_dir(dir.join("sub")).unwrap();
    let p = json!({"size": [64, 64], "fps": 30.0, "scenes": [{"name": "s", "duration": 0.1,
        "layers": [{"id": "synth", "kind": "plugin", "source": {"bin": "../adapter"}}]}]});
    std::fs::write(dir.join("sub/p.cut.json"), p.to_string()).unwrap();
    let o = Command::new(BIN).current_dir(&dir).args(["capture", "sub/p.cut.json"]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(dir.join("sub/.cut-cache").read_dir().unwrap().count() > 1);
}
