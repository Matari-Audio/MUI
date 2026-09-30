//! 3D scenes, evaluated: where the camera is and what it looks at, the
//! lights, the ground and the fog, all in project coordinates (x right, y
//! down, z deeper; the camera of an unturned scene sits at negative z).
//! Pure maths, the same on the web; `gpu3d.rs` hands it to mui-stage.
use serde::{Deserialize, Serialize};

use crate::{Anim, Drawn, Kind, LightType, Rgba, Scene, vector};

/// Flat composite, or layers in a lit 3D space.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
pub enum Mode {
    #[default]
    #[serde(rename = "2d")]
    TwoD,
    #[serde(rename = "3d")]
    ThreeD,
}

/// A floor at `y` project pixels, fading out `radius` pixels from the
/// frame's centre, mirroring the layers by `reflect` (0..1) and darkening
/// under what stands on it by `contact` (0..1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(transform = crate::vars::bindable)]
pub struct Ground {
    pub y: f64,
    #[serde(default = "ground_color")]
    pub color: Rgba,
    #[serde(default = "ground_radius")]
    pub radius: f64,
    #[serde(default)]
    pub reflect: f64,
    #[serde(default = "contact")]
    pub contact: f64,
}
fn ground_color() -> Rgba {
    Rgba([28, 29, 36, 255])
}
fn ground_radius() -> f64 {
    2400.
}
fn contact() -> f64 {
    0.6
}

/// Surfaces fade into `color` from `near` to `far` pixels from the camera.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(transform = crate::vars::bindable)]
pub struct Fog {
    pub color: Rgba,
    pub near: f64,
    pub far: f64,
}

/// Image-based light: an equirectangular `.hdr` or `.exr` (relative to the
/// project; left out, a built-in neutral studio) lighting every surface
/// and mirrored by metals, `intensity` times, turned `rotation` degrees
/// about the vertical. With `background` the camera sees it behind
/// everything instead of the scene's background colour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(transform = crate::vars::bindable)]
pub struct Environment {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub hdri: String,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub intensity: Anim<f64>,
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub rotation: Anim<f64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub background: bool,
}
fn one() -> Anim<f64> {
    Anim::Value(1.)
}
fn is_one(a: &Anim<f64>) -> bool {
    *a == one()
}
fn zero() -> Anim<f64> {
    Anim::Value(0.)
}
fn is_zero(a: &Anim<f64>) -> bool {
    *a == zero()
}

/// The environment at one time.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Env {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub hdri: String,
    pub intensity: f64,
    pub rotation: f64,
    pub background: bool,
}

/// Ambient occlusion: creases and contacts within `radius` pixels darken,
/// by `strength` (1 the full occlusion).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(transform = crate::vars::bindable)]
pub struct Ao {
    #[serde(default = "ao_strength")]
    pub strength: f64,
    #[serde(default = "ao_radius")]
    pub radius: f64,
}
fn ao_strength() -> f64 {
    1.
}
fn ao_radius() -> f64 {
    60.
}

/// 3D: how a layer's surface meets light, after Blender's Principled BSDF
/// and glTF's PBR materials. Every field is keyable; one left out is a
/// slab's default (a rough dielectric), or on a model its glTF material's.
/// `transmission` above zero makes glass, which reflects and refracts what
/// is behind it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::vars::bindable)]
pub struct Material {
    /// 0 a dielectric, 1 a metal (tinted by the layer's colour).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metallic: Option<Anim<f64>>,
    /// 0 a mirror, 1 matte. Slabs default to 0.42.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness: Option<Anim<f64>>,
    /// 0 opaque, 1 glass: all light not reflected passes through, tinted
    /// by the layer's colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transmission: Option<Anim<f64>>,
    /// Index of refraction (glass 1.5, water 1.33, diamond 2.42).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ior: Option<Anim<f64>>,
    /// Pixels light crosses inside; 0 on a slab is its `extrude`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness: Option<Anim<f64>>,
    /// How much the index changes with colour: 20 / the Abbe number
    /// (glTF's `KHR_materials_dispersion`). 0 none, flint glass 0.6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispersion: Option<Anim<f64>>,
    /// The colour white light keeps after `thickness` inside (absorption).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<Anim<Rgba>>,
}

