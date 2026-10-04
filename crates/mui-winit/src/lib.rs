//! A standalone MUI application host using winit's native Wayland, X11,
//! Windows and macOS windows. Run it on the application's main thread.
//!
//! Plugin editors remain foreign-parent windows through `mui-baseview`.
//! On Linux that embedding is X11 (including an XWayland X11 parent): a
//! Wayland compositor's surface is not an X11 parent handle. This standalone
//! host supports native Wayland, but does not embed a plugin in a Wayland DAW.
#![forbid(unsafe_code)]

mod gpu;
pub use gpu::Gpu;
pub use mui::host::{Shared, View, lock};
pub use winit;

use accesskit_winit::{Adapter, Event as AccessEvent, WindowEvent as AccessWindowEvent};
use mui::host::{Driver, KeyEvent, Modifier, NativeKey, Wheel};
use mui::prelude::{Button, Cursor, Ime, Key, Mods, Point, SemanticAction};
use mui::vello::kurbo::Affine;
use mui_access::accesskit::{Action, ActionData};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{
    Ime as NativeIme, KeyEvent as WinitKeyEvent, MouseButton, MouseScrollDelta, StartCause,
    WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WinitKey, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

/// Initial top-level window properties. The current monitor determines the
/// model polling cadence; `poll_interval` overrides it for low-rate models.
#[derive(Clone, Debug)]
pub struct Options {
    pub title: String,
    pub size: (u32, u32),
    pub poll_interval: Option<Duration>,
    /// Explicit motion policy; winit has no portable native reduced-motion query.
    pub motion_policy: mui::MotionPolicy,
    /// Write bounded CPU phase samples and work counters as CSV on close.
    /// GPU completion and scanout are not measured. Disabled by default.
    pub profile_output: Option<std::path::PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            title: "MUI".into(),
            size: (800, 600),
            poll_interval: None,
            motion_policy: mui::MotionPolicy::System,
            profile_output: None,
        }
    }
}

/// Run `view` and its retained UI until the top-level window closes.
/// Native IME, clipboard and AccessKit are enabled in this path.
pub fn run<V: View>(view: V, ui: mui::Ui, options: Options) -> Result<(), String> {
    run_shared(Arc::new(Mutex::new(Shared { view, ui })), options)
}

/// Run shared state, allowing another application thread to update the model.
/// `View::changed` is polled at the current monitor cadence. Zero-sized initial
/// windows and zero poll intervals are rejected before creating an event loop.
pub fn run_shared<V: View>(shared: Arc<Mutex<Shared<V>>>, options: Options) -> Result<(), String> {
    if options.size.0 == 0 || options.size.1 == 0 {
        return Err("MUI window must have a nonzero size".into());
    }
    if options
        .poll_interval
        .is_some_and(|interval| interval.is_zero())
    {
        return Err("MUI poll interval must be nonzero".into());
    }
    lock(&shared).ui.set_motion_policy(options.motion_policy);
    let event_loop = EventLoop::<AccessEvent>::with_user_event()
        .build()
        .map_err(|e| e.to_string())?;
    let mut app = App {
        shared,
        options,
        proxy: event_loop.create_proxy(),
        gpu: None,
        driver: None,
        access: None,
        published: mui_access::Publisher::default(),
        state: WindowState {
            focused: true,
            ..WindowState::default()
        },
        mods: Mods::default(),
        ime_on: false,
        composing: false,
        native_cursor: None,
        next_poll: Instant::now(),
        error: None,
    };
    event_loop.run_app(&mut app).map_err(|e| e.to_string())?;
    app.error.map_or(Ok(()), Err)
}

/// Visibility and scale are independent: a resize must not unhide an occluded
/// window, and a scale event can occur before the matching size event.
#[derive(Default)]
struct WindowState {
    size: (u32, u32),
    scale: f64,
    occluded: bool,
    focused: bool,
}
impl WindowState {
    fn visible(&self) -> bool {
        !self.occluded && self.size.0 != 0 && self.size.1 != 0
    }
    fn update(&mut self, size: (u32, u32), scale: f64) {
        self.size = size;
        if scale.is_finite() && scale > 0.0 {
            self.scale = scale;
        }
    }
    fn points(&self, x: f64, y: f64) -> Point {
        Point::new(x / self.scale, y / self.scale)
    }
}

