//! A native MUI window that knows no plugin framework: a baseview window,
//! a wgpu surface on it, and `GpuRenderer` painting the resolved scene.
//! [`open`] parents it under a plugin host's window (mui-truce's
//! `MuiEditor` is one consumer; any other framework's adapter opens the same
//! window with its own [`View`]); [`run`] is the same window as a standalone
//! app.
//!
//! The tree and the pointer are in **logical points**; the surface is
//! physical, and the paint transform applies the window's scale once.
//!
//! Native events are queued and each gets its own `Ui::frame` on the next
//! display tick, so a press and release that land between two ticks are
//! still two frames. A tick with no event, no model change, no animation
//! and no deadline due paints nothing.
#![deny(unsafe_code)]
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use a11y::A11y;
use baseview::{
    Event, EventStatus, MouseButton, MouseCursor, MouseEvent, Window, WindowEvent, WindowHandle,
    WindowHandler, WindowOpenOptions, WindowScalePolicy,
};
use keyboard_types::{Key as HostKey, KeyState, Modifiers};
use mui::Ui;
use mui::prelude::{
    Button, Cursor, El, Input, Key, KeyPress, Mods, Point, PointerInput, Size, Vec2,
};
use mui::vello::host::{Frame, Host, target_size};
use mui::vello::kurbo::Affine;
use raw_window_handle::HasRawWindowHandle;

/// A frame after a stall advances time by at most this. Springs are closed
/// form and do not need it; a tooltip timer should not jump a whole idle
/// minute on the first hover after it.
const MAX_DT: f64 = 1.0;
const GPU_RETRY: Duration = Duration::from_millis(500);

/// What the window asks of whoever owns the model.
pub trait View: Send + 'static {
    /// This frame's tree.
    fn build(&mut self, ui: &mut Ui) -> El;
    /// Whether the model moved outside the `Ui` since the last call --
    /// automation, a preset, a meter. Polled every display tick.
    fn changed(&mut self) -> bool;
    /// Ask the host for a new window size in logical points. A host may
    /// refuse, and the window clips rather than overflowing.
    fn request_resize(&mut self, width: u32, height: u32) -> bool;
}

/// The retained `Ui` and the model: shared between the window thread and
/// the plugin's editor, which locks it to end gestures on close.
pub struct Shared<V> {
    pub ui: Ui,
    pub view: V,
}

