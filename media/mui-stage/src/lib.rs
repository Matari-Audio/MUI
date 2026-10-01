//! A GPU stage for trailers: MUI layers painted by Vello into mipmapped,
//! linear-light textures, set on extruded slabs (or extruded 3D text) in a
//! lit, perspective 3D scene over a WGSL background and a reflective floor,
//! then given depth of field, bloom, aberration, vignette, a tonemap and
//! grain. Motion blur is real: each frame averages subframes across the
//! shutter.
//!
//! The caller owns time. `Stage::render(t, ..)` asks a closure for the
//! [`Shot`] at every subframe time, so the same shot function renders
//! frame-exact at any fps, and the UI inside each layer is whatever `Ui`
//! produced for that frame.
//!
//! ```no_run
//! # use mui_stage::*;
//! # fn demo(scene: &mui_scene::ResolvedScene) -> Result<(), Error> {
//! let mut stage = Stage::new(1280, 720)?;
//! stage.layer("ui", scene, mui_scene::Size::new(420., 260.), 2.)?;
//! let frame = stage.render(0.5, 1. / 60., 8, &|t| Shot {
//!     planes: vec![Plane::new("ui", 420., 260.).rotate(0., (t * 40.) as f32, 0.).depth(18.)],
//!     ..Shot::new(Camera::front(720., 35.))
//! })?;
//! let rgba8 = frame.rgba8();
//! # Ok(()) }
//! ```
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use mui_geometry::Path;
use mui_scene::{ResolvedScene, Size};
use mui_vello::effects::{Budget, GpuRenderer};
use mui_vello::kurbo::Affine;
use wgpu::util::DeviceExt;

pub mod env;
mod math;
pub use env::EnvImage;
pub use math::{Mat4, disc, halton, sample};

/// Why the stage could not render.
#[derive(Debug)]
pub enum Error {
    /// The adapter, device, readback or a shader said no.
    Gpu(String),
    /// The stage's own device was lost (driver reset, GPU removed). Nothing
    /// on it survives: make a new [`Stage`] and paint its layers again.
    DeviceLost,
    /// A shot names a layer [`Stage::layer`] never painted.
    MissingLayer(String),
    /// A shot names a mesh [`Stage::mesh`] never uploaded.
    MissingMesh(String),
    /// A shot names an environment [`Stage::environment`] never uploaded.
    MissingEnvironment(String),
    /// Vello could not paint a layer.
    Render(mui_vello::effects::Error),
    Text(mui_text::Error),
    Geometry(mui_geometry::Error),
    Scene(mui_scene::SceneError),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Gpu(s) => write!(f, "stage GPU: {s}"),
            Self::DeviceLost => f.write_str("stage GPU: device lost; make a new Stage"),
            Self::MissingLayer(s) => write!(f, "no layer {s:?}: paint it with Stage::layer first"),
            Self::MissingMesh(s) => write!(f, "no mesh {s:?}: upload it with Stage::mesh first"),
            Self::MissingEnvironment(s) => write!(
                f,
                "no environment {s:?}: upload it with Stage::environment first"
            ),
            Self::Render(e) => write!(f, "stage layer: {e}"),
            Self::Text(e) => write!(f, "stage text: {e}"),
            Self::Geometry(e) => write!(f, "stage geometry: {e}"),
            Self::Scene(e) => write!(f, "stage scene: {e}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<mui_vello::effects::Error> for Error {
    fn from(e: mui_vello::effects::Error) -> Self {
        Self::Render(e)
    }
}
impl From<mui_text::Error> for Error {
    fn from(e: mui_text::Error) -> Self {
        Self::Text(e)
    }
}
impl From<mui_geometry::Error> for Error {
    fn from(e: mui_geometry::Error) -> Self {
        Self::Geometry(e)
    }
}
impl From<mui_scene::SceneError> for Error {
    fn from(e: mui_scene::SceneError) -> Self {
        Self::Scene(e)
    }
}
fn gpu(e: impl std::fmt::Display) -> Error {
    Error::Gpu(e.to_string())
}

const SHADER: &str = include_str!("stage.wgsl");
/// Drifting light over a dark floor: something to see through the glass
/// before a custom background is set.
pub const DEFAULT_BACKGROUND: &str = r#"
fn background(uv: vec2f, t: f32) -> vec3f {
    let p = uv * vec2f(3.2, 1.8);
    let n = fbm(p + vec2f(t * 0.05, -t * 0.03) + fbm(p * 1.7 - t * 0.04));
    let glow = exp(-8. * length(uv - vec2f(0.5 + 0.2 * sin(t * 0.3), 0.35)));
    return vec3f(0.006, 0.008, 0.02) + vec3f(0.02, 0.03, 0.09) * n * n + vec3f(0.08, 0.05, 0.25) * glow;
}
"#;
const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const OUT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SAMPLES: u32 = 4;
/// Per-draw uniform slot: `Draw` is 176 bytes, dynamic offsets align to 256.
const SLOT: u64 = 256;
const DRAW: u64 = 176;
/// Most planes and most models a shot draws.
pub const MAX_PLANES: usize = 64;
pub const MAX_MODELS: usize = 64;
/// Most lights with a direction (ambient ones are summed and do not count).
pub const MAX_LIGHTS: usize = 4;
/// Draw slots per subframe: every plane and model, its reflection, and the
/// floor.
const SLOTS: usize = 2 * (MAX_PLANES + MAX_MODELS) + 1;
/// `Globals` in `stage.wgsl`, in floats.
const GLOBALS: usize = 256;
/// The environment a shot may name without uploading it: the built-in
/// neutral studio ([`EnvImage::studio`]).
pub const STUDIO: &str = "studio";
/// Shadow map side, texels; one array layer per light, and one more for the
/// floor's contact shadow.
const SHADOW: u32 = 2048;
const CONTACT_LAYER: u32 = MAX_LIGHTS as u32;
/// A point light's six cube faces' side, texels: one array layer per face,
/// six per light. A light's shadow index past this is a point light's.
const CUBE: u32 = 1024;
const CUBE_BASE: u32 = 8;
const BLOOM_LEVELS: usize = 6;

/// Where the camera is and what it sees. World units are logical pixels,
/// y up, z toward the viewer.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    /// Vertical field of view, degrees.
    pub fov: f32,
    /// Roll about the view axis, degrees.
    pub roll: f32,
}
impl Camera {
    /// Straight on, at the distance where a layer `height` units tall at the
    /// origin exactly fills the frame: a flat, unrotated plane at
    /// `Camera::front(h, _)` reproduces the 2D render.
    pub fn front(height: f32, fov: f32) -> Self {
        let z = height * 0.5 / (fov.to_radians() * 0.5).tan();
        Self {
            eye: [0., 0., z],
            target: [0., 0., 0.],
            fov,
            roll: 0.,
        }
    }
    /// Circle the target: `yaw` about y, `pitch` about x, degrees.
    pub fn orbit(mut self, yaw: f32, pitch: f32) -> Self {
        let d: Vec<f32> = (0..3).map(|i| self.eye[i] - self.target[i]).collect();
        let r = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let (y, p) = (yaw.to_radians(), pitch.to_radians());
        self.eye = [
            self.target[0] + r * p.cos() * y.sin(),
            self.target[1] + r * p.sin(),
            self.target[2] + r * p.cos() * y.cos(),
        ];
        self
    }
    /// Aim at a point of a layer `canvas` wide and tall, centred on the
    /// origin, as a 2D camera would: `centre` in the layer's y-down logical
    /// units comes to the middle of the frame, and `zoom` 2 is twice as
    /// close. This is `mui-reel`'s `Take::camera`; orbit after it and the
    /// punch-in keeps its subject.
    pub fn punch(mut self, canvas: [f32; 2], centre: [f32; 2], zoom: f32) -> Self {
        let shift = [centre[0] - canvas[0] * 0.5, canvas[1] * 0.5 - centre[1]];
        for (i, d) in shift.into_iter().enumerate() {
            self.eye[i] += d;
            self.target[i] += d;
        }
        let k = 1. / zoom.max(1e-3);
        for i in 0..3 {
            self.eye[i] = self.target[i] + (self.eye[i] - self.target[i]) * k;
        }
        self
    }
    /// Move toward the target by `factor` of the distance (0.5 = halfway).
    pub fn dolly(mut self, factor: f32) -> Self {
        for i in 0..3 {
            self.eye[i] += (self.target[i] - self.eye[i]) * factor;
        }
        self
    }
    /// Eye to target: focus here to keep what the camera looks at sharp.
    pub fn distance(&self) -> f32 {
        (0..3)
            .map(|i| (self.eye[i] - self.target[i]).powi(2))
            .sum::<f32>()
            .sqrt()
    }
    /// World to clip space for a frame `aspect` wide per unit high.
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let [_, up, _] = self.basis();
        Mat4::perspective(self.fov.to_radians(), aspect, 1., 100_000.)
            * Mat4::look_at(self.eye, self.target, up)
    }
    /// The view's right, up and forward unit vectors in the world.
    pub fn basis(&self) -> [[f32; 3]; 3] {
        // Rolled about the view axis: straight on, up leans left by `roll`.
        let f: [f32; 3] = std::array::from_fn(|i| self.target[i] - self.eye[i]);
        let f = normalize(f);
        let world_up = if f[1].abs() > 0.999 {
            [0., 0., -1.]
        } else {
            [0., 1., 0.]
        };
        let s = normalize([
            f[1] * world_up[2] - f[2] * world_up[1],
            f[2] * world_up[0] - f[0] * world_up[2],
            f[0] * world_up[1] - f[1] * world_up[0],
        ]);
        let u = [
            s[1] * f[2] - s[2] * f[1],
            s[2] * f[0] - s[0] * f[2],
            s[0] * f[1] - s[1] * f[0],
        ];
        let (sin, cos) = self.roll.to_radians().sin_cos();
        let up: [f32; 3] = std::array::from_fn(|i| u[i] * cos - s[i] * sin);
        let right: [f32; 3] = std::array::from_fn(|i| s[i] * cos + u[i] * sin);
        [right, up, f]
    }
}

/// How a surface meets light, after glTF's metallic-roughness PBR and
/// Blender's Principled BSDF. Anything with `transmission` above zero is
/// glass: drawn after the opaque scene, which it reflects and refracts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    pub metallic: f32,
    pub roughness: f32,
    /// 0 opaque, 1 all the light that is not reflected passes through.
    pub transmission: f32,
    /// Index of refraction at 587.6 nm (glass 1.5, water 1.33).
    pub ior: f32,
    /// World units the light crosses inside. On a plane 0 is its depth.
    pub thickness: f32,
    /// 20 / the Abbe number (glTF's `KHR_materials_dispersion`): 0 none,
    /// crown glass about 0.35, flint 0.6.
    pub dispersion: f32,
    /// Linear RGB left of white light after `thickness` inside.
    pub tint: [f32; 3],
}
impl Material {
    /// A slab's face: a plain, fairly rough dielectric.
    pub const SLAB: Self = Self {
        metallic: 0.,
        roughness: 0.42,
        transmission: 0.,
        ior: 1.5,
        thickness: 0.,
        dispersion: 0.,
        tint: [1.; 3],
    };
    /// Whether it is drawn as glass.
    pub fn glass(&self) -> bool {
        self.transmission > 0.
    }
}
impl Default for Material {
    fn default() -> Self {
        Self::SLAB
    }
}

