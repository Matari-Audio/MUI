//! A window's direct presenter for a host that drives its own model on the
//! window's thread: CPU first, then promoted to a GPU surface, with surface-
//! and device-loss recovery. Unlike [`crate::open`]'s worker, nothing is read
//! back: a translucent GPU surface keeps its alpha, and the device can be lent
//! to the app ([`Presenter::gpu`]) for textures it draws itself.
//!
//! Lifecycle, all on the window's thread:
//! 1. [`Presenter::new`]: an owned CPU surface (`mui-present`), opaque.
//! 2. The first submitted CPU frame starts [`GpuInit`] (off-thread when the
//!    library image can be pinned).
//! 3. [`Presenter::prepare`] adopts the finished device: the CPU surface goes,
//!    the GPU surface comes ([`Transition::Promoted`]).
//! 4. A lost surface is recreated in place; a lost device or a failed frame
//!    drops the GPU and returns to CPU frames ([`Transition::CpuFallback`]);
//!    a transient failure is promoted again after the next CPU frame.
//! 5. Drop: surfaces go before the retained [`PlatformHandle`].
use std::time::{Duration, Instant};

use baseview::PlatformHandle;
use mui::scene::ResolvedScene;
use mui::vello::effects::Fallback;
use mui::vello::host::{FailureClass, Frame, GpuDiagnostics, GpuInit, Host, Transparency};
use mui::vello::kurbo::Affine;
use mui::vello::software::Window as SoftwareWindow;

/// What changed under the presenter since the last [`Presenter::take_transitions`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    /// A GPU device and surface now present; the lent device is new.
    Promoted,
    /// Back on CPU frames; `permanent` means no GPU will be tried again.
    CpuFallback { permanent: bool },
}

/// Which presenter drew the last frame, and how often the GPU pass refused one.
#[derive(Clone, Debug, Default)]
pub struct PresenterStats {
    pub gpu: bool,
    /// Why the presenter is what it is ("GPU ready", "CPU startup", ...).
    pub reason: &'static str,
    /// The GPU pass's refusals; `None` on the CPU.
    pub fallback: Option<Fallback>,
}

/// See the [module](self) docs.
pub struct Presenter {
    gpu: Option<Host>,
    software: Option<SoftwareWindow<PlatformHandle>>,
    init: Option<GpuInit>,
    transparency: Transparency,
    size: (u32, u32),
    scale: f64,
    /// The GPU surface's own layer (a Wayland child subsurface).
    layer: crate::surface::Layer,
    cpu_presented: bool,
    needs_rebuild: bool,
    software_only: bool,
    reason: &'static str,
    gpu_error: Option<String>,
    transitions: Vec<Transition>,
    state: mui_present::PresentationState,
    /// Last: every surface above drops before the native handle.
    handle: PlatformHandle,
}

/// Full CPU quality inside the toolkit's 64 MiB target budget; larger HiDPI
/// windows submit a background frame so GPU initialization can still start.
fn software(
    handle: PlatformHandle,
    size: (u32, u32),
) -> Result<SoftwareWindow<PlatformHandle>, String> {
    if u64::from(size.0) * u64::from(size.1) <= 16 * 1024 * 1024 {
        SoftwareWindow::new(handle, size)
    } else {
        let mut window = SoftwareWindow::new_startup(handle, size)?;
        window.finish_startup(size);
        Ok(window)
    }
}

impl Presenter {
    /// A CPU presenter on `handle`'s window at `size` physical pixels. GPU
    /// promotion asks for `transparency`; CPU frames are always opaque.
    pub fn new(
        handle: PlatformHandle,
        size: (u32, u32),
        transparency: Transparency,
    ) -> Result<Self, String> {
        let software_only = mui_present::software_mode_enabled();
        let cpu = software(handle.clone(), size)?;
        let cpu_backend = cpu.backend();
        let reason = if software_only {
            "CPU forced"
        } else {
            "CPU startup"
        };
        let mut state = mui_present::PresentationState {
            mode: mui_present::PresentationMode::CpuDirect,
            backend: cpu_backend,
            surface_size: size,
            render_size: size,
            frames_presented: 0,
            translucent: false,
            software_forced: software_only,
            reason: reason.to_owned(),
            ..Default::default()
        };
        state.record_fallback(software_only.then_some(mui_present::FallbackReason::ForceCpu));
        Ok(Self {
            gpu: None,
            software: Some(cpu),
            init: None,
            transparency,
            size,
            scale: 1.0,
            layer: crate::surface::Layer::default(),
            cpu_presented: false,
            needs_rebuild: false,
            software_only,
            reason,
            gpu_error: None,
            transitions: Vec::new(),
            state,
            handle,
        })
    }

