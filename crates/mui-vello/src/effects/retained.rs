use super::{Budget, Converted, EffectStats, Error, WeldTextures};
use crate::classic::{Classic, Textures};
use crate::damage::{Change, NOTHING, PARTIAL, PackedRegions, Regions, paint_damage, reach};
use crate::{
    Cache, Canvas as _,
    kurbo::{Affine, Rect, Shape as _, Stroke},
};
use mui_scene::{ExternalWeld, Layer, Painted, ResolvedScene};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// Vello's compute pipelines and atlas are device-wide; retained encodings and
/// targets stay per window. The short queue-submit critical section is serialized.
#[derive(Clone)]
pub(crate) struct SharedVello {
    renderer: Arc<Mutex<vello::Renderer>>,
    pending: Arc<Mutex<Vec<ImageUpdate>>>,
    passes: Arc<[Passes; 2]>,
    weld: Arc<WeldTextures>,
}
enum ImageUpdate {
    Override(
        ImageData,
        Option<wgpu::TexelCopyTextureInfoBase<wgpu::Texture>>,
    ),
    Dirty(ImageData),
}
impl SharedVello {
    fn override_image(
        &self,
        image: &ImageData,
        texture: Option<wgpu::TexelCopyTextureInfoBase<wgpu::Texture>>,
    ) {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(ImageUpdate::Override(image.clone(), texture));
    }
    fn mark_override_image_dirty(&self, image: &ImageData) {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(ImageUpdate::Dirty(image.clone()));
    }
    fn passes(&self, device: &wgpu::Device, format: wgpu::TextureFormat) -> Passes {
        let index = usize::from(format == wgpu::TextureFormat::Bgra8Unorm);
        self.passes[index].for_window(device)
    }
}
use vello::peniko::{self, Blob, ImageAlphaType, ImageData, ImageFormat};

/// Classic Vello on the host's device, retained: the frame is encoded once
/// per change to the paint list and rendered on the GPU, and a frame that
/// changed nothing is one present pass of the texture already holding it.
///
/// Nothing crosses back to the CPU. Weld materials, blurred backdrops and
/// the host's own textures ([`GpuRenderer::set_texture`]) are copied into
/// Vello's image atlas GPU-side, so one device serves the UI and whatever
/// else the host draws with it.
///
/// Device and queue ownership is per renderer. Recreate it on device loss.
pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    vello: SharedVello,
    scene: vello::Scene,
    damage_scene: vello::Scene,
    /// A backdrop's prefix, encoded apart so it can be rendered first.
    prefix: vello::Scene,
    cache: Cache,
    effects: WeldTextures,
    /// Weld allocation id -> the image Vello samples it through.
    welds: HashMap<u64, ImageData>,
    /// The host's textures, by the key an `Image::texture` names.
    textures: Textures,
    passes: Passes,
    target: Target,
    backdrops: Vec<Backdrop>,
    size: [u32; 2],
    retained: Vec<Painted>,
    // External paint entries are stable while their material changes. Keep
    // both placements so damage includes pixels vacated by a moving weld.
    retained_external: HashMap<mui_scene::Id, Rect>,
    external: HashMap<mui_scene::Id, Rect>,
    drawn: Vec<ImageData>,
    live_welds: HashSet<u64>,
    paths: Converted,
    /// Where a short canvas path is converted on its way into the scene.
    short: crate::kurbo::BezPath,
    transform: Option<Affine>,
    /// Bumped by anything that changes which image an entry samples.
    mapping: (u64, u64),
    local_mapping: u64,
    /// The target holds `retained` under `transform`.
    valid: bool,
    /// The encoding is the whole frame, not a damaged corner of it.
    whole: bool,
    /// Where a partial frame renders, up to its far corner, before its box
    /// is copied into `target`.
    // ponytail: grows only, like `target`.
    patch: Option<wgpu::Texture>,
    /// A texture this renderer samples changed since the last render.
    stale: bool,
    /// The view the target was last presented to.
    presented: Option<wgpu::TextureView>,
    incremental_output: bool,
    output: Option<wgpu::TextureView>,
    damage: Vec<[u32; 4]>,
    /// The present writes straight alpha: see [`GpuRenderer::set_straight_alpha`].
    straight: bool,
    format: wgpu::TextureFormat,
}

/// Vello's output: compute shaders write only a storage texture, so the
/// frame lands here and one fragment pass puts it on the surface.
///
/// Its corner is the frame: the texture only ever grows, so a window
/// dragged smaller and back allocates nothing.
// ponytail: never shrinks; the largest size seen stays allocated.
struct Target {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind: wgpu::BindGroup,
    size: [u32; 2],
}

struct Passes {
    layout: wgpu::BindGroupLayout,
    blur: wgpu::RenderPipeline,
    present: wgpu::RenderPipeline,
    /// The present for a surface that composites straight alpha.
    present_straight: wgpu::RenderPipeline,
    /// The present's uniform: only its frame size is read.
    frame: wgpu::Buffer,
}

/// One backdrop's textures: Vello renders what is under it into `prefix`,
/// at `1 / scale` of the device, and two Gaussian passes leave it in `out`.
struct Backdrop {
    size: [u32; 2],
    prefix: wgpu::TextureView,
    tmp: wgpu::TextureView,
    out: wgpu::TextureView,
    image: ImageData,
    uniforms: [wgpu::Buffer; 2],
    binds: [wgpu::BindGroup; 2],
}

/// A texture Vello never reads the pixels of: an override stands in for it.
fn stand_in(size: [u32; 2], alpha_type: ImageAlphaType) -> ImageData {
    ImageData {
        data: Blob::new(Arc::new([0u8; 0])),
        format: ImageFormat::Rgba8,
        alpha_type,
        width: size[0],
        height: size[1],
    }
}

fn whole(texture: &wgpu::Texture) -> wgpu::TexelCopyTextureInfoBase<wgpu::Texture> {
    wgpu::TexelCopyTextureInfoBase {
        texture: texture.clone(),
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
    }
}

fn texture(
    device: &wgpu::Device,
    label: &str,
    size: [u32; 2],
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage,
        view_formats: &[],
    })
}

fn bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    uniform: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: uniform.as_entire_binding(),
            },
        ],
    })
}

