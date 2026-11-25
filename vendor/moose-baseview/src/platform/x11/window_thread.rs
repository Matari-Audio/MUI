use super::*;
use crate::dpi::{PhysicalSize, Size};
use crate::handler::WindowHandlerBuilder;
use crate::host::HostCallbacks;
use crate::platform::x11::event_loop::{EventLoop, MainThreadCaller};
use crate::platform::x11::window_shared::WindowInner;
use crate::utils::SizingStrategy;
use crate::warn;
use crate::window::WindowInitializer;
use crate::{WindowContext, WindowSettings, WindowSize};
use calloop::LoopSignal;
use std::cell::{Cell, RefCell};
use std::panic::resume_unwind;
use std::rc::Rc;
use std::sync::atomic::{fence, AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub(crate) struct WindowThreadShared {
    stopped: AtomicBool,
    scaling_factor: AtomicU64,
    size: AtomicU32,
    final_error: Mutex<Option<String>>,
    stopped_requested_from_host: AtomicBool,
    callbacks_revoked: AtomicBool,
    sizing_strategy: OnceLock<SizingStrategy>,
    frame_requested: AtomicBool,
    /// The worker's loop signal, published once its X window exists.
    loop_signal: OnceLock<LoopSignal>,
    /// Host stop vs. worker publish handshake: whichever runs second sees
    /// the other, so a stop before the signal exists is never lost.
    host_stopped: Mutex<bool>,
}

impl WindowThreadShared {
    pub fn new() -> Self {
        Self {
            stopped: false.into(),
            final_error: None.into(),
            size: 0.into(),
            scaling_factor: 0.into(),
            stopped_requested_from_host: false.into(),
            callbacks_revoked: false.into(),
            sizing_strategy: OnceLock::new(),
            frame_requested: AtomicBool::new(false),
            loop_signal: OnceLock::new(),
            host_stopped: Mutex::new(false),
        }
    }

    /// Host-side answers before the worker has created anything: the
    /// requested size at the scale the settings pin (1.0 if the system's).
    fn publish_requested(&self, settings: &WindowSettings) {
        let scale = settings
            .scale_factor_override
            .or(settings.fallback_scale_factor)
            .and_then(|s| crate::settings::sanitize_scale_factor(Some(s)))
            .unwrap_or(1.0);
        self.set_scaling_factor(scale);
        self.set_size(settings.size.to_physical(scale));
        let _ = self.sizing_strategy.set(SizingStrategy::from_settings(settings));
    }

    /// Worker side: the window exists. False when the host already stopped
    /// it, in which case the worker tears down without building the handler.
    fn publish_window(&self, window: &WindowInner) -> bool {
        self.set_size(window.get_size());
        self.set_scaling_factor(window.scale_factor());
        let stopped = self.host_stopped.lock().unwrap_or_else(|g| g.into_inner());
        if *stopped || self.callbacks_revoked() {
            return false;
        }
        let _ = self.loop_signal.set(window.loop_signal.clone());
        drop(stopped);
        // Pairs with the fence in `request_frame`: a frame asked for before
        // the signal existed is woken here, one asked for after wakes itself.
        fence(Ordering::SeqCst);
        if self.has_frame_request() {
            window.loop_signal.wakeup();
        }
        true
    }

    /// Host side: stop the worker now, or as soon as its window exists.
    fn stop_from_host(&self) {
        self.stopped_requested_from_host.store(true, Ordering::Release);
        let mut stopped = self.host_stopped.lock().unwrap_or_else(|g| g.into_inner());
        *stopped = true;
        if let Some(signal) = self.loop_signal.get() {
            signal.stop();
            signal.wakeup();
        }
    }

    fn request_frame(&self) {
        if self.frame_requested.swap(true, Ordering::AcqRel) {
            return;
        }
        fence(Ordering::SeqCst);
        if let Some(signal) = self.loop_signal.get() {
            signal.wakeup();
        }
    }

    fn record_error(&self, error: String) {
        crate::error!("X11 editor window failed: {}", &error);
        self.final_error.lock().unwrap_or_else(|g| g.into_inner()).replace(error);
    }

    fn last_error(&self) -> Option<String> {
        self.final_error.lock().unwrap_or_else(|g| g.into_inner()).clone()
    }

    pub fn get_size(&self) -> PhysicalSize<u16> {
        let bytes = self.size.load(Ordering::Relaxed);
        let low = (bytes & u16::MAX as u32) as u16;
        let high = (bytes >> 16) as u16;

        PhysicalSize::new(low, high)
    }

    pub fn set_size(&self, size: PhysicalSize<u16>) {
        let bytes = ((size.height as u32) << 16) | (size.width as u32);
        self.size.store(bytes, Ordering::Relaxed);
    }

    pub fn sizing_strategy(&self) -> SizingStrategy {
        self.sizing_strategy.get().copied().unwrap_or_default()
    }

    pub fn get_scaling_factor(&self) -> f64 {
        f64::from_be_bytes(self.scaling_factor.load(Ordering::Relaxed).to_ne_bytes())
    }

    pub fn set_scaling_factor(&self, scale_factor: f64) {
        self.scaling_factor
            .store(u64::from_be_bytes(scale_factor.to_ne_bytes()), Ordering::Relaxed);
    }

    pub fn is_stop_host_requested(&self) -> bool {
        self.stopped_requested_from_host.load(Ordering::Acquire)
    }

    pub fn callbacks_revoked(&self) -> bool {
        self.callbacks_revoked.load(Ordering::Acquire)
    }

    pub fn take_frame_request(&self) -> bool {
        self.frame_requested.swap(false, Ordering::AcqRel)
    }

    pub fn has_frame_request(&self) -> bool {
        self.frame_requested.load(Ordering::Acquire)
    }
}

struct ThreadStopWatcher(Arc<WindowThreadShared>);

impl Drop for ThreadStopWatcher {
    fn drop(&mut self) {
        self.0.stopped.store(true, Ordering::Relaxed);
    }
}

pub enum WindowThreadRequest {
    SuggestScaleFactor(f64),
    SetScaleFactorOverride(Option<f64>),
    Resize(Size),
    SetTitle(String),
    SetParent(ParentWindowHandle),
    Show,
    Hide,
}

/// Host requests are one-way: a host call can race a tick synchronously
/// calling VST3 performEdit on the GUI thread, so the host never waits for
/// the worker. Results the host reads back live in [`WindowThreadShared`];
/// failures are logged by the worker. Requests sent before the window is
/// ready queue here and apply once it is.
pub type WindowThreadMessage = WindowThreadRequest;

/// A full queue fails the request at once rather than blocking the host.
const REQUEST_QUEUE: usize = 32;

pub enum HostCallback {
    Resized { new_size: WindowSize, previous: WindowSize },
    Destroyed,
}

/// Movable commands and metadata; native state stays on the X11 worker.
pub struct WindowThreadControl {
    shared: Arc<WindowThreadShared>,
    event_loop_handle: Cell<Option<JoinHandle<()>>>,
    request_sender: calloop::channel::SyncSender<WindowThreadMessage>,
    can_detach: Cell<bool>,
    close_timeout: Cell<Option<Duration>>,
}

pub struct WindowThreadHandle {
    control: WindowThreadControl,
    callback_receiver: Option<mpsc::Receiver<HostCallback>>,
    host_callbacks: Option<RefCell<Box<dyn HostCallbacks>>>,
}

fn can_detach(settings: &WindowSettings, host: &crate::host::Host) -> bool {
    settings.parent.is_none()
        && !settings.wait_for_parent
        && host.callbacks.is_none()
        && host.main_thread.is_none()
}

// A PIE executable cannot dlopen itself, but unlike a plugin its code cannot
// unload. Fail *window* open for an unpinnable shared library, before spawning.
fn image_is_unload_safe() -> bool {
    if crate::pin_current_image_for_detached_work() {
        return true;
    }
    use std::os::unix::fs::MetadataExt;
    let mut info = std::mem::MaybeUninit::<libc::Dl_info>::zeroed();
    // SAFETY: the symbol is in this image and info is writable.
    if unsafe { libc::dladdr(image_is_unload_safe as *const () as _, info.as_mut_ptr()) } == 0 {
        return false;
    }
    // SAFETY: successful dladdr initialized the struct and owns its filename.
    let info = unsafe { info.assume_init() };
    if info.dli_fname.is_null() {
        return false;
    }
    use std::os::unix::ffi::OsStrExt;
    let name = unsafe { std::ffi::CStr::from_ptr(info.dli_fname) };
    let image = std::fs::metadata(std::ffi::OsStr::from_bytes(name.to_bytes()));
    let executable = std::fs::metadata("/proc/self/exe");
    matches!((image, executable), (Ok(a), Ok(b)) if a.dev() == b.dev() && a.ino() == b.ino())
}

impl WindowThreadHandle {
    pub(crate) fn control(&self) -> &WindowThreadControl {
        &self.control
    }

    pub fn create_window(init: WindowInitializer) -> Result<Self> {
        let can_detach = can_detach(&init.settings, &init.host);
        let (control, callback_receiver) = WindowThreadControl::spawn(
            init.settings,
            init.builder,
            init.host.main_thread,
            can_detach,
        )?;
        Ok(Self { control, callback_receiver, host_callbacks: init.host.callbacks })
    }

    pub fn handle_main_thread_callback(&self) {
        loop {
            let Some(receiver) = self.callback_receiver.as_ref() else { return };
            let Some(callback) = receiver.try_recv().ok() else { return };

            self.handle_main_thread_message(callback);
        }
    }

    fn handle_main_thread_message(&self, msg: HostCallback) {
        let Some(host_callbacks) = self.host_callbacks.as_ref() else { return };
        let mut host_callbacks = host_callbacks.borrow_mut();

        match msg {
            HostCallback::Destroyed => host_callbacks.destroyed(),
            HostCallback::Resized { new_size: new, previous } => {
                if let Err(e) = host_callbacks.request_resize(new) {
                    warn!("Host failed to resize parent window: {}. Reverting.", e);

                    if let Err(e) = self.control.resize(previous.physical.into()) {
                        warn!(
                            "Failed to revert to previous size while handling previous error: {}",
                            e
                        );
                    }
                }
            }
        }
    }
}

impl WindowThreadControl {
    pub(crate) fn create_without_host(
        settings: WindowSettings, builder: WindowHandlerBuilder,
    ) -> Result<Self> {
        let can_detach = settings.parent.is_none() && !settings.wait_for_parent;
        let (control, _) = Self::spawn(settings, builder, None, can_detach)?;
        Ok(control)
    }

    fn spawn(
        settings: WindowSettings, builder: WindowHandlerBuilder,
        main_thread: Option<Box<dyn crate::host::HostMainThreadCaller>>, can_detach: bool,
    ) -> Result<(Self, Option<mpsc::Receiver<HostCallback>>)> {
        if !image_is_unload_safe() {
            warn!("X11 editor window open refused: could not pin plugin image");
            return Err(PlatformError::CreationFailed("Could not pin X11 editor image".into()));
        }
        let shared = Arc::new(WindowThreadShared::new());
        shared.publish_requested(&settings);
        let (request_sender, request_receiver) = calloop::channel::sync_channel(REQUEST_QUEUE);
        let (main_thread_caller, main_thread_receiver) = MainThreadCaller::new(main_thread);

        // Open returns as soon as the worker exists. Creating the X window and
        // building the handler happen there; their failure is an event
        // (logged, `is_open` turns false, later requests report it), never a
        // deadline the host thread waits out.
        let join_handle = {
            let shared = Arc::clone(&shared);
            let stop_watcher = ThreadStopWatcher(Arc::clone(&shared));

            thread::Builder::new()
                .name("baseview-x11".into())
                .spawn(move || {
                    let created = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        WindowThread::create(
                            settings,
                            builder,
                            Arc::clone(&shared),
                            request_receiver,
                            main_thread_caller,
                        )
                    }));
                    match created {
                        Ok(Ok(Some(thread))) => thread.run(),
                        Ok(Ok(None)) => {} // closed by the host before it was ready
                        Ok(Err(e)) => shared.record_error(e.to_string()),
                        Err(_) => shared.record_error("Panic during X11 window creation".into()),
                    }
                    // Forces the stop_watcher to be moved to this closure
                    drop(stop_watcher);
                })
                .map_err(|e| PlatformError::CreationFailed(e.to_string()))?
        };

        Ok((
            Self {
                event_loop_handle: Some(join_handle).into(),
                shared,
                request_sender,
                can_detach: Cell::new(can_detach),
                close_timeout: Cell::new(None),
            },
            main_thread_receiver,
        ))
    }

    pub fn size(&self) -> WindowSize {
        let scale_factor = self.shared.get_scaling_factor();
        let size = self.shared.get_size();

        WindowSize::from_physical(size.cast(), scale_factor)
    }

    pub fn resize(&self, size: Size) -> Result<()> {
        self.request(WindowThreadRequest::Resize(size))
    }

    pub fn set_title(&self, title: &str) -> Result<()> {
        self.request(WindowThreadRequest::SetTitle(title.to_owned()))
    }

    pub fn suggest_scale_factor(&self, scale_factor: f64) -> Result<()> {
        self.request(WindowThreadRequest::SuggestScaleFactor(scale_factor))
    }

    pub fn set_keyboard_capture(&self, _capture: bool) {
        // No-op: ignored key events already propagate to the host on this platform.
    }

    pub fn set_close_timeout(&self, timeout: Duration) {
        self.close_timeout.set(Some(timeout));
    }

    pub fn set_scale_factor_override(&self, scale_factor: Option<f64>) -> Result<()> {
        self.request(WindowThreadRequest::SetScaleFactorOverride(scale_factor))
    }

    /// Queues `req` and returns: Ok means queued, not applied. A full queue
    /// or a stopped worker fails at once; neither ever blocks the host.
    fn request(&self, req: WindowThreadRequest) -> Result<()> {
        self.request_sender.try_send(req).map_err(|e| match e {
            mpsc::TrySendError::Full(_) => RequestFailed::Busy,
            mpsc::TrySendError::Disconnected(_) => RequestFailed::Closed(self.shared.last_error()),
        })?;
        Ok(())
    }

    pub fn visibility_control(&self) -> crate::WindowVisibility {
        let shared = Arc::clone(&self.shared);
        let sender = self.request_sender.clone();
        crate::WindowVisibility::new(move |visible| {
            if shared.callbacks_revoked()
                || shared.is_stop_host_requested()
                || shared.stopped.load(Ordering::Acquire)
            {
                return;
            }
            let request =
                if visible { WindowThreadRequest::Show } else { WindowThreadRequest::Hide };
            let _ = sender.try_send(request);
        })
    }
    pub fn frame_requester(&self) -> crate::FrameRequester {
        let shared = Arc::clone(&self.shared);
        crate::FrameRequester::new(move || {
            if !shared.callbacks_revoked()
                && !shared.is_stop_host_requested()
                && !shared.stopped.load(Ordering::Acquire)
            {
                shared.request_frame();
            }
        })
    }

    pub fn run_until_closed(&self) -> Result<()> {
        let shown = if self.shared.stopped.load(Ordering::Relaxed) {
            Ok(())
        } else {
            self.request(WindowThreadRequest::Show)
        };
        if shown.is_err() {
            self.shared.stop_from_host();
        }
        let joined = self.join_thread();
        shown.and(joined)
    }

    fn join_thread(&self) -> Result<()> {
        let Some(thread) = self.event_loop_handle.take() else { return Ok(()) };
        if thread.thread().id() == thread::current().id() {
            // Self-close completes when the current native callback returns.
            return Ok(());
        }
        if let Err(panic) = thread.join() {
            resume_unwind(panic);
        }
        if let Some(e) = self.shared.final_error.lock().unwrap_or_else(|g| g.into_inner()).take() {
            return Err(PlatformError::Run(e));
        }
        Ok(())
    }

    pub fn show(&self) -> Result<()> {
        self.request(WindowThreadRequest::Show)
    }

    pub fn hide(&self) -> Result<()> {
        self.request(WindowThreadRequest::Hide)
    }

    pub fn is_open(&self) -> bool {
        !self.shared.stopped.load(Ordering::Relaxed)
    }

    pub fn is_resizable(&self) -> bool {
        self.shared.sizing_strategy().is_resizable()
    }

    pub fn min_size(&self) -> Option<Size> {
        self.shared.sizing_strategy().min_size()
    }

    pub fn max_size(&self) -> Option<Size> {
        self.shared.sizing_strategy().max_size()
    }

    pub fn set_parent(&self, new_parent: ParentWindowHandle) -> Result<()> {
        self.can_detach.set(false);
        self.request(WindowThreadRequest::SetParent(new_parent))
    }
}

