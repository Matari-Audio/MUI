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

/// A sunlit sky behind everything (over the scene's background, under an
/// environment shown as the background): the sun `elevation` degrees up
/// and `azimuth` degrees right of straight into the scene, and procedural
/// clouds over `cover` (0..1) of it, a layer overhead (and with the
/// gradient a sea below the horizon), lit from the sun and drifting right
/// `wind` cloud widths a second. Glass refracts and reflects it.
/// `intensity` scales all its light.
///
/// `model` `gradient` (the default) runs from `horizon` to `zenith`, lit
/// by `sun`. `physical` is the air itself scattering sunlight (Rayleigh,
/// Mie and ozone): blue overhead, paler at the horizon, the sun reddening
/// as it sets and twilight after, all from `elevation`; `turbidity` hazes
/// it (1 pure air, 2 a clear day, 10 hazy), `ozone` (1 the earth's) deepens
/// twilight's blue, `ground_albedo` lights the air from below and colours
/// the ground under the horizon, `altitude` is the eye's height in metres.
///
/// `light` makes the sky light the scene as an environment does (diffuse
/// and reflections; an `environment` given lights instead), and
/// `sun_light` adds a directional light along the sun, its colour the
/// sunlight left after the air, casting shadows. Both default on for the
/// physical sky and off for the gradient.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::vars::bindable)]
pub struct Sky {
    #[serde(default, skip_serializing_if = "crate::is_default")]
    pub model: SkyModel,
    #[serde(default = "elevation")]
    pub elevation: Anim<f64>,
    #[serde(default = "zero", skip_serializing_if = "is_zero")]
    pub azimuth: Anim<f64>,
    #[serde(default = "cover")]
    pub cover: Anim<f64>,
    #[serde(default = "wind")]
    pub wind: f64,
    #[serde(default = "zenith")]
    pub zenith: Anim<Rgba>,
    #[serde(default = "horizon")]
    pub horizon: Anim<Rgba>,
    #[serde(default = "sun")]
    pub sun: Anim<Rgba>,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub intensity: Anim<f64>,
    #[serde(default = "turbidity", skip_serializing_if = "is_turbidity")]
    pub turbidity: Anim<f64>,
    #[serde(default = "one_f", skip_serializing_if = "is_one_f")]
    pub ozone: f64,
    #[serde(default = "albedo", skip_serializing_if = "is_albedo")]
    pub ground_albedo: f64,
    #[serde(default, skip_serializing_if = "is_zero_f")]
    pub altitude: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sun_light: Option<bool>,
}

/// How a [`Sky`] is coloured.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum SkyModel {
    /// The artist's gradient from `horizon` to `zenith`.
    #[default]
    Gradient,
    /// Sunlight scattered by the air.
    Physical,
}
fn elevation() -> Anim<f64> {
    Anim::Value(25.)
}
fn cover() -> Anim<f64> {
    Anim::Value(0.45)
}
fn wind() -> f64 {
    0.04
}
fn zenith() -> Anim<Rgba> {
    Anim::Value(Rgba([0x3a, 0x6c, 0xb8, 255]))
}
fn horizon() -> Anim<Rgba> {
    Anim::Value(Rgba([0xcf, 0xdf, 0xee, 255]))
}
fn sun() -> Anim<Rgba> {
    Anim::Value(Rgba([0xff, 0xf1, 0xdc, 255]))
}
fn turbidity() -> Anim<f64> {
    Anim::Value(2.)
}
fn is_turbidity(a: &Anim<f64>) -> bool {
    *a == turbidity()
}
fn one_f() -> f64 {
    1.
}
fn is_one_f(v: &f64) -> bool {
    *v == 1.
}
fn albedo() -> f64 {
    0.3
}
fn is_albedo(v: &f64) -> bool {
    *v == 0.3
}
fn is_zero_f(v: &f64) -> bool {
    *v == 0.
}

impl Sky {
    /// Toward the sun at `t`, mui-stage's world (y up, z toward the viewer).
    pub fn sun_dir(&self, t: f64) -> [f32; 3] {
        let (el, az) = (
            self.elevation.at(t).to_radians(),
            self.azimuth.at(t).to_radians(),
        );
        [az.sin() * el.cos(), el.sin(), -az.cos() * el.cos()].map(|v| v as f32)
    }