    /// Observable snapshot of the active presentation pipeline.
    #[must_use]
    pub fn state(&self) -> &mui_present::PresentationState {
        &self.state
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Actual GPU candidates, absent when presenting without a GPU.
    pub fn diagnostics(&self) -> Option<&GpuDiagnostics> {
        self.gpu.as_ref().map(Host::diagnostics)
    }

    /// Last GPU initialization or runtime failure that fell back to CPU.
    pub fn gpu_error(&self) -> Option<&str> {
        self.gpu_error.as_deref()
    }

    /// Next deadline needed by asynchronous GPU initialization or backoff.
    /// `None` when settled (GPU ready, or software-only / permanent fallback).
    pub fn next_wake(&self, now: Instant) -> Option<Instant> {
        if self.gpu.is_some() || self.software_only {
            return None;
        }
        let init = self.init.as_ref()?;
        if let Some(deadline) = init.deadline() {
            return Some(deadline);
        }
        init.pending().then(|| now + Duration::from_millis(25))
    }

    /// The GPU host, to lend its device ([`Host::device`]) or hand it
    /// textures ([`Host::set_texture`]). `None` while on CPU frames.
    pub fn gpu(&mut self) -> Option<&mut Host> {
        self.gpu.as_mut()
    }

    pub fn using_gpu(&self) -> bool {
        self.gpu.is_some()
    }

    /// The GPU surface composites alpha (a frosted window shows through).
    pub fn translucent(&self) -> bool {
        self.gpu.as_ref().is_some_and(Host::translucent)
    }

    /// Lost between presents: an idle window polls this to repaint.
    pub fn device_lost(&self) -> bool {
        self.gpu.as_ref().is_some_and(Host::device_lost)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.size = (width, height);
        self.layer.resize(self.logical());
    }

    /// The window's scale factor: physical pixels per logical point.
    pub fn set_scale_factor(&mut self, scale: f64) {
        if !scale.is_finite() || scale <= 0.0 || scale == self.scale {
            return;
        }
        self.scale = scale;
        if let Some(software) = &mut self.software {
            software.set_scale_factor(scale);
        }
        self.layer.resize(self.logical());
    }

    fn logical(&self) -> (u32, u32) {
        let points = |px: u32| (f64::from(px) / self.scale).round().max(1.0) as u32;
        (points(self.size.0), points(self.size.1))
    }

    fn open_software(&self) -> Option<SoftwareWindow<PlatformHandle>> {
        let mut window = software(self.handle.clone(), self.size).ok()?;
        window.set_scale_factor(self.scale);
        Some(window)
    }

    /// The next present submits even an unchanged scene, on whichever
    /// presenter is current, without a CPU overlay raster.
    pub fn invalidate(&mut self) {
        if let Some(gpu) = &mut self.gpu {
            gpu.invalidate();
        }
        if let Some(software) = &mut self.software {
            software.invalidate();
        }
    }

    pub fn stats(&self) -> PresenterStats {
        PresenterStats {
            gpu: self.gpu.is_some(),
            reason: self.reason,
            fallback: self.gpu.as_ref().map(|gpu| gpu.fallback().clone()),
        }
    }

    /// Promotions and fallbacks since the last call, oldest first.
    pub fn take_transitions(&mut self) -> Vec<Transition> {
        std::mem::take(&mut self.transitions)
    }

    fn recover(
        &mut self,
        class: FailureClass,
        fallback: mui_present::FallbackReason,
        reason: &'static str,
        error: Option<String>,
    ) {
        self.state.record_fallback(Some(fallback));
        let permanent = class == FailureClass::Permanent;
        self.transitions.push(Transition::CpuFallback { permanent });
        self.gpu = None;
        self.software_only |= permanent;
        if let Some(error) = error {
            self.gpu_error = Some(error);
        }
        if let Some(init) = &mut self.init {
            init.recover(class);
        }
        self.reason = reason;
        self.cpu_presented = false;
        self.needs_rebuild = true;
    }

    /// Once per frame before building: notices device loss, reopens a CPU
    /// surface after a fallback and adopts a finished GPU initialization.
    /// `true`: the presenter changed and the scene must be painted again.
    pub fn prepare(&mut self) -> bool {
        let mut changed = std::mem::take(&mut self.needs_rebuild);
        if self.device_lost() {
            self.recover(
                FailureClass::Transient,
                mui_present::FallbackReason::DeviceLost,
                "CPU GPU recovery",
                Some("GPU device lost".into()),
            );
            changed = true;
        }
        if self.gpu.is_none() && self.software.is_none() {
            self.software = self.open_software();
        }
        // GPU initialization starts only once CPU pixels are on screen.
        if !self.cpu_presented || self.gpu.is_some() || self.software_only {
            return changed;
        }
        let transparency = self.transparency;
        let logical = self.logical();
        let init = self.init.get_or_insert_with(|| {
            GpuInit::new_with_transparency(
                baseview::pin_current_image_for_detached_work(),
                transparency,
            )
        });
        let handle = self.handle.clone();
        // The CPU presenter's layer must go before the GPU installs its own.
        let (cpu, layer) = (&mut self.software, &mut self.layer);
        match init.poll(self.size, |instance| {
            *cpu = None;
            let (surface, made) = crate::surface::create(instance, handle.clone(), logical)?;
            *layer = made;
            Ok(surface)
        }) {
            Some(Ok(gpu)) => {
                self.gpu = Some(gpu);
                self.gpu_error = None;
                self.reason = "GPU ready";
                self.transitions.push(Transition::Promoted);
                true
            }
            Some(Err(error)) => {
                let permanent = error.failure_class() == FailureClass::Permanent;
                self.software_only |= permanent;
                self.gpu_error = Some(error.to_string());
                self.transitions.push(Transition::CpuFallback { permanent });
                self.reason = "CPU GPU initialization failed";
                self.state.record_fallback(Some(error.fallback_reason()));
                if self.software.is_none() {
                    self.software = self.open_software();
                }
                changed
            }
            None => changed,
        }
    }

    /// Paint `scene` under `transform`. `Frame::Current`: nothing changed
    /// since the last frame on screen. A GPU failure returns `Skipped` and
    /// falls back to CPU frames from the next [`Presenter::prepare`].
    pub fn present(&mut self, scene: &ResolvedScene, transform: Affine) -> Result<Frame, String> {
        self.present_inner(scene, transform, None::<fn(&mut dyn mui::vello::Canvas)>)
    }

    /// Paint `scene` under `transform`, followed by a host overlay drawn on
    /// whichever presenter (GPU or CPU) is active.
    pub fn present_with_overlay(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        overlay: impl FnOnce(&mut dyn mui::vello::Canvas),
    ) -> Result<Frame, String> {
        self.present_inner(scene, transform, Some(overlay))
    }

    fn present_inner(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        overlay: Option<impl FnOnce(&mut dyn mui::vello::Canvas)>,
    ) -> Result<Frame, String> {
        let logical = self.logical();
        if let Some(gpu) = self.gpu.as_mut() {
            let frame = gpu
                .resize(self.size.0, self.size.1)
                .and_then(|()| match overlay {
                    Some(overlay) => {
                        gpu.present_with_overlay(scene, transform, |canvas| overlay(canvas))
                    }
                    None => gpu.present(scene, transform),
                });
            return match frame {
                Ok(Frame::SurfaceLost) => {
                    let replaced =
                        crate::surface::create(gpu.instance(), self.handle.clone(), logical)
                            .and_then(|(surface, layer)| {
                                gpu.try_replace_surface(surface)
                                    .map_err(|e| e.to_string())?;
                                self.layer = layer;
                                Ok(())
                            });
                    if let Err(err) = replaced {
                        self.recover(
                            FailureClass::Transient,
                            mui_present::FallbackReason::SurfaceLost,
                            "CPU surface recovery",
                            Some(err),
                        );
                    }
                    Ok(Frame::Skipped)
                }
                Ok(frame) => {
                    if matches!(frame, Frame::Presented(_)) {
                        self.state.translucent = gpu.translucent();
                        self.state.api = gpu.api();
                        self.state.record_fallback(None);
                        self.state.record_frame(
                            mui_present::PresentationMode::DirectGpu,
                            mui_present::PresentationBackend::for_gpu(gpu.api()),
                            self.size,
                            self.size,
                            self.reason,
                        );
                    }
                    Ok(frame)
                }
                Err(error) => {
                    self.recover(
                        error.failure_class(),
                        error.fallback_reason(),
                        "CPU GPU recovery",
                        Some(error.to_string()),
                    );
                    Ok(Frame::Skipped)
                }
            };
        }
        let software = self
            .software
            .as_mut()
            .ok_or("native CPU presenter unavailable")?;
        let backend = software.backend();
        let drew = match overlay {
            Some(overlay) => software.present_with_overlay(scene, transform, self.size, overlay)?,
            None => software.present(scene, transform, self.size)?,
        };
        self.cpu_presented |= drew;
        if drew {
            self.state.translucent = false;
            self.state.api = mui_present::GraphicsApi::Software;
            self.state.record_frame(
                mui_present::PresentationMode::CpuDirect,
                backend,
                self.size,
                self.size,
                self.reason,
            );
        }
        Ok(if drew {
            Frame::Presented(mui::vello::effects::EffectStats::default())
        } else {
            Frame::Current
        })
    }
}
