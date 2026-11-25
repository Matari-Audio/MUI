use super::drag_n_drop::DragNDropState;
use super::keyboard::{convert_key_press_event, convert_key_release_event, key_mods};
use super::*;
use std::result::Result;

use crate::dpi::{PhysicalPosition, PhysicalSize};
use crate::host::HostMainThreadCaller;
use crate::platform::frame_rate::frame_interval;
use crate::platform::x11::error::FatalError;
use crate::platform::x11::window_thread::{HostCallback, WindowThreadMessage, WindowThreadRequest};
use crate::warn;
use crate::wrappers::xkbcommon::XkbcommonState;
use crate::{
    Event, EventStatus, FrameDemand, MouseButton, MouseEvent, ScrollDelta, WindowEvent,
    WindowHandler, WindowSize,
};
use calloop::generic::Generic;
use calloop::timer::{TimeoutAction, Timer};
use calloop::{Interest, LoopHandle, LoopSignal, Mode, PostAction, RegistrationToken};
use matari_audio_drag_and_drop::{X11DropRouter, X11PointerEvent, X11SessionStatus};
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::errors::ConnectionError;
use x11rb::protocol::randr::{ConnectionExt as _, ModeFlag, ModeInfo};
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask, InputFocus, KeyButMask,
    KeyPressEvent, KeyReleaseEvent, NotifyDetail,
};
use x11rb::protocol::Event as XEvent;
use x11rb::CURRENT_TIME;

pub struct MainThreadCaller {
    sender: mpsc::Sender<HostCallback>,
    caller: Box<dyn HostMainThreadCaller>,
}

impl MainThreadCaller {
    pub(crate) fn new(
        main_thread: Option<Box<dyn HostMainThreadCaller>>,
    ) -> (Option<Self>, Option<Receiver<HostCallback>>) {
        let Some(main_thread) = main_thread else {
            return (None, None);
        };

        let (sender, receiver) = mpsc::channel();
        (Some(Self { sender, caller: main_thread }), Some(receiver))
    }

    pub fn send(&mut self, msg: HostCallback) -> Result<(), FatalError> {
        self.sender.send(msg).map_err(|_| FatalError::SendMainThread)?;
        self.caller.call_main_thread();
        Ok(())
    }
}

pub(crate) struct EventLoop {
    handler: Box<dyn WindowHandler>,
    window: Rc<WindowInner>,

    new_size: Option<PhysicalSize<u16>>,
    new_parent_size: Option<PhysicalSize<u16>>,
    exposed: bool,
    /// A `request_frame` from another thread: served on the next paced frame,
    /// never at once, so a renderer that asks again per frame stays at refresh.
    frame_requested: Option<Instant>,

    /// `on_frame` pacing: the refresh period of the monitor under the window (MOOSE).
    frame_interval: Duration,
    /// Set by `ConfigureNotify`: the window may have moved to another monitor.
    refresh_rate_stale: bool,
    refresh_rate_queried_at: Instant,

    loop_signal: LoopSignal,
    loop_handle: LoopHandle<'static, Self>,
    frame_timer: Option<RegistrationToken>,
    drag_timer: Option<RegistrationToken>,
    frame_deadline: Option<Instant>,
    last_frame: Instant,

    drag_n_drop: DragNDropState,
    /// Hyprland's XWayland bridge delivers drops to the host's toplevel;
    /// this routes them down to the editor window.
    drop_router: Option<X11DropRouter>,
    xkb_state: Option<XkbcommonState>,

    run_error: Option<PlatformError>,

    main_thread: Option<MainThreadCaller>,

    /// Keys currently held down in this window, by X keycode. Used to flag
    /// auto-repeat key-downs (MOOSE X01).
    pressed_keys: [bool; 256],
    /// Focus ownership for click-to-focus; pointer-derived focus still drives
    /// the existing focus notifications and IME state.
    owns_focus: bool,
    /// A click asked for the keyboard and its FocusIn has not come yet. The
    /// pointer-derived focus it replaces ends first, with a FocusOut that
    /// must not cancel the very click that asked.
    taking_focus: bool,
    /// Mouse buttons held in this window, as a bitmask of X button numbers.
    /// X's implicit grab delivers their releases here whatever the focus does.
    buttons_held: u32,
    /// A focus loss that came while a button was held: delivered after the
    /// last release. A window manager that keeps the X focus on its toplevel
    /// (GNOME) takes it back from every click's `take_focus`; delivered at
    /// once, `Unfocused` cancelled that click.
    deferred_unfocus: bool,
    /// Where the pointer is inside the window, with the state of its last
    /// event: re-sent after `Unfocused`, which forgets it.
    pointer_at: Option<(i16, i16, KeyButMask)>,
    /// Key-downs the handler ignored and that were forwarded to the embed
    /// parent, so the matching key-up can be synthesized on focus loss/close.
    forwarded_keys: [Option<KeyPressEvent>; 256],
    /// The XEmbed embedder, once it said so: it owns the X focus.
    embedder: Option<u32>,
}

impl EventLoop {
    pub fn new(
        window: Rc<WindowInner>, handler: Box<dyn WindowHandler>,
        request_receiver: calloop::channel::Channel<WindowThreadMessage>,
        main_thread: Option<MainThreadCaller>, inner: &mut calloop::EventLoop<'static, Self>,
    ) -> Result<Self, PlatformError> {
        let loop_handle = inner.handle();

        let frame_interval = frame_interval(query_refresh_hz(&window));

        loop_handle
            .insert_source(
                Generic::new_with_error(
                    Arc::clone(&window.connection.conn),
                    Interest::READ,
                    // Level-triggered: synchronous XCB replies can pull
                    // unrelated events into XCB's userspace queue while
                    // draining the kernel socket, which would strand events
                    // under EPOLLET once settled windows run at 0 Hz idle.
                    Mode::Level,
                ),
                |_, _, e| e.handle_connection_event_ready(),
            )
            .map_err(|e| e.error)?;

        loop_handle
            .insert_source(request_receiver, |e, _, l| l.handle_main_thread_request(e))
            .map_err(|e| e.error)?;

        let drop_router =
            X11DropRouter::install(window.connection.conn.xcb_connection(), window.raw_id())
                .unwrap_or_else(|error| {
                    warn!("Could not install the X11 drop router: {}", error);
                    None
                });

        Ok(Self {
            loop_signal: inner.get_signal(),
            loop_handle,
            frame_timer: None,
            frame_deadline: None,
            drag_timer: None,
            last_frame: Instant::now(),
            handler,
            new_size: None,
            new_parent_size: None,
            exposed: false,
            frame_requested: None,
            frame_interval,
            refresh_rate_stale: false,
            refresh_rate_queried_at: Instant::now(),
            drag_n_drop: DragNDropState::NoCurrentSession,
            drop_router,
            xkb_state: XkbcommonState::new(&window.connection),
            run_error: None,
            main_thread,
            pressed_keys: [false; 256],
            owns_focus: false,
            taking_focus: false,
            buttons_held: 0,
            deferred_unfocus: false,
            pointer_at: None,
            forwarded_keys: [None; 256],
            embedder: None,

            window,
        })
    }

