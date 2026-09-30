//! mui-cut: a small keyframe motion editor on MUI.
//!
//! The project is plain JSON (`*.cut.json`, see the README), so a person in
//! the web editor and an agent with a text editor edit the same file. This
//! crate is the document, a pure evaluator ([`eval`]: scene + time in, a
//! [`Frame`] out, seekable) and a MUI/Vello CPU renderer ([`Renderer`]) that
//! the CLI and the browser both draw through, so a still, a render and the
//! editor viewport are the same pixels.
#![forbid(unsafe_code)]

pub mod check;
mod gpu;
mod gpu3d;
mod motion;
#[cfg(not(target_arch = "wasm32"))]
mod pool;
mod render;
mod sparse;
mod three;
mod vector;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use gpu::Offline;
pub use gpu::{Engine, GpuCanvas};
pub use motion::{
    ANIMATOR_PROPS, Animator, Deform, Deformer, Ease, Falloff, Fx, Order, Unit, text_units,
};
#[cfg(not(target_arch = "wasm32"))]
pub use pool::{CpuPool, shutter};
pub use render::{Assets, Layers, Quad, Renderer};
pub use three::{Cam, Fog, Ground, Lamp, Mode, View};

use serde::{Deserialize, Serialize};

/// The whole file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Project {
    /// The JSON Schema this file validates against (`mui-cut schema`), kept
    /// as written so editors and agents can find it.
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Output pixels, `[width, height]`; layer coordinates are in these.
    pub size: [u32; 2],
    pub fps: f64,
    pub scenes: Vec<Scene>,
}

/// One shot. Scenes play back to back in a render.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Scene {
    pub name: String,
    /// Seconds.
    pub duration: f64,
    #[serde(default = "Rgba::bg")]
    pub background: Rgba,
    /// Bottom first: later layers paint over earlier ones.
    #[serde(default)]
    pub layers: Vec<Layer>,
    /// `3d` sets every layer in a lit 3D space seen through the scene's
    /// camera layer; `2d` (the default) is the flat composite.
    #[serde(default, skip_serializing_if = "is_default")]
    pub mode: Mode,
    /// 3D: a floor under the layers, catching their shadows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ground: Option<Ground>,
    /// 3D: distance fog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fog: Option<Fog>,
}

/// What a layer draws.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Kind {
    Rect,
    Ellipse,
    /// Inter, one line per `\n`, drawn as outlines so each glyph can move.
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "is_default")]
        align: Align,
    },
    /// A PNG, path relative to the project file.
    Image {
        path: String,
    },
    /// SVG path data (`M 0 0 C ...`) in pixels around the layer's origin.
    Path {
        d: String,
    },
    /// `count` copies of a rect, an ellipse or path data `d`, laid out on a
    /// grid, a ring, a line or along path data `along`.
    Duplicator {
        #[serde(default, skip_serializing_if = "is_default")]
        shape: Shape,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        d: String,
        #[serde(default, skip_serializing_if = "is_default")]
        layout: Layout,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        along: String,
        /// Turn each copy with the ring or path it sits on.
        #[serde(default, skip_serializing_if = "is_default")]
        orient: bool,
    },
    /// An SVG file (relative to the project), drawn as vectors, centred.
    Svg {
        path: String,
    },
    /// A Lottie JSON file (relative to the project), centred. It plays at
    /// `speed` from the layer's `time` (seconds into the animation, a
    /// keyable property, so keys on it remap time), looping unless
    /// `"loop": false`.
    Lottie {
        path: String,
        #[serde(default = "one_f", skip_serializing_if = "is_one_f")]
        speed: f64,
        #[serde(default = "yes", rename = "loop", skip_serializing_if = "is_yes")]
        looped: bool,
    },
    /// 3D scenes: the camera. It orbits its target (`x`, `y`, `z`, or the
    /// layer `look_at`) by `ry` (yaw) and `rx` (pitch) at `distance`, rolls
    /// by `rotation`, and moves along `path` (SVG path data seen from
    /// above: x across, y into depth) by `path_offset`. The last camera
    /// with some opacity is the one that shoots.
    Camera {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        look_at: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        path: String,
    },
    /// 3D scenes: a light, coloured by `fill`.
    Light {
        #[serde(rename = "type", default, skip_serializing_if = "is_default")]
        light: LightType,
    },
    /// 3D scenes: a glTF binary (`.glb`, relative to the project), scaled
    /// to `height` pixels tall.
    Model {
        path: String,
    },
}

/// What a light layer is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LightType {
    /// Parallel rays along `rx`/`ry`, with a shadow.
    #[default]
    Directional,
    /// From its position, a cone along `rx`/`ry`, with a shadow.
    Spot,
    /// From its position every way, no shadow.
    Point,
    /// Everywhere, no shadow.
    Ambient,
}

