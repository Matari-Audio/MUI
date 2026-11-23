//! Standalone presenter host over our owned `moose-baseview` + `mui-baseview`
//! window layer: CPU-first startup, asynchronous GPU promotion, and runtime
//! fallback, with zero `winit` dependency.
use std::time::Instant;

use mui::scene::ResolvedScene;
use mui::vello::effects::EffectStats;
use mui::vello::host::{FailureClass, Frame, GpuDiagnostics, Retry, Transparency};
use mui::vello::kurbo::Affine;
use mui_baseview::Presenter;
use mui_baseview::baseview::WindowContext;

pub struct Gpu {
    presenter: Presenter,
    cx: WindowContext,
    current: bool,
    paint_retry: Retry,
}

impl Gpu {
    pub fn try_new(cx: WindowContext) -> Result<Self, String> {
        let physical = cx.size().physical;
        let size = (physical.width.max(1), physical.height.max(1));
        let presenter = Presenter::new(cx.platform_handle(), size, Transparency::Opaque)?;
        Ok(Self {
            presenter,
            cx,
            current: false,
            paint_retry: Retry::default(),
        })
    }

    pub fn window(&self) -> &WindowContext {
        &self.cx
    }

    pub fn size(&self) -> (u32, u32) {
        self.presenter.size()
    }

    pub fn scale_factor(&self) -> f64 {
        self.cx.scale_factor()
    }

    pub fn set_title(&self, title: &str) {
        let _ = self.cx.set_title(title);
    }

    pub fn update(&mut self) {
        if self.presenter.prepare() {
            self.cx.request_redraw();
        }
    }

    pub fn next_wake(&self, now: Instant) -> Option<Instant> {
        self.presenter
            .next_wake(now)
            .into_iter()
            .chain(self.paint_retry.deadline())
            .min()
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.presenter.resize(width, height);
    }

    pub fn try_resize(&mut self, width: u32, height: u32) -> Result<(), String> {
        self.presenter.resize(width, height);
        Ok(())
    }

    pub fn invalidate(&mut self) {
        self.presenter.invalidate();
    }

    pub fn diagnostics(&self) -> Option<&GpuDiagnostics> {
        self.presenter.diagnostics()
    }

    pub fn rendering_mode(&self) -> &'static str {
        if self.presenter.using_gpu() {
            "gpu"
        } else {
            "cpu"
        }
    }

    pub fn gpu_error(&self) -> Option<&str> {
        self.presenter.gpu_error()
    }

    pub fn frame_was_current(&self) -> bool {
        self.current
    }

    pub fn present(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
    ) -> Result<Option<EffectStats>, String> {
        self.update();
        let size = self.presenter.size();
        if size.0 == 0 || size.1 == 0 || !self.paint_retry.ready(Instant::now()) {
            self.current = false;
            return Ok(None);
        }
        let frame = self.presenter.present(scene, transform)?;
        Ok(self.after(&frame))
    }

    pub fn present_with_overlay(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        overlay: impl FnOnce(&mut dyn mui::vello::Canvas),
    ) -> Result<Option<EffectStats>, String> {
        self.update();
        let size = self.presenter.size();
        if size.0 == 0 || size.1 == 0 || !self.paint_retry.ready(Instant::now()) {
            self.current = false;
            return Ok(None);
        }
        let frame = self
            .presenter
            .present_with_overlay(scene, transform, overlay)?;
        Ok(self.after(&frame))
    }

    fn after(&mut self, frame: &Frame) -> Option<EffectStats> {
        self.current = matches!(frame, Frame::Current);
        match frame {
            Frame::Presented(stats) => {
                self.paint_retry.reset();
                Some(*stats)
            }
            Frame::Current => {
                self.paint_retry.reset();
                None
            }
            Frame::Skipped if self.presenter.using_gpu() => {
                self.paint_retry
                    .fail(Instant::now(), FailureClass::Transient);
                None
            }
            Frame::Skipped | Frame::SurfaceLost => {
                self.paint_retry.reset();
                self.cx.request_redraw();
                None
            }
        }
    }
}
