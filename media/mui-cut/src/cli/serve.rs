//! `mui-cut serve`: the web editor's only backend. Serves the built app,
//! keeps the project at a revision and merges edits into it, and pushes
//! every new revision to open editors over SSE.
//!
//! Edits are field-level: JSON Pointer operations made against a base
//! revision (`POST /patch`). The server applies them to the newest
//! revision, so a person's and an agent's edits to different fields both
//! land; where both changed the same field the later edit wins and the
//! reply names it. The editor, `mui-cut mcp` and outside writes to the
//! file all go through it.
//!
//! std only, a thread a connection: one person and a few agents on
//! localhost, not a web server.
use std::collections::VecDeque;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::Project;
use serde_json::{Value, json};

use crate::cli::{Result, write_atomic};

/// How often the project file is re-read for outside edits.
const POLL: Duration = Duration::from_millis(250);

struct Shared {
    project: PathBuf,
    web: PathBuf,
    /// The project as this server last read or wrote it.
    doc: Mutex<Doc>,
    listeners: Mutex<Vec<TcpStream>>,
    /// The open editor's last reported state (scene, playhead, selection)
    /// and when it came: what `mui-cut mcp` shows an agent.
    state: Mutex<(String, Option<std::time::Instant>)>,
    /// The project text whose plugin states were last captured.
    captured: Mutex<String>,
    /// The capture running (`mui-cut capture --json`): its stdin, which
    /// takes the editor's state, for the states nearest the playhead.
    capturing: Mutex<Option<std::process::ChildStdin>>,
    /// The sound, played as the editor plays (`/transport`).
    live: std::sync::OnceLock<Arc<crate::cli::live::Live>>,
}

pub fn serve(project: &Path, port: u16, web: PathBuf) -> Result<()> {
    let known =
        std::fs::read_to_string(project).map_err(|e| format!("{}: {e}", project.display()))?;
    let rate = Project::load(&known)
        .map_err(|e| format!("{}: {e}", project.display()))?
        .sample_rate;
    if !web.join("pkg").exists() {
        eprintln!(
            "mui-cut: {} has no pkg/; run media/mui-cut/web/build.sh first",
            web.display()
        );
    }
    let shared = Arc::new(Shared {
        project: project.to_owned(),
        web,
        doc: Mutex::new(Doc::new(known)),
        listeners: Mutex::new(Vec::new()),
        state: Mutex::new(("null".into(), None)),
        captured: Mutex::new(String::new()),
        capturing: Mutex::new(None),
        live: std::sync::OnceLock::new(),
    });
    let weak = Arc::downgrade(&shared);
    // ponytail: the device runs at the rate the project had when serve
    // started; restart serve after changing `sample_rate`.
    let _ = shared
        .live
        .set(crate::cli::live::Live::new(project, rate, move |msg| {
            if let Some(s) = weak.upgrade() {
                s.broadcast(format!("event: live\ndata: {msg}\n\n").as_bytes());
            }
        }));
    let listener =
        TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("port {port}: {e}"))?;
    let port = listener.local_addr().map_or(port, |a| a.port());
    let _found = Discovery::write(project, port)?;
    println!(
        "mui-cut: editing {} at http://127.0.0.1:{port}/",
        project.display()
    );
    let watch = shared.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(POLL);
            watch.poll();
        }
    });
    // Captures on their own thread: a long one holds up nothing else.
    let watch = shared.clone();
    std::thread::spawn(move || {
        loop {
            watch.capture();
            std::thread::sleep(POLL);
        }
    });
    for stream in listener.incoming().flatten() {
        let shared = shared.clone();
        std::thread::spawn(move || {
            // A browser dropping a connection it no longer wants is not news.
            if let Err(e) = shared.handle(&stream)
                && !e.contains("Broken pipe")
                && !e.contains("Connection reset")
            {
                eprintln!("mui-cut: {e}");
            }
        });
    }
    Ok(())
}

/// The file beside the project that tells `mui-cut mcp` which port this
/// editor listens on: `.NAME.serve` next to `NAME`.
pub fn discovery_path(project: &Path) -> PathBuf {
    let name = project.file_name().unwrap_or_default().to_string_lossy();
    project.with_file_name(format!(".{name}.serve"))
}

/// What the discovery file says: the editor's port and process.
#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq)]
pub struct Found {
    pub port: u16,
    pub pid: u32,
}

