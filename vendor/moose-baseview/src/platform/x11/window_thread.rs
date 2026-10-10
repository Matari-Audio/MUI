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
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
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
    startup_signal: OnceLock<LoopSignal>,
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
            startup_signal: OnceLock::new(),
        }
    }

    fn init(&self, window: &WindowInner) {
        self.set_size(window.get_size());
        self.set_scaling_factor(window.scale_factor());
        let _ = self.sizing_strategy.set(window.sizing_strategy);
        let _ = self.startup_signal.set(window.loop_signal.clone());
        if self.callbacks_revoked() {
            window.loop_signal.stop();
            window.loop_signal.wakeup();
        }
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
        self.stopped_requested_from_host.load(Ordering::Relaxed)
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
    SetParent(ParentWindowHandle),
    Show,
    Hide,
}

pub type WindowThreadResponseMessage = core::result::Result<(), String>;

pub struct WindowThreadMessage {
    pub request: WindowThreadRequest,
    pub response: mpsc::Sender<WindowThreadResponseMessage>,
}

// A host RPC can race a tick synchronously calling VST3 performEdit on the GUI
// thread. Time out instead of waiting for that thread to service itself.
const REQUEST_TIMEOUT: Duration = Duration::from_millis(50);
const OPEN_TIMEOUT: Duration = Duration::from_millis(250);
const CLOSE_TIMEOUT: Duration = Duration::from_millis(250);

pub enum HostCallback {
    Resized { new_size: WindowSize, previous: WindowSize },
    Destroyed,
}

pub struct WindowThreadHandle {
    shared: Arc<WindowThreadShared>,
    loop_signal: LoopSignal,
    event_loop_handle: Cell<Option<JoinHandle<()>>>,

