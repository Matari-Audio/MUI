//! Retained CPU pixels and optional native presentation without a GPU device.
use crate::{Cache, Canvas, Cpu, kurbo::Affine};
use mui_scene::{Painted, ResolvedScene};
use vello_common::pixmap::Pixmap;

/// A single-threaded CPU renderer. Unchanged scenes reuse their pixels.
/// SIMD selection remains Vello's runtime choice; no machine-specific ISA is required.
pub struct Renderer {
    ctx: vello_cpu::RenderContext,
    resources: vello_cpu::Resources,
    cache: Cache,
    pixels: Pixmap,
    retained: Vec<Painted>,
    transform: Option<Affine>,
}

fn dimensions(size: (u32, u32)) -> Result<(u16, u16), String> {
    // Bound the RGBA target to 64 MiB. Native presentation and renderer scratch
    // are additional memory; this is not a total process-memory budget.
    if size.0 == 0 || size.1 == 0 || u64::from(size.0) * u64::from(size.1) > 16 * 1024 * 1024 {
        return Err("CPU target must contain 1..=16777216 pixels".into());
    }
    Ok((
        size.0
            .try_into()
            .map_err(|_| "CPU target width exceeds u16")?,
        size.1
            .try_into()
            .map_err(|_| "CPU target height exceeds u16")?,
    ))
}

fn context((width, height): (u16, u16)) -> vello_cpu::RenderContext {
    vello_cpu::RenderContext::new_with(
        width,
        height,
        vello_cpu::RenderSettings {
            // Do not compete with the DAW's audio workers, even with cpu-threads enabled.
            num_threads: 0,
            ..Default::default()
        },
    )
}