impl Drop for WindowThreadControl {
    fn drop(&mut self) {
        // An embedded window still joins: the host's parent must outlive every
        // use of it, including a handler build in flight. That wait is the
        // build's own length, never a deadline; a close before the build
        // starts skips it.
        self.shared.stop_from_host();

        if self.can_detach.get() {
            if let Some(timeout) = self.close_timeout.get() {
                let Some(thread) = self.event_loop_handle.take() else { return };
                if thread.thread().id() == thread::current().id() {
                    return;
                }
                join_bounded(thread, timeout, || {
                    self.shared.callbacks_revoked.store(true, Ordering::Release);
                });
                return;
            }
        }
        if let Err(error) = self.join_thread() {
            warn!("Error while closing X11 window: {}", error);
        }
    }
}

// Call only for workers whose image was made unload-safe before spawn. Joining
// is permitted only once is_finished proves it cannot wait for GUI host work.
fn join_bounded(thread: JoinHandle<()>, timeout: Duration, revoke: impl FnOnce()) -> bool {
    let started = Instant::now();
    while !thread.is_finished() && started.elapsed() < timeout {
        thread::sleep(Duration::from_millis(1));
    }
    if thread.is_finished() {
        let _ = thread.join();
        true
    } else {
        revoke();
        warn!("X11 close timed out; revoking callbacks and detaching pinned worker");
        false
    }
}

