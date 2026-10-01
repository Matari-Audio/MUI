//! The binary end to end: a render's frame count and duration by ffprobe,
//! a still's size, and the editor server's save and live-reload loop.
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_mui-cut");
const DEMO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/demo.cut.json");

/// A scratch dir on disk (the target dir), not the RAM-backed /tmp.
fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn has(tool: &str) -> bool {
    Command::new(tool).arg("-version").output().is_ok()
}

#[test]
fn render_writes_every_frame_of_the_scene() {
    if !has("ffmpeg") || !has("ffprobe") {
        eprintln!("skipped: no ffmpeg/ffprobe");
        return;
    }
    let out = scratch("render").join("title.mp4");
    let st = Command::new(BIN)
        .args([
            "render", DEMO, "--scene", "title", "--size", "320x180", "--mb", "3", "-o",
        ])
        .arg(&out)
        .status()
        .unwrap();
    assert!(st.success());
    let probe = Command::new("ffprobe")
        .args(["-v", "error", "-count_frames", "-select_streams", "v:0"])
        .args([
            "-show_entries",
            "stream=nb_read_frames,width,height:format=duration",
        ])
        .args(["-of", "default=nw=1"])
        .arg(&out)
        .output()
        .unwrap();
    let text = String::from_utf8(probe.stdout).unwrap();
    // title is 2.5 s at 30 fps.
    assert!(text.contains("nb_read_frames=75"), "{text}");
    assert!(
        text.contains("width=320") && text.contains("height=180"),
        "{text}"
    );
    let dur: f64 = text
        .lines()
        .find_map(|l| l.strip_prefix("duration="))
        .unwrap()
        .parse()
        .unwrap();
    assert!((dur - 2.5).abs() < 0.05, "{dur}");
}

fn probe(file: &Path) -> String {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-count_frames", "-select_streams", "v:0"])
        .args([
            "-show_entries",
            "stream=codec_name,pix_fmt,color_space,color_range,nb_read_frames,width,height:format=format_name",
        ])
        .args(["-of", "default=nw=1"])
        .arg(file)
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap()
}

/// `--variants` renders each named variant at its own size, one file each;
/// several variants without `{name}` in the path would overwrite, so fail.
#[test]
fn render_writes_each_variant() {
    if !has("ffmpeg") || !has("ffprobe") {
        eprintln!("skipped: no ffmpeg/ffprobe");
        return;
    }
    let project = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/variants.cut.json");
    let dir = scratch("variants");
    let st = Command::new(BIN)
        .args([
            "render",
            project,
            "--variants",
            "dark-wide,light-tall",
            "-o",
        ])
        .arg(dir.join("out/{name}.mp4"))
        .status()
        .unwrap();
    assert!(st.success());
    for (name, w, h) in [("dark-wide", 1920, 1080), ("light-tall", 1080, 1920)] {
        let text = probe(&dir.join(format!("out/{name}.mp4")));
        assert!(
            text.contains(&format!("width={w}\nheight={h}")),
            "{name}: {text}"
        );
        assert!(text.contains("nb_read_frames=60"), "{name}: {text}");
    }
    assert!(!dir.join("out/dark-tall.mp4").exists());
    let bad = Command::new(BIN)
        .args(["render", project, "--variants", "all", "-o"])
        .arg(dir.join("one.mp4"))
        .output()
        .unwrap();
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("{name}"));
}