/// One layer set in the world: a slab whose front face is the layer's
/// pixels, whose walls follow `outline`, and whose back is its silhouette.
#[derive(Clone, Debug)]
pub struct Plane {
    pub layer: String,
    /// The layer's size in world units; the slab is centred on its origin.
    pub size: [f32; 2],
    pub position: [f32; 3],
    /// Degrees about x, y, z, applied z then x then y.
    pub rotation: [f32; 3],
    pub scale: f32,
    /// Slab thickness. Zero is a card with no walls.
    pub depth: f32,
    /// Walls' outline in layer space (y-down logical px); `None` is the
    /// layer's rectangle.
    pub outline: Option<Arc<Path>>,
    /// Where the slab's centre sits from its pivot (`position`), in its own
    /// unrotated, unscaled space: the pivot is what it turns and scales about.
    pub offset: [f32; 3],
    /// The part of the layer texture on the face: `[u0, v0, u1, v1]`, so
    /// many planes can share one atlas.
    pub uv: [f32; 4],
    /// Casts shadows, and receives them (only lit shots have any).
    pub cast: bool,
    pub receive: bool,
    /// Wall and back colour, linear RGB.
    pub edge: [f32; 3],
    /// Multiplies the face's light: above 1 it feeds the bloom.
    pub glow: f32,
    pub opacity: f32,
    pub material: Material,
}
impl Plane {
    pub fn new(layer: &str, width: f32, height: f32) -> Self {
        Self {
            layer: layer.into(),
            size: [width, height],
            position: [0.; 3],
            rotation: [0.; 3],
            scale: 1.,
            depth: 0.,
            outline: None,
            offset: [0.; 3],
            uv: [0., 0., 1., 1.],
            cast: true,
            receive: true,
            edge: [0.05, 0.05, 0.07],
            glow: 1.,
            opacity: 1.,
            material: Material::SLAB,
        }
    }
    pub fn at(mut self, x: f32, y: f32, z: f32) -> Self {
        self.position = [x, y, z];
        self
    }
    pub fn rotate(mut self, x: f32, y: f32, z: f32) -> Self {
        self.rotation = [x, y, z];
        self
    }
    pub fn scale(mut self, s: f32) -> Self {
        self.scale = s;
        self
    }
    pub fn depth(mut self, d: f32) -> Self {
        self.depth = d;
        self
    }
    pub fn outline(mut self, p: Arc<Path>) -> Self {
        self.outline = Some(p);
        self
    }
    pub fn offset(mut self, o: [f32; 3]) -> Self {
        self.offset = o;
        self
    }
    pub fn uv(mut self, uv: [f32; 4]) -> Self {
        self.uv = uv;
        self
    }
    pub fn edge(mut self, rgb: [f32; 3]) -> Self {
        self.edge = rgb;
        self
    }
    pub fn glow(mut self, g: f32) -> Self {
        self.glow = g;
        self
    }
    pub fn opacity(mut self, o: f32) -> Self {
        self.opacity = o;
        self
    }
    pub fn material(mut self, m: Material) -> Self {
        self.material = m;
        self
    }
    /// Local slab space to the world.
    pub fn model(&self) -> Mat4 {
        let [x, y, z] = self.rotation.map(f32::to_radians);
        Mat4::translate(self.position)
            * Mat4::rotate_y(y)
            * Mat4::rotate_x(x)
            * Mat4::rotate_z(z)
            * Mat4::scale(self.scale)
            * Mat4::translate(self.offset)
    }
}

/// The lens and the film.
#[derive(Clone, Copy, Debug)]
pub struct Post {
    /// Linear brightness where bloom starts, and its soft knee.
    pub threshold: f32,
    pub knee: f32,
    pub bloom: f32,
    pub exposure: f32,
    /// Radial RGB split at the frame's corners, in UV (0.004 is subtle).
    pub aberration: f32,
    /// 0 none, 1 full.
    pub vignette: f32,
    /// Film grain amplitude in display values (0.03 is visible).
    pub grain: f32,
    /// ACES filmic curve. Off, and with everything else at `Post::NONE`,
    /// a face shows the layer's exact pixels.
    pub tonemap: bool,
    /// Depth of field: the distance from the eye that is sharp, in world
    /// units. Zero is everything sharp. [`Camera::distance`] is the usual
    /// choice.
    pub focus: f32,
    /// Blur radius, in output pixels, of something infinitely far behind
    /// the focus; nearer misses blur proportionally.
    pub aperture: f32,
    /// The largest blur radius in pixels (the gather's reach).
    pub max_blur: f32,
}
impl Post {
    /// The layer as painted: no bloom, lens or film.
    pub const NONE: Self = Self {
        threshold: 1.,
        knee: 0.,
        bloom: 0.,
        exposure: 1.,
        aberration: 0.,
        vignette: 0.,
        grain: 0.,
        tonemap: false,
        focus: 0.,
        aperture: 0.,
        max_blur: 0.,
    };
}
impl Default for Post {
    fn default() -> Self {
        Self {
            threshold: 0.9,
            knee: 0.4,
            bloom: 0.6,
            exposure: 1.,
            aberration: 0.002,
            vignette: 0.5,
            grain: 0.025,
            tonemap: true,
            focus: 0.,
            aperture: 24.,
            max_blur: 16.,
        }
    }
}

/// A glossy floor under the slabs: it mirrors them, fading with their
/// height above it and toward its rim, and melts into the background.
#[derive(Clone, Copy, Debug)]
pub struct Floor {
    /// World height of the floor (y up).
    pub y: f32,
    /// Linear RGB.
    pub color: [f32; 3],
    /// How much of a slab's light the floor mirrors, 0..1. A reflection
    /// always hides the ones behind it; this only fades it into the floor.
    pub reflect: f32,
    /// World distance below the floor over which a reflection fades by 1/e.
    pub falloff: f32,
    /// Radius around the origin at which floor and reflection fade by 1/e.
    pub radius: f32,
    /// Contact shadow: how dark the floor gets right under what stands on
    /// it (0 none, 1 black), fading out over `contact_height` above it.
    pub contact: f32,
    pub contact_height: f32,
}
impl Floor {
    pub fn at(y: f32) -> Self {
        Self {
            y,
            color: [0.004, 0.004, 0.008],
            reflect: 0.35,
            falloff: 120.,
            radius: 1600.,
            contact: 0.,
            contact_height: 120.,
        }
    }
}

/// What a [`Light`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightKind {
    /// Everywhere, from every side, no shadow.
    Ambient,
    /// Parallel rays along `direction` (the sun).
    Directional,
    /// From `position` every way; a cube of shadow maps.
    Point,
    /// From `position` along `direction`, in a cone.
    Spot,
}

/// A light. A shot with no lights is unlit: faces show their layer's exact
/// pixels. With any, faces, walls, models and the floor are shaded by them.
#[derive(Clone, Copy, Debug)]
pub struct Light {
    pub kind: LightKind,
    /// Linear RGB times intensity.
    pub color: [f32; 3],
    pub position: [f32; 3],
    /// The way the light travels (normalised here).
    pub direction: [f32; 3],
    /// Point and spot: distance at which the light has faded out; 0 never.
    pub range: f32,
    /// Spot: half-angle of the cone, degrees, and the fraction of it that
    /// fades (0 hard edge, 1 all soft).
    pub cone: f32,
    pub feather: f32,
    /// Cast a shadow map (a cube of six for a point light), softened by
    /// `softness` shadow texels.
    pub shadows: bool,
    pub softness: f32,
}
impl Light {
    pub fn new(kind: LightKind) -> Self {
        Self {
            kind,
            color: [1.; 3],
            position: [0.; 3],
            direction: [0., -1., 0.],
            range: 0.,
            cone: 30.,
            feather: 0.2,
            shadows: true,
            softness: 1.5,
        }
    }
}

/// A mesh [`Stage::mesh`] uploaded, placed in the world.
#[derive(Clone, Debug)]
pub struct Model {
    pub mesh: String,
    pub transform: Mat4,
    /// Base colour, linear RGB, and opacity.
    pub color: [f32; 4],
    pub material: Material,
    pub cast: bool,
    pub receive: bool,
    /// Images [`Stage::texture`] uploaded, over the mesh's UVs: base colour
    /// (times `color`), a tangent-space normal map, and metallic (blue) and
    /// roughness (green) times the material's, as glTF has them.
    pub maps: [Option<String>; 3],
}

/// Distance fog: surfaces fade into `color` between `near` and `far` units
/// from the eye. The background is not fogged.
#[derive(Clone, Copy, Debug)]
pub struct Fog {
    /// Linear RGB.
    pub color: [f32; 3],
    pub near: f32,
    pub far: f32,
}

/// Image-based light from an environment [`Stage::environment`] uploaded
/// (or [`STUDIO`]): diffuse from its irradiance, and reflections a metal's
/// roughness blurs. A shot with one is lit even with no [`Light`]s.
#[derive(Clone, Debug, PartialEq)]
pub struct Environment {
    pub map: String,
    /// Multiplies the image's light.
    pub intensity: f32,
    /// Degrees about the world's y axis.
    pub rotation: f32,
    /// The camera sees the environment behind everything, instead of the
    /// background or [`Shot::clear`].
    pub background: bool,
}
impl Environment {
    /// The built-in studio at intensity 1.
    pub fn studio() -> Self {
        Self {
            map: STUDIO.into(),
            intensity: 1.,
            rotation: 0.,
            background: false,
        }
    }
}

/// Ground-truth ambient occlusion: a half-resolution screen-space pass
/// that darkens creases and contacts within `radius` world units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ao {
    /// 0 none, 1 the full occlusion; above 1 deepens it.
    pub strength: f32,
    pub radius: f32,
}

/// Everything in front of the camera at one instant.
#[derive(Clone, Debug)]
pub struct Shot {
    pub camera: Camera,
    /// Drawn walls first, then faces far to near, so glass edges blend over
    /// what is behind them. At most [`MAX_PLANES`].
    pub planes: Vec<Plane>,
    /// Opaque meshes, drawn before the planes. At most [`MAX_MODELS`].
    pub models: Vec<Model>,
    pub lights: Vec<Light>,
    pub floor: Option<Floor>,
    pub fog: Option<Fog>,
    /// A solid background (linear RGB) instead of the WGSL one.
    pub clear: Option<[f32; 3]>,
    pub environment: Option<Environment>,
    pub ao: Option<Ao>,
    pub post: Post,
    /// One sample of a beauty frame ([`Stage::beauty`]): the pixel, the
    /// lens (depth of field through a thin lens instead of the post blur),
    /// every shadowing light's area and the occlusion's turn move to
    /// [`sample`]`(i)`, so a mean of many is antialiased, with real
    /// penumbrae and converged occlusion. `None` is the plain instant.
    pub sample: Option<u32>,
    /// Screen-space reflection: lit surfaces mirror what is on screen, not
    /// only the environment. Glass always reflects and refracts the frame.
    pub ssr: bool,
}
impl Shot {
    pub fn new(camera: Camera) -> Self {
        Self {
            camera,
            planes: Vec::new(),
            models: Vec::new(),
            lights: Vec::new(),
            floor: None,
            fog: None,
            clear: None,
            environment: None,
            ao: None,
            post: Post::default(),
            sample: None,
            ssr: true,
        }
    }
}

/// A rendered frame: display-encoded RGBA, 0..1, top row first.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<f32>,
}
impl Frame {
    pub fn rgba8(&self) -> Vec<u8> {
        self.rgba
            .iter()
            .map(|v| (v.clamp(0., 1.) * 255. + 0.5) as u8)
            .collect()
    }
    /// For ffmpeg's `rgba64le`: the 10-bit master keeps the gradients
    /// 8 bits would band.
    pub fn rgba16(&self) -> Vec<u8> {
        self.rgba
            .iter()
            .flat_map(|v| ((v.clamp(0., 1.) * 65535. + 0.5) as u16).to_le_bytes())
            .collect()
    }
}

struct Layer {
    /// What Vello paints: premultiplied sRGB bytes.
    raw: wgpu::Texture,
    /// The same, premultiplied linear light, with a full mip chain.
    texture: wgpu::Texture,
    group: wgpu::BindGroup,
}

struct Pipelines {
    bg: wgpu::RenderPipeline,
    front: wgpu::RenderPipeline,
    back: wgpu::RenderPipeline,
    wall: wgpu::RenderPipeline,
    accum: wgpu::RenderPipeline,
    prefilter: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    fin: wgpu::RenderPipeline,
    floor: wgpu::RenderPipeline,
    linearize: wgpu::RenderPipeline,
    mip: wgpu::RenderPipeline,
    dof: wgpu::RenderPipeline,
    /// Ambient occlusion at half resolution, then upsampled onto the frame.
    gtao: wgpu::RenderPipeline,
    ao_apply: wgpu::RenderPipeline,
    /// `fs_final` into [`Stage::draw`]'s target format.
    present: wgpu::RenderPipeline,
    mesh: wgpu::RenderPipeline,
    /// Depth only, into a shadow map: faces cut out by their alpha, and
    /// walls and meshes solid.
    shadow_face: wgpu::RenderPipeline,
    shadow_solid: wgpu::RenderPipeline,
    /// The opaque frame and its distances into the chain's first level.
    chain: wgpu::RenderPipeline,
    /// Screen-space reflection, added onto the frame.
    ssr: wgpu::RenderPipeline,
    /// A premultiplied layer over the frame (the glass).
    over: wgpu::RenderPipeline,
    glass_face: wgpu::RenderPipeline,
    glass_solid: wgpu::RenderPipeline,
}

