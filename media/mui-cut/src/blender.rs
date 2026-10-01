//! `--renderer blender`: a 3D scene rendered by Blender (EEVEE or Cycles)
//! running as a separate process. Nothing of Blender is linked or shipped:
//! mui-cut writes a JSON description of the scene (every object's state at
//! every output frame, from the same [`eval`] the other renderers draw),
//! paints each layer's 2D content to a PNG, and runs `blender -b` with
//! `blender.py`, which builds the scene, keys it and renders the frames not
//! cached yet.
//!
//! Blender's world is z up, y into the frame, in metres: one project pixel
//! is [`METRES`].
use std::collections::HashMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use mui_stage::Mat4;
use serde::Serialize;

use crate::gpu3d::linear;
use crate::{
    Assets, Drawn, Fnv, Frame, Kind, LightType, Mode, Project, SHUTTER, Scene, eval, three,
};

/// Metres per project pixel: a 1280-pixel frame is a 12.8 m set.
pub const METRES: f32 = 0.01;

/// The Blender side, written next to the description it reads.
pub const SCRIPT: &str = include_str!("blender.py");

/// Sensor height the lens is worked out for, millimetres.
const SENSOR: f64 = 24.;

/// How to render.
#[derive(Clone, Debug, Serialize)]
pub struct Options {
    /// `BLENDER_EEVEE` or `CYCLES`.
    pub engine: &'static str,
    pub samples: u32,
    /// Motion blur subframes (1 is none).
    pub mb: usize,
    /// Output pixels.
    pub size: [u32; 2],
}

impl Options {
    /// `engine` is `eevee` (the default) or `cycles`; `samples` defaults to
    /// 64 for EEVEE and 128 for Cycles.
    pub fn new(
        engine: Option<&str>,
        samples: Option<u32>,
        mb: usize,
        size: [u32; 2],
    ) -> Result<Self, String> {
        let (engine, default) = match engine {
            None | Some("eevee") => ("BLENDER_EEVEE", 64),
            Some("cycles") => ("CYCLES", 128),
            Some(e) => return Err(format!("--engine: `{e}` is not eevee or cycles")),
        };
        Ok(Self {
            engine,
            samples: samples.unwrap_or(default).max(1),
            mb: mb.clamp(1, 64),
            size,
        })
    }
}

/// The `blender` to run: `$MUI_CUT_BLENDER`, else `blender` on PATH.
pub fn binary() -> Result<PathBuf, String> {
    let bin = std::env::var_os("MUI_CUT_BLENDER").map_or_else(|| "blender".into(), PathBuf::from);
    match Command::new(&bin).arg("--version").output() {
        Ok(o) if o.status.success() => Ok(bin),
        _ => Err(format!(
            "--renderer blender needs Blender 4.2 or later: `{}` did not run. Install it \
             (https://www.blender.org/download/) or set MUI_CUT_BLENDER to its path",
            bin.display()
        )),
    }
}

