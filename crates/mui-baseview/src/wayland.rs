//! Floating Wayland adapter using MUI's existing winit Driver and presenters.
//! A surface cannot be embedded through CLAP's Wayland API. Hide destroys the
//! toplevel; the plugin retains its model and opens a new toplevel on show.
use super::*;
pub use mui_winit::floating::FloatingWindow;

pub fn open<V: View + Send + 'static>(
    title: &str,
    size: (u32, u32),
    minimum: (u32, u32),
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
    closed: Arc<dyn Fn() + Send + Sync>,
) -> Result<FloatingWindow, String> {
    let poll = requests.clone();
    let keys = requests.clone();
    let key_shared = shared.clone();
    let status = requests.clone();
    let closing = requests.clone();
    let hooks = mui_winit::floating::Hooks {
        poll: Some(Box::new(move |window, driver| {
            window.set_min_inner_size(Some(winit::dpi::LogicalSize::new(minimum.0, minimum.1)));
            let packed = poll.size.swap(0, Ordering::AcqRel);
            // On Wayland the compositor owns content scale. CLAP set_scale is
            // ignored, rather than multiplying the compositor's scale twice.
            poll.scale.store(0, Ordering::Release);
            if packed != 0 {
                let _ = window.request_inner_size(winit::dpi::LogicalSize::new(
                    (packed >> 32) as u32,
                    packed as u32,
                ));
            }
            if poll.focus.swap(false, Ordering::AcqRel) {
                window.focus_window();
            }
            if poll.redraw.swap(false, Ordering::AcqRel) {
                driver.redraw();
                window.request_redraw();
            }
            poll.idle();
        })),
        key: Some(Box::new(move |event| {
            let hook = keys.keys.lock().ok().and_then(|hook| hook.clone());
            hook.is_some_and(|hook| {
                let mut hook = hook
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                hook(&lock(&key_shared).ui, event)
            })
        })),
        closed: Some(Box::new(move || {
            closing.wayland_open.store(false, Ordering::Release);
            closing.bind_requester(None);
            let hook = closing
                .close
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(hook) = hook {
                hook();
            }
            closed();
        })),
        presented: Some(Box::new(move |size, software| {
            status.presented(size, software, "Wayland")
        })),
    };
    let window = mui_winit::floating::open(
        shared,
        mui_winit::Options {
            title: title.into(),
            size,
            ..Default::default()
        },
        hooks,
    )?;
    requests.wayland_open.store(true, Ordering::Release);
    requests.bind_requester(Some(baseview::FrameRequester::new(window.wake())));
    Ok(window)
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
        requests.resize(480, 300);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let ((width, height), scale) = first.size();
            if (f64::from(width) / scale).round() == 480.
                && (f64::from(height) / scale).round() == 300.
            {
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