fn uniform(device: &wgpu::Device) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("MUI blur"),
        size: 32,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl Passes {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("MUI present"),
            source: wgpu::ShaderSource::Wgsl(include_str!("present.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("MUI present"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
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
                    targets: &[Some(format.into())],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            blur: pipeline("fs_blur", wgpu::TextureFormat::Rgba8Unorm),
            present: pipeline("fs_present", format),
            present_straight: pipeline("fs_present_straight", format),
            frame: uniform(device),
            layout,
        }
    }

    fn for_window(&self, device: &wgpu::Device) -> Self {
        Self {
            layout: self.layout.clone(),
            blur: self.blur.clone(),
            present: self.present.clone(),
            present_straight: self.present_straight.clone(),
            frame: uniform(device),
        }
    }

    fn draw_damage(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        bind: &wgpu::BindGroup,
        view: &wgpu::TextureView,
        rects: &[[u32; 4]],
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("MUI scissored bitmap output"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind, &[]);
        for &[x, y, w, h] in rects {
            pass.set_scissor_rect(x, y, w, h);
            pass.draw(0..3, 0..1);
        }
    }

    fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        bind: &wgpu::BindGroup,
        view: &wgpu::TextureView,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("MUI pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

fn checked_size(device: &wgpu::Device, size: [u32; 2]) -> Result<(), Error> {
    let max = device.limits().max_texture_dimension_2d;
    if size.contains(&0) || size.iter().any(|s| *s > max) {
        return Err(Error::Budget("target dimensions"));
    }
    Ok(())
}

fn retained_target_size(have: [u32; 2], wanted: [u32; 2]) -> [u32; 2] {
    let grown = [have[0].max(wanted[0]), have[1].max(wanted[1])];
    let pixels = |size: [u32; 2]| u64::from(size[0]) * u64::from(size[1]);
    // Retain small resize slack, but reclaim crossed-aspect or oversized targets.
    if pixels(grown) > pixels(wanted).saturating_mul(2) {
        wanted
    } else {
        grown
    }
}

fn visible(e: &ExternalWeld, xf: Affine, size: [u32; 2]) -> bool {
    let b = e.bounds();
    let r = xf.transform_rect_bbox(b);
    r.x1 > 0. && r.y1 > 0. && r.x0 < f64::from(size[0]) && r.y0 < f64::from(size[1])
}

fn external_visible(
    scene: &ResolvedScene,
    key: &str,
    e: &ExternalWeld,
    xf: Affine,
    size: [u32; 2],
) -> bool {
    let pose = scene.surface(key).map_or(Affine::IDENTITY, |s| s.transform);
    visible(e, xf * pose, size)
}

fn render(
    vello: &mut SharedVello,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scene: &vello::Scene,
    view: &wgpu::TextureView,
    [width, height]: [u32; 2],
) -> Result<(), Error> {
    let mut renderer = match vello.renderer.try_lock() {
        Ok(renderer) => renderer,
        Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return Err(Error::Busy),
    };
    for update in vello
        .pending
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .drain(..)
    {
        match update {
            ImageUpdate::Override(image, texture) => {
                renderer.override_image(&image, texture);
            }
            ImageUpdate::Dirty(image) => renderer.mark_override_image_dirty(&image),
        }
    }
    renderer
        .render_to_texture(
            device,
            queue,
            scene,
            view,
            &vello::RenderParams {
                base_color: peniko::Color::TRANSPARENT,
                width,
                height,
                antialiasing_method: vello::AaConfig::Area,
            },
        )
        .map_err(|e| Error::Render(e.to_string()))
}

impl Drop for GpuRenderer {
    fn drop(&mut self) {
        // Queue removals instead of waiting for another editor's driver call.
        for image in self
            .welds
            .values()
            .chain(self.textures.values())
            .chain(self.backdrops.iter().map(|b| &b.image))
        {
            self.vello.override_image(image, None);
        }
    }
}

impl GpuRenderer {
    pub async fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        size: [u32; 2],
        budget: Budget,
    ) -> Result<Self, Error> {
        Self::new_shared(device, queue, format, size, budget, None).await
    }

    pub(crate) fn shared_vello(&self) -> SharedVello {
        self.vello.clone()
    }

    pub(crate) async fn new_shared(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        size: [u32; 2],
        budget: Budget,
        shared: Option<SharedVello>,
    ) -> Result<Self, Error> {
        checked_size(device, size)?;
        // What leaves the present pass is sRGB-encoded (and premultiplied,
        // unless `set_straight_alpha`); an
        // sRGB attachment would encode it a second time on store.
        if format.is_srgb() {
            return Err(Error::Unsupported("target must be a non-sRGB format"));
        }
        let effects = match &shared {
            Some(shared) => shared.weld.for_window(budget)?,
            None => WeldTextures::new(device, queue, budget).await?,
        };
        let vello = if let Some(shared) = shared {
            shared
        } else {
            let cache = crate::host::pipeline_cache::load(device);
            let renderer = vello::Renderer::new(
                device,
                vello::RendererOptions {
                    use_cpu: false,
                    antialiasing_support: vello::AaSupport::area_only(),
                    // Bound compile parallelism so opening an editor does not
                    // saturate the DAW's audio cores.
                    num_init_threads: std::num::NonZeroUsize::new(2),
                    pipeline_cache: cache.as_ref().map(|c| c.cache.clone()),
                },
            )
            .map_err(|e| Error::Device(e.to_string()))?;
            if let Some(cache) = cache {
                cache.save();
            }
            SharedVello {
                weld: Arc::new(effects.for_window(budget)?),
                renderer: Arc::new(Mutex::new(renderer)),
                pending: Arc::new(Mutex::new(Vec::new())),
                passes: Arc::new([
                    Passes::new(device, wgpu::TextureFormat::Rgba8Unorm),
                    Passes::new(device, wgpu::TextureFormat::Bgra8Unorm),
                ]),
            }
        };
        let passes = vello.passes(device, format);
        let target = Self::target(device, &passes, size);
        let renderer = Self {
            device: device.clone(),
            queue: queue.clone(),
            vello,
            scene: vello::Scene::new(),
            damage_scene: vello::Scene::new(),
            prefix: vello::Scene::new(),
            cache: Cache::default(),
            effects,
            welds: HashMap::new(),
            textures: Textures::new(),
            passes,
            target,
            backdrops: Vec::new(),
            size,
            retained: Vec::new(),
            retained_external: HashMap::new(),
            external: HashMap::new(),
            drawn: Vec::new(),
            live_welds: HashSet::new(),
            paths: Converted::default(),
            short: crate::kurbo::BezPath::default(),
            transform: None,
            mapping: (0, 0),
            local_mapping: 0,
            valid: false,
            whole: false,
            patch: None,
            stale: false,
            presented: None,
            incremental_output: false,
            output: None,
            damage: Vec::new(),
            straight: false,
            format,
        };
        renderer.write_frame();
        Ok(renderer)
    }

    pub(crate) fn set_surface_format(&mut self, format: wgpu::TextureFormat) {
        if self.format == format {
            return;
        }
        self.format = format;
        self.passes = self.vello.passes(&self.device, format);
        self.target = Self::target(&self.device, &self.passes, self.size);
        self.write_frame();
        self.invalidate();
    }

    /// Tell the present pass the frame's size: past it, it writes clear.
    fn write_frame(&self) {
        let mut bytes = [0u8; 32];
        bytes[24..28].copy_from_slice(&self.size[0].to_le_bytes());
        bytes[28..32].copy_from_slice(&self.size[1].to_le_bytes());
        self.queue.write_buffer(&self.passes.frame, 0, &bytes);
    }

    /// Present straight alpha rather than premultiplied: for a surface
    /// configured `CompositeAlphaMode::PostMultiplied`, which composites
    /// its colour as it stands. Off by default.
    pub fn set_straight_alpha(&mut self, straight: bool) {
        if straight != self.straight {
            self.straight = straight;
            self.presented = None;
        }
    }

    fn target(device: &wgpu::Device, passes: &Passes, size: [u32; 2]) -> Target {
        use wgpu::TextureUsages as U;
        let texture = texture(
            device,
            "MUI frame",
            size,
            U::STORAGE_BINDING | U::TEXTURE_BINDING | U::COPY_DST,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Target {
            bind: bind(device, &passes.layout, &view, &passes.frame),
            texture,
            view,
            size,
        }
    }

    pub fn resize(&mut self, size: [u32; 2]) -> Result<(), Error> {
        checked_size(&self.device, size)?;
        if size != self.size {
            let have = self.target.size;
            let capacity = retained_target_size(have, size);
            if capacity != have {
                self.target = Self::target(&self.device, &self.passes, capacity);
            }
            // An allocation panic must leave the old size intact so a caught
            // failure cannot make the next resize skip the required allocation.
            self.size = size;
            // Scratch belongs to this viewport too; an earlier giant or wide
            // window must not pin a mostly unused damage texture indefinitely.
            if self.patch.as_ref().is_some_and(|p| {
                p.width() > capacity[0]
                    || p.height() > capacity[1]
                    || u64::from(p.width()) * u64::from(p.height())
                        > 2 * u64::from(size[0]) * u64::from(size[1])
            }) {
                self.patch = None;
            }
            self.write_frame();
            self.invalidate();
        }
        Ok(())
    }

    /// Throw the encoding away; images, glyphs and weld textures stay.
    pub fn invalidate(&mut self) {
        self.valid = false;
        self.presented = None;
    }

    /// Paint `texture` wherever the scene carries [`mui_scene::Image::texture`]
    /// with this `key`. It must be `Rgba8Unorm` with straight alpha and
    /// `COPY_SRC`. Call again after drawing into it: the copy into Vello's
    /// atlas is only refreshed when told to. A frame that draws into the
    /// texture and calls this re-renders even if the scene did not change.
    pub fn set_texture(&mut self, key: u64, texture: &wgpu::Texture) {
        let size = [texture.width(), texture.height()];
        match self.textures.get(&key) {
            Some(image) if [image.width, image.height] == size => {
                // Same stand-in: re-point it, which also marks it dirty.
                self.vello.override_image(image, Some(whole(texture)));
            }
            _ => {
                let image = stand_in(size, ImageAlphaType::Alpha);
                self.vello.override_image(&image, Some(whole(texture)));
                if let Some(old) = self.textures.insert(key, image) {
                    self.vello.override_image(&old, None);
                }
                self.local_mapping += 1;
            }
        }
        self.stale = true;
    }

    pub fn remove_texture(&mut self, key: u64) {
        if let Some(old) = self.textures.remove(&key) {
            self.vello.override_image(&old, None);
            self.local_mapping += 1;
            self.stale = true;
        }
    }

    /// The surface last got `resolved` under `xf`, and nothing it samples
    /// changed since: presenting it again draws the same pixels, so a host
    /// can skip acquiring a surface texture at all. Never with welds, whose
    /// materials may animate on their own.
    pub fn is_current(&self, resolved: &ResolvedScene, xf: Affine) -> bool {
        self.presented.is_some()
            && !self.stale
            && self.transform == Some(xf)
            && self.mapping == (self.effects.mapping_revision(), self.local_mapping)
            && self.retained == resolved.paint
            && resolved
                .external_welds()
                .filter(|(k, e)| external_visible(resolved, k, e, xf, self.size))
                .all(|(key, e)| self.effects.is_current(key, &e.material))
            && resolved
                .paint
                .iter()
                .filter(|p| p.layer == Layer::External)
                .all(|p| {
                    resolved.external_weld(&p.key).is_some_and(|e| {
                        self.retained_external.get(&p.key)
                            == Some(&p.transform.transform_rect_bbox(
                                e.bounds() + crate::kurbo::Vec2::new(p.offset.x, p.offset.y),
                            ))
                    })
                })
    }

    pub fn render(
        &mut self,
        resolved: &ResolvedScene,
        xf: Affine,
        target: &wgpu::TextureView,
    ) -> Result<EffectStats, Error> {
        self.render_inner::<fn(&mut Classic<'_>)>(resolved, xf, target, None)
    }

    /// Debug overlays are never retained: the next plain frame re-encodes.
    pub fn render_with_overlay<F: FnOnce(&mut Classic<'_>)>(
        &mut self,
        resolved: &ResolvedScene,
        xf: Affine,
        target: &wgpu::TextureView,
        overlay: F,
    ) -> Result<EffectStats, Error> {
        self.render_inner(resolved, xf, target, Some(overlay))
    }

    /// Enable dirty-rectangle output to a persistent offscreen target.
    pub(crate) fn incremental_output(&mut self) {
        self.incremental_output = true;
    }
    /// Device rectangles actually changed by the last successful render.
    pub fn damage(&self) -> &[[u32; 4]] {
        &self.damage
    }

    fn render_inner<F: FnOnce(&mut Classic<'_>)>(
        &mut self,
        resolved: &ResolvedScene,
        xf: Affine,
        target: &wgpu::TextureView,
        overlay: Option<F>,
    ) -> Result<EffectStats, Error> {
        if xf.as_coeffs().iter().any(|v| !v.is_finite()) {
            return Err(Error::Unsupported("nonfinite scene transform"));
        }
        self.damage.clear();
        let size = self.size;
        let wanted = resolved
            .external_welds()
            .filter(move |(k, e)| external_visible(resolved, k, e, xf, size))
            .map(|(k, e)| (k, &*e.material));
        let mut stats = self.effects.begin(wanted)?;
        self.effects
            .forget_absent(|k| resolved.external_weld(k).is_some());
        let result = self.frame(resolved, xf, target, overlay, &mut stats);
        match result {
            Ok(()) => {
                self.effects.commit_submitted(&mut stats);
                Ok(stats)
            }
            Err(e) => {
                self.effects.abort();
                self.invalidate();
                Err(e)
            }
        }
    }

    fn frame<F: FnOnce(&mut Classic<'_>)>(
        &mut self,
        resolved: &ResolvedScene,
        xf: Affine,
        target: &wgpu::TextureView,
        overlay: Option<F>,
        stats: &mut EffectStats,
    ) -> Result<(), Error> {
        let size = self.size;
        // Weld materials first, in their own submission: Vello's render
        // submits as it goes, and copies from these textures when it does.
        // No welds, no encoder.
        let mut encoder = None;
        self.drawn.clear();
        self.external.clear();
        let mut weld_damage = NOTHING;
        for p in resolved.paint.iter().filter(|p| p.layer == Layer::External) {
            let e = resolved
                .external_weld(&p.key)
                .ok_or_else(|| Error::Missing(p.key.to_string()))?;
            let b = p
                .transform
                .transform_rect_bbox(e.bounds() + crate::kurbo::Vec2::new(p.offset.x, p.offset.y));
            self.external.insert(p.key.clone(), b);
            if self.retained_external.get(&p.key) != Some(&b) {
                weld_damage = weld_damage.union(b);
                if let Some(old) = self.retained_external.get(&p.key) {
                    weld_damage = weld_damage.union(*old);
                }
            }
        }
        for (key, b) in &self.retained_external {
            if !self.external.contains_key(key) {
                weld_damage = weld_damage.union(*b);
            }
        }
        for (key, e) in resolved
            .external_welds()
            .filter(|(k, e)| external_visible(resolved, k, e, xf, size))
        {
            if self.effects.is_current(key, &e.material)
                && self
                    .effects
                    .texture(key)
                    .is_some_and(|(id, ..)| self.welds.contains_key(&id))
            {
                continue;
            }
            let before = stats.effect_draws;
            let encoder = encoder.get_or_insert_with(|| {
                self.device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("MUI welds"),
                    })
            });
            self.effects.encode(key, &e.material, encoder, stats)?;
            let (id, tex, used) = self
                .effects
                .texture(key)
                .ok_or_else(|| Error::Missing(key.to_string()))?;
            if stats.effect_draws != before {
                if let Some(b) = self.external.get(key) {
                    weld_damage = weld_damage.union(*b);
                }
                if let Some(b) = self.retained_external.get(key) {
                    weld_damage = weld_damage.union(*b);
                }
            }
            match self.welds.get(&id) {
                Some(image) if [image.width, image.height] == used => {
                    if stats.effect_draws != before {
                        self.drawn.push(image.clone());
                    }
                }
                _ => {
                    // The copy reads the image's size from the corner the
                    // material fills, not the whole bucketed texture.
                    let image = stand_in(used, ImageAlphaType::AlphaPremultiplied);
                    self.vello.override_image(&image, Some(whole(tex)));
                    if let Some(old) = self.welds.insert(id, image) {
                        self.vello.override_image(&old, None);
                    }
                    self.local_mapping += 1;
                }
            }
        }
        if let Some(encoder) = encoder.filter(|_| stats.effect_draws > 0) {
            self.queue.submit([encoder.finish()]);
            for image in &self.drawn {
                self.vello.mark_override_image_dirty(image);
            }
        }
        // A weld whose slot went: its stand-in goes too.
        self.live_welds.clear();
        self.live_welds.extend(
            resolved
                .external_welds()
                .filter_map(|(k, _)| self.effects.texture(k).map(|t| t.0)),
        );
        let live = &self.live_welds;
        let vello = &mut self.vello;
        let before = self.welds.len();
        self.welds.retain(|id, image| {
            let keep = live.contains(id);
            if !keep {
                vello.override_image(image, None);
            }
            keep
        });
        if self.welds.len() != before {
            self.local_mapping += 1;
        }

        // The retained backdrop cache owns a complete prefix encoding. A
        // changed prefix invalidates that encoding globally (including changes
        // outside the blur footprint); content above it keeps the cached blur.
        let backdrop_invalid = resolved.paint.iter().enumerate().any(|(i, p)| {
            p.layer == Layer::Backdrop && !self.backdrop_unchanged(&resolved.paint, i, p, xf)
        });
        let change =
            if overlay.is_some() || !self.valid || self.transform != Some(xf) || backdrop_invalid {
                Change::Full
            } else {
                match paint_damage(
                    &self.retained,
                    &resolved.paint,
                    &self.retained_external,
                    &self.external,
                    weld_damage,
                    xf,
                    self.size,
                ) {
                    // A host texture that changed may be sampled anywhere, and
                    // a partial encoding cannot render the frame again.
                    Change::Part(_) | Change::Parts(_) if self.stale => Change::Full,
                    Change::None if self.stale && !self.whole => Change::Full,
                    c => c,
                }
            };
        let packed = if let Change::Parts(regions) = &change {
            regions
                .packed(self.device.limits().max_texture_dimension_2d)
                .filter(|p| {
                    f64::from(p.size[0]) * f64::from(p.size[1])
                        <= PARTIAL * f64::from(size[0]) * f64::from(size[1])
                })
        } else {
            None
        };
        if !matches!(change, Change::None) {
            self.paths.tick();
            self.cache.tick();
        }
        match &change {
            Change::Full => {
                self.valid = false; // a partial encoding must never be reused
                self.presented = None;
                self.encode(resolved, xf, overlay, None)?;
                stats.encoded_scenes += 1;
            }
            Change::Part([.., 0] | [.., 0, _]) => {
                // Changed only off the target.
                self.retained.clone_from(&resolved.paint);
            }
            Change::Part(r) => {
                self.render_region::<F>(resolved, xf, *r, stats)?;
            }
            Change::Parts(regions) => {
                if let Some(packed) = packed {
                    self.render_regions::<F>(resolved, xf, regions, &packed, stats)?;
                } else {
                    for &rect in &regions.rects[..regions.len] {
                        self.render_region::<F>(resolved, xf, rect, stats)?;
                    }
                }
            }
            Change::None => {}
        }
        let full_output = matches!(change, Change::Full) || self.stale;
        if full_output {
            render(
                &mut self.vello,
                &self.device,
                &self.queue,
                &self.scene,
                &self.target.view,
                size,
            )?;
            self.stale = false;
            self.presented = None;
            stats.renders += 1;
            stats.rendered_pixels += u64::from(size[0]) * u64::from(size[1]);
        }
        self.damage =
            if full_output || (self.incremental_output && self.output.as_ref() != Some(target)) {
                vec![[0, 0, size[0], size[1]]]
            } else {
                match change {
                    Change::Full => vec![[0, 0, size[0], size[1]]],
                    Change::Part(r) if r[2] != 0 && r[3] != 0 => vec![r],
                    Change::Parts(r) => r.rects[..r.len].to_vec(),
                    _ => Vec::new(),
                }
            };
        if self.incremental_output {
            if !self.damage.is_empty() {
                let mut encoder =
                    self.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("MUI bitmap damage"),
                        });
                let present = if self.straight {
                    &self.passes.present_straight
                } else {
                    &self.passes.present
                };
                self.passes.draw_damage(
                    &mut encoder,
                    present,
                    &self.target.bind,
                    target,
                    &self.damage,
                );
                self.queue.submit([encoder.finish()]);
            }
            self.output = Some(target.clone());
            self.presented = Some(target.clone());
        } else if self.presented.as_ref() != Some(target) {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("MUI present"),
                });
            let present = if self.straight {
                &self.passes.present_straight
            } else {
                &self.passes.present
            };
            self.passes
                .draw(&mut encoder, present, &self.target.bind, target);
            self.queue.submit([encoder.finish()]);
            if self.valid {
                self.presented = Some(target.clone());
            }
        }
        std::mem::swap(&mut self.retained_external, &mut self.external);
        // Slot insertions/removals can leave every visible pixel unchanged.
        // Remember the observed mapping even when no encoding was necessary.
        self.mapping = (self.effects.mapping_revision(), self.local_mapping);
        Ok(())
    }

    /// One raster submission for all sparse edits. Each clipped region is
    /// translated to a compact atlas, then copied back to its original tile.
    fn render_regions<F: FnOnce(&mut Classic<'_>)>(
        &mut self,
        resolved: &ResolvedScene,
        xf: Affine,
        regions: &Regions,
        packed: &PackedRegions,
        stats: &mut EffectStats,
    ) -> Result<(), Error> {
        self.damage_scene.reset();
        self.presented = None;
        for (i, r) in regions.rects[..regions.len].iter().enumerate() {
            self.valid = false;
            self.encode::<F>(resolved, xf, None, Some(*r))?;
            stats.encoded_scenes += 1;
            let [x, y] = packed.offsets[i].map(f64::from);
            self.damage_scene.push_layer(
                crate::peniko::Fill::NonZero,
                crate::peniko::BlendMode::default(),
                1.,
                Affine::IDENTITY,
                &Rect::new(x, y, x + f64::from(r[2]), y + f64::from(r[3])),
            );
            self.damage_scene
                .append(&self.scene, Some(Affine::translate((x, y))));
            self.damage_scene.pop_layer();
        }
        let have = self
            .patch
            .as_ref()
            .map_or([0, 0], |p| [p.width(), p.height()]);
        let limit = self.device.limits().max_texture_dimension_2d;
        let wanted = packed.size.map(|v| v.next_multiple_of(256).min(limit));
        let capacity = retained_target_size(have, wanted);
        if capacity != have {
            use wgpu::TextureUsages as U;
            self.patch = Some(texture(
                &self.device,
                "MUI damage atlas",
                capacity,
                U::STORAGE_BINDING | U::COPY_SRC,
            ));
        }
        let patch = self.patch.as_ref().expect("allocated above");
        let view = patch.create_view(&wgpu::TextureViewDescriptor::default());
        render(
            &mut self.vello,
            &self.device,
            &self.queue,
            &self.damage_scene,
            &view,
            packed.size,
        )?;
        stats.renders += 1;
        stats.rendered_pixels += u64::from(packed.size[0]) * u64::from(packed.size[1]);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("MUI damage atlas copies"),
            });
        for (i, &[x, y, w, h]) in regions.rects[..regions.len].iter().enumerate() {
            let [sx, sy] = packed.offsets[i];
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    origin: wgpu::Origin3d { x: sx, y: sy, z: 0 },
                    ..patch.as_image_copy()
                },
                wgpu::TexelCopyTextureInfo {
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    ..self.target.texture.as_image_copy()
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.queue.submit([encoder.finish()]);
        Ok(())
    }

    /// Render and copy immediately: a following damage region can reuse the
    /// same scratch texture, while queue ordering preserves the prior pixels.
    fn render_region<F: FnOnce(&mut Classic<'_>)>(
        &mut self,
        resolved: &ResolvedScene,
        xf: Affine,
        r @ [x, y, w, h]: [u32; 4],
        stats: &mut EffectStats,
    ) -> Result<(), Error> {
        self.valid = false;
        self.presented = None;
        self.encode::<F>(resolved, xf, None, Some(r))?;
        stats.encoded_scenes += 1;
        if self
            .patch
            .as_ref()
            .is_none_or(|t| t.width() < w || t.height() < h)
        {
            use wgpu::TextureUsages as U;
            let have = self
                .patch
                .as_ref()
                .map_or([0, 0], |t| [t.width(), t.height()]);
            let grown = [0, 1].map(|i| {
                [w, h][i]
                    .max(have[i])
                    .next_multiple_of(256)
                    .min(self.target.size[i])
            });
            self.patch = Some(texture(
                &self.device,
                "MUI damage",
                grown,
                U::STORAGE_BINDING | U::COPY_SRC,
            ));
        }
        let patch = self.patch.as_ref().expect("allocated above");
        let view = patch.create_view(&wgpu::TextureViewDescriptor::default());
        render(
            &mut self.vello,
            &self.device,
            &self.queue,
            &self.scene,
            &view,
            [w, h],
        )?;
        stats.renders += 1;
        stats.rendered_pixels += u64::from(w) * u64::from(h);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("MUI damage copy"),
            });
        encoder.copy_texture_to_texture(
            patch.as_image_copy(),
            wgpu::TexelCopyTextureInfo {
                origin: wgpu::Origin3d { x, y, z: 0 },
                ..self.target.texture.as_image_copy()
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        Ok(())
    }

    /// The frame, or with `part` only what reaches `[x, y, w, h]` of it.
    /// A part never holds a backdrop or a weld.
    fn encode<F: FnOnce(&mut Classic<'_>)>(
        &mut self,
        resolved: &ResolvedScene,
        xf: Affine,
        overlay: Option<F>,
        part: Option<[u32; 4]>,
    ) -> Result<(), Error> {
        let size = self.size;
        // Backdrops first: each is a render of its own that the frame samples.
        let mut blurred = Vec::new();
        let mut k = 0;
        for (i, p) in resolved.paint.iter().enumerate() {
            if p.layer == Layer::Backdrop && p.blur.is_finite() && p.blur > 0.0 {
                let outline = self.paths.get(&p.path)?.clone();
                let reuse = self.backdrop_unchanged(&resolved.paint, i, p, xf);
                blurred.push(self.backdrop(k, &resolved.paint[..i], p, &outline, xf, reuse)?);
                k += 1;
            } else if crate::path_shadow(p) {
                // The same offscreen blur, over the path alone, sharp:
                // what fills its reach is the shadow.
                let path = self.paths.get(&p.path)?.clone();
                let sharp = Painted {
                    layer: Layer::Fill,
                    blur: 0.,
                    ..p.clone()
                };
                let reuse = self.backdrop_unchanged(&resolved.paint, i, p, xf);
                blurred.push(self.backdrop(
                    k,
                    std::slice::from_ref(&sharp),
                    p,
                    &path,
                    xf,
                    reuse,
                )?);
                k += 1;
            }
        }
        if part.is_none() {
            for backdrop in self.backdrops.drain(k..) {
                self.vello.override_image(&backdrop.image, None);
            }
        }

        // A part renders its box alone, moved to the origin by whole
        // pixels (and whole 16 px tiles): Vello's cost follows the target's
        // extent, so a box near the far corner must not render the frame.
        // At a fractional scale, moved f32 coordinates round apart from the
        // frame's by an ulp: an edge may land a level or a few off.
        let (size, cull, draw) = match part.map(|r| r.map(f64::from)) {
            Some([x, y, w, h]) => (
                [w as u32, h as u32],
                Some(
                    xf.inverse()
                        .transform_rect_bbox(Rect::new(x, y, x + w, y + h)),
                ),
                Affine::translate((-x, -y)) * xf,
            ),
            None => (size, None, xf),
        };
        self.scene.reset();
        let mut canvas = Classic::new(&mut self.scene, &mut self.cache, &self.textures, size);
        let mut blurred = blurred.into_iter();
        for p in &resolved.paint {
            canvas.set_transform(crate::placed(draw, p));
            match p.layer {
                Layer::External => {
                    let e = resolved
                        .external_weld(&p.key)
                        .ok_or_else(|| Error::Missing(p.key.to_string()))?;
                    if !visible(e, xf * p.transform, self.size) {
                        continue;
                    }
                    let image = self
                        .effects
                        .texture(&p.key)
                        .and_then(|(id, ..)| self.welds.get(&id))
                        .ok_or_else(|| Error::Missing(p.key.to_string()))?
                        .clone();
                    let b = e.bounds();
                    let r = b;
                    let local = Affine::translate((r.x0, r.y0))
                        * Affine::scale_non_uniform(
                            r.width() / f64::from(image.width),
                            r.height() / f64::from(image.height),
                        );
                    // In the paint list, under the clips and layers around
                    // it: a weld is not an overlay over menus and tooltips.
                    let brush = canvas.hand_out(image);
                    canvas.set_paint(brush);
                    canvas.set_paint_transform(local);
                    canvas.fill_path(&r.to_path(0.1));
                    canvas.reset_paint_transform();
                }
                Layer::Shadow(_) if crate::path_shadow(p) => {
                    let Some((image, brush_transform)) = blurred.next().flatten() else {
                        continue;
                    };
                    let reach = crate::blur_reach(p, self.paths.get(&p.path)?).to_path(0.1);
                    let brush = canvas.hand_out(image);
                    canvas.set_paint(brush);
                    canvas.set_paint_transform(brush_transform);
                    canvas.fill_path(&reach);
                    canvas.reset_paint_transform();
                }
                Layer::Backdrop => {
                    if !(p.blur.is_finite() && p.blur > 0.0) {
                        continue;
                    }
                    let Some((image, brush_transform)) = blurred.next().flatten() else {
                        continue;
                    };
                    let brush = canvas.hand_out(image);
                    canvas.set_paint(brush);
                    canvas.set_paint_transform(brush_transform);
                    canvas.fill_path(self.paths.get(&p.path)?);
                    canvas.reset_paint_transform();
                }
                _ => {
                    if crate::layered(&mut canvas, p) {
                        continue;
                    }
                    // Clips stay: the pops that close them are not culled.
                    let off = |c: Rect| reach(p).is_some_and(|r| !r.overlaps(c));
                    if p.layer != Layer::Clip && cull.is_some_and(off) {
                        continue;
                    }
                    if let Some(color) = crate::plain(p) {
                        let at = crate::placed(draw, p);
                        let long = p.path.commands.len() >= super::REPLAYED;
                        let stroke = p.width > 0.0;
                        // A glyph run or an icon only moves: encoded once.
                        if long && !stroke {
                            let fill = self.paths.fill(&p.path, color)?;
                            canvas.scene.append(fill, Some(at));
                            continue;
                        }
                        // A canvas draws fresh short paths most frames:
                        // converted in place, never kept.
                        if !long && matches!(p.layer, Layer::Draw(_)) {
                            let path = &mut self.short;
                            crate::bez_path_into(&p.path, crate::ARC_TOLERANCE, path)?;
                            let scene = &mut *canvas.scene;
                            match stroke {
                                true => scene.stroke(&Stroke::new(p.width), at, color, None, path),
                                false => scene.fill(peniko::Fill::NonZero, at, color, None, path),
                            }
                            continue;
                        }
                    }
                    crate::one(
                        &mut canvas,
                        p,
                        self.paths.get(&p.path)?,
                        crate::placed(xf, p),
                    )?;
                }
            }
        }
        if let Some(draw) = overlay {
            draw(&mut canvas);
        } else {
            self.retained.clone_from(&resolved.paint);
            self.transform = Some(xf);
            self.mapping = (self.effects.mapping_revision(), self.local_mapping);
            self.valid = true;
            self.whole = part.is_none();
        }
        Ok(())
    }

    /// Backdrop `k`: `below` rendered on its own around `p`'s `outline`,
    /// blurred by its `blur` scene units, as the image and the brush
    /// transform that put it back where it was, under `p`'s own placement.
    /// `None` when none of it is on the target.
    ///
    /// The prefix renders at a power-of-two fraction of the device -- a
    /// Gaussian that wide has nothing left at full resolution -- so the two
    /// passes after it never take more than 25 taps.
    fn backdrop_unchanged(&self, paint: &[Painted], i: usize, p: &Painted, xf: Affine) -> bool {
        self.transform == Some(xf)
            && !self.stale
            && self.mapping == (self.effects.mapping_revision(), self.local_mapping)
            && self.retained.get(..i).is_some_and(|old| old == &paint[..i])
            && self.retained.get(i).is_some_and(|old| old == p)
    }

    fn backdrop(
        &mut self,
        k: usize,
        below: &[Painted],
        p: &Painted,
        outline: &crate::kurbo::BezPath,
        xf: Affine,
        reuse: bool,
    ) -> Result<Option<(ImageData, Affine)>, Error> {
        let sigma = p.blur;
        let sigma_device = sigma * xf.determinant().abs().sqrt();
        let scale = (sigma_device / 4.).ceil().max(1.) as u32;
        let scale = f64::from(scale.next_power_of_two());
        let reach = crate::scene_box(p, outline).inflate(3. * sigma, 3. * sigma);
        let device = xf.transform_rect_bbox(reach).intersect(Rect::new(
            0.,
            0.,
            f64::from(self.size[0]),
            f64::from(self.size[1]),
        ));
        if device.width() <= 0. || device.height() <= 0. {
            return Ok(None);
        }
        let origin = (device.x0.floor(), device.y0.floor());
        let size = [
            ((device.x1 - origin.0) / scale).ceil().max(1.) as u32,
            ((device.y1 - origin.1) / scale).ceil().max(1.) as u32,
        ];
        let to_prefix = Affine::scale(1. / scale) * Affine::translate((-origin.0, -origin.1)) * xf;
        let brush =
            crate::placed(xf, p).inverse() * Affine::translate(origin) * Affine::scale(scale);

        if reuse && let Some(b) = self.backdrops.get(k).filter(|b| b.size == size) {
            return Ok(Some((b.image.clone(), brush)));
        }

        self.prefix.reset();
        let mut canvas = Classic::new(&mut self.prefix, &mut self.cache, &self.textures, size);
        crate::replay(&mut canvas, below, reach, to_prefix)?;

        if self.backdrops.get(k).is_none_or(|b| b.size != size) {
            let b = self.new_backdrop(size);
            if k < self.backdrops.len() {
                let old = std::mem::replace(&mut self.backdrops[k], b);
                self.vello.override_image(&old.image, None);
            } else {
                self.backdrops.push(b);
            }
            self.local_mapping += 1;
        }
        let b = &self.backdrops[k];
        render(
            &mut self.vello,
            &self.device,
            &self.queue,
            &self.prefix,
            &b.prefix,
            size,
        )?;
        let sigma = (sigma_device / scale) as f32;
        let radius = (3. * sigma).ceil() as i32;
        for (pass, dir) in [[1i32, 0], [0, 1]].into_iter().enumerate() {
            let mut bytes = [0u8; 32];
            bytes[0..4].copy_from_slice(&dir[0].to_le_bytes());
            bytes[4..8].copy_from_slice(&dir[1].to_le_bytes());
            bytes[8..12].copy_from_slice(&radius.to_le_bytes());
            bytes[12..16].copy_from_slice(&sigma.to_le_bytes());
            bytes[16..20].copy_from_slice(&u32::from(pass == 0).to_le_bytes());
            self.queue.write_buffer(&b.uniforms[pass], 0, &bytes);
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("MUI backdrop"),
            });
        self.passes
            .draw(&mut encoder, &self.passes.blur, &b.binds[0], &b.tmp);
        self.passes
            .draw(&mut encoder, &self.passes.blur, &b.binds[1], &b.out);
        self.queue.submit([encoder.finish()]);
        self.vello.mark_override_image_dirty(&b.image);
        Ok(Some((b.image.clone(), brush)))
    }

    fn new_backdrop(&mut self, size: [u32; 2]) -> Backdrop {
        use wgpu::TextureUsages as U;
        let d = &self.device;
        let prefix = texture(
            d,
            "MUI backdrop",
            size,
            U::STORAGE_BINDING | U::TEXTURE_BINDING,
        )
        .create_view(&wgpu::TextureViewDescriptor::default());
        let tmp = texture(
            d,
            "MUI backdrop",
            size,
            U::RENDER_ATTACHMENT | U::TEXTURE_BINDING,
        )
        .create_view(&wgpu::TextureViewDescriptor::default());
        let out = texture(d, "MUI backdrop", size, U::RENDER_ATTACHMENT | U::COPY_SRC);
        let image = stand_in(size, ImageAlphaType::AlphaPremultiplied);
        self.vello.override_image(&image, Some(whole(&out)));
        let uniforms = [uniform(d), uniform(d)];
        let binds = [
            bind(d, &self.passes.layout, &prefix, &uniforms[0]),
            bind(d, &self.passes.layout, &tmp, &uniforms[1]),
        ];
        Backdrop {
            size,
            prefix,
            tmp,
            out: out.create_view(&wgpu::TextureViewDescriptor::default()),
            image,
            uniforms,
            binds,
        }
    }
}