    /// The air of a physical sky at `t`.
    pub fn air(&self, t: f64) -> mui_stage::Atmosphere {
        mui_stage::Atmosphere {
            turbidity: self.turbidity.at(t).clamp(1., 32.) as f32,
            ozone: self.ozone.max(0.) as f32,
            altitude: self.altitude.clamp(0., 50_000.) as f32,
            ground_albedo: self.ground_albedo.clamp(0., 1.) as f32,
            intensity: self.intensity.at(t).max(0.) as f32,
        }
    }

    fn physical(&self) -> bool {
        self.model == SkyModel::Physical
    }

    /// The sky at `t`, as mui-stage takes it.
    pub fn at(&self, t: f64) -> mui_stage::Sky {
        let cover = self.cover.at(t).clamp(0., 1.) as f32;
        let drift = [(self.wind * t) as f32, 0.];
        let light = self.light.unwrap_or(self.physical());
        if self.physical() {
            return mui_stage::Sky {
                light,
                ..mui_stage::Sky::physical(self.sun_dir(t), self.air(t), cover, drift)
            };
        }
        let k = self.intensity.at(t).max(0.) as f32;
        let lin = |a: &Anim<Rgba>| crate::gpu3d::linear(a.at(t)).map(|c| c * k);
        mui_stage::Sky {
            sun: self.sun_dir(t),
            zenith: lin(&self.zenith),
            horizon: lin(&self.horizon),
            // Sunlight is brighter than any sky: the disc and lit cloud.
            sun_color: lin(&self.sun).map(|c| c * 2.6),
            cover,
            drift,
            atmosphere: None,
            light,
        }
    }

    /// With `sun_light`, the sun as a directional lamp at `t`: along the
    /// sun, coloured by the sky's sunlight; none once the sun has set.
    pub fn sun_lamp(&self, t: f64) -> Option<Lamp> {
        if !self.sun_light.unwrap_or(self.physical()) {
            return None;
        }
        let k = self.at(t);
        // The gradient's sun colour is its disc's; fade it as it sets.
        let c = if self.physical() {
            k.sun_color
        } else {
            let up = smooth(-0.02, 0.05, k.sun[1]);
            crate::gpu3d::linear(self.sun.at(t)).map(|c| c * up * self.intensity.at(t) as f32)
        };
        let peak = c.iter().copied().fold(0f32, f32::max);
        (peak > 1e-4).then(|| Lamp {
            kind: LightType::Directional,
            color: crate::Rgba(std::array::from_fn(|i| {
                if i == 3 {
                    255
                } else {
                    (srgb(c[i] / peak) * 255. + 0.5) as u8
                }
            })),
            intensity: f64::from(peak),
            position: [0.; 3],
            // Project axes (y down, z deeper), travelling away from the sun.
            direction: [-k.sun[0], k.sun[1], k.sun[2]].map(f64::from),
            range: 0.,
            cone: 30.,
            feather: 0.3,
            shadows: true,
            // About the sun's half degree.
            softness: 0.7,
        })
    }
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

fn srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1. / 2.4) - 0.055
    }
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

/// Light brighter than `threshold` (1 is white) glows into what is round
/// it, by `strength`, as through a lens: a sun in frame, glints off glass.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::vars::bindable)]
pub struct Bloom {
    #[serde(default = "bloom_strength")]
    pub strength: f64,
    #[serde(default = "bloom_threshold")]
    pub threshold: f64,
}
fn bloom_strength() -> f64 {
    0.5
}
fn bloom_threshold() -> f64 {
    1.
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
    /// Glass: 0 the layer's colour stains the light through it; 1 the layer
    /// is printed on clear glass, its dark clear and its light marks ink (a
    /// dark UI turned to glass).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub print: Option<Anim<f64>>,
    /// Glass: pixels in from a slab's edge its face rounds over, so its rim
    /// bends what is behind it (a flat pane leaves a far sky in place).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bevel: Option<Anim<f64>>,
    /// Glass pressed with a pattern that tilts its face, so it bends,
    /// mirrors and catches light by it. Several mix; key their `strength`
    /// to crossfade one into another.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<Texture>,
    /// The layer gives off its own colour, this times as bright as white
    /// light on it: lit or in the dark, above 1 feeding the bloom (a screen,
    /// a lamp's shade). Opaque surfaces; glass ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emission: Option<Anim<f64>>,
}

