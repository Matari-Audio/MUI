use super::{Budget, Converted, EffectStats, Error, WeldTextures};
use crate::classic::{Classic, Textures};
use crate::{
    kurbo::{Affine, Rect, Shape as _},
    Cache, Canvas as _,
};
use mui_scene::{ExternalWeld, Layer, Painted, ResolvedScene};
use std::collections::HashMap;
use std::sync::Arc;
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
    vello: vello::Renderer,
    scene: vello::Scene,
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
    paths: Converted,
    transform: Option<Affine>,
    /// Bumped by anything that changes which image an entry samples.
    mapping: (u64, u64),
    local_mapping: u64,
    /// The encoding matches `retained` under `transform`.
    valid: bool,
    /// A texture this renderer samples changed since the last render.
    stale: bool,
    /// The view the target was last presented to.
    presented: Option<wgpu::TextureView>,
}

/// Vello's output: compute shaders write only a storage texture, so the
/// frame lands here and one fragment pass puts it on the surface.
struct Target {
    view: wgpu::TextureView,
    bind: wgpu::BindGroup,
}

struct Passes {
    layout: wgpu::BindGroupLayout,
    blur: wgpu::RenderPipeline,
    present: wgpu::RenderPipeline,
    /// The present pass reads no uniform, but shares the layout.
    idle: wgpu::Buffer,
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
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(format.into())],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            blur: pipeline("fs_blur", wgpu::TextureFormat::Rgba8Unorm),
            present: pipeline("fs_present", format),
            idle: uniform(device),
            layout,
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

fn visible(e: &ExternalWeld, xf: Affine, size: [u32; 2]) -> bool {
    let b = e.bounds();
    let r = xf.transform_rect_bbox(Rect::new(b.min.x, b.min.y, b.max.x, b.max.y));
    r.x1 > 0. && r.y1 > 0. && r.x0 < f64::from(size[0]) && r.y0 < f64::from(size[1])
}

