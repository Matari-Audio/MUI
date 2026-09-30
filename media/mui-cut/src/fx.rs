//! Effects: post-processing passes over a layer's pixels or the whole
//! scene's. In the file a stack is `"effects": [{"type": "grain", "amount":
//! 0.2}, ...]` on a layer or a scene; every parameter is a property like any
//! other, a plain value or keys. [`EFFECTS`] is the schema: the loader, the
//! inspector and the GPU passes (`fx/gpu.rs`, one WGSL file per effect) all
//! read it.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Anim, Rgba};

pub(crate) mod gpu;

/// One effect in a stack, as the file has it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    #[serde(rename = "type")]
    pub kind: String,
    /// Left out: the schema's default.
    #[serde(flatten)]
    pub params: BTreeMap<String, Prop>,
}

/// A parameter: a number or a colour, plain or keyed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Prop {
    Num(Anim<f64>),
    Color(Anim<Rgba>),
}

/// An effect evaluated at a time: every parameter present, clamped.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Fx {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub values: BTreeMap<String, Val>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Val {
    Num(f64),
    Color(Rgba),
}

/// What a parameter holds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Ty {
    Num { default: f64, min: f64, max: f64 },
    Color { default: Rgba },
}

#[derive(Debug, Serialize)]
pub struct Param {
    pub name: &'static str,
    #[serde(flatten)]
    pub ty: Ty,
}

/// One effect type. Colour parameters come first: the shader's `Params`
/// struct declares them as `vec4<f32>`, which must sit on 16 bytes.
#[derive(Debug, Serialize)]
pub struct Def {
    pub name: &'static str,
    pub about: &'static str,
    /// Full-frame passes it takes (a separable blur takes two).
    pub passes: u32,
    pub params: &'static [Param],
}

const fn num(name: &'static str, default: f64, min: f64, max: f64) -> Param {
    Param {
        name,
        ty: Ty::Num { default, min, max },
    }
}
const fn color(name: &'static str, rgba: [u8; 4]) -> Param {
    Param {
        name,
        ty: Ty::Color {
            default: Rgba(rgba),
        },
    }
}

/// Lengths are in project pixels, so an effect looks the same at any
/// `--size`.
pub const EFFECTS: &[Def] = &[
    Def {
        name: "grain",
        about: "film grain, new every frame (not every subframe)",
        passes: 1,
        params: &[
            num("amount", 0.12, 0., 1.),
            num("size", 1.5, 0.5, 16.),
            num("seed", 0., 0., 1e6),
        ],
    },
    Def {
        name: "chromatic",
        about: "lens chromatic aberration: red and blue pulled apart towards the edges",
        passes: 1,
        params: &[num("amount", 6., 0., 200.)],
    },
    Def {
        name: "crt",
        about: "a CRT: barrel curvature, scanlines and a vignette",
        passes: 1,
        params: &[
            num("curvature", 0.12, 0., 1.),
            num("scanlines", 0.35, 0., 1.),
            num("line", 4., 1., 64.),
            num("vignette", 0.35, 0., 1.),
        ],
    },
    Def {
        name: "displace",
        about: "pixels pushed around by smooth noise that evolves over time",
        passes: 1,
        params: &[
            num("amount", 16., 0., 500.),
            num("scale", 140., 4., 4000.),
            num("speed", 0.5, 0., 20.),
            num("seed", 0., 0., 1e6),
        ],
    },
    Def {
        name: "blur",
        about: "gaussian blur; radius is the standard deviation",
        passes: 2,
        params: &[num("radius", 8., 0., 64.)],
    },
    Def {
        name: "directional_blur",
        about: "a blur along one direction, like fast motion",
        passes: 1,
        params: &[num("length", 40., 0., 400.), num("angle", 0., -360., 360.)],
    },
    Def {
        name: "levels",
        about: "input black/white points, gamma, saturation and a tint by luminance",
        passes: 1,
        params: &[
            color("tint", [255, 255, 255, 255]),
            num("black", 0., 0., 1.),
            num("white", 1., 0., 1.),
            num("gamma", 1., 0.1, 10.),
            num("saturation", 1., 0., 4.),
            num("tint_amount", 0., 0., 1.),
        ],
    },
    Def {
        name: "plasma",
        about: "generator: a plasma field between two colours, masked by the layer's shape",
        passes: 1,
        params: &[
            color("color_a", [0xff, 0x3c, 0xac, 255]),
            color("color_b", [0x2b, 0x86, 0xc5, 255]),
            num("scale", 120., 4., 4000.),
            num("speed", 1., -20., 20.),
        ],
    },
];