/// Patterns pressed into a glass face (see [`Material::texture`]).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::vars::bindable)]
pub struct Texture {
    /// Reeds running up the face, each a lens across it (fluted glass).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ribbed: Option<Relief>,
    /// Hammered dimples.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hammered: Option<Relief>,
    /// Three crossing ripples that run with time, like water.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ripple: Option<Relief>,
}

/// One pattern of a [`Texture`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::vars::bindable)]
pub struct Relief {
    /// The steepest slope it presses the face to: 0 flat, 1 is 45 degrees.
    pub strength: Anim<f64>,
    /// Pixels across one reed, dimple or wave.
    #[serde(default = "relief_scale")]
    pub scale: Anim<f64>,
}
fn relief_scale() -> Anim<f64> {
    Anim::Value(24.)
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub print: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bevel: Option<f64>,
    /// [`Texture`]'s patterns: strength and scale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ribbed: Option<[f64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hammered: Option<[f64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ripple: Option<[f64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emission: Option<f64>,
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
            ("print", &self.print),
            ("bevel", &self.bevel),
            ("emission", &self.emission),
        ]
        .into_iter()
        .filter_map(|(n, a)| a.as_ref().map(|a| (format!("material.{n}"), Num(a))))
        .collect();
        if let Some(a) = &self.tint {
            out.push(("material.tint".into(), Color(a)));
        }
        for (n, r) in self.reliefs() {
            if let Some(r) = r {
                out.push((format!("material.texture.{n}.strength"), Num(&r.strength)));
                out.push((format!("material.texture.{n}.scale"), Num(&r.scale)));
            }
        }
        out
    }

    fn reliefs(&self) -> [(&'static str, Option<&Relief>); 3] {
        let t = self.texture.as_ref();
        [
            ("ribbed", t.and_then(|t| t.ribbed.as_ref())),
            ("hammered", t.and_then(|t| t.hammered.as_ref())),
            ("ripple", t.and_then(|t| t.ripple.as_ref())),
        ]
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
            print: num(&self.print).map(|v| v.clamp(0., 1.)),
            bevel: num(&self.bevel).map(|v| v.max(0.)),
            emission: num(&self.emission).map(|v| v.max(0.)),
            ..self.reliefs_at(t)
        }
    }

    fn reliefs_at(&self, t: f64) -> Surface {
        let [a, b, c] = self
            .reliefs()
            .map(|(_, r)| r.map(|r| [r.strength.at(t).max(0.), r.scale.at(t).max(0.)]));
        Surface {
            ribbed: a,
            hammered: b,
            ripple: c,
            ..Surface::default()
        }
    }
}

