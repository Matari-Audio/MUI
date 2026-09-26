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
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use a11y::A11y;
/// The baseview this window runs on, so a consumer names `WindowHandle`,
/// `WindowScalePolicy` and friends without depending on it itself.
pub use baseview;
use baseview::{
    DropData, DropEffect, Event, EventStatus, MouseButton, MouseCursor, MouseEvent, ScrollDelta,
    Window, WindowEvent, WindowHandle, WindowHandler, WindowOpenOptions, WindowScalePolicy,
};
use keyboard_types::{Key as HostKey, KeyState, KeyboardEvent, Modifiers};
use mui::host::{Driver, KeyEvent, Modifier, NativeKey, Wheel};
pub use mui::host::{Shared, View, lock};
use mui::prelude::{Button, Cursor, Key, Mods, Point};
use mui::vello::host::{Frame, Host, target_size};
use mui::vello::kurbo::Affine;
use raw_window_handle::HasRawWindowHandle;

const GPU_RETRY: Duration = Duration::from_millis(500);

/// Requests from the host's thread, applied by the window's next tick,
/// which is the only place a baseview `Window` can be touched.
#[derive(Default)]
pub struct Requests {
    size: AtomicU64,
    scale: AtomicU64,
    redraw: AtomicBool,
}

impl Requests {
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

/// Open the window under `parent`, `size` logical points.
pub fn open<V: View + Send + 'static>(
    parent: &impl HasRawWindowHandle,
    title: &str,
    size: (u32, u32),
    scale: WindowScalePolicy,
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
) -> WindowHandle {
    let (options, build) = prepare(title, size, scale, shared, requests, true);
    Window::open_parented(parent, options, build)
}

/// Run a top-level window of `size` logical points at the system scale, until
/// it closes: an app's main loop.
pub fn run<V: View + Send + 'static>(
    title: &str,
    size: (u32, u32),
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
) {
    let scale = WindowScalePolicy::SystemScaleFactor;
    let (options, build) = prepare(title, size, scale, shared, requests, false);
    Window::open_blocking(options, build);
}

/// The window options and the handler constructor `open` and `run` share.
fn prepare<V: View + Send + 'static>(
    title: &str,
    size: (u32, u32),
    scale: WindowScalePolicy,
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
    parented: bool,
) -> (
    WindowOpenOptions,
    impl FnOnce(&mut Window) -> Handler<V> + Send + 'static,
) {
    let options = WindowOpenOptions {
        title: title.to_owned(),
        size: baseview::Size::new(f64::from(size.0), f64::from(size.1)),
        scale,
    };
    let initial_scale = match scale {
        WindowScalePolicy::ScaleFactor(s) => s,
        WindowScalePolicy::SystemScaleFactor => 1.0,
    };
    let physical = (
        (f64::from(size.0) * initial_scale).round() as u32,
        (f64::from(size.1) * initial_scale).round() as u32,
    );
    let build = move |_: &mut Window| {
        let mut handler = Handler::new(shared, requests, physical, initial_scale);
        handler.a11y = Some(A11y::new());
        handler.parented = parented;
        handler
    };
    (options, build)
}

