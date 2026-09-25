//! The gallery host: one device, one queue, one `GpuRenderer`. This is a winit host, NOT a
//! CLAP/VST3 child-window adapter or proof of cross-platform DAW integration.
use crate::device::OnDevice;
use mui::scene::ResolvedScene;
use mui::vello::{
    effects::{Budget, EffectStats, GpuRenderer},
    kurbo::Affine,
};
use std::sync::Arc;
use winit::window::Window;

pub fn target_size(width: u32, height: u32) -> Option<(u32, u32)> {
    (width > 0 && height > 0).then_some((width.min(65535), height.min(65535)))
}

pub struct Gpu {
    instance: wgpu::Instance,
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    gpu: OnDevice<Device>,
    drawable: bool,
}
/// Everything that lives on one device: rebuilt whole when it is lost.
struct Device {
    config: wgpu::SurfaceConfiguration,
    renderer: GpuRenderer,
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
        let gpu = OnDevice::open(&instance, Some(&surface), build(&surface, &window))?;
        Ok(Self {
            instance,
            window,
            surface,
            gpu,
            drawable: size.width > 0 && size.height > 0,
        })
    }
    pub fn window(&self) -> &Window {
        &self.window
    }
    pub fn size(&self) -> (u32, u32) {
        (self.gpu.state.config.width, self.gpu.state.config.height)
    }
    pub fn resize(&mut self, width: u32, height: u32) {
        let Some((width, height)) = target_size(width, height) else {
            self.drawable = false;
            return;
        };
        self.drawable = true;
        let max = self.gpu.device.limits().max_texture_dimension_2d;
        let (width, height) = (width.min(max), height.min(max));
        if (width, height) == self.size() {
            return;
        }
        let Device { config, renderer } = &mut self.gpu.state;
        if let Err(e) = renderer.resize([width, height]) {
            eprintln!("MUI resize: {e}");
            return;
        }
        config.width = width;
        config.height = height;
        self.surface.configure(&self.gpu.device, &*config);
    }
    pub fn present(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
    ) -> Result<Option<EffectStats>, String> {
        self.present_inner::<fn(&mut mui::vello::Classic<'_>)>(scene, transform, None)
    }
    pub fn present_with_overlay<F: FnOnce(&mut mui::vello::Classic<'_>)>(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        overlay: F,
    ) -> Result<Option<EffectStats>, String> {
        self.present_inner(scene, transform, Some(overlay))
    }
    fn present_inner<F: FnOnce(&mut mui::vello::Classic<'_>)>(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        overlay: Option<F>,
    ) -> Result<Option<EffectStats>, String> {
        if !self.drawable {
            return Ok(None);
        }
        let rebuild = build(&self.surface, &self.window);
        if self
            .gpu
            .recover(&self.instance, Some(&self.surface), rebuild)?
        {
            self.window.request_redraw();
            return Ok(None);
        }
        let (device, Device { config, renderer }) = (&self.gpu.device, &mut self.gpu.state);
        use wgpu::CurrentSurfaceTexture as Acquired;
        let frame = match self.surface.get_current_texture() {
            Acquired::Success(frame) | Acquired::Suboptimal(frame) => frame,
            Acquired::Outdated => {
                self.surface.configure(device, &*config);
                self.window.request_redraw();
                return Ok(None);
            }
            Acquired::Timeout => {
                self.window.request_redraw();
                return Ok(None);
            }
            Acquired::Lost => {
                // Recover a lost SURFACE on the same device. A lost DEVICE is
                // `OnDevice::recover`'s business, at the top of this function.
                self.surface = self
                    .instance
                    .create_surface(wgpu::SurfaceTarget::from_window_without_display(
                        self.window.clone(),
                    ))
                    .map_err(|e| e.to_string())?;
                self.surface.configure(device, &*config);
                renderer.invalidate();
                self.window.request_redraw();
                return Ok(None);
            }
            _ => return Ok(None), // Occluded/validation: do not spin while hidden.
        };
        let view = frame.texture.create_view(&Default::default());
        let stats = match overlay {
            Some(draw) => renderer.render_with_overlay(scene, transform, &view, draw),
            None => renderer.render(scene, transform, &view),
        }
        .map_err(|e| e.to_string())?;
        self.window.pre_present_notify();
        self.gpu.queue.present(frame);
        Ok(Some(stats))
    }
}
/// The renderer and surface configuration for `surface` on a device: the
/// first frame and every device loss both come through here.
fn build<'a>(
    surface: &'a wgpu::Surface<'static>,
    window: &'a Window,
) -> impl FnOnce(&wgpu::Adapter, &wgpu::Device, &wgpu::Queue) -> Result<Device, String> + 'a {
    move |adapter, device, queue| {
        let size = window.inner_size();
        let limit = device.limits().max_texture_dimension_2d;
        let (width, height) =
            target_size(size.width.min(limit), size.height.min(limit)).unwrap_or((1, 1));
        let caps = surface.get_capabilities(adapter);
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
            .get_default_config(adapter, width, height)
            .ok_or("surface config")?;
        config.format = format;
        surface.configure(device, &config);
        let renderer = pollster::block_on(GpuRenderer::new(
            device,
            queue,
            format,
            [width, height],
            Budget::default(),
        ))
        .map_err(|e| e.to_string())?;
        Ok(Device { config, renderer })
    }
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