impl Surface {
    /// `base` with the fields this sets replaced.
    pub fn over(&self, base: mui_stage::Material) -> mui_stage::Material {
        let f = |v: Option<f64>, b: f32| v.map_or(b, |v| v as f32);
        let relief = |v: Option<[f64; 2]>, b| {
            v.map_or(b, |[s, k]| mui_stage::Relief {
                strength: s as f32,
                scale: k as f32,
            })
        };
        mui_stage::Material {
            metallic: f(self.metallic, base.metallic),
            roughness: f(self.roughness, base.roughness),
            transmission: f(self.transmission, base.transmission),
            ior: f(self.ior, base.ior),
            thickness: f(self.thickness, base.thickness),
            dispersion: f(self.dispersion, base.dispersion),
            tint: self.tint.map_or(base.tint, crate::gpu3d::linear),
            print: f(self.print, base.print),
            bevel: f(self.bevel, base.bevel),
            ribbed: relief(self.ribbed, base.ribbed),
            hammered: relief(self.hammered, base.hammered),
            ripple: relief(self.ripple, base.ripple),
            emission: f(self.emission, base.emission),
        }
    }
    /// `under` with the fields this sets replaced: a part's surface over
    /// its layer's.
    pub fn or(&self, under: &Surface) -> Surface {
        Surface {
            metallic: self.metallic.or(under.metallic),
            roughness: self.roughness.or(under.roughness),
            transmission: self.transmission.or(under.transmission),
            ior: self.ior.or(under.ior),
            thickness: self.thickness.or(under.thickness),
            dispersion: self.dispersion.or(under.dispersion),
            tint: self.tint.or(under.tint),
            print: self.print.or(under.print),
            bevel: self.bevel.or(under.bevel),
            ribbed: self.ribbed.or(under.ribbed),
            hammered: self.hammered.or(under.hammered),
            ripple: self.ripple.or(under.ripple),
            emission: self.emission.or(under.emission),
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
    /// Drawn flat over the 3D pass (see [`crate::Layer::overlay`]).
    pub overlay: bool,
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
            overlay: false,
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
    /// Handed to mui-stage as is.
    #[serde(skip)]
    pub sky: Option<mui_stage::Sky>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ao: Option<Ao>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bloom: Option<Bloom>,
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
        // The sky's sun first: shadow maps go to the first lights.
        lights: (scene.sky.as_ref())
            .and_then(|k| k.sun_lamp(t))
            .into_iter()
            .chain(
                layers
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
                    }),
            )
            .collect(),
        ground: scene.ground.clone(),
        fog: scene.fog.clone(),
        environment: scene.environment.as_ref().map(|e| Env {
            hdri: e.hdri.clone(),
            intensity: e.intensity.at(t).max(0.),
            rotation: e.rotation.at(t),
            background: e.background,
        }),
        sky: scene.sky.as_ref().map(|k| k.at(t)),
        ao: scene.ao.clone(),
        bloom: scene.bloom.clone(),
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
    // target: a positive pitch lifts it to look down. Looking at a layer,
    // it stands at its own x/y/z instead and only aims.
    let forward = aim(s.rx, s.ry);
    let mut eye: [f64; 3] = match looked {
        Some(_) => [c.x, c.y, s.z],
        None => std::array::from_fn(|i| target[i] - forward[i] * dist),
    };
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

/// One glTF primitive (y up): a position and a normal per vertex in its
/// node's space, texture coordinates, triangles, and its material: base
/// colour (linear RGBA), metallic, roughness, and glass from
/// `KHR_materials_transmission`, `_ior`, `_volume` (thickness in model
/// units) and `_dispersion` where it has them; and its maps, PNGs in
/// [`Mesh::images`]: base colour, normal, metallic-roughness.
#[derive(Clone, Debug)]
pub struct Part {
    pub vertices: Vec<[f32; 6]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub color: [f32; 4],
    pub material: mui_stage::Material,
    pub maps: [Option<usize>; 3],
    /// The node it hangs from: its pose places it.
    pub node: usize,
    /// Skinned: its skin, and each vertex's four joints and weights. Its
    /// vertices are then in the skin's bind space, the node ignored.
    pub skin: Option<(usize, Vec<Weights>)>,
}

/// A vertex's four joints and their weights.
pub type Weights = ([u16; 4], [f32; 4]);

/// An image a part maps: straight RGBA.
#[derive(Clone, Debug)]
pub struct Picture {
    pub rgba: Vec<u8>,
    pub size: [u32; 2],
}

/// A node's rest pose and parent.
#[derive(Clone, Debug)]
struct Node {
    parent: Option<usize>,
    /// Translation, rotation (x, y, z, w), scale.
    trs: ([f32; 3], [f32; 4], [f32; 3]),
}

/// One animated property of a node: keys' times and values (a rotation
/// four, else three); a cubic spline keeps its in- and out-tangents.
#[derive(Clone, Debug)]
struct Channel {
    node: usize,
    /// 0 translation, 1 rotation, 2 scale.
    path: u8,
    times: Vec<f32>,
    values: Vec<[f32; 4]>,
    interp: gltf::animation::Interpolation,
}

/// A skin: its joints (nodes) and their inverse bind matrices.
#[derive(Clone, Debug)]
struct Skin {
    joints: Vec<usize>,
    inverse: Vec<M4>,
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
        print: 0.,
        bevel: 0.,
        ..mui_stage::Material::SLAB
    }
}

/// A model file: its parts, images, nodes, first animation and skins, and
/// the bounds of its rest pose.
#[derive(Clone, Debug)]
pub struct Mesh {
    pub parts: Vec<Part>,
    pub images: Vec<Picture>,
    pub min: [f32; 3],
    pub max: [f32; 3],
    nodes: Vec<Node>,
    channels: Vec<Channel>,
    skins: Vec<Skin>,
}

type M4 = [[f32; 4]; 4];

fn mul(a: &M4, b: &M4) -> M4 {
    std::array::from_fn(|c| std::array::from_fn(|r| (0..4).map(|k| a[k][r] * b[c][k]).sum()))
}