pub fn def(kind: &str) -> Option<(usize, &'static Def)> {
    EFFECTS.iter().enumerate().find(|(_, d)| d.name == kind)
}

/// Sort keys, and refuse unknown effects or parameters, the wrong type, empty
/// key lists and non-finite numbers.
pub(crate) fn check(stack: &mut [Effect], owner: &str) -> Result<(), String> {
    for e in stack {
        let (_, d) = def(&e.kind).ok_or_else(|| {
            let known: Vec<_> = EFFECTS.iter().map(|d| d.name).collect();
            format!(
                "{owner}: unknown effect `{}` (known: {})",
                e.kind,
                known.join(", ")
            )
        })?;
        for (name, p) in &mut e.params {
            let bad = |why: &str| format!("{owner}: effect `{}` `{name}` {why}", e.kind);
            let ty = d
                .params
                .iter()
                .find(|q| q.name == name)
                .ok_or_else(|| bad("is not a parameter"))?
                .ty;
            match (p, ty) {
                (Prop::Num(a), Ty::Num { .. }) => {
                    match a {
                        Anim::Keys(k) if k.is_empty() => return Err(bad("has an empty key list")),
                        Anim::Value(v) if !v.is_finite() => return Err(bad("is not finite")),
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
                            return Err(bad("has a non-finite key"));
                        }
                        _ => {}
                    }
                }
                (Prop::Color(a), Ty::Color { .. }) => {
                    if matches!(a, Anim::Keys(k) if k.is_empty()) {
                        return Err(bad("has an empty key list"));
                    }
                }
                (_, Ty::Num { .. }) => return Err(bad("must be a number")),
                (_, Ty::Color { .. }) => return Err(bad("must be a #rrggbb colour")),
            }
        }
    }
    Ok(())
}

/// A stack at `t`: every parameter, defaults filled in, numbers clamped.
pub(crate) fn eval(stack: &[Effect], t: f64) -> Vec<Fx> {
    stack
        .iter()
        .filter_map(|e| {
            let (_, d) = def(&e.kind)?;
            let values = d
                .params
                .iter()
                .map(|p| {
                    let v = match (p.ty, e.params.get(p.name)) {
                        (Ty::Num { min, max, .. }, Some(Prop::Num(a))) => {
                            Val::Num(a.at(t).clamp(min, max))
                        }
                        (Ty::Num { default, .. }, _) => Val::Num(default),
                        (Ty::Color { .. }, Some(Prop::Color(a))) => Val::Color(a.at(t)),
                        (Ty::Color { default }, _) => Val::Color(default),
                    };
                    (p.name.to_owned(), v)
                })
                .collect();
            Some(Fx {
                kind: e.kind.clone(),
                values,
            })
        })
        .collect()
}

impl Fx {
    /// The shader's `Params`, in schema order: a colour is four floats
    /// (straight sRGB, 0..1), a number one.
    pub(crate) fn pack(&self) -> Option<(usize, Vec<f32>)> {
        let (i, d) = def(&self.kind)?;
        let mut out = Vec::with_capacity(16);
        for p in d.params {
            match self.values.get(p.name)? {
                Val::Num(v) => out.push(*v as f32),
                Val::Color(c) => out.extend(c.0.map(|b| f32::from(b) / 255.)),
            }
        }
        Some((i, out))
    }
}