impl Renderer {
    pub fn new(size: (u32, u32)) -> Result<Self, String> {
        let size = dimensions(size)?;
        Ok(Self {
            ctx: context(size),
            resources: vello_cpu::Resources::default(),
            cache: Cache::default(),
            pixels: Pixmap::new(size.0, size.1),
            retained: Vec::new(),
            transform: None,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (
            u32::from(self.pixels.width()),
            u32::from(self.pixels.height()),
        )
    }

    pub fn resize(&mut self, size: (u32, u32)) -> Result<(), String> {
        if self.size() != size {
            let size = dimensions(size)?;
            self.ctx = context(size);
            self.pixels = Pixmap::new(size.0, size.1);
            self.transform = None;
        }
        Ok(())
    }

    /// Discard damage history after an external full invalidation.
    pub fn invalidate(&mut self) {
        self.transform = None;
    }

    /// Premultiplied sRGB RGBA8, top-to-bottom. Valid after a successful render.
    pub fn pixels(&self) -> &[u8] {
        self.pixels.data_as_u8_slice()
    }

    /// Straight-alpha sRGB RGBA8, top-to-bottom: what a PNG or an image
    /// upload wants. Valid after a successful render.
    pub fn rgba8(&self) -> Vec<u8> {
        let mut rgba = self.pixels().to_vec();
        vello_common::pixmap::unpremultiply_rgba8(&mut rgba);
        rgba
    }

    /// Returns whether pixels changed. GPU materials must first be resolved
    /// with `WeldBackend::Reference`; unsupported scenes fail without presenting.
    pub fn render(&mut self, scene: &ResolvedScene, transform: Affine) -> Result<bool, String> {
        self.render_inner(scene, transform, None::<fn(&mut dyn Canvas)>)
    }

    /// Rasterize only damaged device rectangles, retaining every other pixel.
    /// Shares the GPU renderer's element footprint/paint-order damage policy.
    /// Resize, transform and unknown bounds invalidate the target. Backdrop and
    /// shadow dependencies expand local regions rather than dropping effects.
    pub fn render_damage(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
    ) -> Result<Vec<[u32; 4]>, String> {
        use crate::damage::{Change, NOTHING, paint_damage};
        if !transform.as_coeffs().iter().all(|v| v.is_finite()) {
            return Err("nonfinite CPU scene transform".into());
        }
        if scene
            .paint
            .iter()
            .any(|p| p.layer == mui_scene::Layer::External)
        {
            return Err("CPU damage requires reference weld resolution".into());
        }
        let (width, height) = self.size();
        let empty = std::collections::HashMap::new();
        let change = if self.transform == Some(transform) {
            paint_damage(
                &self.retained,
                &scene.paint,
                &empty,
                &empty,
                NOTHING,
                transform,
                [width, height],
            )
        } else {
            Change::Full
        };
        let rects = match change {
            Change::None => return Ok(Vec::new()),
            Change::Full => vec![[0, 0, width, height]],
            Change::Part(r) => vec![r],
            Change::Parts(r) => r.rects[..r.len].to_vec(),
        };
        // Mark invalid until every region succeeds. An error must not leave a
        // mixed-generation bitmap that compares equal on the recovery attempt.
        self.transform = None;
        for &[x, y, w, h] in &rects {
            if w == 0 || h == 0 {
                continue;
            }
            // Generate strips in the same global coordinate system as a full
            // render. Moving Bezier geometry into a small scratch target changes
            // float coverage rounding, even for integral tile translations.
            self.ctx.reset_and_resize(width as u16, height as u16);
            let mut scratch = Pixmap::new(w as u16, h as u16);
            let mut canvas = Cpu {
                ctx: &mut self.ctx,
                resources: &mut self.resources,
                cache: &mut self.cache,
            };
            let view = crate::kurbo::Rect::new(x.into(), y.into(), (x + w).into(), (y + h).into());
            crate::paint_within(&mut canvas, scene, transform, Some(view))
                .map_err(|e| e.to_string())?;
            self.ctx.flush();
            self.ctx.render_with(
                &mut scratch,
                &mut self.resources,
                vello_cpu::RasterizerSettings {
                    viewport: Some([x as u16, y as u16, w as u16, h as u16]),
                    ..Default::default()
                },
            );
            for row in 0..h as usize {
                let dest = ((y as usize + row) * width as usize + x as usize) * 4;
                let source = row * w as usize * 4;
                self.pixels.data_as_u8_slice_mut()[dest..dest + w as usize * 4]
                    .copy_from_slice(&scratch.data_as_u8_slice()[source..source + w as usize * 4]);
            }
        }
        self.retained.clone_from(&scene.paint);
        self.transform = Some(transform);
        Ok(rects
            .into_iter()
            .filter(|r| r[2] != 0 && r[3] != 0)
            .collect())
    }

    /// Overlays repaint on every call and are included in the presented pixels.
    pub fn render_with_overlay(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        overlay: impl FnOnce(&mut dyn Canvas),
    ) -> Result<bool, String> {
        self.render_inner(scene, transform, Some(overlay))
    }

    fn render_inner(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        overlay: Option<impl FnOnce(&mut dyn Canvas)>,
    ) -> Result<bool, String> {
        if !transform.as_coeffs().iter().all(|v| v.is_finite()) {
            return Err("nonfinite CPU scene transform".into());
        }
        if overlay.is_none() && self.transform == Some(transform) && self.retained == scene.paint {
            return Ok(false);
        }
        self.transform = None;
        let (width, height) = self.size();
        self.ctx.reset_and_resize(width as u16, height as u16);
        let mut canvas = Cpu {
            ctx: &mut self.ctx,
            resources: &mut self.resources,
            cache: &mut self.cache,
        };
        crate::paint(&mut canvas, scene, transform).map_err(|e| e.to_string())?;
        let has_overlay = overlay.is_some();
        if let Some(overlay) = overlay {
            overlay(&mut canvas);
        }
        self.ctx.flush();
        // ponytail: changed frames repaint fully; add damage clipping after
        // measuring active CPU fallback workloads, preserving blend/backdrop order.
        self.ctx.render(&mut self.pixels, &mut self.resources);
        self.retained.clone_from(&scene.paint);
        self.transform = (!has_overlay).then_some(transform);
        Ok(true)
    }
}

/// `scene` under `transform`, rasterised once on a `size` target: straight
/// sRGB RGBA8, top-to-bottom. For a snapshot or a test; a loop that draws
/// every frame keeps a [`Renderer`] so fonts and unchanged frames are reused.
pub fn rgba8(
    scene: &ResolvedScene,
    transform: Affine,
    size: (u32, u32),
) -> Result<Vec<u8>, String> {
    let mut renderer = Renderer::new(size)?;
    renderer.render(scene, transform)?;
    Ok(renderer.rgba8())
}

/// An opaque native window presenting CPU pixels via `mui-present`.
#[cfg(feature = "software-window")]
pub struct Window<W> {
    presenter: mui_present::CpuPresenter,
    renderer: Renderer,
    pixels_0rgb: Vec<u32>,
    presented: bool,
    startup: bool,
    size: (u32, u32),
    startup_color: Option<u32>,
    /// Retained last so `presenter` always drops before the native window handle.
    _window: W,
}

#[cfg(feature = "software-window")]
impl<W: raw_window_handle::HasDisplayHandle + raw_window_handle::HasWindowHandle + Clone>
    Window<W>
{
    pub fn new(window: W, size: (u32, u32)) -> Result<Self, String> {
        Self::open(window, size, false)
    }
    /// Large startup windows submit their scene's background without a costly
    /// full-size CPU rasterization (including optional overlays). GPU handover
    /// resolves the complete scene. Targets above the CPU budget stay background-only.
    pub fn new_startup(window: W, size: (u32, u32)) -> Result<Self, String> {
        Self::open(
            window,
            size,
            u64::from(size.0) * u64::from(size.1) > 1024 * 1024,
        )
    }
    fn open(window: W, size: (u32, u32), startup: bool) -> Result<Self, String> {
        let display = window.display_handle().map_err(|e| e.to_string())?.as_raw();
        let handle = window.window_handle().map_err(|e| e.to_string())?.as_raw();
        let renderer = Renderer::new(if background_only(startup, size) {
            (1, 1)
        } else {
            size
        })?;
        #[expect(
            unsafe_code,
            reason = "owned CpuPresenter borrows raw handles retained by Self::_window"
        )]
        let presenter = {
            // SAFETY: `window` is retained as `_window` and dropped after `presenter`.
            unsafe { mui_present::CpuPresenter::new(display, handle) }
        }
        .map_err(|e| e.to_string())?;
        Ok(Self {
            presenter,
            renderer,
            pixels_0rgb: Vec::new(),
            presented: false,
            startup,
            size,
            startup_color: None,
            _window: window,
        })
    }

