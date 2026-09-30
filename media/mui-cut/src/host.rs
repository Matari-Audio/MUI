//! Plugin layers' captures, native side: build or find each plugin's
//! mui-motion-bridge adapter, replay the layer's commands on the frame grid
//! (`Layer::plugin_track`), and write every state the project shows that
//! the cache lacks. Rendering then only reads the cache.
//!
//! The adapter speaks the bridge's wire (tools/film/README.md): newline
//! JSON in; out, a kind byte (`J` JSON, `A` audio), a little-endian u32
//! length and the payload. Scene packets carry only textures that changed,
//! so a session keeps every texture it has seen.
use std::collections::{HashMap, HashSet};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use base64::Engine as _;
use mui_cut::plugin::{CACHE, Capture, FNV_OFFSET, Fragment, fnv, frame_at};
use mui_cut::{Assets, Kind, Layer, Project, Source};
use serde_json::{Value, json};

use crate::Result;

/// How long one capture may take before the adapter counts as hung.
const PATIENCE: Duration = Duration::from_secs(60);

/// Every plugin layer's steps in every scene: `(layer, steps)`.
fn plugins(p: &Project) -> Vec<(&Layer, Vec<mui_cut::Step>)> {
    p.scenes
        .iter()
        .flat_map(|s| {
            s.layers
                .iter()
                .filter(|l| matches!(l.kind, Kind::Plugin { .. }))
                .map(|l| (l, l.plugin_track(p.fps, frame_at(s.duration, p.fps))))
        })
        .collect()
}

fn source(l: &Layer) -> &Source {
    match &l.kind {
        Kind::Plugin { source, .. } => source,
        _ => unreachable!("plugins() keeps plugin layers"),
    }
}

/// Capture what is missing, then add every capture the project shows (and
/// its images) to `assets`. What failed, as messages.
pub fn load(p: &Project, project: &Path, assets: &mut Assets) -> Vec<String> {
    let dir = project.parent().unwrap_or(Path::new("."));
    let mut errs = capture_missing(p, project);
    let mut seen = HashSet::new();
    for (_, steps) in plugins(p) {
        for s in steps {
            let rel = format!("{CACHE}/{}.json", s.key);
            if !seen.insert(rel.clone()) {
                continue;
            }
            let Ok(bytes) = std::fs::read(dir.join(&rel)) else {
                continue;
            };
            if let Err(e) = assets.add_asset(&rel, &bytes) {
                errs.push(e);
                continue;
            }
            let cap: Capture = serde_json::from_slice(&bytes).expect("add_asset parsed it");
            for f in cap.fragments {
                let rel = format!("{CACHE}/{}", f.src);
                if seen.insert(rel.clone()) {
                    let added = std::fs::read(dir.join(&rel))
                        .map_err(|e| format!("{rel}: {e}"))
                        .and_then(|b| assets.add_asset(&rel, &b));
                    errs.extend(added.err());
                }
            }
        }
    }
    errs
}

/// Run the adapters of the plugin layers whose states are not all cached
/// (or were captured from another build of the adapter). What failed.
pub fn capture_missing(p: &Project, project: &Path) -> Vec<String> {
    let dir = project.parent().unwrap_or(Path::new("."));
    let mut built: HashMap<&Source, std::result::Result<(PathBuf, String), String>> =
        HashMap::new();
    let mut errs = Vec::new();
    for (l, steps) in plugins(p) {
        let src = source(l);
        let exe = built.entry(src).or_insert_with(|| executable(src, dir));
        let (exe, stamp) = match exe {
            Ok(e) => e.clone(),
            Err(e) => {
                errs.push(format!("layer `{}`: {e}", l.id));
                continue;
            }
        };
        if steps.iter().all(|s| fresh(dir, &s.key, &stamp)) {
            continue;
        }
        eprintln!(
            "mui-cut: capturing plugin layer `{}` ({} states) from {}",
            l.id,
            steps.len(),
            exe.display()
        );
        if let Err(e) = replay(&exe, &src.args, dir, &steps, &stamp, &mut errs) {
            errs.push(format!("layer `{}`: {e}", l.id));
        }
    }
    errs
}

/// The adapter to run and its build stamp (size and modification time),
/// building it first when the source is a Cargo target.
fn executable(src: &Source, dir: &Path) -> Result<(PathBuf, String)> {
    let exe = if src.cargo.is_empty() {
        dir.join(&src.bin)
    } else {
        let (flag, name) = if src.example.is_empty() {
            ("--bin", &src.bin)
        } else {
            ("--example", &src.example)
        };
        let manifest = dir.join(&src.cargo);
        // Built from its own directory, with its own toolchain file: not
        // the one of wherever mui-cut was started, nor mui-cut's own.
        let out = Command::new("cargo")
            .current_dir(manifest.parent().unwrap_or(dir))
            .env_remove("RUSTUP_TOOLCHAIN")
            .args([
                "build",
                "--message-format=json-render-diagnostics",
                "--manifest-path",
            ])
            .arg(&manifest)
            .args([flag, name])
            .stderr(Stdio::inherit())
            .output()
            .map_err(|e| format!("cargo: {e}"))?;
        if !out.status.success() {
            return Err(format!("cargo build {flag} {name} failed"));
        }
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .filter(|m| m["reason"] == "compiler-artifact" && m["target"]["name"] == **name)
            .find_map(|m| m["executable"].as_str().map(PathBuf::from))
            .ok_or_else(|| format!("cargo built no executable for {flag} {name}"))?
    };
    let meta = std::fs::metadata(&exe).map_err(|e| format!("{}: {e}", exe.display()))?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    Ok((exe, format!("{}-{mtime}", meta.len())))
}

