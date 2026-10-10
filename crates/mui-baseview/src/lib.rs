//! A native MUI window over baseview: [`open`] parents it under a plugin
//! host's window (mui-truce's `MuiEditor` is one consumer; any other
//! framework's adapter opens the same window with its own [`View`]), and
//! [`run`] is the same window as a standalone app.
//!
//! This crate only translates baseview's events into [`mui::host::Driver`]
//! calls and presents through `mui::vello::host::Host`; the queue, the
//! frame schedule, zoom and key routing are `mui::host`'s, so another
//! window crate hosts the same [`View`] the same way.
#![deny(unsafe_code)]
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use a11y::{AccessibilityUi, NativeAccessibility};
type A11y = (NativeAccessibility, AccessibilityUi);

/// Native IME and accessibility bridges for custom baseview hosts.
pub mod native;
/// The baseview this window runs on (moose-baseview), so a consumer names
/// `Window`, `WindowSettings` and friends without depending on it itself.
pub use baseview;
use baseview::dpi::{LogicalSize, PhysicalPosition};
use baseview::{
    DropData, DropEffect, Event, EventStatus, HandlerError, MouseButton, MouseCursor, MouseEvent,
    ScrollDelta, Window, WindowContext, WindowEvent, WindowHandler, WindowSettings, WindowSize,
};
use keyboard_types::{Key as HostKey, KeyState, KeyboardEvent, Modifiers, NamedKey};
/// What a [`KeyHook`] is handed.
pub use mui::host::KeyEvent;
use mui::host::{Driver, Modifier, NativeKey, Wheel};
pub use mui::host::{Shared, View, lock};
use mui::prelude::{Button, Cursor, Key, Mods, Point};
use mui::vello::host::{Frame, Host, target_size};
use mui::vello::kurbo::Affine;
#[cfg(target_os = "linux")]
use raw_window_handle::HasDisplayHandle;
use raw_window_handle::HasWindowHandle;

const GPU_RETRY: Duration = Duration::from_millis(500);

/// An app's look at every key event, down and up, before MUI routes it:
/// `true` takes the key, and neither MUI nor the host sees it. MUI hands a
/// view key presses only; a computer keyboard that plays notes has to hear
/// the key come up. Runs on the window's thread with the last frame's `Ui`.
pub type KeyHook = Arc<Mutex<dyn FnMut(&mui::Ui, &KeyEvent) -> bool + Send>>;

/// A native-close signal. Runs before borrowing the model or handler; it must
/// only signal cancellation, never borrow the UI/model or wait for workers.
pub type CloseHook = Arc<dyn Fn() + Send + Sync>;

/// Last frame submitted to the native child; not GPU completion or scanout.
#[derive(Clone, Copy, Debug)]
pub struct Presentation {
    pub frames: u64,
    pub size: (u32, u32),
    pub software: bool,
}

/// Requests from the host's thread, applied by the window's next tick,
/// which is the only place baseview's `WindowContext` can be touched.
#[derive(Default)]
pub struct Requests {
    size: AtomicU64,
    scale: AtomicU64,
    redraw: AtomicBool,
    keys: Mutex<Option<KeyHook>>,
    close: Mutex<Option<CloseHook>>,
    presentation: Mutex<Option<Presentation>>,
    #[cfg(target_os = "linux")]
    x11_window: std::sync::atomic::AtomicU32,
}

