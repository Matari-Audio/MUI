//! Plugin layers: a running MUI plugin editor, captured into parts.
//!
//! The editor runs as a `mui-motion-bridge` adapter (its own process, its
//! own real UI and model). The CLI drives it with the layer's keyed
//! parameters and pointer, and the bridge's `CaptureStream` hands back the
//! UI split into parts (`discover_parts`, or the ids in `select`) as PNGs
//! with their rects. Those captures are cached on disk under
//! [`CACHE`], one manifest per state, keyed by the source and every command
//! sent so far, so drawing stays a pure function of the scene and the time:
//! [`eval`](crate::eval) names the state, the renderer looks it up.
//!
//! Everything here builds for wasm; spawning the adapter is the CLI's
//! (`src/host.rs`).
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use mui_vello::kurbo::Affine;

use crate::{Anim, Layer, is_one, is_zero, one, zero};

/// The capture cache, relative to the project file. A state's manifest is
/// `CACHE/<key>.json`, its images `CACHE/img/<hash>.png`.
pub const CACHE: &str = ".cut-cache";

/// How far an exploded part comes towards the viewer at `explode` 1, in the
/// plugin's pixels. Only a 3D scene (`"mode": "3d"`) shows depth; there
/// every part is its own slab (see `Assets::slabs`).
pub const EXPLODE_DEPTH: f64 = 160.;

/// Where the plugin editor comes from: an adapter executable, or one built
/// from source with Cargo.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[schemars(transform = crate::vars::bindable)]
pub struct Source {
    /// A prebuilt adapter executable (relative to the project); with
    /// `cargo`, the package binary to build instead.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bin: String,
    /// A `Cargo.toml` (relative to the project) to build the adapter from.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cargo: String,
    /// With `cargo`: the example target to build.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub example: String,
    /// Arguments for the adapter.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// A plugin crate's folder (relative to the project) or git URL: its
    /// own editor, hosted headless by an adapter mui-cut generates and
    /// builds against its MUI (`mui-cut add`). No code in the plugin.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub plugin: String,
    /// With `plugin`: the plugin crate's Cargo features the adapter turns
    /// on, for a build that runs its DSP headless (a lab build, say).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
}

impl Source {
    pub(crate) fn check(&self) -> Result<(), String> {
        if !self.features.is_empty() && self.plugin.is_empty() {
            return Err("`features` go with a `plugin` source".into());
        }
        match (self.cargo.is_empty(), self.bin.is_empty(), self.example.is_empty(), self.plugin.is_empty()) {
            (true, false, true, true) | (true, true, true, false) => Ok(()),
            (false, b, e, true) if b != e => Ok(()),
            _ => Err("`source` is {\"plugin\": folder or git URL}, {\"bin\": path}, or {\"cargo\": Cargo.toml, \"example\" or \"bin\": name}".into()),
        }
    }
}

/// One of the plugin's parameters, keyed: sent to the adapter as
/// `{"op": "set", "id": .., "field": .., "value": ..}` whenever it changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(transform = crate::vars::bindable)]
pub struct Param {
    /// The adapter's module id (a number or a string, as it names them).
    pub id: Value,
    pub field: String,
    pub value: Anim<f64>,
}

/// A note played into a plugin layer: `t` seconds from the scene's start,
/// held `dur` seconds, MIDI `pitch` (60 is middle C) at velocity `vel`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Note {
    pub t: f64,
    pub dur: f64,
    pub pitch: u8,
    #[serde(default = "vel", skip_serializing_if = "is_vel")]
    pub vel: u8,
}
fn vel() -> u8 {
    100
}
fn is_vel(v: &u8) -> bool {
    *v == 100
}

/// Times finite and not negative, lengths above zero, MIDI numbers.
pub(crate) fn check_notes(notes: &[Note]) -> Result<(), String> {
    for (i, n) in notes.iter().enumerate() {
        if !(n.t.is_finite() && n.t >= 0.) {
            return Err(format!("[{i}].t: seconds, 0 or more"));
        }
        if !(n.dur.is_finite() && n.dur > 0.) {
            return Err(format!("[{i}].dur: seconds, more than 0"));
        }
        if n.pitch > 127 {
            return Err(format!("[{i}].pitch: 0..127, not {}", n.pitch));
        }
        if !(1..=127).contains(&n.vel) {
            return Err(format!("[{i}].vel: 1..127, not {}", n.vel));
        }
    }
    Ok(())
}

