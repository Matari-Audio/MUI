//! The standalone renderer: GPU first, with native CPU presentation after a GPU failure.
//! This is a winit host, NOT a
//! CLAP/VST3 child-window adapter or proof of cross-platform DAW integration.
use mui::scene::ResolvedScene;
use mui::vello::effects::EffectStats;
use mui::vello::host::{Frame, Host};
use mui::vello::kurbo::Affine;
use std::sync::Arc;
use winit::window::Window;

pub struct Gpu {
    window: Arc<Window>,
    host: Option<Host>,
    software: Option<mui::vello::software::Window<Arc<Window>>>,
    size: (u32, u32),
    gpu_error: Option<String>,
    current: bool,
    _reporting: mui::diagnostics::ReportingGuard,
}

impl Gpu {
    pub fn new(window: Arc<Window>, display: Box<winit::event_loop::OwnedDisplayHandle>) -> Self {
        Self::try_new(window, display).expect("initialize MUI window renderer")
    }
    pub fn try_new(
        window: Arc<Window>,
        display: Box<winit::event_loop::OwnedDisplayHandle>,
    ) -> Result<Self, String> {
        let size = window.inner_size();
        let size = (size.width, size.height);
        let mut renderer = Self {
            window,
            host: None,
            software: None,
            size,
            gpu_error: None,
            current: false,
            _reporting: mui::diagnostics::retain_reporter(),
        };
        if std::env::var("MUI_RENDERER").is_ok_and(|v| v.eq_ignore_ascii_case("cpu")) {
            renderer.open_software()?;
            return Ok(renderer);
        }
        let window_ref = &renderer.window;
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
            size,
        );
        match host {
            Ok(host) => renderer.host = Some(host),
            Err(error) => renderer.fallback(error)?,
        }
        Ok(renderer)
    }
    pub fn window(&self) -> &Window {
        &self.window
    }
    pub fn size(&self) -> (u32, u32) {
        self.size
    }
    pub fn resize(&mut self, width: u32, height: u32) {
        if let Err(e) = self.try_resize(width, height) {
            eprintln!("MUI {e}");
        }
    }
    /// Resize with an observable failure, for hosts that must verify presentation.
    pub fn try_resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.size = (width, height);
        if let Some(host) = &mut self.host
            && let Err(error) = host.resize(width, height)
        {
            self.fallback(error.to_string())?;
        }
        if let Some(software) = &mut self.software {
            software.resize((width, height))?;
        }
        Ok(())
    }
    /// Actual GPU candidates, absent when presenting without a GPU.
    pub fn diagnostics(&self) -> Option<&mui::vello::host::GpuDiagnostics> {
        self.host.as_ref().map(Host::diagnostics)
    }
    pub fn rendering_mode(&self) -> &'static str {
        if self.host.is_some() { "gpu" } else { "cpu" }
    }
    /// The GPU failure that selected CPU presentation; also persisted in diagnostics.
    pub fn gpu_error(&self) -> Option<&str> {
        self.gpu_error.as_deref()
    }
    /// Re-present retained CPU pixels when the OS exposes the window.
    pub fn invalidate(&mut self) {
        if let Some(software) = &mut self.software {
            software.invalidate();
        }
    }
    fn open_software(&mut self) -> Result<(), String> {
        self.software = Some(
            mui::vello::software::Window::new(
                self.window.clone(),
                (self.size.0.max(1), self.size.1.max(1)),
            )
            .map_err(|error| format!("CPU presentation: {error}"))?,
        );
        mui::diagnostics::breadcrumb("mui-winit", "renderer_ready", "cpu");
        Ok(())
    }
    fn fallback(&mut self, error: String) -> Result<(), String> {
        mui::diagnostics::error("mui-winit", "gpu_fallback", &error);
        eprintln!("mui-winit: GPU unavailable ({error}); using CPU rendering");
        self.gpu_error = Some(error);
        // Drop the GPU surface before another presenter takes ownership of the window.
        self.host = None;
        self.current = false;
        self.window.request_redraw();
        self.open_software()
            .map_err(|error| format!("{}; {error}", self.gpu_error.as_deref().unwrap_or_default()))
    }
    pub fn present(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
    ) -> Result<Option<EffectStats>, String> {
        self.present_inner(scene, transform, None::<fn(&mut dyn mui::vello::Canvas)>)
    }
    /// Draw an overlay on either renderer through the shared canvas API.
    pub fn present_with_overlay(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        overlay: impl FnOnce(&mut dyn mui::vello::Canvas),
    ) -> Result<Option<EffectStats>, String> {
        self.present_inner(scene, transform, Some(overlay))
    }
    fn present_inner(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        overlay: Option<impl FnOnce(&mut dyn mui::vello::Canvas)>,
    ) -> Result<Option<EffectStats>, String> {
        self.current = false;
        if self.size.0 == 0 || self.size.1 == 0 {
            return Ok(None);
        }
        self.window.pre_present_notify();
        if let Some(host) = &mut self.host {
            let frame = match overlay {
                Some(overlay) => {
                    host.present_with_overlay(scene, transform, |canvas| overlay(canvas))
                }
                None => host.present(scene, transform),
            };
            let result = frame
                .map_err(|e| e.to_string())
                .and_then(|frame| self.after(&frame));
            return match result {
                Ok(stats) => Ok(stats),
                Err(error) => {
                    self.fallback(error.clone())?;
                    // The next frame must resolve GPU-only materials for the CPU.
                    Err(error)
                }
            };
        }
        if self.software.is_none() {
            self.open_software()?;
        }
        let Some(software) = &mut self.software else {
            return Err("CPU presenter unavailable".into());
        };
        let presented = match overlay {
            Some(overlay) => software.present_with_overlay(scene, transform, self.size, overlay),
            None => software.present(scene, transform, self.size),
        }?;
        self.current = !presented;
        // CPU submission has no GPU effect counters, including on retained-pixel exposes.
        Ok(presented.then_some(EffectStats::default()))
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
                let Some(host) = &mut self.host else {
                    return Err("GPU presenter unavailable".into());
                };
                let surface = surface(host.instance(), &self.window)?;
                host.try_replace_surface(surface)
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