/// A state is cached when its manifest came from this build and every
/// image it names is on disk.
fn fresh(dir: &Path, key: &str, stamp: &str) -> bool {
    let cache = dir.join(CACHE);
    std::fs::read(cache.join(format!("{key}.json")))
        .ok()
        .and_then(|b| serde_json::from_slice::<Capture>(&b).ok())
        .is_some_and(|c| {
            c.stamp == stamp && c.fragments.iter().all(|f| cache.join(&f.src).is_file())
        })
}

/// One adapter process: its stdin, and its JSON packets.
struct Session {
    child: Child,
    stdin: ChildStdin,
    packets: Receiver<Value>,
    /// Every texture seen, by the adapter's name, base64 PNG.
    textures: HashMap<String, String>,
    tag: u64,
}

impl Session {
    fn open(exe: &Path, args: &[String], dir: &Path) -> Result<Self> {
        let mut child = Command::new(exe)
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("{}: {e}", exe.display()))?;
        let stdin = child.stdin.take().ok_or("no adapter stdin")?;
        let mut out = child.stdout.take().ok_or("no adapter stdout")?;
        let (tx, packets) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut head = [0u8; 5];
            while out.read_exact(&mut head).is_ok() {
                let n = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
                let mut body = vec![0; n];
                if out.read_exact(&mut body).is_err() {
                    break;
                }
                // Audio blocks are dropped: a capture is pictures.
                if head[0] == b'J'
                    && let Ok(v) = serde_json::from_slice::<Value>(&body)
                    && tx.send(v).is_err()
                {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            packets,
            textures: HashMap::new(),
            tag: 0,
        })
    }

    fn send(&mut self, v: &Value) -> Result<()> {
        writeln!(self.stdin, "{v}")
            .and_then(|()| self.stdin.flush())
            .map_err(|e| format!("adapter stdin: {e}"))
    }

    /// Ask for a scene and wait for the one that follows the ask: every
    /// command sent before it has been applied. Adapter errors (a rejected
    /// parameter, say) go to `errs`.
    fn snapshot(&mut self, errs: &mut Vec<String>) -> Result<Value> {
        self.tag += 1;
        let tag = self.tag;
        self.send(&json!({"op": "snapshot", "tag": tag}))?;
        let mut acked = false;
        loop {
            let v = match self.packets.recv_timeout(PATIENCE) {
                Ok(v) => v,
                Err(RecvTimeoutError::Timeout) => {
                    return Err("the adapter stopped answering".into());
                }
                Err(RecvTimeoutError::Disconnected) => return Err("the adapter exited".into()),
            };
            match v["type"].as_str() {
                Some("scene") => {
                    if let Some(imgs) = v["images"].as_object() {
                        for (k, d) in imgs {
                            if let Some(d) = d.as_str() {
                                self.textures.insert(k.clone(), d.to_owned());
                            }
                        }
                    }
                    if acked {
                        return Ok(v);
                    }
                }
                Some("edit") if v["command"]["op"] == "snapshot" && v["command"]["tag"] == tag => {
                    acked = true;
                }
                Some("error") => errs.push(format!(
                    "adapter: {}",
                    v["message"].as_str().unwrap_or("error")
                )),
                _ => {}
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Run the adapter through every step, capturing each state.
fn replay(
    exe: &Path,
    args: &[String],
    dir: &Path,
    steps: &[mui_cut::Step],
    stamp: &str,
    errs: &mut Vec<String>,
) -> Result<()> {
    let cache = dir.join(CACHE);
    std::fs::create_dir_all(cache.join("img")).map_err(|e| format!("{}: {e}", cache.display()))?;
    let mut s = Session::open(exe, args, dir)?;
    for step in steps {
        for c in &step.commands {
            s.send(c)?;
        }
        // Every step is captured, cached or not: the live host queues
        // input until a scene is taken, and only 256 of it.
        let scene = s.snapshot(errs)?;
        if !fresh(dir, &step.key, stamp) {
            save(&cache, &step.key, stamp, &scene["scene"], &s.textures)?;
        }
    }
    Ok(())
}

/// One state's manifest, its images renamed by their content so states
/// share them.
fn save(
    cache: &Path,
    key: &str,
    stamp: &str,
    manifest: &Value,
    textures: &HashMap<String, String>,
) -> Result<()> {
    let num = |v: &Value| v.as_f64().ok_or("a capture without its size");
    let mut fragments = Vec::new();
    for l in manifest["layers"]
        .as_array()
        .ok_or("a capture without layers")?
    {
        let name = l["src"].as_str().unwrap_or("");
        let data = textures
            .get(name)
            .ok_or_else(|| format!("the adapter never sent texture `{name}`"))?;
        let png = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| format!("texture `{name}`: {e}"))?;
        let src = format!("img/{:016x}.png", fnv(FNV_OFFSET, &png));
        let file = cache.join(&src);
        if !file.is_file() {
            write_atomic(&file, &png)?;
        }
        let r = &l["rect"];
        fragments.push(Fragment {
            group: l["group"].as_str().unwrap_or("background").to_owned(),
            rect: [num(&r[0])?, num(&r[1])?, num(&r[2])?, num(&r[3])?],
            src,
        });
    }
    let cap = Capture {
        width: num(&manifest["width"])?,
        height: num(&manifest["height"])?,
        fragments,
        surfaces: serde_json::from_value(manifest["surfaces"].clone()).unwrap_or_default(),
        stamp: stamp.to_owned(),
    };
    let json = serde_json::to_vec_pretty(&cap).map_err(|e| e.to_string())?;
    write_atomic(&cache.join(format!("{key}.json")), &json)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| format!("{}: {e}", path.display()))
}