/// Text line alignment.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Left,
    #[default]
    Center,
    Right,
}

/// A duplicator's copy.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    #[default]
    Rect,
    Ellipse,
    /// The duplicator's `d`.
    Path,
}

/// Where a duplicator's copies go, centred on the layer's origin.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    /// `columns` wide, `spacing_x` by `spacing_y` apart.
    #[default]
    Grid,
    /// Around a circle of `ring_radius`, the first at twelve o'clock.
    Radial,
    /// In a row `spacing_x` apart.
    Linear,
    /// Evenly along `along` by length, shifted by `path_offset` (0..1).
    Path,
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}
fn one_f() -> f64 {
    1.
}
fn is_one_f(v: &f64) -> bool {
    *v == 1.
}
fn yes() -> bool {
    true
}
fn is_yes(v: &bool) -> bool {
    *v
}

/// One layer. Every property is either a plain value or a list of keys; `x`
/// and `y` are the layer's centre, which is also its rotation and scale pivot.
/// A property left out is its default, and a save leaves defaults out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Layer {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(flatten)]
    pub kind: Kind,
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub x: Anim<f64>,
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub y: Anim<f64>,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub scale: Anim<f64>,
    /// Degrees, clockwise.
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub rotation: Anim<f64>,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub opacity: Anim<f64>,
    /// Ignored by text, which is as wide as its words.
    #[serde(default = "hundred", skip_serializing_if = "is_hundred")]
    pub width: Anim<f64>,
    #[serde(default = "hundred", skip_serializing_if = "is_hundred")]
    pub height: Anim<f64>,
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub radius: Anim<f64>,
    #[serde(default = "white", skip_serializing_if = "is_white")]
    pub fill: Anim<Rgba>,
    #[serde(default = "font_size", skip_serializing_if = "is_font_size")]
    pub font_size: Anim<f64>,
    #[serde(default = "weight", skip_serializing_if = "is_weight")]
    pub weight: Anim<f64>,
    /// Text: line pitch as a multiple of `font_size`.
    #[serde(default = "line_height", skip_serializing_if = "is_line_height")]
    pub line_height: Anim<f64>,
    /// Text: extra pixels after every glyph.
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub tracking: Anim<f64>,
    /// Vector kinds: outline colour and width (0 draws none).
    #[serde(default = "clear", skip_serializing_if = "is_clear")]
    pub stroke: Anim<Rgba>,
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub stroke_width: Anim<f64>,
    /// Vector kinds: keep only `trim_start..trim_end` (fractions of each
    /// contour's length), shifted by `trim_offset`, wrapping.
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub trim_start: Anim<f64>,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub trim_end: Anim<f64>,
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub trim_offset: Anim<f64>,
    /// Duplicator: copies (rounded), grid columns, spacing, ring radius and
    /// the shift along `along` (0..1).
    #[serde(default = "count", skip_serializing_if = "is_count")]
    pub count: Anim<f64>,
    #[serde(default = "columns", skip_serializing_if = "is_columns")]
    pub columns: Anim<f64>,
    #[serde(default = "spacing", skip_serializing_if = "is_spacing")]
    pub spacing_x: Anim<f64>,
    #[serde(default = "spacing", skip_serializing_if = "is_spacing")]
    pub spacing_y: Anim<f64>,
    #[serde(default = "ring", skip_serializing_if = "is_ring")]
    pub ring_radius: Anim<f64>,
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub path_offset: Anim<f64>,
    /// Lottie: seconds into the animation at the scene's start.
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub time: Anim<f64>,
    /// Text glyphs and duplicator copies, applied in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub animators: Vec<Animator>,
    /// Vector kinds, applied in order after everything else.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deformers: Vec<Deformer>,
    /// 3D: depth in pixels (larger is farther), turns in degrees (`rx`
    /// tips the top away, `ry` turns the right side away; `rotation` is
    /// the turn about z), the pivot's depth behind the face, and the slab
    /// thickness with its wall colour.
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub z: Anim<f64>,
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub rx: Anim<f64>,
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub ry: Anim<f64>,
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub anchor_z: Anim<f64>,
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub extrude: Anim<f64>,
    #[serde(default = "edge", skip_serializing_if = "is_edge")]
    pub edge: Anim<Rgba>,
    /// 3D: this layer shadows others, and others shadow it. On a light:
    /// it casts shadow maps at all.
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub cast_shadows: bool,
    #[serde(default = "yes", skip_serializing_if = "is_yes")]
    pub receive_shadows: bool,
    /// Camera: distance from its target (0: where a layer at z 0 is its 2D
    /// size), vertical field of view in degrees, the fraction of the way
    /// to the target it has moved in, depth of field's focus (pixels past
    /// the target) and aperture (blur pixels far out of focus; 0 none).
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub distance: Anim<f64>,
    #[serde(default = "fov", skip_serializing_if = "is_fov")]
    pub fov: Anim<f64>,
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub dolly: Anim<f64>,
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub focus: Anim<f64>,
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub aperture: Anim<f64>,
    /// Light: brightness, a spot's half-angle and soft fraction, the
    /// distance it fades out over (0: never) and the shadow's blur.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub intensity: Anim<f64>,
    #[serde(default = "cone", skip_serializing_if = "is_cone")]
    pub cone: Anim<f64>,
    #[serde(default = "feather", skip_serializing_if = "is_feather")]
    pub feather: Anim<f64>,
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub range: Anim<f64>,
    #[serde(default = "softness", skip_serializing_if = "is_softness")]
    pub softness: Anim<f64>,
}

