//! Native Wayland layer surfaces on a toolkit-owned queue and owner thread.
mod context;
mod drag;
mod input;
mod text_input;

use super::{linux, x11::PlatformError};
use crate::{dpi::*, window::WindowInitializer, *};
use calloop::{channel, EventLoop, LoopSignal};
use calloop_wayland_source::WaylandSource;
pub use context::{Context, PlatformHandle};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::SeatState,
    shell::{
        wlr_layer::{Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_surface},
    Connection, QueueHandle,
};

fn failure(error: impl std::fmt::Display) -> PlatformError {
    PlatformError::CreationFailed(error.to_string())
}

struct Shared {
    open: AtomicBool,
    visible: AtomicBool,
    desired_visible: AtomicBool,
    redraw: AtomicBool,
    size: AtomicU64,
    scale: AtomicU64,
    error: Mutex<Option<String>>,
    surface: Mutex<Option<(LayerSurface, Connection)>>,
}
impl Shared {
    fn size(&self) -> WindowSize {
        let packed = self.size.load(Ordering::Acquire);
        WindowSize::from_physical(
            PhysicalSize::new((packed >> 32) as u32, packed as u32),
            f64::from_bits(self.scale.load(Ordering::Acquire)),
        )
    }
}

enum Command {
    Show,
    Hide,
    Resize(Size),
    Close,
}