struct App<V> {
    shared: Arc<Mutex<Shared<V>>>,
    options: Options,
    proxy: EventLoopProxy<AccessEvent>,
    gpu: Option<Gpu>,
    driver: Option<Driver>,
    access: Option<Adapter>,
    published: mui_access::Publisher,
    state: WindowState,
    mods: Mods,
    ime_on: bool,
    composing: bool,
    native_cursor: Option<Cursor>,
    next_poll: Instant,
    error: Option<String>,
}

impl<V: View> App<V> {
    fn update_size(&mut self, size: (u32, u32), scale: f64) {
        self.state.update(size, scale);
        if let Some(driver) = &mut self.driver {
            driver.resized(size, self.state.scale);
        }
        if let Some(gpu) = &mut self.gpu {
            gpu.resize(size.0, size.1);
        }
    }
    fn cancel(&mut self) {
        self.mods = Mods::default();
        self.composing = false;
        if self.ime_on {
            if let Some(gpu) = &self.gpu {
                gpu.window().set_ime_allowed(false);
            }
            self.ime_on = false;
        }
        if let Some(driver) = &mut self.driver {
            driver.focus(false);
            // Deliver cancellation even while occluded; no presentation needed.
            let interval = driver.min_interval.take();
            driver.advance(&mut lock(&self.shared), Instant::now());
            driver.min_interval = interval;
        }
    }
    fn close(&mut self) {
        if let Some(driver) = &mut self.driver {
            driver.close(&mut lock(&self.shared));
            if let (Some(profile), Some(path)) =
                (driver.take_profiler(), &self.options.profile_output)
            {
                let result = std::fs::File::create(path).and_then(|file| profile.write_csv(file));
                if let Err(error) = result {
                    lock(&self.shared)
                        .view
                        .log(&format!("mui-winit: profile export failed: {error}"));
                }
            }
        }
        if let Some(gpu) = &self.gpu {
            gpu.window().set_ime_allowed(false);
        }
        self.access = None;
        self.gpu = None;
        self.driver = None;
        self.ime_on = false;
        self.native_cursor = None;
    }
    fn publish(&mut self) {
        let (Some(access), Some(driver)) = (&mut self.access, &self.driver) else {
            return;
        };
        let update = {
            let shared = lock(&self.shared);
            let Some(scene) = shared.ui.scene() else {
                return;
            };
            self.published
                .update(scene, shared.ui.focus_key(), driver.ui_scale())
        };
        // Raising native accessibility events can reenter the application.
        access.update_if_active(|| update);
    }
    fn draw(&mut self) {
        if !self.state.visible() {
            return;
        }
        let interval = self.interval();
        let (Some(driver), Some(gpu)) = (&mut self.driver, &mut self.gpu) else {
            return;
        };
        driver.min_interval = Some(interval);
        let (changed, scene) = prepare_frame(driver, &self.shared, Instant::now());
        let window = gpu.window();
        let current_cursor = driver.cursor();
        if self.native_cursor != Some(current_cursor) {
            window.set_cursor(cursor(current_cursor));
            self.native_cursor = Some(current_cursor);
        }
        let area = driver.ime_area();
        let on = area.is_some() && self.state.focused && !self.state.occluded;
        if std::mem::replace(&mut self.ime_on, on) != on {
            window.set_ime_allowed(on);
        }
        if let Some((at, size)) = area {
            let scale = driver.ui_scale();
            window.set_ime_cursor_area(
                PhysicalPosition::new(at.x * scale, at.y * scale),
                PhysicalSize::new(size.width * scale, size.height * scale),
            );
        }
        // RedrawRequested also covers expose/surface recovery when the tree
        // stayed current. Host damage tracking avoids repainting it needlessly.
        let backend_started = driver.profiler().map(|_| Instant::now());
        let presented = scene
            .as_ref()
            .map(|scene| gpu.present(scene, Affine::scale(driver.ui_scale())));
        if let (Some(started), Some(profile)) = (backend_started, driver.profiler_mut()) {
            // Whole backend CPU call: includes encoding, submission and present.
            profile.record_since(mui::profiling::Phase::BackendDraw, started);
            if matches!(&presented, Some(Ok(Some(_)))) {
                // Only a frame submitted for presentation completes the input
                // sample. This measures CPU return, not GPU completion/scanout.
                profile.record_since(mui::profiling::Phase::PresentCall, started);
            }
        }
        if let Some(Err(error)) = presented {
            lock(&self.shared).view.log(&format!("mui-winit: {error}"));
        }
        if changed {
            self.publish();
        }
    }
    fn interval(&self) -> Duration {
        self.options.poll_interval.unwrap_or_else(|| {
            let refresh = self
                .gpu
                .as_ref()
                .and_then(|gpu| gpu.window().current_monitor())
                .and_then(|monitor| monitor.refresh_rate_millihertz())
                .unwrap_or(60_000);
            Duration::from_secs_f64(1000.0 / f64::from(refresh.max(1_000)))
        })
    }
}