fn edge() -> Anim<Rgba> {
    Anim::Value(Rgba([40, 41, 50, 255]))
}
fn is_edge(a: &Anim<Rgba>) -> bool {
    *a == edge()
}
fn fov() -> Anim<f64> {
    Anim::Value(40.)
}
fn is_fov(a: &Anim<f64>) -> bool {
    *a == fov()
}
fn cone() -> Anim<f64> {
    Anim::Value(30.)
}
fn is_cone(a: &Anim<f64>) -> bool {
    *a == cone()
}
fn feather() -> Anim<f64> {
    Anim::Value(0.3)
}
fn is_feather(a: &Anim<f64>) -> bool {
    *a == feather()
}
fn softness() -> Anim<f64> {
    Anim::Value(1.5)
}
fn is_softness(a: &Anim<f64>) -> bool {
    *a == softness()
}

fn line_height() -> Anim<f64> {
    Anim::Value(1.2)
}
fn is_line_height(a: &Anim<f64>) -> bool {
    *a == line_height()
}
fn clear() -> Anim<Rgba> {
    Anim::Value(Rgba([0; 4]))
}
fn is_clear(a: &Anim<Rgba>) -> bool {
    *a == clear()
}
fn count() -> Anim<f64> {
    Anim::Value(12.)
}
fn is_count(a: &Anim<f64>) -> bool {
    *a == count()
}
fn columns() -> Anim<f64> {
    Anim::Value(4.)
}
fn is_columns(a: &Anim<f64>) -> bool {
    *a == columns()
}
fn spacing() -> Anim<f64> {
    Anim::Value(120.)
}
fn is_spacing(a: &Anim<f64>) -> bool {
    *a == spacing()
}
fn ring() -> Anim<f64> {
    Anim::Value(200.)
}
fn is_ring(a: &Anim<f64>) -> bool {
    *a == ring()
}

fn zero() -> Anim<f64> {
    Anim::Value(0.)
}
fn one() -> Anim<f64> {
    Anim::Value(1.)
}
fn hundred() -> Anim<f64> {
    Anim::Value(100.)
}
fn font_size() -> Anim<f64> {
    Anim::Value(64.)
}
fn weight() -> Anim<f64> {
    Anim::Value(600.)
}
fn white() -> Anim<Rgba> {
    Anim::Value(Rgba([255; 4]))
}
fn is_zero(a: &Anim<f64>) -> bool {
    *a == zero()
}
fn is_one(a: &Anim<f64>) -> bool {
    *a == one()
}
fn is_hundred(a: &Anim<f64>) -> bool {
    *a == hundred()
}
fn is_white(a: &Anim<Rgba>) -> bool {
    *a == white()
}
fn is_font_size(a: &Anim<f64>) -> bool {
    *a == font_size()
}
fn is_weight(a: &Anim<f64>) -> bool {
    *a == weight()
}

