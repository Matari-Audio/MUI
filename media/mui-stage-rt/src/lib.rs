//! Ray-traced glass for [`mui_stage`]: a path tracer on wgpu's hardware ray
//! queries for what raster glass cannot show, light bent off screen, many
//! bounces inside thick glass, total internal reflection, dispersion, rough
//! transmission and glass behind glass.
//!
//! It is hybrid: the stage draws the frame as ever, and [`Rt::draw`] lays
//! the glass over it, each glass pixel the mean of the paths traced through
//! it. Paths add up while the shot holds still, so a still converges
//! (`spp` per call, any number of calls); a shot that moves starts again,
//! and an edge-aware filter cleans the first samples.
//!
//! ```no_run
//! # use mui_stage::*;
//! # fn demo(shot: &Shot) -> Result<(), mui_stage_rt::Error> {
//! let (mut stage, mut rt) = mui_stage_rt::open(1280, 720)?;
//! // Paint layers into `stage` as ever, then hand the tracer their pixels:
//! // rt.layer("ui", &stage.layer_target("ui", size, format, mips)?);
//! let frame = rt.render(&mut stage, shot, 64)?;
//! # Ok(()) }
//! ```
//!
//! A device without ray queries (WebGPU, GL, most integrated GPUs) cannot
//! make an [`Rt`]: [`supported`] says so first, and the stage's own raster
//! glass is the fallback.
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use mui_stage::{Frame, LightKind, Mat4, Plane, Shot, Stage};
use wgpu::util::DeviceExt;

mod geom;

/// The device features the tracer needs.
pub const FEATURES: wgpu::Features = wgpu::Features::EXPERIMENTAL_RAY_QUERY;
/// What [`Rt::gpu_times`] needs.
const TIMERS: wgpu::Features =
    wgpu::Features::TIMESTAMP_QUERY.union(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);

const SHADER: &str = include_str!("rt.wgsl");
/// The stage's sky, spliced in so a ray bent off screen sees the same sky.
const STAGE: &str = include_str!("../../mui-stage/src/stage.wgsl");
/// Layer textures the shader binds; planes on others are white.
const LAYERS: usize = 4;
const MAX_INSTANCES: u32 = (mui_stage::MAX_PLANES + mui_stage::MAX_MODELS + 1) as u32;
/// Bounces a path makes at most: enough for light to cross thick glass,
/// reflect inside it and leave.
pub const BOUNCES: u32 = 12;
/// Most surfaces the deterministic trace follows a ray through.
pub const WHITTED_BOUNCES: u32 = 8;
/// Samples per pixel per submission.
const CHUNK: u32 = 4;
/// Below this many samples the filter cleans the mean.
const FILTER_UNTIL: u32 = 64;

#[derive(Debug)]
pub enum Error {
    /// The adapter or device has no hardware ray queries.
    Unavailable(String),
    Gpu(String),
    Stage(mui_stage::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(why) => write!(f, "ray-traced glass unavailable: {why}"),
            Self::Gpu(e) => write!(f, "GPU: {e}"),
            Self::Stage(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<mui_stage::Error> for Error {
    fn from(e: mui_stage::Error) -> Self {
        Self::Stage(e)
    }
}
fn gpu(e: impl std::fmt::Display) -> Error {
    Error::Gpu(e.to_string())
}

/// Whether `adapter` can trace: hardware ray queries, on Vulkan for now
/// (wgpu's ray queries are experimental, and Vulkan is where they are
/// tested).
pub fn supported(adapter: &wgpu::Adapter) -> bool {
    adapter.features().contains(FEATURES)
}

/// A device descriptor for `adapter` that asks for ray queries when it has
/// them, so one device serves the stage and the tracer; otherwise the
/// default.
pub fn device_descriptor(adapter: &wgpu::Adapter) -> wgpu::DeviceDescriptor<'static> {
    if !supported(adapter) {
        return wgpu::DeviceDescriptor::default();
    }
    wgpu::DeviceDescriptor {
        label: Some("mui-stage-rt"),
        // Timestamps too where there are any, for `Rt::gpu_times`.
        required_features: FEATURES | (adapter.features() & TIMERS),
        required_limits: wgpu::Limits::default()
            .using_minimum_supported_acceleration_structure_values(),
        // SAFETY: ray queries are behind wgpu's experimental token. The
        // tracer uses them as wgpu's own ray-query examples do (triangle
        // BLASes, one TLAS rebuilt per frame, queries in a compute pass),
        // and checks the result against analytic answers in its tests.
        experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
        ..Default::default()
    }
}

/// The adapter a headless stage would take, if it can trace.
pub fn probe() -> Result<wgpu::Adapter, Error> {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .map_err(|e| Error::Unavailable(e.to_string()))?;
    if !supported(&adapter) {
        return Err(Error::Unavailable(format!(
            "{} has no hardware ray queries",
            adapter.get_info().name
        )));
    }
    Ok(adapter)
}

/// A headless stage and a tracer on one ray-query device.
pub fn open(width: u32, height: u32) -> Result<(Stage, Rt), Error> {
    let adapter = probe()?;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&device_descriptor(&adapter))).map_err(gpu)?;
    let rt = Rt::new(&device, &queue, width, height)?;
    let stage = Stage::with_device(device, queue, width, height)?;
    Ok((stage, rt))
}