#[cfg(test)]
mod rotation_tests {
    use super::*;
    use mui_scene::prelude::*;

    #[test]
    fn unchanged_and_bounded_weld_damage_preserve_retained_pixels() {
        let external = HashMap::new();
        let classify = |r, xf| paint_damage(&[], &[], &external, &external, r, xf, [1000, 800]);
        assert!(matches!(classify(NOTHING, Affine::IDENTITY), Change::None));
        let old = Rect::new(20., 30., 40., 50.);
        let new = Rect::new(90., 70., 120., 100.);
        let xf = Affine::scale(1.5);
        let Change::Part([x, y, w, h]) = classify(old.union(new), xf) else {
            panic!("a bounded material change must not repaint the surface");
        };
        let changed = Rect::new(x as f64, y as f64, (x + w) as f64, (y + h) as f64);
        for r in [old, new] {
            let transformed = xf.transform_rect_bbox(r);
            assert!(changed.contains(transformed.origin()));
            assert!(changed.x1 >= transformed.x1 && changed.y1 >= transformed.y1);
        }
        assert!((w as u64) * (h as u64) < 800_000 / 10);
        assert!(matches!(
            classify(Rect::new(-100., -100., -50., -50.), xf),
            Change::Part([_, _, 0, 0])
        ));
    }