/// A slab's walls, uploaded: kept while a plane of the same size, depth
/// and outline is drawn.
struct Walls {
    size: [f32; 2],
    depth: f32,
    outline: Option<Arc<Path>>,
    buf: Option<(wgpu::Buffer, u32)>,
    used: bool,
}

struct Mesh {
    vertices: wgpu::Buffer,
    uvs: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    /// Local bounds.
    min: [f32; 3],
    max: [f32; 3],
}

/// The GPU, its targets and the layers painted onto it.
pub struct Stage {
    device: wgpu::Device,
    queue: wgpu::Queue,
    width: u32,
    height: u32,
    globals: wgpu::Buffer,
    draws: wgpu::Buffer,
    post: wgpu::Buffer,
    /// The globals and the shot's environment; one per environment, and
    /// one with none.
    group0: wgpu::BindGroup,
    l0: wgpu::BindGroupLayout,
    env_sampler: wgpu::Sampler,
    lut: wgpu::TextureView,
    no_env: wgpu::BindGroup,
    envs: HashMap<String, (wgpu::BindGroup, [[f32; 3]; 9])>,
    group1: wgpu::BindGroup,
    tex_layout: wgpu::BindGroupLayout,
    layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
    pipes: Pipelines,
    msaa: wgpu::TextureView,
    /// Eye distance and normal per pixel, multisampled and resolved, for
    /// depth of field, occlusion and screen-space reflection.
    msaa_dist: wgpu::TextureView,
    dist: wgpu::TextureView,
    /// Each pixel's reflection weight and roughness, for SSR.
    msaa_spec: wgpu::TextureView,
    spec: wgpu::TextureView,
    /// The opaque frame, colour and eye distance, mip-chained: what SSR
    /// and glass look up.
    chain: wgpu::Texture,
    dof: wgpu::TextureView,
    depth: wgpu::TextureView,
    hdr: wgpu::TextureView,
    /// Where the occlusion pass writes the frame; then it and `hdr` swap.
    lit: wgpu::TextureView,
    /// Half-resolution occlusion, and the eye distance it was found at.
    ao: wgpu::TextureView,
    accum: wgpu::TextureView,
    bloom: Vec<wgpu::TextureView>,
    layers: HashMap<String, Layer>,
    meshes: HashMap<String, Mesh>,
    /// Models' images ([`Stage::texture`]), the white and flat-normal
    /// stand-ins for a map a model has not, the repeating sampler they
    /// take, and a bind group per set of maps in use.
    images: HashMap<String, wgpu::TextureView>,
    blank: [wgpu::TextureView; 2],
    repeat: wgpu::Sampler,
    map_groups: HashMap<[Option<String>; 3], wgpu::BindGroup>,
    /// Extruded planes' walls from the last subframe.
    walls: Vec<Walls>,
    /// How many wall meshes were ever built (the cache's test reads it).
    walls_built: u64,
    /// One Vello pipeline set for every layer, resized to each in turn;
    /// made by the first [`Stage::layer`] (a caller painting its own
    /// layers through [`Stage::layer_target`] never needs it).
    renderer: Option<GpuRenderer>,
    /// The Rgba32Float frame and its readback, made by the first
    /// [`Stage::render`] ([`Stage::draw`] renders into the caller's view).
    readout: Option<(wgpu::Texture, wgpu::Buffer)>,
    background_src: String,
    present_format: wgpu::TextureFormat,
    shadow_layout: wgpu::BindGroupLayout,
    shadow_sampler: wgpu::Sampler,
    /// The shadow maps, the point lights' cube faces (made by the first
    /// point light that casts a shadow) and their bind group, made by the
    /// first lit shot that casts a shadow.
    shadows: Option<Shadows>,
    /// Bound where no shadow map is (or while one is being drawn).
    no_shadows: wgpu::BindGroup,
    /// Set by the device-lost callback of a device [`Stage::new`] opened.
    lost: Arc<AtomicBool>,
    /// The first GPU error no scope caught since the last call, which
    /// wgpu would otherwise panic on. Only on a device the stage opened.
    uncaptured: Arc<Mutex<Option<String>>>,
}