/// One bottom-level structure and where its triangles sit in the shader's
/// buffers.
struct Shape {
    blas: wgpu::Blas,
    tris: geom::Tris,
    vbase: u32,
    ibase: u32,
}
impl Shape {
    /// The first index of geometry `part` in the shader's index buffer.
    fn first(&self, part: usize) -> u32 {
        self.ibase + self.tris.parts.get(part).map_or(0, |p| p.0)
    }
}

/// A plane's shape: what its triangles depend on.
struct Slab {
    size: [f32; 2],
    depth: f32,
    outline: Option<Arc<mui_geometry::Path>>,
    shape: Shape,
    used: bool,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Inst {
    kind: [u32; 4],
    uv: [f32; 4],
    size: [f32; 4],
    edge: [f32; 4],
    mat: [f32; 4],
    mat2: [f32; 4],
    tint: [f32; 4],
    relief: [f32; 4],
    relief_scale: [f32; 4],
}

/// The tracer. One per stage size; it keeps the shapes it has built and
/// the running sum of samples.
pub struct Rt {
    device: wgpu::Device,
    queue: wgpu::Queue,
    width: u32,
    height: u32,
    trace: wgpu::ComputePipeline,
    filter: wgpu::ComputePipeline,
    composite_module: wgpu::ShaderModule,
    composites: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    globals: wgpu::Buffer,
    insts: wgpu::Buffer,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    accum: wgpu::Buffer,
    guide: wgpu::Buffer,
    /// The filter's ping-pong pair; the composite reads the last written.
    ping: [wgpu::Buffer; 2],
    filters: Vec<wgpu::Buffer>,
    post: wgpu::Buffer,
    sampler: wgpu::Sampler,
    white: wgpu::TextureView,
    layers: HashMap<String, wgpu::TextureView>,
    slabs: Vec<Slab>,
    meshes: HashMap<String, Shape>,
    floor: Option<Shape>,
    /// The shapes moved in the shader's buffers: upload them again.
    stale: bool,
    tlas: wgpu::Tlas,
    /// Samples in the sum, and what they are of.
    samples: u32,
    key: u64,
    /// Which ping buffer holds the glass to composite.
    out: usize,
    bounces: u32,
    denoise: bool,
    sky: Sky,
    /// The last trace was the deterministic one.
    whitted: bool,
    timer: Option<Timer>,
}

/// The sky baked by direction each frame it is on, with blurrier mips for
/// rougher glass: a ray reads a texel instead of the clouds' noise.
struct Sky {
    view: wgpu::TextureView,
    mips: Vec<wgpu::TextureView>,
    sampler: wgpu::Sampler,
    bake: wgpu::ComputePipeline,
    down: wgpu::ComputePipeline,
}

/// The baked sky's size: 0.35 degrees a texel.
const SKY: [u32; 2] = [1024, 512];
const SKY_MIPS: u32 = 6;

impl Sky {
    fn new(
        device: &wgpu::Device,
        bake: wgpu::ComputePipeline,
        down: wgpu::ComputePipeline,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mui-stage-rt sky"),
            size: wgpu::Extent3d {
                width: SKY[0],
                height: SKY[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: SKY_MIPS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });
        let mips = (0..SKY_MIPS)
            .map(|m| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: m,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        Self {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
            mips,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: wgpu::AddressMode::Repeat,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            }),
            bake,
            down,
        }
    }

    /// Bake the sky the globals describe, then each mip from the last.
    fn encode(
        &self,
        device: &wgpu::Device,
        globals: &wgpu::Buffer,
        enc: &mut wgpu::CommandEncoder,
    ) {
        let group = |pipe: &wgpu::ComputePipeline, entries: &[wgpu::BindGroupEntry<'_>]| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mui-stage-rt sky"),
                layout: &pipe.get_bind_group_layout(0),
                entries,
            })
        };
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
        let bake = group(
            &self.bake,
            &[
                entry(0, globals.as_entire_binding()),
                entry(14, wgpu::BindingResource::TextureView(&self.mips[0])),
            ],
        );
        pass.set_pipeline(&self.bake);
        pass.set_bind_group(0, &bake, &[]);
        pass.dispatch_workgroups(SKY[0].div_ceil(8), SKY[1].div_ceil(8), 1);
        pass.set_pipeline(&self.down);
        for m in 1..SKY_MIPS as usize {
            let down = group(
                &self.down,
                &[
                    entry(10, wgpu::BindingResource::TextureView(&self.mips[m - 1])),
                    entry(11, wgpu::BindingResource::TextureView(&self.mips[m])),
                ],
            );
            pass.set_bind_group(0, &down, &[]);
            pass.dispatch_workgroups((SKY[0] >> m).div_ceil(8), (SKY[1] >> m).div_ceil(8), 1);
        }
    }
}