/// Every codec, both bit depths and each container, in software (and on
/// VAAPI when `auto` finds it): the stream is what was asked for, tagged
/// BT.709 limited range, every frame there.
#[test]
fn render_encodes_each_codec_depth_and_container() {
    if !has("ffmpeg") || !has("ffprobe") {
        eprintln!("skipped: no ffmpeg/ffprobe");
        return;
    }
    let dir = scratch("codecs");
    for (codec, pix, container, encoder, name) in [
        ("h264", "yuv420p", "mov", "software", "h264"),
        ("h265", "yuv420p10le", "mkv", "software", "hevc"),
        ("av1", "yuv420p", "mp4", "software", "av1"),
        ("h264", "yuv420p", "mp4", "auto", "h264"),
        ("h265", "yuv420p10le", "mp4", "auto", "hevc"),
    ] {
        let out = dir.join(format!("{codec}-{pix}-{encoder}.{container}"));
        let st = Command::new(BIN)
            .args([
                "render", DEMO, "--scene", "outro", "--size", "320x180", "--mb", "2",
            ])
            .args([
                "--codec",
                codec,
                "--pix-fmt",
                pix,
                "--encoder",
                encoder,
                "-o",
            ])
            .arg(&out)
            .status()
            .unwrap();
        assert!(st.success(), "{codec} {pix} {container}");
        let text = probe(&out);
        let want = [
            format!("codec_name={name}"),
            format!("pix_fmt={pix}"),
            "color_space=bt709".into(),
            "color_range=tv".into(),
            // outro is 2 s at 30 fps.
            "nb_read_frames=60".into(),
        ];
        for w in want {
            assert!(
                text.contains(&w),
                "{codec} {pix} {container}: no {w} in {text}"
            );
        }
        assert_eq!(
            text.contains("format_name=matroska"),
            container == "mkv",
            "{text}"
        );
    }
}

#[test]
fn still_writes_a_png_at_the_asked_size() {
    let out = scratch("still").join("f.png");
    let st = Command::new(BIN)
        .args([
            "still", DEMO, "--scene", "shapes", "--t", "1.5", "--size", "640x360", "-o",
        ])
        .arg(&out)
        .status()
        .unwrap();
    assert!(st.success());
    let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&out).unwrap()));
    let info = dec.read_info().unwrap();
    assert_eq!((info.info().width, info.info().height), (640, 360));
}

fn http(port: u16, req: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

#[test]
fn serve_saves_canonical_json_and_pushes_outside_edits() {
    let dir = scratch("serve");
    let project = dir.join("p.cut.json");
    std::fs::copy(DEMO, &project).unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut server = Command::new(BIN)
        .args(["serve"])
        .arg(&project)
        .args(["--port", &port.to_string()])
        .spawn()
        .unwrap();
    let start = Instant::now();
    while TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "server never came up"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let got = http(port, "GET /project HTTP/1.1\r\n\r\n");
    assert!(got.starts_with("HTTP/1.1 200") && got.contains("\"shapes\""));

    // A save from the browser, compact and reordered, lands canonical.
    let edited = std::fs::read_to_string(DEMO)
        .unwrap()
        .replace("\"duration\": 2.5", "\"duration\": 2.0");
    let compact =
        serde_json::to_string(&serde_json::from_str::<serde_json::Value>(&edited).unwrap())
            .unwrap();
    let put = http(
        port,
        &format!(
            "PUT /project HTTP/1.1\r\nContent-Length: {}\r\n\r\n{compact}",
            compact.len()
        ),
    );
    assert!(put.starts_with("HTTP/1.1 200"), "{put}");
    assert_eq!(std::fs::read_to_string(&project).unwrap(), edited);
    let bad = http(port, "PUT /project HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}");
    assert!(bad.starts_with("HTTP/1.1 400"), "{bad}");
    assert_eq!(
        std::fs::read_to_string(&project).unwrap(),
        edited,
        "a bad save changes nothing"
    );

    // An agent edits the file: an open editor hears about it.
    let mut events = TcpStream::connect(("127.0.0.1", port)).unwrap();
    events.write_all(b"GET /events HTTP/1.1\r\n\r\n").unwrap();
    events
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut lines = BufReader::new(events);
    let mut l = String::new();
    while !l.starts_with(": hello") {
        l.clear();
        lines.read_line(&mut l).unwrap();
    }
    std::fs::write(
        &project,
        edited.replace("\"duration\": 2.0", "\"duration\": 1.0"),
    )
    .unwrap();
    loop {
        l.clear();
        lines.read_line(&mut l).unwrap();
        if l.starts_with("data: {") {
            let v: serde_json::Value = serde_json::from_str(&l[6..]).unwrap();
            assert!(
                v["by"] == "disk" && v["doc"].to_string().contains("\"duration\":1.0"),
                "{l}"
            );
            break;
        }
    }
    let _ = server.kill();
    let _ = server.wait();
}

/// Every frame of `file` as 8-bit luma planes.
fn luma(file: &Path, (w, h): (usize, usize)) -> Vec<Vec<u8>> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(file)
        .args(["-f", "rawvideo", "-pix_fmt", "gray", "-"])
        .output()
        .unwrap();
    assert!(
        out.status.success() && out.stderr.is_empty(),
        "{file:?} decodes cleanly: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout.chunks(w * h).map(<[u8]>::to_vec).collect()
}

fn mean_diff(a: &[u8], b: &[u8]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| f64::from(x.abs_diff(*y)))
        .sum::<f64>()
        / a.len() as f64
}

