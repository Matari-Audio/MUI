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
}

/// The numeric properties of an [`Animator`], by JSON name.
pub const ANIMATOR_PROPS: [&str; 11] = [
    "start", "end", "offset", "amount", "stagger", "x", "y", "scale", "rotation", "opacity",
    "tracking",
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
            _ => return None,
        })
    }

    /// A classic setup as JSON-ready data, keyed over `[t0, t0 + dur]`:
    /// `typewriter` (characters appear one by one), `cascade` (characters
    /// rise and fade in, one after another) or `pop` (copies or glyphs scale
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
                    t: t0 + dur,
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

    /// Each unit's weight at `t`: `ranks[u]` is unit `u`'s rank, `n` units.
    fn weights(&self, n: usize, t: f64) -> Vec<f64> {
        let ranks = ranks(self.order, self.seed, n);
        ranks
            .iter()
            .map(|&r| {
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
                w * self.amount.at(t)
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
    let mut fx = vec![
        Fx {
            x: 0.,
            y: 0.,
            scale: 1.,
            rotation: 0.,
            opacity: 1.,
            tracking: 0.,
            fill,
        };
        elements
    ];
    for a in animators {
        let units = unit_of(a.by);
        let n = units.iter().max().map_or(0, |m| m + 1);
        let w = a.weights(n, t);
        let ranks = ranks(a.order, a.seed, n);
        for (f, &u) in fx.iter_mut().zip(&units) {
            let w = w[u];
            if w == 0. {
                continue;
            }
            // The unit's own clock, as its weight used.
            let t = t - ranks[u] as f64 * a.stagger.at(t);
            f.x += w * a.x.at(t);
            f.y += w * a.y.at(t);
            f.scale *= 1. + w * (a.scale.at(t) - 1.);
            f.rotation += w * a.rotation.at(t);
            f.opacity *= (1. + w * (a.opacity.at(t) - 1.)).clamp(0., 1.);
            f.tracking += w * a.tracking.at(t);
            let tint = a.fill.at(t);
            let k = w * f64::from(tint.0[3]) / 255.;
            f.fill = crate::Tween::mix(
                &f.fill,
                &Rgba([tint.0[0], tint.0[1], tint.0[2], f.fill.0[3]]),
                k,
            );
        }
    }
    fx
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