/// GPU timestamps around the tracer's passes, on a device with
/// [`wgpu::Features::TIMESTAMP_QUERY`] and `TIMESTAMP_QUERY_INSIDE_ENCODERS`:
/// 0..1 the sky's bake and the scene's TLAS build, 1..2 the trace, 2..3 the filter, 4..5 the
/// composite.
struct Timer {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
}

/// Milliseconds of GPU time in the last frame's passes.
#[derive(Clone, Copy, Debug)]
pub struct GpuTimes {
    pub tlas: f64,
    pub trace: f64,
    pub filter: f64,
    pub composite: f64,
}

impl Rt {
    /// A tracer `width`x`height` on `device`, which must have [`FEATURES`]
    /// (see [`device_descriptor`]).
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) -> Result<Self, Error> {
        if !device.features().contains(FEATURES) {
            return Err(Error::Unavailable(
                "the device was opened without ray queries".into(),
            ));
        }
        let (w, h) = (width.max(1), height.max(1));
        let module = |label, src: String| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(src.into()),
            })
        };
        let (trace_src, filter_src, composite_src) = sources()?;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let trace_module = module("mui-stage-rt trace", trace_src);
        let filter_module = module("mui-stage-rt filter", filter_src);
        let composite_module = module("mui-stage-rt composite", composite_src);
        let compute = |m: &wgpu::ShaderModule, entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: None,
                module: m,
                entry_point: Some(entry),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            })
        };
        let trace = compute(&trace_module, "trace");
        let filter = compute(&filter_module, "denoise");
        let bake = compute(&trace_module, "bake_sky");
        let down = compute(&filter_module, "sky_down");
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(Error::Gpu(e.to_string()));
        }
        let buffer = |label, size: u64, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let storage = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC;
        let uniform = wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST;
        let px = u64::from(w) * u64::from(h) * 16;
        let white = device
            .create_texture_with_data(
                queue,
                &wgpu::TextureDescriptor {
                    label: Some("mui-stage-rt white"),
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &[255; 4],
            )
            .create_view(&wgpu::TextureViewDescriptor::default());
        Ok(Self {
            globals: buffer("mui-stage-rt globals", 32 * 16, uniform),
            insts: buffer(
                "mui-stage-rt instances",
                u64::from(MAX_INSTANCES) * size_of::<Inst>() as u64,
                storage,
            ),
            vertices: buffer("mui-stage-rt vertices", 24, storage),
            indices: buffer("mui-stage-rt indices", 12, storage),
            accum: buffer("mui-stage-rt sum", px, storage),
            guide: buffer("mui-stage-rt guide", px, storage),
            ping: [
                buffer("mui-stage-rt ping", px, storage),
                buffer("mui-stage-rt pong", px, storage),
            ],
            filters: (0..4)
                .map(|_| buffer("mui-stage-rt filter", 16, uniform))
                .collect(),
            post: buffer("mui-stage-rt post", 16, uniform),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            white,
            sky: Sky::new(device, bake, down),
            tlas: device.create_tlas(&wgpu::CreateTlasDescriptor {
                label: Some("mui-stage-rt scene"),
                max_instances: MAX_INSTANCES,
                flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
                update_mode: wgpu::AccelerationStructureUpdateMode::Build,
            }),
            device: device.clone(),
            queue: queue.clone(),
            width: w,
            height: h,
            trace,
            filter,
            composite_module,
            composites: HashMap::new(),
            layers: HashMap::new(),
            slabs: Vec::new(),
            meshes: HashMap::new(),
            floor: None,
            stale: true,
            samples: 0,
            key: 0,
            out: 0,
            bounces: BOUNCES,
            denoise: true,
            whitted: false,
            timer: device.features().contains(TIMERS).then(|| Timer {
                set: device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("mui-stage-rt timer"),
                    ty: wgpu::QueryType::Timestamp,
                    count: 6,
                }),
                resolve: buffer(
                    "mui-stage-rt timer",
                    48,
                    wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                ),
                read: buffer(
                    "mui-stage-rt timer read",
                    48,
                    wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                ),
            }),
        })
    }

    /// Layer `id`'s pixels, as [`Stage::layer_target`] holds them
    /// (premultiplied sRGB, 8 bits). Call it again when the layer is
    /// repainted: the samples so far are of the old pixels.
    pub fn layer(&mut self, id: &str, texture: &wgpu::Texture) {
        self.layers.insert(
            id.into(),
            texture.create_view(&wgpu::TextureViewDescriptor::default()),
        );
        self.reset();
    }

    /// A mesh, as [`Stage::mesh`] takes it: position and normal per vertex.
    /// Models name it by `id`.
    pub fn mesh(&mut self, id: &str, vertices: &[[f32; 6]], indices: &[u32]) {
        let tris = geom::mesh(vertices, indices);
        let shape = self.shape(tris, "mui-stage-rt mesh");
        self.meshes.insert(id.into(), shape);
        self.reset();
    }

    pub fn has_mesh(&self, id: &str) -> bool {
        self.meshes.contains_key(id)
    }

    /// Bounces per path (default [`BOUNCES`]).
    pub fn bounces(&mut self, n: u32) {
        self.bounces = n.max(1);
        self.reset();
    }

    /// Whether the first samples are filtered (default on). Off, the
    /// glass is the plain mean, noise and all.
    pub fn denoise(&mut self, on: bool) {
        self.denoise = on;
    }

    /// Samples in each glass pixel so far.
    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// Start the sum again.
    pub fn reset(&mut self) {
        self.samples = 0;
    }

    fn shape(&mut self, tris: geom::Tris, label: &str) -> Shape {
        let sizes: Vec<_> = tris
            .parts
            .iter()
            .map(|&(_, count)| wgpu::BlasTriangleGeometrySizeDescriptor {
                vertex_format: wgpu::VertexFormat::Float32x3,
                vertex_count: tris.vertices.len() as u32,
                index_format: Some(wgpu::IndexFormat::Uint32),
                index_count: Some(count),
                // Not opaque: every candidate goes through the shader's
                // coverage test (a face's layer alpha, opacity).
                flags: wgpu::AccelerationStructureGeometryFlags::NO_DUPLICATE_ANY_HIT_INVOCATION,
            })
            .collect();
        let blas = self.device.create_blas(
            &wgpu::CreateBlasDescriptor {
                label: Some(label),
                flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
                update_mode: wgpu::AccelerationStructureUpdateMode::Build,
            },
            wgpu::BlasGeometrySizeDescriptors::Triangles {
                descriptors: sizes.clone(),
            },
        );
        if !tris.indices.is_empty() {
            let input = |contents: &[u8]| {
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("mui-stage-rt blas input"),
                        contents,
                        usage: wgpu::BufferUsages::BLAS_INPUT,
                    })
            };
            let vb = input(bytemuck::cast_slice(&tris.vertices));
            let ib = input(bytemuck::cast_slice(&tris.indices));
            let geometry = sizes
                .iter()
                .zip(&tris.parts)
                .map(|(size, &(first, _))| wgpu::BlasTriangleGeometry {
                    size,
                    vertex_buffer: &vb,
                    first_vertex: 0,
                    vertex_stride: 24,
                    index_buffer: Some(&ib),
                    first_index: Some(first),
                    transform_buffer: None,
                    transform_buffer_offset: None,
                })
                .collect();
            let mut enc = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            enc.build_acceleration_structures(
                std::iter::once(&wgpu::BlasBuildEntry {
                    blas: &blas,
                    geometry: wgpu::BlasGeometries::TriangleGeometries(geometry),
                }),
                std::iter::empty(),
            );
            self.queue.submit([enc.finish()]);
        }
        self.stale = true;
        Shape {
            blas,
            tris,
            vbase: 0,
            ibase: 0,
        }
    }

    /// The plane's slab, built the first time a slab of its size, depth
    /// and outline is traced.
    fn slab(&mut self, p: &Plane) -> usize {
        let same = |s: &Slab| {
            s.size == p.size
                && s.depth == p.depth
                && match (&s.outline, &p.outline) {
                    (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a == b,
                    (None, None) => true,
                    _ => false,
                }
        };
        if let Some(i) = self.slabs.iter().position(same) {
            self.slabs[i].used = true;
            return i;
        }
        let shape = self.shape(geom::plane(p), "mui-stage-rt slab");
        self.slabs.push(Slab {
            size: p.size,
            depth: p.depth,
            outline: p.outline.clone(),
            shape,
            used: true,
        });
        self.slabs.len() - 1
    }

    /// Every shape's triangles into the shader's buffers, where they moved.
    fn upload(&mut self) {
        if !self.stale {
            return;
        }
        self.stale = false;
        let (mut v, mut ix): (Vec<[f32; 6]>, Vec<u32>) = (Vec::new(), Vec::new());
        let shapes = self
            .slabs
            .iter_mut()
            .map(|s| &mut s.shape)
            .chain(self.meshes.values_mut())
            .chain(self.floor.as_mut());
        for s in shapes {
            s.vbase = v.len() as u32;
            s.ibase = ix.len() as u32;
            v.extend_from_slice(&s.tris.vertices);
            ix.extend_from_slice(&s.tris.indices);
        }
        let storage = |label, contents: &[u8]| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: if contents.is_empty() {
                        &[0; 16]
                    } else {
                        contents
                    },
                    usage: wgpu::BufferUsages::STORAGE,
                })
        };
        self.vertices = storage("mui-stage-rt vertices", bytemuck::cast_slice(&v));
        self.indices = storage("mui-stage-rt indices", bytemuck::cast_slice(&ix));
    }

    /// Add `spp` samples per pixel of `shot`'s glass at `t` seconds (what
    /// ripples run by) to the sum, after starting it again if the shot is
    /// not the one summed so far.
    ///
    /// `spp` 0 is the deterministic trace instead: one ray per pixel (one
    /// per colour through dispersive glass), Fresnel-weighted reflections
    /// of the sky by direction, roughness as blur. No noise, no sum: each
    /// call is the whole image.
    pub fn trace(&mut self, shot: &Shot, t: f64, spp: u32) -> Result<(), Error> {
        let key = key_of(shot, t);
        self.whitted = spp == 0;
        if key != self.key || self.whitted {
            self.key = key;
            self.reset();
        }
        // Shapes, then the instances that place them.
        for s in &mut self.slabs {
            s.used = false;
        }
        let slabs: Vec<usize> = shot.planes.iter().map(|p| self.slab(p)).collect();
        if let Some(f) = shot.floor {
            // The stage draws its floor three radii out; so does this.
            let r = f.radius.max(1.) * 3.;
            if self
                .floor
                .as_ref()
                .is_none_or(|s| s.tris.vertices[1][0] != r)
            {
                let shape = self.shape(geom::floor(0., r), "mui-stage-rt floor");
                self.floor = Some(shape);
            }
        }
        self.upload();

        let mut slots: Vec<&str> = Vec::new();
        let mut views: Vec<wgpu::TextureView> = Vec::new();
        let mut insts: Vec<Inst> = Vec::new();
        let mut placed: Vec<(usize, Mat4)> = Vec::new();
        for (p, &si) in shot.planes.iter().zip(&slabs) {
            let slot = match slots.iter().position(|&l| l == p.layer) {
                Some(i) => i as u32,
                None => match self.layers.get(&p.layer) {
                    Some(v) if slots.len() < LAYERS => {
                        slots.push(&p.layer);
                        views.push(v.clone());
                        (slots.len() - 1) as u32
                    }
                    _ => u32::MAX,
                },
            };
            let m = &p.material;
            let thick = if m.thickness > 0. {
                m.thickness
            } else {
                p.depth
            } * p.scale.abs();
            let s = &self.slabs[si].shape;
            insts.push(Inst {
                kind: [0, slot, s.first(2), s.vbase],
                uv: p.uv,
                size: [p.size[0], p.size[1], p.depth, p.glow],
                edge: [p.edge[0], p.edge[1], p.edge[2], p.opacity.clamp(0., 1.)],
                mat: material(m),
                mat2: [
                    thick,
                    m.dispersion.max(0.),
                    m.print.clamp(0., 1.),
                    m.bevel.max(0.),
                ],
                tint: tint(m, p.receive),
                relief: relief(m),
                relief_scale: relief_scale(m),
            });
            placed.push((si, p.model()));
        }
        let mut models = Vec::new();
        for model in &shot.models {
            let Some(s) = self.meshes.get(&model.mesh) else {
                continue;
            };
            let m = &model.material;
            let c = model.color;
            insts.push(Inst {
                kind: [1, u32::MAX, s.first(0), s.vbase],
                uv: [0., 0., 1., 1.],
                size: [0., 0., 0., 1.],
                edge: [c[0], c[1], c[2], c[3].clamp(0., 1.)],
                mat: material(m),
                mat2: [
                    m.thickness.max(0.),
                    m.dispersion.max(0.),
                    m.print.clamp(0., 1.),
                    0.,
                ],
                tint: tint(m, model.receive),
                relief: relief(m),
                relief_scale: relief_scale(m),
            });
            models.push((&model.mesh, model.transform));
        }
        if let Some(f) = &self.floor
            && let Some(fl) = shot.floor
        {
            insts.push(Inst {
                kind: [2, u32::MAX, f.first(0), f.vbase],
                uv: [0.; 4],
                size: [0., 0., 0., 1.],
                edge: [fl.color[0], fl.color[1], fl.color[2], 1.],
                mat: [0., 1., 0., 1.5],
                mat2: [0.; 4],
                tint: [1., 1., 1., 1.],
                relief: [0.; 4],
                relief_scale: [1.; 4],
            });
        }
        for (i, (si, m)) in placed.iter().enumerate() {
            self.tlas[i] = Some(wgpu::TlasInstance::new(
                &self.slabs[*si].shape.blas,
                rows(m),
                i as u32,
                0xff,
            ));
        }
        let mut n = placed.len();
        for (id, m) in models {
            self.tlas[n] = Some(wgpu::TlasInstance::new(
                &self.meshes[id].blas,
                rows(&m),
                n as u32,
                0xff,
            ));
            n += 1;
        }
        if let (Some(f), Some(fl)) = (&self.floor, shot.floor) {
            self.tlas[n] = Some(wgpu::TlasInstance::new(
                &f.blas,
                rows(&Mat4::translate([0., fl.y, 0.])),
                n as u32,
                0xff,
            ));
            n += 1;
        }
        for i in n..MAX_INSTANCES as usize {
            self.tlas[i] = None;
        }
        self.slabs.retain(|s| s.used);
        if insts.is_empty() {
            insts.push(Inst::zeroed());
        }
        self.queue
            .write_buffer(&self.insts, 0, bytemuck::cast_slice(&insts));
        while views.len() < LAYERS {
            views.push(self.white.clone());
        }
        let layout = self.trace.get_bind_group_layout(0);
        let mut entries = vec![
            entry(0, self.globals.as_entire_binding()),
            entry(1, self.tlas.as_binding()),
            entry(2, self.insts.as_entire_binding()),
            entry(3, self.vertices.as_entire_binding()),
            entry(4, self.indices.as_entire_binding()),
            entry(5, self.accum.as_entire_binding()),
            entry(6, self.guide.as_entire_binding()),
            entry(7, wgpu::BindingResource::Sampler(&self.sampler)),
            entry(12, wgpu::BindingResource::TextureView(&self.sky.view)),
            entry(13, wgpu::BindingResource::Sampler(&self.sky.sampler)),
        ];
        for (i, v) in views.iter().enumerate() {
            entries.push(entry(8 + i as u32, wgpu::BindingResource::TextureView(v)));
        }
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mui-stage-rt trace"),
            layout: &layout,
            entries: &entries,
        });
        // A few samples per submission: one long dispatch would trip the
        // driver's GPU timeout.
        let mut left = spp.max(1);
        let mut build = true;
        while left > 0 {
            let n = left.min(CHUNK);
            let mut g = globals(
                shot,
                t,
                self.width,
                self.height,
                self.samples,
                n,
                self.bounces.min(if self.whitted {
                    WHITTED_BOUNCES
                } else {
                    u32::MAX
                }),
            );
            if self.whitted {
                g[27] = 1.;
            }
            self.queue
                .write_buffer(&self.globals, 0, bytemuck::cast_slice(&g));
            let mut enc = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            let first = std::mem::take(&mut build);
            if first {
                stamp(self.timer.as_ref(), &mut enc, 0);
                if shot.sky.is_some() {
                    self.sky.encode(&self.device, &self.globals, &mut enc);
                }
                enc.build_acceleration_structures(std::iter::empty(), std::iter::once(&self.tlas));
                stamp(self.timer.as_ref(), &mut enc, 1);
            }
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
                pass.set_pipeline(&self.trace);
                pass.set_bind_group(0, &group, &[]);
                pass.dispatch_workgroups(self.width.div_ceil(8), self.height.div_ceil(8), 1);
            }
            self.samples += n;
            left -= n;
            if left == 0 {
                stamp(self.timer.as_ref(), &mut enc, 2);
                self.filter_into(&mut enc);
                stamp(self.timer.as_ref(), &mut enc, 3);
            }
            self.queue.submit([enc.finish()]);
        }
        Ok(())
    }

    /// The mean into a ping buffer, then while few samples are in, three
    /// a-trous passes 1, 2 and 4 pixels apart.
    fn filter_into(&mut self, enc: &mut wgpu::CommandEncoder) {
        let steps: &[f32] = if self.denoise && !self.whitted && self.samples < FILTER_UNTIL {
            &[0., 1., 2., 4.]
        } else {
            &[0.]
        };
        let layout = self.filter.get_bind_group_layout(0);
        let (w, h) = (self.width as f32, self.height as f32);
        for (k, &step) in steps.iter().enumerate() {
            self.queue.write_buffer(
                &self.filters[k],
                0,
                bytemuck::cast_slice(&[step, self.samples as f32, w, h]),
            );
            let (src, dst) = (&self.ping[(k + 1) % 2], &self.ping[k % 2]);
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mui-stage-rt filter"),
                layout: &layout,
                entries: &[
                    entry(0, self.filters[k].as_entire_binding()),
                    entry(1, self.accum.as_entire_binding()),
                    entry(2, self.guide.as_entire_binding()),
                    entry(3, src.as_entire_binding()),
                    entry(4, dst.as_entire_binding()),
                ],
            });
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(&self.filter);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(self.width.div_ceil(8), self.height.div_ceil(8), 1);
            self.out = k % 2;
        }
    }

    /// The glass traced so far over `target` (a frame the stage drew into
    /// a `format` view the tracer's size), encoded as the stage encodes
    /// with `shot`'s post: exposure, vignette and tonemap.
    pub fn composite(
        &mut self,
        shot: &Shot,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) {
        let p = &shot.post;
        self.queue.write_buffer(
            &self.post,
            0,
            bytemuck::cast_slice(&[
                p.exposure,
                p.vignette,
                if p.tonemap { 1. } else { 0. },
                self.width as f32,
            ]),
        );
        let module = &self.composite_module;
        let device = &self.device;
        let pipe = self.composites.entry(format).or_insert_with(|| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("mui-stage-rt composite"),
                layout: None,
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("vs_full"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some("fs_composite"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        // Premultiplied over; the frame's alpha stays.
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                                operation: wgpu::BlendOperation::Add,
                            },
                            alpha: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::Zero,
                                dst_factor: wgpu::BlendFactor::One,
                                operation: wgpu::BlendOperation::Add,
                            },
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        });
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mui-stage-rt composite"),
            layout: &pipe.get_bind_group_layout(0),
            entries: &[
                entry(0, self.post.as_entire_binding()),
                entry(1, self.ping[self.out].as_entire_binding()),
            ],
        });
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        stamp(self.timer.as_ref(), &mut enc, 4);
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(pipe);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        stamp(self.timer.as_ref(), &mut enc, 5);
        if let Some(t) = &self.timer {
            enc.resolve_query_set(&t.set, 0..6, &t.resolve, 0);
            enc.copy_buffer_to_buffer(&t.resolve, 0, &t.read, 0, 48);
        }
        self.queue.submit([enc.finish()]);
    }

    /// The last composited frame's GPU time per pass, waiting for it; `None`
    /// on a device without timestamp queries (see [`Timer`]). With more
    /// than [`CHUNK`] samples a frame, the trace includes the gaps between
    /// its submissions.
    pub fn gpu_times(&self) -> Option<GpuTimes> {
        let t = self.timer.as_ref()?;
        let slice = t.read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let s: Vec<u64> = bytemuck::cast_slice(&slice.get_mapped_range().ok()?).to_vec();
        t.read.unmap();
        let ms = |a: usize, b: usize| {
            s[b].saturating_sub(s[a]) as f64 * f64::from(self.queue.get_timestamp_period()) * 1e-6
        };
        Some(GpuTimes {
            tlas: ms(0, 1),
            trace: ms(1, 2),
            filter: ms(2, 3),
            composite: ms(4, 5),
        })
    }

    /// The hybrid frame: `stage` draws `shot` into `target`, `spp` more
    /// samples of its glass are traced, and the glass goes over it.
    pub fn draw(
        &mut self,
        stage: &mut Stage,
        shot: &Shot,
        t: f64,
        spp: u32,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) -> Result<(), Error> {
        stage.draw(shot, t, target, format)?;
        self.trace(shot, t, spp)?;
        self.composite(shot, target, format);
        Ok(())
    }

    /// [`Rt::draw`] into a frame of its own, read back: what
    /// [`Stage::render`] returns, with traced glass. `glass` false is the
    /// stage's raster glass alone, for comparison.
    pub fn render(&mut self, stage: &mut Stage, shot: &Shot, spp: u32) -> Result<Frame, Error> {
        self.frame(stage, shot, spp, true)
    }

    /// [`Rt::render`], or with `glass` false the stage's own frame through
    /// the same readback.
    pub fn frame(
        &mut self,
        stage: &mut Stage,
        shot: &Shot,
        spp: u32,
        glass: bool,
    ) -> Result<Frame, Error> {
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mui-stage-rt frame"),
            size: wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        if glass {
            self.draw(stage, shot, 0., spp, &view, format)?;
        } else {
            stage.draw(shot, 0., &view, format)?;
        }
        let row = (self.width * 4).next_multiple_of(256);
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mui-stage-rt readback"),
            size: u64::from(row) * u64::from(self.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            tex.size(),
        );
        self.queue.submit([enc.finish()]);
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(gpu)?;
        let stride = self.width as usize * 4;
        let mut rgba = Vec::with_capacity(stride * self.height as usize);
        for line in slice
            .get_mapped_range()
            .map_err(gpu)?
            .chunks_exact(row as usize)
        {
            rgba.extend(line[..stride].iter().map(|&b| f32::from(b) / 255.));
        }
        readback.unmap();
        Ok(Frame {
            width: self.width,
            height: self.height,
            rgba,
        })
    }

    /// The device it traces on, which the stage shares.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// The device's queue: for writing layers the stage hands over.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Wait for the GPU: for timing a pass.
    pub fn finish(&self) -> Result<(), Error> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(gpu)
    }
}