    #[inline]
    fn drain_xcb_events(&mut self) -> Result<bool, ConnectionError> {
        if self.window.xcb_window.is_destroyed()
            || self.window.main_thread_shared.callbacks_revoked()
        {
            return Ok(false);
        }
        let mut event_received = false;
        // Core X11 auto-repeat arrives as a KeyRelease/KeyPress pair with the
        // same keycode, timestamp and window. Swallow the synthetic release so
        // handlers see one repeated key-down instead of release + fresh press.
        let mut pending_release: Option<KeyReleaseEvent> = None;
        while let Some(event) = self.window.connection.conn.poll_for_event()? {
            if self.window.xcb_window.is_destroyed()
                || self.window.main_thread_shared.callbacks_revoked()
            {
                break;
            }
            event_received = true;
            match event {
                XEvent::KeyRelease(release) => {
                    if let Some(previous) = pending_release.replace(release) {
                        self.handle_xcb_event(XEvent::KeyRelease(previous))?;
                    }
                }
                XEvent::KeyPress(press) => {
                    let release = pending_release.take();
                    if let Some(release) = release.filter(|r| !is_auto_repeat_pair(r, &press)) {
                        self.handle_xcb_event(XEvent::KeyRelease(release))?;
                    }
                    self.handle_xcb_event(XEvent::KeyPress(press))?;
                }
                event => {
                    if let Some(release) = pending_release.take() {
                        self.handle_xcb_event(XEvent::KeyRelease(release))?;
                    }
                    self.handle_xcb_event(event)?;
                }
            }
        }
        if let Some(release) =
            pending_release.filter(|_| !self.window.main_thread_shared.callbacks_revoked())
        {
            self.handle_xcb_event(XEvent::KeyRelease(release))?;
        }

        Ok(event_received)
    }

    fn schedule_frame(&mut self) -> Result<(), calloop::Error> {
        let visible = !self.window.xcb_window.is_destroyed()
            && !self.window.main_thread_shared.is_stop_host_requested()
            && !self.window.main_thread_shared.callbacks_revoked()
            && self.window.visibility_state.own_window_is_viewable();
        let now = Instant::now();
        if visible
            && self.refresh_rate_stale
            && self.refresh_rate_queried_at.elapsed() >= Duration::from_millis(250)
        {
            self.refresh_rate_stale = false;
            self.refresh_rate_queried_at = now;
            self.frame_interval = frame_interval(query_refresh_hz(&self.window));
        }
        let deadline = if visible {
            let demand = self.handler.frame_demand();
            let demand = self.frame_requested.map_or(demand, |at| soonest(demand, at));
            frame_deadline(demand, now, self.last_frame, self.frame_interval)
        } else {
            None
        };
        if deadline == self.frame_deadline {
            return Ok(());
        }
        if let Some(timer) = self.frame_timer.take() {
            self.loop_handle.remove(timer);
        }
        self.frame_deadline = deadline;
        if let Some(deadline) = deadline {
            self.frame_timer = Some(
                self.loop_handle
                    .insert_source(Timer::from_deadline(deadline), |_, _, e| {
                        e.frame_timer = None;
                        e.frame_deadline = None;
                        e.exposed = true;
                        TimeoutAction::Drop
                    })
                    .map_err(|e| e.error)?,
            );
        }
        Ok(())
    }

    fn handle_redraw(&mut self) {
        if self.window.xcb_window.is_destroyed()
            || self.window.main_thread_shared.is_stop_host_requested()
            || self.window.main_thread_shared.callbacks_revoked()
        {
            return;
        }
        if !self.exposed {
            return;
        }
        self.exposed = false;

        if !self.window.visibility_state.own_window_is_viewable() {
            return;
        }
        self.frame_requested = None;

        self.last_frame = Instant::now();
        if let Err(e) = self.handler.on_frame() {
            self.trigger_fatal_error(e.into());
            return;
        }

        self.drain_ime();
        // Any socket error will be handled in the next poll
        let _ = self.window.connection.conn.flush();
    }

    fn handle_coalesced_resize_events(&mut self) -> Result<(), FatalError> {
        if self.window.xcb_window.is_destroyed()
            || self.window.main_thread_shared.is_stop_host_requested()
            || self.window.main_thread_shared.callbacks_revoked()
        {
            return Ok(());
        }
        let mut comes_from_parent = false;
        if let Some(new_parent_size) = self.new_parent_size.take() {
            if new_parent_size != self.window.get_size() {
                // The parent was resized, which means we should resize ourselves too.
                if let Err(e) = self.window.xcb_window.resize(new_parent_size.cast()) {
                    crate::warn!("Failed to resize window: {}", e);
                } else {
                    // Makes the rest of this function run on the new parent size immediately (without waiting for a ConfigureNotify round-trip)
                    // Also overrides any new sizes we may have received this event loop iteration,it would probably be invalidated anyway
                    self.new_size = Some(new_parent_size);
                    comes_from_parent = true;
                }
            }
        }

        let Some(new_size) = self.new_size.take() else {
            return Ok(());
        };
        let previous = self.window.store_size(new_size);

        if previous == new_size {
            return Ok(());
        };

        let scale_factor = self.window.scaling_factor.get();
        let new_size = WindowSize::from_physical(new_size.cast(), scale_factor);

        if let Err(e) = self.handler.resized(new_size) {
            warn!("Window Handler failed to resize: {}", e);
            self.window.store_size(previous);
            self.window.xcb_window.resize(previous.cast())?.check_warn();
            return Ok(());
        }

        // Host requests use resize_immediately, which stops the previous == new_size condition
        // So if we're here, it's guaranteed not to be from a host request

        if !comes_from_parent {
            if let Some(host) = self.main_thread.as_mut() {
                host.send(HostCallback::Resized {
                    new_size,
                    previous: WindowSize::from_physical(previous.cast(), scale_factor),
                })?;
            }
        }

        // Immediately schedule a redraw, do not wait for an "expose" event
        self.exposed = true;

        Ok(())
    }