/// The segment cache: moving one key re-encodes only the spans it touches,
/// and the spliced file plays, has every frame and the exact duration, and
/// matches a straight render of the edited project.
#[test]
fn render_segments_reuse_unchanged_spans_and_splice_losslessly() {
    if !has("ffmpeg") || !has("ffprobe") {
        eprintln!("skipped: no ffmpeg/ffprobe");
        return;
    }
    let dir = scratch("segments");
    let project = dir.join("demo.cut.json");
    let text = std::fs::read_to_string(DEMO).unwrap();
    std::fs::write(&project, &text).unwrap();
    let render = |out: &str, extra: &[&str]| {
        let o = Command::new(BIN)
            .arg("render")
            .arg(&project)
            .args(["--size", "320x180", "-o"])
            .arg(dir.join(out))
            .args(extra)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
        assert!(
            o.status.success(),
            "{stdout}{}",
            String::from_utf8_lossy(&o.stderr)
        );
        stdout
    };
    let seg = ["--segment", "1"];
    assert!(render("first.mp4", &seg).contains("8 rendered, 0 cached"));
    assert!(render("again.mp4", &seg).contains("0 rendered, 8 cached"));

    // The ball's middle key in `shapes` (2.5 s in): it bends 2.5 s..4.0 s,
    // frames 75..119, which lie in the 1 s spans 2 and 3.
    let edited = text.replacen("\"t\": 0.75, \"v\": 360.0", "\"t\": 0.75, \"v\": 250.0", 1);
    assert_ne!(edited, text, "the demo's key moved");
    std::fs::write(&project, &edited).unwrap();
    let log = render("spliced.mp4", &seg);
    assert!(log.contains("2 rendered, 6 cached"), "{log}");
    render("straight.mp4", &[]);

    let probe = Command::new("ffprobe")
        .args(["-v", "error", "-count_frames", "-select_streams", "v:0"])
        .args(["-show_entries", "stream=nb_read_frames:format=duration"])
        .args(["-of", "default=nw=1"])
        .arg(dir.join("spliced.mp4"))
        .output()
        .unwrap();
    let text = String::from_utf8(probe.stdout).unwrap();
    assert!(text.contains("nb_read_frames=225"), "{text}");
    assert!(text.contains("duration=7.500000"), "{text}");

    let size = (320, 180);
    let (spliced, straight, before) = (
        luma(&dir.join("spliced.mp4"), size),
        luma(&dir.join("straight.mp4"), size),
        luma(&dir.join("first.mp4"), size),
    );
    assert_eq!((spliced.len(), straight.len()), (225, 225));
    for (i, (a, b)) in spliced.iter().zip(&straight).enumerate() {
        let d = mean_diff(a, b);
        assert!(
            d < 1.5,
            "frame {i} differs from a straight render by {d:.2}"
        );
    }
    // The edit is in the splice: the ball sits higher at the key.
    assert!(mean_diff(&spliced[97], &before[97]) > 0.2);
    assert!(
        mean_diff(&spliced[30], &before[30]) < 0.01,
        "an untouched span is the cached chunk"
    );

    // --range forces a span back through the encoder.
    let log = render("forced.mp4", &["--segment", "1", "--range", "0.2s-0.4s"]);
    assert!(log.contains("1 rendered, 7 cached"), "{log}");

    // The CPU pool drains once per span and keeps going; --stats times
    // every drawn frame across all of them.
    let log = render(
        "cpu.mp4",
        &[
            "--segment",
            "1",
            "--range",
            "0s-7.5s",
            "--renderer",
            "cpu",
            "--stats",
        ],
    );
    assert!(log.contains("8 rendered, 0 cached"), "{log}");
    assert!(log.contains("frame time: p50"), "{log}");
    let cpu = luma(&dir.join("cpu.mp4"), size);
    assert_eq!(cpu.len(), 225);
    for i in [0, 97, 224] {
        let d = mean_diff(&cpu[i], &straight[i]);
        assert!(d < 1.5, "CPU frame {i} differs from the GPU by {d:.2}");
    }
}