/// Lock the shared state, through a poisoned lock.
pub fn lock<V>(shared: &Mutex<Shared<V>>) -> MutexGuard<'_, Shared<V>> {
    // A panic caught at the FFI edge (unwinding builds only) poisons the
    // lock; the state is still the last consistent frame's.
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

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
pub fn open<V: View>(
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
pub fn run<V: View>(
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
fn prepare<V: View>(
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

/// A native event, as the frame it will become. Each carries its pointer
/// snapshot, so a fast click is not coalesced into a motionless button.
/// The queue is unbounded on purpose: dropping an edge loses an automation
/// bracket, and the native event loop is already the bounded producer.
#[derive(Debug)]
enum Pending {
    Input(Input),
    /// Hover with no button: consecutive ones coalesce into the newest.
    Move(Input),
    /// Focus went away mid-gesture.
    Cancel(PointerInput),
}

/// The window's event handler. Public so a framework adapter can drive it
/// headless in its own tests ([`Handler::new`], [`Handler::step`],
/// [`Handler::on_event_inner`]); a window gets one from [`open`] or [`run`].
#[doc(hidden)]
pub struct Handler<V> {
    shared: Arc<Mutex<Shared<V>>>,
    gpu: Option<Host>,
    gpu_retry_at: Instant,
    applied_cursor: Option<MouseCursor>,
    /// The current scene is not on screen yet: paint it.
    unpainted: bool,
    /// A screen reader's side; only a real window has one.
    a11y: Option<A11y>,
    /// A child of a host's window: keep it pinned to the parent's top.
    parented: bool,
    driver: Driver,
}

/// Everything but the model and the GPU: the event queue and the frame
/// schedule. Its own struct so a frame can borrow it next to the locked
/// model.
struct Driver {
    requests: Arc<Requests>,
    pointer: PointerInput,
    pending: VecDeque<Pending>,
    clipboard: Clipboard,
    /// Physical pixels, and physical per logical.
    size: (u32, u32),
    scale: f64,
    /// A wheel line in scene units: the theme's text size, as of last frame.
    line: f64,
    cursor: Cursor,
    last_frame: Instant,
    /// The model changed: rebuild.
    dirty: bool,
    animating: bool,
    wake_at: Option<Instant>,
    asked_to_grow: bool,
    /// A refused layout is reported once per run of refusals.
    failing: bool,
}

impl<V: View> Handler<V> {
    /// A handler with no window, GPU or screen reader yet.
    pub fn new(
        shared: Arc<Mutex<Shared<V>>>,
        requests: Arc<Requests>,
        size: (u32, u32),
        scale: f64,
    ) -> Self {
        let now = Instant::now();
        Self {
            shared,
            gpu: None,
            gpu_retry_at: now,
            applied_cursor: None,
            unpainted: true,
            a11y: None,
            parented: false,
            driver: Driver {
                requests,
                pointer: PointerInput::default(),
                pending: VecDeque::new(),
                clipboard: Clipboard::default(),
                size,
                scale,
                line: 16.0,
                cursor: Cursor::Arrow,
                last_frame: now,
                dirty: true,
                animating: false,
                wake_at: None,
                asked_to_grow: false,
                failing: false,
            },
        }
    }

    fn tick(&mut self, window: &mut Window) {
        let requests = &self.driver.requests;
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
        let size = self.driver.size;
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
                    eprintln!("mui-baseview: GPU unavailable ({e}); retrying");
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
                self.driver.dirty = true;
            }
            if let Some(a11y) = a11y
                && a11y.apply(&mut s.ui)
            {
                self.driver.dirty = true;
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
                eprintln!("mui-baseview: {e}");
            }
            match gpu.present(&scene, Affine::scale(self.driver.scale)) {
                Ok(Frame::Presented(_)) => self.unpainted = false,
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
                        eprintln!("mui-baseview: surface lost; rebuilding");
                        self.gpu = None;
                        self.gpu_retry_at = now + GPU_RETRY;
                    }
                }
                Err(e) => {
                    // Not a lost surface: painting it again would fail
                    // again. A lost device rebuilds on its own schedule.
                    eprintln!("mui-baseview: {e}");
                    self.unpainted = false;
                }
            }
        }
        let cursor = native_cursor(self.driver.cursor);
        if self.applied_cursor != Some(cursor) {
            window.set_mouse_cursor(cursor);
            self.applied_cursor = Some(cursor);
        }
    }

    /// One display tick without a window or GPU, a frame's time after the
    /// last: what the headless tests drive.
    pub fn step(&mut self) -> bool {
        let now = self.driver.last_frame + Duration::from_millis(16);
        self.driver.advance(&mut lock(&self.shared), now)
    }

    /// One native event, as baseview delivers it.
    pub fn on_event_inner(&mut self, event: &Event) -> EventStatus {
        let status = self.driver.on_event(event);
        if let (Some(a11y), Event::Window(e @ (WindowEvent::Focused | WindowEvent::Unfocused))) =
            (self.a11y.as_mut(), event)
        {
            a11y.focus(matches!(e, WindowEvent::Focused));
        }
        // A key nothing here has focus for goes back to the host too, so
        // Space still starts its transport.
        // ponytail: a global shortcut read from `Ui::shortcuts` also reaches
        // the host; claiming it needs the tree to say which keys it used.
        if matches!(event, Event::Keyboard(_)) && lock(&self.shared).ui.focus_key().is_none() {
            return EventStatus::Ignored;
        }
        status
    }
}

impl Driver {
    /// Run the queued events and whatever else is due through `Ui::frame`.
    /// Returns whether there is a new scene to paint.
    fn advance<V: View>(&mut self, s: &mut Shared<V>, now: Instant) -> bool {
        self.dirty |= self.requests.redraw.swap(false, Ordering::AcqRel);
        self.dirty |= s.view.changed();
        if target_size(self.size.0, self.size.1).is_none() {
            // Minimised: hold the input edges until there is a size again.
            return false;
        }
        let due = self.wake_at.is_some_and(|at| now >= at);
        if s.ui.scene().is_some()
            && self.pending.is_empty()
            && !self.dirty
            && !self.animating
            && !due
        {
            return false;
        }
        let dt = (now - self.last_frame).as_secs_f64().min(MAX_DT);
        self.last_frame = now;
        // Hit testing needs a scene: the first frame is neutral, and the
        // queued events then land on what it laid out.
        if s.ui.scene().is_none() && self.resolve(s, Input::default(), 0.0, now).is_err() {
            return false;
        }
        let mut timed = false;
        let mut laid_out = true;
        while let Some(event) = self.pending.pop_front() {
            let last = self.pending.is_empty();
            let input = match event {
                Pending::Input(input) | Pending::Move(input) => input,
                Pending::Cancel(pointer) => {
                    s.ui.cancel();
                    Input::from(pointer)
                }
            };
            // Only the last event of the tick carries the elapsed time.
            let event_dt = if last { dt } else { 0.0 };
            // A refused layout has still taken the event's pointer edge,
            // focus change and keys, and queued its gesture edges for the
            // next frame that resolves: replaying it would press or type
            // twice. Only its wheel is lost.
            laid_out = self.resolve(s, input, event_dt, now).is_ok();
            timed |= last;
        }
        if !timed {
            laid_out = self.resolve(s, Input::from(self.pointer), dt, now).is_ok();
        }
        if !laid_out {
            return false;
        }
        self.dirty = false;
        self.ask_to_grow(s);
        true
    }

