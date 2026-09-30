//! `mui-cut serve`: the web editor's only backend. Serves the built app,
//! GETs and PUTs the project file, and tells open editors over SSE when the
//! file changed on disk, so an agent's edit shows up live.
//!
//! std only, a thread a connection: one person and a few agents on
//! localhost, not a web server.
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mui_cut::Project;

use crate::{Result, write_atomic};

/// How often the project file is re-read for outside edits.
const POLL: Duration = Duration::from_millis(250);

struct Shared {
    project: PathBuf,
    web: PathBuf,
    /// The file's contents as this server last read or wrote them: a change
    /// from this is somebody else's edit.
    known: Mutex<String>,
    listeners: Mutex<Vec<TcpStream>>,
    /// The open editor's last reported state (scene, playhead, selection)
    /// and when it came: what `mui-cut mcp` shows an agent.
    state: Mutex<(String, Option<std::time::Instant>)>,
    /// The project text whose plugin states were last captured.
    captured: Mutex<String>,
    /// The sound, played as the editor plays (`/transport`).
    live: std::sync::OnceLock<Arc<crate::live::Live>>,
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
        known: Mutex::new(known),
        listeners: Mutex::new(Vec::new()),
        state: Mutex::new(("null".into(), None)),
        captured: Mutex::new(String::new()),
        live: std::sync::OnceLock::new(),
    });
    let weak = Arc::downgrade(&shared);
    // ponytail: the device runs at the rate the project had when serve
    // started; restart serve after changing `sample_rate`.
    let _ = shared
        .live
        .set(crate::live::Live::new(project, rate, move |msg| {
            if let Some(s) = weak.upgrade() {
                s.broadcast(format!("event: live\ndata: {msg}\n\n").as_bytes());
            }
        }));
    let listener =
        TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("port {port}: {e}"))?;
    println!(
        "mui-cut: editing {} at http://127.0.0.1:{port}/",
        project.display()
    );
    let watch = shared.clone();
    std::thread::spawn(move || {
        loop {
            watch.capture();
            std::thread::sleep(POLL);
            watch.poll();
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
    fn poll(&self) {
        let Ok(now) = std::fs::read_to_string(&self.project) else {
            return;
        };
        {
            let mut known = self.known.lock().expect("no panic holds it");
            if *known == now {
                return;
            }
            *known = now;
        }
        self.broadcast(b"data: changed\n\n");
    }

    /// Capture the plugin states the project as last read or saved shows
    /// and the cache lacks (on the watcher's thread: a capture holds up
    /// noticing outside edits, not the editor), then tell the editors.
    fn capture(&self) {
        let text = self.known.lock().expect("no panic holds it").clone();
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
        let mut errs = crate::host::capture_missing(&p, &self.project);
        errs.extend(crate::host::capture_sources(&p, &self.project));
        for e in errs {
            eprintln!("mui-cut: {e}");
        }
        self.broadcast(b"event: plugin\ndata: ready\n\n");
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
                match Project::load(&text) {
                    Ok(p) => {
                        let json = p.to_json();
                        // Known first: the watcher must not echo our own save.
                        *self.known.lock().expect("no panic holds it") = json.clone();
                        write_atomic(&self.project, &json)?;
                        respond(stream, "200 OK", "application/json", json.as_bytes()).map_err(io)
                    }
                    Err(e) => {
                        respond(stream, "400 Bad Request", "text/plain", e.as_bytes()).map_err(io)
                    }
                }
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
                match crate::build::onboard(from, dir, dir, id, &taken) {
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
                for e in crate::host::capture_missing(&p, &self.project) {
                    eprintln!("mui-cut: {e}");
                }
                let scenes: Vec<_> = p.scenes.iter().collect();
                match crate::audio::mix(&p, &self.project, &scenes) {
                    Ok(Some(pcm)) => respond(
                        stream,
                        "200 OK",
                        "audio/wav",
                        &crate::audio::wav(&pcm, p.sample_rate),
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