/// A [`Material`] at one time: the fields it sets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Surface {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metallic: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roughness: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transmission: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ior: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thickness: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispersion: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tint: Option<Rgba>,
}

impl Material {
    /// Its keyable fields by path (`material.roughness`), those it sets.
    pub fn props(&self) -> Vec<(String, crate::Prop<'_>)> {
        use crate::Prop::{Color, Num};
        let mut out: Vec<(String, crate::Prop<'_>)> = [
            ("metallic", &self.metallic),
            ("roughness", &self.roughness),
            ("transmission", &self.transmission),
            ("ior", &self.ior),
            ("thickness", &self.thickness),
            ("dispersion", &self.dispersion),
        ]
        .into_iter()
        .filter_map(|(n, a)| a.as_ref().map(|a| (format!("material.{n}"), Num(a))))
        .collect();
        if let Some(a) = &self.tint {
            out.push(("material.tint".into(), Color(a)));
        }
        out
    }

    pub fn at(&self, t: f64) -> Surface {
        let num = |a: &Option<Anim<f64>>| a.as_ref().map(|a| a.at(t));
        Surface {
            metallic: num(&self.metallic).map(|v| v.clamp(0., 1.)),
            roughness: num(&self.roughness).map(|v| v.clamp(0., 1.)),
            transmission: num(&self.transmission).map(|v| v.clamp(0., 1.)),
            ior: num(&self.ior).map(|v| v.clamp(1., 4.)),
            thickness: num(&self.thickness).map(|v| v.max(0.)),
            dispersion: num(&self.dispersion).map(|v| v.max(0.)),
            tint: self.tint.as_ref().map(|a| a.at(t)),
        }
    }
}

impl Surface {
    /// `base` with the fields this sets replaced.
    pub fn over(&self, base: mui_stage::Material) -> mui_stage::Material {
        let f = |v: Option<f64>, b: f32| v.map_or(b, |v| v as f32);
        mui_stage::Material {
            metallic: f(self.metallic, base.metallic),
            roughness: f(self.roughness, base.roughness),
            transmission: f(self.transmission, base.transmission),
            ior: f(self.ior, base.ior),
            thickness: f(self.thickness, base.thickness),
            dispersion: f(self.dispersion, base.dispersion),
            tint: self.tint.map_or(base.tint, crate::gpu3d::linear),
        }
    }
    /// Drawn as glass.
    pub fn glass(&self) -> bool {
        self.transmission.is_some_and(|t| t > 0.)
    }
}

/// A layer's 3D properties at one time (see [`crate::Layer`]).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Space {
    pub z: f64,
    pub rx: f64,
    pub ry: f64,
    pub anchor_z: f64,
    pub extrude: f64,
    pub edge: Rgba,
    pub cast_shadows: bool,
    pub receive_shadows: bool,
    pub distance: f64,
    pub fov: f64,
    pub dolly: f64,
    pub focus: f64,
    pub aperture: f64,
    pub intensity: f64,
    pub cone: f64,
    pub feather: f64,
    pub range: f64,
    pub softness: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material: Option<Surface>,
}
impl Default for Space {
    fn default() -> Self {
        Self {
            z: 0.,
            rx: 0.,
            ry: 0.,
            anchor_z: 0.,
            extrude: 0.,
            edge: Rgba([40, 41, 50, 255]),
            cast_shadows: true,
            receive_shadows: true,
            distance: 0.,
            fov: 40.,
            dolly: 0.,
            focus: 0.,
            aperture: 0.,
            intensity: 1.,
            cone: 30.,
            feather: 0.3,
            range: 0.,
            softness: 1.5,
            material: None,
        }
    }
}
impl Space {
    pub fn is_flat(&self) -> bool {
        *self == Self::default()
    }
}

/// The camera at one time, project coordinates.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Cam {
    pub eye: [f64; 3],
    pub target: [f64; 3],
    /// Vertical, degrees.
    pub fov: f64,
    /// Degrees, clockwise.
    pub roll: f64,
    /// Distance from the eye that is sharp; 0 with no depth of field.
    pub focus: f64,
    /// Blur in project pixels of what is far out of focus.
    pub aperture: f64,
}

