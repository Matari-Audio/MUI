//! The instanced SDF pass: every ordinary UI primitive as an instance of one
//! quad pipeline, antialiased in the fragment shader, in one draw call. No
//! compute shaders and no storage buffers, so it runs on a GLES 3 / WebGL2
//! class device.
//!
//! It draws rounded rects (filled or stroked, solid, gradient or image),
//! analytic drop and inset shadows, rect clips and glyph runs from a
//! coverage atlas, under any affine transform for shapes and translation
//! plus uniform scale for text; and any path, filled, stroked, blurred as a
//! drop shadow or clipping, from line segments in a data texture (see
//! [`path`]). Group layers (opacity, blend modes, masks), backdrop blurs,
//! weld materials and host textures go through offscreen targets only when
//! they must (see [`layer`]). Anything else in a paint list is reported by
//! [`classify`] and refused by [`QuadRenderer::render`]; it is never skipped
//! or approximated.
//!
//! Bindings: group 0 is the view (a uniform slot at a dynamic offset: the
//! target's size and device origin), the glyph atlas (an `R8Unorm` array,
//! one layer a page), a linear clamping sampler and the path records (an
//! `Rgba32Float` texture read by `textureLoad`); group 1 is one texture and
//! the snapshot a blending layer composites onto. A texture instance needs
//! its texture bound, so a run of instances is drawn in one call until one
//! wants a different texture than the run already holds: a list with no
//! images, or one image, is one draw call.

mod build;
mod damage;
mod gpu;
mod layer;
mod list;
mod offscreen;
mod path;
mod ramp;
#[cfg(test)]
mod tests;
mod text;

use crate::effects::WeldTextures;
use crate::kurbo::Affine;
use build::{Instance, SLOTS, Walk};
use damage::Retained;
use gpu::{atlas_texture, image_group, image_texture, instance_buffer, path_texture, view_group};
use layer::Kind;
use list::{Instances, Welds};
use mui_scene::{Layer, Painted, ResolvedScene};
use offscreen::{Draw, Offscreen, Slots};
use rustc_hash::FxHashMap;

pub use build::{MAX_GLYPH_PX, MAX_STOPS};
/// Vertex attributes (and inter-stage variables) one instance takes: what a
/// device must offer, at 16 bytes each of instance stride.
pub const INSTANCE_SLOTS: usize = SLOTS;
pub use layer::OFFSCREEN_BUDGET;
pub use offscreen::BLUR_SHADER;
pub use path::PATH_BUDGET;

/// The WGSL of the pass, for validation and tooling.
pub const SHADER: &str = include_str!("quad.wgsl");

/// Why the pass cannot draw an entry yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reason {
    /// A glyph run with a bitmap or colour glyph in it (found when drawn,
    /// not by [`classify`]).
    Text,
    /// A glyph run under a rotation, shear or non-uniform scale.
    TextTransform,
    /// A glyph run larger than [`MAX_GLYPH_PX`] device pixels per em.
    TextSize,
    /// More glyphs in one frame than the atlas budget holds, even emptied
    /// (found when drawn).
    GlyphBudget,
    /// An image paint on a glyph run.
    Image,
    /// An image larger than the device's largest texture (found when
    /// drawn).
    ImageSize,
    /// A retained vector paint.
    Vector,
    /// A path the geometry kernel refuses to convert (a malformed arc), as
    /// [`crate::paint`] refuses the whole list over it.
    Path,
    /// More path records in one frame than the path budget holds, even
    /// emptied (found when drawn).
    PathBudget,
    /// A rounded or rotated clip inside another, or a path clip inside
    /// another: an instance carries one of each.
    ClipDepth,
    /// A blend layer's end with none open, a layer never closed, or a mask
    /// outside any layer: a malformed list (found when drawn).
    Layer,
    /// External GPU material drawn without a weld pool (see
    /// [`QuadRenderer::render_scene`]), or naming a weld the scene does not
    /// hold (found when drawn).
    External,
    /// Offscreen surface identities exhausted, or more target bytes than the budget allows (see
    /// [`QuadRenderer::set_offscreen_budget`]; found when drawn).
    Offscreen,
    /// A non-finite transform.
    NonFinite,
}

