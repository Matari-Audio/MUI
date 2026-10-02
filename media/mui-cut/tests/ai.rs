//! The agent tools end to end: `schema`, `check`, `sheet`, `strip`, `diff`,
//! `gen` and the MCP server over stdio.
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_mui-cut");
const DEMO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/demo.cut.json");

/// A scratch dir on disk (the target dir), not the RAM-backed /tmp.
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
    assert_eq!(png_size(&sheet), (6 + 2 * 398, 6 + 220 + 22 + 6), "{text}");
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
    // Every engine draws the same sheet shape (the CPU one on every core).
    for r in ["classic", "gpu", "cpu"] {
        let (ok, text) = run(&[
            "sheet",
            DEMO,
            "--scene",
            "title",
            "--times",
            "0,1",
            "--renderer",
            r,
            "-o",
            sheet.to_str().unwrap(),
        ]);
        assert!(ok, "{r}: {text}");
        assert_eq!(png_size(&sheet), (6 + 2 * 398, 6 + 220 + 22 + 6), "{r}");
    }

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
    assert_eq!(
        (w, h),
        (6 + 3 * (524 + 6), 6 + 2 * (294 + 22 + 6)),
        "{text}"
    );
    let (ok, text) = run(&["diff", DEMO, DEMO, "-o", out.to_str().unwrap()]);
    assert!(ok && text.contains("no visible differences"), "{text}");
}

#[test]
fn diff_reads_a_git_revision_and_its_assets() {
    let d = scratch("diff-rev");
    let git = |args: &[&str]| {
        let o = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            // No user hooks in a scratch repo.
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "init.defaultBranch=work",
            ])
            .args(args)
            .current_dir(&d)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    };
    let svg = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/mark.svg");
    std::fs::create_dir_all(d.join("art")).unwrap();
    std::fs::copy(svg, d.join("art/mark.svg")).unwrap();
    let project = d.join("p.cut.json");
    std::fs::write(
        &project,
        r##"{"size": [640, 360], "fps": 30, "scenes": [{"name": "s", "duration": 1, "layers": [
            {"id": "m", "kind": "svg", "path": "art/mark.svg", "x": 320, "y": 180, "width": 240, "height": 240}
        ]}]}"##,
    )
    .unwrap();
    git(&["init", "-q"]);
    git(&["add", "."]);
    git(&["commit", "-qm", "one"]);
    // Only the asset changes: the project file is the same at HEAD.
    let red = std::fs::read_to_string(svg)
        .unwrap()
        .replace("#ffcf5c", "#ff0000");
    std::fs::write(d.join("art/mark.svg"), red).unwrap();
    let out = d.join("d.png");
    for args in [
        vec!["diff", "p.cut.json@HEAD"],
        vec!["diff", "p.cut.json", "--rev", "HEAD"],
    ] {
        let o = Command::new(BIN)
            .args(&args)
            .args(["--n", "1", "-o", out.to_str().unwrap()])
            .current_dir(&d)
            .output()
            .unwrap();
        let text =
            String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
        assert!(o.status.success(), "{args:?}: {text}");
        assert!(text.contains("% of pixels changed"), "{args:?}: {text}");
    }
    let left: Vec<_> = std::fs::read_dir(&d)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with(".mui-cut-rev"))
        .collect();
    assert!(left.is_empty(), "the revision's copy is left behind");
    let (ok, text) = run(&["diff", project.to_str().unwrap(), "--rev", "nope"]);
    assert!(!ok && text.contains("nope"), "{text}");
}

/// `mui-cut mcp` driven like an MCP client: one JSON-RPC message a line.
struct Mcp {
    child: std::process::Child,
    out: std::io::BufReader<std::process::ChildStdout>,
    next: u64,
}