fn target(
    device: &wgpu::Device,
    w: u32,
    h: u32,
    format: wgpu::TextureFormat,
    samples: u32,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mui-stage target"),
        size: wgpu::Extent3d {
            width: w.max(1),
            height: h.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}
/// A model's image, uploaded (no mips).
fn image(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    rgba: &[u8],
    [w, h]: [u32; 2],
    srgb: bool,
) -> wgpu::Texture {
    use wgpu::util::DeviceExt;
    device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("mui-stage model map"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: if srgb {
                wgpu::TextureFormat::Rgba8UnormSrgb
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            },
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        rgba,
    )
}

fn view(t: &wgpu::Texture) -> wgpu::TextureView {
    t.create_view(&wgpu::TextureViewDescriptor::default())
}
const RT: wgpu::TextureUsages =
    wgpu::TextureUsages::RENDER_ATTACHMENT.union(wgpu::TextureUsages::TEXTURE_BINDING);

impl Stage {
    /// A headless stage `width`x`height` pixels. How many pixels a world
    /// unit covers is the camera's business: [`Camera::front`] at half the
    /// pixel height puts two pixels on each unit.
    pub fn new(width: u32, height: u32) -> Result<Self, Error> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(gpu)?;
        // A bare GL or software adapter may lack these; say so instead of
        // failing validation halfway through a take.
        let hdr = adapter.get_texture_format_features(HDR);
        let out = adapter.get_texture_format_features(OUT);
        if !hdr.flags.sample_count_supported(SAMPLES)
            || !hdr.allowed_usages.contains(RT)
            || !out
                .allowed_usages
                .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
        {
            return Err(Error::Gpu(format!(
                "{} cannot render the stage: needs {SAMPLES}x MSAA {HDR:?} and a {OUT:?} target",
                adapter.get_info().name
            )));
        }
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(gpu)?;
        // The device is the stage's own, so loss and stray errors are too:
        // reported by the next call as an `Error`, not a panic.
        let stage = Self::with_device(device, queue, width, height)?;
        let flag = Arc::clone(&stage.lost);
        stage
            .device
            .set_device_lost_callback(move |_, _| flag.store(true, Ordering::Release));
        let slot = Arc::clone(&stage.uncaptured);
        stage.device.on_uncaptured_error(Arc::new(move |e| {
            let mut slot = slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            slot.get_or_insert_with(|| e.to_string());
        }));
        Ok(stage)
    }

    /// A lost device or an uncaught GPU error since the last call, as an
    /// `Error`. Polls first: wgpu reports loss from a poll.
    fn fault(&self) -> Result<(), Error> {
        let _ = self.device.poll(wgpu::PollType::Poll);
        if self.lost.load(Ordering::Acquire) {
            return Err(Error::DeviceLost);
        }
        let mut slot = self
            .uncaptured
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        slot.take().map_or(Ok(()), |e| Err(Error::Gpu(e)))
    }

    /// On a device the host already has.
    pub fn with_device(
        device: wgpu::Device,
        queue: wgpu::Queue,
        width: u32,
        height: u32,
    ) -> Result<Self, Error> {
        let uniform = |size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mui-stage uniform"),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let globals = uniform(GLOBALS as u64 * 4);
        let draws = uniform(SLOT * SLOTS as u64);
        let post = uniform(64);
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty,
            count: None,
        };
        let buffer = |dynamic| wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: None,
        };
        let texture = wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let filtering = wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering);
        let l0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                entry(0, buffer(false)),
                entry(1, texture),
                entry(2, filtering),
                entry(3, texture),
            ],
        });
        let l1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[entry(0, buffer(true))],
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                entry(0, texture),
                entry(
                    1,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                entry(2, buffer(false)),
                entry(3, texture),
                entry(4, texture),
            ],
        });
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                ),
                entry(
                    1,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                ),
                entry(
                    2,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                ),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[
                Some(&l0),
                Some(&l1),
                Some(&tex_layout),
                Some(&shadow_layout),
            ],
            immediate_size: 0,
        });
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let none = depth_array(&device, 1, 1);
        let no_shadows = shadow_group(&device, &shadow_layout, &shadow_sampler, &none, &none);
        // The environment wraps round in u and stops at the poles.
        let env_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let lut = env::brdf_lut()
            .into_iter()
            .map(|[a, b]| [a, b, 0., 1.])
            .collect::<Vec<_>>();
        let lut = view(&float_texture(&device, &queue, [env::LUT; 2], 1, &[&lut]));
        let black = view(&float_texture(&device, &queue, [1; 2], 1, &[&[[0.; 4]]]));
        let no_env = env_group(&device, &l0, &globals, &black, &env_sampler, &lut);
        let group0 = no_env.clone();
        let group1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &l1,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &draws,
                    offset: 0,
                    size: wgpu::BufferSize::new(DRAW),
                }),
            }],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 16,
            ..Default::default()
        });
        let repeat = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let blank = [[255, 255, 255, 255], [128, 128, 255, 255]]
            .map(|px| view(&image(&device, &queue, &px, [1, 1], false)));
        let present_format = wgpu::TextureFormat::Rgba8Unorm;
        let pipes = pipelines(&device, &layout, DEFAULT_BACKGROUND, present_format)?;

        let msaa = view(&target(
            &device,
            width,
            height,
            HDR,
            SAMPLES,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        ));
        let depth = view(&target(
            &device,
            width,
            height,
            DEPTH,
            SAMPLES,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        ));
        let msaa_dist = view(&target(
            &device,
            width,
            height,
            HDR,
            SAMPLES,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        ));
        let dist = view(&target(&device, width, height, HDR, 1, RT));
        let msaa_spec = view(&target(
            &device,
            width,
            height,
            HDR,
            SAMPLES,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        ));
        let spec = view(&target(&device, width, height, HDR, 1, RT));
        let chain = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mui-stage chain"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            }
            .max_mips(wgpu::TextureDimension::D2),
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR,
            usage: RT,
            view_formats: &[],
        });
        let dof = view(&target(&device, width, height, HDR, 1, RT));
        let hdr = view(&target(&device, width, height, HDR, 1, RT));
        let lit = view(&target(&device, width, height, HDR, 1, RT));
        let ao = view(&target(
            &device,
            width.div_ceil(2),
            height.div_ceil(2),
            HDR,
            1,
            RT,
        ));
        let accum = view(&target(&device, width, height, HDR, 1, RT));
        let bloom = (1..=BLOOM_LEVELS as u32)
            .map(|i| view(&target(&device, width >> i, height >> i, HDR, 1, RT)))
            .collect();
        Ok(Self {
            device,
            queue,
            width,
            height,
            globals,
            draws,
            post,
            group0,
            l0,
            env_sampler,
            lut,
            no_env,
            envs: HashMap::new(),
            group1,
            tex_layout,
            layout,
            sampler,
            pipes,
            msaa,
            msaa_dist,
            dist,
            msaa_spec,
            spec,
            chain,
            dof,
            depth,
            hdr,
            lit,
            ao,
            accum,
            bloom,
            layers: HashMap::new(),
            meshes: HashMap::new(),
            images: HashMap::new(),
            blank,
            repeat,
            map_groups: HashMap::new(),
            walls: Vec::new(),
            walls_built: 0,
            renderer: None,
            readout: None,
            background_src: DEFAULT_BACKGROUND.into(),
            present_format,
            shadow_layout,
            shadow_sampler,
            shadows: None,
            no_shadows,
            lost: Arc::default(),
            uncaptured: Arc::default(),
        })
    }

    /// Replace the background with WGSL defining
    /// `fn background(uv: vec2f, t: f32) -> vec3f` (linear light; `uv` is
    /// 0..1 with y down; `noise`, `fbm` and `hash2` are in scope). A shader
    /// that does not compile is an error and leaves the old one in place.
    pub fn background(&mut self, wgsl: &str) -> Result<(), Error> {
        self.pipes = pipelines(&self.device, &self.layout, wgsl, self.present_format)?;
        self.background_src = wgsl.into();
        Ok(())
    }

    /// Upload a triangle mesh as `id`: per vertex a position and a normal
    /// (y up, logical units), three indices per triangle.
    pub fn mesh(&mut self, id: &str, vertices: &[[f32; 6]], indices: &[u32]) {
        self.mesh_uv(id, vertices, &[], indices);
    }

    /// [`Stage::mesh`] with a texture coordinate per vertex, which a
    /// [`Model`]'s maps are sampled at (none: all at 0, 0).
    pub fn mesh_uv(&mut self, id: &str, vertices: &[[f32; 6]], uvs: &[[f32; 2]], indices: &[u32]) {
        let init = |contents: &[u8], usage| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mui-stage mesh"),
                    contents,
                    usage,
                })
        };
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        for v in vertices {
            for i in 0..3 {
                min[i] = min[i].min(v[i]);
                max[i] = max[i].max(v[i]);
            }
        }
        let zeros;
        let uvs = if uvs.len() == vertices.len() {
            uvs
        } else {
            zeros = vec![[0f32; 2]; vertices.len()];
            &zeros
        };
        let mesh = Mesh {
            vertices: init(bytemuck::cast_slice(vertices), wgpu::BufferUsages::VERTEX),
            uvs: init(bytemuck::cast_slice(uvs), wgpu::BufferUsages::VERTEX),
            indices: init(bytemuck::cast_slice(indices), wgpu::BufferUsages::INDEX),
            count: indices.len() as u32,
            min,
            max,
        };
        self.meshes.insert(id.into(), mesh);
    }

    /// Keep straight RGBA `rgba`, `size` pixels, as image `id` for a
    /// [`Model`]'s maps, replacing any before; `srgb` for a colour (base
    /// colour), not for data (normals, metallic-roughness).
    pub fn texture(&mut self, id: &str, rgba: &[u8], size: [u32; 2], srgb: bool) {
        let t = image(&self.device, &self.queue, rgba, size, srgb);
        self.images.insert(id.into(), view(&t));
        self.map_groups
            .retain(|k, _| !k.iter().flatten().any(|m| m == id));
    }

    /// Whether [`Stage::texture`] uploaded `id`.
    pub fn has_texture(&self, id: &str) -> bool {
        self.images.contains_key(id)
    }

    /// The bind group of a model's maps, a stand-in for each it has not.
    fn maps_group(&mut self, maps: &[Option<String>; 3]) -> wgpu::BindGroup {
        if let Some(g) = self.map_groups.get(maps) {
            return g.clone();
        }
        let pick = |i: usize, blank: usize| {
            maps[i]
                .as_ref()
                .and_then(|m| self.images.get(m))
                .unwrap_or(&self.blank[blank])
        };
        let (base, normal, mr) = (pick(0, 0), pick(1, 1), pick(2, 0));
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mui-stage model maps"),
            layout: &self.tex_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(base),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.repeat),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.post.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(normal),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(mr),
                },
            ],
        });
        self.map_groups.insert(maps.clone(), group.clone());
        group
    }

    /// Whether [`Stage::mesh`] uploaded `id`.
    pub fn has_mesh(&self, id: &str) -> bool {
        self.meshes.contains_key(id)
    }

    /// Paint `scene` into the texture named `id` at `supersample` pixels per
    /// logical unit. Call it every frame the UI changes; the texture is
    /// kept while the size is.
    pub fn layer(
        &mut self,
        id: &str,
        scene: &ResolvedScene,
        size: Size,
        supersample: f64,
    ) -> Result<(), Error> {
        self.fault()?;
        let (w, h) = (
            (size.width * supersample).ceil().clamp(1., 8192.) as u32,
            (size.height * supersample).ceil().clamp(1., 8192.) as u32,
        );
        let target =
            view(&self.layer_target(id, [w, h], wgpu::TextureFormat::Rgba8Unorm, u32::MAX)?);
        let renderer = match &mut self.renderer {
            Some(r) => r,
            none => none.insert(pollster::block_on(GpuRenderer::new(
                &self.device,
                &self.queue,
                wgpu::TextureFormat::Rgba8Unorm,
                [1, 1],
                Budget::default(),
            ))?),
        };
        // Shared across layers: switching layers re-encodes (damage against
        // the last layer drawn), which a layer call does anyway.
        renderer.resize([w, h])?;
        renderer.render(scene, Affine::scale(supersample), &target)?;
        self.layer_done(id)
    }

    /// The texture of layer `id`, `size` pixels in `format` (a non-sRGB
    /// 8-bit one), for the caller to paint premultiplied sRGB into, as
    /// Vello does; then [`Stage::layer_done`]. It is kept while the size and
    /// format are. `mips` caps the mip chain: an atlas of many layers wants
    /// few, so a far-off one does not bleed into its neighbours. It can be
    /// copied to and from, so a caller can post-process part of it.
    pub fn layer_target(
        &mut self,
        id: &str,
        size: [u32; 2],
        format: wgpu::TextureFormat,
        mips: u32,
    ) -> Result<wgpu::Texture, Error> {
        let [w, h] = size.map(|v| v.clamp(1, 8192));
        let stale = self.layers.get(id).is_none_or(|l| {
            let s = l.raw.size();
            (s.width, s.height, l.raw.format()) != (w, h, format)
        });
        if stale {
            let size = wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            };
            let raw = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("mui-stage layer (vello)"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: RT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            // A slab seen far off or edge-on samples a mip, not a shimmer.
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("mui-stage layer"),
                size,
                mip_level_count: size.max_mips(wgpu::TextureDimension::D2).min(mips.max(1)),
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: HDR,
                usage: RT,
                view_formats: &[],
            });
            let group = self.tex_group(&view(&texture), &view(&texture));
            self.layers.insert(
                id.into(),
                Layer {
                    raw,
                    texture,
                    group,
                },
            );
        }
        Ok(self.layers[id].raw.clone())
    }

    /// Linearise and mipmap what was painted into [`Stage::layer_target`].
    pub fn layer_done(&mut self, id: &str) -> Result<(), Error> {
        let layer = self
            .layers
            .get(id)
            .ok_or_else(|| Error::MissingLayer(id.into()))?;
        let target = view(&layer.raw);
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        // Linearise into mip 0, then halve down the chain.
        let level = |i: u32| {
            layer.texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: i,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let pass = |enc: &mut wgpu::CommandEncoder,
                    pipe: &wgpu::RenderPipeline,
                    to: &wgpu::TextureView,
                    group: &wgpu::BindGroup| {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(attach(to, Some(wgpu::Color::TRANSPARENT)))],
                ..Default::default()
            });
            self.full(&mut rp, pipe, group);
        };
        let raw = self.tex_group(&target, &target);
        pass(&mut enc, &self.pipes.linearize, &level(0), &raw);
        for i in 1..layer.texture.mip_level_count() {
            let from = level(i - 1);
            pass(
                &mut enc,
                &self.pipes.mip,
                &level(i),
                &self.tex_group(&from, &from),
            );
        }
        self.queue.submit([enc.finish()]);
        Ok(())
    }

    /// `text` as a layer named `id`: the run's outline filled with `color`,
    /// and a plane whose walls are the glyphs' own contours, so `.depth(..)`
    /// extrudes real 3D letters. The plane is the run's ink box plus `pad`
    /// on every side, centred on its origin.
    #[expect(
        clippy::too_many_arguments,
        reason = "an options struct comes with the media/ split (docs/DSL-V2.md)"
    )]
    pub fn text_layer(
        &mut self,
        id: &str,
        fonts: &[mui_scene::Font],
        text: &str,
        size_px: f64,
        color: mui_scene::Color,
        pad: f64,
        supersample: f64,
    ) -> Result<Plane, Error> {
        let run = mui_text::text_run(fonts, text, size_px, &[], 0.05)?;
        let path = run
            .path
            .rigid_transform(mui_geometry::Vec2::new(pad, pad + run.ascent), 0.)?;
        let size = Size::new(run.advance + 2. * pad, run.ascent + run.descent + 2. * pad);
        let shape = Arc::new(path);
        let spec = filled(size, shape.clone(), color);
        self.layer(id, &mui_scene::resolve(&spec)?, size, supersample)?;
        Ok(Plane::new(id, size.width as f32, size.height as f32).outline(shape))
    }

    fn tex_group(&self, tex: &wgpu::TextureView, bloom: &wgpu::TextureView) -> wgpu::BindGroup {
        self.tex_group3(tex, bloom, bloom)
    }
    /// With a third texture, `aux2`.
    fn tex_group3(
        &self,
        tex: &wgpu::TextureView,
        bloom: &wgpu::TextureView,
        aux2: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.tex_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(tex),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.post.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(bloom),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(aux2),
                },
            ],
        })
    }

    /// Prefilter `image` (see [`env`]) and keep it as environment `id`,
    /// replacing any before. The GGX chain is convolved on the GPU; the
    /// CPU only resamples the image to the chain's size and sums its SH.
    pub fn environment(&mut self, id: &str, image: &EnvImage) {
        let (tex, sh) = self.prefilter(image);
        let group = env_group(
            &self.device,
            &self.l0,
            &self.globals,
            &view(&tex),
            &self.env_sampler,
            &self.lut,
        );
        self.envs.insert(id.into(), (group, sh));
    }

    /// `image`'s specular chain as a texture, and its irradiance SH.
    fn prefilter(&self, image: &EnvImage) -> (wgpu::Texture, [[f32; 3]; 9]) {
        let base = image.base();
        let sh = env::sh_of(&base);
        let px: Vec<[f32; 4]> = base.rgb.iter().map(|&[r, g, b]| [r, g, b, 1.]).collect();
        let (w, h) = (base.width, base.height);
        // The source pyramid, halved down to 4 texels across, and the chain.
        let (device, queue) = (&self.device, &self.queue);
        let pyramid = float_texture(device, queue, [w, h], (w / 4).ilog2() + 1, &[&px]);
        let chain = float_texture(device, queue, [w, h], env::LEVELS, &[]);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("mui-stage env"),
                source: wgpu::ShaderSource::Wgsl(include_str!("env.wgsl").into()),
            });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty,
            count: None,
        };
        let group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    1,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                entry(
                    2,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&group_layout)],
            immediate_size: 0,
        });
        let make = |entry: &str| {
            self.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("vs"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(entry),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(HDR.into())],
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                })
        };
        let (half, convolve) = (make("fs_half"), make("fs_convolve"));
        let mip = |t: &wgpu::Texture, m: u32, count: Option<u32>| {
            t.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: m,
                mip_level_count: count,
                ..Default::default()
            })
        };
        let texel = 4. * std::f32::consts::PI / (w * h) as f32;
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let mut pass = |pipe: &wgpu::RenderPipeline,
                        from: &wgpu::TextureView,
                        to: &wgpu::TextureView,
                        rough: f32| {
            let uniform = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mui-stage env level"),
                    contents: bytemuck::cast_slice(&[rough, texel, 0., 0.]),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(from),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.env_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform.as_entire_binding(),
                    },
                ],
            });
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(attach(to, None))],
                ..Default::default()
            });
            rp.set_pipeline(pipe);
            rp.set_bind_group(0, &group, &[]);
            rp.draw(0..3, 0..1);
        };
        for m in 1..pyramid.mip_level_count() {
            pass(
                &half,
                &mip(&pyramid, m - 1, Some(1)),
                &mip(&pyramid, m, Some(1)),
                0.,
            );
        }
        // Level 0 is the image itself; the rest, rougher and rougher.
        let all = mip(&pyramid, 0, None);
        for m in 1..env::LEVELS {
            pass(
                &convolve,
                &all,
                &mip(&chain, m, Some(1)),
                env::roughness_of_mip(m),
            );
        }
        enc.copy_texture_to_texture(
            pyramid.as_image_copy(),
            chain.as_image_copy(),
            pyramid.size(),
        );
        self.queue.submit([enc.finish()]);
        (chain, sh)
    }

    /// Whether [`Stage::environment`] made `id`.
    pub fn has_environment(&self, id: &str) -> bool {
        self.envs.contains_key(id)
    }

    /// The frame at `t` seconds: `subframes` shots spread evenly over
    /// `shutter` seconds ending at `t` (1 is no blur; a 180-degree shutter
    /// at 60 fps is `1. / 120.`), averaged, then post-processed once.
    pub fn render(
        &mut self,
        t: f64,
        shutter: f64,
        subframes: u32,
        shot: &dyn Fn(f64) -> Shot,
    ) -> Result<Frame, Error> {
        self.accumulate(t, shutter, subframes, false, shot)
    }

    /// [`Stage::render`] with every subframe also a beauty sample
    /// ([`Shot::sample`]): `samples` shots, each at its own time across the
    /// shutter and its own pixel, lens and light positions, averaged. One
    /// set of draws gives motion blur, antialiasing, soft shadows and depth
    /// of field together.
    pub fn beauty(
        &mut self,
        t: f64,
        shutter: f64,
        samples: u32,
        shot: &dyn Fn(f64) -> Shot,
    ) -> Result<Frame, Error> {
        self.accumulate(t, shutter, samples, true, shot)
    }

    fn accumulate(
        &mut self,
        t: f64,
        shutter: f64,
        subframes: u32,
        beauty: bool,
        shot: &dyn Fn(f64) -> Shot,
    ) -> Result<Frame, Error> {
        self.fault()?;
        let n = subframes.max(1);
        let aspect = self.width as f32 / self.height as f32;
        let mut last = None;
        for i in 0..n {
            // Centred on the frame time's trailing interval: subframe 0 of 1
            // is exactly `t`.
            let at = t - shutter * f64::from(n - 1 - i) / f64::from(n);
            let mut s = shot(at);
            if beauty {
                s.sample = Some(i);
            }
            self.scene(&s, at, aspect)?;
            let mut enc = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            {
                let hdr = self.tex_group(&self.hdr, &self.hdr);
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(attach(
                        &self.accum,
                        (i == 0).then_some(wgpu::Color::TRANSPARENT),
                    ))],
                    ..Default::default()
                });
                let w = 1. / f64::from(n);
                pass.set_blend_constant(wgpu::Color {
                    r: w,
                    g: w,
                    b: w,
                    a: w,
                });
                self.full(&mut pass, &self.pipes.accum, &hdr);
            }
            self.queue.submit([enc.finish()]);
            last = Some(post_of(&s));
        }
        let (width, height) = (self.width, self.height);
        let (out, readback) = self.readout.get_or_insert_with(|| {
            let out = target(
                &self.device,
                width,
                height,
                OUT,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            );
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mui-stage readback"),
                size: u64::from((width * 16).next_multiple_of(256)) * u64::from(height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            (out, readback)
        });
        let (out, readback) = (out.clone(), readback.clone());
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.post(
            &mut enc,
            last.expect("n >= 1"),
            t,
            &self.accum,
            &view(&out),
            &self.pipes.fin,
        );
        let row = (self.width * 16).next_multiple_of(256);
        enc.copy_texture_to_buffer(
            out.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            out.size(),
        );
        self.queue.submit([enc.finish()]);
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let waited = self.device.poll(wgpu::PollType::wait_indefinitely());
        self.fault()?;
        waited.map_err(gpu)?;
        let stride = self.width as usize * 16;
        let mut rgba = Vec::with_capacity(self.width as usize * self.height as usize * 4);
        for line in slice
            .get_mapped_range()
            .map_err(gpu)?
            .chunks_exact(row as usize)
        {
            rgba.extend_from_slice(bytemuck::cast_slice::<u8, f32>(&line[..stride]));
        }
        readback.unmap();
        Ok(Frame {
            width: self.width,
            height: self.height,
            rgba,
        })
    }

    /// One instant of `shot` straight into `target`, a `format` view the
    /// stage's size (a non-sRGB 8-bit format gets display-encoded bytes,
    /// alpha 1): no subframes and no readback, so the caller owns the
    /// shutter. Post runs as for [`Stage::render`].
    pub fn draw(
        &mut self,
        shot: &Shot,
        t: f64,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) -> Result<(), Error> {
        self.fault()?;
        if format != self.present_format {
            self.pipes = pipelines(&self.device, &self.layout, &self.background_src, format)?;
            self.present_format = format;
        }
        let aspect = self.width as f32 / self.height as f32;
        self.scene(shot, t, aspect)?;
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.post(
            &mut enc,
            post_of(shot),
            t,
            &self.hdr,
            target,
            &self.pipes.present,
        );
        self.queue.submit([enc.finish()]);
        Ok(())
    }

    /// The shadow maps, made on first use, and with `cube` the point
    /// lights' faces too.
    fn shadow_maps(&mut self, cube: bool) -> &Shadows {
        let (device, layout, sampler) = (&self.device, &self.shadow_layout, &self.shadow_sampler);
        let s = self.shadows.get_or_insert_with(|| {
            let flat = depth_array(device, SHADOW, CONTACT_LAYER + 1);
            let none = depth_array(device, 1, 1);
            let group = shadow_group(device, layout, sampler, &flat, &none);
            Shadows {
                flat,
                cube: None,
                group,
            }
        });
        if cube && s.cube.is_none() {
            let c = depth_array(device, CUBE, 6 * MAX_LIGHTS as u32);
            s.group = shadow_group(device, layout, sampler, &s.flat, &c);
            s.cube = Some(c);
        }
        s
    }

    /// One subframe into `self.hdr`.
    fn scene(&mut self, s: &Shot, t: f64, aspect: f32) -> Result<(), Error> {
        let q = s.sample.map(sample);
        let (camera, shift) = lens(s, q, self.height as f32, aspect);
        let vp = Mat4::translate([shift[0], shift[1], 0.]) * camera.view_proj(aspect);
        let planes: Vec<&Plane> = s
            .planes
            .iter()
            .filter(|p| p.opacity > 0.)
            .take(MAX_PLANES)
            .collect();
        if let Some(p) = planes.iter().find(|p| !self.layers.contains_key(&p.layer)) {
            return Err(Error::MissingLayer(p.layer.clone()));
        }
        let models: Vec<&Model> = s
            .models
            .iter()
            .filter(|m| m.color[3] > 0.)
            .take(MAX_MODELS)
            .collect();
        if let Some(m) = models.iter().find(|m| !self.meshes.contains_key(&m.mesh)) {
            return Err(Error::MissingMesh(m.mesh.clone()));
        }

        let mut g = [0f32; GLOBALS];
        g[..16].copy_from_slice(&vp.0);
        g[16..19].copy_from_slice(&camera.eye);
        g[20] = t as f32;
        g[21] = self.width as f32;
        g[22] = self.height as f32;
        if let Some(f) = s.floor {
            g[24..27].copy_from_slice(&f.color);
        }
        if let Some(c) = s.clear {
            g[28..31].copy_from_slice(&c);
            g[31] = 1.;
        }
        if let Some(f) = s.fog {
            g[32..35].copy_from_slice(&f.color);
            g[35] = 1.;
            g[36] = f.near;
            g[37] = f.far.max(f.near + 1e-3);
        }
        // Lights, and a shadow map layer for each that casts one.
        let bounds = caster_bounds(&planes, &models, &self.meshes);
        let mut maps = Vec::new();
        let mut cubes = Vec::new();
        if !s.lights.is_empty() {
            g[43] = 1.;
        }
        let mut slot = 0;
        for l in &s.lights {
            let l = &q.map_or(*l, |q| area(l, &q));
            if l.kind == LightKind::Ambient {
                for i in 0..3 {
                    g[40 + i] += l.color[i];
                }
                continue;
            }
            if slot == MAX_LIGHTS {
                continue;
            }
            let o = 44 + slot * 16;
            let dir = normalize(l.direction);
            let outer = l.cone.clamp(0.1, 89.).to_radians();
            let inner = outer * (1. - l.feather.clamp(0., 1.));
            g[o..o + 3].copy_from_slice(&l.position);
            g[o + 3] = match l.kind {
                LightKind::Directional => 1.,
                LightKind::Point => 2.,
                _ => 3.,
            };
            g[o + 4..o + 7].copy_from_slice(&dir);
            g[o + 7] = outer.cos();
            g[o + 8..o + 11].copy_from_slice(&l.color);
            g[o + 11] = inner.cos();
            g[o + 12] = l.range.max(0.);
            g[o + 13] = -1.;
            g[o + 14] = l.softness.max(0.);
            if let (true, Some((lo, hi))) = (l.shadows, bounds) {
                if l.kind == LightKind::Point {
                    // Six faces around it, as far as the light or the
                    // casters reach.
                    let c: [f32; 3] = std::array::from_fn(|i| (lo[i] + hi[i]) * 0.5);
                    let r = (0..3).map(|i| (hi[i] - lo[i]).powi(2)).sum::<f32>().sqrt() * 0.5;
                    let p = l.position;
                    let reach = (0..3).map(|i| (p[i] - c[i]).powi(2)).sum::<f32>().sqrt() + r;
                    g[o + 7] = if l.range > 0. { l.range } else { reach }.max(8.);
                    g[o + 13] = (CUBE_BASE as usize + slot) as f32;
                    g[o + 15] = 0.;
                    cubes.push(slot as u32);
                } else {
                    let (m, bias) = shadow_view(l, dir, (lo, hi));
                    g[108 + slot * 16..][..16].copy_from_slice(&m.0);
                    g[o + 13] = slot as f32;
                    g[o + 15] = bias;
                    maps.push(slot as u32);
                }
            }
            slot += 1;
        }
        if let (Some(f), Some((lo, hi))) = (s.floor, bounds)
            && f.contact > 0.
        {
            // Straight down on what stands over the floor: depth 0 is
            // `contact_height` above it, 1 the floor itself.
            let h = f.contact_height.max(1.);
            let c = [(lo[0] + hi[0]) * 0.5, (lo[2] + hi[2]) * 0.5];
            let r = (hi[0] - lo[0]).max(hi[2] - lo[2]) * 0.5 + 64.;
            let m = Mat4::orthographic(r, r, 0., h)
                * Mat4::look_at([c[0], f.y + h, c[1]], [c[0], f.y, c[1]], [0., 0., -1.]);
            g[108 + 4 * 16..][..16].copy_from_slice(&m.0);
            g[188] = f.contact.min(1.);
            maps.push(CONTACT_LAYER);
        }
        // The view's axes and half-extents, for rays from the eye.
        let [right, up, fwd] = camera.basis();
        let ty = (camera.fov.to_radians() * 0.5).tan();
        g[200..203].copy_from_slice(&right);
        g[203] = ty * aspect;
        g[204..207].copy_from_slice(&up);
        g[207] = ty;
        g[208..211].copy_from_slice(&fwd);
        self.group0 = self.no_env.clone();
        if let Some(e) = &s.environment {
            if e.map == STUDIO && !self.envs.contains_key(STUDIO) {
                self.environment(STUDIO, &EnvImage::studio());
            }
            let (group, sh) = self
                .envs
                .get(&e.map)
                .ok_or_else(|| Error::MissingEnvironment(e.map.clone()))?;
            let (sin, cos) = e.rotation.to_radians().sin_cos();
            g[192..196].copy_from_slice(&[e.intensity.max(0.), cos, sin, 1.]);
            g[196] = f32::from(u8::from(e.background));
            g[197] = (env::LEVELS - 1) as f32;
            for (k, c) in sh.iter().enumerate() {
                g[216 + 4 * k..][..3].copy_from_slice(c);
            }
            // An environment lights the shot.
            g[43] = 1.;
            self.group0 = group.clone();
        }
        let ao = s.ao.filter(|a| a.strength > 0. && a.radius > 0.);
        if let Some(a) = ao {
            g[212..215].copy_from_slice(&[a.strength, a.radius, 1.]);
        }
        // The sample's shift, which eye rays undo, and the occlusion's turn.
        if let Some(q) = q {
            g[252..256].copy_from_slice(&[shift[0], shift[1], q[6], q[7]]);
            g[198] = 1.;
            // Roberts' R2: evenly spread in 2D for any count of samples.
            let i = s.sample.unwrap_or(0) as f32;
            g[38] = (0.5 + i * 0.754_877_7).fract();
            g[39] = (0.5 + i * 0.569_840_3).fract();
        }
        g[199] = (self.chain.mip_level_count() - 1) as f32;
        self.queue
            .write_buffer(&self.globals, 0, bytemuck::cast_slice(&g));

        let (n, k) = (planes.len(), planes.len() + models.len());
        let reflect = s.floor.filter(|f| f.reflect > 0.);
        // Slots: planes 0..n, models n..k, their reflections k..2k, the
        // floor at 2k.
        let mut slots = vec![0u8; SLOT as usize * SLOTS];
        let mut put = |slot: usize, model: Mat4, rows: [[f32; 4]; 7]| {
            let mut d = [0f32; 44];
            d[..16].copy_from_slice(&model.0);
            for (i, r) in rows.iter().enumerate() {
                d[16 + i * 4..][..4].copy_from_slice(r);
            }
            slots[slot * SLOT as usize..][..DRAW as usize]
                .copy_from_slice(bytemuck::cast_slice(&d));
        };
        // A material's rows: receives shadows, metallic, roughness; the
        // glass; the tint, and whether light leaves a slab as it came.
        let material = |m: &Material, receive: bool, thickness: f32, slab: bool| {
            [
                [
                    f32::from(u8::from(receive)),
                    m.metallic.clamp(0., 1.),
                    m.roughness.clamp(0.02, 1.),
                    0.,
                ],
                [
                    m.transmission.clamp(0., 1.),
                    m.ior.max(1.),
                    thickness.max(0.),
                    m.dispersion.max(0.),
                ],
                [
                    m.tint[0].clamp(0., 1.),
                    m.tint[1].clamp(0., 1.),
                    m.tint[2].clamp(0., 1.),
                    f32::from(u8::from(slab)),
                ],
            ]
        };
        let flip = reflect.map(|f| {
            let mut m = Mat4::IDENTITY;
            m.0[5] = -1.;
            (
                Mat4::translate([0., f.y, 0.]) * m * Mat4::translate([0., -f.y, 0.]),
                [f.y, f.reflect, f.falloff.max(1e-3), f.radius.max(1e-3)],
            )
        });
        let mut walls = Vec::new();
        for (i, p) in planes.iter().enumerate() {
            // In the plane's own units, as its walls: scaled with it.
            let thick = if p.material.thickness > 0. {
                p.material.thickness
            } else {
                p.depth
            } * p.scale.abs();
            let [a, b, c] = material(&p.material, p.receive, thick, true);
            let rows = |mirror| {
                [
                    [p.size[0], p.size[1], p.depth, p.glow],
                    [p.edge[0], p.edge[1], p.edge[2], p.opacity],
                    mirror,
                    p.uv,
                    a,
                    b,
                    c,
                ]
            };
            put(i, p.model(), rows([0.; 4]));
            if let Some((flip, mirror)) = flip {
                put(k + i, flip * p.model(), rows(mirror));
            }
            walls.push(if p.depth > 0. { self.walls_of(p) } else { None });
        }
        self.walls.retain_mut(|w| std::mem::take(&mut w.used));
        for (i, m) in models.iter().enumerate() {
            let [a, b, c] = material(&m.material, m.receive, m.material.thickness, false);
            // `size.x`: it has a normal map.
            let bumped = if m.maps[1]
                .as_ref()
                .is_some_and(|n| self.images.contains_key(n))
            {
                1.
            } else {
                0.
            };
            let rows = |mirror| {
                [
                    [bumped, 0., 0., 1.],
                    m.color,
                    mirror,
                    [0., 0., 1., 1.],
                    a,
                    b,
                    c,
                ]
            };
            put(n + i, m.transform, rows([0.; 4]));
            if let Some((flip, mirror)) = flip {
                put(k + n + i, flip * m.transform, rows(mirror));
            }
        }
        if let Some(f) = s.floor {
            put(
                2 * k,
                Mat4::IDENTITY,
                [
                    [f.radius.max(1e-3), 0., 0., 0.],
                    [f.color[0], f.color[1], f.color[2], 1.],
                    [f.y, 0., 0., 0.],
                    [0., 0., 1., 1.],
                    [1., 0., 0.5, 0.],
                    [0., 1.5, 0., 0.],
                    [1., 1., 1., 0.],
                ],
            );
        }
        self.queue.write_buffer(&self.draws, 0, &slots);

        // Passes that sample no layer still need a group 2: bind the
        // frame's own HDR input.
        let none = self.tex_group(&self.accum, &self.accum);
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        // Glass lets most light through: it casts no shadow map.
        let casts = |m: &Material| m.transmission < 0.5;
        if !maps.is_empty() || !cubes.is_empty() {
            let shadows = self.shadow_maps(!cubes.is_empty());
            let (flat, cube) = (shadows.flat.clone(), shadows.cube.clone());
            // Each map: its texture, its layer, and the instance index the
            // shaders read its view from (a cube face's past CUBE_BASE).
            let targets = maps
                .iter()
                .map(|&l| (&flat, l, l))
                .chain(cube.iter().flat_map(|c| {
                    cubes.iter().flat_map(move |&s| {
                        (0..6).map(move |f| (c, s * 6 + f, CUBE_BASE + s * 6 + f))
                    })
                }));
            for (tex, layer, k) in targets {
                let target = tex.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..Default::default()
                });
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &target,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    ..Default::default()
                });
                pass.set_bind_group(0, &self.group0, &[]);
                pass.set_bind_group(2, &none, &[]);
                pass.set_bind_group(3, &self.no_shadows, &[]);
                // The instance index picks the light's matrix.
                let inst = k..k + 1;
                for (i, p) in planes
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.cast && casts(&p.material))
                {
                    pass.set_bind_group(1, &self.group1, &[(i as u64 * SLOT) as u32]);
                    pass.set_bind_group(2, &self.layers[&p.layer].group, &[]);
                    pass.set_pipeline(&self.pipes.shadow_face);
                    pass.draw(0..6, inst.clone());
                    if let Some((buf, count)) = &walls[i] {
                        pass.set_pipeline(&self.pipes.shadow_solid);
                        pass.set_vertex_buffer(0, buf.slice(..));
                        pass.draw(0..*count, inst.clone());
                    }
                }
                pass.set_pipeline(&self.pipes.shadow_solid);
                for (i, m) in models
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.cast && casts(&m.material))
                {
                    let mesh = &self.meshes[&m.mesh];
                    pass.set_bind_group(1, &self.group1, &[((n + i) as u64 * SLOT) as u32]);
                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.count, 0, inst.clone());
                }
            }
        }

        // Opaque faces far to near by the depth of each slab's centre; glass
        // waits for its own pass, back to front, over the finished opaque
        // frame (and is not seen in the floor).
        let depth = |m: Mat4| {
            // Clip-space w of the local origin: its distance along the view.
            (vp * m).0[15]
        };
        let mut order: Vec<usize> = (0..n).filter(|&i| !planes[i].material.glass()).collect();
        order.sort_by(|&a, &b| depth(planes[b].model()).total_cmp(&depth(planes[a].model())));
        let opaque: Vec<usize> = (0..models.len())
            .filter(|&i| !models[i].material.glass())
            .collect();
        let mut glass: Vec<(f32, usize)> = (0..n)
            .filter(|&i| planes[i].material.glass())
            .map(|i| (depth(planes[i].model()), i))
            .chain(
                (0..models.len())
                    .filter(|&i| models[i].material.glass())
                    .map(|i| {
                        let mesh = &self.meshes[&models[i].mesh];
                        let c = std::array::from_fn(|a| (mesh.min[a] + mesh.max[a]) * 0.5);
                        (depth(models[i].transform * Mat4::translate(c)), n + i)
                    }),
            )
            .collect();
        // An opaque plane in front of glass it overlaps on screen (a label
        // on a frosted card) draws after that glass, in its pass: the glass
        // would otherwise see it, blurred, behind itself. ponytail: planes
        // only, by screen boxes; a model in front of glass still leaks.
        let bounds = |p: &Plane| {
            let m = vp * p.model();
            let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
            for [x, y] in [[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]] {
                let l = [x * p.size[0], y * p.size[1], 0.];
                let w = m.0[3] * l[0] + m.0[7] * l[1] + m.0[15];
                if w <= 0. {
                    return None;
                }
                let q = m.project(l);
                b = [
                    b[0].min(q[0]),
                    b[1].min(q[1]),
                    b[2].max(q[0]),
                    b[3].max(q[1]),
                ];
            }
            Some(b)
        };
        let panes: Vec<(f32, [f32; 4])> = glass
            .iter()
            .filter(|&&(_, i)| i < n)
            .filter_map(|&(d, i)| Some((d, bounds(planes[i])?)))
            .collect();
        if !panes.is_empty() {
            order.retain(|&i| {
                let d = depth(planes[i].model());
                let over = bounds(planes[i]).is_some_and(|b| {
                    panes.iter().any(|&(pd, g)| {
                        pd > d && b[0] < g[2] && g[0] < b[2] && b[1] < g[3] && g[1] < b[3]
                    })
                });
                if over {
                    glass.push((d, i));
                }
                !over
            });
        }
        glass.sort_by(|a, b| b.0.total_cmp(&a.0));
        let map_groups: Vec<wgpu::BindGroup> = opaque
            .iter()
            .map(|&i| self.maps_group(&models[i].maps))
            .collect();
        let draw = |pass: &mut wgpu::RenderPass<'_>, base: usize| {
            pass.set_pipeline(&self.pipes.mesh);
            for (&i, maps) in opaque.iter().zip(&map_groups) {
                let mesh = &self.meshes[&models[i].mesh];
                pass.set_bind_group(1, &self.group1, &[((base + n + i) as u64 * SLOT) as u32]);
                pass.set_bind_group(2, maps, &[]);
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_vertex_buffer(1, mesh.uvs.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.count, 0, 0..1);
            }
            for &i in &order {
                let Some((buf, count)) = &walls[i] else {
                    continue;
                };
                pass.set_bind_group(1, &self.group1, &[((base + i) as u64 * SLOT) as u32]);
                pass.set_pipeline(&self.pipes.wall);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..*count, 0..1);
            }
            for &i in &order {
                let p = planes[i];
                pass.set_bind_group(1, &self.group1, &[((base + i) as u64 * SLOT) as u32]);
                pass.set_bind_group(2, &self.layers[&p.layer].group, &[]);
                if p.depth > 0. {
                    pass.set_pipeline(&self.pipes.back);
                    pass.draw(0..6, 0..1);
                }
                pass.set_pipeline(&self.pipes.front);
                pass.draw(0..6, 0..1);
            }
        };
        let keep = !glass.is_empty();
        {
            let mut pass = self.scene_pass(&mut enc, &none, true, reflect.is_none(), keep);
            pass.set_pipeline(&self.pipes.bg);
            pass.draw(0..3, 0..1);
            if s.floor.is_some() {
                pass.set_bind_group(1, &self.group1, &[((2 * k) as u64 * SLOT) as u32]);
                pass.set_pipeline(&self.pipes.floor);
                pass.draw(0..6, 0..1);
            }
            if reflect.is_some() {
                // The mirror image sorts the other way round: far below is
                // far away.
                draw(&mut pass, k);
            } else {
                draw(&mut pass, 0);
            }
        }
        if reflect.is_some() {
            let mut pass = self.scene_pass(&mut enc, &none, false, true, keep);
            draw(&mut pass, 0);
        }
        if ao.is_some() {
            // Occlusion from the eye distances at half resolution, then a
            // depth-aware upsample that darkens the frame into `lit`.
            let group = self.tex_group(&self.dist, &self.dist);
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(attach(&self.ao, Some(wgpu::Color::WHITE)))],
                ..Default::default()
            });
            self.full(&mut rp, &self.pipes.gtao, &group);
            drop(rp);
            let group = self.tex_group3(&self.hdr, &self.ao, &self.dist);
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(attach(&self.lit, Some(wgpu::Color::BLACK)))],
                ..Default::default()
            });
            self.full(&mut rp, &self.pipes.ao_apply, &group);
            drop(rp);
            std::mem::swap(&mut self.hdr, &mut self.lit);
        }
        // Only a lit shot has any reflection to trace.
        let ssr = s.ssr && (!s.lights.is_empty() || s.environment.is_some());
        let chain = view(&self.chain);
        if ssr {
            self.build_chain(&mut enc);
            let group = self.tex_group3(&self.dist, &chain, &self.spec);
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(attach(&self.hdr, None))],
                ..Default::default()
            });
            self.full(&mut rp, &self.pipes.ssr, &group);
        }
        if !glass.is_empty() {
            self.build_chain(&mut enc);
            let groups: HashMap<&str, wgpu::BindGroup> = glass
                .iter()
                .filter(|(_, i)| *i < n)
                .map(|(_, i)| {
                    let l = planes[*i].layer.as_str();
                    (
                        l,
                        self.tex_group3(&view(&self.layers[l].texture), &chain, &chain),
                    )
                })
                .collect();
            let solid = self.tex_group(&chain, &chain);
            // The glass alone into `lit` (laid over the frame after), its
            // distances over the opaque ones.
            let resolve = |view, target, load: bool| wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: target,
                ops: wgpu::Operations {
                    load: if load {
                        wgpu::LoadOp::Load
                    } else {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    },
                    store: wgpu::StoreOp::Discard,
                },
            };
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[
                    Some(resolve(&self.msaa, Some(&self.lit), false)),
                    Some(resolve(&self.msaa_dist, Some(&self.dist), true)),
                    Some(resolve(&self.msaa_spec, None, false)),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.group0, &[]);
            let shadows = self.shadows.as_ref().map_or(&self.no_shadows, |s| &s.group);
            pass.set_bind_group(3, shadows, &[]);
            for &(_, i) in &glass {
                pass.set_bind_group(1, &self.group1, &[(i as u64 * SLOT) as u32]);
                if i < n {
                    pass.set_bind_group(2, &groups[planes[i].layer.as_str()], &[]);
                    if let Some((buf, count)) = &walls[i] {
                        pass.set_pipeline(&self.pipes.glass_solid);
                        pass.set_vertex_buffer(0, buf.slice(..));
                        pass.draw(0..*count, 0..1);
                    }
                    pass.set_pipeline(&self.pipes.glass_face);
                    pass.draw(0..6, 0..1);
                } else {
                    let mesh = &self.meshes[&models[i - n].mesh];
                    pass.set_bind_group(2, &solid, &[]);
                    pass.set_pipeline(&self.pipes.glass_solid);
                    pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                    pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.count, 0, 0..1);
                }
            }
            drop(pass);
            let group = self.tex_group(&self.lit, &self.lit);
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(attach(&self.hdr, None))],
                ..Default::default()
            });
            self.full(&mut rp, &self.pipes.over, &group);
        }
        self.queue.submit([enc.finish()]);
        Ok(())
    }

    /// Plane `p`'s walls: the ones last built for its size, depth and
    /// outline, or new ones.
    fn walls_of(&mut self, p: &Plane) -> Option<(wgpu::Buffer, u32)> {
        let same = |w: &Walls| {
            w.size == p.size
                && w.depth == p.depth
                && match (&w.outline, &p.outline) {
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a == b,
                    (None, None) => true,
                    _ => false,
                }
        };
        if let Some(w) = self.walls.iter_mut().find(|w| same(w)) {
            w.used = true;
            return w.buf.clone();
        }
        let v = wall_mesh(p);
        let buf = (!v.is_empty()).then(|| {
            let buf = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mui-stage walls"),
                    contents: bytemuck::cast_slice(&v),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            (buf, (v.len() / 6) as u32)
        });
        self.walls_built += 1;
        self.walls.push(Walls {
            size: p.size,
            depth: p.depth,
            outline: p.outline.clone(),
            buf: buf.clone(),
            used: true,
        });
        buf
    }

    /// The frame so far and its distances into the chain, then halved down
    /// it.
    fn build_chain(&self, enc: &mut wgpu::CommandEncoder) {
        let level = |i: u32| {
            self.chain.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: i,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        let mut pass = |to: &wgpu::TextureView, pipe, group: &wgpu::BindGroup| {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(attach(to, Some(wgpu::Color::TRANSPARENT)))],
                ..Default::default()
            });
            self.full(&mut rp, pipe, group);
        };
        pass(
            &level(0),
            &self.pipes.chain,
            &self.tex_group(&self.hdr, &self.dist),
        );
        for i in 1..self.chain.mip_level_count() {
            let from = level(i - 1);
            pass(&level(i), &self.pipes.mip, &self.tex_group(&from, &from));
        }
    }

    /// A pass into the multisampled scene targets. `first` clears them;
    /// `last` resolves them into `hdr`, `dist` and `spec`; `keep` keeps the
    /// distances and depth for the glass pass after.
    fn scene_pass<'a>(
        &'a self,
        enc: &'a mut wgpu::CommandEncoder,
        none: &'a wgpu::BindGroup,
        first: bool,
        last: bool,
        keep: bool,
    ) -> wgpu::RenderPass<'a> {
        let ops = |clear: wgpu::Color, keep: bool| wgpu::Operations {
            load: if first {
                wgpu::LoadOp::Clear(clear)
            } else {
                wgpu::LoadOp::Load
            },
            store: if last && !keep {
                wgpu::StoreOp::Discard
            } else {
                wgpu::StoreOp::Store
            },
        };
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: &self.msaa,
                    depth_slice: None,
                    resolve_target: last.then_some(&self.hdr),
                    ops: ops(wgpu::Color::BLACK, false),
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &self.msaa_dist,
                    depth_slice: None,
                    resolve_target: last.then_some(&self.dist),
                    ops: ops(wgpu::Color::BLACK, keep),
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &self.msaa_spec,
                    depth_slice: None,
                    resolve_target: last.then_some(&self.spec),
                    ops: ops(wgpu::Color::TRANSPARENT, false),
                }),
            ],
            // Each pass starts with an empty depth buffer: a reflection
            // lies below the floor, and must not hide what stands on it.
            // The last keeps it for the glass, which the opaque hides.
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.),
                    store: if last && keep {
                        wgpu::StoreOp::Store
                    } else {
                        wgpu::StoreOp::Discard
                    },
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_bind_group(0, &self.group0, &[]);
        pass.set_bind_group(1, &self.group1, &[0]);
        pass.set_bind_group(2, none, &[]);
        let shadows = self.shadows.as_ref().map_or(&self.no_shadows, |s| &s.group);
        pass.set_bind_group(3, shadows, &[]);
        pass
    }

    fn full(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        pipe: &wgpu::RenderPipeline,
        group: &wgpu::BindGroup,
    ) {
        pass.set_pipeline(pipe);
        pass.set_bind_group(0, &self.group0, &[]);
        pass.set_bind_group(1, &self.group1, &[0]);
        pass.set_bind_group(2, group, &[]);
        pass.set_bind_group(3, &self.no_shadows, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Depth of field, bloom and the film look from `src` into `dst`
    /// through `fin`.
    fn post(
        &self,
        enc: &mut wgpu::CommandEncoder,
        p: Post,
        t: f64,
        src: &wgpu::TextureView,
        dst: &wgpu::TextureView,
        fin: &wgpu::RenderPipeline,
    ) {
        let u = [
            p.threshold,
            p.knee.max(1e-4),
            p.bloom,
            p.exposure,
            p.aberration,
            p.vignette,
            p.grain,
            t as f32,
            f32::from(u8::from(p.tonemap)),
            0.,
            0.,
            0.,
            p.focus,
            p.aperture,
            p.max_blur.clamp(0., 64.),
            0.,
        ];
        self.queue
            .write_buffer(&self.post, 0, bytemuck::cast_slice(&u));
        let pass = |enc: &mut wgpu::CommandEncoder,
                    to: &wgpu::TextureView,
                    clear: bool,
                    pipe,
                    from: &wgpu::TextureView| {
            let group = self.tex_group(from, from);
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(attach(to, clear.then_some(wgpu::Color::BLACK)))],
                ..Default::default()
            });
            self.full(&mut pass, pipe, &group);
        };
        let focused = p.focus > 0. && p.aperture > 0. && p.max_blur > 0.;
        if focused {
            let group = self.tex_group(src, &self.dist);
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(attach(&self.dof, Some(wgpu::Color::BLACK)))],
                ..Default::default()
            });
            self.full(&mut rp, &self.pipes.dof, &group);
        }
        let src = if focused { &self.dof } else { src };
        // No bloom is no bloom passes: `fs_final` weighs the (stale) chain
        // by zero.
        if p.bloom > 0. {
            pass(enc, &self.bloom[0], true, &self.pipes.prefilter, src);
            for i in 1..BLOOM_LEVELS {
                pass(
                    enc,
                    &self.bloom[i],
                    true,
                    &self.pipes.down,
                    &self.bloom[i - 1],
                );
            }
            for i in (0..BLOOM_LEVELS - 1).rev() {
                pass(
                    enc,
                    &self.bloom[i],
                    false,
                    &self.pipes.up,
                    &self.bloom[i + 1],
                );
            }
        }
        let group = self.tex_group(src, &self.bloom[0]);
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(attach(dst, Some(wgpu::Color::BLACK)))],
            ..Default::default()
        });
        self.full(&mut rp, fin, &group);
    }
}