struct WindowThread {
    event_loop: EventLoop,
    ev_loop: calloop::EventLoop<'static, EventLoop>,
    shared: Arc<WindowThreadShared>,
}

#[derive(Debug)]
pub enum RequestFailed {
    /// The worker has stopped; its failure, if it recorded one.
    Closed(Option<String>),
    Busy,
}

impl Display for RequestFailed {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestFailed::Closed(Some(e)) => write!(f, "X11 window is closed: {e}"),
            RequestFailed::Closed(None) => f.write_str("X11 window is closed"),
            RequestFailed::Busy => f.write_str("X11 request queue is full; request was not sent"),
        }
    }
}

impl WindowThread {
    pub fn create(
        options: WindowSettings, handler: WindowHandlerBuilder, shared: Arc<WindowThreadShared>,
        receiver: calloop::channel::Channel<WindowThreadMessage>,
        main_thread_caller: Option<MainThreadCaller>,
    ) -> Result<Option<Self>> {
        let mut ev_loop = calloop::EventLoop::try_new()?;
        let inner = WindowInner::create(options, &ev_loop, Arc::clone(&shared))?;

        if !shared.publish_window(&inner) {
            return Ok(None);
        }

        let handler = handler.build(WindowContext::new(Rc::clone(&inner).into()))?;
        let event_loop =
            EventLoop::new(inner, handler, receiver, main_thread_caller, &mut ev_loop)?;

        Ok(Some(Self { event_loop, ev_loop, shared }))
    }