/// `examples/plugin.cut.json` pointed at the built synth adapter (plain
/// `cargo test` builds the examples) in a fresh scratch dir.
fn plugin_project(name: &str) -> PathBuf {
    let synth = Path::new(BIN).parent().unwrap().join("examples/synth");
    assert!(
        synth.is_file(),
        "{} is missing: run the whole `cargo test -p mui-cut` (it builds the examples) or `cargo build -p mui-cut --example synth`",
        synth.display()
    );
    let dir = scratch(name);
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/plugin.cut.json"
    ))
    .unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&src).unwrap();
    for l in v["scenes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|s| s["layers"].as_array_mut().unwrap().iter_mut())
        .filter(|l| l.get("source").is_some())
    {
        assert_eq!(l["source"]["example"], "synth");
        l["source"] = serde_json::json!({ "bin": synth });
    }
    let project = dir.join("plugin.cut.json");
    std::fs::write(&project, v.to_string()).unwrap();
    project
}

fn still(project: &Path, t: &str, out: &Path) -> String {
    let o = Command::new(BIN)
        .args(["still"])
        .arg(project)
        .args(["--t", t, "--size", "480x270", "--renderer", "cpu", "-o"])
        .arg(out)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr).into_owned();
    assert!(o.status.success(), "{err}");
    err
}

/// Two levels deep, the real synth splits into its panels and then the
/// controls in them, by path; each panel's leftover (its card and label,
/// its knobs taken out) is a part of its own. Named without a directory,
/// the project still runs its adapter.
#[test]
fn a_two_level_capture_of_the_real_ui_names_controls_by_path() {
    let project = plugin_project("plugin-deep");
    let dir = project.parent().unwrap();
    let mut v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&project).unwrap()).unwrap();
    v["scenes"][0]["layers"][0]["explode_levels"] = 2.into();
    std::fs::write(&project, v.to_string()).unwrap();
    let o = Command::new(BIN)
        .current_dir(dir)
        .args([
            "still",
            "plugin.cut.json",
            "--t",
            "4.8",
            "--size",
            "480x270",
        ])
        .args(["--renderer", "cpu", "-o", "deep.png"])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success() && !err.contains("No such file"), "{err}");
    let p = mui_cut::Project::load(&std::fs::read_to_string(&project).unwrap()).unwrap();
    let key = &p.scenes[0].layers[0].plugin_track(p.fps, p.sample_rate, 0)[0].key;
    let cap: mui_cut::Capture = serde_json::from_slice(
        &std::fs::read(dir.join(".cut-cache").join(format!("{key}.json"))).unwrap(),
    )
    .unwrap();
    let tree = cap.tree();
    let paths: Vec<&str> = tree.iter().map(|p| p.path.as_str()).collect();
    assert_eq!(paths[..5], ["head", "osc", "filter", "env", "out"]);
    for control in [
        "osc/osc-shape",
        "osc/osc-sync",
        "filter/filter-cutoff",
        "env/env-release",
        "out/out-level",
    ] {
        let part = tree.iter().find(|p| p.path == control);
        assert!(part.is_some(), "{control} not in {paths:?}");
        let parent = control.split('/').next();
        assert_eq!(part.unwrap().parent.as_deref(), parent);
    }
    // The panel's own paint is still there, drawn as its own fragment.
    assert!(cap.fragments.iter().any(|f| f.group == "osc"));
    assert!(cap.fragments.iter().any(|f| f.group == "osc/osc-shape"));
}

