//! Per-element motion: text animators and duplicator staggers (one
//! [`Animator`] type drives both), and deformers. All of it is evaluated in
//! [`crate::eval`], so `mui-cut eval` shows every glyph's and every copy's
//! numbers, and all of it is a pure function of the time.
use serde::{Deserialize, Serialize};

use crate::{Anim, Rgba};

/// What one step of an animator's selector is, for text. A duplicator's
/// steps are always its copies.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    #[default]
    Char,
    Word,
    Line,
}

/// How a selector's weight falls off across its range, evaluated at each
/// unit's centre (After Effects' range selector shapes). `square` instead
/// weighs a unit by how much of it the range covers.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Falloff {
    #[default]
    Square,
    RampUp,
    RampDown,
    Triangle,
    Round,
    Smooth,
}

/// A curve on the selector's weight. `step` makes it all or nothing (a
/// unit counts once the range covers half of it): the typewriter's cut.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Ease {
    #[default]
    Linear,
    In,
    Out,
    InOut,
    Step,
}

/// The order units are ranked in: where each sits in the range, and how
/// much of the stagger delay it gets.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Order {
    #[default]
    Forward,
    Reverse,
    /// A shuffle fixed by `seed`: the same every frame and every render.
    Random,
}

fn z() -> Anim<f64> {
    Anim::Value(0.)
}
fn o() -> Anim<f64> {
    Anim::Value(1.)
}
fn is_z(a: &Anim<f64>) -> bool {
    *a == z()
}
fn is_o(a: &Anim<f64>) -> bool {
    *a == o()
}
fn clear() -> Anim<Rgba> {
    Anim::Value(Rgba([255, 255, 255, 0]))
}
fn is_clear(a: &Anim<Rgba>) -> bool {
    *a == clear()
}
fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// A selector and the offsets it applies, per glyph (text) or per copy
/// (duplicator). With the defaults it selects everything fully and changes
/// nothing; set a property to what a selected unit should become.
///
/// A unit's weight is `amount * ease(falloff)`, all read at the unit's own
/// time `t - rank * stagger`. `start`, `end` and `offset` are fractions of
/// the units, 0..1: the range is `[start + offset, end + offset]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(transform = crate::vars::bindable)]
pub struct Animator {
    #[serde(default, skip_serializing_if = "is_default")]
    pub by: Unit,
    #[serde(default, skip_serializing_if = "is_default")]
    pub shape: Falloff,
    #[serde(default, skip_serializing_if = "is_default")]
    pub ease: Ease,
    #[serde(default, skip_serializing_if = "is_default")]
    pub order: Order,
    #[serde(default, skip_serializing_if = "is_default")]
    pub seed: u32,
    #[serde(default = "z", skip_serializing_if = "is_z")]
    pub start: Anim<f64>,
    #[serde(default = "o", skip_serializing_if = "is_o")]
    pub end: Anim<f64>,
    #[serde(default = "z", skip_serializing_if = "is_z")]
    pub offset: Anim<f64>,
    #[serde(default = "o", skip_serializing_if = "is_o")]
    pub amount: Anim<f64>,
    /// Seconds each rank waits after the one before it.
    #[serde(default = "z", skip_serializing_if = "is_z")]
    pub stagger: Anim<f64>,
    #[serde(default = "z", skip_serializing_if = "is_z")]
    pub x: Anim<f64>,
    #[serde(default = "z", skip_serializing_if = "is_z")]
    pub y: Anim<f64>,
    #[serde(default = "o", skip_serializing_if = "is_o")]
    pub scale: Anim<f64>,
    #[serde(default = "z", skip_serializing_if = "is_z")]
    pub rotation: Anim<f64>,
    #[serde(default = "o", skip_serializing_if = "is_o")]
    pub opacity: Anim<f64>,
    /// Extra advance after each selected glyph, pixels (text only).
    #[serde(default = "z", skip_serializing_if = "is_z")]
    pub tracking: Anim<f64>,
    /// Tint towards this colour; its alpha is the tint's strength.
    #[serde(default = "clear", skip_serializing_if = "is_clear")]
    pub fill: Anim<Rgba>,
    /// Per-unit randomness, fixed by `seed` (the same every frame and
    /// render) and scaled by the unit's weight: each unit moves up to
    /// `jitter_x`/`jitter_y` pixels and turns up to `jitter_rotation`
    /// degrees either way, scales by up to `jitter_scale` (a fraction)
    /// either way, fades by up to `jitter_opacity` (a fraction) and shifts
    /// its fill's hue by up to `jitter_hue` degrees either way.
    #[serde(default = "z", skip_serializing_if = "is_z")]
    pub jitter_x: Anim<f64>,
    #[serde(default = "z", skip_serializing_if = "is_z")]
    pub jitter_y: Anim<f64>,
    #[serde(default = "z", skip_serializing_if = "is_z")]
    pub jitter_rotation: Anim<f64>,
    #[serde(default = "z", skip_serializing_if = "is_z")]
    pub jitter_scale: Anim<f64>,
    #[serde(default = "z", skip_serializing_if = "is_z")]
    pub jitter_opacity: Anim<f64>,
    #[serde(default = "z", skip_serializing_if = "is_z")]
    pub jitter_hue: Anim<f64>,
    /// A spatial effector: units weigh by where they sit, multiplied into
    /// the range selector's weight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub falloff: Option<Effector>,
}