impl Requests {
    /// Coherent presentation evidence; absent until the first successful submit.
    pub fn presentation(&self) -> Option<Presentation> {
        *self
            .presentation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn presented(&self, size: (u32, u32), software: bool) {
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
        });
        drop(status);
        if first_cpu_frame {
            mui::diagnostics::breadcrumb(
                "mui-baseview",
                "cpu_frame_presented",
                "native CPU presentation succeeded",
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
    /// Hand every key event to `hook` first; see [`KeyHook`].
    pub fn on_key(&self, hook: KeyHook) {
        *self
            .keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(hook);
    }
    /// Resize the child window to `width` x `height` logical points.
    pub fn resize(&self, width: u32, height: u32) {
        self.size.store(
            u64::from(width) << 32 | u64::from(height),
            Ordering::Release,
        );
    }
    /// The host's content scale changed.
    pub fn scale(&self, factor: f64) {
        if factor.is_finite() && factor > 0.0 {
            self.scale.store(factor.to_bits(), Ordering::Release);
        }
    }
    /// Rebuild the tree on the next tick even if nothing it polls moved.
    pub fn redraw(&self) {
        self.redraw.store(true, Ordering::Release);
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
    let window = Window::create(settings(title, size), build(shared, requests, false));
    if let Err(e) = window.and_then(Window::run_until_closed) {
        log(&sink, &format!("mui-baseview: window failed ({e})"));
    }
}

fn settings(title: &str, size: (u32, u32)) -> WindowSettings {
    WindowSettings::new()
        .with_title(title)
        .with_size(LogicalSize::new(f64::from(size.0), f64::from(size.1)))
}

/// The handler constructor `open` and `run` share.
fn build<V: View + Send + 'static>(
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
    parented: bool,
) -> impl FnOnce(WindowContext) -> Result<Adapter<V>, HandlerError> + Send + 'static {
    move |cx: WindowContext| {
        let size = cx.size();
        let physical = (size.physical.width, size.physical.height);
        let mut handler = Handler::new(shared, requests, physical, size.scale_factor);
        #[cfg(target_os = "linux")]
        {
            handler.x11_window = cx
                .window_handle()
                .ok()
                .and_then(|handle| match handle.as_raw() {
                    raw_window_handle::RawWindowHandle::Xlib(h) => u32::try_from(h.window).ok(),
                    raw_window_handle::RawWindowHandle::Xcb(h) => Some(h.window.get()),
                    _ => None,
                })
                .unwrap_or(0);
            handler
                .requests
                .x11_window
                .store(handler.x11_window, Ordering::Release);
        }
        // SAFETY: this pre-show builder runs on the window's thread. The
        // handler drops its native adapter before its window context/teardown.
        #[expect(
            unsafe_code,
            reason = "baseview builder owns the native window lifecycle"
        )]
        {
            handler.a11y = cx.window_handle().ok().and_then(|handle| {
                // SAFETY: this builder owns the live window's lifecycle.
                unsafe { NativeAccessibility::new(handle.as_raw()) }
            });
        }
        if let Some((native, _)) = handler.a11y.as_mut() {
            native.focus(cx.has_focus());
        }
        handler.parented = parented;
        let hook = handler
            .requests
            .close
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        Ok(Adapter {
            close: NativeClose {
                hook,
                fired: Cell::new(false),
            },
            cx,
            handler: RefCell::new(handler),
            pending_resize: Cell::new(None),
            pending_events: RefCell::new(VecDeque::new()),
        })
    }
}

/// The window's event handler. Public so a framework adapter can drive it
/// headless in its own tests ([`Handler::new`], [`Handler::step`],
/// [`Handler::on_event_inner`]); a window gets one from [`open`] or [`run`].
#[doc(hidden)]
pub struct Handler<V> {
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
    gpu: Option<Host>,
    software: Option<mui::vello::software::Window<WindowContext>>,
    software_only: bool,
    gpu_retry_at: Instant,
    applied_cursor: Option<MouseCursor>,
    /// The current scene is not on screen yet: paint it.
    unpainted: bool,
    /// A screen reader's side; only a real window has one.
    a11y: Option<A11y>,
    /// A child of a host's window: keep it pinned to the parent's top.
    parented: bool,
    /// The window's scale factor: baseview's pointer is in pixels.
    scale: f64,
    /// The keyboard capture last asked of baseview.
    captured: Option<bool>,
    /// Last successful native text input configuration; changes preserve composition.
    applied_ime: Option<Option<baseview::ImeConfiguration>>,
    /// The queue and the frame schedule.
    pub driver: Driver,
    #[cfg(target_os = "linux")]
    x11_window: u32,
    _reporting: mui::diagnostics::ReportingGuard,
}

#[cfg(target_os = "linux")]
impl<V> Drop for Handler<V> {
    fn drop(&mut self) {
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
        Self {
            shared,
            requests,
            gpu: None,
            software: None,
            software_only: std::env::var("MUI_RENDERER")
                .is_ok_and(|v| v.eq_ignore_ascii_case("cpu")),
            gpu_retry_at: Instant::now(),
            applied_cursor: None,
            unpainted: true,
            a11y: None,
            parented: false,
            scale,
            captured: None,
            applied_ime: None,
            driver: Driver::new(size, scale, Box::new(Clipboard::default())),
            #[cfg(target_os = "linux")]
            x11_window: 0,
            _reporting: mui::diagnostics::retain_reporter(),
        }
    }

