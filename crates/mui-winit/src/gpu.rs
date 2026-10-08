//! The reusable standalone GPU adapter: a winit window over `mui_vello::host`, which owns the
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
    current: bool,
}

impl Gpu {
    pub fn new(window: Arc<Window>, display: Box<winit::event_loop::OwnedDisplayHandle>) -> Self {
        Self::try_new(window, display).expect("initialize MUI GPU host")
    }
    pub fn try_new(
        window: Arc<Window>,
        display: Box<winit::event_loop::OwnedDisplayHandle>,
    ) -> Result<Self, String> {
        let size = window.inner_size();
        let window_ref = &window;
        let host = Host::open_native(
            move |backends| {
                let operation = mui::diagnostics::operation(
                    "mui-winit",
                    "create_instance",
                    &format!("backends={backends:?}"),
                    None,
                );
                let mut descriptor =
                    wgpu::InstanceDescriptor::new_with_display_handle_from_env(display.clone());
                descriptor.backends = backends;
                let instance = wgpu::Instance::new(descriptor);
                drop(operation);
                let surface = surface(&instance, window_ref)?;
                Ok((instance, surface))
            },
            (size.width, size.height),
        )?;
        Ok(Self {
            window,
            host,
            current: false,
        })
    }
    pub fn window(&self) -> &Window {
        &self.window
    }
    pub fn size(&self) -> (u32, u32) {
        self.host.size()
    }
    pub fn resize(&mut self, width: u32, height: u32) {
        if let Err(e) = self.try_resize(width, height) {
            eprintln!("MUI {e}");
        }
    }
    /// Resize with an observable failure, for hosts that must verify presentation.
    pub fn try_resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.host.resize(width, height).map_err(|e| e.to_string())
    }
    /// Actual selected adapter and all failed initialization candidates.
    pub fn diagnostics(&self) -> &mui::vello::host::GpuDiagnostics {
        self.host.diagnostics()
    }
    pub fn present(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
    ) -> Result<Option<EffectStats>, String> {
        self.current = false;
        self.window.pre_present_notify();
        let frame = self
            .host
            .present(scene, transform)
            .map_err(|e| e.to_string())?;
        self.after(&frame)
    }
    pub fn present_with_overlay<F: FnOnce(&mut mui::vello::Classic<'_>)>(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        overlay: F,
    ) -> Result<Option<EffectStats>, String> {
        self.current = false;
        self.window.pre_present_notify();
        let frame = self
            .host
            .present_with_overlay(scene, transform, overlay)
            .map_err(|e| e.to_string())?;
        self.after(&frame)
    }
    /// The last present reused current pixels instead of submitting another frame.
    pub fn frame_was_current(&self) -> bool {
        self.current
    }
    /// A frame that did not reach the screen asks for another.
    fn after(&mut self, frame: &Frame) -> Result<Option<EffectStats>, String> {
        self.current = matches!(frame, Frame::Current);
        match frame {
            Frame::Presented(stats) => return Ok(Some(*stats)),
            Frame::Current => return Ok(None),
            Frame::Skipped => {}
            Frame::SurfaceLost => {
                let surface = surface(self.host.instance(), &self.window)?;
                self.host
                    .try_replace_surface(surface)
                    .map_err(|error| error.to_string())?;
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
    let _operation = mui::diagnostics::operation(
        "mui-winit",
        "create_surface",
        "creating window surface",
        None,
    );
    instance
        .create_surface(wgpu::SurfaceTarget::from_window_without_display(
            window.clone(),
        ))
        .map_err(|e| e.to_string())
}
