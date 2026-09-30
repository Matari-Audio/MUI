//! Plugin layers' captures, native side: build or find each plugin's
//! mui-motion-bridge adapter, replay the layer's commands on the frame grid
//! (`Layer::plugin_track`), and write every state the project shows that
//! the cache lacks. Rendering then only reads the cache.
//!
//! The adapter speaks the bridge's wire (tools/film/README.md): newline
//! JSON in; out, a kind byte (`J` JSON, `A` audio), a little-endian u32
//! length and the payload. Scene packets carry only textures that changed,
//! so a session keeps every texture it has seen.
//!
//! Adapters run on the bridge's manual sample clock at the project's rate
//! (`MUI_BRIDGE_CLOCK`): a layer with notes advances it frame by frame, and
//! the samples that come back are its soundtrack, kept as raw stereo f32
//! under `CACHE/audio/<key>.f32` ([`Layer::plugin_audio`]).
use std::collections::{HashMap, HashSet};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use base64::Engine as _;
use mui_cut::plugin::{CACHE, Capture, FNV_OFFSET, Fragment, Image, fnv, frame_at};
use mui_cut::sources::MediaKind;
use mui_cut::{Assets, Kind, Layer, Project, Source};
use serde_json::{Value, json};

use crate::Result;

/// How long one capture may take before the adapter counts as hung.
const PATIENCE: Duration = Duration::from_secs(60);

/// Every plugin layer's steps in every scene, and its soundtrack's key
/// and last advance when it plays notes: `(layer, steps, audio)`.
fn plugins(p: &Project) -> Vec<(&Layer, Vec<mui_cut::Step>, Option<(String, Value)>)> {
    p.scenes
        .iter()
        .flat_map(|s| {
            s.layers
                .iter()
                .filter(|l| matches!(l.kind, Kind::Plugin { .. }))
                .map(|l| {
                    let steps = l.plugin_track(p.fps, p.sample_rate, frame_at(s.duration, p.fps));
                    let audio = l.plugin_audio(&steps, p.fps, p.sample_rate, p.samples(s));
                    (l, steps, audio)
                })
        })
        .collect()
}

/// Where a plugin soundtrack is cached, relative to the project.
pub fn audio_file(key: &str) -> String {
    format!("{CACHE}/audio/{key}.f32")
}

/// Plugin layer `l`'s soundtrack in scene `s`, stereo interleaved, once
/// [`capture_missing`] has rendered it; `None` for a layer without notes.
pub fn layer_audio(
    p: &Project,
    project: &Path,
    s: &mui_cut::Scene,
    l: &Layer,
) -> Result<Option<Vec<f32>>> {
    let steps = l.plugin_track(p.fps, p.sample_rate, frame_at(s.duration, p.fps));
    let Some((key, _)) = l.plugin_audio(&steps, p.fps, p.sample_rate, p.samples(s)) else {
        return Ok(None);
    };
    let file = project
        .parent()
        .unwrap_or(Path::new("."))
        .join(audio_file(&key));
    let bytes = std::fs::read(&file)
        .map_err(|e| format!("layer `{}`: its audio {}: {e}", l.id, file.display()))?;
    Ok(Some(
        bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect(),
    ))
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
    for (_, steps, _) in plugins(p) {
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
            let srcs = cap
                .fragments
                .into_iter()
                .flat_map(|f| [Some(f.src), f.free.map(|i| i.src)]);
            for src in srcs.flatten() {
                let rel = format!("{CACHE}/{src}");
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
    let jobs = plugins(p)
        .into_iter()
        .map(|(l, steps, audio)| Job {
            what: format!("layer `{}`", l.id),
            source: source(l),
            steps,
            audio,
        })
        .collect();
    capture(jobs, project, p.sample_rate)
}

/// Capture every plugin source's fresh state ([`mui_cut::plugin::home`]),
/// whose manifest lists its parts for the Sources panel. What failed.
pub fn capture_sources(p: &Project, project: &Path) -> Vec<String> {
    let all = p.all_sources();
    let jobs = all
        .iter()
        .filter_map(|m| match &m.kind {
            MediaKind::Plugin { source } => Some(Job {
                what: format!("source `{}`", m.id),
                source,
                steps: vec![mui_cut::plugin::home_step(source)],
                audio: None,
            }),
            _ => None,
        })
        .collect();
    capture(jobs, project, p.sample_rate)
}

/// A plugin to replay: what it is (for messages), its adapter, the steps
/// to capture, and its soundtrack's key and last advance.
struct Job<'a> {
    what: String,
    source: &'a Source,
    steps: Vec<mui_cut::Step>,
    audio: Option<(String, Value)>,
}

/// Replay each job whose states (or soundtrack) are not all cached.
fn capture(jobs: Vec<Job>, project: &Path, rate: u32) -> Vec<String> {
    let dir = project.parent().unwrap_or(Path::new("."));
    let mut built: HashMap<&Source, std::result::Result<(PathBuf, String), String>> =
        HashMap::new();
    let mut errs = Vec::new();
    for Job {
        what,
        source: src,
        steps,
        audio,
    } in jobs
    {
        let exe = built.entry(src).or_insert_with(|| executable(src, dir));
        let (exe, stamp) = match exe {
            Ok(e) => e.clone(),
            Err(e) => {
                errs.push(format!("{what}: {e}"));
                continue;
            }
        };
        let heard = audio
            .as_ref()
            .is_none_or(|(k, _)| dir.join(audio_file(k)).is_file());
        if heard && steps.iter().all(|s| fresh(dir, &s.key, &stamp)) {
            continue;
        }
        eprintln!(
            "mui-cut: capturing plugin {what} ({} states) from {}",
            steps.len(),
            exe.display()
        );
        let clock = Clock {
            rate,
            audio: audio.as_ref(),
        };
        if let Err(e) = replay(&exe, &src.args, dir, &steps, &stamp, clock, &mut errs) {
            errs.push(format!("{what}: {e}"));
        }
    }
    errs
}

/// The adapter to run and its build stamp (size and modification time),
/// building it first when the source is a Cargo target.
pub fn executable(src: &Source, dir: &Path) -> Result<(PathBuf, String)> {
    let exe = if !src.plugin.is_empty() {
        crate::build::adapter(&src.plugin, dir)?
    } else if src.cargo.is_empty() {
        // Absolute: the adapter starts in `dir`, where a path relative to
        // mui-cut's own directory means something else.
        std::path::absolute(dir.join(&src.bin)).map_err(|e| format!("{}: {e}", src.bin))?
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
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("cargo: {e}"))?;
        if !out.status.success() {
            // Quiet unless it fails: `check --json` and friends stay clean.
            return Err(format!(
                "cargo build {flag} {name} failed:\n{}",
                String::from_utf8_lossy(&out.stderr)
            ));
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
            c.stamp == stamp
                && c.fragments.iter().all(|f| {
                    cache.join(&f.src).is_file()
                        && f.free.as_ref().is_none_or(|i| cache.join(&i.src).is_file())
                })
        })
}

