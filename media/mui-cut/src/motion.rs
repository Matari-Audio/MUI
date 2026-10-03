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
    /// Nearest first, from the effector's centre (`falloff`), else the
    /// layer's origin: a unit's rank is its distance, scaled so the
    /// farthest is last, so units equally far start together (a ripple).
    /// A text unit sits at its glyphs' centres.
    Distance,
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
    /// How the stagger's delays spread over the ranks: `linear` evenly;
    /// `in` bunches the first ranks together and spaces the last, `out`
    /// the reverse. The last rank still waits `(n - 1) * stagger`.
    #[serde(default, skip_serializing_if = "is_default")]
    pub stagger_ease: Ease,
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
/// inside and out. A text glyph (word, line) weighs at its centre in the
/// laid-out block.
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
    /// rise and fade in, one after another), `cascade_out` (its exit), `pop` (copies or glyphs scale
    /// up from nothing in a random order), `cascade_children` (a group's
    /// children rise in one after another) or `ripple` (units pop in
    /// nearest the effector's centre first).
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
            // On a group: each child rises in after the one before.
            "cascade_children" => Self {
                amount: keys(1., 0., Interp::Bezier),
                stagger: Anim::Value(0.12),
                y: Anim::Value(60.),
                opacity: z(),
                ..Self::default()
            },
            // Units pop in as a ripple from the effector's centre reaches
            // them (its field covers everything; drag the centre).
            "ripple" => Self {
                order: Order::Distance,
                amount: keys(1., 0., Interp::Bezier),
                stagger: Anim::Value(0.05),
                scale: z(),
                falloff: Some(Effector {
                    radius: Anim::Value(4000.),
                    softness: z(),
                    ..serde_json::from_str("{}").expect("every field has a default")
                }),
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

    /// Each unit's rank, 0..n-1 (fractional for `distance`), at `t`.
    fn ranks(&self, n: usize, t: f64, pos: &[[f64; 2]]) -> Vec<f64> {
        if self.order != Order::Distance || pos.len() != n {
            let order = match self.order {
                Order::Distance => Order::Forward,
                o => o,
            };
            return ranks(order, self.seed, n)
                .into_iter()
                .map(|r| r as f64)
                .collect();
        }
        let c = self
            .falloff
            .as_ref()
            .map_or([0., 0.], |e| [e.x.at(t), e.y.at(t)]);
        let d: Vec<f64> = pos
            .iter()
            .map(|p| (p[0] - c[0]).hypot(p[1] - c[1]))
            .collect();
        let max = d.iter().copied().fold(0., f64::max);
        d.iter()
            .map(|&d| {
                if max > 0. {
                    d / max * (n - 1) as f64
                } else {
                    0.
                }
            })
            .collect()
    }

    /// Seconds rank `r` of `n` waits at `t`.
    fn delay(&self, r: f64, n: usize, t: f64) -> f64 {
        let s = self.stagger.at(t);
        if self.stagger_ease == Ease::Linear || n < 2 {
            return r * s;
        }
        let last = (n - 1) as f64;
        ease(self.stagger_ease, r / last) * last * s
    }

    /// Each unit's weight at `t`, `n` units at `pos` (or none: text).
    fn weights(&self, n: usize, t: f64, pos: &[[f64; 2]]) -> Vec<f64> {
        let ranks = self.ranks(n, t, pos);
        ranks
            .iter()
            .enumerate()
            .map(|(u, &r)| {
                let t = t - self.delay(r, n, t);
                let (lo, hi) = (r / n as f64, (r + 1.) / n as f64);
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
                let w = ease(self.ease, w);
                let field = match (&self.falloff, pos.get(u)) {
                    (Some(e), Some(&p)) => e.weight(p, t),
                    _ => 1.,
                };
                w * field * self.amount.at(t)
            })
            .collect()
    }
}

/// `w` (0..1) through `e`.
fn ease(e: Ease, w: f64) -> f64 {
    match e {
        Ease::Linear => w,
        Ease::In => w * w,
        Ease::Out => 1. - (1. - w) * (1. - w),
        Ease::InOut => w * w * (3. - 2. * w),
        Ease::Step => f64::from(u8::from(w >= 0.5)),
    }
}