    fn handle_main_thread_request(&mut self, event: calloop::channel::Event<WindowThreadMessage>) {
        match event {
            calloop::channel::Event::Closed => {
                // Closed channel means the sender, i.e. the Window Handle has been dropped.
                // It should already stop this event loop on drop, but we'll take the hint.
                self.stop_now();
            }
            calloop::channel::Event::Msg(message) => {
                let result = if self.window.main_thread_shared.callbacks_revoked()
                    || self.window.main_thread_shared.is_stop_host_requested()
                {
                    Err("X11 window is closing".into())
                } else {
                    self.handle_request(message.request).map_err(|e| e.to_string())
                };
                // Late replies belong to this RPC alone, never the next one.
                let _ = message.response.send(result);
            }
        }
    }

    fn stop_now(&self) {
        self.loop_signal.stop();
        self.loop_signal.wakeup();
    }

    fn trigger_fatal_error(&mut self, error: PlatformError) {
        if self.run_error.is_none() {
            self.run_error = Some(error);
        }
        self.stop_now();
    }

    fn handle_request(&mut self, req: WindowThreadRequest) -> Result<(), PlatformError> {
        self.exposed = true;
        match req {
            WindowThreadRequest::Resize(new_size) => {
                if let Some(dock) = self.window.dock.borrow_mut().as_mut() {
                    let logical: crate::dpi::LogicalSize<f64> =
                        new_size.to_logical(self.window.scale_factor());
                    return dock.resize(logical);
                }
                let scale_factor = self.window.scaling_factor.get();
                let new_size = new_size.to_physical(scale_factor);

                self.window.resize_immediately(new_size, &*self.handler)?;

                Ok(())
            }
            WindowThreadRequest::SuggestScaleFactor(scale) => {
                let Some(scale) = crate::settings::sanitize_scale_factor(Some(scale)) else {
                    return Ok(());
                };
                let old_scale = self.window.scale_factor();
                let old_suggestion = self.window.scaling_factor.suggested();
                // If the scaling factor is already provided by the system, do nothing
                if !self.window.scaling_factor.suggest(scale) {
                    return Ok(());
                };

                let new_scale = self.window.scale_factor();
                self.window.main_thread_shared.set_scaling_factor(new_scale);
                let old_size = self.window.get_size();
                let new_physical_size = rescale_physical_size(old_size, old_scale, new_scale);
                let result = self.window.resize_immediately(new_physical_size, &*self.handler);

                // A rejected handler resize restores the previous physical size.
                // Restore its scale too so retrying the suggestion still resizes.
                if result.is_err() && self.window.get_size() == old_size {
                    self.window.scaling_factor.restore_suggestion(old_suggestion);
                    self.window.main_thread_shared.set_scaling_factor(old_scale);
                }
                result
            }
            WindowThreadRequest::SetScaleFactorOverride(scale_factor) => {
                self.window.set_scale_factor_override(scale_factor)
            }
            WindowThreadRequest::SetParent(new_parent) => {
                self.window.xcb_window.reparent(Some(new_parent.window_id))?.check()?;

                Ok(())
            }
            WindowThreadRequest::SetTitle(title) => self.window.set_title(&title),
            WindowThreadRequest::Show => {
                if let Some(dock) = self.window.dock.borrow_mut().as_mut() {
                    dock.sync(true)?;
                }
                self.window.xcb_window.map_window()?.check()?;
                // A top-level MapWindow can be redirected to the WM. A
                // checked reply is not proof of visibility: wait for the real
                // MapNotify/Expose before calling the first renderer frame.
                self.exposed = true;
                Ok(())
            }
            WindowThreadRequest::Hide => {
                if let Some(dock) = self.window.dock.borrow_mut().as_mut() {
                    dock.sync(false)?;
                }
                self.window.xcb_window.unmap_window()?.check()?;
                self.window.visibility_state.window_unmapped(self.window.xcb_window.id());
                Ok(())
            }
        }
    }

    fn handle_connection_event_ready(&mut self) -> Result<PostAction, FatalError> {
        self.drain_xcb_events()?;

        Ok(PostAction::Continue)
    }

    fn handle_idle(&mut self) {
        if let Err(e) = self.try_handle_idle() {
            self.trigger_fatal_error(e);
        }
    }

    fn try_handle_idle(&mut self) -> Result<(), PlatformError> {
        if self.window.main_thread_shared.callbacks_revoked()
            || self.window.main_thread_shared.is_stop_host_requested()
        {
            self.stop_now();
            return Ok(());
        }
        if self.window.main_thread_shared.take_frame_request() {
            self.frame_requested.get_or_insert_with(Instant::now);
        }
        // Check for any events in the internal buffers before going to sleep:
        self.drain_xcb_events()?;

        loop {
            self.handle_coalesced_resize_events()?;
            self.handle_redraw();

            if !self.drain_xcb_events()? {
                break;
            }
        }

        self.schedule_frame()?;
        self.schedule_drag_poll()?;
        self.window.connection.conn.flush()?;
        // Only take_frame_request at the beginning of an idle pass clears the
        // latch. A producer racing demand query/timer re-arm keeps its request
        // pending. Re-notify here; a later producer sends its own notification.
        // Scheduling (e.g. a RandR reply or the handler's frame_demand)
        // can read events into XCB after the redraw loop's last drain.
        // Run another idle pass for any coalesced work those events produced.
        if self.drain_xcb_events()? || self.window.main_thread_shared.has_frame_request() {
            self.loop_signal.wakeup();
        }

        Ok(())
    }

    pub fn run(mut self, mut inner: calloop::EventLoop<Self>) -> Result<(), PlatformError> {
        if self.drain_xcb_events()? {
            // Startup replies can likewise leave work before the first dispatch.
            self.loop_signal.wakeup();
        }
        if !self.window.xcb_window.is_destroyed()
            && !self.window.main_thread_shared.callbacks_revoked()
            && !self.window.main_thread_shared.is_stop_host_requested()
        {
            inner.run(None, &mut self, Self::handle_idle)?;
        }

        self.cancel_matari_drag();
        self.uninstall_drop_router();
        if !self.window.main_thread_shared.callbacks_revoked() {
            if !self.window.xcb_window.is_destroyed() {
                self.release_forwarded_keys();
            }
            self.handle_event(Event::Window(WindowEvent::WillClose));
        }

        // If the event loop doesn't stop because the host asked it to, then we should notify it
        if !self.window.main_thread_shared.is_stop_host_requested() {
            if let Some(main_thread) = self.main_thread.as_mut() {
                if let Err(e) = main_thread.send(HostCallback::Destroyed) {
                    warn!("Could not notify host that X11 thread is stopping: {}", e)
                }
            }
        }

        if let Some(err) = self.run_error {
            return Err(err);
        };

        Ok(())
    }