/// The shape of an [`Effector`]'s field.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Field {
    /// Full within `radius` of the centre.
    #[default]
    Sphere,
    /// Full left of the centre's x (a wall to sweep across), fading over
    /// `softness` to its right; `radius` is unused.
    Linear,
    /// Full within a square `radius` from the centre each way.
    Box,
}

/// A region in the layer's own pixels around its origin (a duplicator's
/// copies' slots, a group's children's places): a unit inside weighs 1,
/// fading to 0 over `softness` pixels past the edge; `invert` swaps
/// inside and out. Text glyphs have no place in it and are not weighed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(transform = crate::vars::bindable)]
pub struct Effector {
    #[serde(default, skip_serializing_if = "is_default")]
    pub shape: Field,
    #[serde(default = "z", skip_serializing_if = "is_z")]
    pub x: Anim<f64>,
    #[serde(default = "z", skip_serializing_if = "is_z")]
    pub y: Anim<f64>,
    #[serde(default = "two_hundred", skip_serializing_if = "is_two_hundred")]
    pub radius: Anim<f64>,
    #[serde(default = "hundred", skip_serializing_if = "is_hundred")]
    pub softness: Anim<f64>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub invert: bool,
}

fn hundred() -> Anim<f64> {
    Anim::Value(100.)
}
fn is_hundred(a: &Anim<f64>) -> bool {
    *a == hundred()
}

impl Effector {
    /// The numeric properties, by JSON name.
    pub const PROPS: [&str; 4] = ["x", "y", "radius", "softness"];

    pub fn num(&self, name: &str) -> Option<&Anim<f64>> {
        Some(match name {
            "x" => &self.x,
            "y" => &self.y,
            "radius" => &self.radius,
            "softness" => &self.softness,
            _ => return None,
        })
    }

    /// The weight at point `p` at `t`.
    pub fn weight(&self, p: [f64; 2], t: f64) -> f64 {
        let (dx, dy) = (p[0] - self.x.at(t), p[1] - self.y.at(t));
        let r = self.radius.at(t);
        let d = match self.shape {
            Field::Sphere => dx.hypot(dy),
            Field::Box => dx.abs().max(dy.abs()),
            Field::Linear => dx + r,
        };
        let soft = self.softness.at(t).max(0.);
        let w = if soft < 1e-9 {
            f64::from(u8::from(d <= r))
        } else {
            let u = ((d - r) / soft).clamp(0., 1.);
            1. - u * u * (3. - 2. * u)
        };
        if self.invert { 1. - w } else { w }
    }
}

