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
use crate::{Assets, Drawn, Fnv, Frame, Kind, LightType, Mode, Project, SHUTTER, Scene, eval, three};

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
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ModelS {
    pub m: [f32; 16],
    pub show: bool,
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
fn placed(m: Mat4) -> [f32; 16] {
    (TO_BLENDER * Mat4::scale(METRES) * m).0.map(|v| r(f64::from(v)))
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
    let all = || frames.iter().flatten();

    // Content layers: each distinct look, the most it is scaled, and which
    // one every instant shows.
    let mut lights = Vec::new();
    let mut layers = Vec::new();
    let mut models = Vec::new();
    // Per layer index: its looks' numbers by key, and each look's content
    // and the most it is scaled.
    let mut looks: Vec<HashMap<String, usize>> = vec![HashMap::new(); scene.layers.len()];
    let mut firsts: Vec<Vec<(Drawn, f64)>> = vec![Vec::new(); scene.layers.len()];
    for (i, l) in scene.layers.iter().enumerate() {
        match &l.kind {
            Kind::Camera { .. } => {}
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
                let abs = std::path::absolute(dir.join(path))
                    .map_err(|e| format!("{path}: {e}"))?;
                models.push((
                    i,
                    ModelD {
                        id: l.id.clone(),
                        path: abs.display().to_string(),
                        shadow: l.cast_shadows,
                    },
                ));
            }
            _ => {
                for f in all() {
                    let d = &f.layers[i];
                    let c = content(d);
                    let key = serde_json::to_string(&c).map_err(|e| e.to_string())?;
                    let n = *looks[i].entry(key).or_insert_with(|| {
                        firsts[i].push((c, 0.));
                        firsts[i].len() - 1
                    });
                    firsts[i][n].1 = firsts[i][n].1.max(d.scale.abs());
                }
                layers.push((
                    i,
                    LayerD {
                        id: l.id.clone(),
                        shadow: l.cast_shadows,
                        variants: Vec::new(),
                    },
                ));
            }
        }
    }

    // Paint each look once; keep its box to place it by.
    let mut textures: Vec<Texture> = Vec::new();
    let mut boxes: HashMap<(usize, usize), (f64, f64, [f64; 2])> = HashMap::new();
    for (i, layer) in &mut layers {
        for (n, (c, most)) in firsts[*i].iter().enumerate() {
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
            boxes.insert((*i, n), (w, hgt, [corner.x, corner.y]));
            if !textures.iter().any(|t| t.name == name) {
                textures.push(Texture {
                    name,
                    rgba,
                    size: [tw, th],
                });
            }
        }
    }

    let state = |f: &Frame| -> Result<State, String> {
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
                // camera's target, not mui-stage's range fade; a true
                // falloff model if lights ever sit far from the subject.
                let reach = sub(pos, target).iter().map(|c| c * c).sum::<f64>().sqrt();
                let energy = if ld.kind == "SUN" {
                    k * std::f64::consts::PI
                } else {
                    k * 4. * std::f64::consts::PI.powi(2) * reach.max(0.1).powi(2)
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
        let layer_states = layers
            .iter()
            .map(|(i, _)| {
                let d = &f.layers[*i];
                let key = serde_json::to_string(&content(d)).map_err(|e| e.to_string())?;
                let n = looks[*i][&key];
                let (w, h, corner) = boxes[&(*i, n)];
                let offset = [
                    (corner[0] + w / 2.) as f32,
                    -(corner[1] + h / 2.) as f32,
                    d.space.anchor_z as f32,
                ];
                let m = three::pose(size, d, Mat4::scale(d.scale as f32) * Mat4::translate(offset));
                let shown = d.opacity > 0. && d.scale != 0.;
                Ok(LayerS {
                    m: placed(m),
                    v: if shown { n as i32 } else { -1 },
                    a: r(d.opacity),
                })
            })
            .collect::<Result<_, String>>()?;
        let model_states = models
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
                }
            })
            .collect();
        Ok(State {
            camera,
            ambient: r3(ambient),
            lights,
            layers: layer_states,
            models: model_states,
        })
    };
    let states = frames
        .iter()
        .map(|subs| subs.iter().map(state).collect::<Result<Vec<_>, _>>())
        .collect::<Result<Vec<_>, _>>()?;

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
        }),
        fog: scene.fog.as_ref().map(|f| FogD {
            color: r3(linear(f.color)),
            start: r(f.near * px),
            depth: r((f.far - f.near).max(1.) * px),
        }),
        lights: lights.into_iter().map(|(_, l)| l).collect(),
        layers: layers.into_iter().map(|(_, l)| l).collect(),
        models: models.into_iter().map(|(_, m)| m).collect(),
        frames: states,
    };
    Ok((desc, textures))
}

/// An outline (or, with none, the `w` by `h` box) as closed rings in the
/// slab's centred, y-up pixels.
fn ring_list(outline: Option<mui_geometry::Path>, w: f64, h: f64) -> Vec<Vec<[f32; 2]>> {
    let Some(o) = outline else {
        return vec![
            [[-w / 2., -h / 2.], [w / 2., -h / 2.], [w / 2., h / 2.], [-w / 2., h / 2.]]
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
        .args(["-b", "--factory-startup", "-noaudio", "--python-exit-code", "1", "--python"])
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
