//! Several native windows on one main-thread winit event loop.
use super::*;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Stable application identity, independent of native handles recreated on resume.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct WindowToken(u64);

/// Native notifications for one window. Callbacks run without the model lock.
/// `Presented` means the GPU host returned `Frame::Presented`, not GPU completion
/// or compositor scanout. File notifications are individual winit paths; winit
/// supplies no position in these events. The View callback uses its current
/// driver pointer, or the origin when that position is unavailable.
#[derive(Clone, Debug, PartialEq)]
pub enum HostEvent {
    Opened,
    Presented {
        size: (u32, u32),
        rendering_mode: &'static str,
    },
    Resized {
        size: (u32, u32),
    },
    Focused(bool),
    PointerEntered,
    PointerLeft,
    HoveredFile(PathBuf),
    DroppedFile(PathBuf),
    HoveredFileCancelled,
    Closed,
}

type Observer<V> = Box<dyn FnMut(WindowToken, HostEvent, &WindowController<V>)>;

/// A window's independent model, UI, options and optional native notification sink.
/// All windows in one runner use the same view type; an enum can wrap different
/// application views. Each window must have a distinct Shared handle; share
/// application data through the views instead of sharing retained UI state. Native handles and notification sinks remain on the main thread.
pub struct WindowSpec<V: 'static> {
    shared: Arc<Mutex<Shared<V>>>,
    options: Options,
    observer: Option<Observer<V>>,
}

impl<V: View + 'static> WindowSpec<V> {
    pub fn new(view: V, ui: mui::Ui, options: Options) -> Self {
        Self::shared(Arc::new(Mutex::new(Shared { view, ui })), options)
    }

    pub fn shared(shared: Arc<Mutex<Shared<V>>>, options: Options) -> Self {
        Self {
            shared,
            options,
            observer: None,
        }
    }

    /// Observe native window events and successful submissions, outside Shared locks.
    /// Use the controller to queue native operations; commands execute after this
    /// callback returns. A callback may update its model using its own Shared handle.
    pub fn on_event(
        mut self,
        observer: impl FnMut(WindowToken, HostEvent, &WindowController<V>) + 'static,
    ) -> Self {
        self.observer = Some(Box::new(observer));
        self
    }
}

enum Command<V: 'static> {
    Open(WindowToken, Box<dyn FnOnce() -> WindowSpec<V> + Send>),
    Close(WindowToken),
    Redraw(WindowToken),
    Resize(WindowToken, (u32, u32)),
}

enum Event<V: 'static> {
    Access(AccessEvent),
    Command(Command<V>),
}
impl<V: 'static> From<AccessEvent> for Event<V> {
    fn from(event: AccessEvent) -> Self {
        Self::Access(event)
    }
}

/// Thread-safe typed commands for one application's main-thread window loop.
/// Cloning a controller does not keep the event loop alive. Commands to a closed
/// token are harmless; sending after loop shutdown returns an error.
pub struct WindowController<V: 'static> {
    proxy: EventLoopProxy<Event<V>>,
    next: Arc<AtomicU64>,
}
impl<V: 'static> Clone for WindowController<V> {
    fn clone(&self) -> Self {
        Self {
            proxy: self.proxy.clone(),
            next: self.next.clone(),
        }
    }
}
impl<V: View + 'static> WindowController<V> {
    fn token(&self) -> WindowToken {
        // Exhausting u64 identities would otherwise alias an existing window.
        WindowToken(
            self.next
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .expect("MUI window identities exhausted"),
        )
    }
    fn send(&self, command: Command<V>) -> Result<(), String> {
        self.proxy
            .send_event(Event::Command(command))
            .map_err(|_| "MUI event loop is closed".into())
    }
    /// Build a new window on the event-loop thread. The Send factory allows the
    /// view, UI and observer themselves to stay main-thread-only. Invalid options
    /// or native/GPU creation failure terminate the runner with an error.
    pub fn open(
        &self,
        factory: impl FnOnce() -> WindowSpec<V> + Send + 'static,
    ) -> Result<WindowToken, String> {
        let token = self.token();
        self.send(Command::Open(token, Box::new(factory)))?;
        Ok(token)
    }
    pub fn close(&self, window: WindowToken) -> Result<(), String> {
        self.send(Command::Close(window))
    }
    pub fn redraw(&self, window: WindowToken) -> Result<(), String> {
        self.send(Command::Redraw(window))
    }
    /// Request logical dimensions. Native platforms may constrain the result;
    /// `HostEvent::Resized` reports the actual physical dimensions.
    pub fn resize(&self, window: WindowToken, size: (u32, u32)) -> Result<(), String> {
        if size.0 == 0 || size.1 == 0 {
            return Err("MUI window must have a nonzero size".into());
        }
        self.send(Command::Resize(window, size))
    }
}