impl Cam {
    /// The editor's orbit preview: this camera swung `yaw` degrees right and
    /// `pitch` degrees down about its target, its distance times `zoom`.
    pub fn orbit(&self, yaw: f64, pitch: f64, zoom: f64) -> Cam {
        let d: [f64; 3] = std::array::from_fn(|i| self.target[i] - self.eye[i]);
        let r = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-9);
        let p0 = (d[1] / r).clamp(-1.0, 1.0).asin().to_degrees();
        let y0 = d[0].atan2(d[2]).to_degrees();
        let f = aim((p0 + pitch).clamp(-89.0, 89.0), y0 + yaw);
        let r = r * zoom.max(0.01);
        Cam {
            eye: std::array::from_fn(|i| self.target[i] - f[i] * r),
            ..self.clone()
        }
    }
}

/// A light at one time, project coordinates.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Lamp {
    pub kind: LightType,
    pub color: Rgba,
    /// Intensity times opacity.
    pub intensity: f64,
    pub position: [f64; 3],
    /// The way the light travels, unit length.
    pub direction: [f64; 3],
    pub range: f64,
    pub cone: f64,
    pub feather: f64,
    pub shadows: bool,
    pub softness: f64,
}

/// What a 3D scene adds to a frame.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct View {
    pub camera: Cam,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lights: Vec<Lamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ground: Option<Ground>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fog: Option<Fog>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<Env>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ao: Option<Ao>,
}

/// A project point (x right, y down, z deeper) in mui-stage's world
/// (centred, y up, z toward the viewer).
pub fn world(size: [u32; 2], p: [f64; 3]) -> [f32; 3] {
    let [w, h] = size.map(f64::from);
    [(p[0] - w / 2.) as f32, (h / 2. - p[1]) as f32, -p[2] as f32]
}

/// The camera as mui-stage takes it.
pub fn stage_camera(size: [u32; 2], c: &Cam) -> mui_stage::Camera {
    mui_stage::Camera {
        eye: world(size, c.eye),
        target: world(size, c.target),
        fov: c.fov as f32,
        roll: c.roll as f32,
    }
}

/// Layer `l`'s placement in mui-stage's world: `offset` (in its own
/// unrotated space) turned about its pivot and moved to it.
pub fn pose(size: [u32; 2], l: &Drawn, offset: mui_stage::Mat4) -> mui_stage::Mat4 {
    use mui_stage::Mat4;
    let s = &l.space;
    Mat4::translate(world(size, [l.x, l.y, s.z]))
        * Mat4::rotate_y(s.ry.to_radians() as f32)
        * Mat4::rotate_x(-s.rx.to_radians() as f32)
        * Mat4::rotate_z(-l.rotation.to_radians() as f32)
        * offset
}

/// The distance at which a `fov`-degree camera sees `height` pixels of the
/// z = 0 plane edge to edge: there a flat layer is its 2D size.
pub fn front_distance(height: f64, fov: f64) -> f64 {
    height * 0.5 / (fov.to_radians() * 0.5).tan()
}

/// A unit vector `pitch` degrees down and `yaw` degrees right of straight
/// into the scene (+z).
pub fn aim(pitch: f64, yaw: f64) -> [f64; 3] {
    let (p, y) = (pitch.to_radians(), yaw.to_radians());
    [y.sin() * p.cos(), p.sin(), y.cos() * p.cos()]
}

/// The camera, lights, ground, fog, environment and occlusion of `scene`
/// at `t`, whose layers evaluated to `layers`.
pub fn view(size: [u32; 2], scene: &Scene, t: f64, layers: &[Drawn]) -> View {
    View {
        camera: camera(size, layers),
        lights: layers
            .iter()
            .filter(|l| l.opacity > 0.)
            .filter_map(|l| match l.kind {
                Kind::Light { light } => Some(Lamp {
                    kind: light,
                    color: l.fill,
                    intensity: l.space.intensity * l.opacity,
                    position: [l.x, l.y, l.space.z],
                    direction: aim(l.space.rx, l.space.ry),
                    range: l.space.range,
                    cone: l.space.cone,
                    feather: l.space.feather,
                    shadows: l.space.cast_shadows,
                    softness: l.space.softness,
                }),
                _ => None,
            })
            .collect(),
        ground: scene.ground.clone(),
        fog: scene.fog.clone(),
        environment: scene.environment.as_ref().map(|e| Env {
            hdri: e.hdri.clone(),
            intensity: e.intensity.at(t).max(0.),
            rotation: e.rotation.at(t),
            background: e.background,
        }),
        ao: scene.ao.clone(),
    }
}

