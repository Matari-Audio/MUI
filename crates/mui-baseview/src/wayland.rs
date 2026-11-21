//! Native Wayland editors, driven by the same handler, presenters and request
//! queue as X11 windows. CLAP floats them as an xdg toplevel ([`open`]): hide
//! destroys the toplevel, and the plugin keeps its model and opens a new one on
//! show. VST3 embeds them as a subsurface of the host's surface ([`embed`]),
//! and CLAP's draft nested compositor places them by token ([`embed_nested`]);
//! the host's run loop pumps both.
use super::*;

/// An editor embedded in a host's Wayland surface. `!Send`: the host's UI
/// thread opens it, pumps it and drops it. Dropping it destroys the surface.
pub struct EmbeddedWindow {
    window: baseview::EmbeddedWaylandWindow,
    /// Only foreign subsurfaces need to coordinate with a host parent commit.
    resize_requests: Option<Arc<Requests>>,
}

impl EmbeddedWindow {
    /// Readable when [`Self::dispatch`] has work.
    pub fn fd(&self) -> std::os::fd::RawFd {
        use std::os::fd::AsRawFd;
        self.window.fd().as_raw_fd()
    }
    /// Handles Wayland events and draws a due frame, without blocking.
    pub fn dispatch(&mut self) -> Result<(), String> {
        if self
            .resize_requests
            .as_ref()
            .is_some_and(|r| r.size.load(Ordering::Acquire) != 0)
        {
            // The pending size is consumed by the paint handler on this same
            // thread. Arm sync before painting it, even if pacing is waiting
            // for a callback from the previous frame.
            self.window.synchronize_next_resize();
        }
        self.window.dispatch().map_err(|e| e.to_string())
    }
    /// Physical size and scale.
    pub fn size(&self) -> ((u32, u32), f64) {
        let size = self.window.size();
        (
            (size.physical.width, size.physical.height),
            size.scale_factor,
        )
    }
}

/// Embeds an editor of `size` logical points in the host's `surface`.
///
/// # Safety
/// `display` is the host's live `wl_display*` and `surface` a live
/// `wl_surface*` proxy on it. Both outlive the returned window, which stays
/// on the calling thread.
///
/// # Errors
/// The compositor has no subsurfaces, or the editor failed to build.
#[expect(
    unsafe_code,
    reason = "the host's Wayland objects arrive as raw pointers"
)]
pub unsafe fn embed<V: View + Send + 'static>(
    display: std::ptr::NonNull<std::ffi::c_void>,
    surface: std::ptr::NonNull<std::ffi::c_void>,
    size: (u32, u32),
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
) -> Result<EmbeddedWindow, String> {
    // SAFETY: forwarded from this function's contract.
    unsafe {
        baseview::EmbeddedWaylandWindow::open(
            settings("MUI", size),
            display,
            surface,
            build(shared, Arc::clone(&requests), true),
        )
    }
    .map(|window| EmbeddedWindow {
        window,
        resize_requests: Some(requests),
    })
    .map_err(|e| e.to_string())
}

/// Embeds an editor of `size` logical points through a host's nested
/// compositor: our own connection on `socket`, placed by `token`.
///
/// # Errors
/// The compositor lacks the embedding protocol, or the editor failed to build.
pub fn embed_nested<V: View + Send + 'static>(
    socket: std::os::fd::OwnedFd,
    token: &str,
    size: (u32, u32),
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
) -> Result<EmbeddedWindow, String> {
    baseview::EmbeddedWaylandWindow::open_nested(
        settings("MUI", size),
        socket,
        token,
        build(shared, requests, true),
    )
    .map(|window| EmbeddedWindow {
        window,
        resize_requests: None,
    })
    .map_err(|e| e.to_string())
}

/// An open floating Wayland editor. Dropping it closes the window.
pub struct FloatingWindow(Option<baseview::ThreadedWindow>);