#[test]
fn plugin_layers_capture_the_real_ui_once_and_draw_it() {
    let project = plugin_project("plugin");
    let dir = project.parent().unwrap();
    let a = dir.join("a.png");
    let first = still(&project, "5.8", &a);
    assert!(first.contains("capturing plugin layer `synth`"), "{first}");
    // One manifest per state the scene shows, content-named images shared.
    let cache = dir.join(".cut-cache");
    let manifests = std::fs::read_dir(&cache)
        .unwrap()
        .filter(|e| e.as_ref().unwrap().path().extension() == Some("json".as_ref()))
        .count();
    let p = mui_cut::Project::load(&std::fs::read_to_string(&project).unwrap()).unwrap();
    let steps = p.scenes[0].layers[0].plugin_track(p.fps, p.sample_rate, 240);
    // And the 3D scene's layer, told nothing, is one more.
    let deck = p.scenes[1].layers[3].plugin_track(p.fps, p.sample_rate, 180);
    assert_eq!(deck.len(), 1);
    assert_eq!(manifests, steps.len() + 1);
    // The real UI split into its named panels.
    let cap: mui_cut::Capture = serde_json::from_slice(
        &std::fs::read(cache.join(format!("{}.json", steps[0].key))).unwrap(),
    )
    .unwrap();
    let paths: Vec<String> = cap.tree().into_iter().map(|p| p.path).collect();
    assert_eq!(paths, ["head", "osc", "filter", "env", "out"]);
    assert!(cap.surfaces.iter().any(|s| s.id == "filter-cutoff"));
    // The keyed cutoff turns the real knob: its part's pixels differ.
    let cutoff = |i: usize| {
        let c: mui_cut::Capture = serde_json::from_slice(
            &std::fs::read(cache.join(format!("{}.json", steps[i].key))).unwrap(),
        )
        .unwrap();
        let f = c.fragments.iter().find(|f| f.group == "filter").unwrap();
        f.src.clone()
    };
    assert_ne!(cutoff(0), cutoff(steps.len() / 2));
    // Cached now: no adapter, the same pixels.
    let b = dir.join("b.png");
    let again = still(&project, "5.8", &b);
    assert!(!again.contains("capturing"), "{again}");
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
    // And it is not an empty frame: most of the middle is the plugin.
    let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&a).unwrap()));
    let mut reader = dec.read_info().unwrap();
    let mut px = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut px).unwrap();
    let bg = [0x0e, 0x0e, 0x10];
    let lit = px.chunks(4).filter(|p| p[..3] != bg).count();
    assert!(lit > 480 * 270 / 4, "{lit} pixels drawn");
}

#[test]
fn serve_captures_plugin_states_and_tells_the_editor() {
    let project = plugin_project("serve-plugin");
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut server = Command::new(BIN)
        .args(["serve"])
        .arg(&project)
        .args(["--port", &port.to_string()])
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    let mut events = loop {
        if let Ok(s) = TcpStream::connect(("127.0.0.1", port)) {
            break s;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "no server");
        std::thread::sleep(Duration::from_millis(20));
    };
    events.write_all(b"GET /events HTTP/1.1\r\n\r\n").unwrap();
    events
        .set_read_timeout(Some(Duration::from_secs(60)))
        .unwrap();
    let mut lines = BufReader::new(events);
    let mut l = String::new();
    while !l.starts_with("event: plugin") {
        l.clear();
        assert!(lines.read_line(&mut l).unwrap() > 0, "events closed");
    }
    let p = mui_cut::Project::load(&std::fs::read_to_string(&project).unwrap()).unwrap();
    let key = &p.scenes[0].layers[0].plugin_track(p.fps, p.sample_rate, 0)[0].key;
    let got = http(
        port,
        &format!("GET /asset/.cut-cache/{key}.json HTTP/1.1\r\n\r\n"),
    );
    assert!(
        got.starts_with("HTTP/1.1 200") && got.contains("\"filter\""),
        "{got}"
    );
    let _ = server.kill();
    let _ = server.wait();
}