pub struct WindowHandle {
    shared: Arc<Shared>,
    sender: channel::SyncSender<Command>,
    signal: LoopSignal,
    thread: Cell<Option<JoinHandle<()>>>,
    close_timeout: Cell<Duration>,
}
impl WindowHandle {
    pub fn create_window(init: WindowInitializer) -> linux::Result<Self> {
        if init.settings.parent.is_some() || init.settings.wait_for_parent {
            return Err(failure("a desktop dock cannot be a host-owned child"));
        }
        if !crate::pin_current_image_for_detached_work() {
            return Err(failure("could not pin the dock's executable image"));
        }
        let shared = Arc::new(Shared {
            open: AtomicBool::new(true),
            visible: AtomicBool::new(false),
            desired_visible: AtomicBool::new(false),
            redraw: AtomicBool::new(true),
            size: AtomicU64::new((800_u64 << 32) | 160),
            scale: AtomicU64::new(1_f64.to_bits()),
            error: Mutex::new(None),
            surface: Mutex::new(None),
        });
        let WindowInitializer { settings, builder, host } = init;
        drop(host);
        let (send, recv) = mpsc::sync_channel(1);
        let (sender, receiver) = channel::sync_channel(8);
        let state = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("mui-dock-wayland".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut event_loop = EventLoop::<Runtime>::try_new().map_err(failure)?;
                    let signal = event_loop.get_signal();
                    let mut runtime =
                        Runtime::new(settings, builder, Arc::clone(&state), &mut event_loop)?;
                    event_loop
                        .handle()
                        .insert_source(receiver, |event, _, runtime| match event {
                            channel::Event::Msg(command) => runtime.command(command),
                            channel::Event::Closed => runtime.exit = true,
                        })
                        .map_err(failure)?;
                    if send.send(Ok(signal)).is_err() {
                        return Ok(());
                    }
                    while !runtime.exit && state.open.load(Ordering::Acquire) {
                        runtime.tick()?;
                        event_loop.dispatch(runtime.deadline(), &mut runtime).map_err(failure)?;
                    }
                    runtime.close();
                    Ok::<(), PlatformError>(())
                }));
                let error = match result {
                    Ok(Ok(())) => None,
                    Ok(Err(e)) => Some(e.to_string()),
                    Err(_) => Some("panic in native dock runtime".into()),
                };
                if let Some(error) = error {
                    let _ = send.try_send(Err(error.clone()));
                    *state.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(error);
                }
                state.open.store(false, Ordering::Release);
            })
            .map_err(failure)?;
        match recv.recv_timeout(Duration::from_secs(3)) {
            Ok(Ok(signal)) => Ok(Self {
                shared,
                sender,
                signal,
                thread: Cell::new(Some(thread)),
                close_timeout: Cell::new(Duration::from_millis(250)),
            }),
            result => {
                shared.open.store(false, Ordering::Release);
                if thread.is_finished() {
                    let _ = thread.join();
                }
                Err(failure(match result {
                    Ok(Err(e)) => e,
                    Err(e) => e.to_string(),
                    _ => "dock creation failed".into(),
                }))
            }
        }
    }
    fn command(&self, command: Command) -> linux::Result<()> {
        self.sender.try_send(command).map_err(failure)
    }
    pub fn size(&self) -> WindowSize {
        self.shared.size()
    }
    pub fn resize(&self, size: Size) -> linux::Result<()> {
        self.command(Command::Resize(size))
    }
    pub fn show(&self) -> linux::Result<()> {
        self.shared.desired_visible.store(true, Ordering::Release);
        self.signal.wakeup();
        Ok(())
    }
    pub fn hide(&self) -> linux::Result<()> {
        self.shared.desired_visible.store(false, Ordering::Release);
        self.signal.wakeup();
        Ok(())
    }
    pub fn suggest_scale_factor(&self, _: f64) -> linux::Result<()> {
        Ok(())
    }
    pub fn set_scale_factor_override(&self, _: Option<f64>) -> linux::Result<()> {
        Ok(())
    }
    pub fn set_keyboard_capture(&self, _: bool) {}
    pub fn set_close_timeout(&self, timeout: Duration) {
        self.close_timeout.set(timeout);
    }
    pub fn is_open(&self) -> bool {
        self.shared.open.load(Ordering::Acquire)
    }
    pub fn is_resizable(&self) -> bool {
        false
    }
    pub fn min_size(&self) -> Option<Size> {
        None
    }
    pub fn max_size(&self) -> Option<Size> {
        None
    }
    pub fn handle_main_thread_callback(&self) {}
    pub fn set_parent(&self, _: super::x11::ParentWindowHandle) -> linux::Result<()> {
        Err(failure("a layer surface cannot be embedded"))
    }
    pub fn visibility_control(&self) -> WindowVisibility {
        let shared = Arc::downgrade(&self.shared);
        let signal = self.signal.clone();
        WindowVisibility::new(move |visible| {
            if let Some(s) = shared.upgrade() {
                if s.open.load(Ordering::Acquire) {
                    s.desired_visible.store(visible, Ordering::Release);
                    s.redraw.store(true, Ordering::Release);
                    signal.wakeup();
                }
            }
        })
    }
    pub fn frame_requester(&self) -> FrameRequester {
        let shared = Arc::downgrade(&self.shared);
        let signal = self.signal.clone();
        FrameRequester::new(move || {
            if let Some(s) = shared.upgrade() {
                s.redraw.store(true, Ordering::Release);
                signal.wakeup();
            }
        })
    }
    pub fn run_until_closed(&self) -> linux::Result<()> {
        self.show()?;
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|_| failure("dock thread panicked"))?;
        }
        match self.shared.error.lock().unwrap_or_else(|e| e.into_inner()).take() {
            Some(e) => Err(failure(e)),
            None => Ok(()),
        }
    }
}
impl Drop for WindowHandle {
    fn drop(&mut self) {
        self.shared.open.store(false, Ordering::Release);
        let _ = self.sender.try_send(Command::Close);
        self.signal.wakeup();
        // Release the reservation even when a product renderer is stalled.
        // Wayland proxies/Connection support requests from another thread; only
        // the native owner dispatches events or accesses the product handler.
        if let Some((layer, conn)) =
            self.shared.surface.lock().unwrap_or_else(|e| e.into_inner()).take()
        {
            layer.set_exclusive_zone(0);
            layer.wl_surface().attach(None, 0, 0);
            layer.wl_surface().commit();
            let _ = conn.flush();
        }
        if let Some(thread) = self.thread.take() {
            let start = Instant::now();
            while !thread.is_finished() && start.elapsed() < self.close_timeout.get() {
                std::thread::sleep(Duration::from_millis(2));
            }
            if thread.is_finished() {
                let _ = thread.join();
            }
            // Startup pinned the image. A stalled callback retains its own
            // handler/surface until retirement; no new product input is admitted.
        }
    }
}

