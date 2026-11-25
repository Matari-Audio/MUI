//! A native MUI window over our owned `moose-baseview` layer: [`open`]
//! parents it under a plugin host's window (Moose's `moose-mui` is one
//! consumer; any host adapter opens the same window with its own [`View`]),
//! [`run`] is the same window as a standalone app, and [`standalone`] hosts
//! custom standalone top-level windows (X11, native Wayland `xdg_toplevel`,
//! Win32, macOS Cocoa) with direct AccessKit accessibility.
//!
//! Native callbacks queue ordered input to an editor worker. The worker owns
//! [`mui::host::Driver`], builds and rasters into persistent surfaces/targets,
//! then presents directly or returns changed bitmap regions. Baseview's
//! display source paces demand; settled idle windows schedule zero periodic
//! timers (true 0 Hz idle) and wake only on input, external notifications, or
//! exact animation/tooltip/caret/recovery deadlines.
// wgpu-core's storage types nest past the default limit when proving `Send`.
#![recursion_limit = "256"]
#![deny(unsafe_code)]

#[cfg(all(panic = "abort", not(feature = "allow-panic-abort")))]
compile_error!(
    "mui-baseview requires panic=unwind for host-safe callback containment; allow-panic-abort explicitly accepts that a panic terminates the host"
);
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use a11y::{AccessibilityUi, NativeAccessibility};
type A11y = (NativeAccessibility, AccessibilityUi);
type RenderTask = Box<dyn FnOnce() + Send>;

/// Native IME and accessibility bridges for custom baseview hosts.
pub mod native;
/// The baseview this window runs on (moose-baseview), so a consumer names
/// `Window`, `WindowSettings` and friends without depending on it itself.
pub use baseview;

/// A drag thumbnail's size in logical pixels, for
/// [`Requests::drag_out_with_image`].
pub const DRAG_PREVIEW_SIZE: (usize, usize) = (224, 90);

mod drag;
mod native_worker;
mod policies;
pub mod presenter;
pub use mui_present::{
    FallbackReason, GraphicsApi, PresentationBackend, PresentationMode, PresentationState,
    set_user_software_mode, user_software_mode,
};
pub use presenter::{Presenter, PresenterStats, Transition};
/// Window policies for hosts that create their own windows: the OS blur
/// behind a frosted window, and keeping a window above others.
#[cfg_attr(
    any(target_os = "windows", target_os = "macos"),
    expect(unsafe_code, reason = "Win32 DWM and AppKit view calls")
)]
pub mod frost;
/// Safe standalone top-level window and direct AccessKit helpers over
/// `moose-baseview` (X11, native Wayland `xdg_toplevel`, Win32, macOS Cocoa).
pub mod standalone;
#[cfg_attr(
    any(target_os = "windows", target_os = "macos"),
    expect(unsafe_code, reason = "Win32 and AppKit window-level calls")
)]
pub mod sticky;
mod timing;
use baseview::dpi::{LogicalSize, PhysicalPosition};
use baseview::{
    DropData, DropEffect, Event, EventStatus, HandlerError, MouseButton, MouseCursor, MouseEvent,
    ScrollDelta, Window, WindowContext, WindowEvent, WindowHandler, WindowSettings, WindowSize,
};
pub use drag::DragQueueError;
use keyboard_types::KeyboardEvent;
#[cfg(test)]
use keyboard_types::Modifiers;
/// Drag images and protocol lifecycle values shared by product editors.
pub use matari_audio_drag_and_drop::{
    DragImage, DragPreview, FailureStage, LinuxOutcome, Outcome, PreviewImageError, PreviewStatus,
    SessionEvent,
};
/// What a [`KeyHook`] is handed.
pub use mui::host::KeyEvent;
use mui::host::{Driver, Wheel};
pub use mui::host::{Shared, View, lock};
use mui::prelude::{Button, Mods, Point};
use mui::vello::host::{FailureClass, Frame, GpuInit, Host, Retry, target_size};
use mui::vello::kurbo::Affine;
use native::{key_event, mods, mouse_button, native_cursor, native_ime};
use native_worker::build;
use policies::WheelState;
pub use policies::{WheelAcceleration, WheelPolicy};
#[cfg(target_os = "linux")]
use raw_window_handle::HasDisplayHandle;
use raw_window_handle::HasWindowHandle;

/// An app's look at every key event, down and up, before MUI routes it:
/// `true` takes the key, and neither MUI nor the host sees it. MUI hands a
/// view key presses only; a computer keyboard that plays notes has to hear
/// the key come up. Runs on the model worker with its retained `Ui`; native calls belong in the native pump.
pub type KeyHook = Arc<Mutex<dyn FnMut(&mui::Ui, &KeyEvent) -> bool + Send>>;

/// Selects native shortcuts without borrowing the model or waiting for its worker.
/// May run on the window or model thread; keep it short and independent of UI state.
pub type KeyFilter = Arc<dyn Fn(&KeyEvent) -> bool + Send + Sync>;

/// Takes a key the UI didn't keep; returns true when it used it.
pub type UnhandledKey = Arc<dyn Fn(&KeyboardEvent) -> bool + Send + Sync>;

type KeyRoutes = HashMap<u64, Option<KeyHook>>;

/// A native-close signal. Runs before borrowing the model or handler; it must
/// never borrow the UI/model or wait for workers.
pub type CloseHook = Arc<dyn Fn() + Send + Sync>;

/// Evaluated with the retained `Ui` on its model worker. Returning true hides
/// the pointer during a gesture; the native thread restores its original position
/// on release. Leaving the window or losing focus cancels restoration.
pub type PointerHook = Arc<Mutex<dyn FnMut(&mui::Ui) -> bool + Send>>;

/// Native-thread creation and cancellation of an owned X11 identifier.
/// This hook must not borrow the UI/model or wait for the model worker.
#[cfg(target_os = "linux")]
pub type X11WindowHook = Arc<Mutex<dyn FnMut(Option<u32>) + Send>>;

/// Last frame submitted to the native child; not GPU completion or scanout.
#[derive(Clone, Copy, Debug)]
pub struct Presentation {
    pub frames: u64,
    pub size: (u32, u32),
    pub software: bool,
    pub reason: &'static str,
}

/// Requests from the host's thread, applied by the window's next tick,
/// which is the only place baseview's `WindowContext` can be touched.
#[derive(Default)]
pub struct Requests {
    #[cfg(feature = "test-support")]
    test_events: Mutex<VecDeque<Event>>,
    size: AtomicU64,
    scale: AtomicU64,
    redraw: AtomicBool,
    focus: AtomicBool,
    #[cfg(target_os = "linux")]
    wayland_open: AtomicBool,
    /// Microseconds; 0 disables telemetry polling.
    poll: AtomicU64,
    keys: Mutex<Option<KeyHook>>,
    key_filter: Mutex<Option<KeyFilter>>,
    unhandled_key: Mutex<Option<UnhandledKey>>,
    pointer: Mutex<Option<PointerHook>>,
    wheel: Mutex<WheelPolicy>,
    #[cfg(target_os = "linux")]
    x11_hook: Mutex<Option<X11WindowHook>>,
    close: Mutex<Option<CloseHook>>,
    idle: Mutex<Option<CloseHook>>,
    requester: Mutex<Option<baseview::FrameRequester>>,
    presentation: Mutex<Option<Presentation>>,
    presentation_state: Mutex<PresentationState>,
    #[cfg(target_os = "linux")]
    x11_window: std::sync::atomic::AtomicU32,
    drag: drag::Mailbox,
    render_active: AtomicBool,
    render_generation: AtomicU64,
    render_cancelled: AtomicBool,
    render_start: Mutex<Option<RenderTask>>,
    render_stop: Mutex<Option<RenderTask>>,
    render_work: Mutex<VecDeque<RenderTask>>,
    native_start: Mutex<Option<RenderTask>>,
    clipboard_write: Mutex<Option<String>>,
}