/// What post runs on a shot: a beauty sample's lens already blurred it.
fn post_of(s: &Shot) -> Post {
    match s.sample {
        Some(_) => Post {
            focus: 0.,
            ..s.post
        },
        None => s.post,
    }
}

/// The camera beauty sample `q` looks through, and the shift in clip space
/// that goes with it: up to half a pixel each way, plus, with depth of
/// field, a thin lens. The eye moves across a disc and the frustum shears
/// back so the focus plane stays put; the disc's radius makes something
/// infinitely far `aperture` pixels out, as the post blur would.
fn lens(s: &Shot, q: Option<[f32; 8]>, height: f32, aspect: f32) -> (Camera, [f32; 2]) {
    let Some(q) = q else {
        return (s.camera, [0.; 2]);
    };
    let mut cam = s.camera;
    let mut shift = [
        (q[0] - 0.5) * 2. / (height * aspect),
        (q[1] - 0.5) * 2. / height,
    ];
    let p = s.post;
    if p.focus > 0. && p.aperture > 0. {
        let ty = (cam.fov.to_radians() * 0.5).tan();
        let r = p.aperture * p.focus * ty / (height * 0.5);
        let [dx, dy] = disc(q[2], q[3]).map(|v| v * r);
        let [right, up, _] = cam.basis();
        for i in 0..3 {
            let o = right[i] * dx + up[i] * dy;
            cam.eye[i] += o;
            cam.target[i] += o;
        }
        shift[0] += dx / (p.focus * ty * aspect);
        shift[1] += dy / (p.focus * ty);
    }
    (cam, shift)
}