/// A keyable property: a number or a colour.
#[derive(Clone, Copy, Debug)]
pub enum Prop<'a> {
    Num(&'a Anim<f64>),
    Color(&'a Anim<Rgba>),
}

impl Layer {
    /// Every property that means something for this layer's kind, by path
    /// (`x`, `fill`, `animators.0.offset`, `deformers.1.angle`), in
    /// inspector order. The editor's inspector, timeline and graph list
    /// these, so a new property shows up there by being listed here.
    pub fn props(&self) -> Vec<(String, Prop<'_>)> {
        self.props_in(false)
    }

    /// [`Layer::props`], with the 3D placement ones (`z`, `rx`, ...) too
    /// when the layer is in a 3D scene. Cameras, lights and models always
    /// list theirs.
    pub fn props_in(&self, three: bool) -> Vec<(String, Prop<'_>)> {
        use Prop::{Color, Num};
        let mut out: Vec<(String, Prop<'_>)> = Vec::new();
        let mut num = |n: &str, a| out.push((n.to_owned(), Num(a)));
        match self.kind {
            Kind::Camera { .. } => {
                for (n, a) in [
                    ("x", &self.x),
                    ("y", &self.y),
                    ("z", &self.z),
                    ("rx", &self.rx),
                    ("ry", &self.ry),
                    ("rotation", &self.rotation),
                    ("distance", &self.distance),
                    ("fov", &self.fov),
                    ("dolly", &self.dolly),
                    ("path_offset", &self.path_offset),
                    ("focus", &self.focus),
                    ("aperture", &self.aperture),
                    ("opacity", &self.opacity),
                ] {
                    num(n, a);
                }
                return out;
            }
            Kind::Light { .. } => {
                for (n, a) in [
                    ("x", &self.x),
                    ("y", &self.y),
                    ("z", &self.z),
                    ("rx", &self.rx),
                    ("ry", &self.ry),
                    ("intensity", &self.intensity),
                    ("cone", &self.cone),
                    ("feather", &self.feather),
                    ("range", &self.range),
                    ("softness", &self.softness),
                    ("opacity", &self.opacity),
                ] {
                    num(n, a);
                }
                out.push(("fill".into(), Color(&self.fill)));
                return out;
            }
            Kind::Model { .. } => {
                for (n, a) in [
                    ("x", &self.x),
                    ("y", &self.y),
                    ("z", &self.z),
                    ("rx", &self.rx),
                    ("ry", &self.ry),
                    ("rotation", &self.rotation),
                    ("scale", &self.scale),
                    ("height", &self.height),
                    ("opacity", &self.opacity),
                ] {
                    num(n, a);
                }
                out.push(("fill".into(), Color(&self.fill)));
                return out;
            }
            _ => {}
        }
        num("x", &self.x);
        num("y", &self.y);
        num("scale", &self.scale);
        num("rotation", &self.rotation);
        num("opacity", &self.opacity);
        let sized = matches!(
            self.kind,
            Kind::Rect | Kind::Ellipse | Kind::Image { .. } | Kind::Duplicator { .. }
        );
        if sized {
            num("width", &self.width);
            num("height", &self.height);
        }
        if matches!(
            self.kind,
            Kind::Rect | Kind::Image { .. } | Kind::Duplicator { .. }
        ) {
            num("radius", &self.radius);
        }
        if let Kind::Text { .. } = self.kind {
            num("font_size", &self.font_size);
            num("weight", &self.weight);
            num("line_height", &self.line_height);
            num("tracking", &self.tracking);
        }
        if let Kind::Duplicator { .. } = self.kind {
            num("count", &self.count);
            num("columns", &self.columns);
            num("spacing_x", &self.spacing_x);
            num("spacing_y", &self.spacing_y);
            num("ring_radius", &self.ring_radius);
            num("path_offset", &self.path_offset);
        }
        if let Kind::Lottie { .. } = self.kind {
            num("time", &self.time);
        }
        let vector = self.vector();
        let own_paint = matches!(self.kind, Kind::Svg { .. } | Kind::Lottie { .. });
        if vector && !own_paint {
            num("stroke_width", &self.stroke_width);
        }
        if vector {
            num("trim_start", &self.trim_start);
            num("trim_end", &self.trim_end);
            num("trim_offset", &self.trim_offset);
        }
        if !own_paint {
            out.push(("fill".into(), Color(&self.fill)));
        }
        if vector && !own_paint {
            out.push(("stroke".into(), Color(&self.stroke)));
        }
        if matches!(self.kind, Kind::Text { .. } | Kind::Duplicator { .. }) {
            for (i, a) in self.animators.iter().enumerate() {
                for n in ANIMATOR_PROPS {
                    let a = a.num(n).expect("ANIMATOR_PROPS are props");
                    out.push((format!("animators.{i}.{n}"), Num(a)));
                }
                out.push((format!("animators.{i}.fill"), Color(&a.fill)));
            }
        }
        if vector {
            for (i, d) in self.deformers.iter().enumerate() {
                for (n, a) in d.nums() {
                    out.push((format!("deformers.{i}.{n}"), Num(a)));
                }
            }
        }
        if three {
            for (n, a) in [
                ("z", &self.z),
                ("rx", &self.rx),
                ("ry", &self.ry),
                ("anchor_z", &self.anchor_z),
                ("extrude", &self.extrude),
            ] {
                out.push((n.into(), Num(a)));
            }
            out.push(("edge".into(), Color(&self.edge)));
        }
        out
    }

    /// A numeric property by its path, as [`Layer::props`] names it.
    pub fn prop(&self, path: &str) -> Option<&Anim<f64>> {
        self.props_in(true).into_iter().find_map(|(n, p)| match p {
            Prop::Num(a) if n == path => Some(a),
            _ => None,
        })
    }

    /// Drawn as outlines through the vector pipeline (trim, deformers).
    pub fn vector(&self) -> bool {
        !matches!(
            self.kind,
            Kind::Rect
                | Kind::Ellipse
                | Kind::Image { .. }
                | Kind::Camera { .. }
                | Kind::Light { .. }
                | Kind::Model { .. }
        )
    }

    /// The file this layer draws, relative to the project.
    pub fn asset(&self) -> Option<&str> {
        match &self.kind {
            Kind::Image { path }
            | Kind::Svg { path }
            | Kind::Lottie { path, .. }
            | Kind::Model { path } => Some(path),
            _ => None,
        }
    }
}

/// A plain value, or keys to interpolate. In JSON: `"x": 640` or
/// `"x": [{"t": 0, "v": 100, "interp": "bezier"}, ...]`.
///
/// Loading sorts keys by time and refuses an empty list or a non-finite
/// number, wherever the property sits (a layer, an animator, a deformer).
#[derive(Clone, Debug, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(untagged)]
#[schemars(rename = "Anim_{T}")]
pub enum Anim<T> {
    Value(T),
    Keys(Vec<Key<T>>),
}

impl<'de, T: Tween + serde::de::DeserializeOwned> Deserialize<'de> for Anim<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        // Through a JSON value rather than an untagged enum, so a mistake
        // says which key and what is wrong with it, not "no variant matched".
        let raw = serde_json::Value::deserialize(d)?;
        let a = match raw {
            serde_json::Value::Array(keys) => {
                if keys.is_empty() {
                    return Err(D::Error::custom("an empty key list"));
                }
                let mut k = Vec::with_capacity(keys.len());
                for (i, key) in keys.into_iter().enumerate() {
                    let key: Key<T> = serde_json::from_value(key)
                        .map_err(|e| D::Error::custom(format!("key [{i}]: {e}")))?;
                    let finite = key.t.is_finite()
                        && key.v.finite()
                        && key
                            .in_
                            .into_iter()
                            .chain(key.out)
                            .flatten()
                            .all(f64::is_finite);
                    if !finite {
                        return Err(D::Error::custom(format!("key [{i}]: a non-finite number")));
                    }
                    k.push(key);
                }
                k.sort_by(|a, b| a.t.total_cmp(&b.t));
                Self::Keys(k)
            }
            v => {
                let v: T = serde_json::from_value(v).map_err(|e| {
                    D::Error::custom(format!(
                        "expected a value or a list of keys [{{\"t\": .., \"v\": ..}}]: {e}"
                    ))
                })?;
                if !v.finite() {
                    return Err(D::Error::custom("a non-finite value"));
                }
                Self::Value(v)
            }
        };
        Ok(a)
    }
}