/// An adapter's stdin, for more than one thread.
pub type Writer = std::sync::Arc<std::sync::Mutex<ChildStdin>>;

/// One command line to an adapter.
pub fn write(w: &Writer, v: &Value) -> Result<()> {
    let mut w = w.lock().map_err(|_| "adapter stdin poisoned")?;
    writeln!(w, "{v}")
        .and_then(|()| w.flush())
        .map_err(|e| format!("adapter stdin: {e}"))
}

/// What comes back on the audio side of the wire: samples (stereo
/// interleaved) and the end of an advance.
pub enum Sound {
    Samples(Vec<f32>),
    Advanced,
}

/// One adapter process: its stdin, its JSON packets, and its sound.
pub struct Session {
    child: Child,
    /// Shared with a live transport's audio thread ([`Session::writer`]).
    stdin: Writer,
    packets: Receiver<Value>,
    /// Taken by a live transport that advances on its own thread.
    pub sound: Option<Receiver<Sound>>,
    /// Every texture seen, by the adapter's name, base64 PNG.
    pub textures: HashMap<String, String>,
    tag: u64,
}

impl Session {
    /// Start `exe` on the manual sample clock at `rate`.
    pub fn open(exe: &Path, args: &[String], dir: &Path, rate: u32) -> Result<Self> {
        // A project named without a directory has an empty parent, which
        // no process can start in.
        let dir = if dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            dir
        };
        let mut child = Command::new(exe)
            .args(args)
            .current_dir(dir)
            .env("MUI_BRIDGE_CLOCK", format!("manual:{rate}"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("{}: {e}", exe.display()))?;
        let stdin = child.stdin.take().ok_or("no adapter stdin")?;
        let mut out = child.stdout.take().ok_or("no adapter stdout")?;
        let (tx, packets) = std::sync::mpsc::channel();
        let (heard, sound) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut head = [0u8; 5];
            while out.read_exact(&mut head).is_ok() {
                let n = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
                let mut body = vec![0; n];
                if out.read_exact(&mut body).is_err() {
                    break;
                }
                // Audio: the block's first sample frame, then stereo f32.
                if head[0] == b'A' {
                    let samples = body
                        .get(8..)
                        .unwrap_or_default()
                        .chunks_exact(4)
                        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                        .collect();
                    let _ = heard.send(Sound::Samples(samples));
                    continue;
                }
                let Ok(v) = serde_json::from_slice::<Value>(&body) else {
                    continue;
                };
                if v["type"] == "advanced" {
                    let _ = heard.send(Sound::Advanced);
                } else if tx.send(v).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            stdin: std::sync::Arc::new(std::sync::Mutex::new(stdin)),
            packets,
            sound: Some(sound),
            textures: HashMap::new(),
            tag: 0,
        })
    }

    /// Run the clock to `{"op": "advance"}`'s sample: the sound up to it.
    fn advance(&mut self, command: &Value) -> Result<Vec<f32>> {
        self.send(command)?;
        let sound = self.sound.as_ref().ok_or("the sound was taken")?;
        let mut out = Vec::new();
        loop {
            match sound.recv_timeout(PATIENCE) {
                Ok(Sound::Samples(s)) => out.extend(s),
                Ok(Sound::Advanced) => return Ok(out),
                Err(RecvTimeoutError::Timeout) => {
                    return Err("the adapter stopped playing".into());
                }
                Err(RecvTimeoutError::Disconnected) => return Err("the adapter exited".into()),
            }
        }
    }

    /// Its stdin, for a thread that sends while another snapshots.
    pub fn writer(&self) -> Writer {
        self.stdin.clone()
    }

    pub fn send(&mut self, v: &Value) -> Result<()> {
        write(&self.stdin, v)
    }

    /// Ask for a scene and wait for the one that follows the ask: every
    /// command sent before it has been applied. Adapter errors (a rejected
    /// parameter, say) go to `errs`.
    pub fn snapshot(&mut self, errs: &mut Vec<String>) -> Result<Value> {
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

/// The sample clock a replay runs on: its rate, and the soundtrack to
/// keep (its key and the advance that ends it), if any.
#[derive(Clone, Copy)]
struct Clock<'a> {
    rate: u32,
    audio: Option<&'a (String, Value)>,
}

