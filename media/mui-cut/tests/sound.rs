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
    // A folder per checkout: checkouts sharing a target dir would
    // otherwise clear each other's scratch mid-test.
    let mut tree = std::hash::DefaultHasher::new();
    std::hash::Hash::hash(env!("CARGO_MANIFEST_DIR"), &mut tree);
    let tree = std::hash::Hasher::finish(&tree);
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("{tree:016x}"))
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn has(tool: &str) -> bool {
    Command::new(tool).arg("-version").output().is_ok()
}

fn synth() -> PathBuf {
    let s = Path::new(BIN).parent().unwrap().join("examples/synth");
    assert!(
        s.is_file(),
        "{} is missing: `cargo build -p mui-cut --example synth`",
        s.display()
    );
    s
}

/// A one-scene project around one synth layer (`layer` merged in).
fn project(name: &str, duration: f64, layer: &Value) -> PathBuf {
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

/// Adapters build into one cache the tests share (a moose build is long).
fn run(project: &Path, args: &[&str]) -> String {
    let o = Command::new(BIN)
        .args(args)
        .arg(project)
        .env(
            "MUI_CUT_CACHE",
            Path::new(env!("CARGO_TARGET_TMPDIR")).join("adapters"),
        )
        .output()
        .unwrap();
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
    let (key, _) = l
        .plugin_audio(&track, p.fps, p.sample_rate, p.samples(s))
        .unwrap();
    let bytes = std::fs::read(
        project
            .parent()
            .unwrap()
            .join(".cut-cache/audio")
            .join(format!("{key}.f32")),
    )
    .unwrap();
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect()
}

fn capture(project: &Path, t: f64) -> mui_cut::Capture {
    let p = load(project);
    let l = &p.scenes[0].layers[0];
    let key = &l
        .plugin_track(p.fps, p.sample_rate, mui_cut::plugin::frame_at(t, p.fps))
        .pop()
        .unwrap()
        .key;
    let file = project
        .parent()
        .unwrap()
        .join(".cut-cache")
        .join(format!("{key}.json"));
    serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap()
}

/// A note sounds from its own sample, not the block or frame around it:
/// silence to the sample before, sound from it, and the whole scene long.
/// A second capture from nothing gives the same bytes.
#[test]
fn notes_play_from_their_sample_and_the_same_every_time() {
    // 0.3104 s is not on a frame (30 fps) or a 480-sample block.
    let notes = json!([{"t": 0.3104, "dur": 0.3, "pitch": 69}, {"t": 0.7, "dur": 0.2, "pitch": 76, "vel": 60}]);
    let project = project("sound-sample", 1.0, &json!({"notes": notes}));
    run(&project, &["capture"]);
    let a = soundtrack(&project);
    assert_eq!(a.len(), 2 * RATE as usize, "a second of stereo");
    let on = mui_cut::plugin::sample_at(0.3104, RATE) as usize;
    let first = a.chunks(2).position(|s| s[0] != 0.).unwrap();
    // The sine starts at phase 0: its first sample after the onset is the
    // first non-zero one.
    assert!(
        first == on || first == on + 1,
        "sound from sample {first}, the note is at {on}"
    );
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
    let project = project("sound-mux", 1.5, &json!({"notes": notes, "volume": 0.8}));
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
        .args([
            "-show_entries",
            "stream=codec_name,sample_rate,channels,duration",
            "-of",
            "default=nw=1",
        ])
        .arg(&out)
        .output()
        .unwrap();
    let text = String::from_utf8(probe.stdout).unwrap();
    assert!(
        text.contains("codec_name=aac") && text.contains("channels=2"),
        "{text}"
    );
    assert!(text.contains(&format!("sample_rate={RATE}")), "{text}");
    let dur: f64 = text
        .lines()
        .find_map(|l| l.strip_prefix("duration="))
        .unwrap()
        .parse()
        .unwrap();
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
        &json!({
            "view_width": [{"t": 0.0, "v": 0.0, "interp": "hold"}, {"t": 0.5, "v": 1000.0, "interp": "hold"}],
            "view_height": [{"t": 0.0, "v": 0.0, "interp": "hold"}, {"t": 0.5, "v": 510.0, "interp": "hold"}],
            "params": [{"id": "filter", "field": "cutoff", "value": 0.2}],
        }),
    );
    run(&project, &["capture"]);
    let (a, b) = (capture(&project, 0.), capture(&project, 0.8));
    assert_eq!([a.width, a.height], [720., 510.]);
    assert_eq!([b.width, b.height], [1000., 510.]);
    let frame =
        |c: &mui_cut::Capture, id: &str| c.tree().into_iter().find(|p| p.path == id).unwrap().frame;
    let (ha, hb) = (frame(&a, "head"), frame(&b, "head"));
    // The header spans the new width; its height stays: a reflow.
    assert!(hb[2] > ha[2] + 200., "{ha:?} -> {hb:?}");
    assert_eq!(ha[3], hb[3]);
    let (oa, ob) = (frame(&a, "filter"), frame(&b, "filter"));
    assert!(ob[0] > oa[0] && ob[2] > oa[2], "{oa:?} -> {ob:?}");
    // The patch: the parameter the layer set, as the plugin reports it.
    let cutoff = a.patch["params"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "filter.cutoff")
        .unwrap();
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
    let o = Command::new(BIN)
        .current_dir(&dir)
        .args(["capture", "sub/p.cut.json"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(dir.join("sub/.cut-cache").read_dir().unwrap().count() > 1);
}

fn http(port: u16, method: &str, path: &str, body: &str) -> Value {
    use std::io::{Read as _, Write as _};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let body = out.split("\r\n\r\n").nth(1).unwrap();
    serde_json::from_str(body).unwrap_or_else(|_| panic!("{out}"))
}

/// `serve` plays the scene: on the null device the audio clock runs in
/// real time from where the editor started it, a key played live is heard
/// within 50 ms, the plugin's UI is captured as it plays and announced,
/// and pause stops the clock.
#[test]
fn serve_plays_the_sound_in_time_with_the_playhead() {
    use std::io::{BufRead as _, Write as _};
    struct Kill(std::process::Child);
    impl Drop for Kill {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let notes = json!([{"t": 0.0, "dur": 3.0, "pitch": 57}]);
    let project = project("sound-live", 4.0, &json!({"notes": notes}));
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let _serve = Kill(
        Command::new(BIN)
            .args(["serve"])
            .arg(&project)
            .args(["--port", &port.to_string()])
            .env("MUI_CUT_AUDIO", "null")
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let start = std::time::Instant::now();
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(start.elapsed().as_secs() < 20, "serve did not start");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let mut sse = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(sse, "GET /events HTTP/1.1\r\n\r\n").unwrap();
    let r = http(
        port,
        "POST",
        "/transport",
        r#"{"playing": true, "t": 1.0, "scene": 0}"#,
    );
    assert_eq!(
        (r["playing"].as_bool(), r["t"].as_f64()),
        (Some(true), Some(1.0))
    );
    std::thread::sleep(std::time::Duration::from_millis(300));
    let a = http(port, "GET", "/transport", "");
    let wall = std::time::Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(600));
    let b = http(port, "GET", "/transport", "");
    let ran = b["t"].as_f64().unwrap() - a["t"].as_f64().unwrap();
    let real = wall.elapsed().as_secs_f64();
    assert!(
        (ran - real).abs() < 0.05,
        "the audio clock ran {ran} s in {real} s"
    );
    assert!(a["t"].as_f64().unwrap() > 1.0 && b["device"] == "null");
    assert!(b["latency_ms"].as_f64().unwrap() < 50., "{b}");
    http(
        port,
        "POST",
        "/live/note",
        r#"{"layer": "synth", "note": 72, "on": true}"#,
    );
    std::thread::sleep(std::time::Duration::from_millis(200));
    let c = http(port, "GET", "/transport", "");
    let heard = c["note_ms"].as_f64().unwrap();
    assert!(heard > 0. && heard < 50., "a live key took {heard} ms");
    // The UI as it plays, announced to the editor.
    let mut lines = std::io::BufReader::new(sse);
    let mut line = String::new();
    let mut state = None;
    while state.is_none() {
        line.clear();
        assert!(lines.read_line(&mut line).unwrap() > 0);
        if let Some(d) = line
            .strip_prefix("data: ")
            .filter(|d| d.contains("\"layer\""))
        {
            let v: Value = serde_json::from_str(d).unwrap();
            // The first ones may be gone: only the last few are kept.
            state = v["state"]
                .as_str()
                .filter(|k| {
                    project
                        .parent()
                        .unwrap()
                        .join(".cut-cache")
                        .join(format!("{k}.json"))
                        .is_file()
                })
                .map(str::to_owned);
        }
    }
    let state = state.unwrap();
    assert!(state.starts_with("live/synth-"), "{state}");
    let cap: mui_cut::Capture = serde_json::from_slice(
        &std::fs::read(
            project
                .parent()
                .unwrap()
                .join(".cut-cache")
                .join(format!("{state}.json")),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(!cap.fragments.is_empty());
    let d = http(port, "POST", "/transport", r#"{"playing": false}"#);
    std::thread::sleep(std::time::Duration::from_millis(200));
    let e = http(port, "GET", "/transport", "");
    assert_eq!(d["t"], e["t"], "paused, the clock stands");
}

/// A fixture plugin mui-cut builds an adapter for (`tests/fixtures/<name>`).
fn fixture(name: &str) -> Value {
    json!({"plugin": Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)})
}

/// The left channel's frequency, from its upward zero crossings, and its peak.
fn tone(stereo: &[f32]) -> (f64, f32) {
    let left: Vec<f32> = stereo.iter().step_by(2).copied().collect();
    let ups: Vec<usize> = (1..left.len())
        .filter(|&i| left[i - 1] < 0. && left[i] >= 0.)
        .collect();
    let (first, last) = (ups[0], ups[ups.len() - 1]);
    let hz = (ups.len() - 1) as f64 * f64::from(RATE) / (last - first) as f64;
    (hz, left.iter().fold(0f32, |m, v| m.max(v.abs())))
}

/// A moose state envelope (`moose::core::state::serialize_state`) for the
/// plugin `clap_id`: its parameter values, no custom or persisted state.
fn moose_state(clap_id: &str, values: &[(u32, f64)]) -> Vec<u8> {
    let hash = clap_id.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    let mut d = b"OAST".to_vec();
    d.extend(1u32.to_le_bytes());
    d.extend(hash.to_le_bytes());
    d.extend((values.len() as u32).to_le_bytes());
    for (id, v) in values {
        d.extend(id.to_le_bytes());
        d.extend(v.to_le_bytes());
    }
    d.extend(0u64.to_le_bytes());
    d.extend(0u64.to_le_bytes());
    d
}

/// A moose plugin plays its tone through the adapter, loads a preset
/// before anything else (a host's saved state of plain values, or one
/// wrapped in the plugin's own file of normalized values), and its patch
/// names each parameter as the plugin shows it, marks what the layer
/// automates and what a route modulates, lists the routes and the notes
/// held.
#[test]
fn a_moose_plugin_plays_loads_a_preset_and_reports_its_patch() {
    let id = "com.mui-cut.tone";
    // Level 0.25, Mod 1: Lfo -> Pitch at 0.5, the `Host 1` slot at 0.3.
    let plain = moose_state(id, &[(0, 0.25), (2, 1.), (3, 1.), (4, 0.5), (5, 0.3)]);
    let mut wrapped = b"TRPS\x01\x00some metadata".to_vec();
    wrapped.extend(moose_state(
        id,
        &[(0, 0.25), (2, 1.), (3, 1.), (4, 0.75), (5, 0.3)],
    ));
    for (name, preset) in [("moose-state", plain), ("moose-preset", wrapped)] {
        let layer = json!({
            "source": fixture("moose-tone"),
            "preset": "tone.preset",
            "notes": [{"t": 0.1, "dur": 0.8, "pitch": 60}],
            "params": [{"id": "Pitch", "field": "value", "value": 880.0}],
        });
        let project = project(name, 1.0, &layer);
        std::fs::write(project.parent().unwrap().join("tone.preset"), preset).unwrap();
        run(&project, &["capture"]);
        let (hz, peak) = tone(&soundtrack(&project));
        assert!((hz - 880.).abs() < 2., "{name}: {hz} Hz");
        assert!(
            (peak - 0.25).abs() < 0.01,
            "{name}: the preset's level, peak {peak}"
        );
        let patch = capture(&project, 0.5).patch;
        let param = |n: &str| {
            patch["params"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["name"] == n)
                .unwrap_or_else(|| panic!("{name}: no {n} in {patch}"))
                .clone()
        };
        assert_eq!(param("Level")["value"], 0.25, "{name}");
        assert_eq!(
            param("Drive")["value"],
            0.3,
            "{name}: the slot's shown name"
        );
        let pitch = param("Pitch");
        assert_eq!(
            (&pitch["automated"], &pitch["modulated"]),
            (&json!(true), &json!(true)),
            "{name}"
        );
        assert_eq!(
            patch["routes"],
            json!([{"source": "Lfo", "target": "Pitch", "depth": 0.5}]),
            "{name}"
        );
        assert_eq!(patch["held"], json!([60]), "{name}");
    }
}

/// A generic adapter's tone: the fixture's sine at its pitch and level,
/// silent before the note, and the note held in the patch.
fn plays_its_tone(name: &str, plugin: &str, layer: &Value, hz: f64, level: f32) -> PathBuf {
    let mut l = json!({"source": fixture(plugin), "notes": [{"t": 0.1, "dur": 0.8, "pitch": 60}]});
    for (k, v) in layer.as_object().unwrap() {
        l[k] = v.clone();
    }
    let project = project(name, 1.0, &l);
    run(&project, &["capture"]);
    let sound = soundtrack(&project);
    let on = mui_cut::plugin::sample_at(0.1, RATE) as usize;
    assert!(
        sound[..2 * on].iter().all(|v| *v == 0.),
        "{name}: sound before the note"
    );
    let (got, peak) = tone(&sound);
    assert!((got - hz).abs() < 2., "{name}: {got} Hz");
    assert!((peak - level).abs() < 0.01, "{name}: peak {peak}");
    assert_eq!(capture(&project, 0.5).patch["held"], json!([60]), "{name}");
    project
}

/// A nice-plug plugin plays through its `process`, set by the layer.
#[test]
fn a_nice_plug_plugin_plays_its_tone() {
    let pitch = json!({"params": [{"id": "Pitch", "field": "value", "value": 880.0}]});
    plays_its_tone("nice-tone", "nice-tone", &pitch, 880., 0.5);
}

/// A plain MUI crate plays through its `mui_audio`.
#[test]
fn a_plain_mui_crate_plays_its_tone() {
    plays_its_tone("plain-tone", "plain", &json!({}), 660., 0.4);
}

/// A truce plugin plays through its `process`, and its patch lists the
/// route its `Mod 1` parameters hold.
#[test]
fn a_truce_plugin_plays_its_tone_and_reports_its_routes() {
    let set = |id: &str, value: f64| json!({"id": id, "field": "value", "value": value});
    let params = json!({"params": [set("Pitch", 880.), set("Mod 1 Source", 1.), set("Mod 1 Target", 1.), set("Mod 1 Amount", 0.5)]});
    let project = plays_its_tone("truce-tone", "truce-tone", &params, 880., 0.5);
    let patch = capture(&project, 0.5).patch;
    assert_eq!(
        patch["routes"],
        json!([{"source": "Lfo", "target": "Pitch", "depth": 0.5}])
    );
    let pitch = patch["params"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "Pitch")
        .cloned();
    assert_eq!(
        pitch.map(|p| p["modulated"].clone()),
        Some(json!(true)),
        "{patch}"
    );
}

/// `still` of a plugin built from source runs inside its adapter: the
/// editor draws from its paint (no capture written), plays the same
/// soundtrack, and looks as its capture does.
#[test]
fn a_plugin_from_source_renders_in_process() {
    let layer = json!({"source": fixture("plain"), "notes": [{"t": 0.1, "dur": 0.8, "pitch": 60}]});
    let project = project("plain-inproc", 1.0, &layer);
    let dir = project.parent().unwrap();
    let still = |name: &str, capture: bool| {
        let out = dir.join(name);
        let mut c = Command::new(BIN);
        c.arg("still")
            .arg(&project)
            .args(["--t", "0.5", "-o"])
            .arg(&out)
            .env(
                "MUI_CUT_CACHE",
                Path::new(env!("CARGO_TARGET_TMPDIR")).join("adapters"),
            );
        if capture {
            c.env("MUI_CUT_CAPTURE", "1");
        }
        let o = c.output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let mut r = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(out).unwrap()))
            .read_info()
            .unwrap();
        let mut px = vec![0; r.output_buffer_size().unwrap()];
        r.next_frame(&mut px).unwrap();
        px
    };
    let vector = still("vector.png", false);
    let key = load(&project).scenes[0].layers[0]
        .plugin_track(
            load(&project).fps,
            RATE,
            mui_cut::plugin::frame_at(0.5, 30.),
        )
        .pop()
        .unwrap()
        .key;
    assert!(
        !dir.join(".cut-cache").join(format!("{key}.json")).exists(),
        "in process, nothing is captured"
    );
    let (hz, _) = tone(&soundtrack(&project));
    assert!((hz - 660.).abs() < 2., "{hz} Hz");
    let raster = still("raster.png", true);
    let off = vector
        .iter()
        .zip(&raster)
        .map(|(a, b)| f64::from(a.abs_diff(*b)))
        .sum::<f64>()
        / vector.len() as f64;
    assert!(off < 2., "vector and capture differ by {off} a channel");
}