/// The last camera layer with some opacity, or straight on at the frame
/// centre from where the z = 0 plane is the 2D frame.
pub fn camera(size: [u32; 2], layers: &[Drawn]) -> Cam {
    let [w, h] = size.map(f64::from);
    let Some((c, look_at, path)) = layers.iter().rev().find_map(|l| match &l.kind {
        Kind::Camera { look_at, path } if l.opacity > 0. => Some((l, look_at, path)),
        _ => None,
    }) else {
        let fov = Space::default().fov;
        return Cam {
            eye: [w / 2., h / 2., -front_distance(h, fov)],
            target: [w / 2., h / 2., 0.],
            fov,
            roll: 0.,
            focus: 0.,
            aperture: 0.,
        };
    };
    let s = &c.space;
    let looked = (!look_at.is_empty())
        .then(|| layers.iter().find(|l| &l.id == look_at))
        .flatten();
    let mut target = looked.map_or([c.x, c.y, s.z], |l| [l.x, l.y, l.space.z]);
    let dist = if s.distance > 0. {
        s.distance
    } else {
        front_distance(h, s.fov)
    };
    // The camera turns right by `ry` and tips down by `rx`, circling its
    // target: a positive pitch lifts it to look down.
    let forward = aim(s.rx, s.ry);
    let mut eye: [f64; 3] = std::array::from_fn(|i| target[i] - forward[i] * dist);
    for i in 0..3 {
        eye[i] += (target[i] - eye[i]) * s.dolly;
    }
    if let Some((dx, dz)) = vector::along(path, c.path_offset) {
        eye[0] += dx;
        eye[2] += dz;
        if looked.is_none() {
            target[0] += dx;
            target[2] += dz;
        }
    }
    let reach = (0..3)
        .map(|i| (eye[i] - target[i]).powi(2))
        .sum::<f64>()
        .sqrt();
    Cam {
        eye,
        target,
        fov: s.fov,
        roll: c.rotation,
        focus: if s.aperture > 0. {
            (reach + s.focus).max(1.)
        } else {
            0.
        },
        aperture: s.aperture,
    }
}

/// One glTF primitive in model space (y up): a position and a normal per
/// vertex, triangles, and its material: base colour (linear RGBA), metallic,
/// roughness, and glass from `KHR_materials_transmission`, `_ior`, `_volume`
/// (thickness in model units) and `_dispersion` where it has them.
#[derive(Clone, Debug)]
pub struct Part {
    pub vertices: Vec<[f32; 6]>,
    pub indices: Vec<u32>,
    pub color: [f32; 4],
    pub material: mui_stage::Material,
}

/// A glTF material as mui-stage draws it.
fn gltf_material(m: &gltf::Material<'_>) -> mui_stage::Material {
    let pbr = m.pbr_metallic_roughness();
    let volume = m.volume();
    let thickness = volume
        .as_ref()
        .map_or(0., gltf::material::Volume::thickness_factor);
    // Attenuation colour is what is left after `attenuationDistance`; the
    // tint is what is left after the thickness.
    let tint = volume.as_ref().map_or([1.; 3], |v| {
        let d = v.attenuation_distance();
        let k = if d.is_finite() && d > 0. && thickness > 0. {
            thickness / d
        } else {
            0.
        };
        v.attenuation_color().map(|c| c.clamp(0., 1.).powf(k))
    });
    mui_stage::Material {
        metallic: pbr.metallic_factor(),
        roughness: pbr.roughness_factor(),
        transmission: m.transmission().map_or(0., |t| t.transmission_factor()),
        ior: m.ior().unwrap_or(1.5),
        thickness,
        dispersion: m
            .extension_value("KHR_materials_dispersion")
            .and_then(|v| v["dispersion"].as_f64())
            .map_or(0., |v| v as f32),
        tint,
    }
}