impl Requests {
    /// Configure baseview notch conversion before opening this editor.
    /// The separate floating Wayland adapter does not use this policy.
    pub fn set_wheel_policy(&self, policy: WheelPolicy) {
        *self
            .wheel
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = policy;
    }

    /// Install the model-worker pointer policy before opening a baseview editor.
    /// The separate floating Wayland adapter does not use this policy.
    pub fn on_pointer(&self, hook: PointerHook) {
        *self
            .pointer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(hook);
    }

    /// Observe creation and cancellation on the native thread. Register before
    /// opening. Each created X11 window retains its hook until cancellation.
    #[cfg(target_os = "linux")]
    pub fn on_x11_window(&self, hook: X11WindowHook) {
        *self
            .x11_hook
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(hook);
    }

    /// Queue an integration-fixture event through the native pump. Bounded,
    /// ordered and absent from production builds. Native focus/capture APIs
    /// still execute normally, including synchronous platform callbacks.
    #[cfg(feature = "test-support")]
    pub fn test_event(&self, event: Event) -> Result<(), String> {
        let mut events = self.test_events.lock().map_err(|e| e.to_string())?;
        if events.len() >= 32 {
            return Err("native fixture event queue is full".into());
        }
        events.push_back(event);
        drop(events);
        self.wake();
        Ok(())
    }

    /// Run once on the native window thread before starting its model worker.
    pub fn on_native_start(&self, start: impl FnOnce() + Send + 'static) {
        *self
            .native_start
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(start));
    }
    /// Install worker-only lifecycle work before opening a native editor.
    /// Neither closure may call a host/native-window API. `stop` must revoke
    /// model-side references; it runs after cancellation, including after panic.
    pub fn on_render_lifecycle(
        &self,
        start: impl FnOnce() + Send + 'static,
        stop: impl FnOnce() + Send + 'static,
    ) {
        *self
            .render_start
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(start));
        *self
            .render_stop
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(stop));
    }
    /// Queue ordered model work on the editor worker and request a
    /// display-paced frame. Never acquire the model lock in a native callback.
    pub fn dispatch_render_work(&self, work: impl FnOnce() + Send + 'static) {
        self.render_work
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(Box::new(work));
        self.redraw();
    }
    /// True while this editor's worker can still access its model. Native close
    /// never waits for it; a reopened worker can sequence its model attachment.
    pub fn render_active(&self) -> bool {
        self.render_active.load(Ordering::Acquire)
    }
    /// Native close has revoked this editor, even if a GPU call is still running.
    pub fn render_cancelled(&self) -> bool {
        self.render_cancelled.load(Ordering::Acquire)
    }
    /// Request native keyboard focus on the window's owning thread.
    pub fn focus(&self) {
        self.focus.store(true, Ordering::Release);
        self.wake();
    }

    /// Coherent presentation evidence; absent until the first successful submit.
    pub fn presentation(&self) -> Option<Presentation> {
        *self
            .presentation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Full observable presentation state (mode, backend, dimensions, frames, fallback).
    pub fn presentation_state(&self) -> PresentationState {
        self.presentation_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// A GPU frame rendered with `api` straight to the window's surface.
    fn presented_gpu(
        &self,
        size: (u32, u32),
        api: GraphicsApi,
        translucent: bool,
        reason: &'static str,
    ) {
        self.presented_with(
            size,
            size,
            PresentationMode::DirectGpu,
            PresentationBackend::for_gpu(api, translucent),
            api,
            translucent,
            None,
            reason,
        );
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "records both legacy Presentation and full PresentationState atomically"
    )]
    fn presented_with(
        &self,
        size: (u32, u32),
        render_size: (u32, u32),
        mode: PresentationMode,
        backend: PresentationBackend,
        api: GraphicsApi,
        translucent: bool,
        fallback: Option<FallbackReason>,
        reason: &'static str,
    ) {
        let software = matches!(
            mode,
            PresentationMode::CpuDirect | PresentationMode::CpuScaled
        );
        let mut status = self
            .presentation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let first_cpu_frame = software && status.is_none_or(|old| !old.software);
        let frames = status.map_or(1, |old| old.frames.saturating_add(1));
        *status = Some(Presentation {
            frames,
            size,
            software,
            reason,
        });
        drop(status);
        {
            let mut state = self
                .presentation_state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.software_forced = mui_present::software_mode_enabled();
            state.api = api;
            state.translucent = translucent;
            state.record_fallback(fallback);
            state.record_frame(mode, backend, size, render_size, reason);
        }
        if first_cpu_frame {
            mui::diagnostics::breadcrumb(
                "mui-baseview",
                "cpu_frame_presented",
                "native CPU presentation succeeded",
            );
        }
    }
    fn bind_requester(&self, requester: Option<baseview::FrameRequester>) {
        *self
            .requester
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = requester;
    }
    fn wake(&self) {
        let requester = self
            .requester
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(requester) = requester {
            requester.request_frame();
        }
    }
    /// Wake the native window loop to run `on_idle` without invalidating the
    /// model, scheduling a render-worker tick, or enabling periodic frames.
    /// This schedules work; it never invokes the hook on the caller's thread.
    /// The native thread is the opening host thread on Windows, but the X11
    /// window thread on Linux. Host callbacks must still check thread ownership.
    pub fn request_native_idle(&self) {
        self.wake();
    }

    /// Pump deferred host work after a frame has released the shared model lock.
    /// The hook must check the calling thread before invoking host callbacks.
    pub fn on_idle(&self, hook: CloseHook) {
        *self
            .idle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(hook);
    }
    fn idle(&self) {
        let hook = self
            .idle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(hook) = hook
            && let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hook()))
        {
            mui::diagnostics::error(
                "mui-baseview",
                "idle_panic",
                mui::diagnostics::panic_message(&*payload),
            );
        }
    }
    /// Register the close signal for the next native window, before opening it.
    /// It runs once on native close, or when the window adapter is dropped.
    pub fn on_close(&self, hook: CloseHook) {
        *self
            .close
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(hook);
    }
    /// Whether a floating Wayland editor currently owns this request queue.
    #[cfg(target_os = "linux")]
    pub fn wayland_open(&self) -> bool {
        self.wayland_open.load(Ordering::Acquire)
    }

    /// The editor's X11 window identifier, for parenting desktop portal dialogs.
    ///
    /// Available after native creation and cleared on close. This is an owned
    /// identifier, not a borrowed native handle; cancel dialogs when their editor
    /// closes, since reading it does not keep the window alive.
    #[cfg(target_os = "linux")]
    pub fn x11_window(&self) -> Option<u32> {
        let window = self.x11_window.load(Ordering::Acquire);
        (window != 0).then_some(window)
    }
    /// Queue an export from the current primary pointer gesture.
    pub fn drag_out(&self, paths: Vec<std::path::PathBuf>) -> Result<(), DragQueueError> {
        self.queue_drag(paths, None)
    }
    pub fn drag_out_with_waveform(
        &self,
        paths: Vec<std::path::PathBuf>,
        buckets: Vec<(f32, f32)>,
    ) -> Result<(), DragQueueError> {
        self.drag_out_with_preview(
            paths,
            matari_audio_drag_and_drop::DragPreview::Waveform { buckets },
        )
    }
    pub fn drag_out_with_image(
        &self,
        paths: Vec<std::path::PathBuf>,
        pixels: Vec<u8>,
    ) -> Result<(), DragQueueError> {
        self.drag_out_with_preview(
            paths,
            matari_audio_drag_and_drop::DragPreview::Image { pixels },
        )
    }
    pub fn drag_out_with_preview(
        &self,
        paths: Vec<std::path::PathBuf>,
        preview: matari_audio_drag_and_drop::DragPreview,
    ) -> Result<(), DragQueueError> {
        self.queue_drag(paths, Some(preview))
    }
    fn queue_drag(
        &self,
        paths: Vec<std::path::PathBuf>,
        preview: Option<matari_audio_drag_and_drop::DragPreview>,
    ) -> Result<(), DragQueueError> {
        self.drag.queue(paths, preview)?;
        self.redraw();
        Ok(())
    }
    /// Hide the source's drag visual while native ownership is queued or active.
    pub fn drag_active(&self) -> bool {
        self.drag.active()
    }
    /// Drain this editor's protocol lifecycle events. Transfer success does not certify host import.
    pub fn take_drag_events(&self) -> Vec<matari_audio_drag_and_drop::SessionEvent> {
        self.drag.events()
    }
    /// Hand every key event to `hook` first; see [`KeyHook`].
    pub fn on_key(&self, hook: KeyHook) {
        *self
            .keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(hook);
    }
    /// Reserve selected shortcuts for a worker key hook on Windows.
    /// Other keys retain normal text-field and host routing. A selected key is
    /// reserved even when the hook later declines it and MUI handles it instead.
    /// On Windows the filter also runs before text assembly, using the layout's
    /// base key. The hook receives the completed text event only once.
    /// Each physical press keeps its hook route through repeats and release,
    /// even if modifiers change. Losing window focus clears these routes.
    pub fn on_key_filtered(&self, filter: KeyFilter, hook: KeyHook) {
        self.on_key(hook);
        *self
            .key_filter
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(filter);
        self.redraw();
    }

    /// Offer every key the UI didn't keep to `hook`, for a window with no
    /// host behind it to fall through to (a standalone's own toplevel).
    pub fn on_unhandled_key(&self, hook: UnhandledKey) {
        *self
            .unhandled_key
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(hook);
    }

    fn unhandled(&self, key: &KeyboardEvent) -> bool {
        let hook = self
            .unhandled_key
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        hook.is_some_and(|hook| hook(key))
    }

    fn offers_keys(&self) -> bool {
        self.key_filter.lock().is_ok_and(|filter| filter.is_some())
    }

    fn claims_key(&self, event: &KeyEvent) -> bool {
        let filter = self
            .key_filter
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        filter.is_some_and(|filter| filter(event))
    }

    fn reserved_key_hook(&self, captured: bool) -> Option<KeyHook> {
        if captured {
            self.keys
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        } else {
            None
        }
    }

    fn key_hook_for(
        &self,
        routes: &mut KeyRoutes,
        ui: &mui::Ui,
        event: &KeyEvent,
    ) -> Option<KeyHook> {
        let previous = if event.down {
            routes.get(&event.code).cloned()
        } else {
            routes.remove(&event.code)
        };
        if let Some(hook) = previous {
            return hook;
        }
        let filter = self
            .key_filter
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let hook = if !ui.focus_is_text() && filter.is_some_and(|filter| !filter(event)) {
            None
        } else {
            self.keys
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        };
        if event.down {
            routes.insert(event.code, hook.clone());
        }
        hook
    }

    /// Resize the child window to `width` x `height` logical points.
    pub fn resize(&self, width: u32, height: u32) {
        self.size.store(
            u64::from(width) << 32 | u64::from(height),
            Ordering::Release,
        );
        self.wake();
    }
    /// The host's content scale changed.
    pub fn scale(&self, factor: f64) {
        if factor.is_finite() && factor > 0.0 {
            self.scale.store(factor.to_bits(), Ordering::Release);
            self.wake();
        }
    }
    /// Poll [`View::changed`] at least this often while `Some` (live meters,
    /// pitch, scopes); `None` allows the window to go idle. This setter
    /// performs only an atomic store and is safe to call from the audio
    /// thread. An idle render worker checks for changes within 250 ms
    /// without requesting frames while the interval stays unchanged.
    pub fn set_poll_interval(&self, interval: Option<Duration>) {
        let micros = interval.map_or(0, |d| d.as_micros().clamp(1, u64::MAX.into()) as u64);
        self.poll.store(micros, Ordering::Relaxed);
    }
    fn poll_interval(&self) -> Option<Duration> {
        match self.poll.load(Ordering::Relaxed) {
            0 => None,
            micros => Some(Duration::from_micros(micros)),
        }
    }
    /// Rebuild the tree on the next tick even if nothing it polls moved.
    pub fn redraw(&self) {
        self.redraw.store(true, Ordering::Release);
        self.wake();
    }
}

