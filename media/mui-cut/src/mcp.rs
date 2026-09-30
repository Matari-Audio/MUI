//! `mui-cut mcp`: the project as Model Context Protocol tools over stdio,
//! so an agent can open, read, patch, check and look at a `*.cut.json` and
//! see (and steer) what the person in `mui-cut serve` is looking at.
//!
//! Newline-delimited JSON-RPC 2.0 on stdin/stdout, hand-rolled: MCP's tool
//! surface is four methods, and the rest of the CLI is synchronous std.
//! Every tool re-reads the file and every edit writes it (validated,
//! canonical, atomic), so the file stays the one source of truth: an open
//! editor reloads an agent's edit, and the next tool call sees the
//! person's.
use std::io::{BufRead as _, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use mui_cut::check::{self, Issue, Severity};
use mui_cut::{Project, eval};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::tools::{self, Picture};
use crate::{Backend, Result, write_atomic};

/// The newest protocol revision this server speaks; older clients get
/// their own echoed back (the tool surface is the same).
const PROTOCOL: &str = "2025-06-18";

const INSTRUCTIONS: &str = "mui-cut edits *.cut.json motion projects (scenes of keyframed layers). \
Start with `open`, then `list` to see scenes and layers. Edit with `patch` (RFC 6902 JSON Patch; \
pointers may name scenes and layers by name/id, e.g. /scenes/intro/layers/title/x), `set`, `key`, \
`add_layer`, `remove_layer`; every edit is validated and written to the file, and an open editor \
reloads it. Look with `still` (one frame), `sheet` (a grid of frames at every key) and `strip` \
(one layer's motion); verify with `check` (lints with JSON paths, times and fixes) and `eval` \
(numbers). `editor_state` shows what the person in the web editor is looking at, `editor_goto` \
moves their playhead or selection. `plugin_parts` captures a plugin layer's live UI and lists its \
parts (animate them as `parts.<id>.x` etc.) and surfaces (aim `pointer_x`/`pointer_y` at their frames). \
`schema` has every field.";

pub fn serve(project: Option<&str>) -> Result<()> {
    let mut server = Server::default();
    if let Some(p) = project {
        crate::load(Path::new(p))?;
        server.project = Some(p.into());
    }
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = server.handle(&line) {
            writeln!(out, "{reply}")
                .and_then(|()| out.flush())
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

struct Job {
    child: Child,
    output: String,
    log: Arc<Mutex<String>>,
}

#[derive(Default)]
struct Server {
    project: Option<PathBuf>,
    port: Option<u16>,
    jobs: Vec<Job>,
}

// ---------------------------------------------------------------- tool args

/// Open a project (and make it the one the other tools use).
#[derive(Deserialize, JsonSchema)]
struct Open {
    /// Path to a `*.cut.json`.
    path: String,
    /// Write a new one-scene project there if there is no file.
    #[serde(default)]
    create: bool,
    /// Port of a `mui-cut serve` on this project (default 8740).
    #[serde(default)]
    editor_port: Option<u16>,
}

/// Optionally a scene by name (default: every scene, or the first).
#[derive(Deserialize, JsonSchema)]
struct SceneArg {
    #[serde(default)]
    scene: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct Get {
    /// JSON Pointer into the file; array items can be named by their `id`
    /// or `name`: `/scenes/intro/layers/title/x`. Empty for the whole file.
    #[serde(default)]
    pointer: String,
}

#[derive(Deserialize, JsonSchema)]
struct Patch {
    /// RFC 6902 operations, applied all or nothing:
    /// `{"op": "add"|"remove"|"replace"|"move"|"copy"|"test", "path": "/scenes/0/layers/-", "value": ..., "from": ...}`.
    /// Paths may name scenes and layers by name/id.
    ops: Vec<Value>,
}

#[derive(Deserialize, JsonSchema)]
struct Set {
    #[serde(default)]
    scene: Option<String>,
    /// Layer id.
    layer: String,
    /// Property path as `list` shows it: `x`, `fill`, `animators.0.opacity`.
    prop: String,
    /// A plain value (number, `#rrggbb`) or a key list
    /// `[{"t": 0, "v": 0, "interp": "bezier", "out": [0.3, 0]}, ...]`.
    value: Value,
}

#[derive(Deserialize, JsonSchema)]
struct KeyArgs {
    #[serde(default)]
    scene: Option<String>,
    layer: String,
    /// Property path: `x`, `fill`, `animators.0.opacity`.
    prop: String,
    /// Seconds into the scene; a key within half a frame is replaced.
    t: f64,
    /// Number, or `#rrggbb` for colours.
    v: Value,
    /// `hold`, `linear` or `bezier` (default): shapes the segment leaving it.
    #[serde(default)]
    interp: Option<String>,
    /// Bezier handle offsets `[seconds, value]`.
    #[serde(default, rename = "in")]
    in_: Option<[f64; 2]>,
    #[serde(default)]
    out: Option<[f64; 2]>,
}

#[derive(Deserialize, JsonSchema)]
struct AddLayer {
    #[serde(default)]
    scene: Option<String>,
    /// The layer: `{"id": "dot", "kind": "ellipse", "x": 640, "y": 360, ...}`.
    layer: Value,
    /// Paint order position (0 = bottom); default on top.
    #[serde(default)]
    index: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct RemoveLayer {
    #[serde(default)]
    scene: Option<String>,
    id: String,
}

#[derive(Deserialize, JsonSchema)]
struct Eval {
    #[serde(default)]
    scene: Option<String>,
    /// Seconds into the scene.
    t: f64,
    /// Just this layer.
    #[serde(default)]
    layer: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct Check {
    /// Leave out `info` notes.
    #[serde(default)]
    warnings_only: bool,
}

#[derive(Deserialize, JsonSchema)]
struct Still {
    #[serde(default)]
    scene: Option<String>,
    t: f64,
    /// Pixels wide (default 960).
    #[serde(default)]
    width: Option<u32>,
    /// `classic` (default), `gpu` (vello_gpu) or `cpu`.
    #[serde(default)]
    renderer: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
struct Sheet {
    #[serde(default)]
    scene: Option<String>,
    /// Exact times, seconds into each scene; default every key and scene
    /// boundary plus an even spread.
    #[serde(default)]
    times: Option<Vec<f64>>,
    /// Frames per scene when choosing times (default 8).
    #[serde(default)]
    n: Option<usize>,
    /// Sheet pixels wide (default 1600).
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    cols: Option<usize>,
    /// `classic` (default), `gpu` (vello_gpu) or `cpu`.
    #[serde(default)]
    renderer: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
struct Strip {
    layer: String,
    #[serde(default)]
    scene: Option<String>,
    /// Ghosts (default 8).
    #[serde(default)]
    n: Option<usize>,
    #[serde(default)]
    width: Option<u32>,
    /// `classic` (default), `gpu` (vello_gpu) or `cpu`.
    #[serde(default)]
    renderer: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
struct Diff {
    /// The other version of the project (a copy, or `git show REV:file > f`).
    against: String,
    /// Changed frames to show (default 6).
    #[serde(default)]
    n: Option<usize>,
    #[serde(default)]
    width: Option<u32>,
    /// `classic` (default), `gpu` (vello_gpu) or `cpu`.
    #[serde(default)]
    renderer: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
struct Gen {
    /// A Rhai script file (see `mui-cut gen`). Returning a project map
    /// replaces the open project; returning an array of layers merges them
    /// into `scene` by id (reruns replace their own layers).
    script: String,
    #[serde(default)]
    seed: Option<u64>,
    #[serde(default)]
    scene: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct Render {
    /// Output video path (`.mp4`), or `null` to only time it.
    output: String,
    #[serde(default)]
    scene: Option<String>,
    /// Motion blur subframes.
    #[serde(default)]
    mb: Option<u32>,
    /// `WxH`.
    #[serde(default)]
    size: Option<String>,
    /// `classic` (default), `gpu` (vello_gpu) or `cpu`.
    #[serde(default)]
    renderer: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
struct JobArg {
    /// The id `render` returned.
    job: usize,
}

#[derive(Deserialize, JsonSchema)]
struct EditorState {
    #[serde(default)]
    port: Option<u16>,
}

#[derive(Deserialize, JsonSchema)]
struct EditorGoto {
    /// Scene name.
    #[serde(default)]
    scene: Option<String>,
    /// Playhead, seconds into the scene.
    #[serde(default)]
    t: Option<f64>,
    /// Layer id to select (null clears).
    #[serde(default)]
    select: Option<Option<String>>,
    /// Property to show in the graph editor.
    #[serde(default)]
    prop: Option<String>,
    #[serde(default)]
    playing: Option<bool>,
    #[serde(default)]
    port: Option<u16>,
}

#[derive(Deserialize, JsonSchema)]
struct PluginParts {
    /// The plugin layer's id.
    layer: String,
    #[serde(default)]
    scene: Option<String>,
    /// Seconds into the scene (default 0): the state its keyed parameters
    /// and pointer leave the UI in.
    #[serde(default)]
    t: f64,
}

#[derive(Deserialize, JsonSchema)]
struct Nothing {}

fn tool<T: JsonSchema>(name: &str, description: &str) -> Value {
    let mut schema = schemars::schema_for!(T).to_value();
    if let Some(o) = schema.as_object_mut() {
        o.remove("$schema");
        o.remove("title");
        o.remove("description");
    }
    json!({ "name": name, "description": description, "inputSchema": schema })
}

fn tools() -> Vec<Value> {
    vec![
        tool::<Open>(
            "open",
            "Open (or with create, start) a *.cut.json project; returns its outline.",
        ),
        tool::<Nothing>(
            "schema",
            "The project file's JSON Schema: every field, its type, default and meaning.",
        ),
        tool::<SceneArg>(
            "list",
            "Outline: size, fps, scenes, and per layer its kind, text, animated properties with key times, animators and deformers.",
        ),
        tool::<Get>(
            "get",
            "Read raw JSON at a pointer (layers and scenes addressable by id/name).",
        ),
        tool::<Patch>(
            "patch",
            "Apply an RFC 6902 JSON Patch to the file, all or nothing; validated, written canonically; returns check counts.",
        ),
        tool::<Set>(
            "set",
            "Set one property of a layer to a plain value or a key list.",
        ),
        tool::<KeyArgs>("key", "Add or replace one keyframe on a layer property."),
        tool::<AddLayer>("add_layer", "Insert a layer into a scene (default on top)."),
        tool::<RemoveLayer>("remove_layer", "Delete a layer by id."),
        tool::<Eval>(
            "eval",
            "Every evaluated value of a scene (or one layer) at a time, including per-glyph/per-copy fx.",
        ),
        tool::<Check>(
            "check",
            "Lint the project: unknown fields, key mistakes, overshoot, missing assets, off-frame/clipped layers, text overlap, low contrast, fast motion, empty frames. Each with a JSON path, times and a fix.",
        ),
        tool::<Still>("still", "One rendered frame as a PNG image."),
        tool::<Sheet>(
            "sheet",
            "A contact sheet PNG: captioned frames at every key and scene boundary (or given times).",
        ),
        tool::<Strip>(
            "strip",
            "One layer's motion as a PNG: onion-skin ghosts from faint (early) to solid, and its path with times.",
        ),
        tool::<Diff>(
            "diff",
            "Compare against another version of the project: the most-changed frames as A | B | heat map rows.",
        ),
        tool::<Gen>(
            "gen",
            "Run a sandboxed, seeded Rhai script that generates a whole project or a scene's layers (hundreds of layers from a loop), validated and written to the open project.",
        ),
        tool::<Render>(
            "render",
            "Start rendering a video in the background; returns a job id for render_status.",
        ),
        tool::<JobArg>("render_status", "A render job's state and output."),
        tool::<PluginParts>(
            "plugin_parts",
            "A plugin layer at a time: runs its adapter if that state is not captured yet, then lists its parts (id, rect in the UI's pixels, its own motion), every surface (id, parent, frame: what `select` and the pointer can aim at), the UI size, explode and pointer.",
        ),
        tool::<EditorState>(
            "editor_state",
            "What the person in `mui-cut serve` sees: scene, playhead, selection, graphed property, playing.",
        ),
        tool::<EditorGoto>(
            "editor_goto",
            "Move the open editor's scene, playhead, selection or graphed property (to show the person something).",
        ),
    ]
}

// ---------------------------------------------------------------- protocol

impl Server {
    fn handle(&mut self, line: &str) -> Option<String> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => return Some(error(&Value::Null, -32700, &format!("parse error: {e}"))),
        };
        let id = msg.get("id").cloned();
        let method = msg["method"].as_str().unwrap_or("");
        // A notification (no id) gets no reply.
        let id = id?;
        let params = &msg["params"];
        let result = match method {
            "initialize" => {
                let asked = params["protocolVersion"].as_str().unwrap_or(PROTOCOL);
                let version = if asked <= PROTOCOL { asked } else { PROTOCOL };
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "mui-cut", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS,
                })
            }
            "ping" => json!({}),
            "tools/list" => json!({ "tools": tools() }),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                match self.call(name, args) {
                    Ok(content) => json!({ "content": content, "isError": false }),
                    Err(e) => json!({ "content": [text(&e)], "isError": true }),
                }
            }
            _ => return Some(error(&id, -32601, &format!("no method `{method}`"))),
        };
        Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string())
    }

    fn call(&mut self, name: &str, args: Value) -> Result<Vec<Value>> {
        fn parse<T: serde::de::DeserializeOwned>(v: Value) -> Result<T> {
            serde_json::from_value(v).map_err(|e| format!("arguments: {e}"))
        }
        match name {
            "open" => self.open(parse(args)?),
            "schema" => Ok(vec![text(&pretty(&Project::json_schema()))]),
            "list" => {
                let a: SceneArg = parse(args)?;
                Ok(vec![text(&pretty(&self.outline(a.scene.as_deref())?))])
            }
            "get" => {
                let a: Get = parse(args)?;
                let raw = self.raw()?;
                let tokens = resolve(&raw, &a.pointer)?;
                let v = at(&raw, &tokens).ok_or("nothing there")?;
                Ok(vec![text(&pretty(v))])
            }
            "patch" => {
                let a: Patch = parse(args)?;
                self.edit(|raw| {
                    for (i, op) in a.ops.iter().enumerate() {
                        apply(raw, op).map_err(|e| format!("ops[{i}]: {e}"))?;
                    }
                    Ok(())
                })
            }
            "set" => {
                let a: Set = parse(args)?;
                let ptr = self.prop_pointer(a.scene.as_deref(), &a.layer, &a.prop)?;
                self.edit(|raw| apply(raw, &json!({ "op": "add", "path": ptr, "value": a.value })))
            }
            "key" => {
                let a: KeyArgs = parse(args)?;
                let ptr = self.prop_pointer(a.scene.as_deref(), &a.layer, &a.prop)?;
                let fps = self.load()?.fps;
                let mut key = json!({ "t": a.t, "v": a.v });
                if let Some(i) = a.interp {
                    key["interp"] = i.into();
                }
                if let Some(h) = a.in_ {
                    key["in"] = json!(h);
                }
                if let Some(h) = a.out {
                    key["out"] = json!(h);
                }
                self.edit(|raw| {
                    let tokens = resolve(raw, &ptr)?;
                    let (last, parent) = tokens.split_last().ok_or("no property")?;
                    let obj = at_mut(raw, parent)
                        .and_then(Value::as_object_mut)
                        .ok_or("no such layer")?;
                    let mut keys = match obj.remove(last.as_str()) {
                        Some(Value::Array(k)) => k,
                        _ => Vec::new(),
                    };
                    keys.retain(|k| (k["t"].as_f64().unwrap_or(f64::NAN) - a.t).abs() >= 0.5 / fps);
                    keys.push(key.clone());
                    keys.sort_by(|x, y| {
                        x["t"]
                            .as_f64()
                            .partial_cmp(&y["t"].as_f64())
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    obj.insert(last.clone(), Value::Array(keys));
                    Ok(())
                })
            }
            "add_layer" => {
                let a: AddLayer = parse(args)?;
                let si = self.scene_index(a.scene.as_deref())?;
                let at = a.index.map_or_else(|| "-".to_owned(), |i| i.to_string());
                self.edit(|raw| apply(raw, &json!({ "op": "add", "path": format!("/scenes/{si}/layers/{at}"), "value": a.layer })))
            }
            "remove_layer" => {
                let a: RemoveLayer = parse(args)?;
                let si = self.scene_index(a.scene.as_deref())?;
                let path = format!("/scenes/{si}/layers/{}", escape(&a.id));
                self.edit(|raw| apply(raw, &json!({ "op": "remove", "path": path })))
            }
            "eval" => {
                let a: Eval = parse(args)?;
                let p = self.load()?;
                let s = pick(&p, a.scene.as_deref())?;
                let f = eval(&p, s, a.t);
                let v = match a.layer {
                    Some(id) => serde_json::to_value(
                        f.layers
                            .iter()
                            .find(|l| l.id == id)
                            .ok_or_else(|| format!("no layer `{id}`"))?,
                    ),
                    None => serde_json::to_value(&f),
                }
                .map_err(|e| e.to_string())?;
                Ok(vec![text(&pretty(&v))])
            }
            "check" => {
                let a: Check = parse(args)?;
                let mut issues = tools::check_file(&self.path()?)?;
                if a.warnings_only {
                    issues.retain(|i| i.severity >= Severity::Warning);
                }
                Ok(vec![text(&summary(&issues, usize::MAX))])
            }
            "still" => {
                let a: Still = parse(args)?;
                let path = self.path()?;
                let p = crate::load(&path)?;
                let s = pick(&p, a.scene.as_deref())?;
                let w = a.width.unwrap_or(960).clamp(16, 4096) & !1;
                let h = ((f64::from(w) * f64::from(p.size[1]) / f64::from(p.size[0])).round()
                    as u32)
                    .max(2)
                    & !1;
                let (w, h) = (w as u16, h.min(4096) as u16);
                let mut b = Backend::open_at(&p, &path, a.renderer.as_deref(), (w, h), 1, None)?;
                let px = tools::draw_all(&mut b, &[eval(&p, s, a.t)])?
                    .pop()
                    .ok_or("no frame came back")?;
                let (w, h) = (u32::from(w), u32::from(h));
                image(&Picture {
                    png: tools::png_bytes(w, h, &px)?,
                    w,
                    h,
                    note: format!("{} at {:.2}s ({w}x{h}, {})", s.name, a.t, b.name()),
                })
            }
            "sheet" => {
                let a: Sheet = parse(args)?;
                image(&tools::sheet(
                    &self.path()?,
                    &tools::SheetOpts {
                        scene: a.scene,
                        times: a.times,
                        per_scene: a.n.unwrap_or(8),
                        width: a.width.unwrap_or(1600),
                        cols: a.cols,
                        renderer: a.renderer,
                    },
                )?)
            }
            "strip" => {
                let a: Strip = parse(args)?;
                image(&tools::strip(
                    &self.path()?,
                    &a.layer,
                    a.scene.as_deref(),
                    a.n.unwrap_or(8),
                    a.width.unwrap_or(1600),
                    a.renderer.as_deref(),
                )?)
            }
            "diff" => {
                let a: Diff = parse(args)?;
                image(&tools::diff(
                    &self.path()?,
                    Path::new(&a.against),
                    a.n.unwrap_or(6),
                    a.width.unwrap_or(1600),
                    a.renderer.as_deref(),
                )?)
            }
            "gen" => {
                let a: Gen = parse(args)?;
                let script =
                    std::fs::read_to_string(&a.script).map_err(|e| format!("{}: {e}", a.script))?;
                let path = self.path()?;
                let json = crate::script::generate(
                    &script,
                    a.seed.unwrap_or(0),
                    Some((&path, a.scene.as_deref())),
                )?;
                self.edit(|raw| {
                    *raw = serde_json::from_str(&json).map_err(|e| e.to_string())?;
                    Ok(())
                })
            }
            "plugin_parts" => {
                let a: PluginParts = parse(args)?;
                let path = self.path()?;
                let p = crate::load(&path)?;
                let s = pick(&p, a.scene.as_deref())?;
                let at = eval(&p, s, a.t)
                    .layers
                    .into_iter()
                    .find(|l| l.id == a.layer)
                    .ok_or_else(|| format!("no layer `{}`", a.layer))?
                    .plugin
                    .ok_or_else(|| format!("layer `{}` is not a plugin", a.layer))?;
                let errs = crate::host::capture_missing(&p, &path);
                if !errs.is_empty() {
                    return Err(errs.join("\n"));
                }
                let manifest = path
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(mui_cut::plugin::CACHE)
                    .join(format!("{}.json", at.state));
                let cap: mui_cut::Capture = std::fs::read_to_string(&manifest)
                    .map_err(|e| e.to_string())
                    .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
                    .map_err(|e| format!("{}: {e}", manifest.display()))?;
                let parts: Vec<Value> = cap
                    .fragments
                    .iter()
                    .filter(|f| f.group != "background")
                    .map(|f| {
                        let own = at.parts.iter().find(|q| q.id == f.group);
                        json!({ "id": f.group, "rect": f.rect, "motion": own })
                    })
                    .collect();
                Ok(vec![text(&pretty(&json!({
                    "state": at.state,
                    "size": [cap.width, cap.height],
                    "explode": at.explode,
                    "pointer": at.pointer,
                    "parts": parts,
                    "surfaces": cap.surfaces,
                })))])
            }
            "render" => self.render(parse(args)?),
            "render_status" => {
                let a: JobArg = parse(args)?;
                let job = self.jobs.get_mut(a.job).ok_or("no such job")?;
                let state = match job.child.try_wait().map_err(|e| e.to_string())? {
                    None => "running".to_owned(),
                    Some(s) if s.success() => "done".to_owned(),
                    Some(s) => format!("failed ({s})"),
                };
                let log = job.log.lock().expect("no panic holds it").clone();
                Ok(vec![text(&format!(
                    "job {}: {state}, {}\n{log}",
                    a.job, job.output
                ))])
            }
            "editor_state" => {
                let a: EditorState = parse(args)?;
                let port = a.port.or(self.port).unwrap_or(8740);
                let body = http(port, "GET", "/state", "")?;
                let mut v: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
                if let (Ok(mine), Some(theirs)) = (self.path(), v["project"].as_str()) {
                    let same =
                        std::fs::canonicalize(&mine).ok() == std::fs::canonicalize(theirs).ok();
                    v["same_project"] = same.into();
                }
                Ok(vec![text(&pretty(&v))])
            }
            "editor_goto" => {
                let a: EditorGoto = parse(args)?;
                let port = a.port.or(self.port).unwrap_or(8740);
                let mut m = json!({});
                if let Some(s) = a.scene {
                    m["scene"] = s.into();
                }
                if let Some(t) = a.t {
                    m["t"] = t.into();
                }
                if let Some(s) = a.select {
                    m["select"] = s.map_or(Value::Null, Value::from);
                }
                if let Some(p) = a.prop {
                    m["prop"] = p.into();
                }
                if let Some(p) = a.playing {
                    m["playing"] = p.into();
                }
                let reply = http(port, "POST", "/control", &m.to_string())?;
                Ok(vec![text(&format!("sent {m} to the editor: {reply}"))])
            }
            _ => Err(format!("no tool `{name}`")),
        }
    }

    // ------------------------------------------------------------ tools

    fn path(&self) -> Result<PathBuf> {
        self.project
            .clone()
            .ok_or_else(|| "no project open: call `open` first".into())
    }
    fn raw(&self) -> Result<Value> {
        let path = self.path()?;
        let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&src).map_err(|e| format!("{}: {e}", path.display()))
    }
    fn load(&self) -> Result<Project> {
        crate::load(&self.path()?)
    }

    fn open(&mut self, a: Open) -> Result<Vec<Value>> {
        let path = PathBuf::from(a.path);
        if !path.exists() {
            if !a.create {
                return Err(format!(
                    "{}: no such file (pass create: true to start one)",
                    path.display()
                ));
            }
            let fresh = json!({ "size": [1920, 1080], "fps": 30, "scenes": [{ "name": "main", "duration": 3, "layers": [] }] });
            let p = Project::load(&fresh.to_string())?;
            write_atomic(&path, &p.to_json())?;
        }
        crate::load(&path)?;
        self.project = Some(path);
        self.port = a.editor_port.or(self.port);
        Ok(vec![text(&pretty(&self.outline(None)?))])
    }

    fn outline(&self, scene: Option<&str>) -> Result<Value> {
        let p = self.load()?;
        let scenes: Vec<Value> = p
            .scenes
            .iter()
            .filter(|s| scene.is_none_or(|n| n == s.name))
            .map(|s| {
                let layers: Vec<Value> = s
                    .layers
                    .iter()
                    .map(|l| {
                        let kind = serde_json::to_value(&l.kind).unwrap_or_default();
                        let mut o = json!({ "id": l.id, "kind": kind["kind"] });
                        for k in ["text", "path"] {
                            if let Some(v) = kind.get(k) {
                                o[k] = v.clone();
                            }
                        }
                        let mut animated = serde_json::Map::new();
                        for (name, prop) in l.props_in(true) {
                            let times: Vec<f64> = match prop {
                                mui_cut::Prop::Num(mui_cut::Anim::Keys(k)) => k.iter().map(|k| k.t).collect(),
                                mui_cut::Prop::Color(mui_cut::Anim::Keys(k)) => k.iter().map(|k| k.t).collect(),
                                _ => continue,
                            };
                            animated.insert(name, json!(times));
                        }
                        let at0 = l.at(0.);
                        o["at_0"] = json!({ "x": at0.x, "y": at0.y, "scale": at0.scale, "opacity": at0.opacity, "fill": at0.fill });
                        if !animated.is_empty() {
                            o["animated"] = Value::Object(animated);
                        }
                        if !l.animators.is_empty() {
                            o["animators"] = l.animators.len().into();
                        }
                        if !l.deformers.is_empty() {
                            o["deformers"] = l.deformers.len().into();
                        }
                        o
                    })
                    .collect();
                json!({ "name": s.name, "duration": s.duration, "background": s.background, "layers": layers })
            })
            .collect();
        Ok(json!({ "project": self.path()?, "size": p.size, "fps": p.fps, "scenes": scenes }))
    }

    fn scene_index(&self, scene: Option<&str>) -> Result<usize> {
        let p = self.load()?;
        match scene {
            None if p.scenes.is_empty() => Err("the project has no scenes".into()),
            None => Ok(0),
            Some(n) => p
                .scenes
                .iter()
                .position(|s| s.name == n)
                .ok_or_else(|| format!("no scene `{n}`")),
        }
    }

    /// `/scenes/I/layers/ID/animators/0/x` for a dotted property path.
    fn prop_pointer(&self, scene: Option<&str>, layer: &str, prop: &str) -> Result<String> {
        let si = self.scene_index(scene)?;
        let rest: Vec<String> = prop.split('.').map(escape).collect();
        Ok(format!(
            "/scenes/{si}/layers/{}/{}",
            escape(layer),
            rest.join("/")
        ))
    }

    /// Change the file's JSON with `f`; refuse (and leave the file alone)
    /// if the result does not load or has fields a save would drop.
    fn edit(&mut self, f: impl FnOnce(&mut Value) -> Result<()>) -> Result<Vec<Value>> {
        let path = self.path()?;
        let mut raw = self.raw()?;
        f(&mut raw)?;
        let src = raw.to_string();
        let lint = check::lint_fields(&src);
        let refused: Vec<&Issue> = lint
            .iter()
            .filter(|i| i.code == "load" || i.code == "unknown_field")
            .collect();
        if !refused.is_empty() {
            let why: Vec<String> = refused.iter().map(ToString::to_string).collect();
            return Err(format!("not written:\n{}", why.join("\n")));
        }
        let p = Project::load(&src)?;
        write_atomic(&path, &p.to_json())?;
        let issues = tools::check_file(&path)?;
        Ok(vec![text(&format!(
            "written {}.\n{}",
            path.display(),
            summary(&issues, 8)
        ))])
    }

    fn render(&mut self, a: Render) -> Result<Vec<Value>> {
        let path = self.path()?;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut cmd = Command::new(exe);
        cmd.arg("render").arg(&path).args(["-o", &a.output]);
        if let Some(s) = &a.scene {
            cmd.args(["--scene", s]);
        }
        if let Some(mb) = a.mb {
            cmd.args(["--mb", &mb.to_string()]);
        }
        if let Some(s) = &a.size {
            cmd.args(["--size", s]);
        }
        if let Some(r) = &a.renderer {
            cmd.args(["--renderer", r]);
        }
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        let log = Arc::new(Mutex::new(String::new()));
        for pipe in [
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let log = log.clone();
            std::thread::spawn(move || {
                let mut s = String::new();
                let mut pipe = pipe;
                let _ = pipe.read_to_string(&mut s);
                log.lock().expect("no panic holds it").push_str(&s);
            });
        }
        self.jobs.push(Job {
            child,
            output: a.output,
            log,
        });
        Ok(vec![text(&format!(
            "render job {} started; poll render_status",
            self.jobs.len() - 1
        ))])
    }
}

fn pick<'a>(p: &'a Project, scene: Option<&str>) -> Result<&'a mui_cut::Scene> {
    match scene {
        Some(n) => p.scene(n).ok_or_else(|| format!("no scene `{n}`")),
        None => p
            .scenes
            .first()
            .ok_or_else(|| "the project has no scenes".into()),
    }
}

fn summary(issues: &[Issue], max: usize) -> String {
    let count = |s| issues.iter().filter(|i| i.severity == s).count();
    let mut out = format!(
        "check: {} errors, {} warnings, {} notes",
        count(Severity::Error),
        count(Severity::Warning),
        count(Severity::Info)
    );
    for i in issues.iter().take(max) {
        out.push('\n');
        out.push_str(&i.to_string());
    }
    if issues.len() > max {
        out.push_str(&format!(
            "\n... and {} more (call `check`)",
            issues.len() - max
        ));
    }
    out
}

fn text(s: &str) -> Value {
    json!({ "type": "text", "text": s })
}
fn pretty(v: &impl serde::Serialize) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}
fn image(p: &Picture) -> Result<Vec<Value>> {
    Ok(vec![
        json!({ "type": "image", "data": base64(&p.png), "mimeType": "image/png" }),
        text(&format!("{}x{}: {}", p.w, p.h, p.note)),
    ])
}
fn error(id: &Value, code: i64, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string()
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = u32::from(c[0]) << 16
            | u32::from(*c.get(1).unwrap_or(&0)) << 8
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A request to the editor server; its body.
fn http(port: u16, method: &str, path: &str, body: &str) -> Result<String> {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).map_err(|e| {
        format!("no editor on port {port} ({e}); start one with `mui-cut serve PROJECT`")
    })?;
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .map_err(|e| e.to_string())?;
    let mut reply = String::new();
    s.read_to_string(&mut reply).map_err(|e| e.to_string())?;
    let (head, body) = reply.split_once("\r\n\r\n").unwrap_or((&reply, ""));
    if !head.starts_with("HTTP/1.1 200") {
        return Err(format!("editor: {}", head.lines().next().unwrap_or("")));
    }
    Ok(body.to_owned())
}

// ---------------------------------------------------------------- JSON Patch

fn escape(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

/// A pointer's tokens, with array items named by `id`/`name` turned into
/// indices. The last token may name nothing yet (an `add`).
fn resolve(root: &Value, pointer: &str) -> Result<Vec<String>> {
    if pointer.is_empty() {
        return Ok(Vec::new());
    }
    let rest = pointer
        .strip_prefix('/')
        .ok_or_else(|| format!("`{pointer}` is not a JSON Pointer (it starts with /)"))?;
    let mut node = Some(root);
    let mut out = Vec::new();
    for raw in rest.split('/') {
        let tok = raw.replace("~1", "/").replace("~0", "~");
        let tok = match node {
            Some(Value::Array(items)) if tok != "-" && tok.parse::<usize>().is_err() => items
                .iter()
                .position(|v| v["id"].as_str() == Some(&tok) || v["name"].as_str() == Some(&tok))
                .ok_or_else(|| format!("nothing with id or name `{tok}` at {}", show(&out)))?
                .to_string(),
            _ => tok,
        };
        node = node.and_then(|n| match n {
            Value::Array(a) => tok.parse::<usize>().ok().and_then(|i| a.get(i)),
            Value::Object(o) => o.get(&tok),
            _ => None,
        });
        out.push(tok);
    }
    Ok(out)
}

fn show(tokens: &[String]) -> String {
    if tokens.is_empty() {
        "the root".into()
    } else {
        format!("/{}", tokens.join("/"))
    }
}

fn at<'a>(root: &'a Value, tokens: &[String]) -> Option<&'a Value> {
    tokens.iter().try_fold(root, |n, t| match n {
        Value::Array(a) => a.get(t.parse::<usize>().ok()?),
        Value::Object(o) => o.get(t),
        _ => None,
    })
}
fn at_mut<'a>(root: &'a mut Value, tokens: &[String]) -> Option<&'a mut Value> {
    tokens.iter().try_fold(root, |n, t| match n {
        Value::Array(a) => a.get_mut(t.parse::<usize>().ok()?),
        Value::Object(o) => o.get_mut(t),
        _ => None,
    })
}

fn add(root: &mut Value, tokens: &[String], v: Value) -> Result<()> {
    let Some((last, parent)) = tokens.split_last() else {
        *root = v;
        return Ok(());
    };
    match at_mut(root, parent).ok_or_else(|| format!("no parent {}", show(parent)))? {
        Value::Object(o) => {
            o.insert(last.clone(), v);
        }
        Value::Array(a) => {
            let i = if last == "-" {
                a.len()
            } else {
                last.parse::<usize>()
                    .map_err(|_| format!("`{last}` is not an index"))?
            };
            if i > a.len() {
                return Err(format!("index {i} is past the end ({})", a.len()));
            }
            a.insert(i, v);
        }
        _ => return Err(format!("{} holds no fields", show(parent))),
    }
    Ok(())
}

fn remove(root: &mut Value, tokens: &[String]) -> Result<Value> {
    let (last, parent) = tokens.split_last().ok_or("cannot remove the root")?;
    let gone = match at_mut(root, parent).ok_or_else(|| format!("no parent {}", show(parent)))? {
        Value::Object(o) => o.remove(last),
        Value::Array(a) => last
            .parse::<usize>()
            .ok()
            .filter(|&i| i < a.len())
            .map(|i| a.remove(i)),
        _ => None,
    };
    gone.ok_or_else(|| format!("nothing at {}", show(tokens)))
}

/// One RFC 6902 operation.
fn apply(root: &mut Value, op: &Value) -> Result<()> {
    let path = op["path"].as_str().ok_or("no `path`")?;
    let tokens = resolve(root, path)?;
    let value = || op.get("value").cloned().ok_or("no `value`");
    match op["op"].as_str().unwrap_or("") {
        "add" => add(root, &tokens, value()?),
        "remove" => remove(root, &tokens).map(drop),
        "replace" => {
            let slot =
                at_mut(root, &tokens).ok_or_else(|| format!("nothing at {path} to replace"))?;
            *slot = value()?;
            Ok(())
        }
        "move" => {
            let from = resolve(root, op["from"].as_str().ok_or("no `from`")?)?;
            let v = remove(root, &from)?;
            let to = resolve(root, path)?;
            add(root, &to, v)
        }
        "copy" => {
            let from = resolve(root, op["from"].as_str().ok_or("no `from`")?)?;
            let v = at(root, &from).cloned().ok_or("nothing at `from`")?;
            add(root, &tokens, v)
        }
        "test" => {
            if at(root, &tokens) == Some(&value()?) {
                Ok(())
            } else {
                Err(format!("test failed at {path}"))
            }
        }
        other => Err(format!("unknown op `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_vectors() {
        for (i, o) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(i.as_bytes()), o);
        }
    }

    #[test]
    fn patches_address_items_by_id_and_apply_all_six_ops() {
        let mut v =
            json!({"scenes": [{"name": "a", "layers": [{"id": "x/y", "x": 1}, {"id": "b"}]}]});
        apply(
            &mut v,
            &json!({"op": "replace", "path": "/scenes/a/layers/x~1y/x", "value": 5}),
        )
        .unwrap();
        apply(
            &mut v,
            &json!({"op": "add", "path": "/scenes/0/layers/-", "value": {"id": "c"}}),
        )
        .unwrap();
        apply(
            &mut v,
            &json!({"op": "copy", "from": "/scenes/a/layers/b", "path": "/scenes/a/layers/0"}),
        )
        .unwrap();
        apply(
            &mut v,
            &json!({"op": "move", "from": "/scenes/a/layers/c", "path": "/scenes/a/layers/1"}),
        )
        .unwrap();
        apply(
            &mut v,
            &json!({"op": "test", "path": "/scenes/a/layers/x~1y/x", "value": 5}),
        )
        .unwrap();
        apply(
            &mut v,
            &json!({"op": "remove", "path": "/scenes/a/layers/3"}),
        )
        .unwrap();
        let ids: Vec<&str> = v["scenes"][0]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["b", "c", "x/y"]);
        assert!(
            apply(
                &mut v,
                &json!({"op": "test", "path": "/scenes/a/layers/b/x", "value": 1})
            )
            .is_err()
        );
        assert!(
            apply(
                &mut v,
                &json!({"op": "remove", "path": "/scenes/a/layers/nope"})
            )
            .unwrap_err()
            .contains("`nope`")
        );
    }
}