/// The numeric properties of an [`Animator`], by JSON name.
pub const ANIMATOR_PROPS: [&str; 17] = [
    "start",
    "end",
    "offset",
    "amount",
    "stagger",
    "x",
    "y",
    "scale",
    "rotation",
    "opacity",
    "tracking",
    "jitter_x",
    "jitter_y",
    "jitter_rotation",
    "jitter_scale",
    "jitter_opacity",
    "jitter_hue",
];

/// The serde defaults: selects everything, changes nothing.
impl Default for Animator {
    fn default() -> Self {
        serde_json::from_str("{}").expect("every field has a default")
    }
}

impl Animator {
    pub fn num(&self, name: &str) -> Option<&Anim<f64>> {
        Some(match name {
            "start" => &self.start,
            "end" => &self.end,
            "offset" => &self.offset,
            "amount" => &self.amount,
            "stagger" => &self.stagger,
            "x" => &self.x,
            "y" => &self.y,
            "scale" => &self.scale,
            "rotation" => &self.rotation,
            "opacity" => &self.opacity,
            "tracking" => &self.tracking,
            "jitter_x" => &self.jitter_x,
            "jitter_y" => &self.jitter_y,
            "jitter_rotation" => &self.jitter_rotation,
            "jitter_scale" => &self.jitter_scale,
            "jitter_opacity" => &self.jitter_opacity,
            "jitter_hue" => &self.jitter_hue,
            _ => return None,
        })
    }

    /// Every numeric property by path inside the animator (`x`,
    /// `falloff.radius`).
    pub fn nums(&self) -> Vec<(String, &Anim<f64>)> {
        let mut out: Vec<(String, &Anim<f64>)> = ANIMATOR_PROPS
            .iter()
            .map(|&n| (n.to_owned(), self.num(n).expect("ANIMATOR_PROPS are props")))
            .collect();
        if let Some(e) = &self.falloff {
            for n in Effector::PROPS {
                out.push((format!("falloff.{n}"), e.num(n).expect("PROPS are props")));
            }
        }
        out
    }

    /// A classic setup as JSON-ready data, keyed over `[t0, t0 + dur]`:
    /// `typewriter` (characters appear one by one), `cascade` (characters
    /// rise and fade in, one after another), `cascade_out` (its exit) or `pop` (copies or glyphs scale
    /// up from nothing in a random order).
    pub fn preset(name: &str, t0: f64, dur: f64) -> Option<Self> {
        use crate::{Interp, Key};
        let keys = |a: f64, b: f64, interp: Interp| {
            Anim::Keys(vec![
                Key {
                    t: t0,
                    v: a,
                    interp,
                    in_: None,
                    out: None,
                },
                Key {
                    // On a microsecond grid: no 0.8999999999999999 in the file.
                    t: ((t0 + dur) * 1e6).round() / 1e6,
                    v: b,
                    interp: Interp::Hold,
                    in_: None,
                    out: None,
                },
            ])
        };
        Some(match name {
            // Everything from `start` on is hidden; `start` sweeps across.
            "typewriter" => Self {
                ease: Ease::Step,
                start: keys(0., 1., Interp::Linear),
                opacity: z(),
                ..Self::default()
            },
            // Each glyph eases in on its own clock, a stagger behind the last.
            "cascade" => Self {
                amount: keys(1., 0., Interp::Bezier),
                stagger: Anim::Value(0.04),
                y: Anim::Value(40.),
                opacity: z(),
                ..Self::default()
            },
            // The cascade's exit: each glyph rises away and fades out.
            "cascade_out" => Self {
                amount: keys(0., 1., Interp::Bezier),
                stagger: Anim::Value(0.03),
                y: Anim::Value(-40.),
                opacity: z(),
                ..Self::default()
            },
            "pop" => Self {
                order: Order::Random,
                amount: keys(1., 0., Interp::Bezier),
                stagger: Anim::Value(0.03),
                scale: z(),
                ..Self::default()
            },
            _ => return None,
        })
    }

