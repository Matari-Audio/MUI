//! The standalone renderer: GPU first, with native CPU presentation after a GPU failure.
//! This is a winit host, NOT a
//! CLAP/VST3 child-window adapter or proof of cross-platform DAW integration.
use mui::scene::ResolvedScene;
use mui::vello::effects::EffectStats;
use mui::vello::host::{FailureClass, Frame, GpuInit, Host, Retry};
use mui::vello::kurbo::Affine;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::window::Window;

pub struct Gpu {
    window: Arc<Window>,
    host: Option<Host>,
    init: Option<GpuInit>,
    retry: Retry,
    cpu_retry: Retry,
    gpu_permanent: bool,
    next_check: Instant,
    software: Option<mui::vello::software::Window<Arc<Window>>>,
    size: (u32, u32),
    gpu_error: Option<String>,
    current: bool,
    _display: Box<winit::event_loop::OwnedDisplayHandle>,
    _reporting: mui::diagnostics::ReportingGuard,
}

impl Gpu {
    pub fn new(window: Arc<Window>, display: Box<winit::event_loop::OwnedDisplayHandle>) -> Self {
        // Construction cannot fail a host process. Fallible CPU setup is
        // retried on a paced frame and reported through gpu_error.
        Self::create(window, display)
    }
    pub fn try_new(
        window: Arc<Window>,
        display: Box<winit::event_loop::OwnedDisplayHandle>,
    ) -> Result<Self, String> {
        Ok(Self::create(window, display))
    }
    fn create(window: Arc<Window>, display: Box<winit::event_loop::OwnedDisplayHandle>) -> Self {
        let size = window.inner_size();
        Self {
            window,
            host: None,
            init: None,
            retry: Retry::default(),
            cpu_retry: Retry::default(),
            gpu_permanent: false,
            next_check: Instant::now(),
            software: None,
            size: (size.width, size.height),
            gpu_error: None,
            current: false,
            _display: display,
            _reporting: mui::diagnostics::retain_reporter(),
        }
    }
    /// Called before resolving the scene so a handover rebuilds GPU materials
    /// from the same model and size, without submitting an old CPU-only scene.
    pub fn update(&mut self) {
        self.next_check = Instant::now() + Duration::from_millis(25);
        if self.host.is_some() {
            return;
        }
        let Some(init) = &mut self.init else { return };
        let software = &mut self.software;
        if let Some(result) = init.poll(self.size, |instance| {
            drop(software.take());
            surface(instance, &self.window)
        }) {
            match result {
                Ok(host) => {
                    self.host = Some(host);
                    self.gpu_error = None;
                    self.retry.reset();
                    self.cpu_retry.reset();
                    // Direct present() callers may install after scene resolve.
                    // One redraw rebuilds the now GPU-capable material scene.
                    self.window.request_redraw();
                }
                Err(error) => {
                    self.gpu_permanent = error.failure_class() == FailureClass::Permanent;
                    if self.gpu_permanent {
                        self.retry.reset();
                    }
                    if self.gpu_permanent
                        && let Some(software) = &mut self.software
                    {
                        software.finish_startup(self.size);
                    }
                    self.gpu_error = Some(error.to_string());
                }
            }
        }
    }
    pub fn next_wake(&self, _now: Instant) -> Option<Instant> {
        [
            self.init
                .as_ref()
                .filter(|i| i.pending())
                .map(|_| self.next_check),
            self.init.as_ref().and_then(GpuInit::deadline),
            self.retry.deadline(),
            self.cpu_retry.deadline(),
        ]
        .into_iter()
        .flatten()
        .min()
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
            let class = error.failure_class();
            self.fallback(error.to_string(), class)?;
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
        let open = if self.gpu_permanent
            || std::env::var("MUI_RENDERER").is_ok_and(|v| v.eq_ignore_ascii_case("cpu"))
        {
            mui::vello::software::Window::new
        } else {
            mui::vello::software::Window::new_startup
        };
        self.software = Some(
            open(
                self.window.clone(),
                (self.size.0.max(1), self.size.1.max(1)),
            )
            .map_err(|error| format!("CPU presentation: {error}"))?,
        );
        mui::diagnostics::breadcrumb("mui-winit", "renderer_ready", "cpu");
        Ok(())
    }
    fn fallback(&mut self, error: String, class: FailureClass) -> Result<(), String> {
        mui::diagnostics::error("mui-winit", "gpu_fallback", &error);
        eprintln!("mui-winit: GPU unavailable ({error}); using CPU rendering");
        self.gpu_error = Some(error);
        // Drop the GPU surface before another presenter takes ownership of the window.
        self.host = None;
        self.gpu_permanent = class == FailureClass::Permanent;
        if let Some(init) = &mut self.init {
            init.recover(class);
        }
        self.retry.fail(Instant::now(), class);
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
        // Direct callers also make progress. Main App polls before resolving
        // its material scene, so handover immediately switches GPU welding.
        self.update();
        self.current = false;
        if self.size.0 == 0 || self.size.1 == 0 {
            return Ok(None);
        }
        if (!self.retry.ready(Instant::now()) && self.software.is_none())
            || (self.host.is_none() && !self.cpu_retry.ready(Instant::now()))
        {
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
            let class = frame.as_ref().err().map_or(
                FailureClass::Transient,
                mui::vello::host::HostError::failure_class,
            );
            let result = frame
                .map_err(|e| e.to_string())
                .and_then(|frame| self.after(&frame));
            return match result {
                Ok(stats) => Ok(stats),
                Err(error) => {
                    self.fallback(error.clone(), class)?;
                    // The next frame must resolve GPU-only materials for the CPU.
                    Err(error)
                }
            };
        }
        if self.software.is_none()
            && let Err(error) = self.open_software()
        {
            self.cpu_retry.fail(Instant::now(), FailureClass::Transient);
            return Err(error);
        }
        let Some(software) = &mut self.software else {
            return Err("CPU presenter unavailable".into());
        };
        let presented = match overlay {
            Some(overlay) => software.present_with_overlay(scene, transform, self.size, overlay),
            None => software.present(scene, transform, self.size),
        };
        let presented = match presented {
            Ok(presented) => {
                self.cpu_retry.reset();
                presented
            }
            Err(error) => {
                self.cpu_retry.fail(Instant::now(), FailureClass::Transient);
                return Err(error);
            }
        };
        self.current = !presented;
        if presented
            && self.init.is_none()
            && !std::env::var("MUI_RENDERER").is_ok_and(|v| v.eq_ignore_ascii_case("cpu"))
        {
            self.init = Some(GpuInit::new(baseview::pin_current_image_for_detached_work()));
        }
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
            Frame::Presented(stats) => {
                self.retry.reset();
                return Ok(Some(*stats));
            }
            Frame::Current => {
                self.retry.reset();
                return Ok(None);
            }
            Frame::Skipped => {
                self.retry.fail(Instant::now(), FailureClass::Transient);
            }
            Frame::SurfaceLost => {
                self.retry.fail(Instant::now(), FailureClass::Transient);
                let Some(host) = &mut self.host else {
                    return Err("GPU presenter unavailable".into());
                };
                let surface = surface(host.instance(), &self.window)?;
                host.try_replace_surface(surface)
                    .map_err(|error| error.to_string())?;
            }
        }
        // No immediate redraw on Timeout/Occluded: the event loop observes
        // next_wake and sleeps through capped backoff.
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
        // Shared instances intentionally have no borrowed display. This safe
        // target retains the window AND supplies its explicit display handle.
        .create_surface(window.clone())
        .map_err(|e| e.to_string())
}