impl Reason {
    pub const ALL: [Self; 14] = [
        Self::Text,
        Self::TextTransform,
        Self::TextSize,
        Self::GlyphBudget,
        Self::Image,
        Self::ImageSize,
        Self::Vector,
        Self::Path,
        Self::PathBudget,
        Self::ClipDepth,
        Self::Layer,
        Self::External,
        Self::Offscreen,
        Self::NonFinite,
    ];
}

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

/// How much of a paint list the pass can draw.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    /// Entries the pass draws, or accepts as drawing nothing (clip ends, a
    /// zero-area transform).
    pub supported: usize,
    /// Entries it cannot draw, per reason, indexed like [`Reason::ALL`].
    pub unsupported: [usize; Reason::ALL.len()],
}

impl Coverage {
    pub fn unsupported(&self, reason: Reason) -> usize {
        self.unsupported[reason as usize]
    }
    /// Every entry the pass can draw.
    pub fn complete(&self) -> bool {
        self.unsupported.iter().all(|&n| n == 0)
    }
}

/// How much of `paint` the pass can draw, under a caller transform that
/// keeps axes (a scale and a translation, as every host uses). A host that
/// rotates the whole scene turns square clips into general ones, which can
/// classify differently; [`QuadRenderer::render`] checks under the real one.
/// External material counts as drawable: [`QuadRenderer::render_scene`]
/// draws it.
pub fn classify(paint: &[Painted]) -> Coverage {
    let mut walk = Walk::default();
    walk.paths.begin();
    walk.begin();
    let mut scratch: Instance = [[0.; 4]; SLOTS];
    let mut coverage = Coverage::default();
    for p in paint {
        match walk.entry(p, Affine::IDENTITY, &mut scratch) {
            Ok(_) => coverage.supported += 1,
            Err(reason) => coverage.unsupported[reason as usize] += 1,
        }
    }
    coverage
}