/// Light `l` moved to a point of its area for beauty sample `q`, the sizes
/// Blender gives the same softness: a sun's direction within a disc
/// `softness * 1.5` degrees across, a lamp's position within a disc
/// `softness * 8` units in radius across its beam. Each sample's own
/// shadow is then nearly hard; their mean has true penumbrae.
fn area(l: &Light, q: &[f32; 8]) -> Light {
    let mut l = *l;
    let [a, b] = disc(q[4], q[5]);
    let d = normalize(l.direction);
    let side = if d[1].abs() < 0.9 {
        [0., 1., 0.]
    } else {
        [1., 0., 0.]
    };
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let t = normalize(cross(d, side));
    let u = cross(d, t);
    let off = |k: f32| std::array::from_fn::<f32, 3, _>(|i| k * (a * t[i] + b * u[i]));
    match l.kind {
        LightKind::Directional => {
            let o = off((l.softness * 0.75).to_radians().tan());
            l.direction = std::array::from_fn(|i| d[i] + o[i]);
        }
        LightKind::Spot | LightKind::Point => {
            let o = off(l.softness * 8.);
            l.position = std::array::from_fn(|i| l.position[i] + o[i]);
        }
        LightKind::Ambient => return l,
    }
    l.softness = 1.;
    l
}