/// The discovery file, written while serving and removed on the way out:
/// on drop, and on Ctrl+C or SIGTERM.
struct Discovery(PathBuf);

impl Discovery {
    fn write(project: &Path, port: u16) -> Result<Self> {
        let path = discovery_path(project);
        let found = Found {
            port,
            pid: std::process::id(),
        };
        let text = serde_json::to_string(&found).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        let gone = path.clone();
        // Only the first handler takes: one `serve` a process.
        let _ = ctrlc::set_handler(move || {
            let _ = std::fs::remove_file(&gone);
            std::process::exit(130);
        });
        Ok(Self(path))
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The editor serving `project`, from its discovery file, if one says so.
pub fn discover(project: &Path) -> Option<Found> {
    let text = std::fs::read_to_string(discovery_path(project)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Revisions merges look back over: an edit made against an older one
/// still applies, it just cannot tell what it overwrote.
const HISTORY: usize = 64;

/// The project at its newest revision, and the revisions before it.
struct Doc {
    rev: u64,
    /// The file's text as last read or written: a change from this is an
    /// outside edit.
    text: String,
    history: VecDeque<(u64, Value)>,
}

impl Doc {
    fn new(text: String) -> Self {
        let value = serde_json::from_str(&text).unwrap_or(Value::Null);
        Self {
            rev: 1,
            text,
            history: VecDeque::from([(1, value)]),
        }
    }
    fn value(&self) -> &Value {
        &self.history.back().expect("never empty").1
    }
    fn push(&mut self, text: String, value: Value) {
        self.rev += 1;
        self.text = text;
        self.history.push_back((self.rev, value));
        if self.history.len() > HISTORY {
            self.history.pop_front();
        }
    }
    /// The SSE message for the newest revision.
    fn event(&self, by: &str, conflicts: &[String]) -> String {
        let msg = json!({ "rev": self.rev, "by": by, "conflicts": conflicts, "doc": self.value() });
        format!("event: doc\ndata: {msg}\n\n")
    }
}

/// Item keys for an array whose items all carry a distinct `id` (layers,
/// sources) or `name` (scenes, variants), usable as pointer tokens.
fn item_keys(items: &[Value]) -> Option<Vec<&str>> {
    let field = ["id", "name"]
        .into_iter()
        .find(|f| items.iter().all(|v| v[*f].is_string()))?;
    let keys: Vec<&str> = items
        .iter()
        .map(|v| v[field].as_str().unwrap_or(""))
        .collect();
    let numeric = |k: &&str| *k == "-" || k.parse::<usize>().is_ok();
    let mut seen = std::collections::HashSet::new();
    keys.iter()
        .all(|k| !numeric(k) && seen.insert(*k))
        .then_some(keys)
}

fn token(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

/// The operations that turn `a` into `b`, field by field: objects key by
/// key; arrays of layers, scenes and sources item by item, addressed by
/// id or name (so they still land after a reorder elsewhere); any other
/// change replaces the value.
pub fn diff(a: &Value, b: &Value) -> Vec<Value> {
    fn walk(a: &Value, b: &Value, path: &str, out: &mut Vec<Value>) {
        if a == b {
            return;
        }
        match (a, b) {
            (Value::Object(x), Value::Object(y)) => {
                for (k, va) in x {
                    let p = format!("{path}/{}", token(k));
                    match y.get(k) {
                        Some(vb) => walk(va, vb, &p, out),
                        None => out.push(json!({ "op": "remove", "path": p })),
                    }
                }
                for (k, vb) in y.iter().filter(|(k, _)| !x.contains_key(*k)) {
                    out.push(
                        json!({ "op": "add", "path": format!("{path}/{}", token(k)), "value": vb }),
                    );
                }
            }
            (Value::Array(x), Value::Array(y)) => {
                if let (Some(kx), Some(ky)) = (item_keys(x), item_keys(y)) {
                    let both: Vec<&str> = kx.iter().copied().filter(|k| ky.contains(k)).collect();
                    let order: Vec<&str> = ky.iter().copied().filter(|k| kx.contains(k)).collect();
                    if both == order {
                        for k in kx.iter().filter(|k| !ky.contains(k)) {
                            out.push(
                                json!({ "op": "remove", "path": format!("{path}/{}", token(k)) }),
                            );
                        }
                        for (i, k) in kx.iter().enumerate() {
                            if let Some(j) = ky.iter().position(|o| o == k) {
                                walk(&x[i], &y[j], &format!("{path}/{}", token(k)), out);
                            }
                        }
                        for (j, _) in ky.iter().enumerate().filter(|(_, k)| !kx.contains(k)) {
                            out.push(json!({ "op": "add", "path": format!("{path}/{j}"), "value": y[j] }));
                        }
                        return;
                    }
                }
                out.push(json!({ "op": "replace", "path": path, "value": b }));
            }
            _ => out.push(json!({ "op": "replace", "path": path, "value": b })),
        }
    }
    let mut out = Vec::new();
    walk(a, b, "", &mut out);
    out
}

/// Whether two pointers touch: one is the other or inside it.
fn touch(a: &str, b: &str) -> bool {
    let inside = |a: &str, b: &str| a == b || a.starts_with(&format!("{b}/")) || b.is_empty();
    inside(a, b) || inside(b, a)
}

/// `ops`, made against `base`, applied to `current`: the merged document,
/// the paths where they overwrote a change made since `base` (the later
/// edit, these ops, wins), and the ops that no longer apply (their layer
/// is gone, say) with why. Without `base` (too old) nothing counts as a
/// conflict.
pub fn merge(
    base: Option<&Value>,
    current: &Value,
    ops: &[Value],
) -> (Value, Vec<String>, Vec<String>) {
    let theirs: Vec<String> = base
        .map(|b| diff(b, current))
        .unwrap_or_default()
        .iter()
        .filter_map(|o| o["path"].as_str().map(str::to_owned))
        .collect();
    let mut out = current.clone();
    let (mut conflicts, mut skipped) = (Vec::new(), Vec::new());
    for op in ops {
        let path = op["path"].as_str().unwrap_or("");
        match crate::cli::mcp::apply(&mut out, op) {
            Ok(()) => {
                if theirs.iter().any(|t| touch(t, path)) && !conflicts.iter().any(|c| c == path) {
                    conflicts.push(path.to_owned());
                }
            }
            Err(e) => skipped.push(format!("{path}: {e}")),
        }
    }
    (out, conflicts, skipped)
}

fn respond(mut s: &TcpStream, status: &str, kind: &str, body: &[u8]) -> std::io::Result<()> {
    write!(
        s,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    s.write_all(body)
}

fn kind(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// `rel` under `root`, refusing anything that climbs out of it.
fn under(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel);
    rel.components()
        .all(|c| matches!(c, Component::Normal(_)))
        .then(|| root.join(rel))
}

impl Shared {
    /// An outside write to the file (a text editor, an agent without the
    /// server) becomes the next revision. One that is not JSON is only
    /// announced, for the editor to show the error.
    fn poll(&self) {
        let Ok(now) = std::fs::read_to_string(&self.project) else {
            return;
        };
        let mut d = self.doc.lock().expect("no panic holds it");
        if d.text == now {
            return;
        }
        if let Ok(v) = serde_json::from_str::<Value>(&now) {
            d.push(now, v);
            let msg = d.event("disk", &[]);
            self.broadcast(msg.as_bytes());
        } else {
            d.text = now;
            self.broadcast(b"data: changed\n\n");
        }
    }

    /// Merge `ops` made against revision `base` into the newest, validate,
    /// write it and tell every editor. The reply: `{rev, doc, conflicts,
    /// skipped}`.
    fn commit(&self, base: u64, ops: &[Value], by: &str) -> Result<Value> {
        let mut d = self.doc.lock().expect("no panic holds it");
        let then = d.history.iter().find(|(r, _)| *r == base).map(|(_, v)| v);
        let (merged, conflicts, skipped) = merge(then, d.value(), ops);
        let p = Project::load(&merged.to_string())?;
        let json = p.to_json();
        if json != d.text {
            let value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            write_atomic(&self.project, &json)?;
            d.push(json, value);
            let msg = d.event(by, &conflicts);
            self.broadcast(msg.as_bytes());
        }
        Ok(json!({ "rev": d.rev, "doc": d.value(), "conflicts": conflicts, "skipped": skipped }))
    }

    /// Capture the plugin states the project as last read or saved shows
    /// and the cache lacks: `mui-cut capture --json`, which makes them in
    /// the plugin's adapter where it can ([`crate::cli::host::capture_all`]),
    /// the states nearest the editor's playhead first. Each batch written
    /// goes to the editors as it lands (`plugin`, the manifests' paths).
    fn capture(&self) {
        let text = self.doc.lock().expect("no panic holds it").text.clone();
        {
            let mut done = self.captured.lock().expect("no panic holds it");
            if *done == text {
                return;
            }
            done.clone_from(&text);
        }
        let Ok(p) = Project::load(&text) else {
            return;
        };
        let all = p.all_sources();
        if !all.iter().any(|m| m.state().is_some()) {
            return;
        }
        let child = std::env::current_exe().and_then(|exe| {
            std::process::Command::new(exe)
                .arg("capture")
                .arg(&self.project)
                .arg("--json")
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
        });
        let mut child = match child {
            Ok(c) => c,
            Err(e) => return eprintln!("mui-cut: capture: {e}"),
        };
        let state = self.state.lock().expect("no panic holds it").0.clone();
        let mut stdin = child.stdin.take();
        if let Some(w) = &mut stdin {
            let _ = writeln!(w, "{state}");
        }
        *self.capturing.lock().expect("no panic holds it") = stdin;
        let out = child.stdout.take().map(BufReader::new);
        // Only its JSON lines: whatever else a plugin prints is not news.
        for line in out
            .into_iter()
            .flat_map(std::io::BufRead::lines)
            .map_while(std::result::Result::ok)
        {
            if line.starts_with('[') {
                self.broadcast(format!("event: plugin\ndata: {line}\n\n").as_bytes());
            }
        }
        *self.capturing.lock().expect("no panic holds it") = None;
        let _ = child.wait();
    }

    /// The plugin states the project shows that the cache has, as the
    /// paths the editor fetches, nearest its playhead first.
    fn captures(&self) -> Vec<String> {
        let dir = self.project.parent().unwrap_or(Path::new("."));
        let text = self.doc.lock().expect("no panic holds it").text.clone();
        let state = self.state.lock().expect("no panic holds it").0.clone();
        let at = serde_json::from_str(&state)
            .ok()
            .and_then(|v| crate::cli::host::playhead(&v));
        Project::load(&text)
            .map(|p| crate::cli::host::by_playhead(&p, at))
            .unwrap_or_default()
            .into_iter()
            .filter(|path| dir.join(path).is_file())
            .collect()
    }

    /// One SSE message to every open editor, dropping the gone ones.
    fn broadcast(&self, msg: &[u8]) {
        let mut ls = self.listeners.lock().expect("no panic holds it");
        ls.retain_mut(|s| s.write_all(msg).and_then(|()| s.flush()).is_ok());
    }

    fn handle(&self, stream: &TcpStream) -> Result<()> {
        let mut r = BufReader::new(stream);
        let mut line = String::new();
        r.read_line(&mut line).map_err(|e| e.to_string())?;
        let mut parts = line.split_whitespace();
        let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
        let path = path.split('?').next().unwrap_or("/").to_owned();
        let mut length = 0usize;
        loop {
            let mut h = String::new();
            if r.read_line(&mut h).map_err(|e| e.to_string())? == 0 || h.trim().is_empty() {
                break;
            }
            if let Some((k, v)) = h.split_once(':')
                && k.eq_ignore_ascii_case("content-length")
            {
                length = v.trim().parse().unwrap_or(0);
            }
        }
        let io = |e: std::io::Error| e.to_string();
        match (method, path.as_str()) {
            ("GET", "/project") => {
                let text = std::fs::read(&self.project).map_err(io)?;
                respond(stream, "200 OK", "application/json", &text).map_err(io)
            }
            ("PUT", "/project") => {
                if length > 16 << 20 {
                    return respond(stream, "413 Payload Too Large", "text/plain", b"too large")
                        .map_err(io);
                }
                let mut body = vec![0; length];
                r.read_exact(&mut body).map_err(io)?;
                let text = String::from_utf8_lossy(&body);
                // A whole document: the difference from the newest
                // revision, merged like any patch.
                let saved = Project::load(&text).and_then(|p| {
                    let new: Value =
                        serde_json::from_str(&p.to_json()).map_err(|e| e.to_string())?;
                    let (rev, ops) = {
                        let d = self.doc.lock().expect("no panic holds it");
                        (d.rev, diff(d.value(), &new))
                    };
                    self.commit(rev, &ops, "put")
                });
                match saved {
                    Ok(_) => {
                        let json = self.doc.lock().expect("no panic holds it").text.clone();
                        respond(stream, "200 OK", "application/json", json.as_bytes()).map_err(io)
                    }
                    Err(e) => {
                        respond(stream, "400 Bad Request", "text/plain", e.as_bytes()).map_err(io)
                    }
                }
            }
            // The project at its newest revision.
            ("GET", "/doc") => {
                let body = {
                    let d = self.doc.lock().expect("no panic holds it");
                    json!({ "rev": d.rev, "project": self.project.to_string_lossy(), "doc": d.value() })
                };
                respond(
                    stream,
                    "200 OK",
                    "application/json",
                    body.to_string().as_bytes(),
                )
                .map_err(io)
            }
            // A field-level edit: `{base, ops, by}`, merged.
            ("POST", "/patch") => {
                if length > 16 << 20 {
                    return respond(stream, "413 Payload Too Large", "text/plain", b"too large")
                        .map_err(io);
                }
                let mut body = vec![0; length];
                r.read_exact(&mut body).map_err(io)?;
                let v: Value = serde_json::from_slice(&body).unwrap_or_default();
                let (Some(base), Some(ops)) = (v["base"].as_u64(), v["ops"].as_array()) else {
                    return respond(stream, "400 Bad Request", "text/plain", b"want {base, ops}")
                        .map_err(io);
                };
                match self.commit(base, ops, v["by"].as_str().unwrap_or("")) {
                    Ok(reply) => respond(
                        stream,
                        "200 OK",
                        "application/json",
                        reply.to_string().as_bytes(),
                    ),
                    Err(e) => respond(stream, "400 Bad Request", "text/plain", e.as_bytes()),
                }
                .map_err(io)
            }
            // The editor reports its state; an agent reads it.
            ("PUT", "/state") | ("POST", "/control") => {
                if length > 64 << 10 {
                    return respond(stream, "413 Payload Too Large", "text/plain", b"too large")
                        .map_err(io);
                }
                let mut body = vec![0; length];
                r.read_exact(&mut body).map_err(io)?;
                let Ok(v) = serde_json::from_slice::<serde_json::Value>(&body) else {
                    return respond(stream, "400 Bad Request", "text/plain", b"not JSON")
                        .map_err(io);
                };
                // Compact, so it is one SSE data line.
                let one = v.to_string();
                if method == "PUT" {
                    if let Some(w) = &mut *self.capturing.lock().expect("no panic holds it") {
                        let _ = writeln!(w, "{one}");
                    }
                    *self.state.lock().expect("no panic holds it") =
                        (one, Some(std::time::Instant::now()));
                } else {
                    self.broadcast(format!("event: control\ndata: {one}\n\n").as_bytes());
                    let editors = self.listeners.lock().expect("no panic holds it").len();
                    let reply = format!("{{\"editors\": {editors}}}");
                    return respond(stream, "200 OK", "application/json", reply.as_bytes())
                        .map_err(io);
                }
                respond(stream, "200 OK", "application/json", b"{}").map_err(io)
            }
            // The Add plugin dialog: a plugin folder (relative to the
            // project) or git URL, detected; the editor adds the entry.
            ("POST", "/plugin") => {
                if length > 64 << 10 {
                    return respond(stream, "413 Payload Too Large", "text/plain", b"too large")
                        .map_err(io);
                }
                let mut body = vec![0; length];
                r.read_exact(&mut body).map_err(io)?;
                let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
                let dir = self.project.parent().unwrap_or(Path::new("."));
                let taken: Vec<String> = std::fs::read_to_string(&self.project)
                    .ok()
                    .and_then(|t| Project::load(&t).ok())
                    .map(|p| p.all_sources().into_iter().map(|m| m.id).collect())
                    .unwrap_or_default();
                let taken: Vec<&str> = taken.iter().map(String::as_str).collect();
                let from = v["from"].as_str().unwrap_or("").trim();
                let id = v["id"].as_str().map(str::trim).filter(|s| !s.is_empty());
                match crate::cli::build::onboard(from, dir, dir, id, &taken) {
                    Ok((entry, report)) => {
                        let body = serde_json::json!({"entry": entry, "report": report});
                        respond(
                            stream,
                            "200 OK",
                            "application/json",
                            body.to_string().as_bytes(),
                        )
                        .map_err(io)
                    }
                    Err(e) => {
                        respond(stream, "400 Bad Request", "text/plain", e.as_bytes()).map_err(io)
                    }
                }
            }
            // The live transport: play, pause, seek, and where it is.
            ("POST", "/transport" | "/live/note") => {
                if length > 64 << 10 {
                    return respond(stream, "413 Payload Too Large", "text/plain", b"too large")
                        .map_err(io);
                }
                let mut body = vec![0; length];
                r.read_exact(&mut body).map_err(io)?;
                let Ok(v) = serde_json::from_slice::<serde_json::Value>(&body) else {
                    return respond(stream, "400 Bad Request", "text/plain", b"not JSON")
                        .map_err(io);
                };
                let live = self.live.get().ok_or("no transport")?;
                let reply = if path == "/transport" {
                    Ok(live.control(&v))
                } else {
                    live.note(&v).map(|()| live.report())
                };
                match reply {
                    Ok(v) => respond(
                        stream,
                        "200 OK",
                        "application/json",
                        v.to_string().as_bytes(),
                    ),
                    Err(e) => respond(stream, "400 Bad Request", "text/plain", e.as_bytes()),
                }
                .map_err(io)
            }
            ("GET", "/transport") => {
                let v = self.live.get().ok_or("no transport")?.report();
                respond(
                    stream,
                    "200 OK",
                    "application/json",
                    v.to_string().as_bytes(),
                )
                .map_err(io)
            }
            ("GET", "/state") => {
                let (state, at) = self.state.lock().expect("no panic holds it").clone();
                let age = at.map_or_else(|| "null".into(), |a| a.elapsed().as_millis().to_string());
                let editors = self.listeners.lock().expect("no panic holds it").len();
                let body = format!(
                    "{{\"project\": {}, \"editors\": {editors}, \"age_ms\": {age}, \"state\": {state}}}",
                    serde_json::Value::from(self.project.to_string_lossy()),
                );
                respond(stream, "200 OK", "application/json", body.as_bytes()).map_err(io)
            }
            // The soundtrack a browser export muxes in: every scene's mix
            // (plugin sound captured first) as a WAV; 204 when none sounds.
            ("GET", "/mix.wav") => {
                let text = std::fs::read_to_string(&self.project).map_err(io)?;
                let p = Project::load(&text)?;
                for e in crate::cli::host::capture_missing(&p, &self.project) {
                    eprintln!("mui-cut: {e}");
                }
                let scenes: Vec<_> = p.scenes.iter().collect();
                match crate::cli::audio::mix(&p, &self.project, &scenes) {
                    Ok(Some(pcm)) => respond(
                        stream,
                        "200 OK",
                        "audio/wav",
                        &crate::cli::audio::wav(&pcm, p.sample_rate),
                    ),
                    Ok(None) => respond(stream, "204 No Content", "text/plain", b""),
                    Err(e) => respond(
                        stream,
                        "500 Internal Server Error",
                        "text/plain",
                        e.as_bytes(),
                    ),
                }
                .map_err(io)
            }
            // The plugin states the editor can fetch now; `plugin` events
            // name the ones written after.
            ("GET", "/captures") => {
                let body = json!(self.captures()).to_string();
                respond(stream, "200 OK", "application/json", body.as_bytes()).map_err(io)
            }
            ("GET", "/name") => {
                let name = self
                    .project
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                respond(
                    stream,
                    "200 OK",
                    "text/plain; charset=utf-8",
                    name.as_bytes(),
                )
                .map_err(io)
            }
            ("GET", "/events") => {
                let mut s = stream.try_clone().map_err(io)?;
                s.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\n\r\n: hello\n\n",
                )
                .map_err(io)?;
                self.listeners.lock().expect("no panic holds it").push(s);
                Ok(())
            }
            // An import: a file dropped on the editor, written beside the
            // project (never over it).
            ("PUT", p) if p.starts_with("/asset/") => {
                if length > 256 << 20 {
                    return respond(stream, "413 Payload Too Large", "text/plain", b"too large")
                        .map_err(io);
                }
                let dir = self.project.parent().unwrap_or(Path::new("."));
                let rel = &p["/asset/".len()..];
                let Some(file) = under(dir, rel).filter(|f| {
                    f.file_name() != self.project.file_name() || f.parent() != Some(dir)
                }) else {
                    return respond(stream, "400 Bad Request", "text/plain", b"bad path")
                        .map_err(io);
                };
                let mut body = vec![0; length];
                r.read_exact(&mut body).map_err(io)?;
                if let Some(d) = file.parent() {
                    std::fs::create_dir_all(d).map_err(io)?;
                }
                std::fs::write(&file, &body).map_err(|e| format!("{}: {e}", file.display()))?;
                respond(stream, "200 OK", "text/plain", rel.as_bytes()).map_err(io)
            }
            ("GET", p) => {
                let (root, rel) = match p.strip_prefix("/asset/") {
                    Some(rel) => (self.project.parent().unwrap_or(Path::new(".")), rel),
                    None => (self.web.as_path(), p.trim_start_matches('/')),
                };
                let rel = if rel.is_empty() { "index.html" } else { rel };
                match under(root, rel).and_then(|f| Some((std::fs::read(&f).ok()?, f))) {
                    Some((bytes, f)) => respond(stream, "200 OK", kind(&f), &bytes).map_err(io),
                    None => {
                        respond(stream, "404 Not Found", "text/plain", b"not found").map_err(io)
                    }
                }
            }
            _ => respond(stream, "405 Method Not Allowed", "text/plain", b"no").map_err(io),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Value {
        json!({"scenes": [{"name": "s", "layers": [
            {"id": "card", "x": 1, "y": 2, "fill": "#000000"},
            {"id": "bar", "name": "card2", "x": 5, "fill": "#111111"}
        ]}]})
    }

    #[test]
    fn diff_addresses_layers_by_id_and_round_trips() {
        let a = base();
        let mut b = a.clone();
        b["scenes"][0]["layers"][1]["x"] = json!(9);
        b["scenes"][0]["layers"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "new"}));
        let ops = diff(&a, &b);
        assert_eq!(
            ops,
            vec![
                json!({"op": "replace", "path": "/scenes/s/layers/bar/x", "value": 9}),
                json!({"op": "add", "path": "/scenes/s/layers/2", "value": {"id": "new"}}),
            ]
        );
        let mut c = a.clone();
        for op in &ops {
            crate::cli::mcp::apply(&mut c, op).unwrap();
        }
        assert_eq!(c, b);
    }

    #[test]
    fn disjoint_edits_both_land_and_the_same_field_is_a_conflict() {
        let b = base();
        // The agent recolours `bar` and puts a layer at the bottom...
        let mut agent = b.clone();
        agent["scenes"][0]["layers"][1]["fill"] = json!("#00ff00");
        agent["scenes"][0]["layers"]
            .as_array_mut()
            .unwrap()
            .insert(0, json!({"id": "under"}));
        // ...while the person, from the older revision, drags `card` and
        // also recolours `bar`.
        let mut person = b.clone();
        person["scenes"][0]["layers"][0]["x"] = json!(100);
        let (m, conflicts, skipped) = merge(Some(&b), &agent, &diff(&b, &person));
        assert_eq!(m["scenes"][0]["layers"][1]["x"], 100, "{m}");
        assert_eq!(m["scenes"][0]["layers"][2]["fill"], "#00ff00", "{m}");
        assert!(conflicts.is_empty() && skipped.is_empty());

        person["scenes"][0]["layers"][1]["fill"] = json!("#ff0000");
        let (m, conflicts, _) = merge(Some(&b), &agent, &diff(&b, &person));
        assert_eq!(
            m["scenes"][0]["layers"][2]["fill"], "#ff0000",
            "the later edit wins"
        );
        assert_eq!(conflicts, ["/scenes/s/layers/bar/fill"]);
    }

    #[test]
    fn an_edit_to_a_layer_since_deleted_is_skipped() {
        let b = base();
        let mut agent = b.clone();
        agent["scenes"][0]["layers"]
            .as_array_mut()
            .unwrap()
            .remove(0);
        let mut person = b.clone();
        person["scenes"][0]["layers"][0]["y"] = json!(50);
        let (m, _, skipped) = merge(Some(&b), &agent, &diff(&b, &person));
        assert_eq!(m, agent);
        assert_eq!(skipped.len(), 1, "{skipped:?}");
    }
}