struct Runtime {
    registry: RegistryState,
    outputs: OutputState,
    seats: SeatState,
    shared: Arc<Shared>,
    context: Option<Rc<Context>>,
    handler: Option<Box<dyn WindowHandler>>,
    keyboard: Option<wayland_client::protocol::wl_keyboard::WlKeyboard>,
    pointer: Option<wayland_client::protocol::wl_pointer::WlPointer>,
    data: drag::Data,
    modifiers: keyboard_types::Modifiers,
    ready: bool,
    configured: bool,
    exit: bool,
}
impl Runtime {
    fn new(
        settings: WindowSettings, builder: crate::handler::WindowHandlerBuilder,
        shared: Arc<Shared>, event_loop: &mut EventLoop<Self>,
    ) -> linux::Result<Self> {
        let conn = Connection::connect_to_env().map_err(failure)?;
        let (globals, mut queue) = registry_queue_init(&conn).map_err(failure)?;
        let qh = queue.handle();
        let compositor = CompositorState::bind(&globals, &qh).map_err(failure)?;
        let layers = LayerShell::bind(&globals, &qh).map_err(failure)?;
        let mut runtime = Self {
            registry: RegistryState::new(&globals),
            outputs: OutputState::new(&globals, &qh),
            seats: SeatState::new(&globals, &qh),
            data: drag::Data::new(&globals, &qh)?,
            shared: Arc::clone(&shared),
            context: None,
            handler: None,
            keyboard: None,
            pointer: None,
            modifiers: keyboard_types::Modifiers::empty(),
            ready: true,
            configured: false,
            exit: false,
        };
        queue.roundtrip(&mut runtime).map_err(failure)?;
        let options = settings.dock.ok_or_else(|| failure("missing dock options"))?;
        let output = runtime.outputs.outputs().find(|output| {
            options.output.as_ref().is_none_or(|name| {
                runtime.outputs.info(output).is_some_and(|i| i.name.as_ref() == Some(name))
            })
        });
        if options.output.is_some() && output.is_none() {
            return Err(failure("selected dock output is unavailable"));
        }
        let surface = compositor.create_surface(&qh);
        let layer = layers.create_layer_surface(
            &qh,
            surface,
            Layer::Top,
            Some("mui-dock"),
            output.as_ref(),
        );
        if matches!(layer.kind(), smithay_client_toolkit::shell::wlr_layer::SurfaceKind::Wlr(w) if w.version() < 4)
        {
            return Err(failure("layer-shell version 4 is required for on-demand keyboard focus"));
        }
        let context = Rc::new(Context::new(
            conn.clone(),
            layer,
            options,
            qh.clone(),
            shared,
            event_loop.get_signal(),
            &globals,
        )?);
        for seat in runtime.seats.seats() {
            context.text.borrow_mut().seat(&seat, &qh);
        }
        *runtime.shared.surface.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((context.layer.clone(), context.conn.clone()));
        runtime.context = Some(Rc::clone(&context));
        // Configure without a buffer, and acknowledge before any presenter can attach.
        context.configure();
        queue.roundtrip(&mut runtime).map_err(failure)?;
        if !runtime.configured {
            return Err(failure("compositor did not configure the dock"));
        }
        let handler = builder
            .build(crate::WindowContext::new(linux::WindowContext::Layer(context)))
            .map_err(failure)?;
        runtime.handler = Some(handler);
        WaylandSource::new(conn, queue).insert(event_loop.handle()).map_err(failure)?;
        Ok(runtime)
    }
    fn command(&mut self, command: Command) {
        let Some(cx) = &self.context else { return };
        match command {
            Command::Close => self.exit = true,
            Command::Resize(size) => {
                let _ = cx.resize(size);
                self.shared.redraw.store(true, Ordering::Release);
            }
            Command::Show => {
                self.shared.visible.store(true, Ordering::Release);
                self.ready = true;
                if !self.configured {
                    cx.configure();
                }
            }
            Command::Hide => {
                self.shared.visible.store(false, Ordering::Release);
                self.configured = false;
                cx.layer.set_exclusive_zone(0);
                cx.surface().attach(None, 0, 0);
                cx.surface().commit();
                self.data.cancel();
                cx.primary.set(false);
                cx.drag_active.set(false);
                cx.press.borrow_mut().take();
                cx.pending_drag.borrow_mut().take();
                if let Some(h) = &self.handler {
                    h.on_event(Event::Window(WindowEvent::Unfocused));
                }
            }
        }
    }
    fn tick(&mut self) -> linux::Result<()> {
        let visible = self.shared.desired_visible.load(Ordering::Acquire);
        if visible != self.shared.visible.load(Ordering::Acquire) {
            self.command(if visible { Command::Show } else { Command::Hide });
        }
        self.start_pending_drag();
        self.data.progress(self.handler.as_deref());
        let (Some(cx), Some(handler)) = (&self.context, &self.handler) else { return Ok(()) };
        if !self.shared.visible.load(Ordering::Acquire) || !self.configured || !self.ready {
            return Ok(());
        }
        let now = Instant::now();
        let requested = self.shared.redraw.swap(false, Ordering::AcqRel);
        let due = match handler.frame_demand() {
            FrameDemand::Idle => false,
            FrameDemand::At(at) => now >= at,
            _ => true,
        };
        if requested || due {
            self.ready = false;
            cx.surface().frame(&cx.qh, FrameCallbackData(cx.surface().clone()));
            handler.on_frame().map_err(failure)?;
            cx.surface().commit();
            cx.conn.flush().map_err(failure)?;
        }
        Ok(())
    }
    fn deadline(&self) -> Option<Duration> {
        if self.data.has_transfers() {
            return Some(Duration::from_millis(2));
        }
        if !self.ready || !self.configured || !self.shared.visible.load(Ordering::Acquire) {
            return None;
        }
        let demand = self.handler.as_ref().map(|h| h.frame_demand()).unwrap_or(FrameDemand::Idle);
        match demand {
            FrameDemand::Idle => None,
            FrameDemand::At(at) => Some(at.saturating_duration_since(Instant::now())),
            _ => Some(Duration::ZERO),
        }
    }
    fn event(&self, event: Event) -> EventStatus {
        if !self.shared.open.load(Ordering::Acquire) {
            return EventStatus::Ignored;
        }
        self.shared.redraw.store(true, Ordering::Release);
        self.handler.as_ref().map_or(EventStatus::Ignored, |h| h.on_event(event))
    }
    fn resized(&mut self) {
        if let (Some(cx), Some(h)) = (&self.context, &self.handler) {
            let _ = h.resized(cx.size());
        }
        self.shared.redraw.store(true, Ordering::Release);
        self.ready = true;
    }
    fn close(&mut self) {
        self.data.cancel();
        if let Some(handler) = self.handler.take() {
            handler.on_event(Event::Window(WindowEvent::WillClose));
            drop(handler);
        }
        if let Some(cx) = self.context.take() {
            cx.close();
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.close();
    }
}

impl LayerShellHandler for Runtime {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }
    fn configure(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface,
        configure: LayerSurfaceConfigure, _: u32,
    ) {
        if let Some(cx) = &self.context {
            cx.set_size(configure.new_size);
        }
        self.configured = true;
        self.resized();
    }
}
impl CompositorHandler for Runtime {
    fn scale_factor_changed(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, factor: i32,
    ) {
        if let Some(cx) = &self.context {
            cx.set_integer_scale(factor);
        }
        self.resized();
    }
    fn transform_changed(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.ready = true;
    }
    fn surface_enter(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}
impl OutputHandler for Runtime {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}
impl ProvidesRegistryState for Runtime {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers!(OutputState, SeatState);
}
smithay_client_toolkit::delegate_registry!(Runtime);
use wayland_client::Proxy;

smithay_client_toolkit::delegate_dispatch2!(Runtime);

#[cfg(test)]
mod tests {
    use super::*;