fn stamp(timer: Option<&Timer>, enc: &mut wgpu::CommandEncoder, i: u32) {
    if let Some(t) = timer {
        enc.write_timestamp(&t.set, i);
    }
}

fn entry(binding: u32, resource: wgpu::BindingResource<'_>) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource }
}

use bytemuck::Zeroable;

fn material(m: &mui_stage::Material) -> [f32; 4] {
    [
        m.metallic.clamp(0., 1.),
        m.roughness.clamp(0.02, 1.),
        m.transmission.clamp(0., 1.),
        m.ior.max(1.),
    ]
}
fn tint(m: &mui_stage::Material, receive: bool) -> [f32; 4] {
    [
        m.tint[0].clamp(0., 1.),
        m.tint[1].clamp(0., 1.),
        m.tint[2].clamp(0., 1.),
        f32::from(u8::from(receive)),
    ]
}

/// A column-major matrix as a TLAS instance's 3x4 rows.
fn rows(m: &Mat4) -> [f32; 12] {
    let c = &m.0;
    [
        c[0], c[4], c[8], c[12], c[1], c[5], c[9], c[13], c[2], c[6], c[10], c[14],
    ]
}

/// What the samples are of: everything the tracer reads from the shot
/// (not its post, nor which beauty sample it is).
fn key_of(s: &Shot, t: f64) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    // Ripples run with time: a shot with any is another at another time.
    let ripples = s
        .planes
        .iter()
        .map(|p| &p.material)
        .chain(s.models.iter().map(|m| &m.material));
    if ripples.into_iter().any(|m| relief(m)[2] > 0.) {
        t.to_bits().hash(&mut h);
    }
    format!(
        "{:?}{:?}{:?}{:?}{:?}{:?}{:?}{:?}",
        s.camera, s.planes, s.models, s.lights, s.floor, s.clear, s.sky, s.environment
    )
    .hash(&mut h);
    h.finish()
}

