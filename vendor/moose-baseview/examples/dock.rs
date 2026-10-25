//! Native dock smoke probe. Closes after two seconds; no audio device is opened.
use baseview::{
    DockEdge, DockOptions, Event, EventStatus, HandlerError, Window, WindowContext, WindowHandler,
    WindowSettings, WindowSize,
};
use moose_baseview as baseview;
use std::{
    cell::RefCell,
    num::NonZeroU32,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

struct Probe {
    surface: RefCell<softbuffer::Surface<baseview::PlatformHandle, baseview::PlatformHandle>>,
    cx: WindowContext,
    deadline: Instant,
    frames: Arc<AtomicUsize>,
}
impl WindowHandler for Probe {
    fn on_frame(&self) -> Result<(), HandlerError> {
        let size = self.cx.size().physical;
        if let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
            let mut surface = self.surface.borrow_mut();
            surface.resize(w, h)?;
            let mut buffer = surface.buffer_mut()?;
            for (i, pixel) in buffer.iter_mut().enumerate() {
                *pixel = if i % (size.width as usize) < size.width as usize / 2 {
                    0x00234d5d
                } else {
                    0x00493668
                };
            }
            buffer.present()?;
            self.frames.fetch_add(1, Ordering::Relaxed);
        }
        if Instant::now() >= self.deadline {
            self.cx.request_close();
        }
        Ok(())
    }
    fn resized(&self, _: WindowSize) -> Result<(), HandlerError> {
        Ok(())
    }
    fn on_event(&self, _: Event) -> EventStatus {
        EventStatus::Captured
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let frames = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&frames);
    let window = Window::create(
        WindowSettings::new()
            .with_title("MUI dock probe")
            .with_dock(DockOptions::new(DockEdge::Top, 160)),
        move |cx| {
            let context = softbuffer::Context::new(cx.platform_handle())?;
            let surface = softbuffer::Surface::new(&context, cx.platform_handle())?;
            Ok(Probe {
                surface: RefCell::new(surface),
                cx,
                deadline: Instant::now() + Duration::from_secs(2),
                frames: counter,
            })
        },
    )?;
    window.run_until_closed()?;
    assert!(frames.load(Ordering::Relaxed) > 0, "the dock must submit a native CPU frame");
    Ok(())
}