    fn drain_ime(&mut self) {
        let events =
            self.window.ime.borrow_mut().as_mut().map(|ime| ime.drain()).unwrap_or_default();
        for event in events {
            // MUI's queued XIM callbacks must observe bounded-close revocation too.
            if self.window.main_thread_shared.callbacks_revoked() {
                break;
            }
            match event {
                super::ime::NativeEvent::Input(event) => {
                    self.handle_event(event);
                }
                super::ime::NativeEvent::Key(event) => {
                    let is_down =
                        event.response_type & 0x7f == x11rb::protocol::xproto::KEY_PRESS_EVENT;
                    let mut key = if is_down {
                        convert_key_press_event(&event, &mut self.xkb_state)
                    } else {
                        convert_key_release_event(&event, &mut self.xkb_state)
                    };
                    if let Some(pressed) = self.pressed_keys.get_mut(usize::from(event.detail)) {
                        key.repeat = is_down && *pressed;
                        *pressed = is_down;
                    }
                    if self.handle_event(Event::Keyboard(key)) == EventStatus::Ignored {
                        self.forward_key_event(event);
                    }
                }
            }
        }
    }

    /// Tracks the primary button and the newest pointer event: an outbound
    /// drag may only start from a live press.
    fn update_pointer_authority(&self, event: &XEvent) {
        let pointer = match event {
            XEvent::MotionNotify(e) => {
                self.window
                    .primary_pointer_down
                    .set(e.state.contains(x11rb::protocol::xproto::KeyButMask::BUTTON1));
                X11PointerEvent { root: e.root, root_x: e.root_x, root_y: e.root_y, time: e.time }
            }
            XEvent::ButtonPress(e) => {
                if e.detail == 1 {
                    self.window.primary_pointer_down.set(true);
                }
                X11PointerEvent { root: e.root, root_x: e.root_x, root_y: e.root_y, time: e.time }
            }
            XEvent::ButtonRelease(e) => {
                X11PointerEvent { root: e.root, root_x: e.root_x, root_y: e.root_y, time: e.time }
            }
            _ => return,
        };
        self.window.last_pointer_event.set(Some(pointer));
    }

    fn forward_matari_drag_event(&self, event: &XEvent) {
        let released = matches!(event, XEvent::ButtonRelease(e) if e.detail == 1);
        let connection = self.window.connection.conn.xcb_connection();
        let session = self.window.external_drag_session.borrow_mut().take();
        if let Some(mut session) = session {
            match session.handle_event(connection, event) {
                Ok(X11SessionStatus::Active) => {
                    *self.window.external_drag_session.borrow_mut() = Some(session);
                }
                Ok(X11SessionStatus::Finished) => {}
                Err(_) => {
                    let _ = session.cancel(connection);
                }
            }
        }
        if released {
            self.window.primary_pointer_down.set(false);
        }
    }

    fn poll_matari_drag(&self) {
        let connection = self.window.connection.conn.xcb_connection();
        let session = self.window.external_drag_session.borrow_mut().take();
        if let Some(mut session) = session {
            match session.poll(connection) {
                Ok(X11SessionStatus::Active) => {
                    *self.window.external_drag_session.borrow_mut() = Some(session);
                }
                Ok(X11SessionStatus::Finished) => {}
                Err(_) => {
                    let _ = session.cancel(connection);
                }
            }
        }
    }

    fn schedule_drag_poll(&mut self) -> Result<(), calloop::Error> {
        if self.window.external_drag_session.borrow().is_none() {
            if let Some(timer) = self.drag_timer.take() {
                self.loop_handle.remove(timer);
            }
        } else if self.drag_timer.is_none() {
            self.drag_timer = Some(
                self.loop_handle
                    .insert_source(
                        Timer::from_duration(Duration::from_millis(100)),
                        |_, _, event_loop| {
                            event_loop.poll_matari_drag();
                            if event_loop.window.external_drag_session.borrow().is_some() {
                                TimeoutAction::ToDuration(Duration::from_millis(100))
                            } else {
                                event_loop.drag_timer = None;
                                TimeoutAction::Drop
                            }
                        },
                    )
                    .map_err(|error| error.error)?,
            );
        }
        Ok(())
    }

    fn uninstall_drop_router(&mut self) {
        if let Some(router) = self.drop_router.take() {
            if let Err(error) = router.uninstall(self.window.connection.conn.xcb_connection()) {
                warn!("Could not uninstall the X11 drop router: {}", error);
            }
        }
    }

    fn cancel_matari_drag(&self) {
        let connection = self.window.connection.conn.xcb_connection();
        let session = self.window.external_drag_session.borrow_mut().take();
        if let Some(session) = session {
            let _ = session.cancel(connection);
        }
    }