    /// Concrete native CPU presentation backend backing this window.
    #[must_use]
    pub fn backend(&self) -> mui_present::PresentationBackend {
        self.presenter.backend()
    }

    /// Updates the display scale factor for Wayland viewport sizing.
    pub fn set_scale_factor(&mut self, scale: f64) {
        self.presenter.set_scale_factor(scale);
    }

    pub fn background_only(&self) -> bool {
        background_only(self.startup, self.size)
    }

    /// Re-presents retained pixels after an expose or a failed native present.
    pub fn invalidate(&mut self) {
        self.presented = false;
    }

    /// Permanent CPU fallback returns to full-quality rasterization where its
    /// bounded RGBA target fits; enormous windows keep the safe background frame.
    /// Returns whether pixels must be repainted: a window that was already
    /// rasterizing in full has nothing new to show, so an idle editor stays idle.
    pub fn finish_startup(&mut self, size: (u32, u32)) -> bool {
        let was_background_only = self.background_only();
        self.startup = false;
        self.size = size;
        let repaint = was_background_only && !self.background_only();
        if repaint {
            self.invalidate();
        }
        repaint
    }

    /// Zero-sized windows retain their pixels until restored.
    pub fn resize(&mut self, size: (u32, u32)) -> Result<(), String> {
        self.size = size;
        if !self.background_only() && size.0 != 0 && size.1 != 0 {
            self.renderer.resize(size)?;
        }
        self.invalidate();
        Ok(())
    }