/// Run independent windows on the application's main thread until all close.
/// `started` receives their tokens, in input order, before entering the event loop.
/// It may retain or send the controller to other threads. Empty initial lists and
/// invalid options fail before event-loop creation. Each window owns its own GPU
/// host, display handle, Driver, IME, accessibility publisher and profile output;
/// use distinct profile paths to avoid overwriting another window's export.
/// Existing `run` and `run_shared` remain available for a single window.
pub fn run_windows<V: View + 'static>(
    windows: Vec<WindowSpec<V>>,
    started: impl FnOnce(WindowController<V>, Vec<WindowToken>),
) -> Result<(), String> {
    if windows.is_empty() {
        return Err("MUI requires an initial window".into());
    }
    for (index, window) in windows.iter().enumerate() {
        validate_options(&window.options)?;
        if windows[..index]
            .iter()
            .any(|other| Arc::ptr_eq(&window.shared, &other.shared))
        {
            return Err("MUI windows must have independent Shared UI state".into());
        }
    }
    let event_loop = EventLoop::<Event<V>>::with_user_event()
        .build()
        .map_err(|e| e.to_string())?;
    let controller = WindowController {
        proxy: event_loop.create_proxy(),
        next: Arc::new(AtomicU64::new(1)),
    };
    let mut app = Windows {
        windows: BTreeMap::new(),
        controller: controller.clone(),
        resumed: false,
        error: None,
    };
    let mut tokens = Vec::with_capacity(windows.len());
    for spec in windows {
        let token = controller.token();
        tokens.push(token);
        app.insert(token, spec);
    }
    started(controller, tokens);
    event_loop.run_app(&mut app).map_err(|e| e.to_string())?;
    app.error.map_or(Ok(()), Err)
}