    fn handle_xcb_event(&mut self, event: XEvent) -> Result<(), ConnectionError> {
        let filtered = self.window.ime.borrow_mut().as_mut().is_some_and(|ime| ime.filter(&event));
        self.drain_ime();
        if self.window.main_thread_shared.callbacks_revoked() {
            return Ok(());
        }
        if filtered {
            return Ok(());
        }
        if let XEvent::KeyPress(key) | XEvent::KeyRelease(key) = &event {
            if key.event == self.window.raw_id() {
                let forwarded =
                    self.window.ime.borrow_mut().as_mut().is_some_and(|ime| ime.forward(key));
                if forwarded {
                    return Ok(());
                }
            }
        }

        // For all the keyboard and mouse events, you can fetch
        // `x`, `y`, `detail`, and `state`.
        // - `x` and `y` are the position inside the window where the cursor currently is
        //   when the event happened.
        // - `detail` will tell you which keycode was pressed/released (for keyboard events)
        //   or which mouse button was pressed/released (for mouse events).
        //   For mouse events, here's what the value means (at least on my current mouse):
        //      1 = left mouse button
        //      2 = middle mouse button (scroll wheel)
        //      3 = right mouse button
        //      4 = scroll wheel up
        //      5 = scroll wheel down
        //      8 = lower side button ("back" button)
        //      9 = upper side button ("forward" button)
        //   Note that you *will* get a "button released" event for even the scroll wheel
        //   events, which you can probably ignore.
        // - `state` will tell you the state of the main three mouse buttons and some of
        //   the keyboard modifier keys at the time of the event.
        //   http://rtbo.github.io/rust-xcb/src/xcb/ffi/xproto.rs.html#445

        let routed = self.drop_router.as_mut().map(|router| {
            router.handle_event(self.window.connection.conn.xcb_connection(), &event)
        });
        match routed {
            Some(Ok(true)) => return Ok(()),
            Some(Err(error)) => {
                warn!("X11 drop router event failed: {}", error);
                self.uninstall_drop_router();
            }
            Some(Ok(false)) | None => {}
        }

        self.update_pointer_authority(&event);
        self.forward_matari_drag_event(&event);
        trace_input(self.window.raw_id(), self.window.scaling_factor.get(), &event);

        match event {
            XEvent::RandrScreenChangeNotify(_) | XEvent::RandrNotify(_) => {
                if let Some(dock) = self.window.dock.borrow_mut().as_mut() {
                    if let Err(error) = dock.refresh() {
                        warn!("dock output changed: {}", error);
                        self.window.request_close();
                    }
                }
            }
            ////
            // window
            ////
            XEvent::ClientMessage(event) if event.window == self.window.raw_id() => {
                if event.format != 32 {
                    return Ok(());
                }
                if event.type_ == self.window.connection.atoms._XEMBED {
                    match xembed::parse(event.data.as_data32()) {
                        xembed::Message::EmbeddedNotify { embedder } => {
                            self.embedder = Some(embedder);
                        }
                        xembed::Message::FocusIn => self.focus_in(true),
                        xembed::Message::FocusOut => self.focus_lost(),
                        xembed::Message::Other => {}
                    }
                    return Ok(());
                }

                if event.data.as_data32()[0] == self.window.connection.atoms.WM_DELETE_WINDOW {
                    if self.window.defer_user_close {
                        self.handle_event(Event::Window(WindowEvent::WillClose));
                    } else {
                        self.window.request_close();
                    }
                    return Ok(());
                }

                ////
                // drag n drop
                ////
                if event.type_ == self.window.connection.atoms.XdndEnter {
                    self.drag_n_drop.handle_enter_event(&self.window, &*self.handler, &event)?;
                } else if event.type_ == self.window.connection.atoms.XdndPosition {
                    self.drag_n_drop.handle_position_event(&self.window, &*self.handler, &event)?;
                } else if event.type_ == self.window.connection.atoms.XdndDrop {
                    self.drag_n_drop.handle_drop_event(&self.window, &*self.handler, &event)?;
                } else if event.type_ == self.window.connection.atoms.XdndLeave {
                    self.drag_n_drop.handle_leave_event(&*self.handler, &event);
                }
            }

            XEvent::SelectionNotify(event) => {
                if event.selection == self.window.connection.atoms.XdndSelection {
                    self.drag_n_drop.handle_selection_notify_event(
                        &self.window,
                        &*self.handler,
                        &event,
                    )?;
                }
            }

            XEvent::Error(e) => {
                warn!("Received leftover X11 error: {:?}", e);
            }

            XEvent::PropertyNotify(e)
                if e.window == self.window.connection.default_screen().root
                    && e.atom == u32::from(AtomEnum::RESOURCE_MANAGER) =>
            {
                self.rescale_from_xft();
            }

            XEvent::ConfigureNotify(event) => {
                // Our window, an ancestor or any top-level moved: maybe onto another monitor.
                self.refresh_rate_stale = true;
                // These are coalesced and then handled asynchronously at the end of the event loop
                if event.window == self.window.raw_id() {
                    let size = (u32::from(event.width), u32::from(event.height));
                    if !self.window.xcb_window.is_stale_echo(size) {
                        self.new_size = Some(PhysicalSize::new(event.width, event.height));
                    }
                } else if Some(event.window)
                    == self.window.visibility_state.parent_id().map(|i| i.get())
                {
                    // Also resize the window if the parent is resized
                    // This works around some hosts that might not call set_size() right away (or at all...)
                    self.new_parent_size = Some(PhysicalSize::new(event.width, event.height));
                }
            }

            XEvent::Expose(e) if e.window == self.window.raw_id() => {
                self.exposed = true;
                if !self.window.main_thread_shared.callbacks_revoked()
                    && !self.window.main_thread_shared.is_stop_host_requested()
                {
                    self.handler.damage(crate::DamageArea::Rect(crate::DamageRect::new(
                        PhysicalPosition::new(u32::from(e.x), u32::from(e.y)),
                        PhysicalSize::new(u32::from(e.width), u32::from(e.height)),
                    )));
                }
                self.handle_event(Event::Window(WindowEvent::RedrawRequested));
            }

            ////
            // mouse
            ////
            XEvent::MotionNotify(event) if event.event == self.window.raw_id() => {
                self.pointer_at = Some((event.event_x, event.event_y, event.state));
                let physical_pos = PhysicalPosition::new(event.event_x, event.event_y);

                self.handle_event(Event::Mouse(MouseEvent::CursorMoved {
                    position: physical_pos.cast(),
                    modifiers: key_mods(event.state),
                }));
            }

            XEvent::EnterNotify(event) if event.event == self.window.raw_id() => {
                self.pointer_at = Some((event.event_x, event.event_y, event.state));
                self.handle_event(Event::Mouse(MouseEvent::CursorEntered));
                // since no `MOTION_NOTIFY` event is generated when `ENTER_NOTIFY` is generated,
                // we generate a CursorMoved as well, so the mouse position from here isn't lost
                let physical_pos = PhysicalPosition::new(event.event_x, event.event_y);
                self.handle_event(Event::Mouse(MouseEvent::CursorMoved {
                    position: physical_pos.cast(),
                    modifiers: key_mods(event.state),
                }));
            }

            XEvent::LeaveNotify(event) if event.event == self.window.raw_id() => {
                self.pointer_at = None;
                self.handle_event(Event::Mouse(MouseEvent::CursorLeft));
            }

            XEvent::ButtonPress(event) if event.event == self.window.raw_id() => {
                match event.detail {
                    4..=7 => {
                        self.handle_event(Event::Mouse(MouseEvent::WheelScrolled {
                            delta: match event.detail {
                                4 => ScrollDelta::Lines { x: 0.0, y: 1.0 },
                                5 => ScrollDelta::Lines { x: 0.0, y: -1.0 },
                                6 => ScrollDelta::Lines { x: -1.0, y: 0.0 },
                                7 => ScrollDelta::Lines { x: 1.0, y: 0.0 },
                                _ => unreachable!(),
                            },
                            modifiers: key_mods(event.state),
                        }));
                    }
                    detail => {
                        // A host does not always give its plugin's window the
                        // keyboard: a click inside takes it, with the click's
                        // own time, and it goes back to the parent after.
                        self.buttons_held |= button_bit(detail);
                        if takes_focus(self.window.parented, self.owns_focus, detail) {
                            self.take_focus(event.time);
                        }
                        self.handle_event(Event::Mouse(MouseEvent::ButtonPressed {
                            button: mouse_id(detail),
                            modifiers: key_mods(event.state),
                        }));
                    }
                }
            }

            XEvent::ButtonRelease(event)
                if event.event == self.window.raw_id() && !(4..=7).contains(&event.detail) =>
            {
                let button_id = mouse_id(event.detail);
                self.handle_event(Event::Mouse(MouseEvent::ButtonReleased {
                    button: button_id,
                    modifiers: key_mods(event.state),
                }));
                self.buttons_held &= !button_bit(event.detail);
                if self.buttons_held == 0 && std::mem::take(&mut self.deferred_unfocus) {
                    self.focus_out();
                }
            }

            ////
            // keys
            ////
            XEvent::KeyPress(event) if event.event == self.window.raw_id() => {
                let mut key = convert_key_press_event(&event, &mut self.xkb_state);
                if let Some(pressed) = self.pressed_keys.get_mut(usize::from(event.detail)) {
                    key.repeat = std::mem::replace(pressed, true);
                }
                if self.handle_event(Event::Keyboard(key)) == EventStatus::Ignored {
                    self.forward_key_event(event);
                }
            }

            XEvent::KeyRelease(event) if event.event == self.window.raw_id() => {
                if let Some(pressed) = self.pressed_keys.get_mut(usize::from(event.detail)) {
                    *pressed = false;
                }
                let key = convert_key_release_event(&event, &mut self.xkb_state);
                if self.handle_event(Event::Keyboard(key)) == EventStatus::Ignored {
                    self.forward_key_event(event);
                }
            }

            // Under XEmbed the embedder keeps the X focus; `_XEMBED` says ours.
            XEvent::FocusIn(event)
                if event.event == self.window.raw_id() && self.embedder.is_none() =>
            {
                self.focus_in(event.detail != NotifyDetail::POINTER);
            }

            XEvent::FocusOut(e)
                if e.event == self.window.raw_id()
                    && self.embedder.is_none()
                    && !(self.taking_focus && e.detail == NotifyDetail::POINTER) =>
            {
                self.focus_lost();
            }

            XEvent::MapNotify(e) => {
                if let Some(window_id) = NonZero::new(e.window) {
                    if window_id == self.window.xcb_window.id() {
                        self.window.is_mapped.set(true);
                    }

                    let became_viewable = self.window.visibility_state.window_mapped(window_id);

                    if became_viewable {
                        self.exposed = true;
                    }
                }
            }

            XEvent::UnmapNotify(e) => {
                if let Some(window_id) = NonZero::new(e.window) {
                    if window_id == self.window.xcb_window.id() {
                        self.window.is_mapped.set(false)
                    }

                    self.window.visibility_state.window_unmapped(window_id);
                    // An unviewable window loses its implicit grab: the held
                    // buttons' releases go elsewhere, so stop waiting for them.
                    if self.buttons_held != 0
                        && !self.window.visibility_state.own_window_is_viewable()
                    {
                        self.buttons_held = 0;
                        if std::mem::take(&mut self.deferred_unfocus) {
                            self.focus_out();
                        }
                    }
                }
            }

            XEvent::ReparentNotify(e) => {
                if let Some(window_id) = NonZero::new(e.window) {
                    self.window.visibility_state.window_reparented(
                        window_id,
                        NonZero::new(e.parent),
                        &self.window.connection,
                    );
                    self.exposed = self.window.visibility_state.own_window_is_viewable();
                }
            }

            XEvent::DestroyNotify(e) => {
                if let Some(window_id) = NonZero::new(e.window) {
                    self.window
                        .visibility_state
                        .window_destroyed(window_id, &self.window.connection);
                    if window_id == self.window.xcb_window.id() {
                        // The host may destroy its parent without first closing
                        // the editor. Stop callbacks before any pending resize or
                        // frame can use the deleted drawable; run() still closes
                        // the handler normally.
                        self.window.xcb_window.mark_destroyed();
                        self.stop_now();
                    }
                }
            }

            _ => {}
        }

        Ok(())
    }

