use super::{Budget, EffectStats, Error, WeldTextures};
use crate::{
    kurbo::{Affine, BezPath, Rect},
    Cache, Canvas as _, Gpu,
};
use mui_scene::{ExternalWeld, Layer, Painted, ResolvedScene};
use vello_common::{geometry::RectU16, peniko::ImageQuality};

/// A single Vello renderer, its resources, and its associated external textures.
/// An unchanged paint list reuses the already-prepared Hybrid scene: this skips
/// strip generation, not merely arc-to-cubic conversion. Rendered into the
/// same target view as last time, an unchanged frame skips the GPU pass too:
/// the target still holds it. A swapchain hands out a fresh view per frame,
/// so there the pass runs; such a host should stop requesting idle frames.
pub struct HybridEffects {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: vello_hybrid::Renderer,
    resources: vello_hybrid::Resources,
    scene: vello_hybrid::Scene,
    cache: Cache,
    effects: WeldTextures,
    size: [u32; 2],
    retained: Vec<Painted>,
    transform: Option<Affine>,
    mapping: u64,
    valid: bool,
    /// The view the last complete frame went to; cleared by anything that
    /// could leave it stale.
    presented: Option<wgpu::TextureView>,
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
        let effects = WeldTextures::new(device, queue, budget).await?;
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
            cache: Cache::default(),
            effects,
            size,
            retained: Vec::new(),
            transform: None,
            mapping: 0,
            valid: false,
            presented: None,
        })
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
        self.presented = None;
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
        self.effects
            .forget_absent(|k| resolved.external_weld(k).is_some());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("MUI effects + Vello"),
            });
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
                self.presented = None;
                self.cache.tick();
                self.scene.reset();
                let mut canvas = Gpu {
                    scene: &mut self.scene,
                    resources: &mut self.resources,
                    cache: &mut self.cache,
                    atlas: Some(crate::Atlas {
                        renderer: &mut self.renderer,
                        device: &self.device,
                        queue: &self.queue,
                    }),
                };
                canvas.set_transform(xf);
                let mut bez = BezPath::new();
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
                        crate::bez_path_into(&p.path, crate::ARC_TOLERANCE, &mut bez)?;
                        crate::one(&mut canvas, p, &bez)?;
                    }
                }
                if let Some(draw) = overlay {
                    draw(&mut canvas);
                } else {
                    super::damage::copy_changed(&mut self.retained, &resolved.paint);
                    self.transform = Some(xf);
                    self.mapping = mapping;
                    self.valid = true;
                }
                stats.encoded_scenes += 1;
            }
            if stats.effect_draws == 0 && self.presented.as_ref() == Some(target) {
                return Ok(());
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
            stats.renders += 1;
            Ok(())
        })();
        match result {
            Ok(()) => {
                // Accepted submission is the cache commit boundary, not an
                // encode call and not a synchronous GPU-completion wait.
                self.queue.submit([encoder.finish()]);
                self.effects.commit_submitted(&mut stats);
                if self.valid {
                    self.presented = Some(target.clone());
                }
                Ok(stats)
            }
            Err(e) => {
                self.effects.abort();
                self.invalidate();
                Err(e)
            }
        }
    }
}
