//! mui-cut: a small keyframe motion editor on MUI.
//!
//! The project is plain JSON (`*.cut.json`, see the README), so a person in
//! the web editor and an agent with a text editor edit the same file. This
//! crate is the document, a pure evaluator ([`eval`]: scene + time in, a
//! [`Frame`] out, seekable) and a MUI/Vello CPU renderer ([`Renderer`]) that
//! the CLI and the browser both draw through, so a still, a render and the
//! editor viewport are the same pixels.
#![forbid(unsafe_code)]

mod gpu;
mod render;
#[cfg(target_arch = "wasm32")]
mod web;

pub use gpu::GpuCanvas;
#[cfg(not(target_arch = "wasm32"))]
pub use gpu::Offline;
pub use render::{Assets, Layers, Quad, Renderer};

use serde::{Deserialize, Serialize};

/// The whole file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    /// Output pixels, `[width, height]`; layer coordinates are in these.
    pub size: [u32; 2],
    pub fps: f64,
    pub scenes: Vec<Scene>,
}

/// One shot. Scenes play back to back in a render.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub name: String,
    /// Seconds.
    pub duration: f64,
    #[serde(default = "Rgba::bg")]
    pub background: Rgba,
    /// Bottom first: later layers paint over earlier ones.
    #[serde(default)]
    pub layers: Vec<Layer>,
}

/// What a layer draws.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Kind {
    Rect,
    Ellipse,
    Text {
        text: String,
    },
    /// A PNG, path relative to the project file.
    Image {
        path: String,
    },
}

/// One layer. Every property is either a plain value or a list of keys; `x`
/// and `y` are the layer's centre, which is also its rotation and scale pivot.
/// A property left out is its default, and a save leaves defaults out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

/// The numeric properties, by their JSON names, in inspector order.
pub const PROPS: [&str; 10] = [
    "x",
    "y",
    "scale",
    "rotation",
    "opacity",
    "width",
    "height",
    "radius",
    "font_size",
    "weight",
];

impl Layer {
    /// A numeric property by its JSON name.
    pub fn prop(&self, name: &str) -> Option<&Anim<f64>> {
        Some(match name {
            "x" => &self.x,
            "y" => &self.y,
            "scale" => &self.scale,
            "rotation" => &self.rotation,
            "opacity" => &self.opacity,
            "width" => &self.width,
            "height" => &self.height,
            "radius" => &self.radius,
            "font_size" => &self.font_size,
            "weight" => &self.weight,
            _ => return None,
        })
    }
}

/// A plain value, or keys to interpolate. In JSON: `"x": 640` or
/// `"x": [{"t": 0, "v": 100, "interp": "bezier"}, ...]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Anim<T> {
    Value(T),
    Keys(Vec<Key<T>>),
}

/// How the segment *leaving* a key gets to the next one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    /// Sort keys by time, so a hand-edited file can list them in any order.
    pub fn sort(&mut self) {
        if let Self::Keys(k) = self {
            k.sort_by(|a, b| a.t.total_cmp(&b.t));
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
}

/// Everything a renderer needs for one instant of one scene.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Frame {
    pub size: [u32; 2],
    pub background: Rgba,
    pub layers: Vec<Drawn>,
}

/// Scene `scene` at `t` seconds: a pure function of its arguments, so any
/// time can be sought in any order.
pub fn eval(project: &Project, scene: &Scene, t: f64) -> Frame {
    Frame {
        size: project.size,
        background: scene.background,
        layers: scene
            .layers
            .iter()
            .map(|l| Drawn {
                id: l.id.clone(),
                kind: l.kind.clone(),
                x: l.x.at(t),
                y: l.y.at(t),
                scale: l.scale.at(t),
                rotation: l.rotation.at(t),
                opacity: l.opacity.at(t).clamp(0., 1.),
                width: l.width.at(t).max(0.),
                height: l.height.at(t).max(0.),
                radius: l.radius.at(t).max(0.),
                fill: l.fill.at(t),
                font_size: l.font_size.at(t).max(1.),
                weight: l.weight.at(t).clamp(100., 900.),
            })
            .collect(),
    }
}

impl Project {
    /// Parse and check a project: keys sorted by time, no empty key lists,
    /// finite numbers, positive size, fps and durations, unique layer ids.
    pub fn load(json: &str) -> Result<Self, String> {
        let mut p: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if p.size[0] == 0 || p.size[1] == 0 || p.size[0] > 8192 || p.size[1] > 8192 {
            return Err("size must be 1..=8192 pixels each way".into());
        }
        if !(p.fps.is_finite() && p.fps > 0. && p.fps <= 240.) {
            return Err("fps must be in (0, 240]".into());
        }
        for s in &mut p.scenes {
            if !(s.duration.is_finite() && s.duration > 0.) {
                return Err(format!("scene `{}`: duration must be > 0", s.name));
            }
            let mut ids = std::collections::HashSet::new();
            for l in &mut s.layers {
                if !ids.insert(l.id.clone()) {
                    return Err(format!("scene `{}`: duplicate layer id `{}`", s.name, l.id));
                }
                let id = l.id.clone();
                let empty = |n: &str| format!("layer `{id}`: `{n}` has an empty key list");
                for name in PROPS {
                    let a = l.prop_mut(name).expect("PROPS are props");
                    a.sort();
                    match a {
                        Anim::Keys(k) if k.is_empty() => return Err(empty(name)),
                        Anim::Keys(k)
                            if k.iter().any(|k| {
                                !k.t.is_finite()
                                    || !k.v.is_finite()
                                    || k.in_
                                        .into_iter()
                                        .chain(k.out)
                                        .flatten()
                                        .any(|x| !x.is_finite())
                            }) =>
                        {
                            return Err(format!("layer `{id}`: `{name}` has a non-finite key"));
                        }
                        _ => {}
                    }
                }
                l.fill.sort();
                if matches!(&l.fill, Anim::Keys(k) if k.is_empty()) {
                    return Err(empty("fill"));
                }
            }
        }
        Ok(p)
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

impl Layer {
    fn prop_mut(&mut self, name: &str) -> Option<&mut Anim<f64>> {
        Some(match name {
            "x" => &mut self.x,
            "y" => &mut self.y,
            "scale" => &mut self.scale,
            "rotation" => &mut self.rotation,
            "opacity" => &mut self.opacity,
            "width" => &mut self.width,
            "height" => &mut self.height,
            "radius" => &mut self.radius,
            "font_size" => &mut self.font_size,
            "weight" => &mut self.weight,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests;