/// How the segment *leaving* a key gets to the next one.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Interp {
    /// Stay on this key's value until the next key, then cut.
    Hold,
    Linear,
    /// A cubic through this key's `out` handle and the next key's `in`.
    #[default]
    Bezier,
}

/// A keyframe. Handles are Cavalry/After Effects style: offsets from the key
/// in `[seconds, value]`, `out` pointing forward (seconds >= 0) and `in`
/// backward (seconds <= 0). A missing handle is a third of the segment, flat:
/// an ease. Handle times are clamped inside their segment, which keeps time
/// monotone however they are dragged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(rename = "Key_{T}")]
pub struct Key<T> {
    pub t: f64,
    pub v: T,
    #[serde(default)]
    pub interp: Interp,
    #[serde(default, rename = "in", skip_serializing_if = "Option::is_none")]
    pub in_: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out: Option<[f64; 2]>,
}

/// `#rrggbb` or `#rrggbbaa` in JSON; straight sRGB bytes here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Rgba(pub [u8; 4]);
impl Rgba {
    fn bg() -> Self {
        Self([16, 16, 20, 255])
    }
}
impl schemars::JsonSchema for Rgba {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Rgba".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "`#rrggbb` or `#rrggbbaa`, straight sRGB.",
            "type": "string",
            "pattern": "^#?([0-9a-fA-F]{6}|[0-9a-fA-F]{8})$"
        })
    }
}
impl TryFrom<String> for Rgba {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        let hex = s.strip_prefix('#').unwrap_or(&s);
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2).unwrap_or("x"), 16);
        let bad = || format!("`{s}` is not #rrggbb or #rrggbbaa");
        if !matches!(hex.len(), 6 | 8) {
            return Err(bad());
        }
        let a = if hex.len() == 8 { byte(6) } else { Ok(255) };
        Ok(Self([
            byte(0).map_err(|_| bad())?,
            byte(2).map_err(|_| bad())?,
            byte(4).map_err(|_| bad())?,
            a.map_err(|_| bad())?,
        ]))
    }
}
impl From<Rgba> for String {
    fn from(c: Rgba) -> String {
        let [r, g, b, a] = c.0;
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }
}