    /// Asks for the keyboard after a click: from the XEmbed embedder, which
    /// keeps the X focus, or from the server.
    fn take_focus(&mut self, time: u32) {
        let conn = &self.window.connection.conn;
        if let Some(embedder) = self.embedder {
            let atom = self.window.connection.atoms._XEMBED;
            let message = ClientMessageEvent::new(32, embedder, atom, xembed::request_focus(time));
            let _ = conn.send_event(false, embedder, EventMask::NO_EVENT, message);
        } else {
            let _ = conn.set_input_focus(InputFocus::PARENT, self.window.raw_id(), time);
            self.taking_focus = true;
        }
        let _ = conn.flush();
    }

    fn focus_in(&mut self, owned: bool) {
        self.taking_focus = false;
        self.deferred_unfocus = false;
        self.owns_focus = owned;
        self.window.is_focused.set(true);
        let config = self.window.ime_configuration.borrow().clone();
        self.window.set_ime_configuration(config);
        self.drain_ime();
        self.handle_event(Event::Window(WindowEvent::Focused));
    }

    /// The keyboard left: at once, or after the held buttons' releases.
    fn focus_lost(&mut self) {
        if self.buttons_held == 0 {
            self.focus_out();
        } else {
            self.deferred_unfocus = true;
        }
    }

    fn focus_out(&mut self) {
        self.owns_focus = false;
        self.window.is_focused.set(false);
        let config = self.window.ime_configuration.borrow().clone();
        self.window.set_ime_configuration(config);
        self.drain_ime();
        self.pressed_keys.fill(false);
        self.release_forwarded_keys();
        self.handle_event(Event::Window(WindowEvent::Unfocused));
        // The keyboard left, not the pointer: a click with no motion before it
        // must still land where the pointer is.
        if let Some((x, y, state)) = self.pointer_at {
            self.handle_event(Event::Mouse(MouseEvent::CursorMoved {
                position: PhysicalPosition::new(x, y).cast(),
                modifiers: key_mods(state),
            }));
        }
    }

    /// `Xft.dpi` changed: a window that follows it keeps its logical size at
    /// the new scale. A host's or override scale still wins.
    fn rescale_from_xft(&mut self) {
        let old = self.window.scaling_factor.get();
        self.window.scaling_factor.set_system(self.window.connection.current_scaling());
        let new = self.window.scaling_factor.get();
        if new.to_bits() == old.to_bits() {
            return;
        }
        self.window.main_thread_shared.set_scaling_factor(new);
        self.handle_event(Event::Window(WindowEvent::ScaleFactorChanged(new)));
        let size = self.window.get_size().to_logical::<f64>(old).to_physical(new);
        if let Err(e) = self.window.resize_immediately(size, &*self.handler) {
            warn!("Failed to resize after an Xft.dpi change: {}", e);
        }
    }