/// Open the window under `parent`, `size` logical points. `scale` pins the
/// window's scale factor (the host's content scale); `None` follows the OS.
/// `None` back when the parent handle is unusable or the window could not be
/// made; the reason goes to [`View::log`]. Dropping the window closes it.
/// `None` too when the process hosts editors headless
/// ([`mui::host::headless`]): the view went there instead.
pub fn open<V: View + Send + 'static>(
    parent: &impl HasWindowHandle,
    title: &str,
    size: (u32, u32),
    scale: Option<f64>,
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
) -> Option<Window> {
    // A headless host (mui-cut's adapter) takes the view instead.
    if mui::host::headless::offer(&shared, size) {
        return None;
    }
    // baseview panics on a handle it cannot read.
    if let Err(e) = parent.window_handle() {
        log(&shared, &format!("mui-baseview: no parent window ({e})"));
        return None;
    }
    let settings = settings(title, size)
        .with_parent(parent)
        .with_scale_factor_override(scale);
    let sink = Arc::clone(&shared);
    let window =
        Window::create(settings, build(shared, requests, true)).and_then(|w| w.show().map(|()| w));
    window
        .map_err(|e| log(&sink, &format!("mui-baseview: window failed ({e})")))
        .ok()
}

/// Open an embedded X11 editor with a movable command controller.
/// Native window state and callbacks stay on the X11 worker. Close the
/// controller before destroying the host parent.
#[cfg(target_os = "linux")]
pub fn open_threaded<V: View + Send + 'static>(
    parent: &impl HasWindowHandle,
    title: &str,
    size: (u32, u32),
    scale: Option<f64>,
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
) -> Option<baseview::ThreadedWindow> {
    if mui::host::headless::offer(&shared, size) {
        return None;
    }
    if let Err(error) = parent.window_handle() {
        log(
            &shared,
            &format!("mui-baseview: no parent window ({error})"),
        );
        return None;
    }
    let settings = settings(title, size)
        .with_parent(parent)
        .with_scale_factor_override(scale);
    let sink = Arc::clone(&shared);
    baseview::ThreadedWindow::create(settings, build(shared, requests, true))
        .and_then(|window| window.show().map(|()| window))
        .map_err(|error| log(&sink, &format!("mui-baseview: window failed ({error})")))
        .ok()
}

/// Open a floating X11 editor without claiming the process's standalone event loop.
#[cfg(target_os = "linux")]
pub fn open_floating_threaded<V: View + Send + 'static>(
    title: &str,
    size: (u32, u32),
    scale: Option<f64>,
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
) -> Option<baseview::ThreadedWindow> {
    if mui::host::headless::offer(&shared, size) {
        return None;
    }
    let sink = Arc::clone(&shared);
    baseview::ThreadedWindow::create(
        settings(title, size).with_scale_factor_override(scale),
        build(shared, requests, false),
    )
    .and_then(|window| window.show().map(|()| window))
    .map_err(|error| log(&sink, &format!("mui-baseview: window failed ({error})")))
    .ok()
}