/// The sample a time falls on at `rate`.
pub fn sample_at(t: f64, rate: u32) -> u64 {
    (t * f64::from(rate)).round().max(0.) as u64
}

/// The notes as the bridge's timed events, `(sample, command)`, in the
/// order they play: by sample, a note's end before a new start.
pub fn note_events(notes: &[Note], rate: u32) -> Vec<(u64, Value)> {
    let mut ev: Vec<(u64, u8, Value)> = notes
        .iter()
        .flat_map(|n| {
            let on = sample_at(n.t, rate);
            let off = sample_at(n.t + n.dur, rate).max(on + 1);
            [
                (
                    on,
                    1,
                    json!({"at": on, "op": "note_on", "note": n.pitch, "velocity": n.vel}),
                ),
                (
                    off,
                    0,
                    json!({"at": off, "op": "note_off", "note": n.pitch}),
                ),
            ]
        })
        .collect();
    ev.sort_by_key(|e| (e.0, e.1));
    ev.into_iter().map(|(s, _, v)| (s, v)).collect()
}

/// `{"op": "advance"}` to sample `to` with the events in `[from, to)`.
pub fn advance(events: &[(u64, Value)], from: u64, to: u64) -> Value {
    let notes: Vec<&Value> = events
        .iter()
        .filter(|e| (from..to).contains(&e.0))
        .map(|e| &e.1)
        .collect();
    json!({"op": "advance", "to": to, "notes": notes})
}

/// A part of the plugin's UI (a surface id the capture split out), moved
/// on its own: offsets from where the UI puts it, in the plugin's pixels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(transform = crate::vars::bindable)]
pub struct Part {
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub x: Anim<f64>,
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub y: Anim<f64>,
    /// Depth from the UI's face, larger is farther, as a layer's `z`: 3D
    /// scenes only.
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub z: Anim<f64>,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub scale: Anim<f64>,
    /// Degrees, clockwise, about the part's centre.
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub rotation: Anim<f64>,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub opacity: Anim<f64>,
    /// 0..1: a flat outline around the part.
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub highlight: Anim<f64>,
}

impl Part {
    /// Its keyable numbers, by name, in inspector order.
    pub fn nums(&self) -> [(&'static str, &Anim<f64>); 7] {
        [
            ("x", &self.x),
            ("y", &self.y),
            ("z", &self.z),
            ("scale", &self.scale),
            ("rotation", &self.rotation),
            ("opacity", &self.opacity),
            ("highlight", &self.highlight),
        ]
    }
}

/// A plugin layer's state at one time: what the renderer needs besides
/// the layer's own transform.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PluginAt {
    /// The capture to show: `CACHE/<state>.json`.
    pub state: String,
    /// A component layer's parts (see [`shows`]); empty for the whole UI.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub show: Vec<String>,
    /// `explode` for each level of parts, panels first: the layer's
    /// `explode`, each level `explode_stagger` seconds behind the last.
    pub explode: Vec<f64>,
    pub backdrop: f64,
    /// `[x, y, down]` in the plugin's pixels.
    pub pointer: [f64; 3],
    pub parts: Vec<PartAt>,
}