/// Why [`QuadRenderer::render`] drew nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// Entry `index` of the list is something the pass cannot draw.
    Unsupported { index: usize, reason: Reason },
    /// The target size is zero or the transform is not finite.
    Target,
    /// The weld pool could not draw a material this frame.
    Weld(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported { index, reason } => {
                write!(
                    f,
                    "paint entry {index} is unsupported by the quad pass: {reason}"
                )
            }
            Self::Target => f.write_str("quad pass target is empty or its transform is not finite"),
            Self::Weld(e) => write!(f, "quad pass weld: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// What the last [`QuadRenderer::render`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Frame {
    /// Whether `render` encoded a render pass at all.
    pub drawn: bool,
    /// The device rect (`x y w h`) a partial frame redrew; `None` for a
    /// frame drawn whole or not at all.
    pub damage: Option<[u32; 4]>,
    /// Instances drawn.
    pub instances: usize,
    /// Draw calls, in every pass: one a pass, plus one each time an
    /// instance needs a different texture than its run holds.
    pub draws: usize,
    /// Render passes: the frame's, and each offscreen target's drawn this
    /// frame (a blurred backdrop takes three or four).
    pub passes: usize,
    /// Bytes the offscreen targets hold after the frame, kept and pooled.
    pub offscreen_bytes: u64,
    /// Glyph cells rasterised this frame.
    pub rasterised: u64,
    /// Bytes written to the atlas this frame, cells and clears.
    pub uploaded: usize,
    /// Bytes the glyph atlas texture holds.
    pub atlas_bytes: u64,
    /// Path records built this frame.
    pub paths_built: u64,
    /// Bytes written to the path texture this frame.
    pub path_uploaded: usize,
    /// Bytes the path texture holds.
    pub path_bytes: u64,
}

/// The default glyph atlas budget: four 1024-pixel pages.
pub const ATLAS_BUDGET: usize = 4 << 20;

const STRIDE: u64 = (SLOTS * 16) as u64;

/// The pass: one pipeline, one instance buffer that grows and is then
/// reused, a glyph atlas texture and one texture per image; offscreen
/// targets and their pipelines once a frame needs them.
pub struct QuadRenderer {
    pipeline: wgpu::RenderPipeline,
    /// The same pass unblended: it draws [`build::clear`] under a scissor.
    clear: wgpu::RenderPipeline,
    module: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    /// Every pass's uniform slot, at dynamic offsets: see [`Slots`].
    uniforms: wgpu::Buffer,
    slots: Slots,
    view_layout: wgpu::BindGroupLayout,
    image_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bind: wgpu::BindGroup,
    /// A 1x1 texture: what an unbound texture slot holds.
    blank: wgpu::TextureView,
    /// Group 1 for a run that samples no texture.
    no_image: wgpu::BindGroup,
    atlas: wgpu::Texture,
    paths: wgpu::Texture,
    instances: wgpu::Buffer,
    cpu: Instances,
    groups: Vec<wgpu::BindGroup>,
    offscreen: Option<Offscreen>,
    /// Host textures by key (see [`QuadRenderer::set_texture`]).
    hosts: FxHashMap<u64, Host>,
    /// Host textures set or removed since the last frame.
    touched: Vec<u64>,
    /// Each drawn weld texture's group, by allocation id.
    welds: FxHashMap<u64, wgpu::BindGroup>,
    /// Welds whose texture this frame drew anew, by key.
    redrawn: Vec<std::sync::Arc<str>>,
    frame: Frame,
    retained: Option<Retained>,
    /// Where every pipeline is built through, offscreen ones included.
    cache: Option<wgpu::PipelineCache>,
    /// A profiling hook: when set, the frame's first render pass writes its
    /// start and its last its end to queries 0 and 1 of this set (the device
    /// needs `TIMESTAMP_QUERY`).
    pub timestamps: Option<wgpu::QuerySet>,
}

struct Host {
    view: wgpu::TextureView,
    group: Option<wgpu::BindGroup>,
}

impl QuadRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self::with_budgets(device, format, ATLAS_BUDGET, PATH_BUDGET)
    }

    /// [`QuadRenderer::new`] whose pipelines, offscreen ones included, are
    /// built through `cache` (a device with `PIPELINE_CACHE`).
    pub fn with_cache(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        cache: Option<&wgpu::PipelineCache>,
    ) -> Self {
        Self::build(device, format, ATLAS_BUDGET, PATH_BUDGET, cache)
    }

    /// A pass whose glyph atlas holds at most `budget` bytes: square
    /// single-channel pages of up to 1024 pixels, as many as fit, at least
    /// one. Glyphs unused for 64 frames are evicted; a frame that needs more
    /// than fits empties the atlas and rasterises again.
    pub fn with_atlas_budget(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        budget: usize,
    ) -> Self {
        Self::with_budgets(device, format, budget, PATH_BUDGET)
    }

    /// A pass with a glyph atlas of `atlas` bytes (see
    /// [`QuadRenderer::with_atlas_budget`]) and a path texture of at most
    /// `paths` bytes. A path record no entry of the frame uses is reused
    /// when space runs short; a frame whose records do not fit empties the
    /// texture and builds them again.
    pub fn with_budgets(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        budget: usize,
        paths: usize,
    ) -> Self {
        Self::build(device, format, budget, paths, None)
    }

    fn build(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        budget: usize,
        paths: usize,
        cache: Option<&wgpu::PipelineCache>,
    ) -> Self {
        let limits = device.limits();
        let mut side = 1024.min(limits.max_texture_dimension_2d);
        while side > 64 && (side * side) as usize > budget {
            side /= 2;
        }
        let pages =
            (budget / (side * side) as usize).clamp(1, limits.max_texture_array_layers as usize);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mui quad"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let (view_layout, image_layout) = gpu::layouts(device);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mui quad"),
            bind_group_layouts: &[Some(&view_layout), Some(&image_layout)],
            immediate_size: 0,
        });
        let clear = gpu::pipeline(device, &module, &layout, format, None, cache);
        let pipeline = gpu::pipeline(device, &module, &layout, format, Some(gpu::OVER), cache);
        let uniforms = gpu::uniforms(device, 4);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mui quad image"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let atlas = atlas_texture(device, u32::from(side as u16), 1);
        let path_rows = path_texture(device, 1);
        let bind = view_group(
            device,
            &view_layout,
            &uniforms,
            &atlas,
            &path_rows,
            &sampler,
        );
        let blank =
            image_texture(device, 1, 1).create_view(&wgpu::TextureViewDescriptor::default());
        let no_image = image_group(device, &image_layout, &blank, &blank);
        let mut cpu = Instances::new(side as u16, pages);
        cpu.walk.max_image = limits.max_texture_dimension_2d;
        cpu.walk.paths = path::Store::new(paths, limits.max_texture_dimension_2d);
        Self {
            pipeline,
            clear,
            module,
            layout,
            uniforms,
            slots: Slots::default(),
            view_layout,
            image_layout,
            sampler,
            bind,
            blank,
            no_image,
            atlas,
            paths: path_rows,
            instances: instance_buffer(device, 256),
            cpu,
            groups: Vec::new(),
            offscreen: None,
            hosts: FxHashMap::default(),
            touched: Vec::new(),
            welds: FxHashMap::default(),
            redrawn: Vec::new(),
            frame: Frame::default(),
            retained: None,
            cache: cache.cloned(),
            timestamps: None,
        }
    }

    /// What the last frame drew, rasterised and uploaded.
    pub fn frame(&self) -> Frame {
        self.frame
    }

    /// Forget what the last target holds: the next frame redraws whole.
    /// Call it when something other than this pass drew on the target.
    pub fn invalidate(&mut self) {
        self.retained = None;
    }

    /// Hold at most `bytes` of offscreen target a frame (default
    /// [`OFFSCREEN_BUDGET`]): a frame asking for more is refused at the
    /// entry that crossed it, [`Reason::Offscreen`].
    pub fn set_offscreen_budget(&mut self, bytes: u64) {
        self.cpu.layers.budget = bytes;
    }

    /// Paint `texture` wherever the scene carries [`mui_scene::Image::texture`]
    /// with this `key`, as [`crate::effects::GpuRenderer::set_texture`]
    /// does: `Rgba8Unorm`, straight alpha, `TEXTURE_BINDING`. It is sampled
    /// in place, never copied; call again after drawing into it, so the
    /// next frame redraws what samples it. An image whose key is not set
    /// paints its solid stand-in.
    pub fn set_texture(&mut self, key: u64, texture: &wgpu::Texture) {
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.hosts.insert(key, Host { view, group: None });
        self.cpu
            .walk
            .hosted
            .insert(key, [texture.width(), texture.height()]);
        self.touched.push(key);
    }

    pub fn remove_texture(&mut self, key: u64) {
        if self.hosts.remove(&key).is_some() {
            self.cpu.walk.hosted.remove(&key);
            self.touched.push(key);
        }
    }

    /// Draw `paint` on `target` (`size` pixels, cleared to transparent
    /// first) under `transform`, the transform [`crate::paint`] takes: the
    /// device scale, usually. Every entry is checked first, so a list with
    /// an unsupported entry draws nothing and names the first such entry.
    /// External material is [`Reason::External`] here: see
    /// [`QuadRenderer::render_scene`].
    ///
    /// The pass assumes `target` still holds what it drew last, when it is
    /// the same view (see [`QuadRenderer::invalidate`]). Then a frame whose
    /// list, transform and size are unchanged draws nothing, and one where
    /// entries changed in place redraws only the device rect they cover,
    /// old and new, under a scissor; any other frame redraws whole. A kept
    /// offscreen target (a group layer, a blurred backdrop) is drawn again
    /// only when something it shows changed.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        paint: &[Painted],
        transform: Affine,
        target: &wgpu::TextureView,
        size: [u32; 2],
    ) -> Result<(), Error> {
        self.draw(device, queue, paint, None, transform, target, size)
    }

    /// [`QuadRenderer::render`] of `scene`'s paint list, with its welds:
    /// each visible weld's material is drawn into its texture in `welds`
    /// (the weld pass `GpuRenderer` uses) when it changed, and sampled
    /// straight from there in its slot. A weld that changed redraws its
    /// slot, and whatever samples it: a backdrop over it, a layer it is in.
    /// Returns the weld pool's account of the frame.
    #[expect(
        clippy::too_many_arguments,
        reason = "render's arguments and the weld pool"
    )]
    pub fn render_scene(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &ResolvedScene,
        welds: &mut WeldTextures,
        transform: Affine,
        target: &wgpu::TextureView,
        size: [u32; 2],
    ) -> Result<crate::effects::EffectStats, Error> {
        if size.contains(&0) || !transform.as_coeffs().iter().all(|v| v.is_finite()) {
            return Err(Error::Target);
        }
        let wanted: Vec<_> = visible_welds(scene, transform, size).collect();
        let weld = |e: crate::effects::Error| Error::Weld(e.to_string());
        let mut stats = welds.begin(wanted.iter().copied()).map_err(weld)?;
        welds.forget_absent(|k| scene.external_weld(k).is_some());
        self.redrawn.clear();
        let mut encoder = None;
        for &(key, material) in &wanted {
            if welds.is_current(key, material) || self.redrawn.iter().any(|k| **k == *key) {
                continue;
            }
            let encoder = encoder.get_or_insert_with(|| {
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("mui quad welds"),
                })
            });
            if let Err(e) = welds.encode(key, material, encoder, &mut stats) {
                welds.abort();
                return Err(weld(e));
            }
            self.redrawn.push(key.into());
        }
        if let Some(encoder) = encoder {
            queue.submit([encoder.finish()]);
        }
        welds.commit_submitted(&mut stats);
        let pool = &*welds;
        let welds = Some(Welds { scene, pool });
        let done = self.draw(device, queue, &scene.paint, welds, transform, target, size);
        self.redrawn.clear();
        done.map(|()| stats)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "render's arguments and the welds"
    )]
    fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        paint: &[Painted],
        welds: Option<Welds>,
        transform: Affine,
        target: &wgpu::TextureView,
        size: [u32; 2],
    ) -> Result<(), Error> {
        if size.contains(&0) || !transform.as_coeffs().iter().all(|v| v.is_finite()) {
            return Err(Error::Target);
        }
        let kept = self
            .retained
            .take()
            .filter(|r| r.transform == transform && r.size == size && r.target == *target);
        self.frame = Frame {
            atlas_bytes: self.frame.atlas_bytes,
            path_bytes: self.frame.path_bytes,
            offscreen_bytes: self.frame.offscreen_bytes,
            ..Frame::default()
        };
        let touched = std::mem::take(&mut self.touched);
        if kept.as_ref().is_some_and(|r| r.paint == paint)
            && touched.is_empty()
            && self.redrawn.is_empty()
        {
            self.retained = kept;
            return Ok(());
        }
        self.cpu.walk.size = size;
        self.cpu.begin(paint.len());
        let old = kept.as_ref().map(|r| &r.paint[..]);
        let layered = paint.iter().any(|p| {
            matches!(
                p.layer,
                Layer::Blend { .. }
                    | Layer::Unblend
                    | Layer::Mask
                    | Layer::Backdrop
                    | Layer::External
            )
        });
        let damage = if layered {
            self.layered(paint, old, transform, size, welds, &touched)?
        } else {
            let damage = match old {
                Some(old) => self.damage(old, paint, transform, size, &touched)?,
                None => None,
            };
            let keep = |rect: [f32; 4]| move |_: usize, b: [f32; 4]| build::overlaps(b, rect);
            match damage {
                Some([x, y, w, h]) => {
                    let [x, y, w, h] = [x, y, w, h].map(|v| v as f32);
                    let rect = keep([x, y, x + w, y + h]);
                    self.cpu.build(paint, transform, &rect, None)?
                }
                None => self.cpu.build(paint, transform, &|_, _| true, None)?,
            };
            damage
        };
        let count = self.cpu.list.len();
        let mut retained = kept.map(|r| r.paint).unwrap_or_default();
        retained.clear();
        retained.extend_from_slice(paint);
        self.retained = Some(Retained {
            paint: retained,
            transform,
            size,
            target: target.clone(),
        });
        self.upload_glyphs(device, queue);
        self.upload_paths(device, queue);
        let layers = &self.cpu.layers;
        if layers.offscreen() && self.offscreen.is_none() {
            let blank = self.blank.clone();
            let cache = self.cache.as_ref();
            self.offscreen = Some(crate::effects::gate(device, || {
                Offscreen::new(
                    device,
                    &self.module,
                    &self.layout,
                    &self.image_layout,
                    blank,
                    cache,
                )
            }));
        }
        if let Some(off) = &mut self.offscreen {
            off.prepare(device, &self.cpu.layers);
        }
        self.bind_runs(device, queue);
        let layers = &self.cpu.layers;
        let offscreen = layers
            .surfaces
            .iter()
            .skip(1)
            .any(|s| s.dirty && s.kind != Kind::Folded);
        if damage.is_some_and(|d| d[2] == 0 || d[3] == 0) && !offscreen {
            // Changed entries that cover no pixel.
            if let Some(off) = &mut self.offscreen {
                off.finish(&self.cpu.layers);
            }
            return Ok(());
        }
        self.cpu.list.push(build::clear(size));
        self.cpu.encode();
        if self.instances.size() < self.cpu.bytes.len() as u64 {
            let want = (count as u64 + 1).next_power_of_two();
            self.instances = instance_buffer(device, want);
        }
        queue.write_buffer(&self.instances, 0, &self.cpu.bytes);
        // Every pass's slot: the frame's, then at most four a target.
        let slots = 1 + 4 * self.cpu.layers.surfaces.len() as u64;
        if self.uniforms.size() < slots * offscreen::SLOT {
            self.uniforms = gpu::uniforms(device, slots.next_power_of_two());
            self.bind = view_group(
                device,
                &self.view_layout,
                &self.uniforms,
                &self.atlas,
                &self.paths,
                &self.sampler,
            );
        }
        self.slots.bytes.clear();
        let [w, h] = size.map(|v| v as f32);
        self.slots.push([w, h, 0., 0., 0., 0., 0., 0.]);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("mui quad"),
        });
        let draw = Draw {
            runs: &self.cpu.layers.runs,
            groups: &self.groups,
            count: count as u32,
            instances: self.instances.slice(..self.cpu.bytes.len() as u64),
            view: &self.bind,
            uniforms: &self.uniforms,
            stamp: std::cell::Cell::new(self.timestamps.as_ref()),
        };
        let (passes, mut draws) = match &mut self.offscreen {
            Some(off) => off.encode(
                device,
                &mut encoder,
                &self.cpu.layers,
                &draw,
                &mut self.slots,
            ),
            None => (0, 0),
        };
        if damage.is_none_or(|d| d[2] > 0 && d[3] > 0) {
            let first = draw.stamp.take().is_some();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mui quad"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: match damage {
                            Some(_) => wgpu::LoadOp::Load,
                            None => wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: self.timestamps.as_ref().map(|query_set| {
                    wgpu::RenderPassTimestampWrites {
                        query_set,
                        beginning_of_pass_write_index: first.then_some(0),
                        end_of_pass_write_index: Some(1),
                    }
                }),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.bind, &[0]);
            pass.set_bind_group(1, &self.no_image, &[]);
            pass.set_vertex_buffer(0, draw.instances);
            if let Some([x, y, w, h]) = damage {
                pass.set_scissor_rect(x, y, w, h);
                pass.set_pipeline(&self.clear);
                pass.draw(0..4, count as u32..count as u32 + 1);
            }
            draws += draw.runs(&mut pass, &self.pipeline, None, u32::MAX, |r| {
                r.surface == 0
            });
            self.frame.passes = passes + 1;
        } else {
            self.frame.passes = passes;
        }
        queue.write_buffer(&self.uniforms, 0, &self.slots.bytes);
        queue.submit([encoder.finish()]);
        if let Some(off) = &mut self.offscreen {
            off.finish(&self.cpu.layers);
            self.frame.offscreen_bytes = off.bytes();
        }
        self.frame.drawn = true;
        self.frame.damage = damage;
        self.frame.instances = count;
        self.frame.draws = draws;
        Ok(())
    }
}

/// The welds of `scene` that land on a `size` target under `transform`:
/// the only ones [`QuadRenderer::render_scene`] draws.
pub(crate) fn visible_welds(
    scene: &ResolvedScene,
    transform: Affine,
    size: [u32; 2],
) -> impl Iterator<Item = (&str, &mui_weld::analytic::AnalyticWeld)> {
    let screen = crate::kurbo::Rect::new(0., 0., f64::from(size[0]), f64::from(size[1]));
    scene
        .paint
        .iter()
        .filter(|p| p.layer == Layer::External)
        .filter_map(move |p| {
            let e = scene.external_weld(&p.key)?;
            let b = e.bounds();
            let local = crate::kurbo::Rect::new(b.x0, b.y0, b.x1, b.y1);
            let on = crate::placed(transform, p)
                .transform_rect_bbox(local)
                .overlaps(screen);
            on.then_some((p.key.as_ref(), &*e.material))
        })
}