    fn resolve<V: View>(
        &mut self,
        s: &mut Shared<V>,
        input: Input,
        dt: f64,
        now: Instant,
    ) -> Result<(), ()> {
        s.ui.set_scale(Some(self.scale));
        self.line = s.ui.theme().text;
        let root = s.view.build(&mut s.ui);
        let offered = logical_size(self.size, self.scale);
        match s.ui.frame(root, Some(offered), input, dt) {
            Ok(frame) => {
                self.failing = false;
                self.cursor = frame.cursor;
                // An edge is dispatched by the tree after the frame that
                // delivered it: that tree has to come even if nothing moves.
                self.animating = frame.animating || !frame.edits.is_empty();
                self.wake_at = frame.repaint_after.map(|after| now + after);
                if let Some(text) = frame.clipboard {
                    self.clipboard.write(&text);
                }
                // ponytail: `frame.ime` is dropped; baseview has no
                // candidate-window API to hand it to.
                Ok(())
            }
            Err(e) => {
                if !self.failing {
                    eprintln!("mui-baseview: layout refused at {offered:?}: {e}");
                }
                self.failing = true;
                Err(())
            }
        }
    }

    /// The tree's measured floor, once per window, to the host: `min_size`
    /// is a hint a host may ignore, so a window opened below the floor gets
    /// one polite request to grow.
    fn ask_to_grow<V: View>(&mut self, s: &mut Shared<V>) {
        if std::mem::replace(&mut self.asked_to_grow, true) {
            return;
        }
        let (Some(floor), available) = (s.ui.min_size(), logical_size(self.size, self.scale))
        else {
            return;
        };
        if available.width < floor.width || available.height < floor.height {
            let w = floor.width.max(available.width).ceil() as u32;
            let h = floor.height.max(available.height).ceil() as u32;
            s.view.request_resize(w, h);
        }
    }

    fn on_event(&mut self, event: &Event) -> EventStatus {
        match event {
            Event::Mouse(mouse) => {
                let mut input = Input::default();
                self.on_mouse(mouse, &mut input);
                input.pointer = self.pointer;
                match mouse {
                    MouseEvent::CursorMoved { .. } => self.push_move(input),
                    // A file drag has nothing to drop onto: MUI has no
                    // payload for it. Its position still moves the hover.
                    MouseEvent::DragEntered { .. }
                    | MouseEvent::DragMoved { .. }
                    | MouseEvent::DragDropped { .. } => {
                        self.push_move(input);
                        return EventStatus::Ignored;
                    }
                    _ => self.pending.push_back(Pending::Input(input)),
                }
            }
            Event::Keyboard(key) => {
                self.pointer.mods = event_mods(&key.key, key.modifiers, key.state);
                let mut input = Input::default();
                if key.state == KeyState::Down {
                    on_key(&mut input, &key.key, key.modifiers);
                    if is_paste(&key.key, key.modifiers) {
                        input.clipboard = self.clipboard.read();
                    }
                }
                input.pointer = self.pointer;
                self.pending.push_back(Pending::Input(input));
            }
            Event::Window(WindowEvent::Resized(info)) => {
                let physical = info.physical_size();
                self.size = (physical.width, physical.height);
                self.scale = info.scale();
                self.dirty = true;
            }
            Event::Window(WindowEvent::Unfocused) => {
                self.pointer = PointerInput::default();
                self.pending.push_back(Pending::Cancel(self.pointer));
            }
            Event::Window(WindowEvent::Focused) => {
                self.pending
                    .push_back(Pending::Input(Input::from(self.pointer)));
            }
            Event::Window(WindowEvent::WillClose) => {
                // No tree is built from here on; the editor's close ends the
                // host's gestures itself.
                self.pending.clear();
                self.pointer = PointerInput::default();
            }
        }
        EventStatus::Captured
    }

    /// Hover samples carry no edges: keep only the newest of a run with the
    /// same modifiers. Drag samples all stay, for freehand curves.
    fn push_move(&mut self, input: Input) {
        if input.pointer.buttons.is_empty() {
            if let Some(Pending::Move(previous)) = self.pending.back_mut()
                && previous.pointer.mods == input.pointer.mods
            {
                *previous = input;
                return;
            }
            self.pending.push_back(Pending::Move(input));
        } else {
            self.pending.push_back(Pending::Input(input));
        }
    }

