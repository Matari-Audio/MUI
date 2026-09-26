//! The gallery host: a winit window over `mui_vello::host`, which owns the
//! device, the loss handling and the renderer. This is a winit host, NOT a
//! CLAP/VST3 child-window adapter or proof of cross-platform DAW integration.
use mui::scene::ResolvedScene;
use mui::vello::effects::EffectStats;
use mui::vello::host::{Frame, Host};
use mui::vello::kurbo::Affine;
use std::sync::Arc;
use winit::window::Window;

pub struct Gpu {
    window: Arc<Window>,
    host: Host,
}

impl Gpu {
    pub fn new(window: Arc<Window>, display: Box<winit::event_loop::OwnedDisplayHandle>) -> Self {
        Self::try_new(window, display).expect("initialize MUI GPU gallery")
    }
    fn try_new(
        window: Arc<Window>,
        display: Box<winit::event_loop::OwnedDisplayHandle>,
    ) -> Result<Self, String> {
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(display),
        );
        let surface = surface(&instance, &window)?;
        let size = window.inner_size();
        let host = Host::new(instance, surface, (size.width, size.height))?;
        Ok(Self { window, host })
    }
    pub fn window(&self) -> &Window {
        &self.window
    }
    pub fn size(&self) -> (u32, u32) {
        self.host.size()
    }
    pub fn resize(&mut self, width: u32, height: u32) {
        if let Err(e) = self.host.resize(width, height) {
            eprintln!("MUI {e}");
        }
    }
    pub fn present(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
    ) -> Result<Option<EffectStats>, String> {
        self.window.pre_present_notify();
        let frame = self.host.present(scene, transform)?;
        self.after(&frame)
    }
    pub fn present_with_overlay<F: FnOnce(&mut mui::vello::Classic<'_>)>(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        overlay: F,
    ) -> Result<Option<EffectStats>, String> {
        self.window.pre_present_notify();
        let frame = self.host.present_with_overlay(scene, transform, overlay)?;
        self.after(&frame)
    }
    /// A frame that did not reach the screen asks for another.
    fn after(&mut self, frame: &Frame) -> Result<Option<EffectStats>, String> {
        match frame {
            Frame::Presented(stats) => return Ok(Some(*stats)),
            Frame::Skipped => {}
            Frame::SurfaceLost => {
                let surface = surface(self.host.instance(), &self.window)?;
                self.host.replace_surface(surface);
            }
        }
        self.window.request_redraw();
        Ok(None)
    }
}

fn surface(
    instance: &wgpu::Instance,
    window: &Arc<Window>,
) -> Result<wgpu::Surface<'static>, String> {
    instance
        .create_surface(wgpu::SurfaceTarget::from_window_without_display(
            window.clone(),
        ))
        .map_err(|e| e.to_string())
}
