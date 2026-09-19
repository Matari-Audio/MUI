use super::{Budget, EffectStats, Error, OutputEncoding, WeldTextures};
use crate::{
    kurbo::{Affine, Rect},
    Canvas as _, Gpu, ImageIds, PathCache,
};
use mui_scene::{ExternalWeld, Layer, Painted, ResolvedScene};
use vello_common::{geometry::RectU16, peniko::ImageQuality};

/// A single Vello renderer, its resources, and its associated external textures.
/// An unchanged paint list reuses the already-prepared Hybrid scene: this skips
/// strip generation, not merely arc-to-cubic conversion. GPU compositing still
/// runs when a frame is requested; the host must stop requesting idle frames.
pub struct HybridEffects {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: vello_hybrid::Renderer,
    resources: vello_hybrid::Resources,
    scene: vello_hybrid::Scene,
    images: ImageIds,
    paths: PathCache,
    effects: WeldTextures,
    size: [u32; 2],
    retained: Vec<Painted>,
    transform: Option<Affine>,
    mapping: u64,
    valid: bool,
    timer: Option<super::GpuTimer>,
}
fn checked_size(device: &wgpu::Device, size: [u32; 2]) -> Result<(), Error> {
    let max = device
        .limits()
        .max_texture_dimension_2d
        .min(u16::MAX as u32);
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
impl HybridEffects {
    pub async fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        size: [u32; 2],
        budget: Budget,
    ) -> Result<Self, Error> {
        checked_size(device, size)?;
        // The effect output is sRGB-encoded premultiplied RGBA. Sampling it into
        // an sRGB attachment would apply another transfer function on store.
        // Add a separately verified linear-light path before widening this list.
        if !matches!(
            format,
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm
        ) {
            return Err(Error::Unsupported(
                "target must be non-sRGB RGBA8/BGRA8 UNORM",
            ));
        }
        let effects = WeldTextures::new(
            device,
            queue,
            budget,
            OutputEncoding::HybridPremultipliedSrgb,
        )
        .await?;
        let (renderer, resources) = vello_hybrid::Renderer::new(
            device,
            &vello_hybrid::RenderTargetConfig {
                format,
                width: size[0],
                height: size[1],
            },
        );
        Ok(Self {
            device: device.clone(),
            queue: queue.clone(),
            renderer,
            resources,
            scene: vello_hybrid::Scene::new(size[0] as u16, size[1] as u16),
            images: Default::default(),
            paths: PathCache::new(),
            effects,
            size,
            retained: Vec::new(),
            transform: None,
            mapping: 0,
            valid: false,
            timer: None,
        })
    }
    /// Optional diagnostic. No queries or mapping callbacks exist until enabled.
    pub fn enable_profiling(&mut self) -> Result<(), Error> {
        if self.timer.is_none() {
            self.timer = Some(super::GpuTimer::new(&self.device, &self.queue)?);
        }
        Ok(())
    }
    pub fn collect_timings(&mut self, out: &mut Vec<super::GpuTiming>) {
        if let Some(timer) = &mut self.timer {
            timer.collect(&self.device, out);
        }
    }
    pub fn timing_losses(&self) -> (u64, u64) {
        self.timer
            .as_ref()
            .map_or((0, 0), |t| (t.dropped_samples, t.failed_samples))
    }
    pub fn resize(&mut self, size: [u32; 2]) -> Result<(), Error> {
        checked_size(&self.device, size)?;
        if size != self.size {
            self.size = size;
            self.scene.reset_and_resize(size[0] as u16, size[1] as u16);
            self.invalidate();
        }
        Ok(())
    }
    /// Invalidate encoded UI work without unnecessarily throwing away images,
    /// glyph preparation resources, or unchanged material textures.
    pub fn invalidate(&mut self) {
        self.valid = false;
    }
    pub fn resident_effect_bytes(&self) -> u64 {
        self.effects.resident_bytes()
    }
    pub fn release_effects(&mut self) {
        self.effects.clear();
        self.invalidate();
    }
    pub fn render(
        &mut self,
        resolved: &ResolvedScene,
        xf: Affine,
        target: &wgpu::TextureView,
    ) -> Result<EffectStats, Error> {
        self.render_inner::<fn(&mut Gpu<'_>)>(resolved, xf, target, None)
    }
    /// Debug overlays are not silently cached. A subsequent non-overlay frame
    /// rebuilds the base encoding once so inspector marks cannot persist.
    pub fn render_with_overlay<F: FnOnce(&mut Gpu<'_>)>(
        &mut self,
        resolved: &ResolvedScene,
        xf: Affine,
        target: &wgpu::TextureView,
        overlay: F,
    ) -> Result<EffectStats, Error> {
        self.render_inner(resolved, xf, target, Some(overlay))
    }
    fn render_inner<F: FnOnce(&mut Gpu<'_>)>(
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
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("MUI effects + Vello"),
            });
        let ticket = self
            .timer
            .as_mut()
            .and_then(|timer| timer.begin(&mut encoder));
        let result = (|| -> Result<(), Error> {
            for (key, e) in resolved
                .external_welds()
                .filter(|(_, e)| visible(e, xf, size))
            {
                self.effects
                    .encode(key, &e.material, &mut encoder, &mut stats)?;
            }
            let mapping = self.effects.mapping_revision();
            let needs_encode = overlay.is_some()
                || !self.valid
                || self.transform != Some(xf)
                || self.mapping != mapping
                || self.retained != resolved.paint;
            if needs_encode {
                self.valid = false; // partial encoding must never be reused after error
                self.scene.reset();
                self.paths.frame = self.paths.frame.wrapping_add(1);
                let frame = self.paths.frame;
                let mut canvas = Gpu {
                    scene: &mut self.scene,
                    resources: &mut self.resources,
                    atlas: Some(crate::Atlas {
                        renderer: &mut self.renderer,
                        device: &self.device,
                        queue: &self.queue,
                        ids: &mut self.images,
                    }),
                };
                canvas.set_transform(xf);
                for p in &resolved.paint {
                    if p.layer == Layer::External {
                        let e = resolved
                            .external_weld(&p.key)
                            .ok_or_else(|| Error::Missing(p.key.to_string()))?;
                        if !visible(e, xf, size) {
                            continue;
                        }
                        let id = self
                            .effects
                            .texture_id(&p.key)
                            .ok_or_else(|| Error::Missing(p.key.to_string()))?;
                        let [w, h] = e.material.pixels();
                        let b = e.bounds();
                        let local = Affine::translate((b.min.x, b.min.y))
                            * Affine::scale_non_uniform(
                                b.width() / f64::from(w),
                                b.height() / f64::from(h),
                            );
                        // Surrounding clip/blend state stays on THIS scene. This
                        // is not an overlay composited after menus or tooltips.
                        canvas.scene.draw_texture_rects(
                            id,
                            ImageQuality::Medium,
                            [vello_hybrid::SampleRect {
                                source_region: RectU16::new(0, 0, w as u16, h as u16),
                                transform: local,
                            }],
                        );
                    } else if !crate::layered(&mut canvas, p) {
                        let bez = self.paths.bez(p)?;
                        crate::one(&mut canvas, p, &bez)?;
                    }
                }
                if let Some(draw) = overlay {
                    draw(&mut canvas);
                } else {
                    self.retained.clone_from(&resolved.paint);
                    self.transform = Some(xf);
                    self.mapping = mapping;
                    self.valid = true;
                }
                self.paths.entries.retain(|_, e| e.frame == frame);
                stats.encoded_scenes += 1;
            }
            self.renderer
                .render(
                    &self.scene,
                    &mut self.resources,
                    &self.device,
                    &self.queue,
                    &mut encoder,
                    &vello_hybrid::RenderSize {
                        width: size[0],
                        height: size[1],
                    },
                    target,
                    self.effects.bindings(),
                )
                .map_err(|e| Error::Render(e.to_string()))?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                // Accepted submission is the cache commit boundary, not an
                // encode call and not a synchronous GPU-completion wait.
                if let (Some(timer), Some(ticket)) = (&mut self.timer, ticket) {
                    timer.finish(&mut encoder, ticket);
                }
                self.queue.submit([encoder.finish()]);
                if let (Some(timer), Some(ticket)) = (&mut self.timer, ticket) {
                    timer.submitted(ticket);
                }
                self.effects.commit_submitted(&mut stats);
                Ok(stats)
            }
            Err(e) => {
                if let (Some(timer), Some(ticket)) = (&mut self.timer, ticket) {
                    timer.abort(ticket);
                }
                self.effects.abort();
                self.valid = false;
                Err(e)
            }
        }
    }
}