    /// Each unit's weight at `t`, `n` units at `pos` (or none: text).
    fn weights(&self, n: usize, t: f64, pos: &[[f64; 2]]) -> Vec<f64> {
        let ranks = ranks(self.order, self.seed, n);
        ranks
            .iter()
            .enumerate()
            .map(|(u, &r)| {
                let t = t - r as f64 * self.stagger.at(t);
                let (lo, hi) = (r as f64 / n as f64, (r + 1) as f64 / n as f64);
                let off = self.offset.at(t);
                let (mut s, mut e) = (self.start.at(t) + off, self.end.at(t) + off);
                if s > e {
                    std::mem::swap(&mut s, &mut e);
                }
                let w = match self.shape {
                    Falloff::Square => ((e.min(hi) - s.max(lo)) / (hi - lo)).clamp(0., 1.),
                    shape => {
                        let c = 0.5 * (lo + hi);
                        if c < s || c > e || e - s < 1e-12 {
                            0.
                        } else {
                            let u = (c - s) / (e - s);
                            match shape {
                                Falloff::RampUp => u,
                                Falloff::RampDown => 1. - u,
                                Falloff::Triangle => 1. - (2. * u - 1.).abs(),
                                Falloff::Round => (1. - (2. * u - 1.).powi(2)).sqrt(),
                                _ => 0.5 - 0.5 * (std::f64::consts::TAU * u).cos(),
                            }
                        }
                    }
                };
                let w = match self.ease {
                    Ease::Linear => w,
                    Ease::In => w * w,
                    Ease::Out => 1. - (1. - w) * (1. - w),
                    Ease::InOut => w * w * (3. - 2. * w),
                    Ease::Step => f64::from(u8::from(w >= 0.5)),
                };
                let field = match (&self.falloff, pos.get(u)) {
                    (Some(e), Some(&p)) => e.weight(p, t),
                    _ => 1.,
                };
                w * field * self.amount.at(t)
            })
            .collect()
    }
}

/// Unit `u`'s rank in `order`.
fn ranks(order: Order, seed: u32, n: usize) -> Vec<usize> {
    match order {
        Order::Forward => (0..n).collect(),
        Order::Reverse => (0..n).rev().collect(),
        Order::Random => {
            let mut by: Vec<usize> = (0..n).collect();
            by.sort_by_key(|&i| hash(seed, i as u64));
            let mut r = vec![0; n];
            for (rank, &i) in by.iter().enumerate() {
                r[i] = rank;
            }
            r
        }
    }
}