fn apply(m: &M4, p: [f32; 3], w: f32) -> [f32; 3] {
    std::array::from_fn(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r] * w)
}

/// Translation, rotation (a unit quaternion, x y z w) and scale as a matrix.
fn trs_matrix((t, q, s): ([f32; 3], [f32; 4], [f32; 3])) -> M4 {
    let [x, y, z, w] = q;
    let r = [
        [
            1. - 2. * (y * y + z * z),
            2. * (x * y + z * w),
            2. * (x * z - y * w),
        ],
        [
            2. * (x * y - z * w),
            1. - 2. * (x * x + z * z),
            2. * (y * z + x * w),
        ],
        [
            2. * (x * z + y * w),
            2. * (y * z - x * w),
            1. - 2. * (x * x + y * y),
        ],
    ];
    std::array::from_fn(|c| {
        if c == 3 {
            [t[0], t[1], t[2], 1.]
        } else {
            [r[c][0] * s[c], r[c][1] * s[c], r[c][2] * s[c], 0.]
        }
    })
}

fn slerp(a: [f32; 4], b: [f32; 4], k: f32) -> [f32; 4] {
    let mut d: f32 = (0..4).map(|i| a[i] * b[i]).sum();
    let b = if d < 0. {
        d = -d;
        b.map(|v| -v)
    } else {
        b
    };
    let (wa, wb) = if d > 0.9995 {
        (1. - k, k)
    } else {
        let th = d.acos();
        let s = th.sin();
        (((1. - k) * th).sin() / s, (k * th).sin() / s)
    };
    let q: [f32; 4] = std::array::from_fn(|i| wa * a[i] + wb * b[i]);
    let l = q.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
    q.map(|v| v / l)
}

impl Channel {
    /// Its value at `t` seconds, held before the first key and after the
    /// last.
    fn at(&self, t: f32) -> [f32; 4] {
        use gltf::animation::Interpolation::{CubicSpline, Step};
        let cubic = self.interp == CubicSpline;
        // A cubic spline stores in-tangent, value, out-tangent per key.
        let value = |i: usize| {
            if cubic {
                self.values[3 * i + 1]
            } else {
                self.values[i]
            }
        };
        let n = self.times.len();
        let i = self.times.partition_point(|&k| k <= t);
        if i == 0 {
            return value(0);
        }
        if i >= n {
            return value(n - 1);
        }
        let (t0, t1) = (self.times[i - 1], self.times[i]);
        let dt = (t1 - t0).max(1e-9);
        let k = ((t - t0) / dt).clamp(0., 1.);
        let (a, b) = (value(i - 1), value(i));
        let v = match self.interp {
            Step => a,
            CubicSpline => {
                let (m0, m1) = (self.values[3 * (i - 1) + 2], self.values[3 * i]);
                let (k2, k3) = (k * k, k * k * k);
                std::array::from_fn(|c| {
                    (2. * k3 - 3. * k2 + 1.) * a[c]
                        + (k3 - 2. * k2 + k) * dt * m0[c]
                        + (-2. * k3 + 3. * k2) * b[c]
                        + (k3 - k2) * dt * m1[c]
                })
            }
            _ if self.path == 1 => return slerp(a, b, k),
            _ => std::array::from_fn(|c| a[c] + (b[c] - a[c]) * k),
        };
        if self.path == 1 {
            let l = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
            v.map(|x| x / l)
        } else {
            v
        }
    }
}

/// Node `i`'s model-space matrix, its parents' resolved into `out` first.
fn node_world(
    i: usize,
    nodes: &[Node],
    trs: &[([f32; 3], [f32; 4], [f32; 3])],
    out: &mut [Option<M4>],
) -> M4 {
    if let Some(m) = out[i] {
        return m;
    }
    let local = trs_matrix(trs[i]);
    let m = match nodes[i].parent {
        Some(p) => mul(&node_world(p, nodes, trs, out), &local),
        None => local,
    };
    out[i] = Some(m);
    m
}

impl Mesh {
    /// Whether it moves: an animation, which [`Mesh::pose`] plays.
    pub fn animated(&self) -> bool {
        !self.channels.is_empty()
    }

