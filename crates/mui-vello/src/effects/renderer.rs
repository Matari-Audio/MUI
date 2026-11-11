//! The window's GPU renderer: the quad pass draws into a retained frame
//! texture, and one present pass puts that frame on whatever view the host
//! hands over. A frame the pass refuses is rasterised by `vello_cpu`
//! instead and uploaded into the same frame texture, so the present path
//! never changes.
use super::{Budget, EffectStats, Error, WeldTextures, gate};
use crate::kurbo::Affine;
use crate::quad::{self, QuadRenderer, Reason};
use crate::{Canvas, software};
use mui_scene::{Painted, ResolvedScene};
use std::sync::Arc;

/// What a shared device's editors reuse: the weld material pipeline and the
/// pipeline cache. Everything else is per window.
#[derive(Clone)]
pub(crate) struct Shared {
    weld: Arc<WeldTextures>,
    cache: Option<wgpu::PipelineCache>,
}

/// How often the quad pass refused a frame, and why last.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fallback {
    /// Frames rasterised on the CPU because the pass refused an entry.
    pub frames: u64,
    /// The entry and reason behind the latest such frame.
    pub last: Option<(usize, Reason)>,
    /// The frame now held is a CPU one; it is tried on the GPU again as
    /// soon as the paint list or transform changes.
    pub active: bool,
}

/// The retained GPU renderer: the quad pass on the host's device.
///
/// A frame whose paint list, transform and size are unchanged submits
/// nothing; presenting it to a new view is one present pass. A frame that
/// changed entries in place redraws only their box (see
/// [`QuadRenderer::render`]), and [`GpuRenderer::damage`] reports it.
///
/// What the pass refuses (see [`quad::Reason`]) is drawn whole by
/// `vello_cpu` and uploaded; [`GpuRenderer::fallback`] counts it. A frame
/// that also holds weld material cannot fall back and is an error.
///
/// Device and queue ownership is per renderer. Recreate it on device loss.
pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    quad: QuadRenderer,
    welds: WeldTextures,
    shared: Shared,
    present: Present,
    frame: Frame,
    size: [u32; 2],
    /// The list and transform the frame holds; `None` after anything that
    /// leaves it holding something else (an overlay, an error).
    shown: Option<(Vec<Painted>, Affine)>,
    /// A host texture changed since the last frame.
    stale: bool,
    /// The view the frame was last presented to.
    presented: Option<wgpu::TextureView>,
    /// Present only damage, onto a target that keeps its pixels.
    incremental: bool,
    damage: Vec<[u32; 4]>,
    straight: bool,
    cpu: Option<software::Renderer>,
    fallback: Fallback,
}

/// The retained frame: what the quad pass draws and the present reads.
struct Frame {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bind: wgpu::BindGroup,
}

struct Present {
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    premultiplied: wgpu::RenderPipeline,
    straight: wgpu::RenderPipeline,
}

/// The frame's format: the quad pass draws premultiplied sRGB bytes, and
/// `vello_cpu`'s pixels upload into it unchanged.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

impl Present {
    fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        cache: Option<&wgpu::PipelineCache>,
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("MUI present"),
            source: wgpu::ShaderSource::Wgsl(super::PRESENT_SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("MUI present"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("MUI present"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(format.into())],
                }),
                multiview_mask: None,
                cache,
            })
        };
        Self {
            format,
            premultiplied: pipeline("fs_present"),
            straight: pipeline("fs_present_straight"),
            layout,
        }
    }
}

fn checked_size(device: &wgpu::Device, size: [u32; 2]) -> Result<(), Error> {
    let max = device.limits().max_texture_dimension_2d;
    if size.contains(&0) || size.iter().any(|s| *s > max) {
        return Err(Error::Budget("target dimensions"));
    }
    Ok(())
}

impl Frame {
    fn new(device: &wgpu::Device, present: &Present, size: [u32; 2]) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("MUI frame"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = Self::bind(device, present, &view);
        Self {
            texture,
            view,
            bind,
        }
    }