/// A material's pressed relief as mui-stage packs it: the steepest slope
/// of its reeds, dimples and ripples (none without a size), and their sizes.
fn relief(m: &mui_stage::Material) -> [f32; 4] {
    let r = |r: mui_stage::Relief| if r.scale > 0. { r.strength.max(0.) } else { 0. };
    [r(m.ribbed), r(m.hammered), r(m.ripple), 0.]
}
fn relief_scale(m: &mui_stage::Material) -> [f32; 4] {
    [m.ribbed.scale, m.hammered.scale, m.ripple.scale, 0.].map(|v| v.max(1e-3))
}

/// The shader's globals (see `Globals` in rt.wgsl).
fn globals(s: &Shot, t: f64, w: u32, h: u32, done: u32, spp: u32, bounces: u32) -> [f32; 128] {
    let mut g = [0f32; 128];
    let cam = &s.camera;
    let [right, up, fwd] = cam.basis();
    let ty = (cam.fov.to_radians() * 0.5).tan();
    let aspect = w as f32 / h as f32;
    let lit = !s.lights.is_empty() || s.environment.is_some();
    g[0..3].copy_from_slice(&cam.eye);
    g[3] = done as f32;
    g[4..7].copy_from_slice(&right);
    g[7] = ty * aspect;
    g[8..11].copy_from_slice(&up);
    g[11] = ty;
    g[12..15].copy_from_slice(&fwd);
    g[15] = spp.max(1) as f32;
    g[16..20].copy_from_slice(&[w as f32, h as f32, bounces as f32, f32::from(u8::from(lit))]);
    if let Some(c) = s.clear {
        g[20..24].copy_from_slice(&[c[0], c[1], c[2], 1.]);
    }
    let mut ambient = [0f32; 3];
    let mut slot = 0;
    for l in &s.lights {
        if l.kind == LightKind::Ambient {
            for (a, c) in ambient.iter_mut().zip(l.color) {
                *a += c;
            }
            continue;
        }
        if slot == mui_stage::MAX_LIGHTS {
            continue;
        }
        let o = 56 + slot * 16;
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
        g[o + 13] = f32::from(u8::from(l.shadows));
        // mui-stage's `area`: a sun's disc is 0.75 degree per softness, a
        // lamp's 8 units.
        g[o + 14] = match l.kind {
            LightKind::Directional => (l.softness.max(0.) * 0.75).to_radians().tan(),
            _ => l.softness.max(0.) * 8.,
        };
        slot += 1;
    }
    g[24..27].copy_from_slice(&ambient);
    if let Some(f) = s.floor {
        g[28..32].copy_from_slice(&[f.color[0], f.color[1], f.color[2], 1.]);
        g[32..36].copy_from_slice(&[
            f.y,
            f.reflect.clamp(0., 1.),
            f.falloff.max(1e-3),
            f.radius.max(1e-3),
        ]);
    }
    let norm = band_norm();
    g[36..39].copy_from_slice(&norm);
    g[39] = t as f32;
    if let Some(k) = &s.sky {
        g[40..43].copy_from_slice(&normalize(k.sun));
        g[43] = 1.;
        g[44..47].copy_from_slice(&k.zenith);
        g[47] = k.cover.clamp(0., 1.);
        g[48..51].copy_from_slice(&k.horizon);
        g[51] = k.drift[0];
        g[52..55].copy_from_slice(&k.sun_color);
        g[55] = k.drift[1];
        if let Some(a) = &k.atmosphere {
            g[43] = 2.;
            g[120..124].copy_from_slice(&a.uniform());
            g[124] = mui_stage::sky::SUN * a.intensity;
        }
    }
    g
}