/// Run a top-level window of `size` logical points at the system scale, until
/// it closes: an app's main loop. Never call it from a plugin: it tells the
/// OS this process is baseview's alone (Windows DPI awareness).
pub fn run<V: View + Send + 'static>(
    title: &str,
    size: (u32, u32),
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
) {
    // SAFETY: `run` is an app's main loop, documented as never a plugin's:
    // this process hosts no other windowing library.
    #[expect(unsafe_code, reason = "baseview's standalone-process opt-in")]
    unsafe {
        baseview::assume_standalone_in_process();
    }
    let sink = Arc::clone(&shared);
    let settings = standalone::settings(title, size);
    let window = Window::create(settings, build(shared, requests, false));
    if let Err(e) = window.and_then(Window::run_until_closed) {
        log(&sink, &format!("mui-baseview: window failed ({e})"));
    }
}

fn settings(title: &str, size: (u32, u32)) -> WindowSettings {
    WindowSettings::new()
        .with_title(title)
        .with_size(LogicalSize::new(f64::from(size.0), f64::from(size.1)))
}

/// Whole logical points, as Wayland viewports take them.
fn logical_size(size: LogicalSize<f64>) -> (u32, u32) {
    (size.width.round() as u32, size.height.round() as u32)
}

/// One `MUI_FRAME_LOG` line; `stats` are the GPU renderer's for a presented frame.
fn log_frame(
    log: &mut Option<mui::profiling::FrameLog>,
    driver: &mut Driver,
    reason: &str,
    size: (u32, u32),
    gpu: bool,
    presented: bool,
    stats: Option<&mui::vello::effects::EffectStats>,
) {
    let (Some(log), Some(profiler)) = (log.as_mut(), driver.profiler_mut()) else {
        return;
    };
    log.write(
        profiler,
        mui::profiling::FrameRecord {
            size,
            gpu,
            reason,
            presented,
            cpu_fallback: stats
                .and_then(|s| s.cpu_fallback.as_ref())
                .map(|r| r as &dyn std::fmt::Debug),
            cpu_patches: stats.map_or(0, |s| s.cpu_patches),
            rendered_pixels: stats.map_or(0, |s| s.rendered_pixels),
        },
    );
}

/// The window's event handler. Public so a framework adapter can drive it
/// headless in its own tests ([`Handler::new`], [`Handler::step`],
/// [`Handler::on_event_inner`]); a window gets one from [`open`] or [`run`].
#[doc(hidden)]
pub struct Handler<V> {
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
    gpu: Option<Host>,
    gpu_init: Option<GpuInit>,
    gpu_retry: mui::vello::host::Retry,
    cpu_retry: Retry,
    presenter_reason: &'static str,
    /// The GPU surface's own layer (a Wayland child subsurface).
    layer: surface::Layer,
    /// Why CPU frames present instead of GPU ones.
    fallback: Option<FallbackReason>,
    software: Option<mui::vello::software::Window<WindowContext>>,
    software_only: bool,
    gpu_retry_at: Instant,
    applied_cursor: Option<MouseCursor>,
    /// The current scene is not on screen yet: paint it.
    unpainted: bool,
    /// A screen reader's side; only a real window has one.
    a11y: Option<A11y>,
    /// The window's scale factor: baseview's pointer is in pixels.
    scale: f64,
    /// The OS announced a new scale; the resize that applies it is next.
    os_rescale: bool,
    /// The keyboard capture last asked of baseview.
    captured: Option<bool>,
    /// Last successful native text input configuration; changes preserve composition.
    applied_ime: Option<Option<baseview::ImeConfiguration>>,
    /// The queue and the frame schedule.
    pub driver: Driver,
    key_routes: KeyRoutes,
    wheel: WheelState,
    wheel_policy: WheelPolicy,
    #[cfg(target_os = "linux")]
    x11_window: u32,
    /// Outbound drag sessions, and whether the window is a host's child.
    drag: drag::Host,
    /// `MUI_FRAME_LOG`: a line per painted frame.
    frame_log: Option<mui::profiling::FrameLog>,
    _reporting: mui::diagnostics::ReportingGuard,
}

impl<V> Drop for Handler<V> {
    fn drop(&mut self) {
        self.drag.close(&self.requests.drag);
        #[cfg(target_os = "linux")]
        self.clear_x11_window();
    }
}

