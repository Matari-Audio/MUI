//! The one GPU host for a window MUI paints into: the surface, a device
//! chosen for it, and the retained [`GpuRenderer`]. The gallery (winit) and
//! the plugin editor (baseview) both drive this; only the window and how a
//! surface is made from it stay theirs.
//!
//! Loss is handled here, not by a panic: a lost device (driver reset,
//! eGPU unplugged) is rebuilt on the next [`Host::present`], and a lost
//! surface is reported as [`Frame::SurfaceLost`] so the caller can hand a
//! fresh one to [`Host::replace_surface`].
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use mui_scene::ResolvedScene;

use crate::Classic;
use crate::effects::{Budget, EffectStats, GpuRenderer};
use crate::kurbo::Affine;

/// A failed device rebuild waits this long before the next attempt, so a
/// GPU that keeps failing does not cost a device creation every frame.
const RETRY: Duration = Duration::from_millis(500);

/// The surface size clamped to what a vello `Scene` holds (`u16`), or
/// `None` when there is nothing to draw into: a minimised window reports
/// 0x0, and configuring a zero-sized surface is invalid.
pub fn target_size(width: u32, height: u32) -> Option<(u32, u32)> {
    let max = u32::from(u16::MAX);
    (width > 0 && height > 0).then(|| (width.min(max), height.min(max)))
}

/// MUI's alpha contract wants a non-sRGB UNORM target: vello writes sRGB
/// values, and an `_Srgb` surface would encode them a second time.
pub fn surface_format(formats: &[wgpu::TextureFormat]) -> Option<wgpu::TextureFormat> {
    formats.iter().copied().find(|f| {
        matches!(
            f,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
        )
    })
}

/// What one [`Host::present`] did.
#[derive(Debug)]
pub enum Frame {
    /// The scene is on screen.
    Presented(EffectStats),
    /// Nothing reached the screen (no size, occluded, outdated, timed out,
    /// or the device was just rebuilt): paint the same scene next frame.
    Skipped,
    /// The surface is gone. Make a new one from the same window, pass it to
    /// [`Host::replace_surface`], and paint again.
    SurfaceLost,
}

/// Everything that lives on one device, rebuilt whole when it is lost:
/// pipelines, atlases, weld textures and retained encodings die with it.
struct OnDevice {
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: GpuRenderer,
    /// Raised by wgpu's device-lost callback, on whatever thread wgpu calls it.
    lost: Arc<AtomicBool>,
}

impl OnDevice {
    /// A device able to present to `surface` (any, headless), configured
    /// for it at `size`, and a renderer on it.
    fn open(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        size: (u32, u32),
    ) -> Result<Self, String> {
        // A desktop with an iGPU enumerates it first; paint on the card.
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: surface,
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| e.to_string())?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(|e| e.to_string())?;
        let lost = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&lost);
        device.set_device_lost_callback(move |_, _| flag.store(true, Ordering::Release));
        let limit = device.limits().max_texture_dimension_2d;
        let (width, height) = target_size(size.0.min(limit), size.1.min(limit)).unwrap_or((1, 1));
        let config = match surface {
            Some(surface) => {
                let format = surface_format(&surface.get_capabilities(&adapter).formats)
                    .ok_or("no non-sRGB UNORM surface format")?;
                let config = wgpu::SurfaceConfiguration {
                    format,
                    ..surface
                        .get_default_config(&adapter, width, height)
                        .ok_or("surface has no default configuration")?
                };
                surface.configure(&device, &config);
                config
            }
            // Headless: a stand-in nothing is configured with.
            None => wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: wgpu::TextureFormat::Rgba8Unorm,
                width,
                height,
                present_mode: wgpu::PresentMode::Fifo,
                desired_maximum_frame_latency: 2,
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: Vec::new(),
                color_space: wgpu::SurfaceColorSpace::Auto,
            },
        };
        let renderer = pollster::block_on(GpuRenderer::new(
            &device,
            &queue,
            config.format,
            [width, height],
            Budget::default(),
        ))
        .map_err(|e| e.to_string())?;
        Ok(Self {
            device,
            queue,
            config,
            renderer,
            lost,
        })
    }

    fn lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }
}

/// A window's surface, the device painting it and the renderer.
pub struct Host {
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    gpu: OnDevice,
    /// The size last asked for, in physical pixels: what a rebuilt device
    /// configures. `None` while there is nothing to draw into.
    wanted: Option<(u32, u32)>,
    retry_at: Option<Instant>,
}

impl Host {
    /// A device for `surface` and a renderer at `size` physical pixels.
    /// `surface` must come from `instance`.
    pub fn new(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        size: (u32, u32),
    ) -> Result<Self, String> {
        let gpu = OnDevice::open(&instance, Some(&surface), size)?;
        Ok(Self {
            instance,
            surface,
            gpu,
            wanted: target_size(size.0, size.1),
            retry_at: None,
        })
    }