/// 1 / the mean of each band of rt.wgsl's `band` over 380..700 nm, so a
/// uniformly drawn wavelength carries white on average.
fn band_norm() -> [f32; 3] {
    let (mu, sigma) = ([605f32, 545., 455.], [42f32, 38., 32.]);
    let n = 3200;
    std::array::from_fn(|c| {
        let sum: f32 = (0..n)
            .map(|i| {
                let nm = 380. + 320. * (i as f32 + 0.5) / n as f32;
                let x = (nm - mu[c]) / sigma[c];
                (-0.5 * x * x).exp()
            })
            .sum();
        n as f32 / sum
    })
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 0. {
        v.map(|c| c / l)
    } else {
        [0., -1., 0.]
    }
}

/// The three shader modules: the tracer with the stage's sky spliced in,
/// the filter, and the composite.
fn sources() -> Result<(String, String, String), Error> {
    let cut = |s: &'static str, from: &str, to: &str| -> Result<&'static str, Error> {
        let a = s
            .find(from)
            .ok_or_else(|| Error::Gpu(format!("shader source has no `{from}`")))?;
        let b = s[a..]
            .find(to)
            .ok_or_else(|| Error::Gpu(format!("shader source has no `{to}`")))?;
        Ok(&s[a..a + b])
    };
    let sky = cut(STAGE, "fn hash2(", "// --- the background")?;
    let tracer = cut(SHADER, "", "// --- the filter")?.replace("{{SKY}}", sky);
    let filter = cut(SHADER, "// --- the filter", "// --- onto the raster frame")?;
    let composite = &SHADER[SHADER
        .find("// --- onto the raster frame")
        .ok_or_else(|| Error::Gpu("no composite".into()))?..];
    Ok((tracer, filter.into(), composite.into()))
}

#[cfg(test)]
mod tests;