#[cfg(target_os = "linux")]
impl<V> Handler<V> {
    fn clear_x11_window(&self) {
        // A retained old handler must not clear a newly opened window's identifier.
        let _ = self.requests.x11_window.compare_exchange(
            self.x11_window,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

impl<V: View> Handler<V> {
    /// A handler with no window, GPU or screen reader yet.
    pub fn new(
        shared: Arc<Mutex<Shared<V>>>,
        requests: Arc<Requests>,
        size: (u32, u32),
        scale: f64,
    ) -> Self {
        lock(&shared).ui.set_gpu_welding_available(false);
        let wheel_policy = *requests
            .wheel
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let drag = drag::Host::new(&requests);
        let mut driver = Driver::new(size, scale, Box::new(Clipboard::default()));
        let frame_log = mui::profiling::FrameLog::from_env();
        if frame_log.is_some() {
            driver.enable_profiling(mui::profiling::ProfileConfig::default());
        }
        Self {
            shared,
            requests,
            gpu: None,
            gpu_init: None,
            gpu_retry: mui::vello::host::Retry::default(),
            cpu_retry: Retry::default(),
            presenter_reason: "CPU startup",
            layer: surface::Layer::default(),
            fallback: mui::vello::host::force_cpu().then_some(FallbackReason::ForceCpu),
            software: None,
            software_only: mui::vello::host::force_cpu(),
            gpu_retry_at: Instant::now(),
            applied_cursor: None,
            unpainted: true,
            a11y: None,
            scale,
            os_rescale: false,
            captured: None,
            applied_ime: None,
            driver,
            key_routes: KeyRoutes::new(),
            wheel: WheelState::default(),
            wheel_policy,
            #[cfg(target_os = "linux")]
            x11_window: 0,
            drag,
            frame_log,
            _reporting: mui::diagnostics::retain_reporter(),
        }
    }

    fn tick(&mut self, window: &WindowContext) {
        let drag_frame = self.drag.frame_token(&self.requests.drag);
        let requests = &self.requests;
        if requests.focus.swap(false, Ordering::AcqRel) && !window.has_focus() {
            let _ = window.focus();
        }
        let packed = requests.size.swap(0, Ordering::AcqRel);
        let bits = requests.scale.swap(0, Ordering::AcqRel);
        if packed != 0 || bits != 0 {
            // Read before the override: it changes the logical size baseview
            // reports, and a new scale keeps the window's points.
            let logical = if packed != 0 {
                LogicalSize::new((packed >> 32) as f64, (packed & u64::from(u32::MAX)) as f64)
            } else {
                window.size().logical
            };
            if bits != 0 {
                let _ = window.set_scale_factor_override(Some(f64::from_bits(bits)));
            }
            let _ = window.resize(logical);
            // Not every platform reports a resize it was asked for.
            self.resized(window.size());
        }
        // Hosts may deliver the initial size/DPI after creation without a
        // separate resize callback. Never configure from the builder's stale size.
        let native_size = window.size();
        if self.driver.size() != (native_size.physical.width, native_size.physical.height)
            || self.scale != native_size.scale_factor
        {
            self.resized(native_size);
        }
        // A hidden or detached editor cannot present, and on Windows this is
        // the host's GUI thread: a blocking present there freezes the host.
        let Ok(handle) = window.window_handle().map(|h| h.as_raw()) else {
            return;
        };
        if platform::should_skip_frame(handle) {
            return;
        }
        #[cfg(target_os = "linux")]
        if let Some((native, _)) = self.a11y.as_mut()
            && let (Ok(display), Ok(handle)) = (window.display_handle(), window.window_handle())
        {
            // SAFETY: both handles borrow this live window on its owning thread.
            #[expect(unsafe_code, reason = "query the owned window's original display")]
            unsafe {
                native.update_bounds(display.as_raw(), handle.as_raw());
            }
        }
        let now = Instant::now();
        let size = self.driver.size();
        if self.requests.redraw.swap(false, Ordering::AcqRel) {
            self.driver.redraw();
            if let Some(software) = &mut self.software {
                software.invalidate();
            }
        }
        // Idle windows do no device polling; a dirty frame observes loss.
        if self.gpu.is_none()
            && self.software.is_none()
            && target_size(size.0, size.1).is_some()
            && self.cpu_retry.ready(now)
        {
            let open = if self.software_only || self.gpu_init.is_some() {
                mui::vello::software::Window::new
            } else {
                mui::vello::software::Window::new_startup
            };
            match open(window.clone(), size) {
                Ok(software) => {
                    self.software = Some(software);
                    self.cpu_retry.reset();
                    lock(&self.shared).ui.set_gpu_welding_available(false);
                    self.driver.redraw();
                    self.unpainted = true;
                }
                Err(error) => {
                    if self.cpu_retry.deadline().is_none() {
                        log(
                            &self.shared,
                            &format!("mui-baseview: CPU presentation unavailable ({error})"),
                        );
                    }
                    self.cpu_retry.fail(now, FailureClass::Transient);
                    self.gpu_retry_at = self.cpu_retry.deadline().unwrap_or(now);
                }
            }
        }
        if self.gpu.is_none()
            && let Some(init) = self.gpu_init.as_mut()
        {
            let (software, layer) = (&mut self.software, &mut self.layer);
            let logical = window.size().logical;
            let result = init.poll(size, |instance| {
                // Called only when the GPU result is ready. Never attach CPU
                // and GPU presenters to the same native layer.
                drop(software.take());
                let (surface, made) =
                    surface::create(instance, window.platform_handle(), logical_size(logical))?;
                *layer = made;
                Ok(surface)
            });
            if let Some(result) = result {
                match result {
                    Ok(gpu) => {
                        self.gpu = Some(gpu);
                        self.presenter_reason = "GPU ready";
                        self.fallback = None;
                        lock(&self.shared).ui.set_gpu_welding_available(true);
                        self.driver.redraw();
                        self.unpainted = true;
                    }
                    Err(error) => {
                        // Once per failure streak: backoff retries must not flood the host log.
                        if self.presenter_reason != "CPU GPU initialization failed" {
                            log(
                                &self.shared,
                                &format!(
                                    "mui-baseview: GPU unavailable ({error}); using CPU rendering"
                                ),
                            );
                        }
                        // Retry classification controls GPU recovery, not CPU scene quality.
                        // Transient failure must also leave the startup placeholder behind.
                        if error.failure_class() == FailureClass::Permanent {
                            self.software_only = true;
                        }
                        self.fallback = Some(error.fallback_reason());
                        let repaint = self
                            .software
                            .as_mut()
                            .is_some_and(|software| software.finish_startup(size));
                        self.presenter_reason = "CPU GPU initialization failed";
                        if repaint {
                            self.driver.redraw();
                            self.unpainted = true;
                        }
                    }
                }
            }
            self.gpu_retry_at = init.deadline().unwrap_or(now);
        }
        // The lock covers the frame and a snapshot of its scene, not the
        // present: acquiring a surface texture can wait out a vsync, and a
        // host-thread close() or state load must not wait with it.
        let mut accessibility_update = None;
        let ime_configuration;
        let capture;
        let scene = {
            let mut s = lock(&self.shared);
            let a11y = self.a11y.as_mut().map(|(_, ui)| ui);
            if let Some(a11y) = &a11y
                && a11y.wants_tree()
            {
                self.driver.redraw();
            }
            if let Some(a11y) = a11y
                && a11y.apply(&mut s.ui)
            {
                self.driver.redraw();
            }
            let fresh = self.driver.advance(&mut s, now);
            ime_configuration = self
                .driver
                .ime_configuration(&s.ui)
                .map(|config| native_ime(config, self.driver.ui_scale()));
            // Selected shortcuts must reach hooks outside text fields too.
            capture = s.ui.focus_is_text() || self.requests.offers_keys();
            if let Some((_, a11y)) = self.a11y.as_mut()
                && (fresh || a11y.wants_tree())
            {
                accessibility_update = a11y.prepare(&s.ui);
            }
            self.unpainted |= fresh;
            if self.unpainted && (self.gpu.is_some() || self.software.is_some()) {
                s.ui.scene_snapshot()
            } else {
                None
            }
        };
        // Capturing the keyboard may synchronously move native focus.
        if self.captured != Some(capture) {
            window.set_keyboard_capture(capture);
            self.captured = Some(capture);
        }
        if let (Some((native, _)), Some(update)) = (self.a11y.as_mut(), accessibility_update) {
            native.publish(update);
        }
        // Native IME APIs may synchronously call the adapter. No model lock is held.
        if self.applied_ime.as_ref() != Some(&ime_configuration) {
            window.set_ime_configuration(ime_configuration.clone());
            self.applied_ime = Some(ime_configuration);
        }
        if let (Some(software), Some(scene)) = (self.software.as_mut(), scene.as_ref()) {
            let start = self.driver.profiler().map(|_| Instant::now());
            let result = software.present(scene, Affine::scale(self.driver.ui_scale()), size);
            if let (Some(profile), Some(start)) = (self.driver.profiler_mut(), start) {
                profile.record_since(mui::profiling::Phase::BackendDraw, start);
                if matches!(result, Ok(true)) {
                    profile.record_since(mui::profiling::Phase::PresentCall, start);
                } else if matches!(result, Ok(false)) {
                    profile.discard_pending_presentation();
                }
            }
            let painted = matches!(result, Ok(true));
            let reason = self.presenter_reason;
            log_frame(
                &mut self.frame_log,
                &mut self.driver,
                reason,
                size,
                false,
                painted,
                None,
            );
            match result {
                Ok(drew) => {
                    self.requests.drag.rendered(drag_frame);
                    self.unpainted = false;
                    if drew {
                        let reason = if software.background_only() {
                            "CPU background-only startup/over-budget"
                        } else {
                            self.presenter_reason
                        };
                        self.requests.presented_with(
                            size,
                            size,
                            PresentationMode::CpuDirect,
                            software.backend(),
                            GraphicsApi::Software,
                            false,
                            self.fallback,
                            reason,
                        );
                    }
                    // First pixels precede both detached and synchronous initialization.
                    self.start_gpu_after_cpu_frame(drew);
                }
                Err(e) => {
                    log(
                        &self.shared,
                        &format!("mui-baseview: CPU render failed ({e})"),
                    );
                    self.software = None;
                    self.cpu_retry.fail(Instant::now(), FailureClass::Transient);
                    self.gpu_retry_at = self.cpu_retry.deadline().unwrap_or(now);
                }
            }
        }
        if let (Some(gpu), Some(scene)) = (self.gpu.as_mut(), scene)
            && now >= self.gpu_retry_at
        {
            if let Err(e) = gpu.resize(size.0, size.1) {
                log(&self.shared, &format!("mui-baseview: {e}"));
                self.gpu_failed(
                    e.failure_class(),
                    e.fallback_reason(),
                    "CPU resize recovery",
                );
                return;
            }
            let draw_start = self.driver.profiler().map(|_| Instant::now());
            let frame = gpu.present(&scene, Affine::scale(self.driver.ui_scale()));
            if let (Some(profiler), Some(start)) = (self.driver.profiler_mut(), draw_start) {
                profiler.record_since(mui::profiling::Phase::BackendDraw, start);
                if matches!(&frame, Ok(Frame::Presented(_))) {
                    // CPU interval through host present return; never a scanout timestamp.
                    profiler.record_since(mui::profiling::Phase::PresentCall, start);
                }
            }
            let stats = match &frame {
                Ok(Frame::Presented(stats)) => Some(stats),
                _ => None,
            };
            let reason = self.presenter_reason;
            let size = gpu.size();
            log_frame(
                &mut self.frame_log,
                &mut self.driver,
                reason,
                size,
                true,
                stats.is_some(),
                stats,
            );
            match frame {
                Ok(Frame::Presented(_)) => {
                    self.requests.drag.rendered(drag_frame);
                    self.gpu_retry.reset();
                    self.unpainted = false;
                    self.requests.presented_gpu(
                        gpu.size(),
                        gpu.api(),
                        gpu.translucent(),
                        self.presenter_reason,
                    );
                }
                Ok(Frame::Current) => {
                    self.requests.drag.rendered(drag_frame);
                    self.gpu_retry.reset();
                    self.unpainted = false;
                    if let Some(profiler) = self.driver.profiler_mut() {
                        profiler.discard_pending_presentation();
                    }
                }
                // Outdated/timed-out acquires are routine mid-resize; the host
                // already reconfigured, so retry next tick instead of backing
                // off (a backoff left the window black and at the old size).
                Ok(Frame::Skipped) => {}
                Ok(Frame::SurfaceLost) => {
                    self.gpu_retry.fail(now, FailureClass::Transient);
                    self.gpu_retry_at = self.gpu_retry.deadline().unwrap_or(now);
                    let surface = surface::create(
                        gpu.instance(),
                        window.platform_handle(),
                        logical_size(window.size().logical),
                    );
                    if let Ok((surface, layer)) = surface {
                        self.layer = layer;
                        if let Err(error) = gpu.try_replace_surface(surface) {
                            self.gpu_failed(
                                error.failure_class(),
                                FallbackReason::SurfaceLost,
                                "CPU surface recovery",
                            );
                        }
                    } else {
                        log(&self.shared, "mui-baseview: surface lost; rebuilding");
                        self.gpu_failed(
                            FailureClass::Transient,
                            FallbackReason::SurfaceLost,
                            "CPU surface recovery",
                        );
                    }
                }
                Err(e) => {
                    log(&self.shared, &format!("mui-baseview: {e}"));
                    // Permanent faults stay on CPU; loss/configuration recover
                    // off-thread, never through a per-frame rebuild.
                    self.gpu_failed(e.failure_class(), e.fallback_reason(), "CPU GPU recovery");
                }
            }
        }
        let cursor = native_cursor(self.driver.cursor());
        if self.applied_cursor != Some(cursor) {
            let _ = window.set_mouse_cursor(cursor);
            self.applied_cursor = Some(cursor);
        }
    }

    fn gpu_failed(&mut self, class: FailureClass, fallback: FallbackReason, reason: &'static str) {
        self.fallback = Some(fallback);
        self.gpu = None;
        self.software = None;
        self.gpu_retry.reset();
        self.software_only = class == FailureClass::Permanent;
        if let Some(init) = &mut self.gpu_init {
            init.recover(class);
        }
        lock(&self.shared).ui.set_gpu_welding_available(false);
        self.driver.redraw();
        self.presenter_reason = reason;
        self.gpu_retry_at = Instant::now();
        self.unpainted = true;
    }

    fn start_gpu_after_cpu_frame(&mut self, presented: bool) {
        if presented && !self.software_only && self.gpu_init.is_none() {
            self.gpu_init = Some(GpuInit::new(baseview::pin_current_image_for_detached_work()));
        }
    }

    /// One display tick without a window or GPU, a frame's time after the
    /// last: what the headless tests drive.
    pub fn step(&mut self) -> bool {
        if self.requests.redraw.swap(false, Ordering::AcqRel) {
            self.driver.redraw();
        }
        let now = self.driver.last_frame() + Duration::from_millis(16);
        let advanced = self.driver.advance(&mut lock(&self.shared), now);
        self.requests.idle();
        advanced
    }

    /// The window's new size, as baseview reports it.
    pub fn resized(&mut self, size: WindowSize) {
        // A host-pinned scale leaves the scale as it was: nothing to report.
        let rescaled = std::mem::take(&mut self.os_rescale)
            && self.scale.to_bits() != size.scale_factor.to_bits();
        if rescaled {
            let logical = (
                size.logical.width.round() as u32,
                size.logical.height.round() as u32,
            );
            lock(&self.shared).view.rescaled(size.scale_factor, logical);
        }
        self.scale = size.scale_factor;
        self.layer.resize(logical_size(size.logical));
        let physical = (size.physical.width, size.physical.height);
        self.driver.resized(physical, size.scale_factor);
    }

    /// One native event, as baseview delivers it.
    pub fn on_event_inner(&mut self, event: &Event) -> EventStatus {
        self.on_event_at(event, Instant::now())
    }

    /// [`Self::on_event_inner`] with the event's arrival time supplied, so
    /// time-dependent policies (wheel acceleration) are deterministic in tests.
    pub fn on_event_at(&mut self, event: &Event, now: Instant) -> EventStatus {
        if let Event::Window(WindowEvent::ScaleFactorChanged(_)) = event {
            self.os_rescale = true;
        }
        if matches!(
            event,
            Event::Window(WindowEvent::Unfocused | WindowEvent::WillClose)
        ) {
            self.key_routes.clear();
        }
        self.drag.observe(&self.requests.drag, event);
        if matches!(event, Event::Window(WindowEvent::WillClose)) {
            self.drag.close(&self.requests.drag);
        }
        #[cfg(target_os = "linux")]
        if matches!(event, Event::Window(WindowEvent::WillClose)) {
            self.clear_x11_window();
        }
        let scale = self.scale;
        let points = |p: PhysicalPosition<f64>| Point::new(p.x / scale, p.y / scale);
        let d = &mut self.driver;
        match event {
            Event::Window(WindowEvent::RedrawRequested) => {
                self.unpainted = true;
                if let Some(software) = &mut self.software {
                    software.invalidate();
                    self.unpainted = true;
                }
            }
            Event::Ime(event) => d.ime(native::ime_event(event)),
            Event::KeyboardCapture { key, captured } => {
                self.key_routes.insert(
                    key_event(key).code,
                    self.requests.reserved_key_hook(*captured),
                );
            }
            Event::Keyboard(key) => {
                let event = key_event(key);
                let hook = self.requests.key_hook_for(
                    &mut self.key_routes,
                    &lock(&self.shared).ui,
                    &event,
                );
                if let Some(hook) = hook {
                    let mut hook = hook
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if hook(&lock(&self.shared).ui, &event) {
                        return EventStatus::Captured;
                    }
                }
                let kept = d.key(&lock(&self.shared), &event);
                // After the `shared` guard drops: the fallback may call into
                // the plugin, which may take it.
                let kept = kept || self.requests.unhandled(key);
                return if kept {
                    EventStatus::Captured
                } else {
                    EventStatus::Ignored
                };
            }
            Event::Mouse(mouse) => match *mouse {
                MouseEvent::CursorMoved {
                    position,
                    modifiers,
                } => d.pointer_moved(points(position), mods(modifiers)),
                MouseEvent::ButtonPressed { button, modifiers }
                | MouseEvent::ButtonReleased { button, modifiers } => {
                    if let Some(b) = mouse_button(button) {
                        let down = matches!(mouse, MouseEvent::ButtonPressed { .. });
                        d.button(b, down, mods(modifiers));
                    }
                }
                MouseEvent::WheelScrolled { delta, modifiers } => {
                    let delta = self.wheel.apply(delta, self.wheel_policy, now);
                    let wheel = match delta {
                        ScrollDelta::Lines { x, y } => Wheel::Lines(f64::from(x), f64::from(y)),
                        ScrollDelta::Pixels { x, y } => Wheel::Pixels(f64::from(x), f64::from(y)),
                    };
                    d.wheel(wheel, mods(modifiers));
                }
                MouseEvent::CursorLeft | MouseEvent::DragLeft => d.pointer_left(),
                MouseEvent::DragEntered {
                    position,
                    modifiers,
                    ref data,
                }
                | MouseEvent::DragMoved {
                    position,
                    modifiers,
                    ref data,
                }
                | MouseEvent::DragDropped {
                    position,
                    modifiers,
                    ref data,
                } => {
                    let at = points(position);
                    let DropData::Files(paths) = data else {
                        d.pointer_moved(at, mods(modifiers));
                        return EventStatus::Ignored;
                    };
                    let dropped = matches!(mouse, MouseEvent::DragDropped { .. });
                    let s = &mut *lock(&self.shared);
                    return if d.drop_files(s, at, mods(modifiers), paths, dropped) {
                        EventStatus::AcceptDrop(DropEffect::Copy)
                    } else {
                        EventStatus::Ignored
                    };
                }
                _ => {}
            },
            Event::Window(e @ (WindowEvent::Focused | WindowEvent::Unfocused)) => {
                let focused = matches!(e, WindowEvent::Focused);
                d.focus(focused);
                if let Some((native, _)) = self.a11y.as_mut() {
                    native.focus(focused);
                }
            }
            Event::Window(WindowEvent::WillClose) => {
                // Drop retained native accessibility views before locking the model.
                drop(self.a11y.take());
                drop(self.software.take());
                drop(self.gpu.take());
                drop(self.gpu_init.take());
                self.applied_ime = None;
                d.close(&mut lock(&self.shared));
            }
            // A scale change arrives as a resize too.
            _ => {}
        }
        EventStatus::Captured
    }
}

/// baseview calls its handler through `&self`, and a call can re-enter it
/// (on Windows, a resize from inside a frame is reported before `resize`
/// returns). A call that finds the handler busy is kept, the latest resize
/// and every event, and delivered once the outer call returns.
#[doc(hidden)]
pub struct Adapter<V> {
    // Cancel owned dialogs before dropping the native context or model.
    close: NativeClose,
    // Drop graphics and native accessibility before their window context.
    handler: RefCell<Handler<V>>,
    cx: WindowContext,
    pending_resize: Cell<Option<WindowSize>>,
    pending_events: RefCell<VecDeque<Event>>,
    buttons: NativeButtons,
}

struct NativeClose {
    hook: Option<CloseHook>,
    fired: Cell<bool>,
}

impl NativeClose {
    fn fire(&self) {
        if !self.fired.replace(true)
            && let Some(hook) = &self.hook
        {
            // This callback precedes the handler's panic guard at the FFI edge.
            if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| hook()))
            {
                mui::diagnostics::error(
                    "mui-baseview",
                    "close_panic",
                    mui::diagnostics::panic_message(&*payload),
                );
                eprintln!(
                    "mui-baseview: panic in native close signal: {}",
                    mui::diagnostics::panic_message(&*payload)
                );
            }
        }
    }
}

impl Drop for NativeClose {
    fn drop(&mut self) {
        self.fire();
    }
}

impl<V: View> Adapter<V> {
    fn drain(&self, h: &mut Handler<V>) {
        if let Some(size) = self.pending_resize.take() {
            guard(h, |h| h.resized(size));
        }
        drain_events(&self.pending_events, |event| {
            guard(h, |h| h.on_event_inner(&event));
        });
    }
}

#[derive(Default)]
struct NativeButtons(RefCell<Vec<MouseButton>>);

impl NativeButtons {
    fn observe(&self, event: &Event) {
        match event {
            Event::Mouse(MouseEvent::ButtonPressed { button, .. }) => {
                let mut buttons = self.0.borrow_mut();
                if !buttons.contains(button) {
                    buttons.push(*button);
                }
            }
            Event::Mouse(MouseEvent::ButtonReleased { button, .. }) => {
                self.0.borrow_mut().retain(|held| held != button);
            }
            Event::Window(WindowEvent::Unfocused | WindowEvent::WillClose) => {
                self.0.borrow_mut().clear();
            }
            _ => {}
        }
    }