    fn handle_event(&mut self, event: Event) -> EventStatus {
        if self.window.main_thread_shared.callbacks_revoked()
            || (self.window.main_thread_shared.is_stop_host_requested()
                && !matches!(event, Event::Window(WindowEvent::WillClose)))
        {
            return EventStatus::Ignored;
        }
        if trace_on()
            && matches!(
                &event,
                Event::Mouse(_) | Event::Window(WindowEvent::Focused | WindowEvent::Unfocused)
            )
        {
            #[expect(
                clippy::print_stderr,
                reason = "MUI_INPUT_TRACE is a user-run stderr diagnostic"
            )]
            {
                eprintln!("[mui-input] deliver {event:?}");
            }
        }
        if matches!(&event, Event::Window(WindowEvent::WillClose)) {
            self.window.primary_pointer_down.set(false);
            self.window.last_pointer_event.set(None);
            self.cancel_matari_drag();
        }
        self.exposed = true;
        self.handler.on_event(event)
    }

    /// Hands a key event the handler ignored to the embed parent, so host
    /// shortcuts (transport, undo, ...) keep working while the editor has
    /// focus. Ported from a host plugin (K26).
    fn forward_key_event(&mut self, mut event: KeyPressEvent) {
        let Some(parent) = self.window.visibility_state.parent_id() else {
            return;
        };
        // XSendEvent sets the high bit. Never bounce a host-redispatched event
        // back; only forward original input.
        if event.response_type & 0x80 != 0 {
            return;
        }
        event.event = parent.get();
        event.child = self.window.raw_id();
        let conn = &self.window.connection.conn;
        let mask = EventMask::KEY_PRESS | EventMask::KEY_RELEASE;
        if conn.send_event(true, parent.get(), mask, event).is_ok() {
            if let Some(slot) = self.forwarded_keys.get_mut(usize::from(event.detail)) {
                *slot = (event.response_type == x11rb::protocol::xproto::KEY_PRESS_EVENT)
                    .then_some(event);
            }
        }
        let _ = conn.flush();
    }

    fn release_forwarded_keys(&mut self) {
        let held: Vec<KeyPressEvent> =
            self.forwarded_keys.iter_mut().filter_map(Option::take).collect();
        for mut event in held {
            event.response_type = x11rb::protocol::xproto::KEY_RELEASE_EVENT;
            event.time = CURRENT_TIME;
            self.forward_key_event(event);
        }
    }
}

/// Whether a press of button `detail` takes the keyboard: only in an
/// embedded window that lacks it, and never for the wheel (4..=7), so a
/// pointer passing over or scrolling leaves the host's keys alone.
fn takes_focus(parented: bool, focused: bool, detail: u8) -> bool {
    parented && !focused && !(4..=7).contains(&detail)
}

fn trace_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("MUI_INPUT_TRACE").is_some_and(|v| v != "0"))
}

/// `MUI_INPUT_TRACE=1`: the pointer and focus events X delivers, on stderr,
/// so a user can show where their clicks go.
#[expect(clippy::print_stderr, reason = "MUI_INPUT_TRACE is a user-run stderr diagnostic")]
fn trace_input(window: u32, scale: f64, event: &XEvent) {
    if !trace_on() {
        return;
    }
    let what = match event {
        XEvent::MotionNotify(e) => {
            format!("motion to={:#x} at={},{}", e.event, e.event_x, e.event_y)
        }
        XEvent::EnterNotify(e) | XEvent::LeaveNotify(e) => format!(
            "{} to={:#x} at={},{} mode={:?} detail={:?}",
            if e.response_type & 0x7f == x11rb::protocol::xproto::ENTER_NOTIFY_EVENT {
                "enter"
            } else {
                "leave"
            },
            e.event,
            e.event_x,
            e.event_y,
            e.mode,
            e.detail
        ),
        XEvent::ButtonPress(e) | XEvent::ButtonRelease(e) => format!(
            "{} {} to={:#x} child={:#x} at={},{}",
            if e.response_type & 0x7f == x11rb::protocol::xproto::BUTTON_PRESS_EVENT {
                "press"
            } else {
                "release"
            },
            e.detail,
            e.event,
            e.child,
            e.event_x,
            e.event_y
        ),
        XEvent::FocusIn(e) | XEvent::FocusOut(e) => format!(
            "{} to={:#x} mode={:?} detail={:?}",
            if e.response_type & 0x7f == x11rb::protocol::xproto::FOCUS_IN_EVENT {
                "focus-in"
            } else {
                "focus-out"
            },
            e.event,
            e.mode,
            e.detail
        ),
        _ => return,
    };
    eprintln!("[mui-input] window={window:#x} scale={scale} {what}");
}

/// A held-button bit for X button `detail`; wheel steps (4-7) hold nothing.
fn button_bit(detail: u8) -> u32 {
    if (4..=7).contains(&detail) {
        0
    } else {
        1u32.checked_shl(u32::from(detail)).unwrap_or(0)
    }
}

fn frame_deadline(
    demand: FrameDemand, now: Instant, last_frame: Instant, interval: Duration,
) -> Option<Instant> {
    let paced = last_frame.checked_add(interval).unwrap_or(now).max(now);
    match demand {
        FrameDemand::Idle => None,
        FrameDemand::Continuous => Some(paced),
        FrameDemand::At(at) => Some(at.max(paced)),
    }
}

/// `demand`, due no later than `now`.
fn soonest(demand: FrameDemand, now: Instant) -> FrameDemand {
    match demand {
        FrameDemand::Idle => FrameDemand::At(now),
        FrameDemand::At(at) => FrameDemand::At(at.min(now)),
        FrameDemand::Continuous => FrameDemand::Continuous,
    }
}

