//! A window's direct presenter for a host that drives its own model on the
//! window's thread: CPU first, then promoted to a GPU surface, with surface-
//! and device-loss recovery. Unlike [`crate::open`]'s worker, nothing is read
//! back: a translucent GPU surface keeps its alpha, and the device can be lent
//! to the app ([`Presenter::gpu`]) for textures it draws itself.
//!
//! Lifecycle, all on the window's thread:
//! 1. [`Presenter::new`]: a CPU surface (softbuffer), opaque.
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
    cpu_presented: bool,
    needs_rebuild: bool,
    software_only: bool,
    reason: &'static str,
    gpu_error: Option<String>,
    transitions: Vec<Transition>,
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

fn surface(
    instance: &wgpu::Instance,
    handle: PlatformHandle,
) -> Result<wgpu::Surface<'static>, String> {
    instance
        .create_surface(wgpu::SurfaceTarget::from(handle))
        .map_err(|e| e.to_string())
}

impl Presenter {
    /// A CPU presenter on `handle`'s window at `size` physical pixels. GPU
    /// promotion asks for `transparency`; CPU frames are always opaque.
    pub fn new(
        handle: PlatformHandle,
        size: (u32, u32),
        transparency: Transparency,
    ) -> Result<Self, String> {
        let software_only = mui::vello::host::force_cpu();
        Ok(Self {
            gpu: None,
            software: Some(software(handle.clone(), size)?),
            init: None,
            transparency,
            size,
            cpu_presented: false,
            needs_rebuild: false,
            software_only,
            reason: if software_only {
                "CPU forced"
            } else {
                "CPU startup"
            },
            gpu_error: None,
            transitions: Vec::new(),
            handle,
        })
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
        let Some(init) = self.init.as_ref() else {
            // Bootstrap the first GPU attempt after the initial CPU frame.
            return self.cpu_presented.then_some(now);
        };
        if let Some(deadline) = init.deadline().filter(|at| *at > now) {
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

    fn recover(&mut self, class: FailureClass, reason: &'static str, error: Option<String>) {
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
                "CPU GPU recovery",
                Some("GPU device lost".into()),
            );
            changed = true;
        }
        if self.gpu.is_none() && self.software.is_none() {
            self.software = software(self.handle.clone(), self.size).ok();
        }
        // GPU initialization starts only once CPU pixels are on screen.
        if !self.cpu_presented || self.gpu.is_some() || self.software_only {
            return changed;
        }
        let transparency = self.transparency;
        let init = self.init.get_or_insert_with(|| {
            GpuInit::new_with_transparency(
                baseview::pin_current_image_for_detached_work(),
                transparency,
            )
        });
        let handle = self.handle.clone();
        // Softbuffer's CPU CALayer must go before Metal installs its own.
        let cpu = &mut self.software;
        match init.poll(self.size, |instance| {
            *cpu = None;
            surface(instance, handle.clone())
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
                if self.software.is_none() {
                    self.software = software(self.handle.clone(), self.size).ok();
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
                    let replaced = surface(gpu.instance(), self.handle.clone())
                        .and_then(|s| gpu.try_replace_surface(s).map_err(|e| e.to_string()));
                    if let Err(err) = replaced {
                        self.recover(FailureClass::Transient, "CPU surface recovery", Some(err));
                    }
                    Ok(Frame::Skipped)
                }
                Ok(frame) => Ok(frame),
                Err(error) => {
                    self.recover(
                        error.failure_class(),
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
        let drew = match overlay {
            Some(overlay) => software.present_with_overlay(scene, transform, self.size, overlay)?,
            None => software.present(scene, transform, self.size)?,
        };
        self.cpu_presented |= drew;
        Ok(if drew {
            Frame::Presented(mui::vello::effects::EffectStats::default())
        } else {
            Frame::Current
        })
    }
}