/// An `Rgba16Float` texture `size` texels with `mips` levels, the first
/// few filled from `levels` of linear RGBA (largest first).
fn float_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    [w, h]: [u32; 2],
    mips: u32,
    levels: &[&[[f32; 4]]],
) -> wgpu::Texture {
    use wgpu::TextureUsages as U;
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mui-stage environment"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: HDR,
        usage: U::TEXTURE_BINDING | U::COPY_DST | U::COPY_SRC | U::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    for (m, px) in levels.iter().enumerate() {
        let (w, h) = ((w >> m).max(1), (h >> m).max(1));
        let bytes: Vec<u8> = px
            .iter()
            .flatten()
            .flat_map(|&v| half::f16::from_f32(v).to_le_bytes())
            .collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: m as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 8),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }
    tex
}

/// Group 0: the globals, an environment's chain and the BRDF LUT.
fn env_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    globals: &wgpu::Buffer,
    env: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    lut: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(env),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(lut),
            },
        ],
    })
}

/// Depth maps `size` texels square, `layers` deep.
fn depth_array(device: &wgpu::Device, size: u32, layers: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mui-stage shadows"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH,
        usage: RT,
        view_formats: &[],
    })
}

/// The shadow maps a shot draws into and samples.
struct Shadows {
    /// A layer per directional or spot light, and the floor's contact view.
    flat: wgpu::Texture,
    /// Six faces per point light.
    cube: Option<wgpu::Texture>,
    group: wgpu::BindGroup,
}