    fn bind(device: &wgpu::Device, present: &Present, view: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("MUI frame"),
            layout: &present.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            }],
        })
    }
}

impl GpuRenderer {
    /// A renderer drawing into views of `format` (a non-sRGB UNORM one),
    /// with a frame of `size` pixels and welds held to `budget`.
    pub async fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        size: [u32; 2],
        budget: Budget,
    ) -> Result<Self, Error> {
        Self::new_shared(device, queue, format, size, budget, None).await
    }

    pub(crate) fn shared(&self) -> Shared {
        self.shared.clone()
    }

    pub(crate) async fn new_shared(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        size: [u32; 2],
        budget: Budget,
        shared: Option<Shared>,
    ) -> Result<Self, Error> {
        checked_size(device, size)?;
        // The frame holds sRGB-encoded bytes; an sRGB view would encode
        // them a second time on store.
        if format.is_srgb() {
            return Err(Error::Unsupported("target must be a non-sRGB format"));
        }
        let (welds, shared, loaded) = if let Some(shared) = shared {
            (shared.weld.for_window(budget)?, shared, None)
        } else {
            let welds = WeldTextures::new(device, queue, budget).await?;
            let loaded = crate::host::pipeline_cache::load(device);
            let shared = Shared {
                weld: Arc::new(welds.for_window(budget)?),
                cache: loaded.as_ref().map(|c| c.cache.clone()),
            };
            (welds, shared, loaded)
        };
        let cache = shared.cache.as_ref();
        let (quad, present) = gate(device, || {
            (
                QuadRenderer::with_cache(device, FORMAT, cache),
                Present::new(device, format, cache),
            )
        });
        if let Some(loaded) = loaded {
            loaded.save();
        }
        let frame = Frame::new(device, &present, size);
        Ok(Self {
            device: device.clone(),
            queue: queue.clone(),
            quad,
            welds,
            shared,
            present,
            frame,
            size,
            shown: None,
            stale: false,
            presented: None,
            incremental: false,
            damage: Vec::new(),
            straight: false,
            cpu: None,
            fallback: Fallback::default(),
        })
    }

    pub(crate) fn set_surface_format(&mut self, format: wgpu::TextureFormat) {
        if self.present.format == format {
            return;
        }
        let cache = self.shared.cache.as_ref();
        self.present = gate(&self.device, || Present::new(&self.device, format, cache));
        self.frame.bind = Frame::bind(&self.device, &self.present, &self.frame.view);
        self.presented = None;
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

    pub fn resize(&mut self, size: [u32; 2]) -> Result<(), Error> {
        checked_size(&self.device, size)?;
        if size != self.size {
            self.frame = Frame::new(&self.device, &self.present, size);
            self.size = size;
            self.invalidate();
        }
        Ok(())
    }

    /// Forget what the frame and the last view hold: the next render draws
    /// whole and presents. Glyphs, images and weld textures stay.
    pub fn invalidate(&mut self) {
        self.quad.invalidate();
        self.shown = None;
        self.presented = None;
    }

    /// Paint `texture` wherever the scene carries [`mui_scene::Image::texture`]
    /// with this `key`: `Rgba8Unorm`, straight alpha, `TEXTURE_BINDING`. It
    /// is sampled in place, never copied; call again after drawing into it,
    /// and the next render redraws what samples it even if the scene did not
    /// change.
    pub fn set_texture(&mut self, key: u64, texture: &wgpu::Texture) {
        self.quad.set_texture(key, texture);
        self.stale = true;
    }

    pub fn remove_texture(&mut self, key: u64) {
        self.quad.remove_texture(key);
        self.stale = true;
    }

    /// The view last got `resolved` under `xf`, and nothing it samples
    /// changed since: presenting again would draw the same pixels, so a
    /// host can skip acquiring a surface texture at all.
    pub fn is_current(&self, resolved: &ResolvedScene, xf: Affine) -> bool {
        self.presented.is_some()
            && !self.stale
            && self
                .shown
                .as_ref()
                .is_some_and(|(paint, at)| *at == xf && *paint == resolved.paint)
            && crate::quad::visible_welds(resolved, xf, self.size)
                .all(|(key, material)| self.welds.is_current(key, material))
    }

    /// Device rectangles the last successful render changed on `target`.
    pub fn damage(&self) -> &[[u32; 4]] {
        &self.damage
    }

    /// How often the quad pass refused a frame; see [`Fallback`].
    pub fn fallback(&self) -> &Fallback {
        &self.fallback
    }

    /// The quad pass's own account of its last frame.
    pub fn quad_frame(&self) -> quad::Frame {
        self.quad.frame()
    }

    /// Present only damaged rectangles onto a target that keeps its pixels
    /// (the worker's readback bitmap), instead of the whole frame.
    pub(crate) fn incremental_output(&mut self) {
        self.incremental = true;
    }

    pub fn render(
        &mut self,
        resolved: &ResolvedScene,
        xf: Affine,
        target: &wgpu::TextureView,
    ) -> Result<EffectStats, Error> {
        let result = self.frame(resolved, xf);
        self.finish(result, target)
    }

    /// [`GpuRenderer::render`] with `overlay` drawn over the scene, never
    /// retained: the frame is rasterised on the CPU (overlays are debug
    /// drawing: inspectors, outlines), and the next plain render draws the
    /// scene on the GPU again.
    pub fn render_with_overlay(
        &mut self,
        resolved: &ResolvedScene,
        xf: Affine,
        target: &wgpu::TextureView,
        overlay: impl FnOnce(&mut dyn Canvas),
    ) -> Result<EffectStats, Error> {
        let result = self.cpu_frame(resolved, xf, Some(overlay)).inspect(|_| {
            self.shown = None;
            self.fallback.active = false;
        });
        self.finish(result, target)
    }

    fn finish(
        &mut self,
        result: Result<EffectStats, Error>,
        target: &wgpu::TextureView,
    ) -> Result<EffectStats, Error> {
        if result.is_err() {
            self.invalidate();
            self.damage.clear();
            return result;
        }
        self.stale = false;
        let full = [0, 0, self.size[0], self.size[1]];
        if self.incremental && self.presented.as_ref() != Some(target) {
            self.damage = vec![full];
        }
        if self.incremental {
            if !self.damage.is_empty() {
                self.draw(target, Some(&self.damage));
            }
        } else if !self.damage.is_empty() || self.presented.as_ref() != Some(target) {
            self.draw(target, None);
        }
        self.presented = Some(target.clone());
        result
    }

    /// The frame onto `target`: whole (cleared first), or only `rects`.
    fn draw(&self, target: &wgpu::TextureView, rects: Option<&[[u32; 4]]>) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("MUI present"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("MUI present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: match rects {
                            Some(_) => wgpu::LoadOp::Load,
                            None => wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(if self.straight {
                &self.present.straight
            } else {
                &self.present.premultiplied
            });
            pass.set_bind_group(0, &self.frame.bind, &[]);
            match rects {
                Some(rects) => {
                    for &[x, y, w, h] in rects {
                        pass.set_scissor_rect(x, y, w, h);
                        pass.draw(0..3, 0..1);
                    }
                }
                None => pass.draw(0..3, 0..1),
            }
        }
        self.queue.submit([encoder.finish()]);
    }

    /// Bring the frame texture up to `resolved` under `xf`, setting
    /// [`GpuRenderer::damage`] to what changed on it.
    fn frame(&mut self, resolved: &ResolvedScene, xf: Affine) -> Result<EffectStats, Error> {
        if xf.as_coeffs().iter().any(|v| !v.is_finite()) {
            return Err(Error::Unsupported("nonfinite scene transform"));
        }
        self.damage.clear();
        let same = self
            .shown
            .as_ref()
            .is_some_and(|(paint, at)| *at == xf && *paint == resolved.paint);
        // A CPU frame of this very list stays until the list changes.
        if self.fallback.active && same && !self.stale {
            return Ok(EffectStats::default());
        }
        let drawn = self.quad.render_scene(
            &self.device,
            &self.queue,
            resolved,
            &mut self.welds,
            xf,
            &self.frame.view,
            self.size,
        );
        let mut stats = match drawn {
            Ok(stats) => stats,
            Err(quad::Error::Unsupported { index, reason }) => {
                if self.fallback.last.is_none_or(|(_, r)| r != reason) {
                    crate::diagnostics::breadcrumb(
                        "mui-vello",
                        "cpu_fallback",
                        &format!("quad pass refused entry {index}: {reason}"),
                    );
                }
                let stats = self.cpu_frame(resolved, xf, None::<fn(&mut dyn Canvas)>)?;
                self.fallback.frames += 1;
                self.fallback.last = Some((index, reason));
                self.fallback.active = true;
                self.remember(resolved, xf);
                return Ok(stats);
            }
            Err(quad::Error::Target) => {
                return Err(Error::Unsupported("empty target or nonfinite transform"));
            }
            Err(quad::Error::Weld(e)) => return Err(Error::Render(e)),
        };
        self.fallback.active = false;
        let frame = self.quad.frame();
        if frame.drawn {
            let rect = frame.damage.unwrap_or([0, 0, self.size[0], self.size[1]]);
            stats.encoded_scenes += 1;
            stats.renders += 1;
            stats.rendered_pixels += u64::from(rect[2]) * u64::from(rect[3]);
            self.damage.push(rect);
        }
        self.remember(resolved, xf);
        Ok(stats)
    }

    fn remember(&mut self, resolved: &ResolvedScene, xf: Affine) {
        match &mut self.shown {
            Some((paint, at)) => {
                paint.clone_from(&resolved.paint);
                *at = xf;
            }
            None => self.shown = Some((resolved.paint.clone(), xf)),
        }
    }

    /// `resolved` (and `overlay`) rasterised by `vello_cpu` and uploaded
    /// into the frame: whole, or only what changed since the last CPU
    /// frame when that is what the frame still holds.
    fn cpu_frame(
        &mut self,
        resolved: &ResolvedScene,
        xf: Affine,
        overlay: Option<impl FnOnce(&mut dyn Canvas)>,
    ) -> Result<EffectStats, Error> {
        let size = (self.size[0], self.size[1]);
        let cpu = match &mut self.cpu {
            Some(cpu) => {
                cpu.resize(size).map_err(Error::Render)?;
                cpu
            }
            None => self
                .cpu
                .insert(software::Renderer::new(size).map_err(Error::Render)?),
        };
        // The frame holds the quad pass's pixels, or an overlay: start whole.
        if !self.fallback.active || self.shown.is_none() {
            cpu.invalidate();
        }
        let cpu_error = |e: String| Error::Render(format!("CPU fallback: {e}"));
        let rects = match overlay {
            Some(overlay) => {
                cpu.render_with_overlay(resolved, xf, overlay)
                    .map_err(cpu_error)?;
                vec![[0, 0, size.0, size.1]]
            }
            None => cpu.render_damage(resolved, xf).map_err(cpu_error)?,
        };
        let pixels = cpu.pixels();
        for &[x, y, w, h] in &rects {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.frame.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &pixels[(y as usize * size.0 as usize + x as usize) * 4..],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size.0 * 4),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        }
        // Something other than the pass drew on its target.
        self.quad.invalidate();
        let mut stats = EffectStats::default();
        for &[.., w, h] in &rects {
            stats.renders += 1;
            stats.rendered_pixels += u64::from(w) * u64::from(h);
        }
        self.damage = rects;
        Ok(stats)
    }
}