/// A rebuilt adapter redraws a plugin with the same state keys; segment
/// re-renders must see the new captures, not reuse the old spans.
#[test]
fn render_segments_redraw_when_a_plugin_capture_changes() {
    if !has("ffmpeg") || !has("ffprobe") {
        eprintln!("skipped: no ffmpeg/ffprobe");
        return;
    }
    let project = plugin_project("plugin-segments");
    let render = || {
        let o = Command::new(BIN)
            .arg("render")
            .arg(&project)
            .args([
                "--scene",
                "synth",
                "--size",
                "320x180",
                "--segment",
                "4",
                "-o",
            ])
            .arg(project.with_file_name("out.mp4"))
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).into_owned()
    };
    assert!(render().contains("2 rendered, 0 cached"));
    assert!(render().contains("0 rendered, 2 cached"));
    // Same keys, other pixels: nudge every captured fragment.
    let cache = project.with_file_name(".cut-cache");
    for e in std::fs::read_dir(&cache).unwrap() {
        let path = e.unwrap().path();
        if path.extension().is_some_and(|x| x == "json") {
            let mut v: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            for f in v["layers"].as_array_mut().unwrap() {
                f["rect"][0] = (f["rect"][0].as_f64().unwrap() + 1.).into();
            }
            std::fs::write(&path, v.to_string()).unwrap();
        }
    }
    let log = render();
    assert!(log.contains("2 rendered, 0 cached"), "{log}");
}

/// A Cargo source builds from its own directory with its own toolchain
/// (rustup picks it from there), whatever directory and toolchain mui-cut
/// itself runs under: `cargo` here is a stand-in that logs both.
#[cfg(unix)]
#[test]
fn a_cargo_source_builds_with_its_own_toolchain() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = scratch("plugin-toolchain");
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/examples/plugin.cut.json"
    ))
    .unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&src).unwrap();
    for l in v["scenes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|s| s["layers"].as_array_mut().unwrap().iter_mut())
        .filter(|l| l.get("source").is_some())
    {
        l["source"]["cargo"] = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml").into();
    }
    let project = dir.join("plugin.cut.json");
    std::fs::write(&project, v.to_string()).unwrap();
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = dir.join("cargo.log");
    let fake = bin.join("cargo");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\necho \"$PWD|$RUSTUP_TOOLCHAIN\" >> '{}'\nexec '{}' \"$@\"\n",
            log.display(),
            env!("CARGO")
        ),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    // The examples were built into the test's target dir; build there.
    let target = Path::new(BIN).parent().unwrap().parent().unwrap();
    let o = Command::new(BIN)
        .current_dir(&dir)
        .env("PATH", path)
        .env("RUSTUP_TOOLCHAIN", "somebody-elses")
        .env("CARGO_TARGET_DIR", target)
        .args(["still"])
        .arg(&project)
        .args(["--t", "1", "--size", "320x180", "-o"])
        .arg(dir.join("a.png"))
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(
        std::fs::read_to_string(&log).unwrap().trim(),
        format!("{}|", env!("CARGO_MANIFEST_DIR"))
    );
}

const STAGE3D: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/stage3d.cut.json");

/// stage3d and its model in a scratch dir, so its cache lands there.
fn stage3d(name: &str) -> PathBuf {
    let d = scratch(name);
    let ex = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    for f in ["stage3d.cut.json", "knot.glb"] {
        std::fs::copy(ex.join(f), d.join(f)).unwrap();
    }
    d.join("stage3d.cut.json")
}

fn count(dir: &Path, ext: &str) -> usize {
    std::fs::read_dir(dir).map_or(0, |d| {
        d.filter(|e| {
            e.as_ref()
                .is_ok_and(|e| e.path().extension().is_some_and(|x| x == ext))
        })
        .count()
    })
}

