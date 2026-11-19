//! Native Wayland surfaces on a toolkit-owned queue: a dock (wlr layer shell) or
//! a floating window (xdg toplevel, with client-side decorations when the
//! compositor draws none) on their own owner thread, or a subsurface of a
//! host's surface, or a surface a host's nested compositor embeds, that the
//! host's run loop drives ([`Embedded`]).
mod context;
mod drag;
mod embedding;
mod input;
mod outputs;
pub(super) use outputs::names as output_names;
mod text_input;
mod toplevel;

use super::{linux, x11::PlatformError};
use crate::{dpi::*, window::WindowInitializer, *};
use calloop::{channel, EventLoop, LoopSignal};
use calloop_wayland_source::WaylandSource;
use context::Role;
pub use context::{Context, PlatformHandle};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    output::{OutputHandler, OutputState},
    reexports::csd_frame::DecorationsFrame,
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::SeatState,
    shell::{
        wlr_layer::{Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        xdg::{window::WindowDecorations, XdgShell},
        WaylandSurface,
    },
    subcompositor::SubcompositorState,
};
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    os::fd::{AsFd, BorrowedFd, OwnedFd},
    ptr::NonNull,
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
    /// A toplevel that is not resizable keeps min = max = its size.
    fixed: AtomicBool,
    size: AtomicU64,
    scale: AtomicU64,
    error: Mutex<Option<String>>,
    surface: Mutex<Option<(Role, Connection)>>,
}
impl Shared {
    fn new(settings: &WindowSettings, visible: bool) -> Arc<Self> {
        Arc::new(Self {
            open: AtomicBool::new(true),
            visible: AtomicBool::new(visible),
            desired_visible: AtomicBool::new(visible),
            redraw: AtomicBool::new(true),
            fixed: AtomicBool::new(!settings.resizable),
            size: AtomicU64::new((800_u64 << 32) | 160),
            scale: AtomicU64::new(1_f64.to_bits()),
            error: Mutex::new(None),
            surface: Mutex::new(None),
        })
    }
    /// Wakes the runtime's loop for a frame; holds neither the window nor the loop alive.
    fn frame_requester(self: &Arc<Self>, signal: LoopSignal) -> FrameRequester {
        let shared = Arc::downgrade(self);
        FrameRequester::new(move || {
            if let Some(s) = shared.upgrade() {
                s.redraw.store(true, Ordering::Release);
                signal.wakeup();
            }
        })
    }
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
    Title(String),
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
        let WindowInitializer { settings, builder, host } = init;
        drop(host);
        Self::spawn(settings, builder)
    }
    /// Opens the surface on its own owner thread; no host callbacks are attached.
    pub(crate) fn spawn(
        settings: WindowSettings, builder: crate::handler::WindowHandlerBuilder,
    ) -> linux::Result<Self> {
        if settings.parent.is_some() || settings.wait_for_parent {
            return Err(failure("a native Wayland surface cannot be a host-owned X11 child"));
        }
        if !crate::pin_current_image_for_detached_work() {
            return Err(failure("could not pin the window's executable image"));
        }
        let shared = Shared::new(&settings, false);
        let (send, recv) = mpsc::sync_channel(1);
        let (sender, receiver) = channel::sync_channel(8);
        let state = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("mui-wayland".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut event_loop = EventLoop::<Runtime>::try_new().map_err(failure)?;
                    let signal = event_loop.get_signal();
                    let mut runtime =
                        Runtime::new(settings, builder, Arc::clone(&state), &mut event_loop, None)?;
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
                    Err(_) => Some("panic in native Wayland runtime".into()),
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
                    _ => "Wayland window creation failed".into(),
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
    pub fn set_title(&self, title: &str) -> linux::Result<()> {
        self.command(Command::Title(title.into()))
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
        !self.shared.fixed.load(Ordering::Acquire)
    }
    pub fn min_size(&self) -> Option<Size> {
        None
    }
    pub fn max_size(&self) -> Option<Size> {
        None
    }
    pub fn handle_main_thread_callback(&self) {}
    pub fn set_parent(&self, _: super::x11::ParentWindowHandle) -> linux::Result<()> {
        Err(failure("a native Wayland surface cannot be embedded in X11"))
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
        self.shared.frame_requester(self.signal.clone())
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
        if let Some((role, conn)) =
            self.shared.surface.lock().unwrap_or_else(|e| e.into_inner()).take()
        {
            role.unmap();
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

/// A subsurface of a host's `wl_surface` on the host's `wl_display`, with no
/// thread: the host watches [`Self::fd`] and calls [`Self::dispatch`] on the
/// thread that opened it. `!Send`, because the runtime is single-threaded.
pub struct Embedded {
    runtime: Runtime,
    event_loop: EventLoop<'static, Runtime>,
}
impl Embedded {
    /// Opens a visible subsurface of `surface` at its origin.
    ///
    /// # Safety
    /// `display` is a live `wl_display*` and `surface` a live `wl_surface*`
    /// proxy on it. Both outlive the returned value, which is used only on
    /// the thread that called this.
    pub unsafe fn open(
        settings: WindowSettings, builder: crate::handler::WindowHandlerBuilder,
        display: NonNull<c_void>, surface: NonNull<c_void>,
    ) -> linux::Result<Self> {
        let mut event_loop = EventLoop::try_new().map_err(failure)?;
        let shared = Shared::new(&settings, true);
        let host = Host::Foreign { display, surface };
        let runtime = Runtime::new(settings, builder, shared, &mut event_loop, Some(host))?;
        Ok(Self { runtime, event_loop })
    }
    /// Opens a surface that a host's nested compositor embeds in its window
    /// (CLAP `wayland-surface-embedding-v1`): our own connection on `socket`,
    /// which must lead to that compositor, placed by the host's `token`.
    pub fn open_nested(
        settings: WindowSettings, builder: crate::handler::WindowHandlerBuilder, socket: OwnedFd,
        token: &str,
    ) -> linux::Result<Self> {
        if token.contains('\0') {
            return Err(failure("embedding token contains a NUL"));
        }
        let mut event_loop = EventLoop::try_new().map_err(failure)?;
        let shared = Shared::new(&settings, true);
        let host = Host::Nested { socket, token: token.to_owned() };
        let runtime = Runtime::new(settings, builder, shared, &mut event_loop, Some(host))?;
        Ok(Self { runtime, event_loop })
    }
    /// Readable when [`Self::dispatch`] has work: Wayland events, a frame request.
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.event_loop.as_fd()
    }
    /// Handles pending events and draws a due frame, without blocking.
    pub fn dispatch(&mut self) -> linux::Result<()> {
        if !self.is_open() {
            return Ok(());
        }
        self.event_loop.dispatch(Some(Duration::ZERO), &mut self.runtime).map_err(failure)?;
        self.runtime.tick()
    }
    pub fn is_open(&self) -> bool {
        !self.runtime.exit && self.runtime.shared.open.load(Ordering::Acquire)
    }
    pub fn size(&self) -> WindowSize {
        self.runtime.shared.size()
    }
    pub fn frame_requester(&self) -> FrameRequester {
        self.runtime.shared.frame_requester(self.event_loop.get_signal())
    }
}

/// What an [`Embedded`] window attaches to.
enum Host {
    /// A host's surface on its own display; see [`Embedded::open`].
    Foreign { display: NonNull<c_void>, surface: NonNull<c_void> },
    /// A host's nested compositor; see [`Embedded::open_nested`].
    Nested { socket: OwnedFd, token: String },
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
    /// Toplevel only: the fallback frame's decorations are subsurfaces.
    subcompositor: Option<Arc<SubcompositorState>>,
    _xdg: Option<XdgShell>,
    /// Report a compositor close request as `WillClose` and keep the window.
    defer_close: bool,
    modifiers: keyboard_types::Modifiers,
    ready: bool,
    /// A frame callback has fired since the last map: the compositor shows us.
    /// Before that a callback may never come (nothing attached yet), so frames
    /// are not gated on it.
    mapped: bool,
    /// When the last frame was committed while waiting for its callback.
    committed: Instant,
    configured: bool,
    exit: bool,
}
impl Runtime {
    fn new(
        settings: WindowSettings, builder: crate::handler::WindowHandlerBuilder,
        shared: Arc<Shared>, event_loop: &mut EventLoop<Self>, host: Option<Host>,
    ) -> linux::Result<Self> {
        let conn = match &host {
            // SAFETY: `Embedded::open`'s contract: a live display that outlives us.
            // A foreign backend never disconnects it.
            Some(Host::Foreign { display, .. }) => Connection::from_backend(unsafe {
                wayland_client::backend::Backend::from_foreign_display(display.as_ptr().cast())
            }),
            Some(Host::Nested { socket, .. }) => {
                let socket =
                    std::os::unix::net::UnixStream::from(socket.try_clone().map_err(failure)?);
                Connection::from_socket(socket).map_err(failure)?
            }
            None => Connection::connect_to_env().map_err(failure)?,
        };
        let (globals, mut queue) = registry_queue_init(&conn).map_err(failure)?;
        let qh = queue.handle();
        let compositor = CompositorState::bind(&globals, &qh).map_err(failure)?;
        let mut runtime = Self {
            registry: RegistryState::new(&globals),
            outputs: OutputState::new(&globals, &qh),
            seats: SeatState::new(&globals, &qh),
            data: drag::Data::new(&globals, &qh, compositor.clone())?,
            shared: Arc::clone(&shared),
            context: None,
            handler: None,
            keyboard: None,
            pointer: None,
            subcompositor: None,
            _xdg: None,
            defer_close: settings.defer_user_close,
            modifiers: keyboard_types::Modifiers::empty(),
            ready: true,
            mapped: false,
            committed: Instant::now(),
            configured: false,
            exit: false,
        };
        queue.roundtrip(&mut runtime).map_err(failure)?;
        let logical = settings.size.to_logical::<f64>(1.0);
        let logical =
            ((logical.width.round() as u32).max(1), (logical.height.round() as u32).max(1));
        let (role, options) = match (host, settings.dock.clone()) {
            (Some(Host::Foreign { surface, .. }), _) => {
                (runtime.subsurface(&globals, &qh, &compositor, &conn, surface)?, None)
            }
            (Some(Host::Nested { token, .. }), _) => {
                let embedding: embedding::protocol::clap_surface_embedding_v1::ClapSurfaceEmbeddingV1 =
                    globals.bind(&qh, 1..=1, context::Noop).map_err(failure)?;
                let surface = compositor.create_surface(&qh);
                let embedded =
                    embedding.get_embedded_surface(&surface, token, &qh, embedding::Embedding);
                embedding.destroy();
                (Role::Nested(embedded, surface), None)
            }
            (None, Some(options)) => {
                let surface = compositor.create_surface(&qh);
                let layer = runtime.dock(&globals, &qh, surface, &options)?;
                (Role::Layer(layer), Some(options))
            }
            (None, None) => {
                let surface = compositor.create_surface(&qh);
                let window =
                    runtime.toplevel(&globals, &qh, &compositor, surface, &settings, logical)?;
                (Role::Toplevel(window), None)
            }
        };
        let context = Rc::new(Context::new(
            conn.clone(),
            role,
            options,
            logical,
            qh.clone(),
            shared,
            event_loop.get_signal(),
            &globals,
        )?);
        for seat in runtime.seats.seats() {
            context.text.borrow_mut().seat(&seat, &qh);
        }
        *runtime.shared.surface.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((context.role.clone(), context.conn.clone()));
        runtime.context = Some(Rc::clone(&context));
        runtime.refresh_outputs();
        // Configure without a buffer, and acknowledge before any presenter can attach.
        // An embedded surface has no configure: the host sized and maps its parent.
        context.configure();
        runtime.configured = matches!(context.role, Role::Subsurface(..) | Role::Nested(..));
        if runtime.configured {
            context.set_size(logical);
        }
        let start = Instant::now();
        while !runtime.configured
            && runtime.shared.open.load(Ordering::Acquire)
            && start.elapsed() < Duration::from_secs(2)
        {
            queue.roundtrip(&mut runtime).map_err(failure)?;
            if !runtime.configured {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        if !runtime.configured || !runtime.shared.open.load(Ordering::Acquire) {
            return Err(failure("compositor did not configure the dock"));
        }
        let handler = builder
            .build(crate::WindowContext::new(linux::WindowContext::Layer(context)))
            .map_err(failure)?;
        runtime.handler = Some(handler);
        WaylandSource::new(conn, queue).insert(event_loop.handle()).map_err(failure)?;
        Ok(runtime)
    }
    /// A desynchronized subsurface of the host's `parent`, at its origin.
    fn subsurface(
        &mut self, globals: &wayland_client::globals::GlobalList, qh: &QueueHandle<Self>,
        compositor: &CompositorState, conn: &Connection, parent: NonNull<c_void>,
    ) -> linux::Result<Role> {
        use wayland_client::{backend::ObjectId, Proxy};
        let subcompositor =
            SubcompositorState::bind(compositor.wl_compositor().clone(), globals, qh)
                .map_err(failure)?;
        // SAFETY: `Embedded::open`'s contract: a live `wl_surface` proxy on this display.
        // The id checks the interface and keeps no claim on the host's object.
        let id = unsafe {
            ObjectId::from_ptr(wl_surface::WlSurface::interface(), parent.as_ptr().cast())
        }
        .map_err(failure)?;
        let parent = wl_surface::WlSurface::from_id(conn, id).map_err(failure)?;
        let (subsurface, surface) = subcompositor.create_subsurface(parent, qh);
        // Frames commit on our own clock, not the host's parent commits.
        subsurface.set_desync();
        self.subcompositor = Some(Arc::new(subcompositor));
        Ok(Role::Subsurface(subsurface, surface))
    }
    fn dock(
        &self, globals: &wayland_client::globals::GlobalList, qh: &QueueHandle<Self>,
        surface: wl_surface::WlSurface, options: &DockOptions,
    ) -> linux::Result<LayerSurface> {
        let layers = LayerShell::bind(globals, qh).map_err(failure)?;
        let output = self.outputs.outputs().find(|output| {
            options.output.as_ref().is_none_or(|name| {
                self.outputs.info(output).is_some_and(|i| i.name.as_ref() == Some(name))
            })
        });
        if options.output.is_some() && output.is_none() {
            return Err(failure("selected dock output is unavailable"));
        }
        let layer =
            layers.create_layer_surface(qh, surface, Layer::Top, Some("mui-dock"), output.as_ref());
        if matches!(layer.kind(), smithay_client_toolkit::shell::wlr_layer::SurfaceKind::Wlr(w) if w.version() < 4)
        {
            return Err(failure("layer-shell version 4 is required for on-demand keyboard focus"));
        }
        Ok(layer)
    }
    fn command(&mut self, command: Command) {
        let Some(cx) = &self.context else { return };
        match command {
            Command::Close => self.exit = true,
            Command::Title(title) => {
                if let Role::Toplevel(window) = &cx.role {
                    window.set_title(title);
                }
            }
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
                self.mapped = false;
                cx.role.unmap();
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
        if self.context.as_ref().is_some_and(|cx| cx.resize_pending.replace(false)) {
            self.resized();
        }
        let (Some(cx), Some(handler)) = (&self.context, &self.handler) else { return Ok(()) };
        if !self.ready && self.explicit_overdue() {
            self.ready = true;
        }
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
            self.ready = !self.mapped;
            self.committed = now;
            cx.surface().frame(&cx.qh, FrameCallbackData(cx.surface().clone()));
            handler.on_frame().map_err(failure)?;
            if let Some(frame) = cx.frame.borrow_mut().as_mut() {
                if frame.is_dirty() && !frame.is_hidden() {
                    frame.draw();
                }
            }
            cx.surface().commit();
            cx.conn.flush().map_err(failure)?;
        }
        Ok(())
    }
    /// A commit that paints nothing earns no frame callback on some compositors
    /// (wlroots, Hyprland). An explicit request (event, resize, redraw) waits at
    /// most this long for one; continuous animation stays callback-paced, so a
    /// hidden window stays throttled.
    const CALLBACK_GRACE: Duration = Duration::from_millis(50);
    fn explicit_overdue(&self) -> bool {
        self.shared.redraw.load(Ordering::Acquire)
            && self.committed.elapsed() >= Self::CALLBACK_GRACE
    }
    fn deadline(&self) -> Option<Duration> {
        if self.data.has_transfers() {
            return Some(Duration::from_millis(2));
        }
        if !self.ready && self.shared.redraw.load(Ordering::Acquire) {
            return Some(Self::CALLBACK_GRACE.saturating_sub(self.committed.elapsed()));
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
        if let Some(cx) = &self.context {
            if let Some(frame) = cx.frame.borrow_mut().as_mut() {
                frame.set_scaling_factor(cx.scale_factor());
            }
        }
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
        self.mapped = true;
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
impl Runtime {
    fn refresh_outputs(&self) {
        if let Some(cx) = &self.context {
            *cx.outputs.borrow_mut() = self
                .outputs
                .outputs()
                .filter_map(|output| self.outputs.info(&output).and_then(|info| info.name))
                .collect();
        }
    }
}
impl OutputHandler for Runtime {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.refresh_outputs();
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.refresh_outputs();
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.refresh_outputs();
    }
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
    struct Counting(Arc<AtomicU64>);
    impl WindowHandler for Counting {
        fn resized(&self, _: WindowSize) -> std::result::Result<(), HandlerError> {
            Ok(())
        }
        fn on_frame(&self) -> std::result::Result<(), HandlerError> {
            self.0.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
        fn on_event(&self, _: Event) -> EventStatus {
            EventStatus::Ignored
        }
    }

    fn readable(fd: BorrowedFd<'_>, timeout: Duration) -> bool {
        use std::os::fd::AsRawFd;
        let mut poll = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: one valid pollfd for the duration of the call.
        unsafe { libc::poll(&mut poll, 1, timeout.as_millis() as i32) == 1 }
    }

    /// A floating toplevel stands in for the VST3 host: its surface and display
    /// are what `IWaylandFrame` would hand the plugin.
    #[test]
    #[ignore = "requires a running Wayland compositor"]
    fn an_embedded_subsurface_runs_on_the_host_loop() {
        use raw_window_handle::{
            HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
        };
        let (send, handle) = mpsc::channel();
        let host = crate::ThreadedWindow::create(
            WindowSettings::new().with_size(LogicalSize::new(300., 200.)).with_wayland(true),
            move |cx| {
                let _ = send.send(cx.platform_handle());
                Ok(Counting(Arc::default()))
            },
        )
        .expect("host toplevel");
        host.show().expect("show host");
        let handle = handle.recv_timeout(Duration::from_secs(3)).expect("host surface");
        let RawDisplayHandle::Wayland(display) = handle.display_handle().expect("display").as_raw()
        else {
            panic!("host is not on Wayland")
        };
        let RawWindowHandle::Wayland(surface) = handle.window_handle().expect("surface").as_raw()
        else {
            panic!("host is not on Wayland")
        };
        let frames = Arc::new(AtomicU64::new(0));
        let count = Arc::clone(&frames);
        // SAFETY: `host` keeps its connection and surface alive past `embedded`.
        let mut embedded = unsafe {
            crate::EmbeddedWaylandWindow::open(
                WindowSettings::new().with_size(LogicalSize::new(120., 80.)),
                display.display.cast(),
                surface.surface.cast(),
                move |_| Ok(Counting(count)),
            )
        }
        .expect("embedded subsurface");
        assert_eq!(embedded.size().logical.width.round(), 120.);
        embedded.dispatch().expect("first dispatch");
        assert_eq!(frames.load(Ordering::Acquire), 1, "the first dispatch draws");

        embedded.frame_requester().request_frame();
        assert!(readable(embedded.fd(), Duration::from_secs(1)), "a frame request wakes the fd");
        // The host's timer covers a frame whose callback a bufferless commit never earns.
        let deadline = Instant::now() + Duration::from_secs(2);
        while frames.load(Ordering::Acquire) < 2 {
            assert!(Instant::now() < deadline, "the requested frame never drew");
            embedded.dispatch().expect("dispatch");
            std::thread::sleep(Duration::from_millis(16));
        }
        assert!(embedded.is_open());
        drop(embedded);
        assert!(host.is_open(), "closing the embedded editor leaves the host alone");
        host.close();
    }
}