/// The window's event handler. Public so a framework adapter can drive it
/// headless in its own tests ([`Handler::new`], [`Handler::step`],
/// [`Handler::on_event_inner`]); a window gets one from [`open`] or [`run`].
#[doc(hidden)]
pub struct Handler<V> {
    shared: Arc<Mutex<Shared<V>>>,
    requests: Arc<Requests>,
    gpu: Option<Host>,
    gpu_retry_at: Instant,
    applied_cursor: Option<MouseCursor>,
    /// The current scene is not on screen yet: paint it.
    unpainted: bool,
    /// A screen reader's side; only a real window has one.
    a11y: Option<A11y>,
    /// A child of a host's window: keep it pinned to the parent's top.
    parented: bool,
    /// The queue and the frame schedule.
    pub driver: Driver,
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
            gpu_retry_at: Instant::now(),
            applied_cursor: None,
            unpainted: true,
            a11y: None,
            parented: false,
            driver: Driver::new(size, scale, Box::new(Clipboard::default())),
        }
    }

    fn tick(&mut self, window: &mut Window) {
        let requests = &self.requests;
        let packed = requests.size.swap(0, Ordering::AcqRel);
        if packed != 0 {
            let (w, h) = (packed >> 32, packed & u64::from(u32::MAX));
            window.resize(baseview::Size::new(w as f64, h as f64));
        }
        let bits = requests.scale.swap(0, Ordering::AcqRel);
        if bits != 0 {
            window.set_scale_factor(f64::from_bits(bits));
        }
        // A hidden or detached editor cannot present, and on Windows this is
        // the host's GUI thread: a blocking present there freezes the host.
        let handle = window.raw_window_handle();
        if truce_gui_utils::should_skip_frame(handle) {
            return;
        }
        // macOS: keep the child pinned to the parent's top as it resizes.
        // A top-level window's view is its content view: leave it be.
        if self.parented {
            truce_gui_utils::reanchor_to_superview_top(handle);
        }
        let now = Instant::now();
        let size = self.driver.size();
        if self.requests.redraw.swap(false, Ordering::AcqRel) {
            self.driver.redraw();
        }
        // Lost between presents: an idle editor would never find out. The
        // next present rebuilds the device.
        if self.gpu.as_ref().is_some_and(Host::device_lost) {
            self.unpainted = true;
        }
        if self.gpu.is_none() && target_size(size.0, size.1).is_some() && now >= self.gpu_retry_at {
            match open_gpu(window, size) {
                Ok(gpu) => {
                    self.gpu = Some(gpu);
                    self.unpainted = true;
                }
                Err(e) => {
                    log(
                        &self.shared,
                        &format!("mui-baseview: GPU unavailable ({e}); retrying"),
                    );
                    self.gpu_retry_at = now + GPU_RETRY;
                }
            }
        }
        // The lock covers the frame and a snapshot of its scene, not the
        // present: acquiring a surface texture can wait out a vsync, and a
        // host-thread close() or state load must not wait with it.
        // ponytail: one scene clone per painted frame; have `Ui` hand out an
        // `Arc<ResolvedScene>` if it shows in a profile.
        let scene = {
            let mut s = lock(&self.shared);
            let a11y = self.a11y.as_mut();
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
            if let Some(a11y) = self.a11y.as_mut()
                && (fresh || a11y.wants_tree())
            {
                a11y.publish(&s.ui);
            }
            self.unpainted |= fresh;
            if self.unpainted && self.gpu.is_some() {
                s.ui.scene().cloned()
            } else {
                None
            }
        };
        if let (Some(gpu), Some(scene)) = (self.gpu.as_mut(), scene) {
            if let Err(e) = gpu.resize(size.0, size.1) {
                log(&self.shared, &format!("mui-baseview: {e}"));
            }
            match gpu.present(&scene, Affine::scale(self.driver.ui_scale())) {
                Ok(Frame::Presented(_) | Frame::Current) => self.unpainted = false,
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
                    // Not a lost surface: painting it again would fail
                    // again. A lost device rebuilds on its own schedule.
                    log(&self.shared, &format!("mui-baseview: {e}"));
                    self.unpainted = false;
                }
            }
        }
        let cursor = native_cursor(self.driver.cursor());
        if self.applied_cursor != Some(cursor) {
            window.set_mouse_cursor(cursor);
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

    /// One native event, as baseview delivers it.
    // ponytail: Windows hosts also want `set_keyboard_capture` while a text
    // field is focused; upstream baseview-truce has no such call yet.
    pub fn on_event_inner(&mut self, event: &Event) -> EventStatus {
        let d = &mut self.driver;
        match event {
            Event::Keyboard(key) => {
                let kept = d.key(&lock(&self.shared), &key_event(key));
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
                } => d.pointer_moved(Point::new(position.x, position.y), mods(modifiers)),
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
                MouseEvent::CursorEntered => {}
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
                    let at = Point::new(position.x, position.y);
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
            },
            Event::Window(WindowEvent::Resized(info)) => {
                let physical = info.physical_size();
                d.resized((physical.width, physical.height), info.scale());
            }
            Event::Window(e @ (WindowEvent::Focused | WindowEvent::Unfocused)) => {
                let focused = matches!(e, WindowEvent::Focused);
                d.focus(focused);
                if let Some(a11y) = self.a11y.as_mut() {
                    a11y.focus(focused);
                }
            }
            Event::Window(WindowEvent::WillClose) => d.close(&mut lock(&self.shared)),
        }
        EventStatus::Captured
    }
}

impl<V: View> WindowHandler for Handler<V> {
    fn on_frame(&mut self, window: &mut Window) {
        // baseview calls this from a platform callback: a panic crossing it
        // takes the host down, not just the editor. The guard only exists
        // under `panic = "unwind"` (the `plugin` profile); under release's
        // abort the panic kills the process before it gets here.
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.tick(window))).is_err() {
            log(
                &self.shared,
                "mui-baseview: panic in a frame, swallowed at the FFI edge",
            );
        }
    }

    fn on_event(&mut self, _window: &mut Window, event: Event) -> EventStatus {
        // Same guard as `on_frame`, and only under an unwinding profile.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.on_event_inner(&event)))
            .unwrap_or(EventStatus::Ignored)
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
            HostKey::Shift => NativeKey::Modifier(Modifier::Shift),
            HostKey::Control => NativeKey::Modifier(Modifier::Ctrl),
            HostKey::Alt | HostKey::AltGraph => NativeKey::Modifier(Modifier::Alt),
            HostKey::Meta | HostKey::Super => NativeKey::Modifier(Modifier::Cmd),
            // keyboard-types prints the W3C name `Key::from_name` reads.
            other => {
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
    lock(shared).view.log(line);
}

/// A device and renderer for this window's surface. A driver panic becomes
/// an error the editor can show, when the plugin unwinds; under
/// `panic = "abort"` it is the host's crash.
fn open_gpu(window: &Window, size: (u32, u32)) -> Result<Host, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        // SAFETY: the surface comes from this window's live native handle,
        // and baseview drops the handler that owns it before the window.
        #[expect(unsafe_code, reason = "calls the unsafe surface constructor")]
        let surface = unsafe { surface::create(&instance, window) }
            .ok_or("native surface creation failed")?;
        Host::new(instance, surface, size).map_err(|e| e.to_string())
    }))
    .map_err(|_| "panic while creating GPU resources".to_owned())?
}

mod a11y;
mod surface;
#[cfg(test)]
mod tests;