    /// Every node's model-space matrix `t` seconds into the first
    /// animation (`None`: the rest pose).
    pub fn pose(&self, t: Option<f64>) -> Vec<M4> {
        let mut trs: Vec<_> = self.nodes.iter().map(|n| n.trs).collect();
        if let Some(t) = t {
            for c in &self.channels {
                let v = c.at(t as f32);
                let n = &mut trs[c.node];
                match c.path {
                    0 => n.0 = [v[0], v[1], v[2]],
                    1 => n.1 = v,
                    _ => n.2 = [v[0], v[1], v[2]],
                }
            }
        }
        // Parents come first: glTF does not promise it, so resolve lazily.
        let mut out: Vec<Option<M4>> = vec![None; self.nodes.len()];
        (0..self.nodes.len())
            .map(|i| node_world(i, &self.nodes, &trs, &mut out))
            .collect()
    }

    /// A part's matrix in `pose` (a skinned part's is the identity: its
    /// joints place it).
    pub fn place(&self, part: &Part, pose: &[M4]) -> mui_stage::Mat4 {
        let m = if part.skin.is_some() {
            return mui_stage::Mat4::IDENTITY;
        } else {
            pose[part.node]
        };
        mui_stage::Mat4(std::array::from_fn(|i| m[i / 4][i % 4]))
    }

    /// A skinned part's vertices bent by its joints in `pose`.
    pub fn skinned(&self, part: &Part, pose: &[M4]) -> Option<Vec<[f32; 6]>> {
        let (skin, weights) = part.skin.as_ref()?;
        let skin = &self.skins[*skin];
        let joints: Vec<M4> = skin
            .joints
            .iter()
            .zip(&skin.inverse)
            .map(|(&j, inv)| mul(&pose[j], inv))
            .collect();
        Some(
            part.vertices
                .iter()
                .zip(weights)
                .map(|(v, (js, ws))| {
                    let mut m = [[0f32; 4]; 4];
                    for (j, w) in js.iter().zip(ws) {
                        let Some(jm) = joints.get(usize::from(*j)) else {
                            continue;
                        };
                        for c in 0..4 {
                            for r in 0..4 {
                                m[c][r] += w * jm[c][r];
                            }
                        }
                    }
                    let p = apply(&m, [v[0], v[1], v[2]], 1.);
                    let n = unit(apply(&m, [v[3], v[4], v[5]], 0.));
                    [p[0], p[1], p[2], n[0], n[1], n[2]]
                })
                .collect(),
        )
    }
}