impl FloatingWindow {
    fn window(&self) -> &baseview::ThreadedWindow {
        self.0.as_ref().expect("taken only by drop")
    }
    pub fn is_open(&self) -> bool {
        self.window().is_open()
    }
    /// Physical size and scale, as the compositor last configured them.
    pub fn size(&self) -> ((u32, u32), f64) {
        let size = self.window().size();
        (
            (size.physical.width, size.physical.height),
            size.scale_factor,
        )
    }
    pub fn title(&self, title: &str) {
        let _ = self.window().set_title(title);
    }
}

impl Drop for FloatingWindow {
    fn drop(&mut self) {
        // The owner thread retires a stalled renderer on its own; never block the host.
        if let Some(window) = self.0.take() {
            window.close_bounded(Duration::from_millis(250));
        }
    }
}

pub fn open<V: View + Send + 'static>(
    title: &str,
    size: (u32, u32),
    minimum: (u32, u32),
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
    closed: Arc<dyn Fn() + Send + Sync>,
) -> Result<FloatingWindow, String> {
    let closing = Arc::downgrade(&requests);
    requests.on_close(Arc::new(move || {
        if let Some(requests) = closing.upgrade() {
            requests.wayland_open.store(false, Ordering::Release);
        }
        closed();
    }));
    let opened = Arc::clone(&requests);
    let settings = settings(title, size)
        .with_wayland(true)
        .with_min_size(LogicalSize::new(f64::from(minimum.0), f64::from(minimum.1)));
    let window = baseview::ThreadedWindow::create(settings, build(shared, requests, false))
        .and_then(|window| window.show().map(|()| window))
        .map_err(|e| e.to_string())?;
    opened.wayland_open.store(true, Ordering::Release);
    Ok(FloatingWindow(Some(window)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    struct Probe;
    impl View for Probe {
        fn build(&mut self, _: &mut mui::Ui, _: &mui::input::Input) -> mui::scene::El {
            mui::prelude::block(120., 80.)
        }
        fn changed(&mut self) -> bool {
            false
        }
        fn request_resize(&mut self, _: u32, _: u32) -> bool {
            false
        }
    }
    #[test]
    #[ignore = "requires a running Wayland compositor; opens native test windows"]
    fn native_wayland_multiple_instances_resize_close_and_reopen() {
        let closed = Arc::new(AtomicUsize::new(0));
        let make = || {
            let shared = Arc::new(Mutex::new(Shared {
                ui: mui::Ui::default(),
                view: Probe,
            }));
            let requests = Arc::new(Requests::default());
            let count = closed.clone();
            let window = open(
                "MUI Wayland lifecycle probe",
                (320, 240),
                (160, 120),
                shared,
                requests.clone(),
                Arc::new(move || {
                    count.fetch_add(1, Ordering::AcqRel);
                }),
            )
            .unwrap();
            (window, requests)
        };
        let (first, requests) = make();
        let (second, second_requests) = make();
        let deadline = Instant::now() + Duration::from_secs(20);
        while requests.presentation().is_none() || second_requests.presentation().is_none() {
            assert!(Instant::now() < deadline, "Wayland editors did not present");
            std::thread::sleep(Duration::from_millis(20));
        }
        // Give a tiling compositor's test driver time to float these owned
        // probe windows. Client sizes are advisory until its policy allows it.
        std::thread::sleep(Duration::from_millis(500));
        requests.resize(480, 300);
        let deadline = Instant::now() + Duration::from_secs(5);
        let controlled = std::env::var_os("MUI_WAYLAND_TEST_RESIZE").is_some();
        loop {
            let ((width, height), scale) = first.size();
            let request_drained = requests.size.load(Ordering::Acquire) == 0;
            let configured = (f64::from(width) / scale).round() == 480.
                && (f64::from(height) / scale).round() == 300.;
            // Client resize is advisory. With a controlled compositor driver,
            // additionally verify a real 480x300 configure reached the renderer.
            if request_drained && (!controlled || configured) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Wayland resize not applied: {:?}",
                first.size()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(first);
        assert_eq!(closed.load(Ordering::Acquire), 1);
        assert!(second.is_open());
        drop(second);
        assert_eq!(closed.load(Ordering::Acquire), 2);
        let (reopened, _) = make();
        assert!(reopened.is_open());
        drop(reopened);
        assert_eq!(closed.load(Ordering::Acquire), 3);
    }
}
