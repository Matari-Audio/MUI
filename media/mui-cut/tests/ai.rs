//! The agent tools end to end: `schema`, `check`, `sheet`, `strip`, `diff`,
//! `gen` and the MCP server over stdio.
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_mui-cut");
const DEMO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/demo.cut.json");

/// A scratch dir on disk (the target dir), not the RAM-backed /tmp.
fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn png_size(path: &Path) -> (u32, u32) {
    let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    let r = dec.read_info().unwrap();
    (r.info().width, r.info().height)
}

fn run(args: &[&str]) -> (bool, String) {
    let o = Command::new(BIN).args(args).output().unwrap();
    let text =
        String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    (o.status.success(), text)
}

#[test]
fn schema_prints_a_2020_12_schema() {
    let (ok, text) = run(&["schema"]);
    assert!(ok, "{text}");
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(v["$schema"].as_str().unwrap().contains("2020-12"));
    assert!(v["$defs"]["Layer"].is_object());
}

#[test]
fn check_exits_non_zero_on_errors_and_speaks_json() {
    let d = scratch("check");
    let bad = d.join("bad.cut.json");
    std::fs::write(
        &bad,
        r#"{"size": [320, 180], "fps": 30, "scenes": [{"name": "s", "duration": 1, "layers": [
            {"id": "pic", "kind": "image", "path": "nope.png", "x": 160, "y": 90, "opcity": 1}]}]}"#,
    )
    .unwrap();
    let (ok, text) = run(&["check", bad.to_str().unwrap(), "--json"]);
    assert!(!ok);
    let json: serde_json::Value =
        serde_json::from_str(text.split("\nmui-cut:").next().unwrap()).unwrap();
    assert_eq!(json["errors"], 1, "{json}");
    let codes: Vec<&str> = json["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["code"].as_str().unwrap())
        .collect();
    assert!(
        codes.contains(&"missing_asset") && codes.contains(&"unknown_field"),
        "{codes:?}"
    );
    let (ok, text) = run(&["check", DEMO]);
    assert!(ok, "{text}");
}

#[test]
fn sheet_strip_and_diff_write_pictures_of_the_right_size() {
    let d = scratch("pictures");
    let sheet = d.join("s.png");
    let (ok, text) = run(&[
        "sheet",
        DEMO,
        "--scene",
        "title",
        "--times",
        "0,1",
        "-o",
        sheet.to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    // Two 392x220 tiles (1600 wide / 4 columns), 6 px gaps round them.
    assert_eq!(png_size(&sheet), (6 + 2 * 398, 6 + 226), "{text}");
    let (ok, text) = run(&[
        "sheet",
        DEMO,
        "--width",
        "800",
        "-o",
        sheet.to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    assert_eq!(png_size(&sheet).0, 6 + 4 * (192 + 6), "{text}");

    let strip = d.join("t.png");
    let (ok, text) = run(&[
        "strip",
        DEMO,
        "--layer",
        "title",
        "--width",
        "640",
        "-o",
        strip.to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    assert_eq!(png_size(&strip), (640, 360));
    assert!(text.contains("0.00s → 0.90s"), "{text}");

    let b = d.join("b.cut.json");
    std::fs::write(
        &b,
        std::fs::read_to_string(DEMO)
            .unwrap()
            .replace("#7c6cff", "#ff5050"),
    )
    .unwrap();
    let out = d.join("d.png");
    let (ok, text) = run(&[
        "diff",
        DEMO,
        b.to_str().unwrap(),
        "--n",
        "2",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert!(ok, "{text}");
    assert_eq!(text.matches("% of pixels changed").count(), 2, "{text}");
    let (w, h) = png_size(&out);
    assert_eq!((w, h), (6 + 3 * (524 + 6), 6 + 2 * (294 + 6)), "{text}");
    let (ok, text) = run(&["diff", DEMO, DEMO, "-o", out.to_str().unwrap()]);
    assert!(ok && text.contains("no visible differences"), "{text}");
}