    /// The instance the surface came from, for making a replacement.
    pub fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }

    /// The configured surface size, physical pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.gpu.config.width, self.gpu.config.height)
    }

    /// The device was lost; the next [`Host::present`] rebuilds it. An idle
    /// window polls this so it does not wait for an event to find out.
    pub fn device_lost(&self) -> bool {
        self.gpu.lost()
    }

    /// Resize to `width` x `height` physical pixels, clamped to what the
    /// device and a vello scene hold. Zero hides: presents skip until a
    /// real size comes back.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.wanted = target_size(width, height);
        let Some((width, height)) = self.wanted else {
            return Ok(());
        };
        let limit = self.gpu.device.limits().max_texture_dimension_2d;
        let (width, height) = (width.min(limit), height.min(limit));
        if (width, height) == self.size() {
            return Ok(());
        }
        let OnDevice {
            device,
            config,
            renderer,
            ..
        } = &mut self.gpu;
        renderer
            .resize([width, height])
            .map_err(|e| format!("resize to {width}x{height}: {e}"))?;
        config.width = width;
        config.height = height;
        self.surface.configure(device, config);
        Ok(())
    }

    /// Swap in a new surface for the same window after
    /// [`Frame::SurfaceLost`]. It must come from [`Host::instance`].
    pub fn replace_surface(&mut self, surface: wgpu::Surface<'static>) {
        self.surface = surface;
        self.surface.configure(&self.gpu.device, &self.gpu.config);
        self.gpu.renderer.invalidate();
    }

    /// Paint `scene` under `xf` and present it. `Err` is a render or
    /// device-rebuild failure, not a lost surface; painting the same scene
    /// again would fail the same way.
    pub fn present(&mut self, scene: &ResolvedScene, xf: Affine) -> Result<Frame, String> {
        self.present_inner::<fn(&mut Classic<'_>)>(scene, xf, None)
    }

    /// [`Host::present`] with `overlay` drawn on top, never retained.
    pub fn present_with_overlay<F: FnOnce(&mut Classic<'_>)>(
        &mut self,
        scene: &ResolvedScene,
        xf: Affine,
        overlay: F,
    ) -> Result<Frame, String> {
        self.present_inner(scene, xf, Some(overlay))
    }

    fn present_inner<F: FnOnce(&mut Classic<'_>)>(
        &mut self,
        scene: &ResolvedScene,
        xf: Affine,
        overlay: Option<F>,
    ) -> Result<Frame, String> {
        use wgpu::CurrentSurfaceTexture as Acquired;
        let Some(size) = self.wanted else {
            return Ok(Frame::Skipped);
        };
        if self.gpu.lost() {
            let now = Instant::now();
            if self.retry_at.is_some_and(|at| now < at) {
                return Ok(Frame::Skipped);
            }
            match OnDevice::open(&self.instance, Some(&self.surface), size) {
                Ok(gpu) => {
                    self.gpu = gpu;
                    self.retry_at = None;
                    return Ok(Frame::Skipped);
                }
                Err(e) => {
                    self.retry_at = Some(now + RETRY);
                    return Err(format!("rebuilding a lost device: {e}"));
                }
            }
        }
        let OnDevice {
            device,
            queue,
            config,
            renderer,
            ..
        } = &mut self.gpu;
        let frame = match self.surface.get_current_texture() {
            Acquired::Success(frame) | Acquired::Suboptimal(frame) => frame,
            Acquired::Outdated => {
                self.surface.configure(device, config);
                return Ok(Frame::Skipped);
            }
            Acquired::Occluded | Acquired::Timeout => return Ok(Frame::Skipped),
            Acquired::Lost | Acquired::Validation => return Ok(Frame::SurfaceLost),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let stats = match overlay {
            Some(draw) => renderer.render_with_overlay(scene, xf, &view, draw),
            None => renderer.render(scene, xf, &view),
        }
        .map_err(|e| e.to_string())?;
        queue.present(frame);
        Ok(Frame::Presented(stats))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui_scene::prelude::*;

    #[test]
    fn sizes_clamp_and_zero_is_nothing_to_draw_into() {
        assert_eq!(target_size(0, 600), None);
        assert_eq!(target_size(800, 0), None);
        assert_eq!(target_size(70_000, 600), Some((65_535, 600)));
    }

    #[test]
    fn the_surface_format_is_never_srgb() {
        use wgpu::TextureFormat as F;
        assert_eq!(
            surface_format(&[F::Bgra8UnormSrgb, F::Bgra8Unorm]),
            Some(F::Bgra8Unorm)
        );
        assert_eq!(surface_format(&[F::Rgba8UnormSrgb]), None);
    }

    /// A real loss, not a flag flipped by hand: `Device::destroy` fires the
    /// lost callback, and a device opened again renders.
    #[test]
    fn a_destroyed_device_is_seen_and_a_new_one_renders() {
        let instance = wgpu::Instance::default();
        let gpu = match OnDevice::open(&instance, None, (16, 16)) {
            Ok(gpu) => gpu,
            Err(e) => return eprintln!("SKIPPED: no wgpu device ({e})"),
        };
        assert!(!gpu.lost());
        gpu.device.destroy();
        let _ = gpu.device.poll(wgpu::PollType::Poll);
        assert!(gpu.lost(), "loss unseen");
        let mut gpu2 = OnDevice::open(&instance, None, (16, 16)).unwrap();
        assert_ne!(gpu2.device, gpu.device);
        let target = gpu2.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let scope = gpu2.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let root = leaf(16., 16.).fill(Role::Primary);
        let scene = resolve_scene(&SceneSpec::new(root).offered(Size::new(16., 16.))).unwrap();
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        gpu2.renderer
            .render(&scene, Affine::IDENTITY, &view)
            .unwrap();
        assert!(pollster::block_on(scope.pop()).is_none());
        assert!(!gpu2.lost());
    }
}