impl<V: View> ApplicationHandler<AccessEvent> for App<V> {
    fn new_events(&mut self, _: &ActiveEventLoop, _: StartCause) {
        if let Some(profile) = self.driver.as_mut().and_then(Driver::profiler_mut) {
            profile.count(mui::profiling::Counter::NativeWakes, 1);
        }
    }
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gpu.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(&self.options.title)
            .with_visible(false)
            .with_inner_size(LogicalSize::new(self.options.size.0, self.options.size.1));
        let result = event_loop
            .create_window(attrs)
            .map_err(|e| e.to_string())
            .and_then(|window| {
                let window = Arc::new(window);
                self.access = Some(Adapter::with_event_loop_proxy(
                    event_loop,
                    &window,
                    self.proxy.clone(),
                ));
                Gpu::try_new(window, Box::new(event_loop.owned_display_handle()))
            });
        match result {
            Ok(gpu) => {
                let size = gpu.window().inner_size();
                self.state
                    .update((size.width, size.height), gpu.window().scale_factor());
                self.driver = Some(Driver::new(
                    self.state.size,
                    self.state.scale,
                    Box::new(Clipboard::new()),
                ));
                if self.options.profile_output.is_some() {
                    self.driver
                        .as_mut()
                        .expect("created above")
                        .enable_profiling(mui::profiling::ProfileConfig::default());
                }
                self.driver.as_mut().expect("created above").min_interval = Some(
                    self.options
                        .poll_interval
                        .unwrap_or(Duration::from_secs_f64(1.0 / 60.0)),
                );
                gpu.window().set_visible(true);
                gpu.window().request_redraw(); // First frame must not depend on mouse input.
                self.gpu = Some(gpu);
                self.next_poll = Instant::now() + self.interval();
            }
            Err(error) => {
                self.error = Some(error);
                event_loop.exit();
            }
        }
    }
    fn suspended(&mut self, _: &ActiveEventLoop) {
        self.cancel();
        self.close();
        self.published.reset();
    }
    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.close();
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if !self.state.visible() || self.gpu.is_none() {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        let now = Instant::now();
        let wake = self
            .driver
            .as_ref()
            .and_then(Driver::next_wake)
            .map_or(self.next_poll, |at| at.min(self.next_poll));
        if now >= wake {
            if let Some(gpu) = &self.gpu {
                gpu.window().request_redraw();
            }
            self.next_poll = now + self.interval();
            // A due Driver deadline is consumed on RedrawRequested; waiting
            // until next_poll avoids spinning before winit delivers that redraw.
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_poll));
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(wake));
        }
    }
    fn user_event(&mut self, _: &ActiveEventLoop, event: AccessEvent) {
        let Some(gpu) = &self.gpu else { return };
        if event.window_id != gpu.window().id() {
            return;
        }
        match event.window_event {
            AccessWindowEvent::InitialTreeRequested => {
                self.published.reset();
                self.publish();
            }
            AccessWindowEvent::AccessibilityDeactivated => return,
            AccessWindowEvent::ActionRequested(request) => {
                let mut shared = lock(&self.shared);
                let key = shared
                    .ui
                    .scene()
                    .and_then(|scene| mui_access::surface_of(scene, request.target_node))
                    .map(|s| s.key.to_string());
                if let Some(key) = key {
                    let action = match (request.action, request.data) {
                        (Action::Focus, _) => Some(SemanticAction::focus(key)),
                        (Action::Click, _) => Some(SemanticAction::activate(key)),
                        (Action::Increment, _) => Some(SemanticAction::increment(key)),
                        (Action::Decrement, _) => Some(SemanticAction::decrement(key)),
                        (Action::SetValue, Some(ActionData::NumericValue(value))) => {
                            Some(SemanticAction::set_value(key, value))
                        }
                        (
                            Action::SetTextSelection,
                            Some(ActionData::SetTextSelection(selection)),
                        ) => shared
                            .ui
                            .scene()
                            .and_then(|scene| {
                                mui_access::selection_of(scene, request.target_node, &selection)
                            })
                            .map(|(anchor, caret)| {
                                SemanticAction::set_selection(key, anchor, caret)
                            }),
                        _ => None,
                    };
                    if let Some(action) = action {
                        shared.ui.request_action(action);
                    }
                    if let Some(driver) = &mut self.driver {
                        driver.redraw();
                    }
                }
            }
        }
        if let Some(gpu) = &self.gpu {
            gpu.window().request_redraw();
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(gpu) = &self.gpu else { return };
        if id != gpu.window().id() {
            return;
        }
        if let Some(access) = &mut self.access {
            access.process_event(gpu.window(), &event);
        }
        match event {
            WindowEvent::CloseRequested => {
                self.close();
                event_loop.exit();
                return;
            }
            WindowEvent::RedrawRequested => {
                self.draw();
                return;
            }
            WindowEvent::Resized(size) => {
                self.update_size((size.width, size.height), self.state.scale);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let size = self
                    .gpu
                    .as_ref()
                    .expect("window exists")
                    .window()
                    .inner_size();
                self.update_size((size.width, size.height), scale_factor);
            }
            WindowEvent::Moved(_) => {
                // Crossing monitors may change refresh cadence independently of DPI.
                self.next_poll = Instant::now() + self.interval();
            }
            WindowEvent::Occluded(hidden) => {
                self.state.occluded = hidden;
                if hidden {
                    self.cancel();
                } else if let Some(driver) = &mut self.driver {
                    driver.redraw();
                }
            }
            WindowEvent::Focused(focused) => {
                self.state.focused = focused;
                if focused {
                    if let Some(driver) = &mut self.driver {
                        driver.focus(true);
                    }
                } else {
                    self.cancel();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let modifiers = modifiers.state();
                self.mods = Mods {
                    shift: modifiers.shift_key(),
                    ctrl: modifiers.control_key(),
                    alt: modifiers.alt_key(),
                    cmd: modifiers.super_key(),
                };
                if let Some(driver) = &mut self.driver {
                    let at = driver.pointer().pos;
                    if let Some(at) = at {
                        let zoom = driver.ui_scale() / self.state.scale;
                        driver.pointer_moved(Point::new(at.x * zoom, at.y * zoom), self.mods);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(driver) = &mut self.driver {
                    driver.pointer_moved(self.state.points(position.x, position.y), self.mods);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(driver) = &mut self.driver {
                    driver.pointer_left();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let (Some(driver), Some(button)) = (&mut self.driver, button_of(button)) {
                    driver.button(button, state.is_pressed(), self.mods);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let wheel = match delta {
                    MouseScrollDelta::LineDelta(x, y) => Wheel::Lines(-f64::from(x), f64::from(y)),
                    MouseScrollDelta::PixelDelta(at) => {
                        Wheel::Pixels(-at.x / self.state.scale, at.y / self.state.scale)
                    }
                };
                if let Some(driver) = &mut self.driver {
                    driver.wheel(wheel, self.mods);
                }
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } => {
                let key = key_event(&event, self.mods, self.composing);
                if let Some(driver) = &mut self.driver {
                    driver.key(&lock(&self.shared), &key);
                }
            }
            WindowEvent::Ime(event) => {
                self.composing = matches!(&event, NativeIme::Preedit(text, _) if !text.is_empty());
                let event = match event {
                    NativeIme::Enabled => Ime::Enabled,
                    NativeIme::Disabled => Ime::Disabled,
                    NativeIme::Commit(text) => Ime::Commit(text),
                    NativeIme::Preedit(text, cursor) => Ime::Preedit { text, cursor },
                };
                if let Some(driver) = &mut self.driver {
                    driver.ime(event);
                }
            }
            WindowEvent::HoveredFile(path) => {
                if let Some(driver) = &mut self.driver {
                    driver.drop_files(&mut lock(&self.shared), &[path], false);
                }
            }
            WindowEvent::DroppedFile(path) => {
                if let Some(driver) = &mut self.driver {
                    driver.drop_files(&mut lock(&self.shared), &[path], true);
                }
            }
            _ => return,
        }
        if self.state.visible()
            && let Some(gpu) = &self.gpu
        {
            gpu.window().request_redraw();
        }
    }
}

/// Own the scene before entering native code, which may block or reenter.
fn prepare_frame<V: View>(
    driver: &mut Driver,
    shared: &Mutex<Shared<V>>,
    now: Instant,
) -> (bool, Option<mui::scene::ResolvedScene>) {
    let mut shared = lock(shared);
    let changed = driver.advance(&mut shared, now);
    // ponytail: owned snapshot per redraw; an Arc scene API can remove this
    // clone without holding model locks during surface waits.
    (changed, shared.ui.scene().cloned())
}

fn key_event(event: &WinitKeyEvent, mods: Mods, composing: bool) -> KeyEvent {
    let mut code = DefaultHasher::new();
    event.physical_key.hash(&mut code);
    KeyEvent {
        code: code.finish(),
        down: event.state.is_pressed(),
        mods,
        key: native_key(&event.logical_key, event.text.as_deref(), mods, composing),
    }
}
fn native_key(logical: &WinitKey, typed: Option<&str>, mods: Mods, composing: bool) -> NativeKey {
    match logical {
        WinitKey::Character(text) if !composing || mods.ctrl || mods.cmd => {
            NativeKey::Text(if mods.ctrl || mods.cmd {
                text.to_string()
            } else {
                typed.unwrap_or(text.as_str()).to_owned()
            })
        }
        WinitKey::Named(NamedKey::Shift) => NativeKey::Modifier(Modifier::Shift),
        WinitKey::Named(NamedKey::Control) => NativeKey::Modifier(Modifier::Ctrl),
        WinitKey::Named(NamedKey::Alt | NamedKey::AltGraph) => NativeKey::Modifier(Modifier::Alt),
        WinitKey::Named(NamedKey::Super | NamedKey::Meta) => NativeKey::Modifier(Modifier::Cmd),
        WinitKey::Named(named) => {
            Key::from_fmt(format_args!("{named:?}")).map_or(NativeKey::Other, NativeKey::Named)
        }
        _ => NativeKey::Other,
    }
}
fn button_of(button: MouseButton) -> Option<Button> {
    match button {
        MouseButton::Left => Some(Button::Primary),
        MouseButton::Right => Some(Button::Secondary),
        MouseButton::Middle => Some(Button::Middle),
        _ => None,
    }
}
fn cursor(cursor: Cursor) -> CursorIcon {
    match cursor {
        Cursor::Arrow => CursorIcon::Default,
        Cursor::Hand => CursorIcon::Pointer,
        Cursor::Grab => CursorIcon::Grab,
        Cursor::Grabbing => CursorIcon::Grabbing,
        Cursor::Text => CursorIcon::Text,
        Cursor::ResizeH => CursorIcon::EwResize,
        Cursor::ResizeV => CursorIcon::NsResize,
        Cursor::Crosshair => CursorIcon::Crosshair,
        Cursor::Forbidden => CursorIcon::NotAllowed,
    }
}
struct Clipboard {
    native: Option<arboard::Clipboard>,
    fallback: String,
}
impl Clipboard {
    fn new() -> Self {
        Self {
            native: arboard::Clipboard::new().ok(),
            fallback: String::new(),
        }
    }
}
impl mui::Clipboard for Clipboard {
    fn get(&mut self) -> Option<String> {
        self.native
            .as_mut()
            .and_then(|native| native.get_text().ok())
            .or_else(|| Some(self.fallback.clone()))
    }
    fn set(&mut self, text: &str) {
        self.fallback = text.into();
        if let Some(native) = &mut self.native {
            let _ = native.set_text(text);
        }
    }
}

#[cfg(test)]
mod tests;