/// A value keys can move between.
pub trait Tween: Clone + Default {
    fn mix(a: &Self, b: &Self, u: f64) -> Self;
    fn finite(&self) -> bool {
        true
    }
    /// The value a bezier segment lands on. Numbers use the handles' value
    /// offsets; anything else eases along the handles' timing only.
    fn bezier(a: &Key<Self>, b: &Key<Self>, t: f64) -> Self {
        let flat = |h: Option<[f64; 2]>| h.map(|[dt, _]| [dt, 0.]);
        let u = cubic((a.t, 0.), flat(a.out), flat(b.in_), (b.t, 1.), t);
        Self::mix(&a.v, &b.v, u)
    }
}
impl Tween for f64 {
    fn mix(a: &Self, b: &Self, u: f64) -> Self {
        a + (b - a) * u
    }
    fn finite(&self) -> bool {
        self.is_finite()
    }
    fn bezier(a: &Key<Self>, b: &Key<Self>, t: f64) -> Self {
        cubic((a.t, a.v), a.out, b.in_, (b.t, b.v), t)
    }
}
impl Tween for Rgba {
    fn mix(a: &Self, b: &Self, u: f64) -> Self {
        Self(std::array::from_fn(|i| {
            f64::mix(&f64::from(a.0[i]), &f64::from(b.0[i]), u)
                .round()
                .clamp(0., 255.) as u8
        }))
    }
}

/// The segment's four control points in (time, value), handle times clamped
/// into the segment. With both inner times inside `[t0, t1]` the time
/// polynomial is monotone, so each time has exactly one value.
pub fn controls(
    p0: (f64, f64),
    out: Option<[f64; 2]>,
    inn: Option<[f64; 2]>,
    p3: (f64, f64),
) -> [(f64, f64); 4] {
    let third = (p3.0 - p0.0) / 3.;
    let [odt, odv] = out.unwrap_or([third, 0.]);
    let [idt, idv] = inn.unwrap_or([-third, 0.]);
    let p1 = ((p0.0 + odt).clamp(p0.0, p3.0), p0.1 + odv);
    let p2 = ((p3.0 + idt).clamp(p0.0, p3.0), p3.1 + idv);
    [p0, p1, p2, p3]
}

/// The value of the cubic at time `t`: bisect the (monotone) time polynomial
/// for its parameter, then read the value polynomial there.
fn cubic(
    p0: (f64, f64),
    out: Option<[f64; 2]>,
    inn: Option<[f64; 2]>,
    p3: (f64, f64),
    t: f64,
) -> f64 {
    let [a, b, c, d] = controls(p0, out, inn, p3);
    let bez = |a: f64, b: f64, c: f64, d: f64, s: f64| {
        let m = 1. - s;
        m * m * m * a + 3. * m * m * s * b + 3. * m * s * s * c + s * s * s * d
    };
    let (mut lo, mut hi) = (0f64, 1f64);
    for _ in 0..48 {
        let mid = 0.5 * (lo + hi);
        if bez(a.0, b.0, c.0, d.0, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez(a.1, b.1, c.1, d.1, 0.5 * (lo + hi))
}

impl<T: Tween> Anim<T> {
    /// The value at `t` seconds: before the first key its value, after the
    /// last key its value, exactly a key's value at its time.
    pub fn at(&self, t: f64) -> T {
        let keys = match self {
            Self::Value(v) => return v.clone(),
            Self::Keys(k) => k,
        };
        let Some(first) = keys.first() else {
            // `Project::load` refuses an empty list; a hand-built one is zero.
            return T::default();
        };
        // Keys are read in time order whatever order the file lists them.
        let i = keys.partition_point(|k| k.t <= t);
        if i == 0 {
            return first.v.clone();
        }
        let a = &keys[i - 1];
        let Some(b) = keys.get(i) else {
            return a.v.clone();
        };
        if t <= a.t || b.t <= a.t {
            return a.v.clone();
        }
        match a.interp {
            Interp::Hold => a.v.clone(),
            Interp::Linear => T::mix(&a.v, &b.v, (t - a.t) / (b.t - a.t)),
            Interp::Bezier => T::bezier(a, b, t),
        }
    }
}

/// One layer with every property evaluated at a time.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Drawn {
    pub id: String,
    #[serde(flatten)]
    pub kind: Kind,
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub rotation: f64,
    pub opacity: f64,
    pub width: f64,
    pub height: f64,
    pub radius: f64,
    pub fill: Rgba,
    pub font_size: f64,
    pub weight: f64,
    pub line_height: f64,
    pub tracking: f64,
    pub stroke: Rgba,
    pub stroke_width: f64,
    /// `[start, end, offset]`.
    pub trim: [f64; 3],
    pub count: usize,
    pub columns: usize,
    pub spacing: [f64; 2],
    pub ring_radius: f64,
    pub path_offset: f64,
    /// Lottie: seconds into the animation.
    pub time: f64,
    /// Per glyph (text, newlines skipped) or per copy (duplicator).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fx: Vec<Fx>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub deformers: Vec<Deform>,
    /// The 3D properties, left out while they are all their defaults.
    #[serde(skip_serializing_if = "three::Space::is_flat")]
    pub space: three::Space,
}

/// Everything a renderer needs for one instant of one scene.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Frame {
    pub size: [u32; 2],
    pub background: Rgba,
    pub layers: Vec<Drawn>,
    /// A 3D scene's camera, lights, ground and fog; `None` in 2D.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<View>,
}