/// splitmix64 of the seed and an index: a fixed, well-spread shuffle key.
pub(crate) fn hash(seed: u32, i: u64) -> u64 {
    let mut z = (u64::from(seed) << 32 ^ i).wrapping_add(0x9e3779b97f4a7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

/// One glyph's or copy's evaluated offsets: added to its place (`x`, `y`,
/// `rotation` degrees), multiplied into its `scale` and `opacity`, `fill`
/// replacing the layer's, `tracking` added after it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Fx {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub rotation: f64,
    pub opacity: f64,
    pub tracking: f64,
    pub fill: Rgba,
}

/// Every element's [`Fx`] at `t`. `unit_of(by)` gives each element's unit
/// index for a text unit kind (a duplicator ignores `by`).
pub fn apply(
    animators: &[Animator],
    elements: usize,
    fill: Rgba,
    t: f64,
    unit_of: impl Fn(Unit) -> Vec<usize>,
) -> Vec<Fx> {
    apply_to(animators, elements, &|_| fill, t, unit_of, &[])
}

/// [`apply`] with each element's own base fill.
pub(crate) fn apply_to(
    animators: &[Animator],
    elements: usize,
    fill: &dyn Fn(usize) -> Rgba,
    t: f64,
    unit_of: impl Fn(Unit) -> Vec<usize>,
    pos: &[[f64; 2]],
) -> Vec<Fx> {
    let mut fx: Vec<Fx> = (0..elements)
        .map(|e| Fx {
            x: 0.,
            y: 0.,
            scale: 1.,
            rotation: 0.,
            opacity: 1.,
            tracking: 0.,
            fill: fill(e),
        })
        .collect();
    for a in animators {
        let units = unit_of(a.by);
        let n = units.iter().max().map_or(0, |m| m + 1);
        let w = a.weights(n, t, pos);
        let ranks = ranks(a.order, a.seed, n);
        for (f, &u) in fx.iter_mut().zip(&units) {
            let w = w[u];
            if w == 0. {
                continue;
            }
            // The unit's own clock, as its weight used.
            let t = t - ranks[u] as f64 * a.stagger.at(t);
            // -1..1, fixed per unit, property and seed.
            let r =
                |k: u64| (hash(a.seed, u as u64 * 8 + k) >> 11) as f64 / (1u64 << 52) as f64 - 1.;
            f.x += w * (a.x.at(t) + r(0) * a.jitter_x.at(t));
            f.y += w * (a.y.at(t) + r(1) * a.jitter_y.at(t));
            f.scale *= 1. + w * (a.scale.at(t) - 1.);
            f.scale *= (1. + w * r(2) * a.jitter_scale.at(t)).max(0.);
            f.rotation += w * (a.rotation.at(t) + r(3) * a.jitter_rotation.at(t));
            f.opacity *= (1. + w * (a.opacity.at(t) - 1.)).clamp(0., 1.);
            f.opacity *= (1. - w * 0.5 * (r(4) + 1.) * a.jitter_opacity.at(t)).clamp(0., 1.);
            f.tracking += w * a.tracking.at(t);
            let tint = a.fill.at(t);
            let k = w * f64::from(tint.0[3]) / 255.;
            f.fill = crate::Tween::mix(
                &f.fill,
                &Rgba([tint.0[0], tint.0[1], tint.0[2], f.fill.0[3]]),
                k,
            );
            let hue = w * r(5) * a.jitter_hue.at(t);
            if hue != 0. {
                f.fill = hue_shift(f.fill, hue);
            }
        }
    }
    fx
}

/// `c` with its hue turned by `deg` degrees (HSV; lightness and alpha kept).
fn hue_shift(c: Rgba, deg: f64) -> Rgba {
    let [r, g, b, a] = c.0.map(|v| f64::from(v) / 255.);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    if d <= 0. {
        return c;
    }
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.)
    } else if max == g {
        (b - r) / d + 2.
    } else {
        (r - g) / d + 4.
    };
    let h = (h * 60. + deg).rem_euclid(360.) / 60.;
    let x = d * (1. - (h.rem_euclid(2.) - 1.).abs());
    let (r, g, b) = match h as u32 {
        0 => (d, x, 0.),
        1 => (x, d, 0.),
        2 => (0., d, x),
        3 => (0., x, d),
        4 => (x, 0., d),
        _ => (d, 0., x),
    };
    let byte = |v: f64| ((v + min) * 255.).round().clamp(0., 255.) as u8;
    Rgba([byte(r), byte(g), byte(b), (a * 255.).round() as u8])
}

/// Each char's unit index (newlines skipped: they are not glyphs) for `by`.
/// Spaces belong to the word before them.
pub fn text_units(text: &str, by: Unit) -> Vec<usize> {
    let mut out = Vec::with_capacity(text.len());
    let (mut word, mut line, mut in_word, mut any) = (0, 0, false, false);
    for ch in text.chars() {
        if ch == '\n' {
            line += 1;
            in_word = false;
            continue;
        }
        let ws = ch.is_whitespace();
        if !ws && !in_word {
            if any {
                word += 1;
            }
            any = true;
        }
        in_word = !ws;
        out.push(match by {
            Unit::Char => out.len(),
            Unit::Word => word,
            Unit::Line => line,
        });
    }
    out
}