/// Run the adapter through every step, capturing each state, and keep the
/// soundtrack the advances played.
fn replay(
    exe: &Path,
    args: &[String],
    dir: &Path,
    steps: &[mui_cut::Step],
    stamp: &str,
    clock: Clock,
    errs: &mut Vec<String>,
) -> Result<()> {
    let cache = dir.join(CACHE);
    std::fs::create_dir_all(cache.join("img")).map_err(|e| format!("{}: {e}", cache.display()))?;
    let mut s = Session::open(exe, args, dir, clock.rate)?;
    let mut sound = Vec::new();
    for step in steps {
        for c in &step.commands {
            // The clock first, and finished, so what follows (a parameter,
            // the capture) lands after exactly those samples.
            if c["op"] == "advance" {
                sound.extend(s.advance(c)?);
            } else {
                s.send(c)?;
            }
        }
        // Every step is captured, cached or not: the live host queues
        // input until a scene is taken, and only 256 of it.
        let scene = s.snapshot(errs)?;
        if !fresh(dir, &step.key, stamp) {
            save(&cache, &step.key, stamp, &scene, &s.textures)?;
        }
    }
    if let Some((key, tail)) = clock.audio {
        sound.extend(s.advance(tail)?);
        let bytes: Vec<u8> = sound.iter().flat_map(|v| v.to_le_bytes()).collect();
        let file = dir.join(audio_file(key));
        std::fs::create_dir_all(cache.join("audio"))
            .map_err(|e| format!("{}: {e}", cache.display()))?;
        write_atomic(&file, &bytes)?;
    }
    Ok(())
}

/// One state's manifest (a scene packet's `scene`, with its `patch`), its
/// images renamed by their content so states share them.
pub fn save(
    cache: &Path,
    key: &str,
    stamp: &str,
    packet: &Value,
    textures: &HashMap<String, String>,
) -> Result<()> {
    let manifest = &packet["scene"];
    let num = |v: &Value| v.as_f64().ok_or("a capture without its size");
    let rect = |r: &Value| -> Result<[f64; 4]> {
        Ok([num(&r[0])?, num(&r[1])?, num(&r[2])?, num(&r[3])?])
    };
    // A texture, renamed by its content, written once.
    let store = |name: &str| -> Result<String> {
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
        Ok(src)
    };
    let mut fragments = Vec::new();
    for l in manifest["layers"]
        .as_array()
        .ok_or("a capture without layers")?
    {
        let free = match l.get("free") {
            Some(f) if f.is_object() => Some(Image {
                src: store(f["src"].as_str().unwrap_or(""))?,
                rect: rect(&f["rect"])?,
            }),
            _ => None,
        };
        fragments.push(Fragment {
            group: l["group"].as_str().unwrap_or("background").to_owned(),
            rect: rect(&l["rect"])?,
            src: store(l["src"].as_str().unwrap_or(""))?,
            free,
        });
    }
    let cap = Capture {
        width: num(&manifest["width"])?,
        height: num(&manifest["height"])?,
        fragments,
        surfaces: serde_json::from_value(manifest["surfaces"].clone()).unwrap_or_default(),
        parts: serde_json::from_value(manifest["parts"].clone()).unwrap_or_default(),
        stamp: stamp.to_owned(),
        patch: packet["patch"].clone(),
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