    #[test]
    fn resizing_does_not_retain_the_product_of_previous_axis_maxima() {
        assert_eq!(retained_target_size([8192, 512], [512, 8192]), [512, 8192]);
        assert_eq!(retained_target_size([8192, 8192], [512, 512]), [512, 512]);
        assert_eq!(retained_target_size([1024, 768], [1000, 750]), [1024, 768]);
        assert_eq!(retained_target_size([1024, 768], [1200, 800]), [1200, 800]);
        assert_eq!(
            retained_target_size([u32::MAX, u32::MAX], [u32::MAX, u32::MAX]),
            [u32::MAX, u32::MAX]
        );
    }

    #[test]
    fn rotation_damage_reaches_both_original_and_rotated_footprints() {
        let spec = |angle| {
            SceneSpec::new(
                stack([stack([block(10., 4.)
                    .offset(6., 2.)
                    .anchor(Align::Start, Align::Start)
                    .fill(Role::Primary)
                    .id("damage.leaf")])
                .size(20., 10.)
                .offset(10., 10.)
                .anchor(Align::Start, Align::Start)
                .rotation(angle)])
                .size(40., 40.),
            )
        };
        let first = resolve(&spec(0.)).unwrap();
        let changed = resolve(&spec(std::f64::consts::FRAC_PI_2)).unwrap();
        let paint = |scene: &mui_scene::ResolvedScene| {
            scene
                .paint
                .iter()
                .find(|p| p.key.as_str() == "damage.leaf" && p.layer == Layer::Fill)
                .unwrap()
                .clone()
        };
        let before = paint(&first);
        let after = paint(&changed);
        assert_ne!(
            before, after,
            "retained equality must notice the paint transform"
        );
        let before = reach(&before).unwrap();
        let after = reach(&after).unwrap();
        assert!((before.width() - 10.).abs() < 1e-8 && (before.height() - 4.).abs() < 1e-8);
        assert!((after.width() - 4.).abs() < 1e-8 && (after.height() - 10.).abs() < 1e-8);
        let dirty = before.union(after);
        assert!(
            dirty.contains(Point::new(16.5, 12.5)) && dirty.contains(Point::new(22.5, 20.5)),
            "partial damage must cover pixels cleared and pixels newly drawn"
        );
        // An external member above the viewport rotates into it; preparation
        // and compositing must use exactly the same effective visibility.
        let external = resolve(
            &SceneSpec::new(
                stack([stack([block(10., 10.)
                    .fill(Role::Primary)
                    .anchor(Align::Start, Align::Start)])
                .size(10., 10.)
                .offset(0., -100.)
                .anchor(Align::Start, Align::Start)
                .weld(Weld::shape())
                .id("external")])
                .size(100., 100.)
                .rotation(std::f64::consts::PI),
            )
            .weld_backend(WeldBackend::AnalyticGpu),
        )
        .unwrap();
        let material = external.external_weld("external").unwrap();
        assert!(
            !visible(material, Affine::IDENTITY, [300, 300]),
            "unrotated material domain {:?}",
            material.bounds()
        );
        assert!(external_visible(
            &external,
            "external",
            material,
            Affine::IDENTITY,
            [300, 300]
        ));
        let fill = external
            .paint
            .iter()
            .find(|p| p.layer == Layer::External)
            .unwrap();
        assert!(visible(material, fill.transform, [300, 300]));

        let member = |angle| {
            stack([block(10., 10.).fill(Role::Primary).rotation(angle)]).weld(Weld::shape())
        };
        assert!(
            matches!(
                resolve(&SceneSpec::new(member(0.1))),
                Err(mui_scene::SceneError::UnsupportedWeld(_))
            ),
            "member rotation cannot silently disappear into the welded material"
        );
        assert!(
            resolve(&SceneSpec::new(member(0.).rotation(0.1))).is_ok(),
            "whole welded group rotation preserves local material geometry"
        );
    }
}
