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

use crate::{Anim, Layer, is_one, is_zero, one, zero};

/// The capture cache, relative to the project file. A state's manifest is
/// `CACHE/<key>.json`, its images `CACHE/img/<hash>.png`.
pub const CACHE: &str = ".cut-cache";

/// How far an exploded part comes towards the viewer at `explode` 1, in the
/// plugin's pixels: for a 3D stage. The 2D renderer ignores depth.
pub const EXPLODE_DEPTH: f64 = 160.;

/// Where the plugin editor comes from: an adapter executable, or one built
/// from source with Cargo.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
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
}

impl Source {
    pub(crate) fn check(&self) -> Result<(), String> {
        match (self.cargo.is_empty(), self.bin.is_empty(), self.example.is_empty()) {
            (true, false, true) => Ok(()),
            (false, b, e) if b != e => Ok(()),
            _ => Err("`source` is {\"bin\": path}, or {\"cargo\": Cargo.toml, \"example\" or \"bin\": name}".into()),
        }
    }
}

/// One of the plugin's parameters, keyed: sent to the adapter as
/// `{"op": "set", "id": .., "field": .., "value": ..}` whenever it changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Param {
    /// The adapter's module id (a number or a string, as it names them).
    pub id: Value,
    pub field: String,
    pub value: Anim<f64>,
}

/// A part of the plugin's UI (a surface id the capture split out), moved
/// on its own: offsets from where the UI puts it, in the plugin's pixels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Part {
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub x: Anim<f64>,
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub y: Anim<f64>,
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
    pub fn nums(&self) -> [(&'static str, &Anim<f64>); 6] {
        [
            ("x", &self.x),
            ("y", &self.y),
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
    pub explode: f64,
    pub backdrop: f64,
    /// `[x, y, down]` in the plugin's pixels.
    pub pointer: [f64; 3],
    pub parts: Vec<PartAt>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PartAt {
    pub id: String,
    pub x: f64,
    pub y: f64,
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
    pub fn plugin_track(&self, fps: f64, last: usize) -> Vec<Step> {
        let crate::Kind::Plugin {
            source,
            select,
            params,
            ..
        } = &self.kind
        else {
            return Vec::new();
        };
        let src = serde_json::to_string(source).expect("a source serialises");
        let mut h = fnv(FNV_OFFSET, src.as_bytes());
        let mut steps = Vec::new();
        let mut last_params: Vec<f64> = Vec::new();
        let mut last_pointer = NO_POINTER;
        for f in 0..=last {
            let t = f as f64 / fps;
            let mut commands = Vec::new();
            if f == 0 && !select.is_empty() {
                commands.push(json!({"op": "input", "kind": "select", "ids": select}));
            }
            let values: Vec<f64> = params.iter().map(|p| round(p.value.at(t), 1e6)).collect();
            for (i, (p, v)) in params.iter().zip(&values).enumerate() {
                if last_params.get(i) != Some(v) {
                    commands.push(json!({"op": "set", "id": p.id, "field": p.field, "value": v}));
                }
            }
            last_params = values;
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

    /// `[x, y, down]`, x and y to a hundredth of a pixel, down 0 or 1.
    fn pointer_at(&self, t: f64) -> [f64; 3] {
        [
            round(self.pointer_x.at(t), 100.),
            round(self.pointer_y.at(t), 100.),
            f64::from(self.pointer_down.at(t) >= 0.5),
        ]
    }

    /// A plugin layer at `t`; `None` for other kinds.
    pub(crate) fn plugin_at(&self, t: f64, fps: f64) -> Option<PluginAt> {
        let crate::Kind::Plugin { parts, .. } = &self.kind else {
            return None;
        };
        let f = frame_at(t, fps);
        let state = self
            .plugin_track(fps, f)
            .pop()
            .map(|s| s.key)
            .unwrap_or_default();
        // Parameters and pointer are read on the grid, like the capture.
        let tq = f as f64 / fps;
        Some(PluginAt {
            state,
            explode: self.explode.at(t),
            backdrop: self.backdrop.at(t).clamp(0., 1.),
            pointer: self.pointer_at(tq),
            parts: parts
                .iter()
                .map(|(id, p)| PartAt {
                    id: id.clone(),
                    x: p.x.at(t),
                    y: p.y.at(t),
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
    /// The adapter build this came from; a rebuilt adapter recaptures.
    #[serde(default)]
    pub stamp: String,
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
    pub group: String,
    /// `[x, y, w, h]` in the UI's pixels.
    pub rect: [f64; 4],
    /// Relative to [`CACHE`].
    pub src: String,
}

impl Capture {
    /// Its parts' ids, in paint order, once each.
    pub fn parts(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for f in &self.fragments {
            if f.group != "background" && !out.contains(&f.group.as_str()) {
                out.push(&f.group);
            }
        }
        out
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