/// Refresh rate of the RandR CRTC under the window's centre, `None` when RandR is missing or
/// the window is off every CRTC (MOOSE).
fn query_refresh_hz(window: &WindowInner) -> Option<f64> {
    let conn = window.connection.conn.xcb_connection();
    let root = window.connection.conn.default_screen().root;
    let origin = conn.translate_coordinates(window.raw_id(), root, 0, 0).ok()?.reply().ok()?;
    let size = window.get_size();
    let x = i32::from(origin.dst_x).saturating_add(i32::from(size.width >> 1));
    let y = i32::from(origin.dst_y).saturating_add(i32::from(size.height >> 1));

    let resources = conn.randr_get_screen_resources_current(root).ok()?.reply().ok()?;
    let crtcs: Vec<_> = resources
        .crtcs
        .iter()
        .filter_map(|&crtc| conn.randr_get_crtc_info(crtc, resources.config_timestamp).ok())
        .collect();
    let crtc = crtcs.into_iter().filter_map(|cookie| cookie.reply().ok()).find(|crtc| {
        let (cx, cy) = (i32::from(crtc.x), i32::from(crtc.y));
        crtc.mode != 0
            && (cx..cx.saturating_add(crtc.width.into())).contains(&x)
            && (cy..cy.saturating_add(crtc.height.into())).contains(&y)
    })?;
    mode_refresh_hz(resources.modes.iter().find(|mode| mode.id == crtc.mode)?)
}

fn mode_refresh_hz(mode: &ModeInfo) -> Option<f64> {
    let mut lines = f64::from(mode.vtotal);
    if mode.mode_flags.contains(ModeFlag::DOUBLE_SCAN) {
        lines *= 2.0;
    }
    if mode.mode_flags.contains(ModeFlag::INTERLACE) {
        lines /= 2.0;
    }
    let pixels = f64::from(mode.htotal) * lines;
    (pixels > 0.0).then(|| f64::from(mode.dot_clock) / pixels)
}

fn is_auto_repeat_pair(release: &KeyReleaseEvent, press: &KeyPressEvent) -> bool {
    release.detail == press.detail && release.time == press.time && release.event == press.event
}

fn rescale_physical_size(
    current: PhysicalSize<u16>, old_scale: f64, new_scale: f64,
) -> PhysicalSize<u16> {
    let old_scale = if old_scale.is_finite() && old_scale > 0.0 { old_scale } else { 1.0 };
    let new_scale = if new_scale.is_finite() && new_scale > 0.0 { new_scale } else { old_scale };
    current.to_logical::<f64>(old_scale).to_physical(new_scale)
}

fn mouse_id(id: u8) -> MouseButton {
    match id {
        1 => MouseButton::Left,
        2 => MouseButton::Middle,
        3 => MouseButton::Right,
        8 => MouseButton::Back,
        9 => MouseButton::Forward,
        id => MouseButton::Other(id),
    }
}

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects, reason = "test clocks use fixed small durations")]
mod tests {
    #[test]
    fn only_a_click_in_an_unfocused_embedded_window_takes_the_keyboard() {
        assert!(super::takes_focus(true, false, 1));
        assert!(super::takes_focus(true, false, 3));
        assert!(!super::takes_focus(true, true, 1), "it has it already");
        assert!(!super::takes_focus(false, false, 1), "a top level's is the WM's");
        assert!((4..=7).all(|w| !super::takes_focus(true, false, w)), "the wheel never");
    }

    use super::mode_refresh_hz;
    use x11rb::protocol::randr::{ModeFlag, ModeInfo};

    fn mode(dot_clock: u32, htotal: u16, vtotal: u16, flags: ModeFlag) -> ModeInfo {
        ModeInfo {
            id: 1,
            width: 0,
            height: 0,
            dot_clock,
            hsync_start: 0,
            hsync_end: 0,
            htotal,
            hskew: 0,
            vsync_start: 0,
            vsync_end: 0,
            vtotal,
            name_len: 0,
            mode_flags: flags,
        }
    }

    #[test]
    fn idle_has_no_deadline_and_demand_never_schedules_catchup_frames() {
        use super::frame_deadline;
        use crate::FrameDemand;
        use std::time::{Duration, Instant};
        let now = Instant::now();
        let interval = Duration::from_millis(16);
        assert_eq!(frame_deadline(FrameDemand::Idle, now, now, interval), None);
        assert_eq!(
            frame_deadline(FrameDemand::Continuous, now, now, interval),
            Some(now + interval)
        );
        let later = now + Duration::from_secs(1);
        assert_eq!(frame_deadline(FrameDemand::At(later), now, now, interval), Some(later));
        assert_eq!(frame_deadline(FrameDemand::Continuous, later, now, interval), Some(later));
    }

    #[test]
    fn a_frame_request_right_after_a_frame_waits_for_the_refresh() {
        use super::{frame_deadline, soonest};
        use crate::FrameDemand;
        use std::time::{Duration, Instant};
        let now = Instant::now();
        let interval = Duration::from_millis(16);
        let asked = soonest(FrameDemand::Idle, now);
        assert_eq!(frame_deadline(asked, now, now, interval), Some(now + interval));
        let long_ago = now.checked_sub(interval * 2).unwrap_or(now);
        assert_eq!(frame_deadline(asked, now, long_ago, interval), Some(now));
        // A request is never held behind a later deadline the handler gave.
        let later = now + Duration::from_secs(5);
        assert_eq!(soonest(FrameDemand::At(later), now), FrameDemand::At(now));
    }

    #[test]
    fn refresh_rate_from_mode_timings() {
        // CVT 1920x1080@60 reduced blanking: 138.5 MHz, 2080x1111.
        let hz = mode_refresh_hz(&mode(138_500_000, 2080, 1111, ModeFlag::from(0u32)));
        assert!((hz.unwrap_or_default() - 59.934).abs() < 0.001);
        let double = mode_refresh_hz(&mode(100, 1, 1, ModeFlag::DOUBLE_SCAN));
        assert_eq!(double, Some(50.0));
        assert_eq!(mode_refresh_hz(&mode(100, 1, 1, ModeFlag::INTERLACE)), Some(200.0));
        assert_eq!(mode_refresh_hz(&mode(100, 0, 0, ModeFlag::from(0u32))), None);
    }

    #[test]
    fn suggest_scale_factor_rescales_from_previous_scale_without_compounding() {
        use super::rescale_physical_size;
        use crate::dpi::PhysicalSize;

        // Logical 400x300 at 1.0 -> 2.0 becomes 800x600.
        let s1 = rescale_physical_size(PhysicalSize::new(400, 300), 1.0, 2.0);
        assert_eq!(s1, PhysicalSize::new(800, 600));
        // Repeating 2.0 stays 800x600 instead of compounding to 1600x1200.
        let s2 = rescale_physical_size(s1, 2.0, 2.0);
        assert_eq!(s2, PhysicalSize::new(800, 600));
        // Stepping 2.0 -> 1.5 becomes 600x450, then back to 1.0 returns to 400x300.
        let s3 = rescale_physical_size(s2, 2.0, 1.5);
        assert_eq!(s3, PhysicalSize::new(600, 450));
        let s4 = rescale_physical_size(s3, 1.5, 1.0);
        assert_eq!(s4, PhysicalSize::new(400, 300));
    }
}