/// Most copies a duplicator makes.
pub const MAX_COPIES: usize = 10_000;

/// Scene `scene` at `t` seconds: a pure function of its arguments, so any
/// time can be sought in any order.
pub fn eval(project: &Project, scene: &Scene, t: f64) -> Frame {
    let layers: Vec<Drawn> = scene.layers.iter().map(|l| l.at(t)).collect();
    let view = (scene.mode == Mode::ThreeD).then(|| three::view(project.size, scene, &layers));
    Frame {
        size: project.size,
        background: scene.background,
        layers,
        view,
    }
}

impl Layer {
    /// Every property at `t`.
    pub fn at(&self, t: f64) -> Drawn {
        let fill = self.fill.at(t);
        let count = self.count.at(t).round().clamp(0., MAX_COPIES as f64) as usize;
        let fx = match &self.kind {
            Kind::Text { text, .. } if !self.animators.is_empty() => {
                let n = text.chars().filter(|&c| c != '\n').count();
                motion::apply(&self.animators, n, fill, t, |by| text_units(text, by))
            }
            Kind::Duplicator { .. } => {
                motion::apply(&self.animators, count, fill, t, |_| (0..count).collect())
            }
            _ => Vec::new(),
        };
        let time = match self.kind {
            Kind::Lottie { speed, .. } => self.time.at(t) + t * speed,
            _ => 0.,
        };
        Drawn {
            id: self.id.clone(),
            kind: self.kind.clone(),
            x: self.x.at(t),
            y: self.y.at(t),
            scale: self.scale.at(t),
            rotation: self.rotation.at(t),
            opacity: self.opacity.at(t).clamp(0., 1.),
            width: self.width.at(t).max(0.),
            height: self.height.at(t).max(0.),
            radius: self.radius.at(t).max(0.),
            fill,
            font_size: self.font_size.at(t).max(1.),
            weight: self.weight.at(t).clamp(100., 900.),
            line_height: self.line_height.at(t),
            tracking: self.tracking.at(t),
            stroke: self.stroke.at(t),
            stroke_width: self.stroke_width.at(t).max(0.),
            trim: [
                self.trim_start.at(t),
                self.trim_end.at(t),
                self.trim_offset.at(t),
            ],
            count,
            columns: self.columns.at(t).round().max(1.) as usize,
            spacing: [self.spacing_x.at(t), self.spacing_y.at(t)],
            ring_radius: self.ring_radius.at(t),
            path_offset: self.path_offset.at(t),
            time,
            fx,
            deformers: self.deformers.iter().map(|d| d.at(t)).collect(),
            space: three::Space {
                z: self.z.at(t),
                rx: self.rx.at(t),
                ry: self.ry.at(t),
                anchor_z: self.anchor_z.at(t),
                extrude: self.extrude.at(t).max(0.),
                edge: self.edge.at(t),
                cast_shadows: self.cast_shadows,
                receive_shadows: self.receive_shadows,
                distance: self.distance.at(t).max(0.),
                fov: self.fov.at(t).clamp(1., 170.),
                dolly: self.dolly.at(t),
                focus: self.focus.at(t),
                aperture: self.aperture.at(t).max(0.),
                intensity: self.intensity.at(t).max(0.),
                cone: self.cone.at(t).clamp(0.1, 89.),
                feather: self.feather.at(t).clamp(0., 1.),
                range: self.range.at(t).max(0.),
                softness: self.softness.at(t).max(0.),
            },
        }
    }
}