    pub fn present(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        size: (u32, u32),
    ) -> Result<bool, String> {
        self.present_inner(scene, transform, size, None::<fn(&mut dyn Canvas)>)
    }

    pub fn present_with_overlay(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        size: (u32, u32),
        overlay: impl FnOnce(&mut dyn Canvas),
    ) -> Result<bool, String> {
        self.present_inner(scene, transform, size, Some(overlay))
    }

    fn present_inner(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        size: (u32, u32),
        overlay: Option<impl FnOnce(&mut dyn Canvas)>,
    ) -> Result<bool, String> {
        if size.0 == 0 || size.1 == 0 {
            return Ok(false);
        }
        if self.size != size {
            self.invalidate();
            self.size = size;
        }
        if self.background_only() {
            let rgb = startup_color(scene);
            if self.presented && self.startup_color == Some(rgb) {
                return Ok(false);
            }
            self.presenter
                .fill_solid(rgb, size)
                .map_err(|e| e.to_string())?;
            self.startup_color = Some(rgb);
            self.presented = true;
            return Ok(true);
        }
        self.renderer.resize(size)?;
        let was_presented = self.presented;
        let mut damage = if let Some(overlay) = overlay {
            self.renderer
                .render_with_overlay(scene, transform, overlay)?;
            vec![[0, 0, size.0, size.1]]
        } else {
            self.renderer.render_damage(scene, transform)?
        };
        if damage.is_empty() && self.presented {
            return Ok(false);
        }
        if !was_presented {
            damage = vec![[0, 0, size.0, size.1]];
        }
        let len = size.0 as usize * size.1 as usize;
        if self.pixels_0rgb.len() != len {
            self.pixels_0rgb.resize(len, 0);
            damage = vec![[0, 0, size.0, size.1]];
        }
        for &[x, y, w, h] in &damage {
            for row in y..y + h {
                let start = row as usize * size.0 as usize + x as usize;
                for (out, rgba) in self.pixels_0rgb[start..start + w as usize].iter_mut().zip(
                    self.renderer.pixels()[start * 4..(start + w as usize) * 4]
                        .as_chunks::<4>()
                        .0,
                ) {
                    *out =
                        (u32::from(rgba[0]) << 16) | (u32::from(rgba[1]) << 8) | u32::from(rgba[2]);
                }
            }
        }
        if debug_damage() {
            for &[x, y, w, h] in &damage {
                for row in y..y + h {
                    for col in x..x + w {
                        if col < x + 2 || row < y + 2 || col + 2 >= x + w || row + 2 >= y + h {
                            self.pixels_0rgb[row as usize * size.0 as usize + col as usize] =
                                0x00ff_0040;
                        }
                    }
                }
            }
        }
        let rects: Vec<mui_present::DamageRect> = damage
            .iter()
            .copied()
            .map(mui_present::DamageRect::from_array)
            .collect();
        self.presenter
            .present_full_with_damage(&self.pixels_0rgb, size, &rects)
            .map_err(|e| e.to_string())?;
        self.presented = true;
        Ok(true)
    }
}

#[cfg(any(test, feature = "software-window"))]
fn background_only(startup: bool, size: (u32, u32)) -> bool {
    startup || dimensions(size).is_err()
}

#[cfg(any(test, feature = "software-window"))]
fn startup_color(scene: &ResolvedScene) -> u32 {
    let Some(paint) = scene
        .paint
        .iter()
        .find(|p| p.layer == mui_scene::Layer::Fill)
    else {
        return 0x202020;
    };
    let rgb = paint.paint.solid().to_srgb().to_rgba8();
    (u32::from(rgb.r) << 16) | (u32::from(rgb.g) << 8) | u32::from(rgb.b)
}