impl GpuRenderer {
    pub async fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        size: [u32; 2],
        budget: Budget,
    ) -> Result<Self, Error> {
        checked_size(device, size)?;
        // What leaves the present pass is sRGB-encoded and premultiplied; an
        // sRGB attachment would encode it a second time on store.
        if format.is_srgb() {
            return Err(Error::Unsupported("target must be a non-sRGB format"));
        }
        let effects = WeldTextures::new(device, queue, budget).await?;
        let vello = vello::Renderer::new(
            device,
            vello::RendererOptions {
                use_cpu: false,
                // Area coverage: analytic, and the only mode MUI's snapshots
                // were drawn against.
                antialiasing_support: vello::AaSupport::area_only(),
                ..Default::default()
            },
        )
        .map_err(|e| Error::Device(e.to_string()))?;
        let passes = Passes::new(device, format);
        let target = Self::target(device, &passes, size);
        Ok(Self {
            device: device.clone(),
            queue: queue.clone(),
            vello,
            scene: vello::Scene::new(),
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
            paths: Converted::default(),
            transform: None,
            mapping: (0, 0),
            local_mapping: 0,
            valid: false,
            stale: false,
            presented: None,
        })
    }

    fn target(device: &wgpu::Device, passes: &Passes, size: [u32; 2]) -> Target {
        let view = texture(
            device,
            "MUI frame",
            size,
            wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        )
        .create_view(&Default::default());
        Target {
            bind: bind(device, &passes.layout, &view, &passes.idle),
            view,
        }
    }

    pub fn resize(&mut self, size: [u32; 2]) -> Result<(), Error> {
        checked_size(&self.device, size)?;
        if size != self.size {
            self.size = size;
            self.target = Self::target(&self.device, &self.passes, size);
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
        let size = self.size;
        let wanted = resolved
            .external_welds()
            .filter(move |(_, e)| visible(e, xf, size))
            .map(|(k, e)| (k, &e.material));
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
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("MUI welds"),
            });
        let mut drawn = Vec::new();
        for (key, e) in resolved
            .external_welds()
            .filter(|(_, e)| visible(e, xf, size))
        {
            let before = stats.effect_draws;
            self.effects.encode(key, &e.material, &mut encoder, stats)?;
            let (id, tex, used) = self
                .effects
                .texture(key)
                .ok_or_else(|| Error::Missing(key.to_string()))?;
            match self.welds.get(&id) {
                Some(image) if [image.width, image.height] == used => {
                    if stats.effect_draws != before {
                        drawn.push(image.clone());
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
        if stats.effect_draws > 0 {
            self.queue.submit([encoder.finish()]);
            for image in &drawn {
                self.vello.mark_override_image_dirty(image);
            }
            self.stale = true;
        }
        // A weld whose slot went: its stand-in goes too.
        let live: Vec<u64> = resolved
            .external_welds()
            .filter_map(|(k, _)| self.effects.texture(k).map(|t| t.0))
            .collect();
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

        let mapping = (self.effects.mapping_revision(), self.local_mapping);
        let needs_encode = overlay.is_some()
            || !self.valid
            || self.transform != Some(xf)
            || self.mapping != mapping
            || self.retained != resolved.paint;
        if needs_encode {
            self.valid = false; // a partial encoding must never be reused
            self.presented = None;
            self.encode(resolved, xf, overlay)?;
            stats.encoded_scenes += 1;
        }
        if needs_encode || self.stale {
            self.vello
                .render_to_texture(
                    &self.device,
                    &self.queue,
                    &self.scene,
                    &self.target.view,
                    &vello::RenderParams {
                        base_color: peniko::Color::TRANSPARENT,
                        width: size[0],
                        height: size[1],
                        antialiasing_method: vello::AaConfig::Area,
                    },
                )
                .map_err(|e| Error::Render(e.to_string()))?;
            self.stale = false;
            self.presented = None;
            stats.renders += 1;
        }
        if self.presented.as_ref() != Some(target) {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("MUI present"),
                });
            self.passes.draw(
                &mut encoder,
                &self.passes.present,
                &self.target.bind,
                target,
            );
            self.queue.submit([encoder.finish()]);
            if self.valid {
                self.presented = Some(target.clone());
            }
        }
        Ok(())
    }

    fn encode<F: FnOnce(&mut Classic<'_>)>(
        &mut self,
        resolved: &ResolvedScene,
        xf: Affine,
        overlay: Option<F>,
    ) -> Result<(), Error> {
        let size = self.size;
        self.paths.resize(resolved.paint.len());
        // Backdrops first: each is a render of its own that the frame samples.
        let mut blurred = Vec::new();
        let mut k = 0;
        for (i, p) in resolved.paint.iter().enumerate() {
            if p.layer == Layer::Backdrop && p.blur.is_finite() && p.blur > 0.0 {
                let outline = self.paths.get(i, &p.path)?.clone();
                blurred.push(self.backdrop(k, &resolved.paint[..i], p.blur, &outline, xf)?);
                k += 1;
            }
        }
        self.backdrops.truncate(k);

        self.scene.reset();
        let mut canvas = Classic::new(&mut self.scene, &mut self.cache, &self.textures, size);
        canvas.begin_frame();
        canvas.set_transform(xf);
        let mut blurred = blurred.into_iter();
        for (i, p) in resolved.paint.iter().enumerate() {
            match p.layer {
                Layer::External => {
                    let e = resolved
                        .external_weld(&p.key)
                        .ok_or_else(|| Error::Missing(p.key.to_string()))?;
                    if !visible(e, xf, size) {
                        continue;
                    }
                    let image = self
                        .effects
                        .texture(&p.key)
                        .and_then(|(id, ..)| self.welds.get(&id))
                        .ok_or_else(|| Error::Missing(p.key.to_string()))?
                        .clone();
                    let b = e.bounds();
                    let r = Rect::new(b.min.x, b.min.y, b.max.x, b.max.y);
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
                    canvas.fill_path(self.paths.get(i, &p.path)?);
                    canvas.reset_paint_transform();
                }
                _ => {
                    if !crate::layered(&mut canvas, p) {
                        crate::one(&mut canvas, p, self.paths.get(i, &p.path)?)?;
                    }
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
        }
        Ok(())
    }

    /// Backdrop `k`: `below` rendered on its own around `outline`, blurred
    /// by `sigma` scene units, as the image and the brush transform that
    /// put it back where it was. `None` when none of it is on the target.
    ///
    /// The prefix renders at a power-of-two fraction of the device -- a
    /// Gaussian that wide has nothing left at full resolution -- so the two
    /// passes after it never take more than 25 taps.
    fn backdrop(
        &mut self,
        k: usize,
        below: &[Painted],
        sigma: f64,
        outline: &crate::kurbo::BezPath,
        xf: Affine,
    ) -> Result<Option<(ImageData, Affine)>, Error> {
        let sigma_device = sigma * xf.determinant().abs().sqrt();
        let scale = (sigma_device / 4.).ceil().max(1.) as u32;
        let scale = f64::from(scale.next_power_of_two());
        let reach = outline.bounding_box().inflate(3. * sigma, 3. * sigma);
        let device = xf
            .transform_rect_bbox(reach)
            .intersect(Rect::new(0., 0., f64::from(self.size[0]), f64::from(self.size[1])));
        if device.width() <= 0. || device.height() <= 0. {
            return Ok(None);
        }
        let origin = (device.x0.floor(), device.y0.floor());
        let size = [
            ((device.x1 - origin.0) / scale).ceil().max(1.) as u32,
            ((device.y1 - origin.1) / scale).ceil().max(1.) as u32,
        ];
        let to_prefix =
            Affine::scale(1. / scale) * Affine::translate((-origin.0, -origin.1)) * xf;

        self.prefix.reset();
        let mut canvas = Classic::new(&mut self.prefix, &mut self.cache, &self.textures, size);
        canvas.set_transform(to_prefix);
        crate::replay(&mut canvas, below, reach)?;

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
        self.vello
            .render_to_texture(
                &self.device,
                &self.queue,
                &self.prefix,
                &b.prefix,
                &vello::RenderParams {
                    base_color: peniko::Color::TRANSPARENT,
                    width: size[0],
                    height: size[1],
                    antialiasing_method: vello::AaConfig::Area,
                },
            )
            .map_err(|e| Error::Render(e.to_string()))?;
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
        let brush = xf.inverse() * Affine::translate(origin) * Affine::scale(scale);
        Ok(Some((b.image.clone(), brush)))
    }

    fn new_backdrop(&mut self, size: [u32; 2]) -> Backdrop {
        use wgpu::TextureUsages as U;
        let d = &self.device;
        let prefix = texture(d, "MUI backdrop", size, U::STORAGE_BINDING | U::TEXTURE_BINDING)
            .create_view(&Default::default());
        let tmp = texture(d, "MUI backdrop", size, U::RENDER_ATTACHMENT | U::TEXTURE_BINDING)
            .create_view(&Default::default());
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
            out: out.create_view(&Default::default()),
            image,
            uniforms,
            binds,
        }
    }
}