impl Mcp {
    fn start() -> Self {
        let mut child = Command::new(BIN)
            .arg("mcp")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let out = std::io::BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            out,
            next: 0,
        }
    }
    fn send(&mut self, msg: &serde_json::Value) {
        use std::io::Write as _;
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
    }
    fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        use std::io::BufRead as _;
        self.next += 1;
        let id = self.next;
        let mut msg = serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method});
        msg["params"] = params;
        self.send(&msg);
        let mut line = String::new();
        self.out.read_line(&mut line).unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], id, "{v}");
        v
    }
    /// A tool's content, and whether it was an error.
    fn call(&mut self, name: &str, args: serde_json::Value) -> (Vec<serde_json::Value>, bool) {
        let mut params = serde_json::json!({ "name": name });
        params["arguments"] = args;
        let v = self.request("tools/call", params);
        let r = &v["result"];
        (
            r["content"].as_array().unwrap().clone(),
            r["isError"].as_bool().unwrap(),
        )
    }
    fn text(&mut self, name: &str, args: serde_json::Value) -> String {
        let (c, err) = self.call(name, args);
        let t = c
            .iter()
            .filter_map(|c| c["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!err, "{name}: {t}");
        t
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn http(port: u16, req: &str) -> String {
    use std::io::{Read as _, Write as _};
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

#[test]
fn mcp_opens_edits_checks_and_looks_at_a_project() {
    let d = scratch("mcp");
    let project = d.join("p.cut.json");
    std::fs::copy(DEMO, &project).unwrap();
    let mut m = Mcp::start();
    let init = m.request("initialize", serde_json::json!({"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}));
    assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(init["result"]["serverInfo"]["name"], "mui-cut");
    m.send(&serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    let list = m.request("tools/list", serde_json::json!({}));
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for n in [
        "open",
        "schema",
        "list",
        "get",
        "patch",
        "set",
        "key",
        "add_layer",
        "remove_layer",
        "eval",
        "check",
        "still",
        "sheet",
        "strip",
        "diff",
        "render",
        "render_status",
        "editor_state",
        "editor_goto",
    ] {
        assert!(names.contains(&n), "{n} missing from {names:?}");
    }
    assert_eq!(list["result"]["tools"][0]["inputSchema"]["type"], "object");
    let unknown = m.request("nope", serde_json::json!({}));
    assert_eq!(unknown["error"]["code"], -32601);

    // Nothing open yet: a tool error, not a protocol one.
    let (c, err) = m.call("list", serde_json::json!({}));
    assert!(err && c[0]["text"].as_str().unwrap().contains("open"));

    let outline = m.text("open", serde_json::json!({"path": project}));
    assert!(
        outline.contains("\"shapes\"") && outline.contains("\"animated\""),
        "{outline}"
    );
    let got = m.text(
        "get",
        serde_json::json!({"pointer": "/scenes/title/layers/title/text"}),
    );
    assert_eq!(got, "\"mui-cut\"");

    // Edits by name, written canonically.
    let r = m.text(
        "patch",
        serde_json::json!({"ops": [
        {"op": "replace", "path": "/scenes/title/layers/title/text", "value": "hello"}]}),
    );
    assert!(r.contains("written") && r.contains("check:"), "{r}");
    let file = std::fs::read_to_string(&project).unwrap();
    assert!(file.contains("\"text\": \"hello\""));
    m.text(
        "set",
        serde_json::json!({"scene": "title", "layer": "bar", "prop": "fill", "value": "#ff0000"}),
    );
    m.text(
        "key",
        serde_json::json!({"scene": "title", "layer": "bar", "prop": "x", "t": 1, "v": 700}),
    );
    m.text("key", serde_json::json!({"scene": "title", "layer": "bar", "prop": "x", "t": 0, "v": 600, "interp": "linear"}));
    let e = m.text(
        "eval",
        serde_json::json!({"scene": "title", "t": 0.5, "layer": "bar"}),
    );
    let e: serde_json::Value = serde_json::from_str(&e).unwrap();
    assert_eq!(e["x"], 650.0);
    assert_eq!(e["fill"], "#ff0000");
    m.text("add_layer", serde_json::json!({"scene": "title", "layer": {"id": "dot", "kind": "ellipse", "x": 100, "y": 100}}));
    m.text(
        "remove_layer",
        serde_json::json!({"scene": "title", "id": "dot"}),
    );

    // A typo would be dropped by a save, so it is refused; so is a bad value.
    let before = std::fs::read_to_string(&project).unwrap();
    let (c, err) = m.call(
        "set",
        serde_json::json!({"scene": "title", "layer": "bar", "prop": "opacty", "value": 1}),
    );
    assert!(
        err && c[0]["text"]
            .as_str()
            .unwrap()
            .contains("did you mean `opacity`"),
        "{c:?}"
    );
    let (c, err) = m.call(
        "set",
        serde_json::json!({"scene": "title", "layer": "bar", "prop": "x", "value": "far"}),
    );
    assert!(
        err && c[0]["text"]
            .as_str()
            .unwrap()
            .contains("scenes[0].layers[0].x"),
        "{c:?}"
    );
    assert_eq!(std::fs::read_to_string(&project).unwrap(), before);

    let (c, err) = m.call(
        "still",
        serde_json::json!({"scene": "shapes", "t": 1, "width": 320}),
    );
    assert!(!err, "{c:?}");
    assert_eq!(c[0]["type"], "image");
    assert_eq!(c[0]["mimeType"], "image/png");
    assert!(c[0]["data"].as_str().unwrap().starts_with("iVBORw0KGgo"));
    let (c, err) = m.call(
        "sheet",
        serde_json::json!({"scene": "title", "n": 3, "width": 800}),
    );
    assert!(!err && c[0]["type"] == "image", "{c:?}");
    let (c, err) = m.call("strip", serde_json::json!({"layer": "title", "width": 400}));
    assert!(!err && c[0]["type"] == "image", "{c:?}");
    let check = m.text("check", serde_json::json!({}));
    assert!(check.starts_with("check: 0 errors"), "{check}");
}

/// An agent builds a scene in two calls: `open` creates it at its size and
/// length, and one `batch` adds layers, keys and named motions, written
/// once. A bad call anywhere leaves the file as it was.
#[test]
fn mcp_batches_edits_and_motions_into_one_write() {
    let d = scratch("mcp-batch");
    let project = d.join("p.cut.json");
    let mut m = Mcp::start();
    let out = m.text(
        "open",
        serde_json::json!({"path": project, "create": true, "size": [640, 360],
            "scene": "intro", "duration": 4, "background": "#202024"}),
    );
    assert!(out.contains("\"intro\""), "{out}");
    let said = m.text(
        "batch",
        serde_json::json!({"calls": [
            {"tool": "add_layer", "args": {"layer": {"id": "title", "kind": "text", "text": "Hi", "x": 320, "y": 180, "font_size": 60}}},
            {"tool": "motion", "args": {"scene": "intro", "layer": "title", "preset": "rise_in", "t": 0.2, "dur": 0.5}},
            {"tool": "motion", "args": {"layer": "title", "preset": "fade_out", "t": 3, "dur": 0.5}},
            {"tool": "add_layer", "args": {"layer": {"id": "dot", "kind": "ellipse", "x": 100, "y": 100, "width": 20, "height": 20}}},
            {"tool": "key", "args": {"layer": "dot", "prop": "x", "t": 1, "v": 540}},
            {"tool": "set", "args": {"layer": "dot", "prop": "fill", "value": "#ff5a36"}}
        ]}),
    );
    assert_eq!(said.matches("written").count(), 1, "{said}");
    let saved = std::fs::read_to_string(&project).unwrap();
    let v: serde_json::Value = serde_json::from_str(&saved).unwrap();
    let s = &v["scenes"][0];
    assert_eq!(
        (s["name"].as_str(), s["duration"].as_f64()),
        (Some("intro"), Some(4.))
    );
    assert_eq!(v["size"], serde_json::json!([640, 360]));
    let title = &s["layers"][0];
    assert_eq!(title["opacity"].as_array().unwrap().len(), 4, "{title}");
    assert_eq!(title["y"][0]["v"], 220.0);
    assert_eq!(s["layers"][1]["x"].as_array().unwrap().len(), 1);
    assert_eq!(s["layers"][1]["fill"], "#ff5a36");

    // All or nothing: the third call fails, so the first two never land.
    let (c, err) = m.call(
        "batch",
        serde_json::json!({"calls": [
            {"tool": "set", "args": {"layer": "dot", "prop": "y", "value": 300}},
            {"tool": "remove_layer", "args": {"id": "title"}},
            {"tool": "key", "args": {"layer": "dott", "prop": "x", "t": 2, "v": 0}}
        ]}),
    );
    let e = c[0]["text"].as_str().unwrap();
    assert!(
        err && e.contains("calls[2] (key)") && e.contains("did you mean `dot`"),
        "{e}"
    );
    assert_eq!(std::fs::read_to_string(&project).unwrap(), saved);
    // Only edit tools batch.
    let (c, err) = m.call(
        "batch",
        serde_json::json!({"calls": [{"tool": "still", "args": {"t": 0}}]}),
    );
    assert!(err && c[0]["text"].as_str().unwrap().contains("not an edit tool"));

    // A beauty still says so, with its time; a bad sample count is refused.
    let (c, err) = m.call(
        "still",
        serde_json::json!({"t": 1, "width": 320, "samples": 0}),
    );
    assert!(
        err && c[0]["text"].as_str().unwrap().contains("1..=4096"),
        "{c:?}"
    );
    let (c, err) = m.call("still", serde_json::json!({"t": 1, "width": 320}));
    assert!(
        !err && c[1]["text"].as_str().unwrap().contains(" ms"),
        "{c:?}"
    );
}

#[test]
fn mcp_sees_and_steers_the_open_editor() {
    use std::io::{BufRead as _, Write as _};
    let d = scratch("mcp-editor");
    let project = d.join("p.cut.json");
    std::fs::copy(DEMO, &project).unwrap();
    let port = free_port();
    let mut server = Command::new(BIN)
        .arg("serve")
        .arg(&project)
        .args(["--port", &port.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(start.elapsed().as_secs() < 10, "server never came up");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    // An editor: listening for events, reporting its state.
    let mut events = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    events.write_all(b"GET /events HTTP/1.1\r\n\r\n").unwrap();
    events
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    let mut lines = std::io::BufReader::new(events);
    let mut l = String::new();
    while !l.starts_with(": hello") {
        l.clear();
        lines.read_line(&mut l).unwrap();
    }
    let state =
        r#"{"scene": "shapes", "t": 1.25, "selection": "card", "prop": "x", "playing": false}"#;
    let put = http(
        port,
        &format!(
            "PUT /state HTTP/1.1\r\nContent-Length: {}\r\n\r\n{state}",
            state.len()
        ),
    );
    assert!(put.starts_with("HTTP/1.1 200"), "{put}");

    let mut m = Mcp::start();
    m.request(
        "initialize",
        serde_json::json!({"protocolVersion": "2025-06-18"}),
    );
    m.text(
        "open",
        serde_json::json!({"path": project, "editor_port": port}),
    );
    let seen = m.text("editor_state", serde_json::json!({}));
    let seen: serde_json::Value = serde_json::from_str(&seen).unwrap();
    assert_eq!(seen["state"]["selection"], "card", "{seen}");
    assert_eq!(seen["state"]["t"], 1.25);
    assert_eq!(seen["same_project"], true, "{seen}");
    assert_eq!(seen["editors"], 1, "{seen}");

    let sent = m.text(
        "editor_goto",
        serde_json::json!({"scene": "title", "t": 0.5, "select": "bar"}),
    );
    assert!(sent.contains("\"editors\": 1"), "{sent}");
    let mut got = Vec::new();
    loop {
        l.clear();
        lines.read_line(&mut l).unwrap();
        got.push(l.trim().to_owned());
        if l.starts_with("data: {") {
            break;
        }
    }
    assert!(got.contains(&"event: control".to_owned()), "{got:?}");
    let msg: serde_json::Value =
        serde_json::from_str(l.trim().strip_prefix("data: ").unwrap()).unwrap();
    assert_eq!(
        msg,
        serde_json::json!({"scene": "title", "t": 0.5, "select": "bar"})
    );

    // An agent's edit goes through the editor's server and reaches the
    // editor as the merged document.
    let said = m.text(
        "set",
        serde_json::json!({"scene": "title", "layer": "bar", "prop": "y", "value": 450}),
    );
    assert!(
        said.contains(&format!("through the editor on port {port}")),
        "{said}"
    );
    let doc = loop {
        l.clear();
        lines.read_line(&mut l).unwrap();
        if let Some(d) = l.trim().strip_prefix("data: {") {
            let v: serde_json::Value = serde_json::from_str(&format!("{{{d}")).unwrap();
            if v["by"] == "agent" {
                break v;
            }
        }
    };
    assert_eq!(doc["by"], "agent", "{doc}");
    let title = doc["doc"]["scenes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "title")
        .unwrap();
    let bar = title["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["id"] == "bar")
        .unwrap();
    assert_eq!(bar["y"], 450.0, "{bar}");

    // The person, still on the revision before it, moves `bar` (x and y):
    // x merges, y was the agent's too and the later edit (theirs) wins.
    let base = doc["rev"].as_u64().unwrap() - 1;
    let patch = serde_json::json!({"base": base, "by": "editor", "ops": [
        {"op": "replace", "path": "/scenes/title/layers/bar/x", "value": 77.0},
        {"op": "replace", "path": "/scenes/title/layers/bar/y", "value": 88.0},
    ]})
    .to_string();
    let got = http(
        port,
        &format!(
            "POST /patch HTTP/1.1\r\nContent-Length: {}\r\n\r\n{patch}",
            patch.len()
        ),
    );
    let reply: serde_json::Value =
        serde_json::from_str(got.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(
        reply["conflicts"],
        serde_json::json!(["/scenes/title/layers/bar/y"]),
        "{reply}"
    );
    let file: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&project).unwrap()).unwrap();
    let title = file["scenes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "title")
        .unwrap();
    let bar = title["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["id"] == "bar")
        .unwrap();
    assert_eq!(
        (bar["x"].as_f64(), bar["y"].as_f64()),
        (Some(77.0), Some(88.0)),
        "{bar}"
    );
    let _ = server.kill();
    let _ = server.wait();
}

#[test]
fn mcp_finds_the_editor_by_its_discovery_file() {
    let d = scratch("mcp-discovery");
    let project = d.join("p.cut.json");
    std::fs::copy(DEMO, &project).unwrap();
    let found = d.join(".p.cut.json.serve");
    let port = free_port();
    let mut server = Command::new(BIN)
        .arg("serve")
        .arg(&project)
        .args(["--port", &port.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let start = std::time::Instant::now();
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() || !found.exists() {
        assert!(start.elapsed().as_secs() < 10, "server never came up");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&found).unwrap()).unwrap();
    assert_eq!(v["port"], port, "{v}");
    assert_eq!(v["pid"], server.id(), "{v}");

    // Not on 8740, and `open` is not told the port.
    let mut m = Mcp::start();
    m.request(
        "initialize",
        serde_json::json!({"protocolVersion": "2025-06-18"}),
    );
    m.text("open", serde_json::json!({"path": project}));
    let seen: serde_json::Value =
        serde_json::from_str(&m.text("editor_state", serde_json::json!({}))).unwrap();
    assert_eq!(seen["same_project"], true, "{seen}");

    // SIGTERM: the file goes with the editor.
    #[cfg(unix)]
    {
        let ok = Command::new("kill")
            .args(["-TERM", &server.id().to_string()])
            .status()
            .unwrap();
        assert!(ok.success());
        let _ = server.wait();
        assert!(!found.exists(), "the discovery file outlived the editor");
    }
    let _ = server.kill();
    let _ = server.wait();
}

#[test]
fn gen_is_deterministic_and_merges_layers_into_a_scene() {
    let d = scratch("gen");
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/gen/grid.rhai");
    let (a, b) = (d.join("a.cut.json"), d.join("b.cut.json"));
    for out in [&a, &b] {
        let (ok, text) = run(&["gen", script, "--seed", "5", "-o", out.to_str().unwrap()]);
        assert!(ok, "{text}");
    }
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
    let (ok, text) = run(&["check", a.to_str().unwrap()]);
    assert!(ok, "{text}");

    // Layers from a script land in an existing scene; a rerun replaces them.
    let project = d.join("p.cut.json");
    std::fs::copy(DEMO, &project).unwrap();
    let layers = d.join("dots.rhai");
    std::fs::write(&layers, r#"let l = []; for i in 0..30 { l.push(#{ id: `dot${i}`, kind: "ellipse", x: rand(0.0, 1280.0), y: 60, width: 8, height: 8 }) } l"#).unwrap();
    for _ in 0..2 {
        let (ok, text) = run(&[
            "gen",
            layers.to_str().unwrap(),
            "--into",
            project.to_str().unwrap(),
            "--scene",
            "shapes",
        ]);
        assert!(ok, "{text}");
    }
    let p: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&project).unwrap()).unwrap();
    let shapes = p["scenes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "shapes")
        .unwrap();
    let dots = shapes["layers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["id"].as_str().unwrap().starts_with("dot"))
        .count();
    assert_eq!(dots, 30);
}

/// Every example project, and what every example script writes, fits
/// `mui-cut schema` (variable bindings included) and `check`s without an
/// error.
#[test]
fn every_example_fits_the_schema_and_checks_clean() {
    let (ok, text) = run(&["schema"]);
    assert!(ok, "{text}");
    let schema: serde_json::Value = serde_json::from_str(&text).unwrap();
    let v = jsonschema::validator_for(&schema).unwrap();
    let d = scratch("examples");
    let ex = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut files: Vec<PathBuf> = Vec::new();
    for e in std::fs::read_dir(&ex).unwrap() {
        let p = e.unwrap().path();
        if p.to_str().unwrap().ends_with(".cut.json") {
            files.push(p);
        }
    }
    for e in std::fs::read_dir(ex.join("gen")).unwrap() {
        let script = e.unwrap().path();
        let out = d
            .join(script.file_stem().unwrap())
            .with_extension("cut.json");
        let (ok, text) = run(&["gen", script.to_str().unwrap(), "-o", out.to_str().unwrap()]);
        assert!(ok, "{}: {text}", script.display());
        files.push(out);
    }
    files.sort();
    let names: Vec<_> = files.iter().map(|f| f.file_name().unwrap()).collect();
    for want in [
        "variants.cut.json",
        "stage3d.cut.json",
        "glass.cut.json",
        "effects.cut.json",
        "grid.cut.json",
        "graphite-title.cut.json",
        "glass-orbit.cut.json",
        "synth-explode.cut.json",
    ] {
        assert!(names.iter().any(|n| *n == want), "{want} in {names:?}");
    }
    for f in &files {
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        let errs: Vec<String> = v
            .iter_errors(&doc)
            .map(|e| format!("{} at {}", e, e.instance_path()))
            .collect();
        assert!(errs.is_empty(), "{}: {errs:#?}", f.display());
        let (ok, text) = run(&["check", f.to_str().unwrap(), "--json"]);
        let json: serde_json::Value =
            serde_json::from_str(text.split("\nmui-cut:").next().unwrap()).unwrap();
        assert!(ok && json["errors"] == 0, "{}: {text}", f.display());
    }
}

/// An agent asks what a plugin layer is made of: the adapter runs, and it
/// gets back the parts it can animate and the surfaces it can aim at.
#[test]
fn mcp_lists_a_plugin_layers_parts_and_surfaces() {
    let synth = Path::new(BIN).parent().unwrap().join("examples/synth");
    assert!(
        synth.is_file(),
        "run the whole `cargo test -p mui-cut`: it builds the synth example"
    );
    let d = scratch("mcp-plugin");
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
        l["source"] = serde_json::json!({ "bin": synth });
    }
    v["scenes"][0]["layers"][0]["explode_levels"] = 2.into();
    let project = d.join("p.cut.json");
    std::fs::write(&project, v.to_string()).unwrap();
    let mut m = Mcp::start();
    m.text("open", serde_json::json!({"path": project}));
    let got: serde_json::Value = serde_json::from_str(&m.text(
        "plugin_parts",
        serde_json::json!({"layer": "synth", "t": 6.0}),
    ))
    .unwrap();
    let ids: Vec<&str> = got["parts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["head", "osc", "filter", "env", "out"]);
    assert_eq!(got["size"], serde_json::json!([720.0, 510.0]));
    // The example moves `filter` up and highlights it by 6 s.
    let filter = &got["parts"][2]["motion"];
    assert_eq!(filter["y"], -40.0);
    assert!(filter["highlight"].as_f64().unwrap() > 0.99);
    // Two levels: each panel's controls nest under it, by path, each with
    // a thumbnail on disk.
    let kids: Vec<&str> = got["parts"][2]["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        kids,
        [
            "filter/filter-cutoff",
            "filter/filter-res",
            "filter/filter-drive"
        ]
    );
    let cutoff = &got["parts"][2]["children"][0];
    assert_eq!(
        (cutoff["surface"].as_str(), cutoff["level"].as_u64()),
        (Some("filter-cutoff"), Some(2))
    );
    assert!(d.join(cutoff["thumb"].as_str().unwrap()).is_file());
    assert_eq!(got["explode"].as_array().unwrap().len(), 2);
    let surfaces = got["surfaces"].as_array().unwrap();
    assert!(
        surfaces.iter().any(|s| s["id"] == "out-level"),
        "{surfaces:?}"
    );
    assert!(
        d.join(".cut-cache")
            .join(format!("{}.json", got["state"].as_str().unwrap()))
            .is_file()
    );
    // Not a plugin: a tool error.
    let (c, err) = m.call("plugin_parts", serde_json::json!({"layer": "caption"}));
    assert!(err && c[0]["text"].as_str().unwrap().contains("not a plugin"));
}

/// An agent writes a melody into a plugin layer, reads back the patch the
/// plugin reports, and renders the sound to listen to.
#[test]
fn mcp_plays_notes_into_a_plugin_and_reads_its_patch() {
    let synth = Path::new(BIN).parent().unwrap().join("examples/synth");
    assert!(synth.is_file(), "run the whole `cargo test -p mui-cut`");
    let d = scratch("mcp-notes");
    let project = d.join("p.cut.json");
    let p = serde_json::json!({"size": [320, 180], "fps": 30, "scenes": [{"name": "s", "duration": 1.0,
        "layers": [{"id": "syn", "kind": "plugin", "source": {"bin": synth},
                    "params": [{"id": "out", "field": "level", "value": 0.5}]}]}]});
    std::fs::write(&project, p.to_string()).unwrap();
    let mut m = Mcp::start();
    m.text("open", serde_json::json!({"path": project}));
    m.text(
        "notes_set",
        serde_json::json!({"layer": "syn", "notes": [{"t": 0.5, "dur": 0.2, "pitch": 64}]}),
    );
    m.text("notes_add", serde_json::json!({"layer": "syn", "notes": [{"t": 0.1, "dur": 0.2, "pitch": 60, "vel": 90}]}));
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&project).unwrap()).unwrap();
    let notes = &saved["scenes"][0]["layers"][0]["notes"];
    assert_eq!(notes[0]["pitch"], 60, "kept in time order: {notes}");
    assert_eq!(notes[1]["pitch"], 64);
    let patch: serde_json::Value =
        serde_json::from_str(&m.text("patch_get", serde_json::json!({"layer": "syn", "t": 0.2})))
            .unwrap();
    assert_eq!(patch["plugin"], "MUI Synth");
    let level = patch["params"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "out.level")
        .unwrap();
    assert_eq!(level["value"], 0.5);
    let wav = d.join("slice.wav");
    let said = m.text(
        "plugin_play",
        serde_json::json!({"from": 0.05, "to": 0.45, "out": wav}),
    );
    assert!(said.contains("0.40 s"), "{said}");
    // A float WAV: 44-byte header, then 0.4 s of stereo f32 at 48 kHz.
    let bytes = std::fs::read(&wav).unwrap();
    assert_eq!(bytes.len(), 44 + 19_200 * 8);
    assert!(
        bytes[44..]
            .chunks(4)
            .any(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]).abs() > 0.01)
    );
}