/// Native dirty-bitmap transport backed by `mui-present`. No scene traversal,
/// raster or GPU work occurs here; callers submit completed pixels from an
/// editor worker.
#[cfg(feature = "software-window")]
pub struct BitmapWindow<W> {
    presenter: mui_present::CpuPresenter,
    /// Retained last so `presenter` always drops before the native window handle.
    _window: W,
}
#[cfg(feature = "software-window")]
impl<W: raw_window_handle::HasDisplayHandle + raw_window_handle::HasWindowHandle + Clone>
    BitmapWindow<W>
{
    pub fn new(window: W) -> Result<Self, String> {
        let display = window.display_handle().map_err(|e| e.to_string())?.as_raw();
        let handle = window.window_handle().map_err(|e| e.to_string())?.as_raw();
        #[expect(
            unsafe_code,
            reason = "owned CpuPresenter borrows raw handles retained by Self::_window"
        )]
        let presenter = {
            // SAFETY: `window` is retained as `_window` and dropped after `presenter`.
            unsafe { mui_present::CpuPresenter::new(display, handle) }
        }
        .map_err(|e| e.to_string())?;
        Ok(Self {
            presenter,
            _window: window,
        })
    }

    /// Concrete native CPU presentation backend backing this bitmap window.
    #[must_use]
    pub fn backend(&self) -> mui_present::PresentationBackend {
        self.presenter.backend()
    }

    /// Updates the display scale factor for Wayland viewport sizing.
    pub fn set_scale_factor(&mut self, scale: f64) {
        self.presenter.set_scale_factor(scale);
    }

    pub fn present_patch(
        &mut self,
        rect: [u32; 4],
        pixels: &[u32],
        size: (u32, u32),
    ) -> Result<(), String> {
        self.presenter
            .present_patch(mui_present::DamageRect::from_array(rect), pixels, size)
            .map_err(|e| e.to_string())
    }

    /// Present a budgeted `src_size` bitmap scaled to fill `dst_size` physical
    /// pixels (used when a HiDPI / ultrawide window exceeds the 16 Mi pixel
    /// 1:1 CPU target budget so painting never stops).
    pub fn present_scaled(
        &mut self,
        src_size: (u32, u32),
        pixels: &[u32],
        dst_size: (u32, u32),
    ) -> Result<(), String> {
        self.presenter
            .present_scaled(src_size, pixels, dst_size)
            .map_err(|e| e.to_string())
    }
}

fn debug_damage() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("MUI_DEBUG_DAMAGE").as_deref() == Ok("1"))
}