/// A glTF binary (`.glb`): every triangle primitive of the default scene,
/// its material's factors and PNG maps, its nodes, the first animation and
/// skins. A JPEG map is left out (no decoder here): the factor stands.
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
    let get = |b: gltf::Buffer<'_>| buffers.get(b.index()).copied().flatten();
    let scene = g
        .default_scene()
        .or_else(|| g.scenes().next())
        .ok_or("no scene")?;
    let mut nodes: Vec<Node> = g
        .nodes()
        .map(|n| {
            let (t, r, s) = n.transform().decomposed();
            Node {
                parent: None,
                trs: (t, r, s),
            }
        })
        .collect();
    for n in g.nodes() {
        for c in n.children() {
            nodes[c.index()].parent = Some(n.index());
        }
    }
    let images: Vec<Picture> = g
        .images()
        .map(|im| match im.source() {
            gltf::image::Source::View {
                view,
                mime_type: "image/png",
            } => {
                let data = get(view.buffer())?;
                let bytes = data.get(view.offset()..view.offset() + view.length())?;
                let (rgba, size) = crate::render::png_rgba(bytes).ok()?;
                Some(Picture { rgba, size })
            }
            _ => None,
        })
        .map(|p| {
            p.unwrap_or(Picture {
                rgba: Vec::new(),
                size: [0, 0],
            })
        })
        .collect();
    let map = |t: Option<gltf::Texture<'_>>| {
        let i = t?.source().index();
        (!images.get(i)?.rgba.is_empty()).then_some(i)
    };
    let mut skins = Vec::new();
    for skin in g.skins() {
        let joints: Vec<usize> = skin.joints().map(|j| j.index()).collect();
        let inverse = skin.reader(get).read_inverse_bind_matrices().map_or_else(
            || vec![trs_matrix(([0.; 3], [0., 0., 0., 1.], [1.; 3])); joints.len()],
            Iterator::collect,
        );
        if inverse.len() < joints.len() {
            return Err("a skin with fewer inverse bind matrices than joints".into());
        }
        skins.push(Skin { joints, inverse });
    }
    let channels: Vec<Channel> = g
        .animations()
        .next()
        .map(|a| {
            a.channels()
                .filter_map(|c| {
                    use gltf::animation::util::ReadOutputs;
                    let r = c.reader(get);
                    let times: Vec<f32> = r.read_inputs()?.collect();
                    let (path, values): (u8, Vec<[f32; 4]>) = match r.read_outputs()? {
                        ReadOutputs::Translations(v) => {
                            (0, v.map(|p| [p[0], p[1], p[2], 0.]).collect())
                        }
                        ReadOutputs::Rotations(v) => (1, v.into_f32().collect()),
                        ReadOutputs::Scales(v) => (2, v.map(|p| [p[0], p[1], p[2], 0.]).collect()),
                        ReadOutputs::MorphTargetWeights(_) => return None,
                    };
                    let interp = c.sampler().interpolation();
                    let per = if interp == gltf::animation::Interpolation::CubicSpline {
                        3
                    } else {
                        1
                    };
                    (!times.is_empty() && values.len() >= per * times.len()).then_some(Channel {
                        node: c.target().node().index(),
                        path,
                        times,
                        values,
                        interp,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let mut parts = Vec::new();
    // Depth first, in the file's order.
    let mut stack: Vec<gltf::Node<'_>> = scene.nodes().collect();
    stack.reverse();
    while let Some(node) = stack.pop() {
        stack.extend(node.children().collect::<Vec<_>>().into_iter().rev());
        let Some(mesh) = node.mesh() else { continue };
        let skin = node.skin().map(|s| s.index());
        for prim in mesh.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = prim.reader(get);
            let Some(pos) = reader.read_positions() else {
                continue;
            };
            let pos: Vec<[f32; 3]> = pos.collect();
            let indices: Vec<u32> = reader.read_indices().map_or_else(
                || (0..pos.len() as u32).collect(),
                |i| i.into_u32().collect(),
            );
            if indices.iter().any(|&i| i as usize >= pos.len()) {
                return Err("an index past the vertices".into());
            }
            let normals: Vec<[f32; 3]> = if let Some(n) = reader.read_normals() {
                n.map(unit).collect()
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
            let uvs: Vec<[f32; 2]> = reader
                .read_tex_coords(0)
                .map(|t| t.into_f32().collect())
                .filter(|t: &Vec<[f32; 2]>| t.len() == pos.len())
                .unwrap_or_default();
            let weights = match (skin, reader.read_joints(0), reader.read_weights(0)) {
                (Some(s), Some(j), Some(w)) => {
                    let w: Vec<([u16; 4], [f32; 4])> = j.into_u16().zip(w.into_f32()).collect();
                    (w.len() == pos.len()).then_some((s, w))
                }
                _ => None,
            };
            let material = prim.material();
            let pbr = material.pbr_metallic_roughness();
            let mut maps = [
                map(pbr.base_color_texture().map(|t| t.texture())),
                map(material.normal_texture().map(|t| t.texture())),
                map(pbr.metallic_roughness_texture().map(|t| t.texture())),
            ];
            if uvs.is_empty() {
                maps = [None; 3];
            }
            parts.push(Part {
                vertices: pos
                    .iter()
                    .zip(&normals)
                    .map(|(p, n)| [p[0], p[1], p[2], n[0], n[1], n[2]])
                    .collect(),
                uvs,
                indices,
                color: pbr.base_color_factor(),
                material: gltf_material(&material),
                maps,
                node: node.index(),
                skin: weights,
            });
        }
    }
    if parts.is_empty() {
        return Err("no triangles".into());
    }
    let mut out = Mesh {
        parts,
        images,
        min: [f32::MAX; 3],
        max: [f32::MIN; 3],
        nodes,
        channels,
        skins,
    };
    // Bounds of the rest pose.
    let rest = out.pose(None);
    let (mut min, mut max) = (out.min, out.max);
    for part in &out.parts {
        let skinned = out.skinned(part, &rest);
        let m = if part.skin.is_some() {
            None
        } else {
            Some(rest[part.node])
        };
        for v in skinned.as_ref().unwrap_or(&part.vertices) {
            let p = m.map_or([v[0], v[1], v[2]], |m| apply(&m, [v[0], v[1], v[2]], 1.));
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
    }
    (out.min, out.max) = (min, max);
    Ok(out)
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
