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
use std::sync::Arc;

use mui_geometry::Path;
use mui_scene::{ResolvedScene, Size};
use mui_vello::effects::{Budget, GpuRenderer};
use mui_vello::kurbo::Affine;
use wgpu::util::DeviceExt;

mod math;
pub use math::Mat4;

/// Why the stage could not render.
#[derive(Debug)]
pub enum Error {
    /// The adapter, device, readback or a shader said no.
    Gpu(String),
    /// A shot names a layer [`Stage::layer`] never painted.
    MissingLayer(String),
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
            Self::MissingLayer(s) => write!(f, "no layer {s:?}: paint it with Stage::layer first"),
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
/// Per-draw uniform slot: `Draw` is 112 bytes, dynamic offsets align to 256.
const SLOT: u64 = 256;
const DRAW: u64 = 112;
/// Draw slots per subframe: every plane, its reflection, and the floor.
const SLOTS: usize = 129;
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
    fn view_proj(&self, aspect: f32) -> Mat4 {
        let up = [
            -self.roll.to_radians().sin(),
            self.roll.to_radians().cos(),
            0.,
        ];
        Mat4::perspective(self.fov.to_radians(), aspect, 1., 100_000.)
            * Mat4::look_at(self.eye, self.target, up)
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
    /// Wall and back colour, linear RGB.
    pub edge: [f32; 3],
    /// Multiplies the face's light: above 1 it feeds the bloom.
    pub glow: f32,
    pub opacity: f32,
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
            edge: [0.05, 0.05, 0.07],
            glow: 1.,
            opacity: 1.,
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
    fn model(&self) -> Mat4 {
        let [x, y, z] = self.rotation.map(f32::to_radians);
        Mat4::translate(self.position)
            * Mat4::rotate_y(y)
            * Mat4::rotate_x(x)
            * Mat4::rotate_z(z)
            * Mat4::scale(self.scale)
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
}
impl Floor {
    pub fn at(y: f32) -> Self {
        Self {
            y,
            color: [0.004, 0.004, 0.008],
            reflect: 0.35,
            falloff: 120.,
            radius: 1600.,
        }
    }
}

/// Everything in front of the camera at one instant.
#[derive(Clone, Debug)]
pub struct Shot {
    pub camera: Camera,
    /// Drawn walls first, then faces far to near, so glass edges blend over
    /// what is behind them. At most 64.
    pub planes: Vec<Plane>,
    pub floor: Option<Floor>,
    pub post: Post,
}
impl Shot {
    pub fn new(camera: Camera) -> Self {
        Self {
            camera,
            planes: Vec::new(),
            floor: None,
            post: Post::default(),
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
    group0: wgpu::BindGroup,
    group1: wgpu::BindGroup,
    tex_layout: wgpu::BindGroupLayout,
    layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
    pipes: Pipelines,
    msaa: wgpu::TextureView,
    /// Eye distance per pixel, multisampled and resolved, for depth of field.
    msaa_dist: wgpu::TextureView,
    dist: wgpu::TextureView,
    dof: wgpu::TextureView,
    depth: wgpu::TextureView,
    hdr: wgpu::TextureView,
    accum: wgpu::TextureView,
    bloom: Vec<wgpu::TextureView>,
    out: wgpu::Texture,
    readback: wgpu::Buffer,
    layers: HashMap<String, Layer>,
    /// One Vello pipeline set for every layer, resized to each in turn.
    renderer: GpuRenderer,
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
        Self::with_device(device, queue, width, height)
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
        let globals = uniform(112);
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
        let l0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[entry(0, buffer(false))],
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
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&l0), Some(&l1), Some(&tex_layout)],
            immediate_size: 0,
        });
        let group0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &l0,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals.as_entire_binding(),
            }],
        });
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
        let pipes = pipelines(&device, &layout, DEFAULT_BACKGROUND)?;
        let renderer = pollster::block_on(GpuRenderer::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            [1, 1],
            Budget::default(),
        ))?;

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
        let dof = view(&target(&device, width, height, HDR, 1, RT));
        let hdr = view(&target(&device, width, height, HDR, 1, RT));
        let accum = view(&target(&device, width, height, HDR, 1, RT));
        let bloom = (1..=BLOOM_LEVELS as u32)
            .map(|i| view(&target(&device, width >> i, height >> i, HDR, 1, RT)))
            .collect();
        let out = target(
            &device,
            width,
            height,
            OUT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mui-stage readback"),
            size: u64::from((width * 16).next_multiple_of(256)) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Ok(Self {
            device,
            queue,
            width,
            height,
            globals,
            draws,
            post,
            group0,
            group1,
            tex_layout,
            layout,
            sampler,
            pipes,
            msaa,
            msaa_dist,
            dist,
            dof,
            depth,
            hdr,
            accum,
            bloom,
            out,
            readback,
            layers: HashMap::new(),
            renderer,
        })
    }

    /// Replace the background with WGSL defining
    /// `fn background(uv: vec2f, t: f32) -> vec3f` (linear light; `uv` is
    /// 0..1 with y down; `noise`, `fbm` and `hash2` are in scope). A shader
    /// that does not compile is an error and leaves the old one in place.
    pub fn background(&mut self, wgsl: &str) -> Result<(), Error> {
        self.pipes = pipelines(&self.device, &self.layout, wgsl)?;
        Ok(())
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
        let (w, h) = (
            (size.width * supersample).ceil().clamp(1., 8192.) as u16,
            (size.height * supersample).ceil().clamp(1., 8192.) as u16,
        );
        let stale = self.layers.get(id).is_none_or(|l| {
            let s = l.texture.size();
            (s.width, s.height) != (w.into(), h.into())
        });
        if stale {
            let size = wgpu::Extent3d {
                width: w.into(),
                height: h.into(),
                depth_or_array_layers: 1,
            };
            let raw = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("mui-stage layer (vello)"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: RT,
                view_formats: &[],
            });
            // A slab seen far off or edge-on samples a mip, not a shimmer.
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("mui-stage layer"),
                size,
                mip_level_count: size.max_mips(wgpu::TextureDimension::D2),
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
        let target = view(&self.layers[id].raw);
        // Shared across layers: switching layers re-encodes (damage against
        // the last layer drawn), which a layer call does anyway.
        self.renderer.resize([w.into(), h.into()])?;
        self.renderer
            .render(scene, Affine::scale(supersample), &target)?;
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        // Linearise into mip 0, then halve down the chain.
        let layer = &self.layers[id];
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
            .rigid_transform(mui_geometry::Point::new(pad, pad + run.ascent), 0.)?;
        let size = Size::new(run.advance + 2. * pad, run.ascent + run.descent + 2. * pad);
        let shape = Arc::new(path);
        let spec = filled(size, shape.clone(), color);
        self.layer(id, &mui_scene::resolve(&spec)?, size, supersample)?;
        Ok(Plane::new(id, size.width as f32, size.height as f32).outline(shape))
    }

    fn tex_group(&self, tex: &wgpu::TextureView, bloom: &wgpu::TextureView) -> wgpu::BindGroup {
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
            ],
        })
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
        let n = subframes.max(1);
        let aspect = self.width as f32 / self.height as f32;
        let mut last = None;
        for i in 0..n {
            // Centred on the frame time's trailing interval: subframe 0 of 1
            // is exactly `t`.
            let at = t - shutter * f64::from(n - 1 - i) / f64::from(n);
            let s = shot(at);
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
            last = Some(s.post);
        }
        self.post(last.expect("n >= 1"), t)
    }

    /// One subframe into `self.hdr`.
    fn scene(&mut self, s: &Shot, t: f64, aspect: f32) -> Result<(), Error> {
        let vp = s.camera.view_proj(aspect);
        let mut g = [0f32; 28];
        g[..16].copy_from_slice(&vp.0);
        g[16..19].copy_from_slice(&s.camera.eye);
        g[20] = t as f32;
        g[21] = self.width as f32;
        g[22] = self.height as f32;
        if let Some(f) = s.floor {
            g[24..27].copy_from_slice(&f.color);
        }
        self.queue
            .write_buffer(&self.globals, 0, bytemuck::cast_slice(&g));

        let planes: Vec<&Plane> = s
            .planes
            .iter()
            .filter(|p| p.opacity > 0.)
            .take(64)
            .collect();
        if let Some(p) = planes.iter().find(|p| !self.layers.contains_key(&p.layer)) {
            return Err(Error::MissingLayer(p.layer.clone()));
        }
        let n = planes.len();
        // Slots: planes 0..n, their reflections n..2n, the floor at 2n.
        let mut slots = vec![0u8; SLOT as usize * SLOTS];
        let mut put =
            |slot: usize, model: Mat4, size: [f32; 4], edge: [f32; 4], mirror: [f32; 4]| {
                let mut d = [0f32; 28];
                d[..16].copy_from_slice(&model.0);
                d[16..20].copy_from_slice(&size);
                d[20..24].copy_from_slice(&edge);
                d[24..28].copy_from_slice(&mirror);
                slots[slot * SLOT as usize..][..DRAW as usize]
                    .copy_from_slice(bytemuck::cast_slice(&d));
            };
        let flip = s.floor.map(|f| {
            let mut m = Mat4::IDENTITY;
            m.0[5] = -1.;
            Mat4::translate([0., f.y, 0.]) * m * Mat4::translate([0., -f.y, 0.])
        });
        let mut walls = Vec::new();
        for (i, p) in planes.iter().enumerate() {
            let size = [p.size[0], p.size[1], p.depth, p.glow];
            let edge = [p.edge[0], p.edge[1], p.edge[2], p.opacity];
            put(i, p.model(), size, edge, [0.; 4]);
            if let (Some(f), Some(flip)) = (s.floor, flip) {
                put(
                    n + i,
                    flip * p.model(),
                    size,
                    edge,
                    [f.y, f.reflect, f.falloff.max(1e-3), f.radius.max(1e-3)],
                );
            }
            walls.push(if p.depth > 0. {
                let v = wall_mesh(p);
                (!v.is_empty()).then(|| {
                    let buf = self
                        .device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("mui-stage walls"),
                            contents: bytemuck::cast_slice(&v),
                            usage: wgpu::BufferUsages::VERTEX,
                        });
                    (buf, (v.len() / 6) as u32)
                })
            } else {
                None
            });
        }
        if let Some(f) = s.floor {
            put(
                2 * n,
                Mat4::IDENTITY,
                [f.radius.max(1e-3), 0., 0., 0.],
                [f.color[0], f.color[1], f.color[2], 1.],
                [f.y, 0., 0., 0.],
            );
        }
        self.queue.write_buffer(&self.draws, 0, &slots);

        // Faces far to near by the depth of each slab's centre.
        let mut order: Vec<usize> = (0..n).collect();
        let depth = |i: usize| {
            let c = vp * planes[i].model();
            // Clip-space w of the local origin: its distance along the view.
            c.0[15]
        };
        order.sort_by(|&a, &b| depth(b).total_cmp(&depth(a)));
        // The mirror image sorts the other way round: far below is far away.
        let draw = |pass: &mut wgpu::RenderPass<'_>, base: usize| {
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
        // The background samples nothing, but its layout still has a
        // group 2: bind the frame's own HDR input.
        let none = self.tex_group(&self.accum, &self.accum);
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = self.scene_pass(&mut enc, &none, true, s.floor.is_none());
            pass.set_pipeline(&self.pipes.bg);
            pass.draw(0..3, 0..1);
            if s.floor.is_some() {
                pass.set_bind_group(1, &self.group1, &[((2 * n) as u64 * SLOT) as u32]);
                pass.set_pipeline(&self.pipes.floor);
                pass.draw(0..6, 0..1);
                draw(&mut pass, n);
            } else {
                draw(&mut pass, 0);
            }
        }
        if s.floor.is_some() {
            let mut pass = self.scene_pass(&mut enc, &none, false, true);
            draw(&mut pass, 0);
        }
        self.queue.submit([enc.finish()]);
        Ok(())
    }

    /// A pass into the multisampled scene targets. `first` clears them;
    /// `last` resolves them into `hdr` and `dist`.
    fn scene_pass<'a>(
        &'a self,
        enc: &'a mut wgpu::CommandEncoder,
        none: &'a wgpu::BindGroup,
        first: bool,
        last: bool,
    ) -> wgpu::RenderPass<'a> {
        let ops = |clear: wgpu::Color| wgpu::Operations {
            load: if first {
                wgpu::LoadOp::Clear(clear)
            } else {
                wgpu::LoadOp::Load
            },
            store: if last {
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
                    ops: ops(wgpu::Color::BLACK),
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &self.msaa_dist,
                    depth_slice: None,
                    resolve_target: last.then_some(&self.dist),
                    ops: ops(wgpu::Color::BLACK),
                }),
            ],
            // Each pass starts with an empty depth buffer: a reflection
            // lies below the floor, and must not hide what stands on it.
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_bind_group(0, &self.group0, &[]);
        pass.set_bind_group(1, &self.group1, &[0]);
        pass.set_bind_group(2, none, &[]);
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
        pass.draw(0..3, 0..1);
    }

    fn post(&mut self, p: Post, t: f64) -> Result<Frame, Error> {
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
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
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
            let group = self.tex_group(&self.accum, &self.dist);
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(attach(&self.dof, Some(wgpu::Color::BLACK)))],
                ..Default::default()
            });
            self.full(&mut rp, &self.pipes.dof, &group);
        }
        let src = if focused { &self.dof } else { &self.accum };
        pass(&mut enc, &self.bloom[0], true, &self.pipes.prefilter, src);
        for i in 1..BLOOM_LEVELS {
            pass(
                &mut enc,
                &self.bloom[i],
                true,
                &self.pipes.down,
                &self.bloom[i - 1],
            );
        }
        for i in (0..BLOOM_LEVELS - 1).rev() {
            pass(
                &mut enc,
                &self.bloom[i],
                false,
                &self.pipes.up,
                &self.bloom[i + 1],
            );
        }
        {
            let group = self.tex_group(src, &self.bloom[0]);
            let out = view(&self.out);
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(attach(&out, Some(wgpu::Color::BLACK)))],
                ..Default::default()
            });
            self.full(&mut rp, &self.pipes.fin, &group);
        }
        let row = (self.width * 16).next_multiple_of(256);
        enc.copy_texture_to_buffer(
            self.out.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            self.out.size(),
        );
        self.queue.submit([enc.finish()]);
        let slice = self.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(gpu)?;
        let stride = self.width as usize * 16;
        let mut rgba = Vec::with_capacity(self.width as usize * self.height as usize * 4);
        for line in slice
            .get_mapped_range()
            .map_err(gpu)?
            .chunks_exact(row as usize)
        {
            rgba.extend_from_slice(bytemuck::cast_slice::<u8, f32>(&line[..stride]));
        }
        self.readback.unmap();
        Ok(Frame {
            width: self.width,
            height: self.height,
            rgba,
        })
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
    let wall_attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
    let wall_buf = [Some(wgpu::VertexBufferLayout {
        array_stride: 24,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wall_attrs,
    })];
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
                // A scene draw also writes its eye distance, unblended: the
                // nearest opaque-enough surface is what depth of field sees.
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
                ][..if scene { 2 } else { 1 }],
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
    };
    match pollster::block_on(scope.pop()) {
        Some(e) => Err(gpu(e)),
        None => Ok(p),
    }
}

#[cfg(test)]
mod tests;
