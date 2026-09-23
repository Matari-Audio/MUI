//! Experimental gallery host for the production-shaped effects renderer.
//! One device, one queue, one Vello renderer. This is a winit host, NOT a
//! CLAP/VST3 child-window adapter or proof of cross-platform DAW integration.
use mui::scene::ResolvedScene;
use mui::vello::{
    effects::{Budget, EffectStats, HybridEffects, TiledEffects},
    kurbo::Affine,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use winit::window::Window;

pub fn target_size(width: u32, height: u32) -> Option<(u32, u32)> {
    (width > 0 && height > 0).then_some((width.min(65535), height.min(65535)))
}

#[allow(clippy::large_enum_variant)]
enum Renderer {
    Whole(HybridEffects),
    Tiled(TiledEffects),
}
impl Renderer {
    fn resize(&mut self, size: [u32; 2]) -> Result<(), mui::vello::effects::Error> {
        match self {
            Self::Whole(r) => r.resize(size),
            Self::Tiled(r) => r.resize(size),
        }
    }
    fn invalidate(&mut self) {
        match self {
            Self::Whole(r) => r.invalidate(),
            Self::Tiled(r) => r.invalidate(),
        }
    }
    fn render(
        &mut self,
        s: &ResolvedScene,
        t: Affine,
        v: &wgpu::TextureView,
    ) -> Result<EffectStats, mui::vello::effects::Error> {
        match self {
            Self::Whole(r) => r.render(s, t, v),
            Self::Tiled(r) => r.render(s, t, v),
        }
    }
    fn render_with_overlay<F: FnOnce(&mut mui::vello::Gpu<'_>)>(
        &mut self,
        s: &ResolvedScene,
        t: Affine,
        v: &wgpu::TextureView,
        f: F,
    ) -> Result<EffectStats, mui::vello::effects::Error> {
        match self {
            Self::Whole(r) => r.render_with_overlay(s, t, v, f),
            Self::Tiled(_) => Err(mui::vello::effects::Error::Unsupported(
                "imperative debug overlay is not retained; disable MUI_TILED for the inspector",
            )),
        }
    }
}
pub struct Gpu {
    instance: wgpu::Instance,
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    drawable: bool,
    /// Raised by wgpu's device-lost callback, on whatever thread wgpu calls it.
    lost: Arc<AtomicBool>,
}
/// Everything that lives on one device: rebuilt whole when it is lost.
struct Device {
    device: wgpu::Device,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    lost: Arc<AtomicBool>,
}
impl Gpu {
    pub async fn new(
        window: Arc<Window>,
        display: Box<winit::event_loop::OwnedDisplayHandle>,
    ) -> Self {
        Self::try_new(window, display)
            .await
            .expect("initialize MUI GPU gallery")
    }
    async fn try_new(
        window: Arc<Window>,
        display: Box<winit::event_loop::OwnedDisplayHandle>,
    ) -> Result<Self, String> {
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(display),
        );
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::from_window_without_display(
                window.clone(),
            ))
            .map_err(|e| e.to_string())?;
        let size = window.inner_size();
        let d = open(&instance, &surface, &window).await?;
        Ok(Self {
            instance,
            window,
            surface,
            device: d.device,
            config: d.config,
            renderer: d.renderer,
            drawable: size.width > 0 && size.height > 0,
            lost: d.lost,
        })
    }
    pub fn window(&self) -> &Window {
        &self.window
    }
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }
    pub fn resize(&mut self, width: u32, height: u32) {
        let Some((width, height)) = target_size(width, height) else {
            self.drawable = false;
            return;
        };
        self.drawable = true;
        let max = self.device.limits().max_texture_dimension_2d;
        let (width, height) = (width.min(max), height.min(max));
        if (width, height) == self.size() {
            return;
        }
        if let Err(e) = self.renderer.resize([width, height]) {
            eprintln!("MUI resize: {e}");
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }
    pub fn present(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
    ) -> Result<Option<EffectStats>, String> {
        self.present_inner::<fn(&mut mui::vello::Gpu<'_>)>(scene, transform, None)
    }
    pub fn present_with_overlay<F: FnOnce(&mut mui::vello::Gpu<'_>)>(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        overlay: F,
    ) -> Result<Option<EffectStats>, String> {
        self.present_inner(scene, transform, Some(overlay))
    }
    fn present_inner<F: FnOnce(&mut mui::vello::Gpu<'_>)>(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        overlay: Option<F>,
    ) -> Result<Option<EffectStats>, String> {
        if !self.drawable {
            return Ok(None);
        }
        if self.lost.load(Ordering::Acquire) {
            // Pipelines, weld textures, atlas ids and the retained encoding all
            // died with the device; only the window and surface survive.
            let d = pollster::block_on(open(&self.instance, &self.surface, &self.window))?;
            eprintln!("MUI GPU device lost; renderer rebuilt");
            self.device = d.device;
            self.config = d.config;
            self.renderer = d.renderer;
            self.lost = d.lost;
            self.window.request_redraw();
            return Ok(None);
        }
        use wgpu::CurrentSurfaceTexture as Acquired;
        let frame = match self.surface.get_current_texture() {
            Acquired::Success(frame) | Acquired::Suboptimal(frame) => frame,
            Acquired::Outdated => {
                self.surface.configure(&self.device, &self.config);
                self.window.request_redraw();
                return Ok(None);
            }
            Acquired::Timeout => {
                self.window.request_redraw();
                return Ok(None);
            }
            Acquired::Lost => {
                // Recover a lost SURFACE on the same device. A lost DEVICE is
                // the `lost` flag's business, at the top of this function.
                self.surface = self
                    .instance
                    .create_surface(wgpu::SurfaceTarget::from_window_without_display(
                        self.window.clone(),
                    ))
                    .map_err(|e| e.to_string())?;
                self.surface.configure(&self.device, &self.config);
                self.renderer.invalidate();
                self.window.request_redraw();
                return Ok(None);
            }
            _ => return Ok(None), // Occluded/validation: do not spin while hidden.
        };
        let view = frame.texture.create_view(&Default::default());
        let stats = match overlay {
            Some(draw) => self
                .renderer
                .render_with_overlay(scene, transform, &view, draw),
            None => self.renderer.render(scene, transform, &view),
        }
        .map_err(|e| e.to_string())?;
        self.window.pre_present_notify();
        frame.present();
        Ok(Some(stats))
    }
}
/// An adapter, a device and a renderer for `surface`, configured to the
/// window's size. The first frame and every device loss both come through here.
async fn open(
    instance: &wgpu::Instance,
    surface: &wgpu::Surface<'static>,
    window: &Window,
) -> Result<Device, String> {
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(surface),
            ..Default::default()
        })
        .await
        .map_err(|e| e.to_string())?;
    eprintln!("MUI GPU adapter: {:?}", adapter.get_info());
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .map_err(|e| e.to_string())?;
    let lost = Arc::new(AtomicBool::new(false));
    let flag = lost.clone();
    device.set_device_lost_callback(move |_, _| flag.store(true, Ordering::Release));
    let size = window.inner_size();
    let limit = device.limits().max_texture_dimension_2d;
    let (width, height) =
        target_size(size.width.min(limit), size.height.min(limit)).unwrap_or((1, 1));
    let caps = surface.get_capabilities(&adapter);
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|f| {
            matches!(
                f,
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
            )
        })
        .ok_or("MUI's selected alpha contract requires a non-sRGB UNORM surface")?;
    let mut config = surface
        .get_default_config(&adapter, width, height)
        .ok_or("surface config")?;
    config.format = format;
    surface.configure(&device, &config);
    let renderer = if std::env::var("MUI_TILED").as_deref() == Ok("1") {
        Renderer::Tiled(
            TiledEffects::new(
                &device,
                &queue,
                format,
                [width, height],
                Budget::default(),
                64 * 1024 * 1024,
            )
            .await
            .map_err(|e| e.to_string())?,
        )
    } else {
        Renderer::Whole(
            HybridEffects::new(&device, &queue, format, [width, height], Budget::default())
                .await
                .map_err(|e| e.to_string())?,
        )
    };
    Ok(Device {
        device,
        config,
        renderer,
        lost,
    })
}
#[cfg(test)]
mod tests {
    use super::target_size;
    #[test]
    fn zero_size_is_not_configured() {
        assert_eq!(target_size(0, 600), None);
        assert_eq!(target_size(800, 0), None);
    }
    #[test]
    fn dimensions_do_not_wrap() {
        assert_eq!(target_size(70000, 600), Some((65535, 600)));
    }
}