    pub fn run(self) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.event_loop.run(self.ev_loop)
        }));
        let result =
            result.unwrap_or_else(|_| Err(PlatformError::Run("Panic in X11 event loop".into())));
        if let Err(e) = result {
            self.shared.record_error(e.to_string());
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "failed test setup/assertions must fail the regression")]
mod close_tests {
    use super::*;

    #[test]
    fn executable_worker_is_unload_safe_without_dlopen() {
        assert!(image_is_unload_safe());
    }

    #[test]
    fn stalled_pinned_thread_never_joins_unboundedly() {
        let (release, waiting) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        let thread = thread::spawn(move || {
            assert!(waiting.recv().is_ok());
            done.send(()).unwrap();
        });
        let started = Instant::now();
        let revoked = AtomicBool::new(false);
        assert!(!join_bounded(thread, Duration::from_millis(5), || revoked
            .store(true, Ordering::Release)));
        assert!(revoked.load(Ordering::Acquire));
        assert!(started.elapsed() < Duration::from_millis(100));
        release.send(()).unwrap();
        finished.recv_timeout(Duration::from_secs(1)).unwrap();
    }

    fn control(
        request_sender: calloop::channel::SyncSender<WindowThreadMessage>,
    ) -> WindowThreadControl {
        WindowThreadControl {
            shared: Arc::new(WindowThreadShared::new()),
            event_loop_handle: Cell::new(None),
            request_sender,
            can_detach: Cell::new(false),
            close_timeout: Cell::new(None),
        }
    }