/// Unit `u`'s rank in `order` (`distance` is not ranked here).
fn ranks(order: Order, seed: u32, n: usize) -> Vec<usize> {
    match order {
        Order::Forward | Order::Distance => (0..n).collect(),
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

/// [`apply`] with each element's own base fill and place (`pos`, one per
/// element in the layer's own pixels, or none): a unit sits at the mean of
/// its elements' places, for effectors and `distance` order.
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
        let pos = unit_places(&units, n, pos);
        let w = a.weights(n, t, &pos);
        let ranks = a.ranks(n, t, &pos);
        for (f, &u) in fx.iter_mut().zip(&units) {
            let w = w[u];
            if w == 0. {
                continue;
            }
            // The unit's own clock, as its weight used.
            let t = t - a.delay(ranks[u], n, t);
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

/// Each of `n` units' place: the mean of its elements' (none without one
/// per element).
fn unit_places(units: &[usize], n: usize, pos: &[[f64; 2]]) -> Vec<[f64; 2]> {
    if pos.len() != units.len() {
        return Vec::new();
    }
    let mut sum = vec![[0., 0., 0.]; n];
    for (&u, p) in units.iter().zip(pos) {
        sum[u] = [sum[u][0] + p[0], sum[u][1] + p[1], sum[u][2] + 1.];
    }
    sum.iter()
        .map(|&[x, y, k]| [x / k.max(1.), y / k.max(1.)])
        .collect()
}

/// Whether text animators `a` weigh glyphs by place (an effector, or
/// `distance` order): then the renderer, which knows the font's
/// advances, runs them ([`crate::Drawn::glyph_motion`]).
pub(crate) fn placed(a: &[Animator]) -> bool {
    a.iter()
        .any(|a| a.falloff.is_some() || a.order == Order::Distance)
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
                        let pos: Vec<[f64; 2]> =
                            (0..n).map(|e| [slots[e / m].0.x, slots[e / m].0.y]).collect();
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

/// Every animator property by path on a layer (`animators.0.x`,
/// `animators.0.falloff.radius`, `animators.0.fill`).
pub(crate) fn props(animators: &[Animator]) -> Vec<(String, crate::Prop<'_>)> {
    let mut out = Vec::new();
    for (i, a) in animators.iter().enumerate() {
        for (n, v) in a.nums() {
            out.push((format!("animators.{i}.{n}"), crate::Prop::Num(v)));
        }
        out.push((format!("animators.{i}.fill"), crate::Prop::Color(&a.fill)));
    }
    out
}

/// Groups with animators: their units are their child layers, in scene
/// order, each placed (for effectors and `distance`) at its own offset in
/// the group. The offsets go onto the children's own transforms before
/// parenting composes them, so a child's subtree follows it.
pub(crate) fn group_units(scene: &crate::Scene, layers: &mut [crate::Drawn], t: f64) {
    for (g, l) in scene.layers.iter().enumerate() {
        if !matches!(l.kind, crate::Kind::Group) || l.animators.is_empty() {
            continue;
        }
        let kids: Vec<usize> = (0..scene.layers.len())
            .filter(|&c| c != g && scene.layers[c].parent == l.id)
            .collect();
        let n = kids.len();
        let pos: Vec<[f64; 2]> = kids.iter().map(|&c| [layers[c].x, layers[c].y]).collect();
        let fx = apply_to(
            &l.animators,
            n,
            &|e| layers[kids[e]].fill,
            t,
            |_| (0..n).collect(),
            &pos,
        );
        for (&c, f) in kids.iter().zip(fx) {
            let d = &mut layers[c];
            d.x += f.x;
            d.y += f.y;
            d.rotation += f.rotation;
            d.scale *= f.scale;
            d.opacity *= f.opacity;
            d.fill = f.fill;
        }
    }
}

/// How a [`Behaviour`] moves its property.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Wave {
    /// Smooth seeded noise, up to `amount` either way.
    #[default]
    Wiggle,
    /// A sine of `amount`, `freq` cycles per second, from `phase`.
    Oscillate,
    /// A damped spring chasing the property's own keys: it rests on the
    /// first key and lags, overshoots and settles after every move, ringing
    /// at `freq` Hz with `damping` (0 rings forever, 1 settles without
    /// overshoot). `amount`, `seed` and `phase` are unused.
    Spring,
}

/// Motion added on top of a layer property's keys, every frame: a wiggle
/// or an oscillation of `amount` around its keyed value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(transform = crate::vars::bindable)]
pub struct Behaviour {
    /// The property moved: one of [`BEHAVIOUR_PROPS`].
    pub prop: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub kind: Wave,
    #[serde(default = "ten", skip_serializing_if = "is_ten")]
    pub amount: Anim<f64>,
    /// Cycles (oscillate) or features (wiggle) per second.
    #[serde(default = "o", skip_serializing_if = "is_o")]
    pub freq: Anim<f64>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub seed: u32,
    /// Cycles (or noise features) to start in at 0 s.
    #[serde(default = "z", skip_serializing_if = "is_z")]
    pub phase: Anim<f64>,
    /// Spring only: the damping ratio, 0 (rings forever) up; 1 is the
    /// fastest settle without overshoot.
    #[serde(default = "half", skip_serializing_if = "is_half")]
    pub damping: Anim<f64>,
}

fn half() -> Anim<f64> {
    Anim::Value(0.5)
}
fn is_half(a: &Anim<f64>) -> bool {
    *a == half()
}

fn ten() -> Anim<f64> {
    Anim::Value(10.)
}
fn is_ten(a: &Anim<f64>) -> bool {
    *a == ten()
}

/// What a behaviour can move.
pub const BEHAVIOUR_PROPS: [&str; 16] = [
    "x",
    "y",
    "z",
    "scale",
    "rotation",
    "rx",
    "ry",
    "opacity",
    "width",
    "height",
    "radius",
    "font_size",
    "tracking",
    "stroke_width",
    "path_offset",
    "ring_radius",
];

impl Behaviour {
    /// Its numeric properties, by JSON name: the ones its kind reads.
    pub fn props(&self) -> &'static [&'static str] {
        match self.kind {
            Wave::Spring => &["freq", "damping"],
            _ => &["amount", "freq", "phase"],
        }
    }

    pub fn num(&self, name: &str) -> Option<&Anim<f64>> {
        Some(match name {
            "amount" => &self.amount,
            "freq" => &self.freq,
            "phase" => &self.phase,
            "damping" => &self.damping,
            _ => return None,
        })
    }

    /// Where the spring is at `t` chasing `target`: at rest on its first
    /// key, then stepped forward from there in fixed 1/240 s steps (the
    /// last one shorter, ending on `t`), so any frame renders on its own,
    /// the same every time. Damping is taken implicitly: stable however
    /// stiff. ponytail: re-simulates from the first key every call
    /// (240 steps per second of scene); cache per layer if hour-long scenes
    /// need it.
    pub fn spring(&self, target: &Anim<f64>, t: f64) -> f64 {
        const DT: f64 = 1. / 240.;
        let Anim::Keys(keys) = target else {
            return target.at(t);
        };
        let t0 = keys.iter().map(|k| k.t).fold(f64::INFINITY, f64::min);
        if t <= t0 {
            return target.at(t);
        }
        let steps = ((t - t0) / DT).ceil() as u64;
        let (mut x, mut v) = (target.at(t0), 0.);
        for i in 0..steps {
            let s = t0 + i as f64 * DT;
            let h = DT.min(t - s);
            let w = std::f64::consts::TAU * self.freq.at(s).clamp(0.01, 60.);
            let z = self.damping.at(s).max(0.);
            v = (v + h * w * w * (target.at(s + h) - x)) / (1. + 2. * z * w * h);
            x += h * v;
        }
        x
    }

    /// The offset at `t` (a spring's is [`Behaviour::spring`]'s, which
    /// needs its target: see [`behave`]).
    pub fn at(&self, t: f64) -> f64 {
        let x = self.freq.at(t) * t + self.phase.at(t);
        let a = self.amount.at(t);
        match self.kind {
            Wave::Spring => 0.,
            Wave::Oscillate => a * (std::f64::consts::TAU * x).sin(),
            Wave::Wiggle => {
                // A different curve per property, from one seed.
                let seed = self.prop.bytes().fold(self.seed, |h, b| {
                    h.wrapping_mul(31).wrapping_add(u32::from(b))
                });
                a * wiggle(seed, x)
            }
        }
    }
}

/// Smooth value noise in -1..1: seeded values at whole `x`, joined by
/// Catmull-Rom (so it moves through them, never flat at them).
fn wiggle(seed: u32, x: f64) -> f64 {
    let i = x.floor();
    let f = x - i;
    let v = |k: f64| {
        let n = (i + k) as i64 as u64;
        (hash(seed, n) >> 11) as f64 / (1u64 << 52) as f64 - 1.
    };
    let (p0, p1, p2, p3) = (v(-1.), v(0.), v(1.), v(2.));
    let c = 0.5
        * (2. * p1
            + (-p0 + p2) * f
            + (2. * p0 - 5. * p1 + 4. * p2 - p3) * f * f
            + (-p0 + 3. * p1 - 3. * p2 + p3) * f * f * f);
    c.clamp(-1., 1.)
}

/// Every behaviour property by path (`behaviours.0.amount`).
pub(crate) fn behaviour_props(bs: &[Behaviour]) -> Vec<(String, crate::Prop<'_>)> {
    bs.iter()
        .enumerate()
        .flat_map(|(i, b)| {
            b.props().iter().map(move |&n| {
                let a = b.num(n).expect("PROPS are props");
                (format!("behaviours.{i}.{n}"), crate::Prop::Num(a))
            })
        })
        .collect()
}

/// `d`'s evaluated property `prop`, to move.
fn slot<'a>(d: &'a mut crate::Drawn, prop: &str) -> Option<&'a mut f64> {
    Some(match prop {
        "x" => &mut d.x,
        "y" => &mut d.y,
        "z" => &mut d.space.z,
        "scale" => &mut d.scale,
        "rotation" => &mut d.rotation,
        "rx" => &mut d.space.rx,
        "ry" => &mut d.space.ry,
        "opacity" => &mut d.opacity,
        "width" => &mut d.width,
        "height" => &mut d.height,
        "radius" => &mut d.radius,
        "font_size" => &mut d.font_size,
        "tracking" => &mut d.tracking,
        "stroke_width" => &mut d.stroke_width,
        "path_offset" => &mut d.path_offset,
        "ring_radius" => &mut d.ring_radius,
        _ => return None,
    })
}