    fn held(&self) -> bool {
        !self.0.borrow().is_empty()
    }
}

fn enqueue_event(queue: &mut VecDeque<Event>, event: Event, dragging: bool) {
    // Only hover is replaceable state. A held gesture needs every position for
    // Driver's trail, including when its press preceded this inbox/drain.
    // Never cross a key, IME, button, focus, wheel, drop or accessibility barrier.
    if let (
        Some(Event::Mouse(MouseEvent::CursorMoved {
            modifiers: previous,
            ..
        })),
        Event::Mouse(MouseEvent::CursorMoved { modifiers, .. }),
    ) = (queue.back(), &event)
        && !dragging
        && previous == modifiers
    {
        if let Some(last) = queue.back_mut() {
            *last = event;
        }
        return;
    }
    queue.push_back(event);
}

fn drain_events<T>(queue: &RefCell<VecDeque<T>>, mut deliver: impl FnMut(T)) {
    loop {
        // Release the queue borrow before invoking reentrant native code.
        let event = queue.borrow_mut().pop_front();
        let Some(event) = event else { break };
        deliver(event);
    }
}

/// baseview calls from a platform callback: a panic crossing it takes the
/// host down, not just the editor. The guard only exists under
/// `panic = "unwind"` (the `plugin` profile); under release's abort the
/// panic kills the process before it gets here.
fn guard<V: View, R>(h: &mut Handler<V>, f: impl FnOnce(&mut Handler<V>) -> R) -> Option<R> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(h))) {
        Ok(value) => Some(value),
        Err(payload) => {
            log(
                &h.shared,
                &format!(
                    "mui-baseview: panic at the window FFI edge: {}",
                    mui::diagnostics::panic_message(&*payload)
                ),
            );
            None
        }
    }
}

