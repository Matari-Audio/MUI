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
Start with `open` (create: true with size/duration/mode/background for a new one), then `list` to see scenes and layers. \
Edit with `patch` (RFC 6902 JSON Patch; pointers may name scenes and layers by name/id, e.g. \
/scenes/intro/layers/title/x), `set`, `key`, `motion` (named entrances and exits), `add_layer`, \
`remove_layer`, and `batch` to send many of them in one call (one write, one check); every edit is \
validated and written to the file, and an open editor reloads it. Look with `still` (one frame), `sheet` (a grid of frames at every key) and `strip` \
(one layer's motion); verify with `check` (lints with JSON paths, times and fixes) and `eval` \
(numbers). `editor_state` shows what the person in the web editor is looking at, `editor_goto` \
moves their playhead or selection. `plugin_parts` captures a plugin layer's live UI and lists its \
parts as a tree (animate them as `parts.<path>.x` etc.; a control in a panel is `parts.osc/osc-shape.x`; \
`explode_levels` 2 captures and explodes panels, then their controls) and surfaces (aim `pointer_x`/`pointer_y` at their frames). \
`sources_list` lists the project's sources (files and plugins, a plugin with its part tree), \
`source_add` imports one (`plugin_add` onboards a plugin crate by folder or git URL, zero config); drag a part in as a layer with a plugin layer's `show: [part]`. \
`layer_parent` parents a layer to another (Cavalry style: it inherits position, rotation, scale, \
z and opacity) keeping it where it is on screen. `schema` has every field.";

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
#[serde(deny_unknown_fields)]
struct Open {
    /// Path to a `*.cut.json`.
    path: String,
    /// Write a new one-scene project there if there is no file.
    #[serde(default)]
    create: bool,
    /// Port of a `mui-cut serve` on this project (default 8740).
    #[serde(default)]
    editor_port: Option<u16>,
    /// With `create`: the new project's `[width, height]` (default
    /// `[1920, 1080]`), `fps` (30), and its scene's `scene` name ("main"),
    /// `duration` (3 s), `mode` ("2d" or "3d") and `background` colour.
    #[serde(default)]
    size: Option<[u32; 2]>,
    #[serde(default)]
    fps: Option<f64>,
    #[serde(default)]
    scene: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    background: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SchemaArgs {
    /// Just one definition, e.g. `Layer`, `Animator`, `Material`, `Scene`
    /// (the full schema is ~85 KB); its `$ref`s name others to ask for.
    #[serde(default)]
    def: Option<String>,
}

/// Several edits in one call: applied in order to one copy of the file,
/// all or nothing, written once (one reload, one undo step in the editor)
/// and checked once.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Batch {
    /// `[{"tool": "key", "args": {...}}, ...]`; tools: `patch`, `set`,
    /// `key`, `motion`, `add_layer`, `remove_layer`, `notes_set`,
    /// `notes_add`, `source_add`, `layer_parent`.
    calls: Vec<BatchCall>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BatchCall {
    tool: String,
    #[serde(default)]
    args: Value,
}

/// A named motion keyed onto a layer from `t` for `dur` seconds, merged
/// with its other keys (keys inside that span are replaced).
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Motion {
    #[serde(default)]
    scene: Option<String>,
    layer: String,
    /// Whole layer, from and back to its own values: `fade_in`,
    /// `fade_out`, `rise_in` / `rise_out` (fade while moving up by
    /// `distance`), `slide_in` / `slide_out` (from / to `dir`), `pop_in` /
    /// `pop_out` (scale with a little overshoot). Per glyph (text) or copy
    /// (duplicator), as an animator: `typewriter`, `cascade` (rise in one
    /// after another), `cascade_out`, `pop` (random order).
    preset: String,
    /// Start, seconds into the scene.
    t: f64,
    /// Seconds (default 0.6; per glyph for the per-glyph ones).
    #[serde(default)]
    dur: Option<f64>,
    /// slide: `left` (default in), `right` (default out), `up`, `down`.
    #[serde(default)]
    dir: Option<String>,
    /// Pixels moved by rise and slide (default 40 and 160).
    #[serde(default)]
    distance: Option<f64>,
    /// Seconds between glyphs/copies for the per-glyph ones.
    #[serde(default)]
    stagger: Option<f64>,
}

/// Optionally a scene by name (default: every scene, or the first).
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SceneArg {
    #[serde(default)]
    scene: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Get {
    /// JSON Pointer into the file; array items can be named by their `id`
    /// or `name`: `/scenes/intro/layers/title/x`. Empty for the whole file.
    #[serde(default)]
    pointer: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Patch {
    /// RFC 6902 operations, applied all or nothing:
    /// `{"op": "add"|"remove"|"replace"|"move"|"copy"|"test", "path": "/scenes/0/layers/-", "value": ..., "from": ...}`.
    /// Paths may name scenes and layers by name/id.
    ops: Vec<Value>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
struct RemoveLayer {
    #[serde(default)]
    scene: Option<String>,
    id: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
struct Check {
    /// Leave out `info` notes.
    #[serde(default)]
    warnings_only: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
    /// 3D beauty: the mean of this many jittered samples (soft shadows,
    /// depth of field, clean glass); 64 is final quality.
    #[serde(default)]
    samples: Option<usize>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
struct JobArg {
    /// The id `render` returned.
    job: usize,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EditorState {
    #[serde(default)]
    port: Option<u16>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
struct NotesArgs {
    /// The plugin layer's id.
    layer: String,
    #[serde(default)]
    scene: Option<String>,
    /// Notes, seconds from the scene's start: `{"t": 0.5, "dur": 0.25,
    /// "pitch": 60, "vel": 100}` (MIDI pitch, 60 is middle C; vel 1..127,
    /// default 100).
    notes: Vec<mui_cut::Note>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PluginPlay {
    #[serde(default)]
    scene: Option<String>,
    /// Seconds into the scene to start (default 0).
    #[serde(default)]
    from: f64,
    /// Seconds into the scene to end (default: its end).
    #[serde(default)]
    to: Option<f64>,
    /// Where to write it (default `.cut-cache/preview.wav` next to the
    /// project): `.wav` is the sound alone, `.mp4` the frames with it.
    #[serde(default)]
    out: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SourceAdd {
    /// The source: a file `{"id": "logo", "kind": "svg", "path": "logo.svg"}`
    /// (`image`, `svg`, `lottie`, `model`; the path relative to the project)
    /// or a plugin `{"id": "synth", "kind": "plugin", "source": {"cargo":
    /// "../Cargo.toml", "example": "synth"}}` (or `"source": {"bin": path}`).
    source: Value,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PluginAdd {
    /// A MUI plugin crate's folder (relative to the project, or absolute)
    /// or git URL.
    from: String,
    /// The source's id (default: the package name).
    #[serde(default)]
    id: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LayerParent {
    #[serde(default)]
    scene: Option<String>,
    /// The layer to attach.
    layer: String,
    /// The layer to attach it to; null or "" detaches it.
    #[serde(default)]
    parent: Option<String>,
    /// Seconds into the scene at which it keeps its place on screen
    /// (default 0).
    #[serde(default)]
    t: f64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
        tool::<SchemaArgs>(
            "schema",
            "The project file's JSON Schema: every field, its type, default and meaning; `def` for one definition (Layer, Animator, Material...).",
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
        tool::<Motion>(
            "motion",
            "Key a named motion onto a layer: fade_in/out, rise_in/out, slide_in/out, pop_in/out (the whole layer, eased, back to its own values) or typewriter, cascade, cascade_out, pop (per glyph or copy).",
        ),
        tool::<Batch>(
            "batch",
            "Several edits (patch, set, key, motion, add_layer, remove_layer, notes_*, source_add, layer_parent) in one call: all or nothing, one write, one check.",
        ),
        tool::<AddLayer>(
            "add_layer",
            "Insert a layer into a scene (default on top); keys inline (`\"x\": [{\"t\": 0, \"v\": 100}, ...]`). A plugin layer's `source` may name an imported source by id.",
        ),
        tool::<RemoveLayer>("remove_layer", "Delete a layer by id."),
        tool::<Eval>(
            "eval",
            "Every evaluated value of a scene (or one layer) at a time, including per-glyph/per-copy fx.",
        ),
        tool::<Check>(
            "check",
            "Lint the project: unknown fields, key mistakes, overshoot, missing assets, off-frame/clipped layers, text overlap, low contrast, fast motion, empty frames. Each with a JSON path, times and a fix.",
        ),
        tool::<Still>(
            "still",
            "One rendered frame as a PNG image, with how long it took; `samples` for a 3D beauty frame.",
        ),
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
            "A plugin layer at a time: runs its adapter if that state is not captured yet, then lists its parts as a tree (id: the path to key as `parts.<id>.x`, e.g. `osc/osc-shape`; its surface, frame and fragment rects in the UI's pixels, its own motion, children), as deep as the layer's `explode_levels` or its deepest keyed part; every surface (id, parent, frame: what `select` and the pointer can aim at), the UI size, explode and pointer.",
        ),
        tool::<NotesArgs>(
            "notes_set",
            "Replace a plugin layer's notes (the melody its DSP plays into the soundtrack and its UI follows).",
        ),
        tool::<NotesArgs>(
            "notes_add",
            "Add notes to a plugin layer, kept in time order.",
        ),
        tool::<PluginPlay>(
            "plugin_play",
            "Render a slice of a scene's sound (every plugin layer's notes and audio layer, mixed) to a WAV, or with an .mp4 `out` the frames with it; returns the file, its length and peak.",
        ),
        tool::<PluginParts>(
            "patch_get",
            "A plugin layer's patch at a time, as the plugin reports it: parameters off their defaults (value, text, normalised), modulation routes (source, target, depth, live value) and held notes.",
        ),
        tool::<Nothing>(
            "sources_list",
            "The project's sources: imported files and plugins (`listed`) and the ones layers use without importing, each with the layers that use it; a plugin's with its part tree (ids to put in a plugin layer's `show` or animate as `parts.<id>.x`), capturing it first if needed.",
        ),
        tool::<SourceAdd>(
            "source_add",
            "Import a file or plugin into the project's `sources` (the editor's Sources panel), validated.",
        ),
        tool::<PluginAdd>(
            "plugin_add",
            "Onboard a MUI plugin with no code in it: detects its crate, framework (moose, truce, nice-plug or plain MUI), MUI crates and editor, adds it to `sources`, builds a generic adapter for its real editor against this MUI (the plugin's checkout and Cargo.lock untouched) and captures it. Returns that report with the discovered part tree (ids for `show` and `parts.<id>.x`); if the plugin lacks something, the error says exactly what to add.",
        ),
        tool::<LayerParent>(
            "layer_parent",
            "Parent a layer to another (or detach it), keeping it where it is on screen at `t` in every variant: its local x, y, z, turns (rx, ry, rotation) and scale keys are rewritten into the new parent's space; a changed binding is written resolved per variant. Children inherit position, turns (tilts too, in 3D), scale, z and multiply opacity.",
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
            n if EDITS.contains(&n) => self.edit(|raw| self.edit_in(n, args, raw)),
            "batch" => {
                let a: Batch = parse(args)?;
                self.edit(|raw| {
                    for (i, c) in a.calls.into_iter().enumerate() {
                        if !EDITS.contains(&c.tool.as_str()) {
                            return Err(format!(
                                "calls[{i}]: `{}` is not an edit tool; batch takes {}",
                                c.tool,
                                EDITS.join(", ")
                            ));
                        }
                        let args = if c.args.is_null() { json!({}) } else { c.args };
                        self.edit_in(&c.tool, args, raw)
                            .map_err(|e| format!("calls[{i}] ({}): {e}", c.tool))?;
                    }
                    Ok(())
                })
            }
            "open" => self.open(parse(args)?),
            "schema" => {
                let a: SchemaArgs = parse(args)?;
                let schema = Project::json_schema();
                let Some(def) = a.def else {
                    return Ok(vec![text(&pretty(&schema))]);
                };
                let defs = schema["$defs"].as_object().ok_or("no $defs")?;
                let v = match def.as_str() {
                    "Project" | "" => {
                        let mut top = schema.clone();
                        if let Some(o) = top.as_object_mut() {
                            o.remove("$defs");
                        }
                        top
                    }
                    d => defs.get(d).cloned().ok_or_else(|| {
                        format!(
                            "no definition `{d}`{}",
                            hint(d, defs.keys().map(String::as_str))
                        )
                    })?,
                };
                Ok(vec![text(&pretty(&v))])
            }
            "list" => {
                let a: SceneArg = parse(args)?;
                Ok(vec![text(&pretty(&tidy(
                    self.outline(a.scene.as_deref())?,
                )))])
            }
            "get" => {
                let a: Get = parse(args)?;
                let raw = self.raw()?;
                let tokens = resolve(&raw, &a.pointer)?;
                let v = at(&raw, &tokens).ok_or("nothing there")?;
                Ok(vec![text(&pretty(v))])
            }
            "eval" => {
                let a: Eval = parse(args)?;
                let p = self.load()?;
                let s = pick(&p, a.scene.as_deref())?;
                let f = eval(&p, s, a.t);
                let v = match a.layer {
                    Some(id) => serde_json::to_value(
                        f.layers.iter().find(|l| l.id == id).ok_or_else(|| {
                            format!(
                                "no layer `{id}`{}",
                                hint(&id, f.layers.iter().map(|l| l.id.as_str()))
                            )
                        })?,
                    ),
                    None => serde_json::to_value(&f),
                }
                .map_err(|e| e.to_string())?;
                Ok(vec![text(&pretty(&tidy(v)))])
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
                let start = std::time::Instant::now();
                let mut b = Backend::open_at(&p, &path, a.renderer.as_deref(), (w, h), 1, None)?;
                let n = match a.samples {
                    None => 1,
                    Some(n @ 1..=4096) => {
                        match &mut b {
                            Backend::Gpu(g) => g.beauty = true,
                            _ => return Err("`samples` needs a GPU renderer".into()),
                        }
                        n
                    }
                    Some(n) => return Err(format!("`samples`: {n} is not 1..=4096")),
                };
                let frame = eval(&p, s, a.t);
                let px = match b.push(vec![frame; n])? {
                    Some(px) => px,
                    None => b.finish()?.pop().ok_or("no frame came back")?,
                };
                let (w, h) = (u32::from(w), u32::from(h));
                image(&Picture {
                    png: tools::png_bytes(w, h, &px)?,
                    w,
                    h,
                    note: format!(
                        "{} at {:.2}s ({w}x{h}, {}) in {} ms",
                        s.name,
                        a.t,
                        b.name(),
                        start.elapsed().as_millis()
                    ),
                })
            }
            "sheet" => {
                let a: Sheet = parse(args)?;
                let start = std::time::Instant::now();
                let mut pic = tools::sheet(
                    &self.path()?,
                    &tools::SheetOpts {
                        scene: a.scene,
                        times: a.times,
                        per_scene: a.n.unwrap_or(8),
                        width: a.width.unwrap_or(1600),
                        cols: a.cols,
                        renderer: a.renderer,
                    },
                )?;
                pic.note += &format!(" (in {} ms)", start.elapsed().as_millis());
                image(&pic)
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
                Ok(vec![text(&pretty(&tidy(mui_cut::plugin::tree_json(
                    &cap, &at,
                ))))])
            }
            "patch_get" => {
                let a: PluginParts = parse(args)?;
                let path = self.path()?;
                let p = crate::load(&path)?;
                let s = pick(&p, a.scene.as_deref())?;
                let at = eval(&p, s, a.t)
                    .layers
                    .into_iter()
                    .find(|l| l.id == a.layer)
                    .and_then(|l| l.plugin)
                    .ok_or_else(|| format!("no plugin layer `{}`", a.layer))?;
                let errs = crate::host::capture_missing(&p, &path);
                if !errs.is_empty() {
                    return Err(errs.join("\n"));
                }
                let manifest = path
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(mui_cut::plugin::CACHE)
                    .join(format!("{}.json", at.state));
                let cap: mui_cut::Capture = std::fs::read(&manifest)
                    .map_err(|e| e.to_string())
                    .and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()))
                    .map_err(|e| format!("{}: {e}", manifest.display()))?;
                if cap.patch.is_null() {
                    return Err(format!("`{}`'s plugin reports no patch", a.layer));
                }
                Ok(vec![text(&pretty(&cap.patch))])
            }
            "plugin_play" => {
                let a: PluginPlay = parse(args)?;
                let path = self.path()?;
                let p = crate::load(&path)?;
                let s = pick(&p, a.scene.as_deref())?;
                let dir = path.parent().unwrap_or(Path::new("."));
                let out = a.out.map_or_else(
                    || dir.join(mui_cut::plugin::CACHE).join("preview.wav"),
                    std::path::PathBuf::from,
                );
                let errs = crate::host::capture_missing(&p, &path);
                if !errs.is_empty() {
                    return Err(errs.join("\n"));
                }
                let pcm =
                    crate::audio::mix(&p, &path, &[s])?.ok_or("nothing in the scene sounds")?;
                let (from, to) = (a.from.max(0.), a.to.unwrap_or(s.duration).min(s.duration));
                if to <= from {
                    return Err("`to` must come after `from`".into());
                }
                let r = f64::from(p.sample_rate);
                let slice = &pcm[2 * (from * r) as usize..(2 * (to * r) as usize).min(pcm.len())];
                if out.extension().is_some_and(|e| e == "mp4") {
                    crate::audio::clip(&path, &s.name, from, to, &out)?;
                } else {
                    if let Some(d) = out.parent() {
                        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                    }
                    std::fs::write(&out, crate::audio::wav(slice, p.sample_rate))
                        .map_err(|e| format!("{}: {e}", out.display()))?;
                }
                let peak = slice.iter().fold(0f32, |m, v| m.max(v.abs()));
                Ok(vec![text(&format!(
                    "wrote {} ({:.2} s, peak {:.1} dBFS)",
                    out.display(),
                    to - from,
                    20. * f64::from(peak.max(1e-9)).log10()
                ))])
            }
            "sources_list" => {
                let path = self.path()?;
                let p = crate::load(&path)?;
                let errs = crate::host::capture_sources(&p, &path);
                let cache = path
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(mui_cut::plugin::CACHE);
                let rows: Vec<Value> = p
                    .all_sources()
                    .iter()
                    .map(|m| {
                        let mut v = serde_json::to_value(m).unwrap_or_default();
                        v["listed"] = p.sources.iter().any(|s| s.id == m.id).into();
                        let users: Vec<String> = p
                            .scenes
                            .iter()
                            .flat_map(|s| s.layers.iter().map(move |l| (s, l)))
                            .filter(|(_, l)| {
                                mui_cut::sources::Media::of(&l.kind).as_ref() == Some(&m.kind)
                            })
                            .map(|(s, l)| format!("{}/{}", s.name, l.id))
                            .collect();
                        v["used_by"] = json!(users);
                        if let Some((k, cap)) = m.state().and_then(|k| {
                            let b = std::fs::read(cache.join(format!("{k}.json"))).ok()?;
                            Some((k, serde_json::from_slice::<mui_cut::Capture>(&b).ok()?))
                        }) {
                            v["size"] = json!([cap.width, cap.height]);
                            v["parts"] = mui_cut::plugin::home_tree(&cap, &k);
                        }
                        v
                    })
                    .collect();
                Ok(vec![text(&pretty(&tidy(
                    json!({ "sources": rows, "errors": errs }),
                )))])
            }
            "plugin_add" => {
                let a: PluginAdd = parse(args)?;
                let path = self.path()?;
                let dir = path.parent().unwrap_or(Path::new("."));
                let r = crate::build::add(&path, &a.from, dir, a.id.as_deref())?;
                Ok(vec![text(&pretty(&r))])
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
            _ => Err(format!(
                "no tool `{name}`{}",
                hint(name, tools().iter().filter_map(|t| t["name"].as_str()))
            )),
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
            let mut scene = json!({
                "name": a.scene.as_deref().unwrap_or("main"),
                "duration": a.duration.unwrap_or(3.),
                "layers": [],
            });
            if let Some(m) = a.mode {
                scene["mode"] = m.into();
            }
            if let Some(b) = a.background {
                scene["background"] = b.into();
            }
            let fresh = json!({
                "size": a.size.unwrap_or([1920, 1080]),
                "fps": a.fps.unwrap_or(30.),
                "scenes": [scene],
            });
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
                        if !l.parent.is_empty() {
                            o["parent"] = l.parent.clone().into();
                        }
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

    /// One edit tool's change, made to `raw` (the file's JSON, or a batch's
    /// copy of it): everything is resolved against `raw` itself, so a
    /// batch's later calls see its earlier ones.
    fn edit_in(&self, name: &str, args: Value, raw: &mut Value) -> Result<()> {
        fn parse<T: serde::de::DeserializeOwned>(v: Value) -> Result<T> {
            serde_json::from_value(v).map_err(|e| format!("arguments: {e}"))
        }
        match name {
            "patch" => {
                let a: Patch = parse(args)?;
                for (i, op) in a.ops.iter().enumerate() {
                    apply(raw, op).map_err(|e| format!("ops[{i}]: {e}"))?;
                }
                Ok(())
            }
            "set" => {
                let a: Set = parse(args)?;
                let ptr = prop_pointer(raw, a.scene.as_deref(), &a.layer, &a.prop)?;
                apply(raw, &json!({ "op": "add", "path": ptr, "value": a.value }))
            }
            "key" => {
                let a: KeyArgs = parse(args)?;
                let ptr = prop_pointer(raw, a.scene.as_deref(), &a.layer, &a.prop)?;
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
                let fps = fps(raw);
                let tokens = resolve(raw, &ptr)?;
                let (last, parent) = tokens.split_last().ok_or("no property")?;
                let obj = at_mut(raw, parent)
                    .and_then(Value::as_object_mut)
                    .ok_or("no such layer")?;
                put_keys(obj, last, vec![key], fps);
                Ok(())
            }
            "motion" => {
                let a: Motion = parse(args)?;
                let si = scene_index(raw, a.scene.as_deref())?;
                let fps = fps(raw);
                let tokens = resolve(raw, &format!("/scenes/{si}/layers/{}", escape(&a.layer)))?;
                let layer = at_mut(raw, &tokens)
                    .and_then(Value::as_object_mut)
                    .ok_or_else(|| format!("no layer `{}`", a.layer))?;
                motion(layer, &a, fps)
            }
            "add_layer" => {
                let a: AddLayer = parse(args)?;
                let si = scene_index(raw, a.scene.as_deref())?;
                let at = a.index.map_or_else(|| "-".to_owned(), |i| i.to_string());
                apply(
                    raw,
                    &json!({ "op": "add", "path": format!("/scenes/{si}/layers/{at}"), "value": a.layer }),
                )
            }
            "remove_layer" => {
                let a: RemoveLayer = parse(args)?;
                let si = scene_index(raw, a.scene.as_deref())?;
                let path = format!("/scenes/{si}/layers/{}", escape(&a.id));
                apply(raw, &json!({ "op": "remove", "path": path }))
            }
            "notes_set" | "notes_add" => {
                let a: NotesArgs = parse(args)?;
                let ptr = prop_pointer(raw, a.scene.as_deref(), &a.layer, "notes")?;
                let mut notes = a.notes;
                if name == "notes_add" {
                    let old = resolve(raw, &ptr)
                        .ok()
                        .and_then(|t| at(raw, &t).cloned())
                        .unwrap_or(json!([]));
                    let old: Vec<mui_cut::Note> =
                        serde_json::from_value(old).map_err(|e| e.to_string())?;
                    notes.splice(0..0, old);
                }
                notes.sort_by(|x, y| x.t.total_cmp(&y.t).then(x.pitch.cmp(&y.pitch)));
                apply(raw, &json!({ "op": "add", "path": ptr, "value": notes }))
            }
            "source_add" => {
                let a: SourceAdd = parse(args)?;
                let list = raw
                    .as_object_mut()
                    .ok_or("the project is not an object")?
                    .entry("sources")
                    .or_insert_with(|| json!([]));
                list.as_array_mut()
                    .ok_or("`sources` is not a list")?
                    .push(a.source);
                Ok(())
            }
            "layer_parent" => {
                let a: LayerParent = parse(args)?;
                let si = scene_index(raw, a.scene.as_deref())?;
                // The rewrite works on loaded scenes: plugin sources named by
                // id are filled in first.
                inline_sources(raw)?;
                *raw = mui_cut::place::rewrite(raw, si, &|p, s| {
                    let l = mui_cut::place::reparent(p, s, &a.layer, a.parent.as_deref(), a.t)?;
                    let mut s = s.clone();
                    if let Some(o) = s.layers.iter_mut().find(|o| o.id == l.id) {
                        *o = l;
                    }
                    Ok(s)
                })?;
                Ok(())
            }
            _ => Err(format!("`{name}` is not an edit tool")),
        }
    }

    /// Change the file's JSON with `f`; refuse (and leave the file alone)
    /// if the result does not load or has fields a save would drop.
    fn edit(&self, f: impl FnOnce(&mut Value) -> Result<()>) -> Result<Vec<Value>> {
        let path = self.path()?;
        let mut raw = self.raw()?;
        f(&mut raw)?;
        inline_sources(&mut raw)?;
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
        Some(n) => p.scene(n).ok_or_else(|| {
            format!(
                "no scene `{n}`{}",
                hint(n, p.scenes.iter().map(|s| s.name.as_str()))
            )
        }),
        None => p
            .scenes
            .first()
            .ok_or_else(|| "the project has no scenes".into()),
    }
}

/// The tools that change the file, which `batch` can run.
const EDITS: [&str; 10] = [
    "patch",
    "set",
    "key",
    "motion",
    "add_layer",
    "remove_layer",
    "notes_set",
    "notes_add",
    "source_add",
    "layer_parent",
];

/// `; did you mean `x`?` for the nearest of `known`, else the names to pick
/// from (at most 20).
fn hint<'a>(name: &str, known: impl IntoIterator<Item = &'a str>) -> String {
    let known: Vec<String> = known.into_iter().map(str::to_owned).collect();
    if let Some(n) = check::nearest(name, &known) {
        return format!("; did you mean `{n}`?");
    }
    if known.is_empty() {
        return String::new();
    }
    let more = if known.len() > 20 { ", ..." } else { "" };
    format!(
        ": there is {}{more}",
        known[..known.len().min(20)].join(", ")
    )
}

fn fps(raw: &Value) -> f64 {
    raw["fps"].as_f64().filter(|f| *f > 0.).unwrap_or(30.)
}

/// A scene's index in the JSON by name, or the first.
fn scene_index(raw: &Value, scene: Option<&str>) -> Result<usize> {
    let scenes = raw["scenes"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    match scene {
        None if scenes.is_empty() => Err("the project has no scenes".into()),
        None => Ok(0),
        Some(n) => scenes
            .iter()
            .position(|s| s["name"].as_str() == Some(n))
            .ok_or_else(|| {
                format!(
                    "no scene `{n}`{}",
                    hint(n, scenes.iter().filter_map(|s| s["name"].as_str()))
                )
            }),
    }
}

/// `/scenes/I/layers/ID/animators/0/x` for a dotted property path.
fn prop_pointer(raw: &Value, scene: Option<&str>, layer: &str, prop: &str) -> Result<String> {
    let si = scene_index(raw, scene)?;
    let rest: Vec<String> = prop.split('.').map(escape).collect();
    Ok(format!(
        "/scenes/{si}/layers/{}/{}",
        escape(layer),
        rest.join("/")
    ))
}

/// Merge `keys` into `obj[prop]`: a plain value or nothing becomes the key
/// list; existing keys within half a frame of the new ones' span go.
fn put_keys(obj: &mut serde_json::Map<String, Value>, prop: &str, keys: Vec<Value>, fps: f64) {
    let t = |k: &Value| k["t"].as_f64().unwrap_or(f64::NAN);
    let (lo, hi) = keys
        .iter()
        .map(t)
        .fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x), b.max(x)));
    let mut all = match obj.remove(prop) {
        Some(Value::Array(k)) => k,
        _ => Vec::new(),
    };
    all.retain(|k| !(t(k) > lo - 0.5 / fps && t(k) < hi + 0.5 / fps));
    all.extend(keys);
    all.sort_by(|x, y| t(x).total_cmp(&t(y)));
    obj.insert(prop.to_owned(), Value::Array(all));
}

/// A layer's `prop` at `t` as the file has it (its default if unset).
fn value_at(
    layer: &serde_json::Map<String, Value>,
    prop: &str,
    t: f64,
    default: f64,
) -> Result<f64> {
    match layer.get(prop) {
        None => Ok(default),
        Some(v) => serde_json::from_value::<mui_cut::Anim<f64>>(v.clone())
            .map(|a| a.at(t))
            .map_err(|e| format!("`{prop}` cannot take a motion ({e})")),
    }
}

/// Key `m.preset` onto a layer (see [`Motion`]).
fn motion(layer: &mut serde_json::Map<String, Value>, m: &Motion, fps: f64) -> Result<()> {
    let (t0, d) = (m.t, m.dur.unwrap_or(0.6).max(1. / fps));
    // Key times on a microsecond grid: no 0.8999999999999999 in the file.
    let grid = |t: f64| (t * 1e6).round() / 1e6;
    let t1 = grid(t0 + d);
    let name = m.preset.as_str();
    // Per glyph / per copy: an animator, as the editor's presets.
    if matches!(name, "typewriter" | "cascade" | "cascade_out" | "pop") {
        let kind = layer.get("kind").and_then(Value::as_str).unwrap_or("");
        if !matches!(kind, "text" | "duplicator") {
            return Err(format!(
                "`{name}` moves glyphs or copies: the layer is a {kind}, not text or a duplicator"
            ));
        }
        let mut a = mui_cut::Animator::preset(name, t0, d).ok_or("no such preset")?;
        if let Some(s) = m.stagger {
            a.stagger = mui_cut::Anim::Value(s);
        }
        let a = serde_json::to_value(a).map_err(|e| e.to_string())?;
        let list = layer.entry("animators").or_insert_with(|| json!([]));
        list.as_array_mut()
            .ok_or("`animators` is not a list")?
            .push(a);
        return Ok(());
    }
    let entering = name.ends_with("_in");
    // Entrances land on the layer's own value at the end, exits leave from
    // it at the start. Eased like CSS's expo out (in) and in (out).
    let rest_t = if entering { t1 } else { t0 };
    let ease = |a: f64, b: f64| -> Vec<Value> {
        if entering {
            vec![
                json!({ "t": t0, "v": a, "out": [0.16 * d, b - a] }),
                json!({ "t": t1, "v": b, "in": [-0.7 * d, 0.] }),
            ]
        } else {
            vec![
                json!({ "t": t0, "v": a, "out": [0.7 * d, 0.] }),
                json!({ "t": t1, "v": b, "in": [-0.16 * d, a - b] }),
            ]
        }
    };
    // From `away` to `rest` coming in, the other way going out.
    let span = |away: f64, rest: f64| {
        if entering {
            ease(away, rest)
        } else {
            ease(rest, away)
        }
    };
    let mut keys: Vec<(&str, Vec<Value>)> = Vec::new();
    let opacity = value_at(layer, "opacity", rest_t, 1.)?;
    let fade = |keys: &mut Vec<(&str, Vec<Value>)>| keys.push(("opacity", span(0., opacity)));
    match name {
        "fade_in" | "fade_out" => fade(&mut keys),
        "rise_in" | "rise_out" => {
            let y = value_at(layer, "y", rest_t, 0.)?;
            let dy = m.distance.unwrap_or(40.);
            fade(&mut keys);
            keys.push(("y", span(if entering { y + dy } else { y - dy }, y)));
        }
        "slide_in" | "slide_out" => {
            let dir = m
                .dir
                .as_deref()
                .unwrap_or(if entering { "left" } else { "right" });
            let dist = m.distance.unwrap_or(160.);
            let (prop, sign) = match dir {
                "left" => ("x", -1.),
                "right" => ("x", 1.),
                "up" => ("y", -1.),
                "down" => ("y", 1.),
                d => return Err(format!("`dir`: `{d}` is not left, right, up or down")),
            };
            let v = value_at(layer, prop, rest_t, 0.)?;
            fade(&mut keys);
            keys.push((prop, span(v + sign * dist, v)));
        }
        "pop_in" | "pop_out" => {
            let s = value_at(layer, "scale", rest_t, 1.)?;
            let peak = grid(if entering {
                t0 + 0.65 * d
            } else {
                t0 + 0.35 * d
            });
            let k = if entering {
                vec![
                    json!({ "t": t0, "v": 0., "out": [0.12 * d, 0.9 * s] }),
                    json!({ "t": peak, "v": 1.08 * s }),
                    json!({ "t": t1, "v": s }),
                ]
            } else {
                vec![
                    json!({ "t": t0, "v": s }),
                    json!({ "t": peak, "v": 1.08 * s }),
                    json!({ "t": t1, "v": 0., "in": [-0.12 * d, 0.9 * s] }),
                ]
            };
            keys.push(("scale", k));
        }
        p => {
            return Err(format!(
                "no preset `{p}`{}",
                hint(
                    p,
                    [
                        "fade_in",
                        "fade_out",
                        "rise_in",
                        "rise_out",
                        "slide_in",
                        "slide_out",
                        "pop_in",
                        "pop_out",
                        "typewriter",
                        "cascade",
                        "cascade_out",
                        "pop"
                    ]
                )
            ));
        }
    }
    for (prop, k) in keys {
        put_keys(layer, prop, k, fps);
    }
    Ok(())
}

/// Plugin layers whose `source` names an imported source by id get that
/// source's spec, as the file stores it.
fn inline_sources(raw: &mut Value) -> Result<()> {
    let sources: Vec<(String, Value)> = raw["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["kind"] == "plugin")
        .filter_map(|s| Some((s["id"].as_str()?.to_owned(), s["source"].clone())))
        .collect();
    for layer in raw["scenes"]
        .as_array_mut()
        .into_iter()
        .flatten()
        .filter_map(|s| s["layers"].as_array_mut())
        .flatten()
    {
        let Some(id) = layer["source"].as_str().map(str::to_owned) else {
            continue;
        };
        let spec = sources
            .iter()
            .find(|(s, _)| *s == id)
            .map(|(_, v)| v.clone())
            .ok_or_else(|| {
                format!(
                    "layer `{}`: no plugin source `{id}` in `sources`{}",
                    layer["id"].as_str().unwrap_or(""),
                    hint(&id, sources.iter().map(|(s, _)| s.as_str()))
                )
            })?;
        layer["source"] = spec;
    }
    Ok(())
}

/// Numbers rounded to 0.01 for reading: capture rects and evaluated values
/// come out of float maths with 16 digits.
fn tidy(v: Value) -> Value {
    match v {
        Value::Number(n) if n.is_f64() => {
            let x = n.as_f64().unwrap_or_default();
            json!((x * 100.).round() / 100.)
        }
        Value::Array(a) => Value::Array(a.into_iter().map(tidy).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, tidy(v))).collect()),
        v => v,
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
                .ok_or_else(|| {
                    let names = items
                        .iter()
                        .filter_map(|v| v["id"].as_str().or_else(|| v["name"].as_str()));
                    format!(
                        "nothing with id or name `{tok}` at {}{}",
                        show(&out),
                        hint(&tok, names)
                    )
                })?
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

    fn layer(v: Value) -> serde_json::Map<String, Value> {
        match v {
            Value::Object(o) => o,
            _ => panic!("not a layer"),
        }
    }
    fn mo(preset: &str, t: f64, dur: f64) -> Motion {
        serde_json::from_value(json!({"layer": "l", "preset": preset, "t": t, "dur": dur})).unwrap()
    }
    fn ts(keys: &Value) -> Vec<f64> {
        keys.as_array()
            .unwrap()
            .iter()
            .map(|k| k["t"].as_f64().unwrap())
            .collect()
    }

    /// Presets key from and back to the layer's own values, eased, and
    /// merge with the keys it has: an exit after an entrance keeps both.
    #[test]
    fn motion_presets_land_on_the_layers_own_values_and_merge() {
        let mut l = layer(json!({"id": "l", "kind": "rect", "y": 300, "opacity": 0.8}));
        motion(&mut l, &mo("rise_in", 1., 0.5), 30.).unwrap();
        assert_eq!(ts(&l["y"]), [1., 1.5]);
        assert_eq!(
            (l["y"][0]["v"].as_f64(), l["y"][1]["v"].as_f64()),
            (Some(340.), Some(300.))
        );
        assert_eq!(
            (l["opacity"][0]["v"].as_f64(), l["opacity"][1]["v"].as_f64()),
            (Some(0.), Some(0.8))
        );
        // An ease out: the first handle carries the whole change early.
        assert_eq!(l["y"][0]["out"], json!([0.08, -40.]));
        motion(&mut l, &mo("fade_out", 4., 0.5), 30.).unwrap();
        assert_eq!(ts(&l["opacity"]), [1., 1.5, 4., 4.5]);
        assert_eq!(l["opacity"][2]["v"], 0.8);
        assert_eq!(l["opacity"][3]["v"], 0.);
        // Re-keying the same span replaces it rather than piling up.
        motion(&mut l, &mo("fade_out", 4., 0.5), 30.).unwrap();
        assert_eq!(ts(&l["opacity"]).len(), 4);
        // The result loads: keys sorted, handles inside their segments.
        let doc = json!({"size": [320, 180], "fps": 30, "scenes": [{"name": "s", "duration": 5, "layers": [Value::Object(l)]}]});
        Project::load(&doc.to_string()).unwrap();

        let mut l = layer(json!({"id": "l", "kind": "rect", "x": 100, "scale": 2}));
        motion(&mut l, &mo("pop_in", 0., 0.3), 30.).unwrap();
        let v: Vec<f64> = l["scale"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| k["v"].as_f64().unwrap())
            .collect();
        assert_eq!(v, [0., 2.16, 2.]);
        let mut m = mo("slide_in", 0., 0.4);
        m.dir = Some("right".into());
        motion(&mut l, &m, 30.).unwrap();
        assert_eq!(l["x"][0]["v"], 260.);
        // Float noise stays out of the file.
        let mut l = layer(json!({"id": "l", "kind": "text", "text": "hi"}));
        motion(&mut l, &mo("cascade", 0.2, 0.7), 30.).unwrap();
        assert_eq!(l["animators"][0]["amount"][1]["t"], 0.9);
    }

    #[test]
    fn motion_errors_say_what_would_work() {
        let mut l = layer(json!({"id": "l", "kind": "rect"}));
        let e = motion(&mut l, &mo("cascade", 0., 1.), 30.).unwrap_err();
        assert!(e.contains("not text or a duplicator"), "{e}");
        let e = motion(&mut l, &mo("fade_inn", 0., 1.), 30.).unwrap_err();
        assert!(e.contains("did you mean `fade_in`"), "{e}");
        let mut m = mo("slide_in", 0., 1.);
        m.dir = Some("sideways".into());
        assert!(
            motion(&mut l, &m, 30.)
                .unwrap_err()
                .contains("left, right, up or down")
        );
    }

    #[test]
    fn misses_suggest_the_nearest_name_or_list_them() {
        let v = json!({"scenes": [{"name": "intro", "layers": [{"id": "title"}, {"id": "logo"}]}]});
        let e = resolve(&v, "/scenes/intro/layers/titel/x").unwrap_err();
        assert!(e.contains("did you mean `title`"), "{e}");
        let e = resolve(&v, "/scenes/intro/layers/zzzzzzzz").unwrap_err();
        assert!(e.contains("there is title, logo"), "{e}");
        let e = scene_index(&v, Some("intor")).unwrap_err();
        assert!(e.contains("did you mean `intro`"), "{e}");
        let mut s = Server::default();
        let e = s.call("stil", json!({})).unwrap_err();
        assert!(e.contains("did you mean `still`"), "{e}");
        // Arguments are checked by name too, not silently dropped.
        let e = s.call("still", json!({"time": 1})).unwrap_err();
        assert!(e.contains("unknown field `time`"), "{e}");
    }

    #[test]
    fn plugin_layers_may_name_an_imported_source() {
        let spec = json!({"cargo": "../Cargo.toml", "example": "synth"});
        let mut v = json!({"sources": [{"id": "synth", "kind": "plugin", "source": spec}],
            "scenes": [{"layers": [{"id": "a", "kind": "plugin", "source": "synth"}, {"id": "b", "kind": "rect"}]}]});
        inline_sources(&mut v).unwrap();
        assert_eq!(v["scenes"][0]["layers"][0]["source"], spec);
        v["scenes"][0]["layers"][0]["source"] = "synht".into();
        let e = inline_sources(&mut v).unwrap_err();
        assert!(e.contains("did you mean `synth`"), "{e}");
    }

    #[test]
    fn tidy_rounds_floats_and_keeps_the_rest() {
        let v = tidy(json!({"a": [125.56818199157716, 3, "x"], "b": {"c": 0.30000000000000004}}));
        assert_eq!(v, json!({"a": [125.57, 3, "x"], "b": {"c": 0.3}}));
    }

    #[test]
    fn schema_gives_one_definition_on_request() {
        let mut s = Server::default();
        let c = s.call("schema", json!({"def": "Animator"})).unwrap();
        let v: Value = serde_json::from_str(c[0]["text"].as_str().unwrap()).unwrap();
        assert!(v["properties"]["stagger"].is_object(), "{v}");
        let e = s.call("schema", json!({"def": "Animater"})).unwrap_err();
        assert!(e.contains("did you mean `Animator`"), "{e}");
    }
}