    fn on_mouse(&mut self, mouse: &MouseEvent, input: &mut Input) {
        match *mouse {
            MouseEvent::CursorMoved {
                position,
                modifiers,
            }
            | MouseEvent::DragEntered {
                position,
                modifiers,
                ..
            }
            | MouseEvent::DragMoved {
                position,
                modifiers,
                ..
            }
            | MouseEvent::DragDropped {
                position,
                modifiers,
                ..
            } => {
                self.pointer.pos = Some(Point::new(position.x, position.y));
                self.pointer.mods = mods(modifiers);
            }
            MouseEvent::ButtonPressed { button, modifiers }
            | MouseEvent::ButtonReleased { button, modifiers } => {
                if let Some(b) = mouse_button(button) {
                    let down = matches!(mouse, MouseEvent::ButtonPressed { .. });
                    self.pointer.buttons = self.pointer.buttons.set(b, down);
                }
                self.pointer.mods = mods(modifiers);
            }
            MouseEvent::WheelScrolled { delta, modifiers } => {
                let (x, y) = match delta {
                    // A line is one text row of scene units.
                    baseview::ScrollDelta::Lines { x, y } => {
                        (f64::from(x) * self.line, f64::from(y) * self.line)
                    }
                    baseview::ScrollDelta::Pixels { x, y } => (f64::from(x), f64::from(y)),
                };
                input.wheel = Vec2::new(x, -y);
                self.pointer.mods = mods(modifiers);
            }
            MouseEvent::CursorLeft | MouseEvent::DragLeft => self.pointer.pos = None,
            MouseEvent::CursorEntered => {}
        }
    }
}

impl<V: View> WindowHandler for Handler<V> {
    fn on_frame(&mut self, window: &mut Window) {
        // baseview calls this from a platform callback: a panic crossing it
        // takes the host down, not just the editor. The guard only exists
        // under `panic = "unwind"` (the `plugin` profile); under release's
        // abort the panic kills the process before it gets here.
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.tick(window))).is_err() {
            eprintln!("mui-baseview: panic in a frame, swallowed at the FFI edge");
        }
    }

    fn on_event(&mut self, _window: &mut Window, event: Event) -> EventStatus {
        // Same guard as `on_frame`, and only under an unwinding profile.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.on_event_inner(&event)))
            .unwrap_or(EventStatus::Ignored)
    }
}

/// Space is both streams: text for a focused field, a key for a shortcut.
/// With Ctrl or Cmd held a character is a shortcut key; otherwise it is
/// text, never both, or every character would type twice.
fn on_key(input: &mut Input, key: &HostKey, modifiers: Modifiers) {
    let mods = mods(modifiers);
    if let HostKey::Character(s) = key {
        if s == " " {
            input.keys.push(KeyPress {
                key: Key::Space,
                mods,
            });
            if !mods.ctrl && !mods.cmd {
                input.text.push(' ');
            }
        } else if mods.ctrl || mods.cmd {
            input.keys.extend(s.chars().map(|c| KeyPress {
                key: Key::Char(c),
                mods,
            }));
        } else {
            input.text.push_str(s);
        }
    } else if let Some(key) = named_key(key) {
        input.keys.push(KeyPress { key, mods });
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

/// X11 reports a key press without the modifier being pressed and a release
/// still with it: make the edge explicit, so Shift or Alt alone works
/// without a mouse event in between.
fn event_mods(key: &HostKey, modifiers: Modifiers, state: KeyState) -> Mods {
    let mut m = mods(modifiers);
    let down = state == KeyState::Down;
    match key {
        HostKey::Shift => m.shift = down,
        HostKey::Control => m.ctrl = down,
        HostKey::Alt | HostKey::AltGraph => m.alt = down,
        HostKey::Meta | HostKey::Super => m.cmd = down,
        _ => {}
    }
    m
}

fn is_paste(key: &HostKey, modifiers: Modifiers) -> bool {
    modifiers.intersects(Modifiers::CONTROL | Modifiers::META)
        && matches!(key, HostKey::Character(s) if s.eq_ignore_ascii_case("v"))
}

/// keyboard-types prints the W3C name `Key::from_name` reads.
fn named_key(key: &HostKey) -> Option<Key> {
    Key::from_fmt(format_args!("{key}"))
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

fn logical_size(physical: (u32, u32), scale: f64) -> Size {
    Size::new(f64::from(physical.0) / scale, f64::from(physical.1) / scale)
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