/// Layer `l`'s own animation of `prop`: what a spring chases.
fn target<'a>(l: &'a crate::Layer, prop: &str) -> Option<&'a Anim<f64>> {
    Some(match prop {
        "x" => &l.x,
        "y" => &l.y,
        "z" => &l.z,
        "scale" => &l.scale,
        "rotation" => &l.rotation,
        "rx" => &l.rx,
        "ry" => &l.ry,
        "opacity" => &l.opacity,
        "width" => &l.width,
        "height" => &l.height,
        "radius" => &l.radius,
        "font_size" => &l.font_size,
        "tracking" => &l.tracking,
        "stroke_width" => &l.stroke_width,
        "path_offset" => &l.path_offset,
        "ring_radius" => &l.ring_radius,
        _ => return None,
    })
}

/// Layer `l`'s behaviours added to its evaluated values at `t`, kept in
/// range (opacity 0..1, sizes not below 0).
pub(crate) fn behave(l: &crate::Layer, d: &mut crate::Drawn, t: f64) {
    for b in &l.behaviours {
        let on = l.on(t);
        let off = match (b.kind, target(l, &b.prop)) {
            (Wave::Spring, Some(a)) => b.spring(a, t) - a.at(t),
            _ => b.at(t),
        };
        if let Some(v) = slot(d, &b.prop) {
            *v += off;
            match b.prop.as_str() {
                // Off stays off.
                "opacity" => *v = if on { v.clamp(0., 1.) } else { 0. },
                "width" | "height" | "radius" | "stroke_width" | "ring_radius" => *v = v.max(0.),
                "font_size" => *v = v.max(1.),
                _ => {}
            }
        }
    }
}

/// Every behaviour of `l` names a property it can move.
pub(crate) fn check_behaviours(l: &crate::Layer) -> Result<(), String> {
    for (i, b) in l.behaviours.iter().enumerate() {
        if !BEHAVIOUR_PROPS.contains(&b.prop.as_str()) {
            return Err(format!(
                "behaviours[{i}].prop: layer `{}`: `{}` is not one of {}",
                l.id,
                b.prop,
                BEHAVIOUR_PROPS.join(", ")
            ));
        }
    }
    Ok(())
}