/// Group 3: the flat maps, the comparison sampler and the cube faces.
fn shadow_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    flat: &wgpu::Texture,
    cube: &wgpu::Texture,
) -> wgpu::BindGroup {
    let all = |t: &wgpu::Texture| {
        t.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        })
    };
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&all(flat)),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&all(cube)),
            },
        ],
    })
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l < 1e-6 {
        [0., -1., 0.]
    } else {
        v.map(|c| c / l)
    }
}

/// The world box around everything that casts a shadow.
fn caster_bounds(
    planes: &[&Plane],
    models: &[&Model],
    meshes: &HashMap<String, Mesh>,
) -> Option<([f32; 3], [f32; 3])> {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    let mut add = |m: &Mat4, p: [f32; 3]| {
        let q = m.project(p);
        for i in 0..3 {
            lo[i] = lo[i].min(q[i]);
            hi[i] = hi[i].max(q[i]);
        }
    };
    for p in planes.iter().filter(|p| p.cast) {
        let (m, [w, h]) = (p.model(), p.size.map(|v| v * 0.5));
        for [x, y, z] in [
            [-w, -h, 0.],
            [w, h, -p.depth],
            [-w, h, 0.],
            [w, -h, -p.depth],
        ] {
            add(&m, [x, y, z]);
            add(&m, [-x, -y, z]);
        }
    }
    for m in models.iter().filter(|m| m.cast) {
        let Some(mesh) = meshes.get(&m.mesh) else {
            continue;
        };
        for i in 0..8 {
            let pick = |a: usize| {
                if i >> a & 1 == 0 {
                    mesh.min[a]
                } else {
                    mesh.max[a]
                }
            };
            add(&m.transform, [pick(0), pick(1), pick(2)]);
        }
    }
    (lo[0] <= hi[0]).then_some((lo, hi))
}

/// A light's view of the casters, and its depth bias. A directional light
/// sees them through an orthographic box fitted around them (reaching far
/// behind, where the floor catches their shadow); a spot light through its
/// cone.
fn shadow_view(l: &Light, dir: [f32; 3], (lo, hi): ([f32; 3], [f32; 3])) -> (Mat4, f32) {
    let c: [f32; 3] = std::array::from_fn(|i| (lo[i] + hi[i]) * 0.5);
    let r = (0..3)
        .map(|i| (hi[i] - lo[i]).powi(2))
        .sum::<f32>()
        .sqrt()
        .max(1.)
        * 0.5;
    let up = if dir[1].abs() > 0.99 {
        [0., 0., 1.]
    } else {
        [0., 1., 0.]
    };
    if l.kind == LightKind::Directional {
        let eye: [f32; 3] = std::array::from_fn(|i| c[i] - dir[i] * (r + 1.));
        let far = 2. * r + 20_000.;
        let m = Mat4::orthographic(r, r, 0., far) * Mat4::look_at(eye, c, up);
        (m, 1.5 / far)
    } else {
        let p = l.position;
        let reach = (0..3).map(|i| (p[i] - c[i]).powi(2)).sum::<f32>().sqrt() + r;
        let far = if l.range > 0. { l.range } else { reach };
        let fov = (2. * l.cone.clamp(0.1, 89.) + 4.).min(178.).to_radians();
        let at: [f32; 3] = std::array::from_fn(|i| p[i] + dir[i]);
        let m = Mat4::perspective(fov, 1., 4., far.max(8.)) * Mat4::look_at(p, at, up);
        (m, 0.)
    }
}

/// A layer that is one outline, filled.
fn filled(size: Size, shape: Arc<Path>, color: mui_scene::Color) -> mui_scene::SceneSpec {
    use mui_scene::prelude::*;
    SceneSpec::new(
        block(size.width, size.height)
            .outline(move |_| (*shape).clone())
            .fill(color),
    )
}

fn attach(
    view: &wgpu::TextureView,
    clear: Option<wgpu::Color>,
) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: clear.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear),
            store: wgpu::StoreOp::Store,
        },
    }
}

/// Wall quads from the outline: position and outward normal per vertex, in
/// the slab's local space (y up, front face at z = 0).
fn wall_mesh(p: &Plane) -> Vec<f32> {
    let [w, h] = p.size;
    let rect;
    let outline = if let Some(o) = &p.outline {
        &**o
    } else {
        rect = Path::polyline(
            [(0., 0.), (w, 0.), (w, h), (0., h)]
                .map(|(x, y)| mui_geometry::Point::new(f64::from(x), f64::from(y))),
            true,
        );
        &rect
    };
    let Ok(contours) = outline.flatten(0.25, 1 << 14) else {
        return Vec::new();
    };
    let rings: Vec<Vec<[f32; 2]>> = contours
        .iter()
        .map(|c| {
            c.iter()
                .map(|q| [q.x as f32 - w * 0.5, h * 0.5 - q.y as f32])
                .collect::<Vec<_>>()
        })
        .filter(|r| r.len() >= 3)
        .collect();
    let area = |pts: &[[f32; 2]]| -> f32 {
        let n = pts.len();
        (0..n)
            .map(|i| {
                let (a, b) = (pts[i], pts[(i + 1) % n]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum()
    };
    // Outward is to the right of an edge wound like the largest contour, so
    // a letter's counter (a hole wound the other way) faces into the hole.
    let out = rings
        .iter()
        .map(|r| area(r))
        .max_by(|a, b| a.abs().total_cmp(&b.abs()))
        .map_or(1., f32::signum);
    let mut v = Vec::new();
    for pts in rings {
        let n = pts.len();
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-4 {
                continue;
            }
            let nrm = [dy / len * out, -dx / len * out, 0.];
            let z = -p.depth;
            for (pt, zz) in [(a, 0.), (b, 0.), (a, z), (a, z), (b, 0.), (b, z)] {
                v.extend_from_slice(&[pt[0], pt[1], zz]);
                v.extend_from_slice(&nrm);
            }
        }
    }
    v
}

fn pipelines(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    background: &str,
    present: wgpu::TextureFormat,
) -> Result<Pipelines, Error> {
    let source = SHADER.replace("{{BACKGROUND}}", background);
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("mui-stage"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let premul = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;
    let add = wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent::REPLACE,
    };
    let weighted = wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Constant,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Constant,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
    };
    // SSR adds its (signed) change to the colour and leaves alpha.
    let add_rgb = wgpu::BlendState {
        color: add.color,
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
    };
    let wall_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
    let wall_buf = [Some(wgpu::VertexBufferLayout {
        array_stride: 24,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wall_attrs,
    })];
    let uv_attrs = wgpu::vertex_attr_array![2 => Float32x2];
    let mesh_buf = [
        wall_buf[0].clone(),
        Some(wgpu::VertexBufferLayout {
            array_stride: 8,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &uv_attrs,
        }),
    ];
    // `scene`: into the multisampled HDR target with depth.
    let make = |vs: &str,
                fs: &str,
                format: wgpu::TextureFormat,
                blend: Option<wgpu::BlendState>,
                scene: bool,
                depth_write: bool,
                buffers: &[Option<wgpu::VertexBufferLayout>]| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(fs),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(vs),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(fs),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                // A scene draw also writes its eye distance and normal and
                // its reflection's weight, unblended: the nearest
                // opaque-enough surface is what depth of field and SSR see.
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    scene.then_some(wgpu::ColorTargetState {
                        format: HDR,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    scene.then_some(wgpu::ColorTargetState {
                        format: HDR,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ][..if scene { 3 } else { 1 }],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: scene.then(|| wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(depth_write),
                depth_compare: Some(if depth_write {
                    wgpu::CompareFunction::LessEqual
                } else {
                    wgpu::CompareFunction::Always
                }),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: if scene { SAMPLES } else { 1 },
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        })
    };
    // Depth only, into a shadow map; the slope bias keeps a lit surface
    // from shadowing itself.
    let shadow = |vs: &str, fs: Option<&str>, buffers: &[Option<wgpu::VertexBufferLayout>]| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(vs),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(vs),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers,
            },
            fragment: fs.map(|fs| wgpu::FragmentState {
                module: &module,
                entry_point: Some(fs),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 0,
                    slope_scale: 1.5,
                    clamp: 0.,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    };
    let p = Pipelines {
        bg: make("vs_full", "fs_bg", HDR, None, true, false, &[]),
        front: make("vs_front", "fs_front", HDR, Some(premul), true, true, &[]),
        back: make("vs_back", "fs_back", HDR, Some(premul), true, true, &[]),
        wall: make(
            "vs_wall",
            "fs_wall",
            HDR,
            Some(premul),
            true,
            true,
            &wall_buf,
        ),
        accum: make("vs_full", "fs_copy", HDR, Some(weighted), false, false, &[]),
        prefilter: make("vs_full", "fs_prefilter", HDR, None, false, false, &[]),
        down: make("vs_full", "fs_down", HDR, None, false, false, &[]),
        up: make("vs_full", "fs_up", HDR, Some(add), false, false, &[]),
        fin: make("vs_full", "fs_final", OUT, None, false, false, &[]),
        floor: make("vs_floor", "fs_floor", HDR, Some(premul), true, false, &[]),
        linearize: make("vs_full", "fs_linearize", HDR, None, false, false, &[]),
        mip: make("vs_full", "fs_copy", HDR, None, false, false, &[]),
        dof: make("vs_full", "fs_dof", HDR, None, false, false, &[]),
        gtao: make("vs_full", "fs_gtao", HDR, None, false, false, &[]),
        ao_apply: make("vs_full", "fs_ao_apply", HDR, None, false, false, &[]),
        present: make("vs_full", "fs_final", present, None, false, false, &[]),
        mesh: make(
            "vs_mesh",
            "fs_mesh",
            HDR,
            Some(premul),
            true,
            true,
            &mesh_buf,
        ),
        shadow_face: shadow("vs_shadow_face", Some("fs_shadow_face"), &[]),
        shadow_solid: shadow("vs_shadow_solid", None, &wall_buf),
        chain: make("vs_full", "fs_chain", HDR, None, false, false, &[]),
        ssr: make("vs_full", "fs_ssr", HDR, Some(add_rgb), false, false, &[]),
        over: make("vs_full", "fs_copy", HDR, Some(premul), false, false, &[]),
        glass_face: make("vs_front", "fs_glass", HDR, Some(premul), true, true, &[]),
        glass_solid: make(
            "vs_wall",
            "fs_glass_solid",
            HDR,
            Some(premul),
            true,
            true,
            &wall_buf,
        ),
    };
    // Waiting on the scope blocks, which a browser cannot: there a bad
    // shader shows up as an uncaptured error instead.
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(e) = pollster::block_on(scope.pop()) {
        return Err(gpu(e));
    }
    #[cfg(target_arch = "wasm32")]
    drop(scope);
    Ok(p)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_glass;
