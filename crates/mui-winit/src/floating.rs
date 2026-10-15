//! Linux floating plugin windows. One private Wayland event loop serves all
//! instances; X11 embedding continues to use baseview. No host event loop is run.
use super::*;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{OnceLock, mpsc};
use winit::platform::wayland::EventLoopBuilderExtWayland;

/// Hooks run on the native thread, outside the model lock.
#[derive(Default)]
pub struct Hooks {
    pub poll: Option<Box<dyn FnMut(&Window, &mut Driver) + Send>>,
    pub key: Option<Box<dyn FnMut(&KeyEvent) -> bool + Send>>,
    pub closed: Option<Box<dyn FnMut() + Send>>,
    pub presented: Option<Box<dyn FnMut((u32, u32), bool) + Send>>,
}
struct Status {
    open: AtomicBool,
    size: AtomicU64,
    scale: AtomicU64,
}
/// Thread-safe ownership of a floating editor. Drop waits for native/model
/// teardown, so callbacks cannot survive the host's editor destruction.
pub struct FloatingWindow {
    id: u64,
    proxy: EventLoopProxy<Event>,
    status: Arc<Status>,
}
impl FloatingWindow {
    pub fn is_open(&self) -> bool {
        self.status.open.load(Ordering::Acquire)
    }
    pub fn size(&self) -> ((u32, u32), f64) {
        let size = self.status.size.load(Ordering::Acquire);
        (
            ((size >> 32) as u32, size as u32),
            f64::from_bits(self.status.scale.load(Ordering::Acquire)),
        )
    }
    pub fn title(&self, title: String) {
        let id = self.id;
        let _ = self
            .proxy
            .send_event(Event::Command(Box::new(move |runtime, _| {
                if let Some(host) = runtime.windows.get_mut(&id) {
                    host.title(&title);
                }
            })));
    }
    pub fn wake(&self) -> impl Fn() + Send + Sync + 'static {
        let proxy = self.proxy.clone();
        let id = self.id;
        move || {
            let _ = proxy.send_event(Event::Command(Box::new(move |runtime, _| {
                if let Some(host) = runtime.windows.get_mut(&id) {
                    host.wake();
                }
            })));
        }
    }
}
impl Drop for FloatingWindow {
    fn drop(&mut self) {
        let (send, recv) = mpsc::sync_channel(0);
        let id = self.id;
        if self
            .proxy
            .send_event(Event::Command(Box::new(move |runtime, _| {
                runtime.windows.remove(&id);
                let _ = send.send(());
            })))
            .is_ok()
        {
            let _ = recv.recv();
        }
    }
}
type Command = Box<dyn FnOnce(&mut Runtime, &ActiveEventLoop) + Send>;
enum Event {
    Access(AccessEvent),
    Command(Command),
}
impl From<AccessEvent> for Event {
    fn from(e: AccessEvent) -> Self {
        Self::Access(e)
    }
}
trait Native {
    fn window_id(&self) -> Option<WindowId>;
    fn event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent);
    fn access(&mut self, event_loop: &ActiveEventLoop, event: AccessEvent);
    fn poll(&mut self, now: Instant) -> Option<Instant>;
    fn title(&self, title: &str);
    fn wake(&mut self);
}
struct Hosted<V: View> {
    app: App<V, Event>,
    hooks: Hooks,
    status: Arc<Status>,
}
impl<V: View> Hosted<V> {
    fn update_status(&self) {
        let Some(gpu) = &self.app.gpu else { return };
        let size = gpu.window().inner_size();
        self.status.size.store(
            (u64::from(size.width) << 32) | u64::from(size.height),
            Ordering::Release,
        );
        self.status
            .scale
            .store(gpu.window().scale_factor().to_bits(), Ordering::Release);
    }
}
impl<V: View> Drop for Hosted<V> {
    fn drop(&mut self) {
        if let Some(closed) = &mut self.hooks.closed {
            closed();
        }
        self.app.close();
        self.status.open.store(false, Ordering::Release);
    }
}
impl<V: View + 'static> Native for Hosted<V> {
    fn window_id(&self) -> Option<WindowId> {
        self.app.gpu.as_ref().map(|g| g.window().id())
    }
    fn event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if let WindowEvent::KeyboardInput { event, .. } = &event {
            let key = key_event(event, self.app.mods, self.app.composing);
            if self.hooks.key.as_mut().is_some_and(|hook| hook(&key)) {
                return;
            }
        }
        let before = self.app.presentations;
        self.app.window_event(event_loop, id, event);
        self.update_status();
        if self.app.presentations != before {
            if let (Some(hook), Some(gpu)) = (&mut self.hooks.presented, &self.app.gpu) {
                hook(gpu.size(), gpu.rendering_mode() != "gpu");
            }
        }
    }
    fn access(&mut self, event_loop: &ActiveEventLoop, event: AccessEvent) {
        self.app.user_event(event_loop, event);
    }
    fn poll(&mut self, now: Instant) -> Option<Instant> {
        if let (Some(hook), Some(gpu), Some(driver)) =
            (&mut self.hooks.poll, &self.app.gpu, &mut self.app.driver)
        {
            hook(gpu.window(), driver);
        }
        // Wayland request_inner_size may apply synchronously without a
        // Resized event. Reconcile both the presenter and the shared status.
        if let Some(gpu) = &self.app.gpu {
            let size = gpu.window().inner_size();
            let scale = gpu.window().scale_factor();
            if self.app.state.size != (size.width, size.height) || self.app.state.scale != scale {
                self.app.update_size((size.width, size.height), scale);
            }
        }
        self.update_status();
        self.app.poll(now)
    }
    fn title(&self, title: &str) {
        if let Some(gpu) = &self.app.gpu {
            gpu.window().set_title(title);
        }
    }
    fn wake(&mut self) {
        if let Some(driver) = &mut self.app.driver {
            driver.redraw();
        }
        if let Some(gpu) = &self.app.gpu {
            gpu.window().request_redraw();
        }
    }
}
struct Runtime {
    windows: BTreeMap<u64, Box<dyn Native>>,
}
impl ApplicationHandler<Event> for Runtime {
    fn resumed(&mut self, _: &ActiveEventLoop) {}
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
        match event {
            Event::Command(command) => command(self, event_loop),
            Event::Access(event) => {
                if let Some(host) = self
                    .windows
                    .values_mut()
                    .find(|host| host.window_id() == Some(event.window_id))
                {
                    host.access(event_loop, event);
                }
            }
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(token) = self
            .windows
            .iter()
            .find_map(|(token, host)| (host.window_id() == Some(id)).then_some(*token))
        else {
            return;
        };
        if matches!(event, WindowEvent::CloseRequested | WindowEvent::Destroyed) {
            self.windows.remove(&token);
        } else if let Some(host) = self.windows.get_mut(&token) {
            host.event(event_loop, id, event);
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let next = self
            .windows
            .values_mut()
            .filter_map(|host| host.poll(now))
            .min();
        event_loop.set_control_flow(next.map_or(ControlFlow::Wait, ControlFlow::WaitUntil));
    }
}
fn proxy() -> Result<EventLoopProxy<Event>, String> {
    static PROXY: OnceLock<Result<EventLoopProxy<Event>, String>> = OnceLock::new();
    PROXY
        .get_or_init(|| {
            // The loop persists across editor close/reopen. Pin its containing DSO
            // before spawning, as baseview does for detached native/GPU workers.
            if !baseview::pin_current_image_for_detached_work() {
                return Err("cannot pin Wayland window runtime".into());
            }
            let (send, recv) = mpsc::sync_channel(1);
            std::thread::Builder::new()
                .name("mui-wayland".into())
                .spawn(move || {
                    let result = EventLoop::<Event>::with_user_event()
                        .with_wayland()
                        .with_any_thread(true)
                        .build();
                    match result {
                        Ok(event_loop) => {
                            let _ = send.send(Ok(event_loop.create_proxy()));
                            let _ = event_loop.run_app(&mut Runtime {
                                windows: BTreeMap::new(),
                            });
                        }
                        Err(error) => {
                            let _ = send.send(Err(error.to_string()));
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            recv.recv().map_err(|e| e.to_string())?
        })
        .clone()
}
/// Open a wl_surface-backed floating plugin editor when CLAP calls show. Creation
/// errors are returned; this API never falls back to X11 after Wayland negotiation.
pub fn open<V: View + Send + 'static>(
    shared: Arc<Mutex<Shared<V>>>,
    options: Options,
    hooks: Hooks,
) -> Result<FloatingWindow, String> {
    validate_options(&options)?;
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let proxy = proxy()?;
    let native_proxy = proxy.clone();
    let status = Arc::new(Status {
        open: AtomicBool::new(false),
        size: AtomicU64::new(0),
        scale: AtomicU64::new(1f64.to_bits()),
    });
    let native_status = status.clone();
    let (send, recv) = mpsc::sync_channel(1);
    proxy
        .send_event(Event::Command(Box::new(move |runtime, event_loop| {
            let mut app = App::new(shared, options, native_proxy);
            app.exit_on_error = false;
            app.resumed(event_loop);
            if let Some(error) = app.error.take() {
                let _ = send.send(Err(error));
                return;
            }
            let host = Hosted {
                app,
                hooks,
                status: native_status,
            };
            host.update_status();
            host.status.open.store(true, Ordering::Release);
            runtime.windows.insert(id, Box::new(host));
            let _ = send.send(Ok(()));
        })))
        .map_err(|_| "Wayland runtime stopped".to_string())?;
    recv.recv().map_err(|e| e.to_string())??;
    Ok(FloatingWindow { id, proxy, status })
}