/// Push-driven schedule for the synchronous [`Adapter`]: settled windows return
/// [`baseview::FrameDemand::Idle`] (0 Hz) with zero periodic polling, waking
/// only on input/redraw requests, active animation/tooltip/caret deadlines,
/// explicit telemetry [`Requests::set_poll_interval`], active drags, or
/// pending GPU init/retry backoff.
#[expect(
    clippy::too_many_arguments,
    reason = "pure scheduler inputs kept explicit for deterministic unit testing"
)]
fn adapter_frame_demand(
    now: Instant,
    driver_wake: Option<Instant>,
    poll_interval: Option<Duration>,
    drag_active: bool,
    redraw_pending: bool,
    unpainted: bool,
    gpu_missing: bool,
    gpu_retry_at: Instant,
    gpu_init_pending: bool,
) -> baseview::FrameDemand {
    const RECOVERY_POLL_INTERVAL: Duration = Duration::from_millis(25);
    if redraw_pending {
        return baseview::FrameDemand::At(now);
    }
    let recovery_poll = now.checked_add(RECOVERY_POLL_INTERVAL).unwrap_or(now);
    let mut at: Option<Instant> = None;
    let mut sooner = |candidate: Instant| {
        at = Some(match at {
            Some(existing) => existing.min(candidate),
            None => candidate,
        });
    };
    if let Some(wake) = driver_wake {
        sooner(wake);
    }
    if let Some(poll) = poll_interval {
        sooner(now.checked_add(poll).unwrap_or(now));
    }
    if drag_active {
        sooner(now + Duration::from_millis(10));
    }
    if unpainted || (gpu_missing && gpu_retry_at > now) {
        sooner(gpu_retry_at.max(recovery_poll));
    }
    if gpu_init_pending {
        sooner(recovery_poll);
    }
    at.map_or(baseview::FrameDemand::Idle, baseview::FrameDemand::At)
}