    struct Stalled {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        retired: mpsc::Sender<()>,
        once: Cell<bool>,
    }
    impl WindowHandler for Stalled {
        fn resized(&self, _: WindowSize) -> std::result::Result<(), HandlerError> {
            Ok(())
        }
        fn on_frame(&self) -> std::result::Result<(), HandlerError> {
            if !self.once.replace(true) {
                let _ = self.entered.send(());
                let _ = self.release.recv_timeout(Duration::from_secs(5));
            }
            Ok(())
        }
        fn on_event(&self, _: Event) -> EventStatus {
            EventStatus::Ignored
        }
    }
    impl Drop for Stalled {
        fn drop(&mut self) {
            let _ = self.retired.send(());
        }
    }

    #[test]
    #[ignore = "requires a running layer-shell compositor"]
    fn a_stalled_dock_does_not_block_host_close() {
        let (entered, started) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let (retired, finished) = mpsc::channel();
        let window = Window::create(
            WindowSettings::new().with_dock(DockOptions::new(DockEdge::Top, 120)),
            move |_| Ok(Stalled { entered, release: released, retired, once: Cell::new(false) }),
        )
        .expect("native dock");
        window.show().expect("show");
        started.recv_timeout(Duration::from_secs(3)).expect("renderer entered");
        let start = Instant::now();
        window.close_bounded(Duration::from_millis(30));
        assert!(start.elapsed() < Duration::from_millis(150), "host close must stay bounded");
        release.send(()).expect("stalled callback still owns its resources");
        finished
            .recv_timeout(Duration::from_secs(3))
            .expect("owner retires after callback returns");
    }
}