    /// The worker is busy (building its handler, or inside a callback that
    /// waits on the host): requests still return at once and stay queued.
    #[test]
    fn host_requests_never_wait_for_the_worker() {
        let (request_sender, queued) = calloop::channel::sync_channel(REQUEST_QUEUE);
        let handle = control(request_sender);
        let start = Instant::now();
        handle.request(WindowThreadRequest::Show).unwrap();
        handle.resize(crate::dpi::LogicalSize::new(10.0, 20.0).into()).unwrap();
        for _ in 2..REQUEST_QUEUE {
            handle.request(WindowThreadRequest::Hide).unwrap();
        }
        let full = handle.request(WindowThreadRequest::Show).unwrap_err().to_string();
        assert!(full.contains("queue is full"), "{full}");
        // Generous bound for a loaded machine; the old RPC waited 50 ms each.
        assert!(start.elapsed() < Duration::from_millis(40), "{:?}", start.elapsed());
        drop(queued);
        handle.shared.record_error("parent window vanished".into());
        let closed = handle.request(WindowThreadRequest::Show).unwrap_err().to_string();
        assert!(closed.contains("parent window vanished"), "{closed}");
    }

    #[test]
    fn requested_size_is_answered_before_the_window_exists() {
        let shared = WindowThreadShared::new();
        let settings = WindowSettings::new()
            .with_size(crate::dpi::LogicalSize::new(300.0, 200.0))
            .with_scale_factor_override(Some(2.0));
        shared.publish_requested(&settings);
        assert_eq!(shared.get_size(), PhysicalSize::new(600, 400));
        assert_eq!(shared.get_scaling_factor(), 2.0);
    }

    /// Structural guard: no host-facing path in this file may wait on the
    /// worker with a deadline or a reply. Open and requests are events; the
    /// one timed wait is `close_bounded`, whose bound the caller chooses.
    #[test]
    fn host_paths_have_no_deadlines_or_replies() {
        let source = include_str!("window_thread.rs");
        let production = source.split("#[cfg(test)]").next().unwrap();
        for banned in ["recv_timeout", ".recv()", "OPEN_TIMEOUT", "REQUEST_TIMEOUT", "response"] {
            assert!(!production.contains(banned), "host-facing X11 code uses `{banned}`");
        }
        let sleeps: Vec<_> = production.match_indices("thread::sleep").collect();
        let bounded = production.find("fn join_bounded").unwrap();
        assert_eq!(sleeps.len(), 1, "only close_bounded may wait on a clock");
        assert!(sleeps[0].0 > bounded);
    }

    #[test]
    fn full_request_queue_fails_without_blocking() {
        let (sender, _receiver) = calloop::channel::sync_channel(1);
        sender.try_send(WindowThreadRequest::Show).unwrap();
        assert!(matches!(
            sender.try_send(WindowThreadRequest::Hide),
            Err(mpsc::TrySendError::Full(_))
        ));
    }
}