    request_sender: calloop::channel::SyncSender<WindowThreadMessage>,
    callback_receiver: Option<mpsc::Receiver<HostCallback>>,
    host_callbacks: Option<RefCell<Box<dyn HostCallbacks>>>,
    close_timeout: Cell<Option<Duration>>,
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
    pub fn create_window(init: WindowInitializer) -> Result<Self> {
        if !image_is_unload_safe() {
            warn!("X11 editor window open refused: could not pin plugin image");
            return Err(PlatformError::CreationFailed("Could not pin X11 editor image".into()));
        }
        let (tx, rx) = result_channel();
        let shared = Arc::new(WindowThreadShared::new());
        let (request_sender, request_receiver) = calloop::channel::sync_channel(1);
        let (main_thread_caller, main_thread_receiver) =
            MainThreadCaller::new(init.host.main_thread);

        let join_handle = {
            let shared = Arc::clone(&shared);
            let stop_watcher = ThreadStopWatcher(Arc::clone(&shared));

            thread::Builder::new()
                .name("baseview-x11".into())
                .spawn(move || {
                    let created = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        WindowThread::create(
                            init.settings,
                            init.builder,
                            shared,
                            request_receiver,
                            main_thread_caller,
                        )
                    }));
                    let thread = match created {
                        Ok(Ok(thread)) => thread,
                        Ok(Err(e)) => return tx.send_error(e),
                        Err(_) => {
                            return tx.send_error(PlatformError::CreationFailed(
                                "Panic during X11 window creation".into(),
                            ))
                        }
                    };

                    if tx.send_success(&thread) {
                        thread.run()
                    }

                    // Forces the stop_watcher to be moved to this closure
                    drop(stop_watcher);
                })
                .map_err(|e| PlatformError::CreationFailed(e.to_string()))?
        };

        let loop_signal = match rx.receive() {
            Ok(signal) => signal,
            Err(e) => {
                shared.callbacks_revoked.store(true, Ordering::Release);
                if let Some(signal) = shared.startup_signal.get() {
                    signal.stop();
                    signal.wakeup();
                }
                // The startup receiver is gone: a late creation will tear down
                // instead of entering its loop. The image was pinned above.
                join_bounded(join_handle, Duration::ZERO, || {});
                return Err(e);
            }
        };

        Ok(WindowThreadHandle {
            event_loop_handle: Some(join_handle).into(),
            shared,
            loop_signal,
            request_sender,
            host_callbacks: init.host.callbacks.map(|c| c.into_inner().into()),
            callback_receiver: main_thread_receiver,
            close_timeout: Cell::new(None),
        })
    }

    pub fn size(&self) -> WindowSize {
        let scale_factor = self.shared.get_scaling_factor();
        let size = self.shared.get_size();

        WindowSize::from_physical(size.cast(), scale_factor)
    }

    pub fn resize(&self, size: Size) -> Result<()> {
        self.request(WindowThreadRequest::Resize(size))
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

    /// A timeout means the queued request may still run, not that it was
    /// cancelled. Each request owns its reply, so a late acknowledgement can
    /// never satisfy the next RPC. A full queue fails immediately.
    fn request(&self, req: WindowThreadRequest) -> Result<()> {
        let (response, receiver) = mpsc::channel();
        self.request_sender.try_send(WindowThreadMessage { request: req, response }).map_err(
            |e| match e {
                mpsc::TrySendError::Full(_) => RequestFailed::Busy,
                mpsc::TrySendError::Disconnected(_) => RequestFailed::Send,
            },
        )?;
        receive_response(&receiver, REQUEST_TIMEOUT)
    }

    pub fn frame_requester(&self) -> crate::FrameRequester {
        let shared = Arc::clone(&self.shared);
        let signal = self.loop_signal.clone();
        crate::FrameRequester::new(move || {
            if !shared.callbacks_revoked()
                && !shared.is_stop_host_requested()
                && !shared.stopped.load(Ordering::Acquire)
                && !shared.frame_requested.swap(true, Ordering::AcqRel)
            {
                signal.wakeup();
            }
        })
    }

    pub fn run_until_closed(&self) -> Result<()> {
        if !self.shared.stopped.load(Ordering::Relaxed) {
            self.request(WindowThreadRequest::Show)?;
        }

        let Some(thread) = self.event_loop_handle.take() else { return Ok(()) };

        if let Err(panic) = thread.join() {
            resume_unwind(panic);
        }

        // Ignore poisoned mutex
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

    pub fn handle_main_thread_callback(&self) {
        loop {
            let Some(receiver) = self.callback_receiver.as_ref() else { return };
            let Some(callback) = receiver.try_recv().ok() else { return };

            self.handle_main_thread_message(callback);
        }
    }

    pub fn set_parent(&self, new_parent: ParentWindowHandle) -> Result<()> {
        self.request(WindowThreadRequest::SetParent(new_parent))
    }

    fn handle_main_thread_message(&self, msg: HostCallback) {
        let Some(host_callbacks) = self.host_callbacks.as_ref() else { return };
        let mut host_callbacks = host_callbacks.borrow_mut();

        match msg {
            HostCallback::Destroyed => host_callbacks.destroyed(),
            HostCallback::Resized { new_size: new, previous } => {
                if let Err(e) = host_callbacks.request_resize(new) {
                    warn!("Host failed to resize parent window: {}. Reverting.", e);

                    if let Err(e) = self.resize(previous.physical.into()) {
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

impl Drop for WindowThreadHandle {
    fn drop(&mut self) {
        self.shared.stopped_requested_from_host.store(true, Ordering::Relaxed);
        self.loop_signal.stop();
        self.loop_signal.wakeup();

        if let Some(thread) = self.event_loop_handle.take() {
            // Preserve normal WillClose/handler teardown when it completes in
            // budget. After a timeout, no new handler calls may enter.
            // Pinning was verified before spawn, not while a stalled driver may
            // hold loader locks at close.
            join_bounded(thread, self.close_timeout.get().unwrap_or(CLOSE_TIMEOUT), || {
                self.shared.callbacks_revoked.store(true, Ordering::Release);
            });
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

fn receive_response(
    receiver: &mpsc::Receiver<WindowThreadResponseMessage>, timeout: Duration,
) -> Result<()> {
    match receiver.recv_timeout(timeout) {
        Ok(result) => result.map_err(|e| RequestFailed::Response(e).into()),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(RequestFailed::Timeout.into()),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(RequestFailed::Recv.into()),
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

    #[test]
    fn request_timeout_does_not_consume_a_later_reply() {
        let (first, first_rx) = mpsc::channel();
        let started = Instant::now();
        assert!(receive_response(&first_rx, Duration::from_millis(5)).is_err());
        assert!(started.elapsed() < Duration::from_millis(100));
        drop(first_rx);
        assert!(first.send(Ok(())).is_err());
        let (second, second_rx) = mpsc::channel();
        second.send(Err("second request failed".into())).unwrap();
        assert!(receive_response(&second_rx, Duration::ZERO)
            .unwrap_err()
            .to_string()
            .contains("second request failed"));
    }

    #[test]
    fn host_request_times_out_when_window_thread_is_inside_a_callback() {
        let loop_: calloop::EventLoop<'static, ()> = calloop::EventLoop::try_new().unwrap();
        let (request_sender, _waiting) = calloop::channel::sync_channel(1);
        let handle = WindowThreadHandle {
            shared: Arc::new(WindowThreadShared::new()),
            loop_signal: loop_.get_signal(),
            event_loop_handle: Cell::new(None),
            request_sender,
            callback_receiver: None,
            host_callbacks: None,
            close_timeout: Cell::new(None),
        };
        let start = Instant::now();
        let error = handle.request(WindowThreadRequest::Show).unwrap_err().to_string();
        assert!(error.contains("timed out"));
        assert!(start.elapsed() >= REQUEST_TIMEOUT);
        assert!(start.elapsed() < Duration::from_millis(150));
        assert!(handle
            .request(WindowThreadRequest::Hide)
            .unwrap_err()
            .to_string()
            .contains("queue is full"));
    }

    #[test]
    fn full_request_queue_fails_without_blocking() {
        let (sender, _receiver) = calloop::channel::sync_channel(1);
        let (response, _rx) = mpsc::channel();
        sender
            .try_send(WindowThreadMessage { request: WindowThreadRequest::Show, response })
            .unwrap();
        let (response, _rx) = mpsc::channel();
        assert!(matches!(
            sender.try_send(WindowThreadMessage { request: WindowThreadRequest::Hide, response }),
            Err(mpsc::TrySendError::Full(_))
        ));
    }
}

enum WindowOpenResult {
    Success { loop_signal: LoopSignal },
    Error(String),
}

struct WindowThread {
    event_loop: EventLoop,
    ev_loop: calloop::EventLoop<'static, EventLoop>,
    shared: Arc<WindowThreadShared>,
}

#[derive(Debug)]
pub enum RequestFailed {
    Send,
    Recv,
    Timeout,
    Busy,
    Response(String),
}

impl Display for RequestFailed {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestFailed::Send => f.write_str("Request to X11 thread failed: Could not send request (X11 thread disconnected)"),
            RequestFailed::Recv => f.write_str("Request to X11 thread failed: Could not receive response (X11 thread disconnected)"),
            RequestFailed::Timeout => f.write_str("X11 request timed out after 50 ms; it may still execute"),
            RequestFailed::Busy => f.write_str("X11 request queue is full; request was not sent"),
            RequestFailed::Response(e) => f.write_str(e),
        }
    }
}

impl WindowThread {
    pub fn create(
        options: WindowSettings, handler: WindowHandlerBuilder, shared: Arc<WindowThreadShared>,
        receiver: calloop::channel::Channel<WindowThreadMessage>,
        main_thread_caller: Option<MainThreadCaller>,
    ) -> Result<Self> {
        let mut ev_loop = calloop::EventLoop::try_new()?;
        let inner = WindowInner::create(options, &ev_loop, Arc::clone(&shared))?;

        shared.init(&inner);
        if shared.callbacks_revoked() {
            return Err(PlatformError::CreationFailed("X11 window open was cancelled".into()));
        }

        let handler = handler.build(WindowContext::new(Rc::clone(&inner)))?;
        let event_loop =
            EventLoop::new(inner, handler, receiver, main_thread_caller, &mut ev_loop)?;

        Ok(Self { event_loop, ev_loop, shared })
    }

    pub fn run(self) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.event_loop.run(self.ev_loop)
        }));
        let result =
            result.unwrap_or_else(|_| Err(PlatformError::Run("Panic in X11 event loop".into())));
        if let Err(e) = result {
            crate::warn!("X11 window thread stopped: {e}");
            // Ignore a poisoned mutex, we just fully override this value anyway.
            let mut guard = self.shared.final_error.lock().unwrap_or_else(|g| g.into_inner());

            guard.replace(e.to_string());
        }
    }
}

fn result_channel() -> (WindowResultSender, WindowResultReceiver) {
    let (tx, rx) = mpsc::sync_channel::<WindowOpenResult>(1);
    (WindowResultSender(tx), WindowResultReceiver(rx))
}

struct WindowResultSender(mpsc::SyncSender<WindowOpenResult>);
impl WindowResultSender {
    pub fn send_error(self, error: PlatformError) {
        if let Err(err) = self.0.send(WindowOpenResult::Error(format!("{}", error))) {
            crate::error!("Window creation failed: {}", error);
            crate::warn!("Failed to send error to main thread: {}", err);
        }
    }

    pub fn send_success(self, thread: &WindowThread) -> bool {
        let msg = WindowOpenResult::Success { loop_signal: thread.ev_loop.get_signal() };

        if let Err(err) = self.0.send(msg) {
            crate::error!("Failed to send created window to main thread: {}. Aborting.", err);
            return false;
        }

        true
    }
}

struct WindowResultReceiver(mpsc::Receiver<WindowOpenResult>);
impl WindowResultReceiver {
    pub fn receive(self) -> Result<LoopSignal> {
        // Explicit first-open budget, including the builder. A slow/stalled X
        // server may refuse this editor open, but cannot freeze the host GUI.
        let result = self
            .0
            .recv_timeout(OPEN_TIMEOUT)
            .map_err(|_| PlatformError::CreationFailed("X11 window open exceeded 250 ms".into()))?;

        match result {
            WindowOpenResult::Error(e) => Err(PlatformError::CreationFailed(e)),
            WindowOpenResult::Success { loop_signal } => Ok(loop_signal),
        }
    }
}