struct Hosted<V: 'static> {
    app: App<V, Event<V>>,
    observer: Option<Observer<V>>,
}
struct Windows<V: 'static> {
    windows: BTreeMap<WindowToken, Hosted<V>>,
    controller: WindowController<V>,
    resumed: bool,
    error: Option<String>,
}
impl<V: View + 'static> Windows<V> {
    fn insert(&mut self, token: WindowToken, spec: WindowSpec<V>) {
        self.windows.insert(
            token,
            Hosted {
                app: App::new(spec.shared, spec.options, self.controller.proxy.clone()),
                observer: spec.observer,
            },
        );
    }
    fn notify(&mut self, token: WindowToken, event: HostEvent) {
        if let Some(observer) = self
            .windows
            .get_mut(&token)
            .and_then(|host| host.observer.as_mut())
        {
            observer(token, event, &self.controller);
        }
    }
    fn find(&self, id: WindowId) -> Option<WindowToken> {
        self.windows.iter().find_map(|(token, host)| {
            (host
                .app
                .gpu
                .as_ref()
                .is_some_and(|gpu| gpu.window().id() == id))
            .then_some(*token)
        })
    }
    fn open_native(&mut self, token: WindowToken, event_loop: &ActiveEventLoop) {
        let host = self.windows.get_mut(&token).expect("known window");
        let was_open = host.app.gpu.is_some();
        host.app.resumed(event_loop);
        if let Some(error) = host.app.error.take() {
            self.error = Some(error);
        } else if !was_open {
            self.notify(token, HostEvent::Opened);
        }
    }
    fn close(&mut self, token: WindowToken, event_loop: &ActiveEventLoop) {
        if let Some(mut host) = self.windows.remove(&token) {
            host.app.close();
            if let Some(observer) = &mut host.observer {
                observer(token, HostEvent::Closed, &self.controller);
            }
        }
        if self.windows.is_empty() {
            event_loop.exit();
        }
    }
}
impl<V: View + 'static> ApplicationHandler<Event<V>> for Windows<V> {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        for host in self.windows.values_mut() {
            host.app.new_events(event_loop, cause);
        }
    }
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.resumed = true;
        for token in self.windows.keys().copied().collect::<Vec<_>>() {
            self.open_native(token, event_loop);
        }
    }
    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.resumed = false;
        for host in self.windows.values_mut() {
            host.app.suspended(event_loop);
        }
    }
    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        for (token, mut host) in std::mem::take(&mut self.windows) {
            host.app.exiting(event_loop);
            if let Some(observer) = &mut host.observer {
                observer(token, HostEvent::Closed, &self.controller);
            }
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // A single deadline covers all windows; one occluded window must not
        // replace another window's deadline with ControlFlow::Wait.
        let now = Instant::now();
        let wake = self
            .windows
            .values_mut()
            .filter_map(|host| host.app.poll(now))
            .min();
        event_loop.set_control_flow(wake.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event<V>) {
        match event {
            Event::Access(event) => {
                if let Some(token) = self.find(event.window_id) {
                    self.windows
                        .get_mut(&token)
                        .expect("known window")
                        .app
                        .user_event(event_loop, event);
                }
            }
            Event::Command(Command::Open(token, factory)) => {
                let spec = factory();
                if let Err(error) = validate_options(&spec.options) {
                    self.error = Some(error);
                    event_loop.exit();
                    return;
                }
                if self
                    .windows
                    .values()
                    .any(|host| Arc::ptr_eq(&host.app.shared, &spec.shared))
                {
                    self.error = Some("MUI windows must have independent Shared UI state".into());
                    event_loop.exit();
                    return;
                }
                self.insert(token, spec);
                if self.resumed {
                    self.open_native(token, event_loop);
                }
            }
            Event::Command(Command::Close(token)) => self.close(token, event_loop),
            Event::Command(command) => {
                let (token, size) = match command {
                    Command::Redraw(token) => (token, None),
                    Command::Resize(token, size) => (token, Some(size)),
                    _ => unreachable!(),
                };
                if let Some(host) = self.windows.get_mut(&token) {
                    let app = &mut host.app;
                    if let Some(gpu) = &app.gpu {
                        if let Some(size) = size {
                            let actual = gpu
                                .window()
                                .request_inner_size(LogicalSize::new(size.0, size.1));
                            if let Some(actual) = actual {
                                app.update_size((actual.width, actual.height), app.state.scale);
                            }
                        } else if let Some(driver) = &mut app.driver {
                            driver.redraw();
                        }
                        if let Some(gpu) = &app.gpu {
                            gpu.window().request_redraw();
                        }
                    } else if let Some(size) = size {
                        app.options.size = size;
                    }
                }
            }
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(token) = self.find(id) else {
            return;
        };
        if matches!(&event, WindowEvent::CloseRequested | WindowEvent::Destroyed) {
            self.close(token, event_loop);
            return;
        }
        let notification = match &event {
            WindowEvent::Resized(size) => Some(HostEvent::Resized {
                size: (size.width, size.height),
            }),
            WindowEvent::Focused(focused) => Some(HostEvent::Focused(*focused)),
            WindowEvent::CursorEntered { .. } => Some(HostEvent::PointerEntered),
            WindowEvent::CursorLeft { .. } => Some(HostEvent::PointerLeft),
            WindowEvent::HoveredFile(path) => Some(HostEvent::HoveredFile(path.clone())),
            WindowEvent::DroppedFile(path) => Some(HostEvent::DroppedFile(path.clone())),
            WindowEvent::HoveredFileCancelled => Some(HostEvent::HoveredFileCancelled),
            _ => None,
        };
        let host = self.windows.get_mut(&token).expect("known window");
        let presentations = host.app.presentations;
        host.app.window_event(event_loop, id, event);
        let presented = (host.app.presentations != presentations).then(|| {
            (
                host.app.state.size,
                host.app
                    .gpu
                    .as_ref()
                    .expect("presented window")
                    .rendering_mode(),
            )
        });
        if let Some(event) = notification {
            self.notify(token, event);
        }
        if let Some((size, rendering_mode)) = presented {
            self.notify(
                token,
                HostEvent::Presented {
                    size,
                    rendering_mode,
                },
            );
        }
    }
}