/// A deformer: moves every point of the layer's shapes (after copies,
/// animators and trim), in the layer's own pixels around its origin. Paths
/// are subdivided first, so even a rectangle bends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "lowercase")]
#[schemars(transform = crate::vars::bindable)]
pub enum Deformer {
    /// Seeded gradient noise: each point moves up to `amount` pixels;
    /// `frequency` is features per pixel, `speed` how fast the field drifts
    /// (noise units per second).
    Noise {
        #[serde(default = "twenty", skip_serializing_if = "is_twenty")]
        amount: Anim<f64>,
        #[serde(default = "freq", skip_serializing_if = "is_freq")]
        frequency: Anim<f64>,
        #[serde(default = "o", skip_serializing_if = "is_o")]
        speed: Anim<f64>,
        #[serde(default, skip_serializing_if = "is_default")]
        seed: u32,
    },
    /// A swirl: `angle` degrees at the centre, fading to none at `radius`.
    Twist {
        #[serde(default = "ninety", skip_serializing_if = "is_ninety")]
        angle: Anim<f64>,
        #[serde(default = "two_hundred", skip_serializing_if = "is_two_hundred")]
        radius: Anim<f64>,
    },
    /// Curls the x axis into an arc: `angle` degrees over `length` pixels.
    Bend {
        #[serde(default = "ninety", skip_serializing_if = "is_ninety")]
        angle: Anim<f64>,
        #[serde(default = "four_hundred", skip_serializing_if = "is_four_hundred")]
        length: Anim<f64>,
    },
    /// A sine along x moving y: `speed` wavelengths per second.
    Wave {
        #[serde(default = "twenty", skip_serializing_if = "is_twenty")]
        amplitude: Anim<f64>,
        #[serde(default = "two_hundred", skip_serializing_if = "is_two_hundred")]
        wavelength: Anim<f64>,
        #[serde(default = "o", skip_serializing_if = "is_o")]
        speed: Anim<f64>,
    },
}

fn twenty() -> Anim<f64> {
    Anim::Value(20.)
}
fn is_twenty(a: &Anim<f64>) -> bool {
    *a == twenty()
}
fn freq() -> Anim<f64> {
    Anim::Value(0.01)
}
fn is_freq(a: &Anim<f64>) -> bool {
    *a == freq()
}
fn ninety() -> Anim<f64> {
    Anim::Value(90.)
}
fn is_ninety(a: &Anim<f64>) -> bool {
    *a == ninety()
}
fn two_hundred() -> Anim<f64> {
    Anim::Value(200.)
}
fn is_two_hundred(a: &Anim<f64>) -> bool {
    *a == two_hundred()
}
fn four_hundred() -> Anim<f64> {
    Anim::Value(400.)
}
fn is_four_hundred(a: &Anim<f64>) -> bool {
    *a == four_hundred()
}