#[test]
fn blender_renders_a_small_3d_still_and_then_reuses_it() {
    let ok = Command::new("blender")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        eprintln!("skipped: no blender on PATH");
        return;
    }
    let project = stage3d("blender-still");
    let cache = project.parent().unwrap().join(".mui-cut-cache/blender");
    let out = project.with_file_name("f.png");
    let still = || {
        Command::new(BIN)
            .args(["still"])
            .arg(&project)
            .args([
                "--renderer",
                "blender",
                "--samples",
                "4",
                "--t",
                "3",
                "--size",
                "160x90",
                "-o",
            ])
            .arg(&out)
            .output()
            .unwrap()
    };
    let first = still();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&out).unwrap()));
    let mut r = dec.read_info().unwrap();
    assert_eq!((r.info().width, r.info().height), (160, 90));
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    r.next_frame(&mut buf).unwrap();
    // Cards, shadows and floor, not one flat colour.
    let lo = buf.iter().step_by(4).min().unwrap();
    let hi = buf.iter().step_by(4).max().unwrap();
    assert!(hi - lo > 100, "{lo}..{hi}");
    assert_eq!(count(&cache.join("frames"), "png"), 1);
    assert_eq!(count(&cache, "blend"), 1);
    // Nothing changed: the same frame, from the cache.
    let again = still();
    assert!(again.status.success());
    assert!(!String::from_utf8_lossy(&again.stderr).contains("blender frame 1/1"));
    assert_eq!(count(&cache.join("frames"), "png"), 1);
    assert_eq!(count(&cache, "blend"), 1);
    // The scene's grain runs over Blender's frame: without it, the same
    // cached frame comes out untouched.
    let grained = std::fs::read(&out).unwrap();
    let json = std::fs::read_to_string(&project).unwrap();
    let grain = "\"type\": \"grain\",\n          \"amount\": 0.05";
    assert!(json.contains(grain));
    std::fs::write(
        &project,
        json.replace(grain, "\"type\": \"grain\",\n          \"amount\": 0.0"),
    )
    .unwrap();
    let plain = still();
    assert!(
        plain.status.success(),
        "{}",
        String::from_utf8_lossy(&plain.stderr)
    );
    assert_eq!(count(&cache.join("frames"), "png"), 1);
    assert_ne!(grained, std::fs::read(&out).unwrap());
}

/// Blender plays a model's glTF animation at the frame's time, as
/// mui-stage does: the toy's knee is straight at 0 s and bent at 1 s, and
/// the caption overlay is drawn over both.
#[test]
fn blender_plays_a_models_animation_at_the_frames_time() {
    let ok = Command::new("blender")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        eprintln!("skipped: no blender on PATH");
        return;
    }
    let d = scratch("blender-toy");
    let ex = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    std::fs::copy(ex.join("toy.glb"), d.join("toy.glb")).unwrap();
    // The camera held still: only the model moves.
    let mut doc: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(ex.join("stage3d-overlay.cut.json")).unwrap(),
    )
    .unwrap();
    doc["scenes"][0]["layers"][0]["x"] = serde_json::json!(600.0);
    std::fs::write(d.join("stage3d-overlay.cut.json"), doc.to_string()).unwrap();
    let still = |t: &str| {
        let out = d.join(format!("f{t}.png"));
        let o = Command::new(BIN)
            .arg("still")
            .arg(d.join("stage3d-overlay.cut.json"))
            .args([
                "--renderer",
                "blender",
                "--samples",
                "4",
                "--size",
                "320x180",
            ])
            .args(["--t", t, "-o"])
            .arg(&out)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&out).unwrap()));
        let mut r = dec.read_info().unwrap();
        assert_eq!(r.info().color_type, png::ColorType::Rgba);
        let mut buf = vec![0; r.output_buffer_size().unwrap()];
        r.next_frame(&mut buf).unwrap();
        buf
    };
    let (a, b) = (still("0"), still("1"));
    let px = |buf: &[u8], x: usize, y: usize| {
        let i = (y * 320 + x) * 4;
        [buf[i], buf[i + 1], buf[i + 2]]
    };
    let moved = a
        .iter()
        .zip(&b)
        .filter(|(x, y)| x.abs_diff(**y) > 40)
        .count();
    assert!(moved > 300, "{moved} channels moved");
    // The caption: white text on the dark plate, low in the middle.
    let white = |buf: &[u8]| (150..170).any(|y| (110..210).any(|x| px(buf, x, y)[0] > 200));
    assert!(white(&a) && white(&b));
}

#[test]
fn blender_refuses_2d_scenes_and_says_when_it_is_missing() {
    let out = scratch("blender-errors").join("f.png");
    let o = Command::new(BIN)
        .args(["still", DEMO, "--renderer", "blender", "-o"])
        .arg(&out)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success() && err.contains("is 2D"), "{err}");
    let o = Command::new(BIN)
        .args(["still", STAGE3D, "--renderer", "blender", "-o"])
        .arg(&out)
        .env("MUI_CUT_BLENDER", "/nonexistent/blender")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        !o.status.success() && err.contains("MUI_CUT_BLENDER"),
        "{err}"
    );
}

