use mui::prelude::*;

use crate::{Scene, Script, run};

fn scene() -> Scene {
    let font = Font::new(epaint_default_fonts::HACK_REGULAR).unwrap();
    let mut on = false;
    let mut gain = 0.5;
    Scene::new("demo", (240, 120), Ui::default().font(font), move |ui| {
        let t = toggle(ui, "bypass", "Bypass", &mut on).el.into_el();
        let g = slider(ui, "gain", "Gain", &mut gain, 0.0..=1.0)
            .el
            .into_el();
        col![t, g].gap(M).pad(L).fill(Role::Surface)
    })
}

fn go(args: &[&str]) -> crate::Value {
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
    run(vec![scene()], &args).unwrap_or_else(|e| panic!("{e}"))
}

fn tmp(name: &str) -> String {
    let d = std::env::temp_dir().join("mui-headless-tests");
    std::fs::create_dir_all(&d).unwrap();
    d.join(name).to_str().unwrap().to_owned()
}

const CLICK: &str =
    r#"[{"t":0.1,"type":"click","key":"bypass"},{"t":0.5,"type":"mark","name":"end"}]"#;

#[test]
fn a_script_and_its_times_parse() {
    let s = Script::parse(CLICK, 60).unwrap();
    assert_eq!(s.end, 0.5);
    assert_eq!(s.time("mark:end+0.25").unwrap(), 0.75);
    assert_eq!(s.time("1.5").unwrap(), 1.5);
    assert!(s.time("mark:nope").is_err());
    assert!(Script::parse(r#"[{"type":"wiggle"}]"#, 60).is_err());
    assert!(Script::parse(r#"[{"type":"move","x":1}]"#, 60).is_err());
}

#[test]
fn a_mid_animation_frame_is_reproducible_and_differs_from_the_settled_one() {
    let (a, b, c) = (tmp("a.png"), tmp("b.png"), tmp("c.png"));
    let script = tmp("click.json");
    std::fs::write(&script, CLICK).unwrap();
    go(&["--script", &script, "--at", "0.15", "--snap", &a]);
    go(&["--script", &script, "--at", "0.15", "--snap", &b]);
    go(&["--script", &script, "--at", "1.5", "--snap", &c]);
    let read = |p: &str| std::fs::read(p).unwrap();
    assert_eq!(read(&a), read(&b), "same time, different bytes");
    assert_ne!(
        read(&a),
        read(&c),
        "the toggle did not move between 0.15s and 1.5s"
    );
}

#[test]
fn a_click_flips_the_toggle_and_the_trace_says_it_settled() {
    let (tree, trace) = (tmp("t.json"), tmp("t.jsonl"));
    let script = tmp("click2.json");
    std::fs::write(&script, CLICK).unwrap();
    let r = go(&[
        "--script", &script, "--at", "2", "--tree", &tree, "--trace", &trace,
    ]);
    let v: crate::Value = serde_json::from_str(&std::fs::read_to_string(&tree).unwrap()).unwrap();
    let t = v["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "bypass")
        .unwrap();
    assert_eq!(t["value"], true, "{t}");
    let lines: Vec<crate::Value> = std::fs::read_to_string(&trace)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 120);
    assert!(
        lines
            .iter()
            .any(|l| !l["springs"].as_array().unwrap().is_empty()),
        "no spring ever moved"
    );
    assert_eq!(lines.last().unwrap()["animating"], false, "never settled");
    // Nothing changes before the click lands, and the click's frame is named.
    assert!(
        lines[..5]
            .iter()
            .all(|l| l["changed"].as_array().unwrap().is_empty())
    );
    assert!(lines.iter().any(|l| {
        l["changed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "bypass")
    }));
    assert!(r["settled_after_frame"].as_u64().unwrap() < 120);
}

#[test]
fn a_wrong_element_id_names_the_ones_that_exist() {
    let script = tmp("bad.json");
    std::fs::write(&script, r#"[{"t":0.1,"type":"click","key":"nope"}]"#).unwrap();
    let args: Vec<String> = ["--script", &script, "--tree", "/dev/null"]
        .map(Into::into)
        .to_vec();
    let e = run(vec![scene()], &args).err().unwrap().to_string();
    assert!(e.contains("'nope'") && e.contains("bypass"), "{e}");
}

#[test]
fn frames_and_a_strip_are_written() {
    let dir = tmp("frames");
    let strip = tmp("strip.png");
    let r = go(&[
        "--frames", "6", "--every", "2", "--out", &dir, "--strip", &strip,
    ]);
    assert_eq!(r["wrote"].as_array().unwrap().len(), 4, "{r}");
    assert!(std::path::Path::new(&dir).join("frame_0004.png").exists());
}

#[test]
fn unbounded_inputs_are_refused_and_outputs_find_their_way() {
    let refuse = |args: &[&str]| {
        let args: Vec<String> = args.iter().map(ToString::to_string).collect();
        run(vec![scene()], &args).err().map(|e| e.to_string())
    };
    // Times, durations and sizes that would run or allocate without end.
    assert!(refuse(&["--at", "1e300"]).is_some());
    assert!(refuse(&["--at", "inf"]).is_some());
    assert!(refuse(&["--size", "60000x60000"]).is_some());
    assert!(refuse(&["--frames", "100000", "--strip", &tmp("big.png")]).is_some());
    assert!(Script::parse(r#"[{"type":"drag","key":"gain","dur":1e9}]"#, 60).is_err());
    assert!(Script::parse(r#"[{"type":"wait","t":3000,"dur":3000}]"#, 60).is_err());
    // Two clicks at once would press while the first is held.
    let both = r#"[{"t":0.1,"key":"bypass"},{"t":0.1,"key":"gain"}]"#;
    assert!(Script::parse(both, 60).is_err());
    assert!(Script::parse(r#"[{"t":0.1,"key":"bypass"},{"t":0.3,"key":"gain"}]"#, 60).is_ok());
    // A name outside ASCII is still a name.
    assert_eq!(crate::slug("東京 Night"), "東京-night");
    // Missing directories are made, and a first-frame click is a change.
    let dir = tmp("nested/a/b");
    let _ = std::fs::remove_dir_all(tmp("nested"));
    let script = tmp("first.json");
    std::fs::write(&script, r#"[{"t":0,"type":"click","key":"bypass"}]"#).unwrap();
    let r = go(&[
        "--script",
        &script,
        "--tree",
        &format!("{dir}/tree.json"),
        "--trace",
        &format!("{dir}/trace.jsonl"),
    ]);
    assert_eq!(r["values"][0]["id"], "bypass", "{r}");
    let first = std::fs::read_to_string(format!("{dir}/trace.jsonl")).unwrap();
    let first: crate::Value = serde_json::from_str(first.lines().next().unwrap()).unwrap();
    assert!(!first["changed"].as_array().unwrap().is_empty(), "{first}");
}