impl PluginAt {
    /// Whether it draws the fragments of `group`: a component layer draws
    /// its parts alone, no backdrop.
    pub fn draws(&self, group: &str) -> bool {
        if group == "background" {
            self.show.is_empty()
        } else {
            shows(&self.show, group)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PartAt {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub scale: f64,
    pub rotation: f64,
    pub opacity: f64,
    pub highlight: f64,
}

/// A frame of the grid where the adapter is sent something, and the state
/// key once it has been.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub frame: usize,
    pub key: String,
    pub commands: Vec<Value>,
}

/// FNV-1a, 64 bits: stable across runs, platforms and Rust versions, which
/// std's hasher does not promise. Cache keys are written to disk.
pub fn fnv(seed: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(seed, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}
/// FNV-1a's starting state.
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// How deep the Sources panel captures a plugin's parts: its panels and
/// the controls in them.
pub const HOME_DEPTH: usize = 2;

/// The first step of a plugin fresh from its source, its parts captured
/// [`HOME_DEPTH`] deep: what the Sources panel shows, and the first frame
/// of a bare plugin layer with `explode_levels` 2.
pub fn home_step(source: &Source) -> Step {
    let select = json!({"op": "input", "kind": "select", "ids": [], "depth": HOME_DEPTH});
    let h = fnv(home_hash(source), select.to_string().as_bytes());
    Step {
        frame: 0,
        key: format!("{h:016x}"),
        commands: vec![select],
    }
}

/// The state [`home_step`] leaves.
pub fn home(source: &Source) -> String {
    home_step(source).key
}

/// A home capture's parts as [`tree_json`] shows them, nothing moved.
pub fn home_tree(cap: &Capture, state: &str) -> Value {
    let at = PluginAt {
        state: state.into(),
        show: Vec::new(),
        explode: vec![0.],
        backdrop: 1.,
        pointer: NO_POINTER,
        parts: Vec::new(),
    };
    tree_json(cap, &at)["parts"].take()
}
fn home_hash(source: &Source) -> u64 {
    let src = serde_json::to_string(source).expect("a source serialises");
    fnv(FNV_OFFSET, src.as_bytes())
}

/// Whether a component layer showing `show` draws part `id`: one it names,
/// or one nested under it (`panel/knob` under `panel`). Everything shows
/// when `show` is empty.
pub fn shows(show: &[String], id: &str) -> bool {
    show.is_empty()
        || show.iter().any(|s| {
            id.strip_prefix(s.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        })
}

/// The grid frame a time falls in: a plugin's UI changes once a frame.
pub fn frame_at(t: f64, fps: f64) -> usize {
    (t * fps + 1e-6).floor().max(0.) as usize
}

/// The pointer's rest state: off the UI, no button.
const NO_POINTER: [f64; 3] = [-1., -1., 0.];

impl Layer {
    /// A plugin layer's commands, from frame 0 to `last`, at the frames where
    /// something changed, each with the state key it leaves: the key hashes
    /// the source and every command before it, so two times share a
    /// capture exactly when the adapter was told the same things. Empty for
    /// other kinds.
    // ponytail: eval walks the grid from 0 for every time (O(frames) per
    // eval); memoise per layer if long scenes make eval show up in a profile.
    ///
    /// A layer with notes runs on the adapter's sample clock: every frame
    /// first advances it to the frame's sample (at `rate`), playing the
    /// notes that fall before it, so each frame is a state of its own and
    /// the UI shows what the sound is doing.
    pub fn plugin_track(&self, fps: f64, rate: u32, last: usize) -> Vec<Step> {
        let crate::Kind::Plugin {
            source,
            select,
            params,
            parts,
            explode_levels,
            show,
            notes,
            preset,
            ..
        } = &self.kind
        else {
            return Vec::new();
        };
        // Parts come as deep as explode reaches, or a keyed part's path.
        // ponytail: a surface id with a `/` in it counts as deeper; that only
        // captures a level more than needed, up to the 8 an adapter serves.
        let depth = parts
            .keys()
            .chain(show)
            .map(|k| k.split('/').count())
            .chain([*explode_levels as usize])
            .max()
            .unwrap_or(1)
            .min(8);
        let mut h = home_hash(source);
        let mut steps = Vec::new();
        let mut last_params: Vec<f64> = Vec::new();
        let mut last_pointer = NO_POINTER;
        let mut last_view = [0.; 2];
        let events = note_events(notes, rate);
        let mut clock = 0;
        for f in 0..=last {
            let t = f as f64 / fps;
            let mut commands = Vec::new();
            if !notes.is_empty() && f > 0 {
                let to = sample_at(t, rate);
                commands.push(advance(&events, clock, to));
                clock = to;
            }
            if f == 0 && depth > 1 {
                commands
                    .push(json!({"op": "input", "kind": "select", "ids": select, "depth": depth}));
            } else if f == 0 && !select.is_empty() {
                commands.push(json!({"op": "input", "kind": "select", "ids": select}));
            }
            // ponytail: the key hashes the preset's path, not its bytes; an
            // edited preset file needs `.cut-cache` cleared to re-capture.
            if f == 0 && !preset.is_empty() {
                commands.push(json!({"op": "preset", "path": preset}));
            }
            let values: Vec<f64> = params.iter().map(|p| round(p.value.at(t), 1e6)).collect();
            for (i, (p, v)) in params.iter().zip(&values).enumerate() {
                if last_params.get(i) != Some(v) {
                    commands.push(json!({"op": "set", "id": p.id, "field": p.field, "value": v}));
                }
            }
            last_params = values;
            let view = self.view_at(t);
            if view != last_view {
                commands.push(
                    json!({"op": "input", "kind": "view", "width": view[0], "height": view[1]}),
                );
                last_view = view;
            }
            let pointer = self.pointer_at(t);
            if pointer != last_pointer {
                commands.push(json!({
                    "op": "input", "kind": "pointer",
                    "x": pointer[0], "y": pointer[1], "buttons": u8::from(pointer[2] > 0.),
                }));
                last_pointer = pointer;
            }
            if f == 0 || !commands.is_empty() {
                for c in &commands {
                    h = fnv(h, c.to_string().as_bytes());
                }
                steps.push(Step {
                    frame: f,
                    key: format!("{h:016x}"),
                    commands,
                });
            }
        }
        steps
    }

    /// A plugin layer with notes: its soundtrack's cache key, and the last
    /// advance, to `samples`, that finishes it after `track` (its steps,
    /// as the capture replays them). `None` for a silent layer.
    pub fn plugin_audio(
        &self,
        track: &[Step],
        fps: f64,
        rate: u32,
        samples: u64,
    ) -> Option<(String, Value)> {
        let crate::Kind::Plugin { notes, .. } = &self.kind else {
            return None;
        };
        let step = track.last().filter(|_| !notes.is_empty())?;
        let from = sample_at(step.frame as f64 / fps, rate);
        let tail = advance(&note_events(notes, rate), from, samples.max(from));
        let h = fnv(
            fnv(FNV_OFFSET, step.key.as_bytes()),
            tail.to_string().as_bytes(),
        );
        Some((format!("{h:016x}"), tail))
    }

    /// `[width, height]` the editor is laid out at, whole pixels; `[0, 0]`
    /// (either at 0 or less) is the plugin's own size.
    fn view_at(&self, t: f64) -> [f64; 2] {
        let [w, h] = [
            self.view_width.at(t).round(),
            self.view_height.at(t).round(),
        ];
        if w > 0. && h > 0. {
            [w.max(8.), h.max(8.)]
        } else {
            [0., 0.]
        }
    }

    /// `[x, y, down]`, x and y to a hundredth of a pixel, down 0 or 1.
    fn pointer_at(&self, t: f64) -> [f64; 3] {
        [
            round(self.pointer_x.at(t), 100.),
            round(self.pointer_y.at(t), 100.),
            f64::from(self.pointer_down.at(t) >= 0.5),
        ]
    }

    /// A plugin layer at `t`; `None` for other kinds.
    pub(crate) fn plugin_at(&self, t: f64, fps: f64, rate: u32) -> Option<PluginAt> {
        let crate::Kind::Plugin {
            parts,
            explode_levels,
            explode_stagger,
            show,
            ..
        } = &self.kind
        else {
            return None;
        };
        let f = frame_at(t, fps);
        let state = self
            .plugin_track(fps, rate, f)
            .pop()
            .map(|s| s.key)
            .unwrap_or_default();
        // Parameters and pointer are read on the grid, like the capture.
        let tq = f as f64 / fps;
        Some(PluginAt {
            state,
            show: show.clone(),
            explode: (0..*explode_levels)
                .map(|i| self.explode.at(t - f64::from(i) * explode_stagger))
                .collect(),
            backdrop: self.backdrop.at(t).clamp(0., 1.),
            pointer: self.pointer_at(tq),
            parts: parts
                .iter()
                .map(|(id, p)| PartAt {
                    id: id.clone(),
                    x: p.x.at(t),
                    y: p.y.at(t),
                    z: p.z.at(t),
                    scale: p.scale.at(t),
                    rotation: p.rotation.at(t),
                    opacity: p.opacity.at(t).clamp(0., 1.),
                    highlight: p.highlight.at(t).clamp(0., 1.),
                })
                .collect(),
        })
    }
}

fn round(v: f64, k: f64) -> f64 {
    (v * k).round() / k
}

/// How an exploded part moves: away from the UI's centre by `amount` times
/// its distance from it, and `amount * EXPLODE_DEPTH` towards the viewer.
/// At 0 every part is where the UI put it.
pub fn explode(centre: [f64; 2], part: [f64; 2], amount: f64) -> [f64; 3] {
    [
        (part[0] - centre[0]) * amount,
        (part[1] - centre[1]) * amount,
        amount * EXPLODE_DEPTH,
    ]
}

/// One captured state: the UI's size in its own pixels and its fragments,
/// bottom first. A part's fragment carries its surface id as `group`;
/// everything else is `background`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Capture {
    pub width: f64,
    pub height: f64,
    #[serde(rename = "layers")]
    pub fragments: Vec<Fragment>,
    /// Every surface of the UI: what `select` and the pointer can aim at.
    #[serde(default)]
    pub surfaces: Vec<Surface>,
    /// The parts as a tree, parents first. Older captures lack it: then
    /// each fragment's group is a part of its own (see [`Capture::tree`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<PartInfo>,
    /// The adapter build this came from; a rebuilt adapter recaptures.
    #[serde(default)]
    pub stamp: String,
    /// What the plugin says its patch is (parameters, modulation routes),
    /// for a patch layer; null when it says nothing.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub patch: Value,
}

/// A resolved surface of the plugin's UI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Surface {
    pub id: String,
    #[serde(default)]
    pub parent: Option<String>,
    /// `[x, y, w, h]` in the UI's pixels.
    pub frame: [f64; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fragment {
    /// The part's path (`panel/knob`), or `background`.
    pub group: String,
    /// `[x, y, w, h]` in the UI's pixels.
    pub rect: [f64; 4],
    /// Relative to [`CACHE`].
    pub src: String,
    /// The same paint pulled out of its ancestors' clips, when a clip cut
    /// it: drawn instead once the part moves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub free: Option<Image>,
}

/// A captured image and where it sits, `[x, y, w, h]` in the UI's pixels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Image {
    pub src: String,
    pub rect: [f64; 4],
}

/// A part of the UI: a surface split out, its path the part ids from the
/// outermost down joined by `/` (`osc`, `osc/osc-shape`), keyed as
/// `parts.<path>.x`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartInfo {
    pub path: String,
    /// Its surface id.
    pub id: String,
    /// Its parent part's path; none at the top level.
    #[serde(default)]
    pub parent: Option<String>,
    /// `[x, y, w, h]` in the UI's pixels: its surface's frame.
    pub frame: [f64; 4],
}

impl Capture {
    /// The parts, parents first: the manifest's tree, or for an older
    /// capture one top-level part per group, framed by its fragments.
    pub fn tree(&self) -> Vec<PartInfo> {
        if !self.parts.is_empty() {
            return self.parts.clone();
        }
        let mut out: Vec<PartInfo> = Vec::new();
        for f in self.fragments.iter().filter(|f| f.group != "background") {
            let [x, y, w, h] = f.rect;
            match out.iter_mut().find(|p| p.path == f.group) {
                Some(p) => {
                    let [px, py, pw, ph] = p.frame;
                    let (x0, y0) = (px.min(x), py.min(y));
                    let (x1, y1) = ((px + pw).max(x + w), (py + ph).max(y + h));
                    p.frame = [x0, y0, x1 - x0, y1 - y0];
                }
                None => out.push(PartInfo {
                    path: f.group.clone(),
                    id: f.group.clone(),
                    parent: None,
                    frame: f.rect,
                }),
            }
        }
        out
    }
}

/// Where a part is at one time, as the renderers draw it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pose {
    /// From the UI's pixels to where this part puts them (in the UI's
    /// pixels): its parents' motion, then its own.
    pub at: Affine,
    /// Its and its parents' turns, degrees, and scales.
    pub rotation: f64,
    pub scale: f64,
    pub opacity: f64,
    /// Towards the viewer from the UI's face, in the UI's pixels: every
    /// level a pixel and its explode's depth in front of its parent.
    pub depth: f64,
    pub highlight: f64,
    /// 1 for panels, 2 for their controls, ...
    pub level: usize,
    /// Off where the UI put it (it or a parent moved): drawn free of its
    /// ancestors' clips.
    pub moved: bool,
}

/// Every part's pose, parents first. A part moves with its parent; at its
/// level `explode` pulls it away from its parent's centre (the UI's, at the
/// top) and towards the viewer, then its own tracks move it about its
/// centre. Deeper than `explode` reaches, a part rides with its parent.
pub fn poses(cap: &Capture, p: &PluginAt) -> Vec<(PartInfo, Pose)> {
    let root = Pose {
        at: Affine::IDENTITY,
        rotation: 0.,
        scale: 1.,
        opacity: 1.,
        depth: 0.,
        highlight: 0.,
        level: 0,
        moved: false,
    };
    let centre = |f: [f64; 4]| [f[0] + f[2] / 2., f[1] + f[3] / 2.];
    let mut out: Vec<(PartInfo, Pose)> = Vec::new();
    for info in cap.tree() {
        let (parent, pc) = match info
            .parent
            .as_ref()
            .and_then(|q| out.iter().find(|(i, _)| &i.path == q))
        {
            Some((i, pose)) => (pose.clone(), centre(i.frame)),
            None => (root.clone(), [cap.width / 2., cap.height / 2.]),
        };
        let level = parent.level + 1;
        let amount = p.explode.get(level - 1).copied().unwrap_or(0.);
        let c = centre(info.frame);
        let [ex, ey, ez] = explode(pc, c, amount);
        let own = p.parts.iter().find(|q| q.id == info.path);
        let (dx, dy, dz, s, r, o, hl) = own.map_or((0., 0., 0., 1., 0., 1., 0.), |q| {
            (q.x, q.y, q.z, q.scale, q.rotation, q.opacity, q.highlight)
        });
        let local = Affine::translate((c[0] + ex + dx, c[1] + ey + dy))
            * Affine::rotate(r.to_radians())
            * Affine::scale(s)
            * Affine::translate((-c[0], -c[1]));
        let moved = parent.moved || amount != 0. || [dx, dy, r] != [0., 0., 0.] || s != 1.;
        let pose = Pose {
            at: parent.at * local,
            rotation: parent.rotation + r,
            scale: parent.scale * s,
            opacity: parent.opacity * o,
            // A pixel proud of its parent even when collapsed: coplanar
            // slabs shadow each other in speckles.
            depth: parent.depth + ez - dz + 1.,
            highlight: hl,
            level,
            moved,
        };
        out.push((info, pose));
    }
    out
}

/// A plugin layer's parts at one time as a JSON tree, the one shape the
/// editor, the MCP `plugin_parts` tool and a sources panel read: per part
/// `id` (its path, keyed as `parts.<id>.x`), `surface`, `level`, `frame`
/// and `rects` (UI pixels), `thumb` (its largest image, a project asset
/// path), `motion` (its own tracks at the time) and `children`.
pub fn tree_json(cap: &Capture, at: &PluginAt) -> Value {
    fn nodes(
        tree: &[PartInfo],
        cap: &Capture,
        at: &PluginAt,
        parent: Option<&str>,
        level: usize,
    ) -> Vec<Value> {
        tree.iter()
            .filter(|p| p.parent.as_deref() == parent)
            .map(|p| {
                let own: Vec<&Fragment> =
                    cap.fragments.iter().filter(|f| f.group == p.path).collect();
                let thumb = own
                    .iter()
                    .max_by(|a, b| (a.rect[2] * a.rect[3]).total_cmp(&(b.rect[2] * b.rect[3])))
                    .map(|f| format!("{CACHE}/{}", f.src));
                json!({
                    "id": p.path,
                    "surface": p.id,
                    "level": level,
                    "frame": p.frame,
                    "rects": own.iter().map(|f| f.rect).collect::<Vec<_>>(),
                    "thumb": thumb,
                    "motion": at.parts.iter().find(|q| q.id == p.path),
                    "children": nodes(tree, cap, at, Some(&p.path), level + 1),
                })
            })
            .collect()
    }
    json!({
        "state": at.state,
        "size": [cap.width, cap.height],
        "explode": at.explode,
        "pointer": at.pointer,
        "parts": nodes(&cap.tree(), cap, at, None, 1),
        "surfaces": cap.surfaces,
    })
}

impl Fragment {
    /// The image to draw and where, for a part posed `moved` or not.
    pub fn image(&self, moved: bool) -> (&str, [f64; 4]) {
        match &self.free {
            Some(f) if moved => (&f.src, f.rect),
            _ => (&self.src, self.rect),
        }
    }
}

/// The part tracks keyed by path, as [`Layer::props`] lists them.
pub(crate) fn part_props(parts: &BTreeMap<String, Part>) -> Vec<(String, &Anim<f64>)> {
    parts
        .iter()
        .flat_map(|(id, p)| {
            p.nums()
                .into_iter()
                .map(move |(n, a)| (format!("parts.{id}.{n}"), a))
        })
        .collect()
}