/// `mui-cut add` on a plugin crate that depends on MUI by git: zero config,
/// it is built against this MUI tree (its lock names MUI as the local path,
/// and the crate uses `mui::host::headless`, which only this tree has), its
/// own Cargo.lock stays byte for byte, the project gets a valid source, and
/// the part tree comes back.
#[test]
fn add_builds_a_git_mui_plugin_against_this_tree_and_lists_its_parts() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plain");
    let lock = std::fs::read(fixture.join("Cargo.lock")).unwrap();
    let dir = scratch("add");
    let project = dir.join("p.cut.json");
    std::fs::write(
        &project,
        r#"{"size": [640, 360], "fps": 30, "scenes": [{"name": "main", "duration": 1, "layers": []}]}"#,
    )
    .unwrap();
    let out = Command::new(BIN)
        .args(["add"])
        .arg(&fixture)
        .arg("--project")
        .arg(&project)
        .arg("--json")
        .env("MUI_CUT_CACHE", dir.join("cache"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let r: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(r["framework"], "mui");
    assert_eq!(r["id"], "fixture-plain");
    assert!(
        Path::new(r["adapter"].as_str().unwrap()).is_file(),
        "the report names the built adapter: {}",
        r["adapter"]
    );
    let parts = r["parts"].to_string();
    assert!(
        parts.contains("tone-color") && parts.contains("space-mix"),
        "{parts}"
    );
    assert_eq!(
        std::fs::read(fixture.join("Cargo.lock")).unwrap(),
        lock,
        "the plugin's lock is untouched"
    );
    // The adapter's lock: MUI from the local path (no git source).
    let adapters = std::fs::read_dir(dir.join("cache/adapters")).unwrap();
    let built =
        std::fs::read_to_string(adapters.flatten().next().unwrap().path().join("Cargo.lock"))
            .unwrap();
    let mui = built
        .split("[[package]]")
        .find(|b| b.contains("\nname = \"mui\"\n"))
        .unwrap();
    assert!(!mui.contains("source ="), "mui came from git:\n{mui}");
    let p: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&project).unwrap()).unwrap();
    let src = &p["sources"][0];
    assert_eq!(src["kind"], "plugin");
    assert!(
        src["source"]["plugin"]
            .as_str()
            .unwrap()
            .ends_with("tests/fixtures/plain"),
        "{src}"
    );
    let check = Command::new(BIN)
        .arg("check")
        .arg(&project)
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stdout)
    );
    // A keyed view size reflows the generated adapter's editor, as a
    // window resize would: the panels grow into the wider view.
    let source = src["source"].clone();
    let mut p = p;
    p["scenes"][0]["layers"] = serde_json::json!([{
        "id": "k", "kind": "plugin", "source": source,
        "view_width": [{"t": 0, "v": 400, "interp": "hold"}, {"t": 0.5, "v": 700, "interp": "hold"}],
        "view_height": 300,
    }]);
    std::fs::write(&project, p.to_string()).unwrap();
    let capture = Command::new(BIN)
        .arg("capture")
        .arg(&project)
        .env("MUI_CUT_CACHE", dir.join("cache"))
        .output()
        .unwrap();
    assert!(
        capture.status.success(),
        "{}",
        String::from_utf8_lossy(&capture.stderr)
    );
    let at = |t: &str| -> serde_json::Value {
        let out = Command::new(BIN)
            .args(["eval"])
            .arg(&project)
            .args(["--t", t])
            .output()
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let state = v["layers"][0]["plugin"]["state"]
            .as_str()
            .unwrap()
            .to_owned();
        serde_json::from_str(
            &std::fs::read_to_string(dir.join(".cut-cache").join(state + ".json")).unwrap(),
        )
        .unwrap()
    };
    let (narrow, wide) = (at("0.2"), at("0.8"));
    assert_eq!(
        (narrow["width"].as_f64(), wide["width"].as_f64()),
        (Some(400.), Some(700.))
    );
    // The second panel's knob moves right as the first panel grows.
    let x = |m: &serde_json::Value| {
        m["parts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "space-size")
            .unwrap()["frame"][0]
            .as_f64()
            .unwrap()
    };
    assert!(
        x(&wide) > x(&narrow) + 100.,
        "{} vs {}",
        x(&wide),
        x(&narrow)
    );
}
