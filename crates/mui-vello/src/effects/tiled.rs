//! Opt-in persistent tile renderer. Dirty tiles recompose ALL intersecting
//! content in paint order; the swapchain is fully covered by cached tiles each
//! presentation. This first version keeps paint-list diffing O(n), and unknown
//! text/effect extents conservatively retain their draw commands.
use super::{
    damage::{self, DamagePlan, DamageTracker, Tile},
    Budget, EffectStats, Error, WeldTextures,
};
use crate::{
    kurbo::{Affine, BezPath, Rect},
    Cache, Canvas as _, Gpu,
};
use mui_scene::{Layer, ResolvedScene};
use vello_common::{geometry::RectU16, peniko::ImageQuality};
const SIDE: u32 = 256;
const GUARD: u32 = 2;
struct CachedTile {
    tile: Tile,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}
#[derive(Clone, Debug, Default)]
pub struct TileStats {
    pub dirty_tiles: usize,
    pub total_tiles: usize,
    pub dirty_pixels: u64,
    pub tile_submissions: usize,
    /// The tile cache was populated from one full-window raster pass.
    pub full_redraw: bool,
    pub replayed_ops: usize,
    pub culled_ops: usize,
}
pub struct TiledEffects {
    device: wgpu::Device,
    queue: wgpu::Queue,
    size: [u32; 2],
    renderer: vello_hybrid::Renderer,
    resources: vello_hybrid::Resources,
    present_renderer: vello_hybrid::Renderer,
    present_resources: vello_hybrid::Resources,
    tile_scene: vello_hybrid::Scene,
    present_scene: vello_hybrid::Scene,
    cache: Cache,
    effects: WeldTextures,
    tiles: Vec<CachedTile>,
    /// `tiles`' rectangles, for the damage tracker.
    grid: Vec<Tile>,
    full: Option<CachedTile>,
    bindings: vello_hybrid::TextureBindings,
    damage: DamageTracker,
    plan: DamagePlan,
    /// Each paint entry's converted path and bounds on a dirty frame, reused
    /// so the conversion runs once per entry rather than once per tile.
    ops: Vec<(BezPath, Option<Rect>)>,
    limit: u64,
    stats: TileStats,
}
impl TiledEffects {
    pub async fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        size: [u32; 2],
        budget: Budget,
        tile_bytes: u64,
    ) -> Result<Self, Error> {
        if !matches!(
            format,
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm
        ) {
            return Err(Error::Unsupported("tile output requires non-sRGB UNORM"));
        }
        let effects = WeldTextures::new(device, queue, budget).await?;
        let (renderer, resources) = vello_hybrid::Renderer::new(
            device,
            &vello_hybrid::RenderTargetConfig {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: SIDE + 2 * GUARD,
                height: SIDE + 2 * GUARD,
            },
        );
        let (present_renderer, present_resources) = vello_hybrid::Renderer::new(
            device,
            &vello_hybrid::RenderTargetConfig {
                format,
                width: size[0].max(1),
                height: size[1].max(1),
            },
        );
        let mut s = Self {
            device: device.clone(),
            queue: queue.clone(),
            size: [0, 0],
            renderer,
            resources,
            present_renderer,
            present_resources,
            tile_scene: vello_hybrid::Scene::new(
                (SIDE + 2 * GUARD) as u16,
                (SIDE + 2 * GUARD) as u16,
            ),
            present_scene: vello_hybrid::Scene::new(1, 1),
            cache: Cache::default(),
            effects,
            tiles: Vec::new(),
            grid: Vec::new(),
            full: None,
            bindings: Default::default(),
            damage: Default::default(),
            plan: Default::default(),
            ops: Vec::new(),
            limit: tile_bytes,
            stats: Default::default(),
        };
        s.resize(size)?;
        Ok(s)
    }
    pub fn stats(&self) -> &TileStats {
        &self.stats
    }
    pub fn invalidate(&mut self) {
        self.damage.invalidate();
    }
    pub fn resize(&mut self, size: [u32; 2]) -> Result<(), Error> {
        if size == self.size {
            return Ok(());
        }
        let max = self.device.limits().max_texture_dimension_2d.min(65535);
        if size.contains(&0) || size.iter().any(|s| *s > max) {
            return Err(Error::Budget("tile viewport"));
        }
        let list = damage::tiles(size, SIDE);
        let bytes = list
            .iter()
            .map(|t| u64::from(t.width + 2 * GUARD) * u64::from(t.height + 2 * GUARD) * 4)
            .sum::<u64>();
        if bytes > self.limit || list.len() > 1024 {
            return Err(Error::Budget("persistent tiles exceed explicit budget"));
        }
        // The optional full-window target shares the explicit tile budget.
        // Small budgets and maximum-size viewports keep the per-tile path.
        let full_size = [size[0] + 2 * GUARD, size[1] + 2 * GUARD];
        let full_bytes = u64::from(full_size[0]) * u64::from(full_size[1]) * 4;
        let whole = (list.len() > 1
            && full_size.iter().all(|s| *s <= max)
            && bytes + full_bytes <= self.limit)
            .then_some(Tile {
                x: 0,
                y: 0,
                width: size[0],
                height: size[1],
            });
        self.full = None;
        self.tiles.clear();
        self.grid.clone_from(&list);
        self.bindings = Default::default();
        self.size = size;
        self.present_scene
            .reset_and_resize(size[0] as u16, size[1] as u16);
        let tile_count = list.len();
        for (i, tile) in list.into_iter().chain(whole).enumerate() {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("MUI retained tile"),
                size: wgpu::Extent3d {
                    width: tile.width + 2 * GUARD,
                    height: tile.height + 2 * GUARD,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            if i == tile_count {
                self.full = Some(CachedTile {
                    tile,
                    texture,
                    view,
                });
                continue;
            }
            let id = vello_hybrid::TextureId(i as u64 + 1);
            self.bindings.insert(id, view.clone());
            // Guard pixels are rasterized but never overlap neighboring tiles.
            // The transform maps the source region's *local* rectangle, so
            // the guard offset lives in `source_region` alone. The blit is
            // 1:1, and bilinear sampling bleeds the guard texel into the
            // last content column, so it samples nearest.
            self.present_scene.draw_texture_rects(
                id,
                ImageQuality::Low,
                [vello_hybrid::SampleRect {
                    source_region: RectU16::new(
                        GUARD as u16,
                        GUARD as u16,
                        (GUARD + tile.width) as u16,
                        (GUARD + tile.height) as u16,
                    ),
                    transform: Affine::translate((f64::from(tile.x), f64::from(tile.y))),
                }],
            );
            self.tiles.push(CachedTile {
                tile,
                texture,
                view,
            });
        }
        self.invalidate();
        Ok(())
    }
    pub fn render(
        &mut self,
        resolved: &ResolvedScene,
        xf: Affine,
        target: &wgpu::TextureView,
    ) -> Result<EffectStats, Error> {
        if xf.as_coeffs().iter().any(|v| !v.is_finite()) {
            return Err(Error::Unsupported("nonfinite tile transform"));
        }
        let mut plan = std::mem::take(&mut self.plan);
        self.damage.plan(resolved, xf, &self.grid, &mut plan);
        self.stats = TileStats {
            dirty_tiles: plan.dirty.len(),
            total_tiles: self.grid.len(),
            dirty_pixels: plan.dirty_pixels,
            ..Default::default()
        };
        let result = self.effects_then_tiles(resolved, xf, target, &plan.dirty);
        self.plan = plan;
        if result.is_ok() {
            self.damage.commit(resolved, xf);
        } else {
            self.invalidate();
        }
        result
    }
    fn effects_then_tiles(
        &mut self,
        resolved: &ResolvedScene,
        xf: Affine,
        target: &wgpu::TextureView,
        dirty: &[usize],
    ) -> Result<EffectStats, Error> {
        // Prepare each effect ONCE, not per tile, and only the ones some tile
        // can sample: every tile region lies within the guard-inflated
        // viewport. Offscreen textures stay resident as the pool budget allows.
        let view = Rect::new(0., 0., f64::from(self.size[0]), f64::from(self.size[1]))
            .inflate(f64::from(GUARD), f64::from(GUARD));
        let wanted = resolved
            .external_welds()
            .filter(move |(_, e)| damage::intersects(damage::external_bounds(e, xf), view));
        let mut stats = self
            .effects
            .begin(wanted.clone().map(|(k, e)| (k, &e.material)))?;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("MUI tile effects"),
            });
        for (k, e) in wanted {
            if let Err(err) = self
                .effects
                .encode(k, &e.material, &mut encoder, &mut stats)
            {
                self.effects.abort();
                return Err(err);
            }
        }
        self.queue.submit([encoder.finish()]);
        self.effects.commit_submitted(&mut stats);
        self.draw_tiles(resolved, xf, target, dirty, &mut stats)?;
        Ok(stats)
    }
    fn draw_tiles(
        &mut self,
        resolved: &ResolvedScene,
        xf: Affine,
        target: &wgpu::TextureView,
        dirty: &[usize],
        stats: &mut EffectStats,
    ) -> Result<(), Error> {
        if !dirty.is_empty() {
            self.ops.clear();
            for p in &resolved.paint {
                let bez = crate::bez_path(&p.path, crate::ARC_TOLERANCE)?;
                let bounds = damage::bounds(p, &bez, xf);
                self.ops.push((bez, bounds));
            }
        }
        let full_redraw =
            // Once most tiles need replay, rasterizing the window once avoids
            // repeated path/text processing. Sparse damage keeps tile culling.
            dirty.len() > self.tiles.len() / 2 && self.full.is_some();
        self.stats.full_redraw = full_redraw;
        let passes = if full_redraw { 1 } else { dirty.len() };
        for &i in dirty.iter().take(passes) {
            let target_tile = if full_redraw {
                self.full
                    .as_ref()
                    .expect("full target admitted within budget")
            } else {
                &self.tiles[i]
            };
            let tile = target_tile.tile;
            let region = tile.rect().inflate(GUARD as f64, GUARD as f64);
            let transform = Affine::translate((-region.x0, -region.y0)) * xf;
            let size = [tile.width + 2 * GUARD, tile.height + 2 * GUARD];
            self.tile_scene
                .reset_and_resize(size[0] as u16, size[1] as u16);
            let mut canvas = Gpu {
                scene: &mut self.tile_scene,
                resources: &mut self.resources,
                cache: &mut self.cache,
                atlas: Some(crate::Atlas {
                    renderer: &mut self.renderer,
                    device: &self.device,
                    queue: &self.queue,
                }),
            };
            canvas.set_transform(transform);
            for (p, (bez, bounds)) in resolved.paint.iter().zip(&self.ops) {
                if bounds.is_some_and(|b| !damage::intersects(b, region)) {
                    self.stats.culled_ops += 1;
                    continue;
                }
                self.stats.replayed_ops += 1;
                if p.layer == Layer::External {
                    let e = resolved
                        .external_weld(&p.key)
                        .ok_or_else(|| Error::Missing(p.key.to_string()))?;
                    if !damage::intersects(damage::external_bounds(e, xf), region) {
                        continue;
                    }
                    let id = self
                        .effects
                        .texture_id(&p.key)
                        .ok_or_else(|| Error::Missing(p.key.to_string()))?;
                    let [w, h] = e.material.pixels();
                    let b = e.bounds();
                    canvas.scene.draw_texture_rects(
                        id,
                        ImageQuality::Medium,
                        [vello_hybrid::SampleRect {
                            source_region: RectU16::new(0, 0, w as u16, h as u16),
                            transform: Affine::translate((b.min.x, b.min.y))
                                * Affine::scale_non_uniform(
                                    b.width() / w as f64,
                                    b.height() / h as f64,
                                ),
                        }],
                    );
                } else if !crate::layered(&mut canvas, p) {
                    crate::one(&mut canvas, p, bez)?;
                }
            }
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("MUI dirty tile"),
                });
            self.renderer
                .render(
                    &self.tile_scene,
                    &mut self.resources,
                    &self.device,
                    &self.queue,
                    &mut encoder,
                    &vello_hybrid::RenderSize {
                        width: size[0],
                        height: size[1],
                    },
                    &target_tile.view,
                    self.effects.bindings(),
                )
                .map_err(|e| Error::Render(e.to_string()))?;
            if full_redraw {
                // Include the same guard band as individual tile rasterization.
                // The full target starts at (-GUARD, -GUARD), so tile origins
                // already address their corresponding guard pixels.
                for cached in &self.tiles {
                    encoder.copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &target_tile.texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: cached.tile.x,
                                y: cached.tile.y,
                                z: 0,
                            },
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::TexelCopyTextureInfo {
                            texture: &cached.texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::Extent3d {
                            width: cached.tile.width + 2 * GUARD,
                            height: cached.tile.height + 2 * GUARD,
                            depth_or_array_layers: 1,
                        },
                    );
                }
            }
            // Submit before the same Vello renderer rewrites internal buffers
            // for another tile. No completion wait, mapping or readback occurs.
            self.queue.submit([encoder.finish()]);
            self.stats.tile_submissions += 1;
            stats.encoded_scenes += 1;
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("MUI retained tile presentation"),
            });
        self.present_renderer
            .render(
                &self.present_scene,
                &mut self.present_resources,
                &self.device,
                &self.queue,
                &mut encoder,
                &vello_hybrid::RenderSize {
                    width: self.size[0],
                    height: self.size[1],
                },
                target,
                &self.bindings,
            )
            .map_err(|e| Error::Render(e.to_string()))?;
        self.queue.submit([encoder.finish()]);
        Ok(())
    }
}