impl Deformer {
    /// Its numeric properties, by JSON name.
    pub fn nums(&self) -> Vec<(&'static str, &Anim<f64>)> {
        match self {
            Self::Noise {
                amount,
                frequency,
                speed,
                ..
            } => vec![
                ("amount", amount),
                ("frequency", frequency),
                ("speed", speed),
            ],
            Self::Twist { angle, radius } => vec![("angle", angle), ("radius", radius)],
            Self::Bend { angle, length } => vec![("angle", angle), ("length", length)],
            Self::Wave {
                amplitude,
                wavelength,
                speed,
            } => vec![
                ("amplitude", amplitude),
                ("wavelength", wavelength),
                ("speed", speed),
            ],
        }
    }
    pub fn at(&self, t: f64) -> Deform {
        match self {
            Self::Noise {
                amount,
                frequency,
                speed,
                seed,
            } => Deform::Noise {
                amount: amount.at(t),
                frequency: frequency.at(t),
                phase: speed.at(t) * t,
                seed: *seed,
            },
            Self::Twist { angle, radius } => Deform::Twist {
                angle: angle.at(t),
                radius: radius.at(t).max(1e-6),
            },
            Self::Bend { angle, length } => Deform::Bend {
                angle: angle.at(t),
                length: length.at(t).max(1e-6),
            },
            Self::Wave {
                amplitude,
                wavelength,
                speed,
            } => Deform::Wave {
                amplitude: amplitude.at(t),
                wavelength: wavelength.at(t).max(1e-6),
                phase: speed.at(t) * t,
            },
        }
    }
}

/// A [`Deformer`] at one time: what the renderer applies.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Deform {
    Noise {
        amount: f64,
        frequency: f64,
        phase: f64,
        seed: u32,
    },
    Twist {
        angle: f64,
        radius: f64,
    },
    Bend {
        angle: f64,
        length: f64,
    },
    Wave {
        amplitude: f64,
        wavelength: f64,
        phase: f64,
    },
}

/// For each layer of `scene`, whether a duplicator instances it in place
/// of drawing it: a hiding duplicator's `source` and everything parented
/// under it.
pub(crate) fn instanced(scene: &crate::Scene) -> Vec<bool> {
    let mut out = vec![false; scene.layers.len()];
    for l in &scene.layers {
        if let crate::Kind::Duplicator {
            source,
            show_source: false,
            ..
        } = &l.kind
            && let Some(s) = scene.layers.iter().position(|o| &o.id == source)
        {
            for i in subtree(scene, s, &[]) {
                out[i] = true;
            }
        }
    }
    out
}

/// Layer `s` and its descendants, in scene order, not going into the
/// layers in `skip` (other duplicators' hidden sources: those draw as
/// their duplicator's copies).
fn subtree(scene: &crate::Scene, s: usize, skip: &[usize]) -> Vec<usize> {
    let mut out = vec![s];
    let mut k = 0;
    while k < out.len() {
        let p = &scene.layers[out[k]].id;
        for (c, l) in scene.layers.iter().enumerate() {
            if &l.parent == p && !skip.contains(&c) && !out.contains(&c) {
                out.push(c);
            }
        }
        k += 1;
    }
    out.sort_unstable();
    out
}