/// Diagnostic only: `MUI_DEBUG_DAMAGE=1` flashes the automatically derived
/// damaged rectangle's outline. App code never supplies invalidation regions.
pub fn damage_flash(pixels: &[u32], size: (u32, u32)) -> Option<Vec<u32>> {
    if !debug_damage() || size.0 == 0 || size.1 == 0 {
        return None;
    }
    let mut out = pixels.to_vec();
    for y in 0..size.1 {
        for x in 0..size.0 {
            if x < 2 || y < 2 || x + 2 >= size.0 || y + 2 >= size.1 {
                out[(y * size.0 + x) as usize] = 0x00ff0040;
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui_scene::prelude::*;

    #[test]
    fn cpu_damage_matches_full_raster_for_motion_overlap_and_resize() {
        let scene = |x| {
            mui_scene::resolve(&SceneSpec::new(
                stack![
                    block(512., 384.)
                        .radius(0.)
                        .fill(Color::srgb(0.1, 0.1, 0.1))
                        .id("background"),
                    block(32., 32.)
                        .fill(Color::oklcha(0.7, 0.2, 30., 0.6))
                        .id("moving")
                        .at(x, 30.),
                    block(16., 16.)
                        .fill(Color::oklcha(0.8, 0.2, 200., 0.5))
                        .id("overlap")
                        .at(48., 40.),
                ]
                .size(512., 384.),
            ))
            .unwrap()
        };
        let mut incremental = Renderer::new((512, 384)).unwrap();
        let mut full = Renderer::new((512, 384)).unwrap();
        assert_eq!(
            incremental
                .render_damage(&scene(30.), Affine::IDENTITY)
                .unwrap(),
            vec![[0, 0, 512, 384]]
        );
        for x in [34., 42., 60., 35., 34.125, 42.75, 60.5, 35.25] {
            let scene = scene(x);
            let damage = incremental.render_damage(&scene, Affine::IDENTITY).unwrap();
            assert!(!damage.is_empty());
            assert!(damage.iter().map(|r| r[2] * r[3]).sum::<u32>() < 512 * 384);
            full.render(&scene, Affine::IDENTITY).unwrap();
            assert_eq!(incremental.pixels(), full.pixels());
            assert!(
                incremental
                    .render_damage(&scene, Affine::IDENTITY)
                    .unwrap()
                    .is_empty()
            );
        }
        incremental.resize((600, 400)).unwrap();
        assert_eq!(
            incremental
                .render_damage(&scene(35.), Affine::IDENTITY)
                .unwrap(),
            vec![[0, 0, 600, 400]]
        );
        assert_eq!(
            incremental
                .render_damage(&scene(35.), Affine::scale(1.25))
                .unwrap(),
            vec![[0, 0, 600, 400]]
        );
    }
    #[test]
    fn partial_scene_preserves_backdrops_and_shifted_inset_shadows() {
        let scene = |x, blur| {
            mui_scene::resolve(&SceneSpec::new(
                stack![
                    block(512., 384.)
                        .fill(Color::srgb(0.1, 0.2, 0.3))
                        .id("background"),
                    block(32., 32.)
                        .fill(Color::srgb(1., 0., 0.))
                        .id("under-glass")
                        .at(x, 72.),
                    block(96., 64.)
                        .radius(8.)
                        .fill(Color::oklcha(0.7, 0.02, 30., 0.2))
                        .backdrop_blur(4.)
                        .shadow(Shadow::soft(3.))
                        .shadow(Shadow {
                            dx: 30.,
                            ..Shadow::inset(blur)
                        })
                        .id("glass")
                        .at(64., 64.),
                    block(32., 32.)
                        .fill(Color::srgb(0., 1., 0.))
                        .id("unrelated")
                        .at(350., 240.),
                ]
                .size(512., 384.),
            ))
            .unwrap()
        };
        let mut partial = Renderer::new((512, 384)).unwrap();
        let mut full = Renderer::new((512, 384)).unwrap();
        partial
            .render_damage(&scene(72., 2.), Affine::IDENTITY)
            .unwrap();
        for (x, blur) in [(80., 2.), (80., 6.), (96., 6.), (72., 2.)] {
            let scene = scene(x, blur);
            let damage = partial.render_damage(&scene, Affine::IDENTITY).unwrap();
            assert!(!damage.is_empty());
            assert!(
                damage
                    .iter()
                    .all(|r| r[0] + r[2] < 256 && r[1] + r[3] < 192)
            );
            full.render(&scene, Affine::IDENTITY).unwrap();
            assert_eq!(partial.pixels(), full.pixels(), "x={x}, blur={blur}");
        }
    }
    #[test]
    fn cpu_shrink_returns_to_full_quality_but_gpu_startup_waits_for_handover() {
        assert!(background_only(false, (8192, 4096)));
        assert!(!background_only(false, (1920, 1080)));
        assert!(background_only(true, (1920, 1080)));
        assert!(!background_only(false, (240, 200)));
    }

    #[test]
    fn startup_background_uses_the_scene_colour_without_full_rasterization() {
        let scene = mui_scene::resolve(&SceneSpec::new(
            block(3425., 2073.).fill(Color::srgb(0., 1., 0.)),
        ))
        .unwrap();
        assert_eq!(startup_color(&scene), 0x00ff00);
    }

    #[test]
    fn gpu_materials_are_rebuilt_for_cpu_and_can_return_to_gpu() {
        let root = || {
            row![
                block(20., 20.).fill(Role::Primary),
                block(20., 20.).fill(Role::Primary),
            ]
            .weld(Weld::all().reach(4.).blend(4.))
        };
        let mut ui = mui::Ui::default().gpu_welding();
        let mut renderer = Renderer::new((80, 40)).unwrap();
        for available in [true, false, true] {
            ui.set_gpu_welding_available(available);
            let frame = ui
                .frame(
                    root(),
                    Some(Size::new(80., 40.)),
                    mui::prelude::PointerInput::default(),
                    0.,
                )
                .unwrap();
            assert_eq!(frame.scene.external_welds().count() > 0, available);
            assert_eq!(
                renderer.render(frame.scene, Affine::IDENTITY).is_ok(),
                !available
            );
        }
    }

    #[test]
    fn cpu_overlay_is_visible_and_removing_it_restores_retained_scene() {
        use crate::kurbo::{Rect, Shape};
        let scene = mui_scene::resolve(&SceneSpec::new(
            block(8., 8.).radius(0.).fill(Color::srgb(1., 0., 0.)),
        ))
        .unwrap();
        let mut renderer = Renderer::new((8, 8)).unwrap();
        renderer.render(&scene, Affine::IDENTITY).unwrap();
        for green in [true, false] {
            let color = if green {
                Color::srgb(0., 1., 0.)
            } else {
                Color::srgb(0., 0., 1.)
            };
            assert!(
                renderer
                    .render_with_overlay(&scene, Affine::IDENTITY, |canvas| {
                        canvas.set_transform(Affine::IDENTITY);
                        canvas.set_paint(color.to_srgb().into());
                        canvas.fill_path(&Rect::new(0., 0., 4., 8.).to_path(0.1));
                    })
                    .unwrap()
            );
            assert_eq!(
                &renderer.pixels()[..4],
                if green {
                    &[0, 255, 0, 255]
                } else {
                    &[0, 0, 255, 255]
                }
            );
            assert_eq!(&renderer.pixels()[7 * 4..8 * 4], &[255, 0, 0, 255]);
        }
        assert!(renderer.render(&scene, Affine::IDENTITY).unwrap());
        assert_eq!(&renderer.pixels()[..4], &[255, 0, 0, 255]);
        assert!(!renderer.render(&scene, Affine::IDENTITY).unwrap());
    }

    #[test]
    fn retained_cpu_pixels_handle_changes_resize_and_invalid_input() {
        let scene = |color| {
            mui_scene::resolve(&SceneSpec::new(block(8., 8.).radius(0.).fill(color))).unwrap()
        };
        let red = scene(Color::srgb(1., 0., 0.));
        let green = scene(Color::srgb(0., 1., 0.));
        let mut renderer = Renderer::new((8, 8)).unwrap();
        assert!(renderer.render(&red, Affine::IDENTITY).unwrap());
        assert_eq!(&renderer.pixels()[..4], &[255, 0, 0, 255]);
        assert!(!renderer.render(&red, Affine::IDENTITY).unwrap());
        assert!(renderer.render(&green, Affine::IDENTITY).unwrap());
        assert_eq!(&renderer.pixels()[..4], &[0, 255, 0, 255]);
        assert!(renderer.render(&green, Affine::scale(f64::NAN)).is_err());
        assert!(renderer.resize((u32::MAX, u32::MAX)).is_err());
        assert_eq!(renderer.size(), (8, 8));
        renderer.resize((16, 16)).unwrap();
        assert!(renderer.render(&green, Affine::scale(2.)).unwrap());
        assert_eq!(
            &renderer.pixels()[renderer.pixels().len() - 4..],
            &[0, 255, 0, 255]
        );
        assert!(Renderer::new((0, 8)).is_err());
    }
}