impl Project {
    /// Parse and check a project: keys sorted by time, no empty key lists,
    /// finite numbers, positive size, fps and durations, unique layer ids,
    /// path data that parses.
    ///
    /// Every error starts with the JSON path it is about
    /// (`scenes[0].layers[2].x: key [1]: ...`), then serde's line and column
    /// where there is one.
    pub fn load(json: &str) -> Result<Self, String> {
        let de = &mut serde_json::Deserializer::from_str(json);
        let p: Self = serde_path_to_error::deserialize(de).map_err(|e| {
            let path = e.path().to_string();
            if path == "." {
                e.into_inner().to_string()
            } else {
                format!("{path}: {}", e.into_inner())
            }
        })?;
        if p.size[0] == 0 || p.size[1] == 0 || p.size[0] > 8192 || p.size[1] > 8192 {
            return Err("size: must be 1..=8192 pixels each way".into());
        }
        if !(p.fps.is_finite() && p.fps > 0. && p.fps <= 240.) {
            return Err("fps: must be in (0, 240]".into());
        }
        for (si, s) in p.scenes.iter().enumerate() {
            if !(s.duration.is_finite() && s.duration > 0.) {
                return Err(format!(
                    "scenes[{si}].duration: scene `{}`: duration must be > 0",
                    s.name
                ));
            }
            let mut ids = std::collections::HashSet::new();
            for (li, l) in s.layers.iter().enumerate() {
                let at = format!("scenes[{si}].layers[{li}]");
                if !ids.insert(l.id.clone()) {
                    return Err(format!(
                        "{at}.id: scene `{}`: duplicate layer id `{}`",
                        s.name, l.id
                    ));
                }
                let id = &l.id;
                let bad = |what: &str, d: &str| -> Result<(), String> {
                    if !d.is_empty() && mui_vello::kurbo::BezPath::from_svg(d).is_err() {
                        return Err(format!(
                            "{at}.{what}: layer `{id}`: `{what}` is not SVG path data"
                        ));
                    }
                    Ok(())
                };
                match &l.kind {
                    Kind::Path { d } => bad("d", d)?,
                    Kind::Duplicator { d, along, .. } => {
                        bad("d", d)?;
                        bad("along", along)?;
                    }
                    Kind::Camera { path, .. } => bad("path", path)?,
                    Kind::Lottie { speed, .. } if !speed.is_finite() => {
                        return Err(format!("{at}.speed: layer `{id}`: `speed` must be finite"));
                    }
                    _ => {}
                }
            }
            for l in &s.layers {
                if let Kind::Camera { look_at, .. } = &l.kind
                    && !look_at.is_empty()
                    && !ids.contains(look_at)
                {
                    return Err(format!(
                        "scene `{}`: camera `{}` looks at no layer `{look_at}`",
                        s.name, l.id
                    ));
                }
            }
        }
        Ok(p)
    }

    /// The project file's JSON Schema, generated from these types: what
    /// `mui-cut schema` prints and a file's `$schema` points at.
    pub fn json_schema() -> serde_json::Value {
        schemars::schema_for!(Project).to_value()
    }
    /// Pretty JSON in the struct's field order, one keyframe a line, so a
    /// save diffs cleanly and reads like the hand-written examples.
    pub fn to_json(&self) -> String {
        let mut s = tidy(&serde_json::to_string_pretty(self).expect("a project serialises"));
        s.push('\n');
        s
    }
    pub fn scene(&self, name: &str) -> Option<&Scene> {
        self.scenes.iter().find(|s| s.name == name)
    }
}

/// Put every number array (`[0.1, 0.0]`) and every keyframe object on one
/// line of serde's pretty output. Only whitespace outside strings changes:
/// JSON strings hold no raw newline, and only newline runs are collapsed.
fn tidy(pretty: &str) -> String {
    let mut out = String::with_capacity(pretty.len());
    // Per open bracket: where it starts in `out`, and whether it holds one.
    let mut open: Vec<(usize, bool)> = Vec::new();
    let (mut in_str, mut escaped) = (false, false);
    for c in pretty.chars() {
        out.push(c);
        if in_str {
            (in_str, escaped) = (!(c == '"' && !escaped), c == '\\' && !escaped);
            continue;
        }
        match c {
            '"' => in_str = true,
            '[' | '{' => {
                if let Some(o) = open.last_mut() {
                    o.1 = true;
                }
                open.push((out.len() - 1, false));
            }
            ']' | '}' => {
                let Some((start, nested)) = open.pop() else {
                    continue;
                };
                let body = &out[start..];
                let key = c == '}'
                    && body
                        .trim_start_matches(['{', ' ', '\n'])
                        .starts_with("\"t\":");
                // A keyframe's own `in`/`out` arrays were collapsed already;
                // they still mark it nested, so allow exactly those.
                let flat = if key {
                    !body[1..].contains('{')
                } else {
                    c == ']' && !nested
                };
                if flat && body.contains('\n') {
                    let mut one = String::with_capacity(body.len());
                    let mut ws = false;
                    for ch in body.chars() {
                        if ch == '\n' {
                            ws = true;
                        } else if ws && ch == ' ' {
                        } else {
                            if ws {
                                one.push(' ');
                            }
                            ws = false;
                            one.push(ch);
                        }
                    }
                    let one = if c == ']' {
                        one.replace("[ ", "[").replace(" ]", "]")
                    } else {
                        one
                    };
                    out.truncate(start);
                    out.push_str(&one);
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests3d;