/// A model file's triangles, node transforms applied, and their bounds.
#[derive(Clone, Debug)]
pub struct Mesh {
    pub parts: Vec<Part>,
    pub min: [f32; 3],
    pub max: [f32; 3],
}

type M4 = [[f32; 4]; 4];

fn mul(a: &M4, b: &M4) -> M4 {
    std::array::from_fn(|c| std::array::from_fn(|r| (0..4).map(|k| a[k][r] * b[c][k]).sum()))
}

/// A glTF binary (`.glb`): every triangle primitive of the default scene,
/// with its material's factors (textures are not read).
pub fn glb(bytes: &[u8]) -> Result<Mesh, String> {
    let g = gltf::Gltf::from_slice(bytes).map_err(|e| e.to_string())?;
    let blob = g.blob.as_deref();
    let buffers: Vec<Option<&[u8]>> = g
        .buffers()
        .map(|b| match b.source() {
            gltf::buffer::Source::Bin => blob,
            gltf::buffer::Source::Uri(_) => None,
        })
        .collect();
    let scene = g
        .default_scene()
        .or_else(|| g.scenes().next())
        .ok_or("no scene")?;
    let mut parts = Vec::new();
    let mut stack: Vec<(gltf::Node<'_>, M4)> = scene
        .nodes()
        .map(|n| {
            let m = n.transform().matrix();
            (n, m)
        })
        .collect();
    while let Some((node, m)) = stack.pop() {
        for c in node.children() {
            let cm = mul(&m, &c.transform().matrix());
            stack.push((c, cm));
        }
        let Some(mesh) = node.mesh() else { continue };
        for prim in mesh.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = prim.reader(|b| buffers.get(b.index()).copied().flatten());
            let Some(pos) = reader.read_positions() else {
                continue;
            };
            let at = |p: [f32; 3], w: f32| -> [f32; 3] {
                std::array::from_fn(|r| {
                    m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r] * w
                })
            };
            let pos: Vec<[f32; 3]> = pos.map(|p| at(p, 1.)).collect();
            let indices: Vec<u32> = reader.read_indices().map_or_else(
                || (0..pos.len() as u32).collect(),
                |i| i.into_u32().collect(),
            );
            if indices.iter().any(|&i| i as usize >= pos.len()) {
                return Err("an index past the vertices".into());
            }
            let normals: Vec<[f32; 3]> = if let Some(n) = reader.read_normals() {
                n.map(|n| unit(at(n, 0.))).collect()
            } else {
                // Smooth normals from the faces around each vertex.
                let mut acc = vec![[0f32; 3]; pos.len()];
                for t in indices.as_chunks::<3>().0 {
                    let [a, b, c] = [0, 1, 2].map(|k| pos[t[k] as usize]);
                    let (u, v) = (sub(b, a), sub(c, a));
                    let n = [
                        u[1] * v[2] - u[2] * v[1],
                        u[2] * v[0] - u[0] * v[2],
                        u[0] * v[1] - u[1] * v[0],
                    ];
                    for &i in t {
                        for k in 0..3 {
                            acc[i as usize][k] += n[k];
                        }
                    }
                }
                acc.into_iter().map(unit).collect()
            };
            let material = prim.material();
            parts.push(Part {
                vertices: pos
                    .iter()
                    .zip(&normals)
                    .map(|(p, n)| [p[0], p[1], p[2], n[0], n[1], n[2]])
                    .collect(),
                indices,
                color: material.pbr_metallic_roughness().base_color_factor(),
                material: gltf_material(&material),
            });
        }
    }
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for v in parts.iter().flat_map(|p| &p.vertices) {
        for k in 0..3 {
            min[k] = min[k].min(v[k]);
            max[k] = max[k].max(v[k]);
        }
    }
    if parts.is_empty() {
        return Err("no triangles".into());
    }
    Ok(Mesh { parts, min, max })
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn unit(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-12 {
        v.map(|c| c / l)
    } else {
        [0., 1., 0.]
    }
}