impl<V: View + 'static> WindowHandler for Adapter<V> {
    fn captures_key(&self, key: &KeyboardEvent) -> bool {
        let Ok(h) = self.handler.try_borrow() else {
            return false;
        };
        let text = lock(&h.shared).ui.focus_is_text();
        text || h.requests.claims_key(&key_event(key))
    }

    fn frame_demand(&self) -> baseview::FrameDemand {
        let Ok(h) = self.handler.try_borrow() else {
            return baseview::FrameDemand::Continuous;
        };
        adapter_frame_demand(
            Instant::now(),
            h.driver.next_wake(),
            h.requests.poll_interval(),
            h.requests.drag_active(),
            h.requests.redraw.load(Ordering::Acquire),
            h.unpainted,
            h.gpu.is_none(),
            h.gpu_retry_at,
            h.gpu_init.as_ref().is_some_and(GpuInit::pending),
        )
    }

    fn on_frame(&self) -> Result<(), HandlerError> {
        let mut idle = None;
        if let Ok(mut h) = self.handler.try_borrow_mut() {
            idle = Some(Arc::clone(&h.requests));
            let wake_start = h.driver.profiler().map(|_| Instant::now());
            self.drain(&mut h);
            guard(&mut h, |h| h.tick(&self.cx));
            self.drain(&mut h);
            guard(&mut h, |h| {
                if h.drag.drive(&h.requests, &self.cx) {
                    h.driver.button(Button::Primary, false, Mods::default());
                    h.driver.pointer_left();
                    h.requests.redraw();
                }
            });
            if let (Some(profiler), Some(start)) = (h.driver.profiler_mut(), wake_start) {
                profiler.record_since(mui::profiling::Phase::NativeWake, start);
            }
        }
        if let Some(requests) = idle {
            requests.idle();
        }
        Ok(())
    }

    fn resized(&self, size: WindowSize) -> Result<(), HandlerError> {
        match self.handler.try_borrow_mut() {
            Ok(mut h) => {
                guard(&mut h, |h| h.resized(size));
                self.drain(&mut h);
            }
            Err(_) => self.pending_resize.set(Some(size)),
        }
        Ok(())
    }

    fn on_event(&self, event: Event) -> EventStatus {
        // Observe directly delivered edges too, before the handler borrow can
        // fail. The button set survives inbox drains and tracks every button.
        self.buttons.observe(&event);
        if matches!(event, Event::Window(WindowEvent::WillClose)) {
            self.close.fire();
        }
        let Ok(mut h) = self.handler.try_borrow_mut() else {
            enqueue_event(
                &mut self.pending_events.borrow_mut(),
                event,
                self.buttons.held(),
            );
            return EventStatus::Ignored;
        };
        let status = guard(&mut h, |h| h.on_event_inner(&event)).unwrap_or(EventStatus::Ignored);
        self.drain(&mut h);
        status
    }
}

/// X11 drops a selection when its owner goes, so Linux keeps an arboard
/// owner alive. baseview writes the clipboard elsewhere but cannot read it:
/// there, a paste gets what this editor copied last.
///
/// Also a [`mui::Clipboard`], for a baseview host of your own:
/// `Ui::new(theme).clipboard(Clipboard::default())`.
#[derive(Default)]
pub struct Clipboard {
    #[cfg(target_os = "linux")]
    x11: Option<arboard::Clipboard>,
    #[cfg(not(target_os = "linux"))]
    last: Option<String>,
}

impl mui::Clipboard for Clipboard {
    fn get(&mut self) -> Option<String> {
        self.read()
    }
    fn set(&mut self, text: &str) {
        self.write(text);
    }
}

impl Clipboard {
    #[cfg(target_os = "linux")]
    fn read(&mut self) -> Option<String> {
        if self.x11.is_none() {
            self.x11 = arboard::Clipboard::new().ok();
        }
        self.x11.as_mut()?.get_text().ok()
    }
    #[cfg(target_os = "linux")]
    fn write(&mut self, text: &str) {
        if self.x11.is_none() {
            self.x11 = arboard::Clipboard::new().ok();
        }
        if let Some(x11) = self.x11.as_mut() {
            let _ = x11.set_text(text.to_owned());
        }
    }
    #[cfg(not(target_os = "linux"))]
    fn read(&mut self) -> Option<String> {
        self.last.clone()
    }
    #[cfg(not(target_os = "linux"))]
    fn write(&mut self, text: &str) {
        baseview::copy_to_clipboard(text);
        self.last = Some(text.to_owned());
    }
}

fn log<V: View>(shared: &Mutex<Shared<V>>, line: &str) {
    mui::diagnostics::error("mui-baseview", "window_error", line);
    if let Err(payload) =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match shared.try_lock() {
            Ok(mut shared) => shared.view.log(line),
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner().view.log(line),
            Err(std::sync::TryLockError::WouldBlock) => {}
        }))
    {
        mui::diagnostics::error(
            "mui-baseview",
            "logger_panic",
            mui::diagnostics::panic_message(&*payload),
        );
    }
}

// Native tests explicitly request the synchronous GPU lane. Production windows
// use GpuInit only, after a successful CPU frame.
#[cfg(all(test, target_os = "linux"))]
fn open_gpu(window: &WindowContext, size: (u32, u32)) -> Result<Host, String> {
    Host::open_native(
        |backends| {
            let mut descriptor = mui::vello::host::instance_descriptor();
            descriptor.backends = backends;
            let instance = mui_hal::Instance::new(descriptor);
            let (surface, _) = surface::create(
                &instance,
                window.platform_handle(),
                logical_size(window.size().logical),
            )?;
            Ok((instance, surface))
        },
        size,
    )
}

mod a11y;
mod platform;
mod surface;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod gpu_policy_tests {
    use super::*;
    struct Empty;
    impl View for Empty {
        fn build(&mut self, _: &mut mui::Ui, _: &mui::prelude::Input) -> mui::scene::El {
            mui::prelude::block(240., 200.)
        }
        fn changed(&mut self) -> bool {
            false
        }
        fn request_resize(&mut self, _: u32, _: u32) -> bool {
            false
        }
    }
    fn handler() -> Handler<Empty> {
        Handler::new(
            Arc::new(Mutex::new(Shared {
                ui: mui::Ui::default(),
                view: Empty,
            })),
            Arc::default(),
            (240, 200),
            1.0,
        )
    }
    #[test]
    fn cpu_frame_is_required_before_the_gpu_initializer_exists() {
        let mut handler = handler();
        handler.software_only = false;
        handler.start_gpu_after_cpu_frame(false);
        assert!(handler.gpu_init.is_none());
        handler.start_gpu_after_cpu_frame(true);
        assert!(handler.gpu_init.is_some());
        assert!(handler.gpu.is_none());
    }
    #[test]
    fn requests_publish_atomics_before_waking_the_native_window() {
        let requests = Arc::new(Requests::default());
        let observed = Arc::new(AtomicU64::new(0));
        let weak = Arc::downgrade(&requests);
        let count = observed.clone();
        requests.bind_requester(Some(baseview::FrameRequester::new(move || {
            let requests = weak.upgrade().unwrap();
            assert!(requests.size.load(Ordering::Acquire) != 0);
            count.fetch_add(1, Ordering::Relaxed);
        })));
        requests.resize(240, 200);
        requests.scale(2.0);
        requests.redraw();
        assert_eq!(observed.load(Ordering::Relaxed), 3);
    }
    #[test]
    fn headless_idle_hook_runs_without_the_shared_model_lock() {
        let mut handler = handler();
        let shared = handler.shared.clone();
        let called = Arc::new(AtomicBool::new(false));
        let flag = called.clone();
        handler.requests.on_idle(Arc::new(move || {
            assert!(shared.try_lock().is_ok());
            flag.store(true, Ordering::Release);
        }));
        handler.step();
        assert!(called.load(Ordering::Acquire));
    }
}

/// Native Wayland editors: floating (CLAP) and embedded in a host surface (VST3).
#[cfg(target_os = "linux")]
pub mod wayland;