/// Everything `blender.py` builds a scene from.
#[derive(Clone, Debug, Serialize)]
pub struct Desc {
    pub options: Options,
    /// Linear RGB the camera sees behind everything.
    pub background: [f32; 3],
    pub ground: Option<GroundD>,
    pub fog: Option<FogD>,
    /// The environment the world is lit by (see [`State::env`]).
    pub world: Option<WorldD>,
    pub lights: Vec<LightD>,
    pub layers: Vec<LayerD>,
    pub models: Vec<ModelD>,
    /// Per output frame, its states: one, or with motion blur the shutter
    /// open to closed in `mb` steps.
    pub frames: Vec<Vec<State>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GroundD {
    /// Height, metres.
    pub z: f32,
    pub color: [f32; 3],
    /// Fades by 1/e this far from the frame's centre, metres.
    pub radius: f32,
    pub roughness: f32,
    /// Principled "Specular IOR Level": 0 is matte, 0.5 plain dielectric.
    pub specular: f32,
}

/// The world's environment image: a file in the texture directory (the
/// built-in studio, baked) or an absolute path, and the hash of its bytes,
/// so the cache follows an edited file.
#[derive(Clone, Debug, Serialize)]
pub struct WorldD {
    pub image: String,
    pub hash: String,
    /// The camera sees it, not the background colour.
    pub background: bool,
}

/// Mist: surfaces fade into `color` from `start` metres over `depth`.
#[derive(Clone, Debug, Serialize)]
pub struct FogD {
    pub color: [f32; 3],
    pub start: f32,
    pub depth: f32,
}

#[derive(Clone, Debug, Serialize)]
pub struct LightD {
    pub id: String,
    /// `SUN`, `SPOT` or `POINT`.
    pub kind: &'static str,
    pub shadow: bool,
}

/// A layer: one object per distinct content it shows (usually one).
#[derive(Clone, Debug, Serialize)]
pub struct LayerD {
    pub id: String,
    pub shadow: bool,
    pub variants: Vec<Variant>,
}

/// One look of a layer: its texture on a card, or on a slab whose walls
/// follow `rings`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Variant {
    /// PNG file name in the texture directory.
    pub tex: String,
    /// Project pixels.
    pub size: [f32; 2],
    /// Object x/y (pixels, centred, y up) to texture u/v: scale, then offset.
    pub uv: [f32; 4],
    /// Closed outlines, pixels, centred, y up; empty on a flat card.
    pub rings: Vec<Vec<[f32; 2]>>,
    /// Slab thickness and bevel, pixels.
    pub depth: f32,
    pub bevel: f32,
    /// Walls, bevel and back, linear RGB.
    pub edge: [f32; 3],
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelD {
    pub id: String,
    /// Absolute path of the `.glb`.
    pub path: String,
    pub shadow: bool,
}

/// Every object at one instant. Matrices are Blender world matrices,
/// column-major.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct State {
    pub camera: CamS,
    /// World light, linear RGB.
    pub ambient: [f32; 3],
    /// The environment's strength and its turn about the world's z, in
    /// radians, as the Mapping node takes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<[f32; 2]>,
    pub lights: Vec<LightS>,
    pub layers: Vec<LayerS>,
    pub models: Vec<ModelS>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CamS {
    pub m: [f32; 16],
    /// Millimetres on a 24 mm tall sensor.
    pub lens: f32,
    /// Metres; 0 is no depth of field.
    pub focus: f32,
    pub fstop: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LightS {
    pub m: [f32; 16],
    pub color: [f32; 3],
    /// Watts (spot, point) or W/m² (sun).
    pub energy: f32,
    /// Sun: angular diameter, radians; spot and point: radius, metres.
    pub size: f32,
    /// Spot: full cone, radians, and its soft fraction.
    pub spot: f32,
    pub blend: f32,
    /// Metres; 0 is unlimited.
    pub range: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LayerS {
    pub m: [f32; 16],
    /// Which variant shows; -1 none.
    pub v: i32,
    pub a: f32,
    /// Its surface, every field set.
    pub mat: MatS,
}

/// A surface for the Principled BSDF: `material` of a layer or a model.
/// A model's sets only what its layer overrides; the rest is its glTF's.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MatS {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metallic: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roughness: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transmission: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ior: Option<f32>,
    /// Object units the light crosses inside: the Material Output's
    /// Thickness, which EEVEE reads in object space (a layer's are pixels).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thickness: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispersion: Option<f32>,
    /// Linear RGB multiplied into the base colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tint: Option<[f32; 3]>,
}

impl MatS {
    fn full(m: mui_stage::Material) -> Self {
        let f = |v: f32| Some(r(f64::from(v)));
        Self {
            metallic: f(m.metallic),
            roughness: f(m.roughness),
            transmission: f(m.transmission),
            ior: f(m.ior),
            thickness: f(m.thickness),
            dispersion: f(m.dispersion),
            tint: Some(r3(m.tint)),
        }
    }
    /// Only what `s` sets; `scale` takes its thickness to object units.
    fn over(s: &three::Surface, scale: f64) -> Self {
        let f = |v: Option<f64>| v.map(r);
        Self {
            metallic: f(s.metallic),
            roughness: f(s.roughness),
            transmission: f(s.transmission),
            ior: f(s.ior),
            thickness: f(s.thickness.map(|t| t * scale)),
            dispersion: f(s.dispersion),
            tint: s.tint.map(|c| r3(linear(c))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ModelS {
    pub m: [f32; 16],
    pub show: bool,
    /// Seconds into its glTF's animation, if it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mat: Option<MatS>,
}

/// mui-stage's built-in studio as an OpenEXR file, named by its content.
pub fn studio_exr() -> &'static (String, Vec<u8>) {
    static STUDIO: std::sync::OnceLock<(String, Vec<u8>)> = std::sync::OnceLock::new();
    STUDIO.get_or_init(|| {
        let bytes = mui_stage::EnvImage::studio()
            .to_exr()
            .expect("an in-memory EXR writes");
        let mut h = Fnv::default();
        let _ = h.write_all(&bytes);
        (format!("studio-{}.exr", h.hex()), bytes)
    })
}

/// A layer texture to write: file name, straight RGBA, pixel size.
pub struct Texture {
    pub name: String,
    pub rgba: Vec<u8>,
    pub size: [u16; 2],
}

/// Four decimals: plenty (0.1 mm), and the same text on every machine.
fn r(v: f64) -> f32 {
    ((v * 1e4).round() / 1e4) as f32
}
fn r3(v: [f32; 3]) -> [f32; 3] {
    v.map(|c| r(f64::from(c)))
}

/// mui-stage's world (y up, z toward the viewer, pixels) to Blender's
/// (z up, y into the frame): a rotation.
const TO_BLENDER: Mat4 = Mat4([
    1., 0., 0., 0., 0., 0., 1., 0., 0., -1., 0., 0., 0., 0., 0., 1.,
]);
const FROM_BLENDER: Mat4 = Mat4([
    1., 0., 0., 0., 0., 0., -1., 0., 0., 1., 0., 0., 0., 0., 0., 1.,
]);

/// A stage-world placement as a Blender world matrix, in metres.
pub(crate) fn placed(m: Mat4) -> [f32; 16] {
    (TO_BLENDER * Mat4::scale(METRES) * m)
        .0
        .map(|v| r(f64::from(v)))
}

/// A stage-world point or direction in Blender's axes.
fn axes(v: [f32; 3]) -> [f64; 3] {
    [f64::from(v[0]), -f64::from(v[2]), f64::from(v[1])]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(v: [f64; 3]) -> [f64; 3] {
    let l = v.iter().map(|c| c * c).sum::<f64>().sqrt().max(1e-12);
    v.map(|c| c / l)
}
/// Columns x, y, z and a position as a Blender matrix.
fn columns(x: [f64; 3], y: [f64; 3], z: [f64; 3], p: [f64; 3]) -> [f32; 16] {
    let mut m = [0.; 16];
    for (c, v) in [x, y, z, p].into_iter().enumerate() {
        for k in 0..3 {
            m[c * 4 + k] = r(v[k]);
        }
    }
    m[15] = 1.;
    m
}
/// A camera or lamp at `p` facing `forward` (Blender's -z), `up` its y.
fn facing(p: [f64; 3], forward: [f64; 3]) -> [f32; 16] {
    let z = norm(forward.map(|c| -c));
    let hint = if z[2].abs() > 0.999 {
        [0., 1., 0.]
    } else {
        [0., 0., 1.]
    };
    let x = norm(cross(hint, z));
    columns(x, cross(z, x), z, p)
}

/// A project point in Blender metres.
fn at(size: [u32; 2], p: [f64; 3]) -> [f64; 3] {
    axes(three::world(size, p)).map(|c| c * f64::from(METRES))
}

fn camera(size: [u32; 2], c: &three::Cam) -> (CamS, [f64; 3]) {
    let (eye, target) = (at(size, c.eye), at(size, c.target));
    let f = norm(sub(target, eye));
    let world_up = if f[2].abs() > 0.999 {
        [0., 1., 0.]
    } else {
        [0., 0., 1.]
    };
    let s = norm(cross(f, world_up));
    let u = cross(s, f);
    // Rolled as mui-stage rolls: up leans left by `roll`.
    let (sin, cos) = c.roll.to_radians().sin_cos();
    let up: [f64; 3] = std::array::from_fn(|i| u[i] * cos - s[i] * sin);
    let right: [f64; 3] = std::array::from_fn(|i| s[i] * cos + u[i] * sin);
    let lens = SENSOR / 2. / (c.fov.to_radians() / 2.).tan();
    let focus = c.focus * f64::from(METRES);
    // `aperture` is the blur radius, in project pixels, of what is
    // infinitely far behind the focus: a circle of confusion f²/(N(s-f)).
    let (focus, fstop) = if c.aperture > 0. && c.focus > 0. {
        let (fm, sm) = (lens, focus * 1000.);
        let h = f64::from(size[1]);
        let n = fm * fm * h / (2. * c.aperture * SENSOR * (sm - fm).max(1.));
        (focus, n.clamp(0.1, 1000.))
    } else {
        (0., 0.)
    };
    let cam = CamS {
        m: columns(right, up, f.map(|v| -v), eye),
        lens: r(lens),
        focus: r(focus),
        fstop: r(fstop),
    };
    (cam, target)
}

/// What a layer draws, placement and 3D pose left out: two instants with
/// equal keys look the same.
fn content(l: &Drawn) -> Drawn {
    Drawn {
        x: 0.,
        y: 0.,
        rotation: 0.,
        scale: 1.,
        opacity: 1.,
        space: three::Space {
            extrude: l.space.extrude,
            edge: l.space.edge,
            ..three::Space::default()
        },
        // Effects are not drawn here (see `render`).
        effects: Vec::new(),
        ..l.clone()
    }
}

/// Texture pixels per project pixel for output `size` of a project
/// `height` tall, for a layer shown up to `scale` times.
fn density(out: [u32; 2], height: u32, scale: f64) -> f64 {
    let base = (f64::from(out[1]) / f64::from(height) * 2.).clamp(1., 4.);
    base * 2f64.powf(scale.abs().max(1e-3).log2().ceil().clamp(-2., 2.))
}

/// `scene` at each of `times` as Blender will build it, and the layer
/// textures it names. `dir` resolves model paths.
pub fn describe(
    p: &Project,
    scene: &Scene,
    times: &[f64],
    assets: &Assets,
    dir: &Path,
    o: &Options,
) -> Result<(Desc, Vec<Texture>), String> {
    if scene.mode != Mode::ThreeD {
        return Err(format!(
            "scene `{}` is 2D: --renderer blender draws 3D scenes (\"mode\": \"3d\"); \
             render 2D with classic, gpu or cpu",
            scene.name
        ));
    }
    let size = p.size;
    let steps = if o.mb > 1 { o.mb + 1 } else { 1 };
    let frames: Vec<Vec<Frame>> = times
        .iter()
        .map(|&t| {
            (0..steps)
                .map(|k| eval(p, scene, t + SHUTTER / p.fps * k as f64 / o.mb as f64))
                .collect()
        })
        .collect();
    let slabbed: Vec<Vec<Vec<(String, Drawn)>>> = frames
        .iter()
        .map(|subs| subs.iter().map(|f| slabs(assets, f)).collect())
        .collect();

    // Content slabs by name: each distinct look, the most it is scaled.
    let mut layers: Vec<LayerD> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    // Per slab: its looks' numbers by key, and each look's content and the
    // most it is scaled.
    let mut looks: Vec<HashMap<String, usize>> = Vec::new();
    let mut firsts: Vec<Vec<(Drawn, f64)>> = Vec::new();
    for (name, d) in slabbed.iter().flatten().flatten() {
        let j = *index.entry(name.clone()).or_insert_with(|| {
            layers.push(LayerD {
                id: name.clone(),
                shadow: d.space.cast_shadows,
                variants: Vec::new(),
            });
            looks.push(HashMap::new());
            firsts.push(Vec::new());
            layers.len() - 1
        });
        let c = content(d);
        let key = serde_json::to_string(&c).map_err(|e| e.to_string())?;
        let n = *looks[j].entry(key).or_insert_with(|| {
            firsts[j].push((c, 0.));
            firsts[j].len() - 1
        });
        firsts[j][n].1 = firsts[j][n].1.max(d.scale.abs());
    }

    let mut lights = Vec::new();
    let mut models = Vec::new();
    for (i, l) in scene.layers.iter().enumerate() {
        match &l.kind {
            Kind::Light { light } => {
                let kind = match light {
                    LightType::Ambient => continue,
                    LightType::Directional => "SUN",
                    LightType::Spot => "SPOT",
                    LightType::Point => "POINT",
                };
                lights.push((
                    i,
                    LightD {
                        id: l.id.clone(),
                        kind,
                        shadow: l.cast_shadows,
                    },
                ));
            }
            Kind::Model { path } => {
                if assets.model(path).is_none() {
                    eprintln!("mui-cut: model `{path}` not loaded; left out");
                    continue;
                }
                let abs =
                    std::path::absolute(dir.join(path)).map_err(|e| format!("{path}: {e}"))?;
                models.push((
                    i,
                    ModelD {
                        id: l.id.clone(),
                        path: abs.display().to_string(),
                        shadow: l.cast_shadows,
                    },
                ));
            }
            _ => {}
        }
    }

    // Paint each look once; keep its box to place it by.
    let mut textures: Vec<Texture> = Vec::new();
    let mut boxes: HashMap<(usize, usize), (f64, f64, [f64; 2])> = HashMap::new();
    for (j, layer) in layers.iter_mut().enumerate() {
        for (n, (c, most)) in firsts[j].iter().enumerate() {
            let k = density(o.size, size[1], *most);
            let (rgba, [tw, th], bx, corner) = assets.paint(c, k)?;
            let mut h = Fnv::default();
            let _ = write!(h, "{tw}x{th}|");
            let _ = h.write_all(&rgba);
            let name = format!("{}.png", h.hex());
            let (w, hgt) = (bx.width, bx.height);
            let depth = c.space.extrude;
            let rings = if depth > 0. {
                ring_list(assets.outline(c, bx, corner), w, hgt)
            } else {
                Vec::new()
            };
            layer.variants.push(Variant {
                tex: name.clone(),
                size: [r(w), r(hgt)],
                uv: [
                    r(k / f64::from(tw)),
                    r(k / f64::from(th)),
                    r(w / 2. * k / f64::from(tw)),
                    r(1. - hgt / 2. * k / f64::from(th)),
                ],
                rings,
                depth: r(depth),
                bevel: r((depth * 0.15).min(2.)),
                edge: r3(linear(c.space.edge)),
            });
            boxes.insert((j, n), (w, hgt, [corner.x, corner.y]));
            if !textures.iter().any(|t| t.name == name) {
                textures.push(Texture {
                    name,
                    rgba,
                    size: [tw, th],
                });
            }
        }
    }

    let state = |f: &Frame, shown: &[(String, Drawn)]| -> Result<State, String> {
        let view = f.view.as_ref().ok_or("a 3D frame has a view")?;
        let (camera, target) = camera(size, &view.camera);
        let mut ambient = [0f32; 3];
        for d in &f.layers {
            if let Kind::Light {
                light: LightType::Ambient,
            } = d.kind
            {
                let k = (d.space.intensity * d.opacity) as f32;
                for (a, c) in ambient.iter_mut().zip(linear(d.fill)) {
                    *a += c * k;
                }
            }
        }
        let here: HashMap<&str, &Drawn> = shown.iter().map(|(n, d)| (n.as_str(), d)).collect();
        let layer_states: Vec<LayerS> = layers
            .iter()
            .enumerate()
            .map(|(j, layer)| {
                let Some(d) = here.get(layer.id.as_str()) else {
                    return Ok(LayerS {
                        m: placed(Mat4::IDENTITY),
                        v: -1,
                        a: 0.,
                        mat: MatS::full(mui_stage::Material::SLAB),
                    });
                };
                let key = serde_json::to_string(&content(d)).map_err(|e| e.to_string())?;
                let n = looks[j][&key];
                let (w, h, corner) = boxes[&(j, n)];
                let offset = [
                    (corner[0] + w / 2.) as f32,
                    -(corner[1] + h / 2.) as f32,
                    d.space.anchor_z as f32,
                ];
                let m = three::pose(
                    size,
                    d,
                    Mat4::scale(d.scale as f32) * Mat4::translate(offset),
                );
                let shown = d.opacity > 0. && d.scale != 0.;
                // As the stage draws it: a slab's glass is its depth thick
                // unless it says otherwise.
                let mut mat = d.space.material.map_or(mui_stage::Material::SLAB, |s| {
                    s.over(mui_stage::Material::SLAB)
                });
                if mat.thickness <= 0. {
                    mat.thickness = d.space.extrude as f32;
                }
                Ok(LayerS {
                    m: placed(m),
                    v: if shown { n as i32 } else { -1 },
                    a: r(d.opacity),
                    mat: MatS::full(mat),
                })
            })
            .collect::<Result<_, String>>()?;
        let model_states: Vec<ModelS> = models
            .iter()
            .map(|(i, _)| {
                let d = &f.layers[*i];
                let Kind::Model { path } = &d.kind else {
                    unreachable!("model slots hold models")
                };
                let mesh = assets.model(path).expect("checked above");
                let tall = (mesh.max[1] - mesh.min[1]).max(1e-6);
                let k = (d.height / f64::from(tall) * d.scale) as f32;
                let mid: [f32; 3] = std::array::from_fn(|a| -(mesh.min[a] + mesh.max[a]) / 2.);
                let m = three::pose(size, d, Mat4::scale(k) * Mat4::translate(mid));
                // Blender's glTF import already turned the model z up.
                let m = TO_BLENDER * Mat4::scale(METRES) * m * FROM_BLENDER;
                ModelS {
                    m: m.0.map(|v| r(f64::from(v))),
                    show: d.opacity > 0. && d.scale != 0.,
                    at: mesh.animated().then_some(d.time as f32),
                    // Pixels to the model's own units, as its glTF's.
                    mat: d
                        .space
                        .material
                        .as_ref()
                        .map(|s| MatS::over(s, 1. / f64::from(k))),
                }
            })
            .collect();
        // Where the layers and models shown stand: what lamps light.
        let subjects: Vec<[f64; 3]> = layer_states
            .iter()
            .filter(|l| l.v >= 0)
            .map(|l| l.m)
            .chain(
                model_states
                    .iter()
                    .filter(|m: &&ModelS| m.show)
                    .map(|m| m.m),
            )
            .map(|m| [12, 13, 14].map(|i| f64::from(m[i])))
            .collect();
        let lights = lights
            .iter()
            .map(|(i, ld)| {
                let d = &f.layers[*i];
                let s = &d.space;
                let pos = at(size, [d.x, d.y, s.z]);
                let dir = three::aim(s.rx, s.ry);
                let dir = axes([dir[0] as f32, -dir[1] as f32, -dir[2] as f32]);
                let k = s.intensity * d.opacity;
                // Blender's sun of π W/m² lights white to 1; a point or
                // spot gives P/(4π²d²). ponytail: lit to `intensity` at the
                // nearest subject it faces (else the camera's target), and
                // by the inverse square past it where mui-stage fades
                // linearly to `range`; Cycles' falloff node if that matters.
                let dist = |p: [f64; 3]| sub(p, pos).iter().map(|c| c * c).sum::<f64>().sqrt();
                let facing_it = |p: &[f64; 3]| {
                    ld.kind == "POINT" || (0..3).map(|a| (p[a] - pos[a]) * dir[a]).sum::<f64>() > 0.
                };
                let reach = subjects
                    .iter()
                    .filter(|p| facing_it(p))
                    .map(|p| dist(*p))
                    .reduce(f64::min)
                    .unwrap_or_else(|| dist(target));
                let energy = if ld.kind == "SUN" {
                    k * std::f64::consts::PI
                } else {
                    k * 4. * std::f64::consts::PI.powi(2) * reach.max(0.5).powi(2)
                };
                LightS {
                    m: facing(pos, dir),
                    color: r3(linear(d.fill)),
                    energy: r(energy),
                    size: if ld.kind == "SUN" {
                        r((s.softness * 1.5).to_radians())
                    } else {
                        r(s.softness * 8. * f64::from(METRES))
                    },
                    spot: r((2. * s.cone).to_radians()),
                    blend: r(s.feather),
                    range: r(s.range * f64::from(METRES)),
                }
            })
            .collect();
        // The stage turns its lookup by -rotation about y (y up); in
        // Blender's z-up world that is the same turn about z.
        let env = view
            .environment
            .as_ref()
            .map(|e| [r(e.intensity), r(-e.rotation.to_radians())]);
        Ok(State {
            camera,
            ambient: r3(ambient),
            env,
            lights,
            layers: layer_states,
            models: model_states,
        })
    };
    let states = frames
        .iter()
        .zip(&slabbed)
        .map(|(subs, slabs)| {
            subs.iter()
                .zip(slabs)
                .map(|(f, s)| state(f, s))
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;

    if scene.sky.is_some() {
        // ponytail: no sky in Blender yet; a Sky Texture world would match it.
        eprintln!("mui-cut: Blender draws no `sky`; the background colour shows behind");
    }
    if scene.bloom.is_some() {
        // ponytail: Blender's own glare node would match it.
        eprintln!("mui-cut: Blender draws no `bloom`");
    }
    if scene
        .layers
        .iter()
        .any(|l| l.material.as_ref().is_some_and(|m| m.texture.is_some()))
    {
        // ponytail: no relief in Blender yet; a Bump node would match it.
        eprintln!("mui-cut: Blender draws no glass `texture`; its faces stay smooth");
    }
    let world = scene.environment.as_ref().map(|e| {
        let file = (!e.hdri.is_empty()).then(|| dir.join(&e.hdri));
        let read = file
            .as_ref()
            .map(|f| (std::path::absolute(f), std::fs::read(f)));
        let (image, hash) = match read {
            Some((Ok(abs), Ok(bytes))) => {
                let mut h = Fnv::default();
                let _ = h.write_all(&bytes);
                (abs.display().to_string(), h.hex())
            }
            other => {
                if other.is_some() {
                    eprintln!(
                        "mui-cut: environment `{}` not read; the studio lights",
                        e.hdri
                    );
                }
                (studio_exr().0.clone(), String::new())
            }
        };
        WorldD {
            image,
            hash,
            background: e.background,
        }
    });
    let px = f64::from(METRES);
    let h = f64::from(size[1]);
    let desc = Desc {
        options: o.clone(),
        background: r3(linear(scene.background)),
        ground: scene.ground.as_ref().map(|g| GroundD {
            z: r((h / 2. - g.y) * px),
            color: r3(linear(g.color)),
            radius: r(g.radius.max(1.) * px),
            roughness: r(1. - 0.8 * g.reflect.clamp(0., 1.)),
            specular: r(g.reflect.clamp(0., 1.)),
        }),
        fog: scene.fog.as_ref().map(|f| FogD {
            color: r3(linear(f.color)),
            start: r(f.near * px),
            depth: r((f.far - f.near).max(1.) * px),
        }),
        world,
        lights: lights.into_iter().map(|(_, l)| l).collect(),
        layers,
        models: models.into_iter().map(|(_, m)| m).collect(),
        frames: states,
    };
    Ok((desc, textures))
}

/// A frame's content layers as slabs (a plugin layer is several; see
/// [`Assets::slabs`]), each named uniquely within the frame.
fn slabs(assets: &Assets, f: &Frame) -> Vec<(String, Drawn)> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    f.layers
        .iter()
        // Overlays go over Blender's frame flat, as over the stage's.
        .filter(|l| {
            !l.space.overlay
                && !matches!(
                    l.kind,
                    Kind::Camera { .. }
                        | Kind::Light { .. }
                        | Kind::Model { .. }
                        | Kind::Audio { .. }
                )
        })
        .flat_map(|l| assets.slabs(l))
        .map(|d| {
            let n = seen.entry(d.id.clone()).or_default();
            let name = if *n == 0 {
                d.id.clone()
            } else {
                format!("{}~{n}", d.id)
            };
            *n += 1;
            (name, d)
        })
        .collect()
}

/// An outline (or, with none, the `w` by `h` box) as closed rings in the
/// slab's centred, y-up pixels.
fn ring_list(outline: Option<mui_geometry::Path>, w: f64, h: f64) -> Vec<Vec<[f32; 2]>> {
    let Some(o) = outline else {
        return vec![
            [
                [-w / 2., -h / 2.],
                [w / 2., -h / 2.],
                [w / 2., h / 2.],
                [-w / 2., h / 2.],
            ]
            .map(|[x, y]| [r(x), r(y)])
            .to_vec(),
        ];
    };
    o.flatten(0.25, 1 << 16)
        .unwrap_or_default()
        .iter()
        .map(|c| {
            c.iter()
                .map(|q| [r(q.x - w / 2.), r(h / 2. - q.y)])
                .collect::<Vec<_>>()
        })
        .filter(|ring| ring.len() >= 3)
        .collect()
}

impl Desc {
    /// The key of what every frame draws, and each frame's key: the
    /// scene's shared parts (textures by their pixels' hash, geometry,
    /// settings, this build and its script) and the frame's own states.
    pub fn keys(&self) -> (String, Vec<String>) {
        let mut base = Fnv::default();
        let _ = write!(base, "{}|", env!("CARGO_PKG_VERSION"));
        let _ = base.write_all(SCRIPT.as_bytes());
        let _ = serde_json::to_writer(
            &mut base,
            &Desc {
                frames: Vec::new(),
                ..self.clone()
            },
        );
        let frames: Vec<String> = self
            .frames
            .iter()
            .map(|f| {
                let mut h = Fnv(base.0);
                let _ = serde_json::to_writer(&mut h, f);
                h.hex()
            })
            .collect();
        let mut all = Fnv(base.0);
        for k in &frames {
            let _ = all.write_all(k.as_bytes());
        }
        (all.hex(), frames)
    }
}

/// Render every `(scene, t)` of `jobs` to a PNG under `cache`, in order.
/// Textures, the `.blend` and frames are kept by content hash, so only
/// frames whose key is new go to Blender. `dir` resolves model paths.
pub fn render(
    p: &Project,
    jobs: &[(&Scene, f64)],
    assets: &Assets,
    dir: &Path,
    cache: &Path,
    o: &Options,
) -> Result<Vec<PathBuf>, String> {
    if let Some((s, _)) = jobs.iter().find(|(s, _)| s.mode != Mode::ThreeD) {
        describe(p, s, &[], assets, dir, o)?;
    }
    let bin = binary()?;
    let io = |path: &Path, e: std::io::Error| format!("{}: {e}", path.display());
    // Blender runs in its own directory: every path it gets is absolute.
    let cache = &std::path::absolute(cache).map_err(|e| io(cache, e))?;
    let (tex, out) = (cache.join("tex"), cache.join("frames"));
    for d in [&tex, &out] {
        std::fs::create_dir_all(d).map_err(|e| io(d, e))?;
    }
    let script = cache.join("mui-cut-blender.py");
    if std::fs::read_to_string(&script).ok().as_deref() != Some(SCRIPT) {
        std::fs::write(&script, SCRIPT).map_err(|e| io(&script, e))?;
    }
    let mut pngs = Vec::with_capacity(jobs.len());
    for group in jobs.chunk_by(|a, b| std::ptr::eq(a.0, b.0)) {
        let times: Vec<f64> = group.iter().map(|j| j.1).collect();
        let (desc, textures) = describe(p, group[0].0, &times, assets, dir, o)?;
        let (studio, bytes) = studio_exr();
        let baked = tex.join(studio);
        if desc.world.as_ref().is_some_and(|w| &w.image == studio) && !baked.exists() {
            let part = baked.with_extension("part");
            std::fs::write(&part, bytes)
                .and_then(|()| std::fs::rename(&part, &baked))
                .map_err(|e| io(&baked, e))?;
        }
        for t in textures {
            let path = tex.join(&t.name);
            if !path.exists() {
                write_png(&path, &t.rgba, t.size)?;
            }
        }
        let (blend, keys) = desc.keys();
        let files: Vec<PathBuf> = keys.iter().map(|k| out.join(format!("{k}.png"))).collect();
        let mut todo: Vec<(usize, String)> = Vec::new();
        for (i, f) in files.iter().enumerate() {
            if !f.exists() && !todo.iter().any(|(_, t)| *t == f.display().to_string()) {
                todo.push((i, f.display().to_string()));
            }
        }
        if !todo.is_empty() {
            let json = cache.join(format!("{blend}.json"));
            let job = serde_json::json!({
                "desc": desc,
                "tex": tex.display().to_string(),
                "blend": cache.join(format!("{blend}.blend")).display().to_string(),
                "render": todo,
            });
            std::fs::write(&json, job.to_string()).map_err(|e| io(&json, e))?;
            run(&bin, &script, &json)?;
            let _ = std::fs::remove_file(&json);
            if let Some(f) = files.iter().find(|f| !f.exists()) {
                return Err(format!("blender wrote no {}", f.display()));
            }
        }
        pngs.extend(files);
    }
    Ok(pngs)
}

/// `blender -b` on `script` with the job file; its log is kept for errors,
/// the script's progress lines go to stderr.
fn run(bin: &Path, script: &Path, job: &Path) -> Result<(), String> {
    let o = Command::new(bin)
        .args([
            "-b",
            "--factory-startup",
            "-noaudio",
            "--python-exit-code",
            "1",
            "--python",
        ])
        .arg(script)
        .arg("--")
        .arg(job)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("{}: {e}", bin.display()))?;
    if o.status.success() {
        return Ok(());
    }
    let log = String::from_utf8_lossy(&o.stdout);
    let tail: Vec<&str> = log.lines().rev().take(30).collect();
    Err(format!(
        "blender failed ({}):\n{}",
        o.status,
        tail.into_iter().rev().collect::<Vec<_>>().join("\n")
    ))
}

/// Straight RGBA as a PNG, through a sibling temp file.
fn write_png(path: &Path, rgba: &[u8], [w, h]: [u16; 2]) -> Result<(), String> {
    let tmp = path.with_extension("part");
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let file = std::fs::File::create(&tmp).map_err(|e| err(&e))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w.into(), h.into());
    enc.set_color(png::ColorType::Rgba);
    enc.write_header()
        .and_then(|mut wr| wr.write_image_data(rgba))
        .map_err(|e| err(&e))?;
    std::fs::rename(&tmp, path).map_err(|e| err(&e))
}

/// A PNG as straight RGBA and its size.
pub fn read_png(path: &Path) -> Result<(Vec<u8>, [u32; 2]), String> {
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let file = std::fs::File::open(path).map_err(|e| err(&e))?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(file));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().map_err(|e| err(&e))?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("png too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| err(&e))?;
    let px = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        c => return Err(format!("{}: {c:?} png", path.display())),
    };
    Ok((rgba, [info.width, info.height]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAGE3D: &str = include_str!("../examples/stage3d.cut.json");
    const KNOT: &[u8] = include_bytes!("../examples/knot.glb");
    const GOLDEN: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/stage3d-blender-t0.json"
    );

    fn stage(times: &[f64], mb: usize) -> Desc {
        let p = Project::load(STAGE3D).unwrap();
        let mut assets = Assets::default();
        assets.add_asset("knot.glb", KNOT).unwrap();
        let o = Options::new(None, None, mb, [1280, 720]).unwrap();
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        describe(&p, &p.scenes[0], times, &assets, &dir, &o)
            .unwrap()
            .0
    }

    fn col(m: &[f32; 16], c: usize) -> [f64; 3] {
        std::array::from_fn(|k| f64::from(m[c * 4 + k]))
    }
    fn near(a: [f64; 3], b: [f64; 3], eps: f64) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < eps)
    }

    #[test]
    fn stage3d_maps_onto_blender_objects() {
        let d = stage(&[0., 3.], 1);
        let p = Project::load(STAGE3D).unwrap();
        let f = eval(&p, &p.scenes[0], 0.);
        let view = f.view.as_ref().unwrap();

        // Camera: at the eye, looking down -z at the target, a vertical
        // 38° field on a 24 mm sensor, focused on its target.
        let cam = &d.frames[0][0].camera;
        let eye = at(p.size, view.camera.eye);
        let target = at(p.size, view.camera.target);
        assert!(near(col(&cam.m, 3), eye, 1e-3));
        let back = norm(sub(eye, target));
        assert!(near(col(&cam.m, 2), back, 1e-3), "{:?}", col(&cam.m, 2));
        assert!((cam.lens - 34.8501).abs() < 1e-3, "{}", cam.lens);
        let reach = sub(eye, target).iter().map(|c| c * c).sum::<f64>().sqrt();
        assert!((f64::from(cam.focus) - reach).abs() < 1e-3);
        assert!(cam.fstop > 0.1 && cam.fstop < 2., "{}", cam.fstop);

        // The ambient light is the world; sun and spot are objects.
        let kinds: Vec<_> = d.lights.iter().map(|l| (l.id.as_str(), l.kind)).collect();
        assert_eq!(kinds, [("sun", "SUN"), ("spot", "SPOT")]);
        let sky = linear(crate::Rgba([0xa4, 0xac, 0xff, 255])).map(|c| c * 0.32);
        assert!(near(
            d.frames[0][0].ambient.map(f64::from),
            sky.map(f64::from),
            1e-4
        ));
        let sun = &d.frames[0][0].lights[0];
        assert!((f64::from(sun.energy) - 1.15 * std::f64::consts::PI).abs() < 1e-3);
        let dir = three::aim(52., -70.);
        let travel = axes([dir[0] as f32, -dir[1] as f32, -dir[2] as f32]);
        assert!(near(col(&sun.m, 2), travel.map(|c| -c), 1e-3));
        let spot = &d.frames[0][0].lights[1];
        assert!((f64::from(spot.spot) - 44f64.to_radians()).abs() < 1e-3);
        assert!((spot.range - 14.).abs() < 1e-3);

        // Cards are bevelled slabs, labels flat cards, the title a slab of
        // its glyphs; each shows one look throughout.
        let layer = |id: &str| d.layers.iter().find(|l| l.id == id).unwrap();
        assert_eq!(d.layers.len(), 13);
        assert!(d.layers.iter().all(|l| l.variants.len() == 1));
        let card = &layer("card0").variants[0];
        assert_eq!((card.size, card.depth, card.bevel), ([300., 180.], 16., 2.));
        assert!(card.rings.len() == 1 && card.rings[0].len() > 8);
        assert!(layer("label0").variants[0].rings.is_empty());
        assert!(!layer("label0").shadow);
        let title = &layer("title").variants[0];
        assert!(
            title.depth == 14. && title.rings.len() >= 8,
            "{}",
            title.rings.len()
        );
        // A flat card at the frame centre's height, 1 cm per pixel.
        let c0 = &d.frames[0][0].layers[0];
        assert!(near(col(&c0.m, 3), at(p.size, [300., 300., 0.]), 1e-3));
        assert!((c0.m[0] - METRES).abs() < 1e-6 && c0.v == 0 && c0.a == 1.);
        // The title fades in: hidden at 0, shown by 3.
        let ti = d.layers.iter().position(|l| l.id == "title").unwrap();
        assert_eq!(d.frames[0][0].layers[ti].v, -1);
        assert_eq!(d.frames[1][0].layers[ti].v, 0);

        assert!(d.models[0].path.ends_with("knot.glb"));
        // No HDRI given: the world is the studio mui-stage lights with,
        // turned and scaled per frame as the scene keys them.
        let w = d.world.as_ref().unwrap();
        assert!(w.image == studio_exr().0 && !w.background);
        assert_eq!(d.frames[0][0].env, Some([0.45, 0.3491]));
        let g = d.ground.as_ref().unwrap();
        assert!((g.z + 2.6).abs() < 1e-4 && (g.radius - 26.).abs() < 1e-4);
        // `reflect: 0` is a matte floor: no grazing sheen mui-stage lacks.
        assert_eq!(g.specular, 0.);
        // The spot is lit to its intensity at the nearest thing it faces,
        // not blown out on a card right under it.
        let near_card = d.frames[0][0]
            .layers
            .iter()
            .filter(|l| l.v >= 0)
            .map(|l| {
                let p = col(&l.m, 3);
                let q = col(&spot.m, 3);
                (0..3).map(|a| (p[a] - q[a]).powi(2)).sum::<f64>()
            })
            .fold(f64::MAX, f64::min);
        let want = 0.9 * 4. * std::f64::consts::PI.powi(2) * near_card;
        assert!(
            (f64::from(spot.energy) - want).abs() / want < 1e-3,
            "{}",
            spot.energy
        );
        let fog = d.fog.as_ref().unwrap();
        assert!((fog.start - 18.).abs() < 1e-4 && (fog.depth - 24.).abs() < 1e-4);
    }

    /// The whole first instant, against the checked-in golden file
    /// (`UPDATE_GOLDEN=1` rewrites it).
    #[test]
    fn stage3d_at_0_matches_its_golden_state() {
        let got = serde_json::to_string_pretty(&stage(&[0.], 1).frames[0][0]).unwrap() + "\n";
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::create_dir_all(Path::new(GOLDEN).parent().unwrap()).unwrap();
            std::fs::write(GOLDEN, &got).unwrap();
        }
        let want = std::fs::read_to_string(GOLDEN).expect("run with UPDATE_GOLDEN=1 once");
        assert!(got == want, "the Blender state drifted from {GOLDEN}");
    }

    /// A layer's surface goes to Blender whole, the stage's slab where it
    /// says nothing; a model's only what its layer overrides.
    #[test]
    fn materials_reach_the_principled_bsdf() {
        let golden = std::fs::read_to_string(GOLDEN).unwrap();
        assert!(golden.contains("\"mat\"") && golden.contains("\"roughness\": 0.42"));
        let p = Project::load(
            r##"{"size":[640,360],"fps":30,"scenes":[{"name":"a","duration":1,"mode":"3d","layers":[
            {"id":"pane","kind":"rect","width":200,"height":100,"extrude":10,"fill":"#ffffff",
             "material":{"transmission":1,"ior":1.45,"tint":"#ff0000",
                         "roughness":[{"t":0,"v":0},{"t":1,"v":0.5}]}},
            {"id":"flat","kind":"rect","width":200,"height":100,"fill":"#ffffff",
             "material":{"transmission":1,"thickness":0}},
            {"id":"knot","kind":"model","path":"knot.glb","height":100,
             "material":{"thickness":50,"metallic":0}}]}]}"##,
        )
        .unwrap();
        let mut assets = Assets::default();
        assets.add_asset("knot.glb", KNOT).unwrap();
        let o = Options::new(None, None, 1, [640, 360]).unwrap();
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        let d = describe(&p, &p.scenes[0], &[0., 1.], &assets, &dir, &o)
            .unwrap()
            .0;
        let mat = |f: usize, id: &str| {
            let i = d.layers.iter().position(|l| l.id == id).unwrap();
            d.frames[f][0].layers[i].mat.clone()
        };
        let pane = mat(0, "pane");
        assert_eq!(
            (pane.transmission, pane.ior, pane.metallic, pane.thickness),
            (Some(1.), Some(1.45), Some(0.), Some(10.)),
            "its depth, in object units"
        );
        assert_eq!(pane.tint, Some([1., 0., 0.]));
        assert_eq!(
            (pane.roughness, mat(1, "pane").roughness),
            (Some(0.), Some(0.5))
        );
        assert_eq!(mat(0, "flat").thickness, Some(0.), "a thin wall");
        let knot = d.frames[0][0].models[0].mat.clone().unwrap();
        assert_eq!((knot.metallic, knot.roughness), (Some(0.), None));
        let tall = {
            let m = assets.model("knot.glb").unwrap();
            f64::from(m.max[1] - m.min[1])
        };
        let want = 50. * tall / 100.;
        assert!((f64::from(knot.thickness.unwrap()) - want).abs() < 1e-3);
        // The script keys what it gets and turns EEVEE's refraction on.
        for needle in [
            "use_raytrace_refraction",
            "Transmission Weight",
            "Thin Wall",
        ] {
            assert!(SCRIPT.contains(needle), "{needle}");
        }
    }

    /// Every key the script reads from the job is one mui-cut writes.
    #[test]
    fn the_script_reads_only_keys_the_description_has() {
        let d = stage(&[0.], 2);
        let job = serde_json::json!({"desc": d, "tex": "", "blend": "", "render": []});
        let text = job.to_string();
        let mut missing = Vec::new();
        for owner in [
            "D", "O", "job", "s", "c", "l", "L", "v", "g", "f", "M", "ls", "ms", "W",
        ] {
            let pat = format!("{owner}[\"");
            for (i, _) in SCRIPT.match_indices(&pat) {
                let before = SCRIPT[..i].chars().last();
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let rest = &SCRIPT[i + pat.len()..];
                let key = &rest[..rest.find('"').unwrap()];
                if !text.contains(&format!("\"{key}\":")) {
                    missing.push(format!("{owner}[{key}]"));
                }
            }
        }
        assert!(missing.is_empty(), "{missing:?}");
        // Motion blur: each frame carries its shutter, open to closed.
        assert_eq!(d.frames[0].len(), 3);
    }

    #[test]
    fn cache_keys_are_deterministic_and_follow_content() {
        let (a, b) = (stage(&[0., 1., 3.], 1), stage(&[0., 1., 3.], 1));
        assert_eq!(a.keys(), b.keys());
        let (base, frames) = a.keys();
        assert_eq!(frames.len(), 3);
        assert!(frames[0] != frames[1] && frames[1] != frames[2]);
        // Frame keys depend on the instant, not on what else is rendered.
        assert_eq!(stage(&[1.], 1).keys().1[0], frames[1]);
        // A new look changes every frame's key and the scene's.
        let mut c = a.clone();
        c.layers[0].variants[0].tex = "other.png".into();
        let (cb, cf) = c.keys();
        assert!(cb != base && cf.iter().zip(&frames).all(|(x, y)| x != y));
        // So do the render settings.
        let mut s = a.clone();
        s.options.samples += 1;
        assert!(s.keys().1[0] != frames[0]);
        // A still scene draws the same frame at every instant.
        let p = Project::load(
            r#"{"size":[64,36],"fps":10,"scenes":[{"name":"a","duration":1,"mode":"3d",
                "layers":[{"id":"r","kind":"rect","width":20,"height":10,"extrude":4}]}]}"#,
        )
        .unwrap();
        let o = Options::new(None, Some(4), 1, [64, 36]).unwrap();
        let (d, tex) = describe(
            &p,
            &p.scenes[0],
            &[0., 0.5],
            &Assets::default(),
            Path::new(""),
            &o,
        )
        .unwrap();
        let k = d.keys().1;
        assert_eq!(k[0], k[1]);
        assert_eq!(tex.len(), 1);
        assert_eq!(tex[0].size, [40, 20]);
    }

    /// A plugin layer is its slabs, as mui-stage draws it: the backdrop,
    /// each part where the 2D drawing puts it, and a highlight plate.
    #[test]
    fn plugin_parts_become_their_own_slabs() {
        let p = Project::load(
            r#"{"size":[400,200],"fps":30,"scenes":[{"name":"a","duration":2,"mode":"3d",
                "layers":[{"id":"syn","kind":"plugin","source":{"bin":"x"},"x":200,"y":100,
                "explode":0.5,"parts":{"a":{"highlight":1}}}]}]}"#,
        )
        .unwrap();
        let assets = crate::tests::capture_assets(&p);
        let o = Options::new(None, Some(4), 1, [400, 200]).unwrap();
        let (d, tex) = describe(&p, &p.scenes[0], &[0.], &assets, Path::new(""), &o).unwrap();
        let ids: Vec<&str> = d.layers.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["syn", "syn#a", "syn#a~1", "syn#b"]);
        assert!(tex.len() >= 2, "the backdrop and part images");
        let slabs = assets.slabs(&eval(&p, &p.scenes[0], 0.).layers[0]);
        for (s, st) in slabs.iter().zip(&d.frames[0][0].layers) {
            assert_eq!(st.v, 0);
            let want = at(p.size, [s.x, s.y, s.space.z]);
            assert!(near(col(&st.m, 3), want, 1e-3), "{}", s.id);
        }
    }

    #[test]
    fn a_2d_scene_or_an_unknown_engine_is_a_clear_error() {
        let p = Project::load(include_str!("../examples/demo.cut.json")).unwrap();
        let o = Options::new(None, None, 1, [64, 36]).unwrap();
        let e = describe(
            &p,
            &p.scenes[0],
            &[0.],
            &Assets::default(),
            Path::new(""),
            &o,
        )
        .err()
        .unwrap();
        assert!(
            e.contains("is 2D") && e.contains("classic, gpu or cpu"),
            "{e}"
        );
        let e = Options::new(Some("workbench"), None, 1, [64, 36]).unwrap_err();
        assert!(e.contains("eevee or cycles"), "{e}");
    }
}