    fn tick(&mut self, window: &WindowContext) {
        let requests = &self.requests;
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
        // macOS: keep the child pinned to the parent's top as it resizes.
        // A top-level window's view is its content view: leave it be.
        if self.parented {
            platform::reanchor_to_superview_top(handle);
        }
        let now = Instant::now();
        let size = self.driver.size();
        if self.requests.redraw.swap(false, Ordering::AcqRel) {
            self.driver.redraw();
            if let Some(software) = &mut self.software {
                software.invalidate();
            }
        }
        // Lost between presents: an idle editor would never find out. The
        // next present rebuilds the device.
        if self.gpu.as_ref().is_some_and(Host::device_lost) {
            self.unpainted = true;
        }
        if self.gpu.is_none()
            && self.software.is_none()
            && target_size(size.0, size.1).is_some()
            && now >= self.gpu_retry_at
        {
            if !self.software_only {
                match open_gpu(window, size) {
                    Ok(gpu) => {
                        self.gpu = Some(gpu);
                        lock(&self.shared).ui.set_gpu_welding_available(true);
                        self.unpainted = true;
                    }
                    Err(e) => {
                        log(
                            &self.shared,
                            &format!("mui-baseview: GPU unavailable ({e}); using CPU rendering"),
                        );
                        self.software_only = true;
                    }
                }
            }
            if self.software_only {
                match mui::vello::software::Window::new(window.clone(), size) {
                    Ok(software) => {
                        self.software = Some(software);
                        lock(&self.shared).ui.set_gpu_welding_available(false);
                        self.driver.redraw();
                        self.unpainted = true;
                    }
                    Err(e) => {
                        log(
                            &self.shared,
                            &format!("mui-baseview: CPU presentation unavailable ({e})"),
                        );
                        self.gpu_retry_at = Instant::now() + GPU_RETRY;
                    }
                }
            }
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
            // Keys typed into a field must not reach the host's shortcuts;
            // every other key does. Windows only; a no-op elsewhere, where an
            // ignored key already goes to the host. Only on a change: each
            // call also moves focus.
            capture = s.ui.focus_is_text();
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
            match result {
                Ok(drew) => {
                    self.unpainted = false;
                    if drew {
                        self.requests.presented(size, true);
                    }
                }
                Err(e) => {
                    log(
                        &self.shared,
                        &format!("mui-baseview: CPU render failed ({e})"),
                    );
                    self.software = None;
                    self.gpu_retry_at = Instant::now() + GPU_RETRY;
                }
            }
        }
        if let (Some(gpu), Some(scene)) = (self.gpu.as_mut(), scene)
            && now >= self.gpu_retry_at
        {
            if let Err(e) = gpu.resize(size.0, size.1) {
                log(&self.shared, &format!("mui-baseview: {e}"));
                // Resize errors are terminal configuration/allocation failures.
                // Drop the partly configured surface before attaching CPU presentation.
                self.gpu = None;
                self.software_only = true;
                self.gpu_retry_at = now;
                self.unpainted = true;
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
            match frame {
                Ok(Frame::Presented(_)) => {
                    self.unpainted = false;
                    self.requests.presented(gpu.size(), false);
                }
                Ok(Frame::Current) => {
                    self.unpainted = false;
                    if let Some(profiler) = self.driver.profiler_mut() {
                        profiler.discard_pending_presentation();
                    }
                }
                Ok(Frame::Skipped) => {}
                Ok(Frame::SurfaceLost) => {
                    // SAFETY: the surface comes from this window's live
                    // native handle, and baseview drops the handler that owns
                    // it before the window.
                    #[expect(unsafe_code, reason = "calls the unsafe surface constructor")]
                    let surface = unsafe { surface::create(gpu.instance(), window) };
                    if let Some(surface) = surface {
                        gpu.replace_surface(surface);
                    } else {
                        log(&self.shared, "mui-baseview: surface lost; rebuilding");
                        self.gpu = None;
                        self.gpu_retry_at = now + GPU_RETRY;
                    }
                }
                Err(e) => {
                    log(&self.shared, &format!("mui-baseview: {e}"));
                    // Drop the GPU surface before attaching software presentation.
                    // Keep CPU rendering for this window; do not retry broken
                    // drivers periodically while the user is working.
                    self.gpu = None;
                    self.software_only = true;
                    self.gpu_retry_at = Instant::now();
                    self.unpainted = true;
                }
            }
        }
        let cursor = native_cursor(self.driver.cursor());
        if self.applied_cursor != Some(cursor) {
            let _ = window.set_mouse_cursor(cursor);
            self.applied_cursor = Some(cursor);
        }
    }

    /// One display tick without a window or GPU, a frame's time after the
    /// last: what the headless tests drive.
    pub fn step(&mut self) -> bool {
        if self.requests.redraw.swap(false, Ordering::AcqRel) {
            self.driver.redraw();
        }
        let now = self.driver.last_frame() + Duration::from_millis(16);
        self.driver.advance(&mut lock(&self.shared), now)
    }

    /// The window's new size, as baseview reports it.
    pub fn resized(&mut self, size: WindowSize) {
        self.scale = size.scale_factor;
        let physical = (size.physical.width, size.physical.height);
        self.driver.resized(physical, size.scale_factor);
    }

    /// One native event, as baseview delivers it.
    pub fn on_event_inner(&mut self, event: &Event) -> EventStatus {
        #[cfg(target_os = "linux")]
        if matches!(event, Event::Window(WindowEvent::WillClose)) {
            self.clear_x11_window();
        }
        let scale = self.scale;
        let points = |p: PhysicalPosition<f64>| Point::new(p.x / scale, p.y / scale);
        let d = &mut self.driver;
        match event {
            Event::Window(WindowEvent::RedrawRequested) => {
                if let Some(software) = &mut self.software {
                    software.invalidate();
                    self.unpainted = true;
                }
            }
            Event::Ime(event) => d.ime(native::ime_event(event)),
            Event::Keyboard(key) => {
                let event = key_event(key);
                let hook = self.requests.keys.lock().ok().and_then(|h| h.clone());
                if let Some(hook) = hook {
                    let mut hook = hook
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if hook(&lock(&self.shared).ui, &event) {
                        return EventStatus::Captured;
                    }
                }
                let kept = d.key(&lock(&self.shared), &event);
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
struct Adapter<V> {
    // Cancel owned dialogs before dropping the native context or model.
    close: NativeClose,
    // Drop graphics and native accessibility before their window context.
    handler: RefCell<Handler<V>>,
    cx: WindowContext,
    pending_resize: Cell<Option<WindowSize>>,
    pending_events: RefCell<VecDeque<Event>>,
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

impl<V: View + 'static> WindowHandler for Adapter<V> {
    fn frame_demand(&self) -> baseview::FrameDemand {
        // ponytail: View::changed, plugin meters and a11y requests are polled.
        // Push-based change notifications would remove this idle heartbeat.
        const MODEL_POLL_INTERVAL: Duration = Duration::from_millis(250);
        const RECOVERY_POLL_INTERVAL: Duration = Duration::from_millis(25);
        let Ok(h) = self.handler.try_borrow() else {
            return baseview::FrameDemand::Continuous;
        };
        let now = Instant::now();
        let mut at = now.checked_add(MODEL_POLL_INTERVAL).unwrap_or(now);
        let recovery_poll = now.checked_add(RECOVERY_POLL_INTERVAL).unwrap_or(now);
        if let Some(wake) = h.driver.next_wake() {
            at = at.min(wake);
        }
        if h.requests.redraw.load(Ordering::Acquire) {
            at = now;
        }
        if h.unpainted || (h.gpu.is_none() && h.gpu_retry_at > now) {
            at = at.min(h.gpu_retry_at.max(recovery_poll));
        }
        // GPU-INIT-DEMAND: at merge, if h.gpu_init.as_ref().is_some_and(|init|
        // init.pending()), set at = at.min(recovery_poll).
        // CPU presentation does not mean asynchronous GPU recovery is idle.
        baseview::FrameDemand::At(at)
    }

    fn on_frame(&self) -> Result<(), HandlerError> {
        if let Ok(mut h) = self.handler.try_borrow_mut() {
            let wake_start = h.driver.profiler().map(|_| Instant::now());
            self.drain(&mut h);
            guard(&mut h, |h| h.tick(&self.cx));
            self.drain(&mut h);
            if let (Some(profiler), Some(start)) = (h.driver.profiler_mut(), wake_start) {
                profiler.record_since(mui::profiling::Phase::NativeWake, start);
            }
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
        if matches!(event, Event::Window(WindowEvent::WillClose)) {
            self.close.fire();
        }
        let Ok(mut h) = self.handler.try_borrow_mut() else {
            self.pending_events.borrow_mut().push_back(event);
            return EventStatus::Ignored;
        };
        let status = guard(&mut h, |h| h.on_event_inner(&event)).unwrap_or(EventStatus::Ignored);
        self.drain(&mut h);
        status
    }
}

/// Scene geometry is in UI points; baseview's IME contract uses client pixels.
fn native_ime(config: mui::host::ImeConfiguration, scale: f64) -> baseview::ImeConfiguration {
    let config = native::ime_configuration(config, scale);
    baseview::ImeConfiguration {
        id: config.id,
        position: PhysicalPosition::new(config.area.0.x, config.area.0.y),
        size: baseview::dpi::PhysicalSize::new(config.area.1.width, config.area.1.height),
        text: config.text,
        selection: config.selection,
        marked: config.marked,
    }
}

/// A baseview key as `mui::host` names it.
fn key_event(key: &KeyboardEvent) -> KeyEvent {
    let mut code = DefaultHasher::new();
    key.code.hash(&mut code);
    KeyEvent {
        code: code.finish(),
        key: match &key.key {
            HostKey::Character(s) => NativeKey::Text(s.clone()),
            HostKey::Named(NamedKey::Shift) => NativeKey::Modifier(Modifier::Shift),
            HostKey::Named(NamedKey::Control) => NativeKey::Modifier(Modifier::Ctrl),
            HostKey::Named(NamedKey::Alt | NamedKey::AltGraph) => {
                NativeKey::Modifier(Modifier::Alt)
            }
            HostKey::Named(NamedKey::Meta) => NativeKey::Modifier(Modifier::Cmd),
            // keyboard-types prints the W3C name `Key::from_name` reads.
            HostKey::Named(other) => {
                Key::from_fmt(format_args!("{other}")).map_or(NativeKey::Other, NativeKey::Named)
            }
        },
        down: key.state == KeyState::Down,
        mods: mods(key.modifiers),
    }
}

const fn mouse_button(b: MouseButton) -> Option<Button> {
    match b {
        MouseButton::Left => Some(Button::Primary),
        MouseButton::Right => Some(Button::Secondary),
        MouseButton::Middle => Some(Button::Middle),
        _ => None,
    }
}

const fn mods(m: Modifiers) -> Mods {
    Mods {
        shift: m.contains(Modifiers::SHIFT),
        ctrl: m.contains(Modifiers::CONTROL),
        alt: m.contains(Modifiers::ALT),
        cmd: m.contains(Modifiers::META),
    }
}

const fn native_cursor(cursor: Cursor) -> MouseCursor {
    match cursor {
        Cursor::Arrow => MouseCursor::Default,
        Cursor::Hand | Cursor::Grab => MouseCursor::Hand,
        Cursor::Grabbing => MouseCursor::HandGrabbing,
        Cursor::Text => MouseCursor::Text,
        Cursor::ResizeH => MouseCursor::EwResize,
        Cursor::ResizeV => MouseCursor::NsResize,
        Cursor::Crosshair => MouseCursor::Crosshair,
        Cursor::Forbidden => MouseCursor::NotAllowed,
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
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        lock(shared).view.log(line);
    })) {
        mui::diagnostics::error(
            "mui-baseview",
            "logger_panic",
            mui::diagnostics::panic_message(&*payload),
        );
    }
}

/// A device and renderer for this window's surface. A driver panic becomes
/// an error the editor can show, when the plugin unwinds; under
/// `panic = "abort"` it is the host's crash.
fn open_gpu(window: &WindowContext, size: (u32, u32)) -> Result<Host, String> {
    Host::open_native(
        |backends| {
            let operation = mui::diagnostics::operation(
                "mui-baseview",
                "create_instance",
                &format!("backends={backends:?}"),
                None,
            );
            let display = surface::Display::new(window).map_err(|e| e.to_string())?;
            let mut descriptor =
                wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(display));
            descriptor.backends = backends;
            let instance = wgpu::Instance::new(descriptor);
            drop(operation);
            let operation = mui::diagnostics::operation(
                "mui-baseview",
                "create_surface",
                "creating native window surface",
                None,
            );
            // SAFETY: the surface uses this window's live native handle. Baseview's
            // owned close paths drop the handler/renderer before destroying it.
            // An embedding host must keep that handle alive during callbacks;
            // forced external destruction cannot satisfy the surface lifetime.
            #[expect(unsafe_code, reason = "calls the unsafe surface constructor")]
            let surface = unsafe { surface::create(&instance, window) }
                .ok_or("native surface creation failed")?;
            drop(operation);
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