/// Duplicators with a `source`: each copy gets the source layer and its
/// subtree as they draw now, re-based so the source's own transform is the
/// copy's (its offsets, animators and slot), in the duplicator's `comp`
/// (ids `dup/copy/layer`). Hiding sources then draw nothing themselves. A
/// duplicator whose source holds another one runs after it, so copies of
/// copies nest.
pub(crate) fn instance(scene: &crate::Scene, layers: &mut [crate::Drawn], t: f64, three: bool) {
    use crate::{Kind, place::Xf};
    let src = |i: usize| match &scene.layers[i].kind {
        Kind::Duplicator { source, .. } if !source.is_empty() => {
            scene.layers.iter().position(|o| &o.id == source)
        }
        _ => None,
    };
    let mut pending: Vec<usize> = (0..scene.layers.len())
        .filter(|&i| src(i).is_some())
        .collect();
    if pending.is_empty() {
        return;
    }
    let hidden = instanced(scene);
    let skip: Vec<usize> = pending
        .iter()
        .filter(|&&i| {
            matches!(
                scene.layers[i].kind,
                Kind::Duplicator {
                    show_source: false,
                    ..
                }
            )
        })
        .filter_map(|&i| src(i))
        .collect();
    while !pending.is_empty() {
        let before = pending.len();
        let mut k = 0;
        while k < pending.len() {
            let i = pending[k];
            let s = src(i).expect("pending has sources");
            let skip: Vec<usize> = skip.iter().copied().filter(|&o| o != s).collect();
            let sub = subtree(scene, s, &skip);
            // Itself under its source (loading refuses it), or waiting on
            // a duplicator inside.
            if sub.contains(&i) || sub.iter().any(|j| pending.contains(j) && *j != i) {
                k += 1;
                continue;
            }
            let at = Xf::of(&layers[i], three);
            let source = Xf::of(&layers[s], false);
            let Kind::Duplicator {
                layout,
                along,
                orient,
                ..
            } = &layers[i].kind
            else {
                unreachable!("a source is a duplicator's")
            };
            let slots = crate::vector::slots(&layers[i], *layout, along, *orient);
            let mut comp = Vec::new();
            if source.opacity > 0. && source.scale != 0. && layers[i].opacity > 0. {
                let from = source.inverse();
                let members: Vec<&crate::Drawn> = sub
                    .iter()
                    .map(|&j| &layers[j])
                    .filter(|d| {
                        !matches!(
                            d.kind,
                            Kind::Camera { .. }
                                | Kind::Light { .. }
                                | Kind::Model { .. }
                                | Kind::Audio { .. }
                                | Kind::Group
                        )
                    })
                    .collect();
                // Tints and hue jitter recolour each copied layer from its
                // own fill.
                let m = members.len().max(1);
                let animators = &scene.layers[i].animators;
                let fills = animators
                    .iter()
                    .any(|a| !is_clear(&a.fill) || !is_z(&a.jitter_hue))
                    .then(|| {
                        let n = slots.len() * m;
                        let pos: Vec<[f64; 2]> = slots.iter().map(|(p, _)| [p.x, p.y]).collect();
                        apply_to(
                            animators,
                            n,
                            &|e| members[e % m].fill,
                            t,
                            |_| (0..n).map(|e| e / m).collect(),
                            &pos,
                        )
                    });
                for (c, ((p, turn), f)) in slots.iter().zip(&layers[i].fx).enumerate() {
                    if f.opacity <= 0. || f.scale == 0. {
                        continue;
                    }
                    let copy = at.then(&Xf {
                        x: p.x + f.x,
                        y: p.y + f.y,
                        rotation: turn + f.rotation,
                        scale: f.scale,
                        opacity: f.opacity,
                        ..Xf::FRAME
                    });
                    for (mi, m) in members.iter().enumerate() {
                        let mut d = (*m).clone();
                        if let Some(fills) = &fills {
                            d.fill = fills[c * members.len() + mi].fill;
                        }
                        crate::place::rebase(&mut d, &from, &copy, three);
                        d.opacity = d.opacity.min(1.);
                        d.id = format!("{}/{c}/{}", layers[i].id, m.id);
                        comp.push(d);
                    }
                }
            }
            layers[i].comp = comp;
            pending.remove(k);
        }
        if pending.len() == before {
            break;
        }
    }
    for (d, h) in layers.iter_mut().zip(hidden) {
        if h {
            d.opacity = 0.;
            d.comp.clear();
        }
    }
}

/// Duplicator `l`'s `source` is another layer of `s`, not one it sits
/// under (which would instance itself).
pub(crate) fn check_source(s: &crate::Scene, l: &crate::Layer) -> Result<(), String> {
    let crate::Kind::Duplicator { source, .. } = &l.kind else {
        return Ok(());
    };
    if source.is_empty() {
        return Ok(());
    }
    let Some(i) = s.layers.iter().position(|o| &o.id == source) else {
        return Err(format!("layer `{}`: no layer `{source}` to instance", l.id));
    };
    let me = s.layers.iter().position(|o| o.id == l.id);
    if subtree(s, i, &[]).into_iter().any(|j| Some(j) == me) {
        return Err(format!(
            "layer `{}`: `{source}` holds this duplicator; it cannot instance itself",
            l.id
        ));
    }
    Ok(())
}
