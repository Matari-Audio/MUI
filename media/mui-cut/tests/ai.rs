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
        self.send(
            &serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        );
        let mut line = String::new();
        self.out.read_line(&mut line).unwrap();
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["id"], id, "{v}");
        v
    }
    /// A tool's content, and whether it was an error.
    fn call(&mut self, name: &str, args: serde_json::Value) -> (Vec<serde_json::Value>, bool) {
        let v = self.request(
            "tools/call",
            serde_json::json!({"name": name, "arguments": args}),
        );
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

    // An agent's edit reaches the editor as a reload.
    m.text(
        "set",
        serde_json::json!({"scene": "title", "layer": "bar", "prop": "y", "value": 450}),
    );
    loop {
        l.clear();
        lines.read_line(&mut l).unwrap();
        if l.starts_with("data: changed") {
            break;
        }
    }
    let _ = server.kill();
    let _ = server.wait();
}
