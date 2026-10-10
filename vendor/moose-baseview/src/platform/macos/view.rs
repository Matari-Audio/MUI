#![expect(deprecated, reason = "Allow use of NSFilenamesPboardType for now")]

use super::keyboard::{make_modifiers, KeyboardState};
use super::policy::{deadline_can_fire, pacing, top_origin, HandlerSlot, Pace, WakeState};
use super::window::WindowSharedState;
use crate::dpi::{LogicalPosition, LogicalSize, Size};
use crate::host::Host;
use crate::platform::frame_rate::frame_interval;
use crate::platform::macos::cursor::CursorManager;
use crate::platform::*;
use crate::tracing::warn;
use crate::utils::SizingStrategy;
use crate::window::WindowInitializer;
use crate::wrappers::appkit::*;
use crate::MouseEvent::{ButtonPressed, ButtonReleased};
use crate::{
    DropData, DropEffect, Event, EventStatus, FrameDemand, FrameRequester, MouseButton, MouseEvent,
    ScrollDelta, WindowEvent, WindowHandler, WindowSize,
};
use objc2::__framework_prelude::Retained;
use objc2::rc::Weak;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{msg_send, sel, AllocAnyThread, ClassType, MainThreadMarker, Message};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSDragOperation, NSDraggingInfo, NSEvent,
    NSEventModifierFlags, NSEventType, NSFilenamesPboardType, NSResponder, NSScreen,
    NSTrackingArea, NSTrackingAreaOptions, NSView, NSWindow, NSWindowDidBecomeKeyNotification,
    NSWindowDidResignKeyNotification, NSWindowOcclusionState, NSWindowWillCloseNotification,
};
use objc2_foundation::{
    NSArray, NSNotification, NSPoint, NSPointInRect, NSRect, NSRunLoop, NSRunLoopCommonModes,
    NSSize, NSString,
};
use objc2_quartz_core::CADisplayLink;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Instant;

thread_local! {
    // Main-thread-only weak refs. Requesters carry an ID, never an AppKit pointer,
    // so their destruction on an audio/background thread cannot dispatch_sync.
    static VIEWS: RefCell<HashMap<u64, Weak<View<BaseviewView>>>> = RefCell::new(HashMap::new());
}
static NEXT_VIEW_ID: AtomicU64 = AtomicU64::new(1);
#[cfg(debug_assertions)]
static LIVE_VIEWS: AtomicU64 = AtomicU64::new(0);

pub enum ViewParentingType {
    Parented { parent_view: Weak<NSView> },
    Windowed { owned_window: Weak<NSWindow> },
    Uninitialized,
}

impl ViewParentingType {
    fn setup(&self, child: &View<BaseviewView>) {
        match self {
            ViewParentingType::Parented { parent_view } => {
                if let Some(parent_view) = parent_view.load() {
                    parent_view.addSubview(child);
                }
            }
            ViewParentingType::Windowed { owned_window, .. } => {
                if let Some(owned_window) = owned_window.load() {
                    owned_window.setContentView(Some(child));
                    set_delegate(&owned_window, child);
                }
            }
            _ => {}
        }
    }

    fn teardown(self) {
        if let ViewParentingType::Windowed { owned_window: parent_window } = self {
            if let Some(parent_window) = parent_window.load() {
                parent_window.setDelegate(None);
                parent_window.setContentView(None);
                parent_window.close();
            }
        }
    }
}

pub(crate) struct BaseviewView {
    pub(crate) state: Rc<WindowSharedState>,
    pub(crate) mtm: MainThreadMarker,
    window_handler: WindowHandlerContainer,
    ready: Cell<bool>,
    resizing: Cell<bool>,
    attached: Cell<bool>,
    view_id: u64,
    wake: Arc<WakeState>,
    demand: Cell<FrameDemand>,
    frame_requested: Cell<bool>,
    deadline_timer: Cell<Option<TimerHandle>>,
    armed_deadline: Cell<Option<Instant>>,
    tracking_area: Cell<Option<Retained<NSTrackingArea>>>,
    keyboard_capture: Cell<bool>,
    previous_responder: Cell<Option<Weak<NSResponder>>>,

    /// Drives `on_frame`: the view's display link on macOS 14+, else a timer at the screen's rate.
    display_link: Cell<Option<Retained<CADisplayLink>>>,
    frame_timer: Cell<Option<TimerHandle>>,
    notification_center_observer: Cell<Option<NotificationCenterObserver>>,

    keyboard_state: KeyboardState,
    pub(crate) ime: super::ime::NativeIme,

    parenting: RefCell<ViewParentingType>,
    pub(crate) lifetime_tied_to_app: Cell<Option<Weak<NSApplication>>>,

    host: Host,
    pub(crate) cursor_manager: CursorManager,

    #[cfg(feature = "opengl")]
    pub(crate) gl_context: std::cell::OnceCell<super::gl::GlContext>,
}

impl BaseviewView {
    pub fn new(
        init: WindowInitializer, parenting: ViewParentingType, final_size: LogicalSize<f64>,
        mtm: MainThreadMarker,
    ) -> Result<(Retained<View<Self>>, Rc<WindowSharedState>)> {
        let view_rect =
            NSRect::new(NSPoint::ZERO, NSSize::new(final_size.width, final_size.height));

        let state = Rc::new(WindowSharedState::new(
            final_size,
            1.0,
            SizingStrategy::from_settings(&init.settings),
        ));

        let inner = BaseviewView {
            mtm,
            state: Rc::clone(&state),

            keyboard_state: KeyboardState::new(),
            ime: Default::default(),
            display_link: None.into(),
            frame_timer: None.into(),
            window_handler: WindowHandlerContainer::new(),
            ready: Cell::new(false),
            resizing: Cell::new(false),
            attached: Cell::new(false),
            view_id: NEXT_VIEW_ID.fetch_add(1, Ordering::Relaxed),
            wake: Arc::new(WakeState::default()),
            demand: Cell::new(FrameDemand::Idle),
            frame_requested: Cell::new(true),
            deadline_timer: Cell::new(None),
            armed_deadline: Cell::new(None),
            tracking_area: Cell::new(None),
            keyboard_capture: Cell::new(false),
            previous_responder: Cell::new(None),
            notification_center_observer: None.into(),
            parenting: ViewParentingType::Uninitialized.into(),
            host: init.host,
            lifetime_tied_to_app: None.into(),
            cursor_manager: CursorManager::new(),

            #[cfg(feature = "opengl")]
            gl_context: std::cell::OnceCell::new(),
        };

        #[cfg(debug_assertions)]
        LIVE_VIEWS.fetch_add(1, Ordering::Relaxed);
        let view = View::new(view_rect, inner, |view| {
            VIEWS.with(|views| views.borrow_mut().insert(view.view_id, Weak::new(view.view)));
            // AppKit must establish the Metal backing layer BEFORE insertion or
            // surface creation. Zero-sized frames stay zero until the host sizes us.
            view.view.setWantsLayer(true);
            let metal = objc2::runtime::AnyClass::get(c"CAMetalLayer")
                .ok_or(PlatformError::CreationFailed("CAMetalLayer unavailable"))?;
            if !view.view.layer().is_some_and(|layer| layer.isKindOfClass(metal)) {
                return Err(PlatformError::CreationFailed("Metal backing layer unavailable"));
            }
            view.view.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            // Set up parenting before handler setup
            parenting.setup(view.view);
            view.parenting.replace(parenting);

            view.state.scale_factor.set(view.view.backing_scale_factor());
            view.state.size.set(view.view.size());

            Self::apply_size_constraints(view);

            #[cfg(feature = "opengl")]
            if let Some(gl_config) = init.settings.gl_config {
                let gl_context = super::gl::GlContext::create(view.view, gl_config, view.mtm)?;
                view.gl_context.set(gl_context).map_err(|_| {
                    PlatformError::CreationFailed("OpenGL context already initialized")
                })?;
            }

            let context = WindowContext::new(view);
            let handler = init.builder.build(crate::WindowContext::new(context))?;

            // Initialize handler
            view.window_handler.set(handler);
            if view.state.closed.get() {
                return Err(PlatformError::CreationFailed(
                    "view closed during handler initialization",
                ));
            }
            view.ready.set(true);

            // Set up anything that might trigger events to the handler

            // SAFETY: This static is a read-only constant
            let ns_filenames_pboard_type = unsafe { NSFilenamesPboardType };
            view.view.registerForDraggedTypes(&NSArray::from_slice(&[ns_filenames_pboard_type]));

            Self::reanchor_to_superview_top(view);
            Self::sync_layer(view);
            Self::update_frame_demand(view);

            Ok(())
        })?;

        Ok((view, state))
    }

    pub fn show(this: ViewRef<Self>) {
        if this.state.closed.get() {
            return;
        }
        let Ok(parent) = this.parenting.try_borrow() else { return };

        if let ViewParentingType::Windowed { owned_window } = &*parent {
            if let Some(window) = owned_window.load() {
                window.makeKeyAndOrderFront(None)
            }
        }
        drop(parent);
        Self::request_frame(this);
    }

    pub fn hide(this: ViewRef<Self>) {
        if this.state.closed.get() {
            return;
        }
        Self::stop_frame_driver(this);
        let Ok(parent) = this.parenting.try_borrow() else { return };

        if let ViewParentingType::Windowed { owned_window } = &*parent {
            if let Some(window) = owned_window.load() {
                window.orderOut(None)
            }
        }
    }

    pub fn close(this: ViewRef<Self>, from_host: bool) {
        if this.state.closed.replace(true) {
            return;
        }
        // Revocation happens before any Objective-C call that can reenter us.
        this.wake.revoke();
        let _ = VIEWS.try_with(|views| views.borrow_mut().remove(&this.view_id));
        let _keep_alive = this.view.retain();
        Self::detach_callbacks(this);
        this.ime.focused.set(false);
        this.ime.marked.borrow_mut().clear();
        // Explicit close, not dealloc/retainCount, breaks renderer/layer cycles.
        this.window_handler.destroy();
        this.view.unregisterDraggedTypes();
        this.view.removeFromSuperview();

        let parenting = this.parenting.replace(ViewParentingType::Uninitialized);
        parenting.teardown();

        if let Some(app) = this.lifetime_tied_to_app.take() {
            if let Some(app) = app.load() {
                app.stop(Some(&app));
                // stop() from a display link/timer only sets a flag; run()
                // checks it after dispatching an NSEvent. Never stop the host's loop.
                if let Some(event) = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
                    NSEventType::ApplicationDefined,
                    NSPoint::new(0., 0.),
                    NSEventModifierFlags::empty(),
                    0., 0, None, 0, 0, 0,
                ) {
                    app.postEvent_atStart(&event, true);
                }
            }
        }

        if !from_host {
            this.host.notify_destroyed();
        }
    }

    pub fn set_parent(this: ViewRef<Self>, new_parent: Retained<NSView>) {
        if this.state.closed.get() {
            return;
        }
        let previous_parenting = this.parenting.replace(ViewParentingType::Uninitialized);
        previous_parenting.teardown();

        let parenting = ViewParentingType::Parented { parent_view: Weak::from(new_parent) };
        parenting.setup(this.view);

        this.parenting.replace(parenting);
        Self::reanchor_to_superview_top(this);
        Self::request_frame(this);
    }

    pub fn resize(this: ViewRef<Self>, size: Size, notify_host: bool, from_window: bool) {
        if this.state.closed.get() {
            return;
        }
        let size = size.to_logical::<f64>(this.view.backing_scale_factor());
        // NOTE: macOS gives you a personal rave if you pass in fractional pixels here. Even
        // though the size is in fractional pixels.
        let size = NSSize::new(size.width.round(), size.height.round());

        // setFrameSize: is also the host-initiated resize hook. Suppress its
        // notification during our own resize so notify_host is honored.
        struct Resizing<'a>(&'a Cell<bool>);
        impl Drop for Resizing<'_> {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        this.resizing.set(true);
        let resizing = Resizing(&this.resizing);
        this.view.setFrameSize(size);
        drop(resizing);
        this.view.setNeedsDisplay(true);

        // When using OpenGL the `NSOpenGLView` needs to be resized separately? Why? Because
        // macOS.
        #[cfg(feature = "opengl")]
        if let Some(gl_context) = this.gl_context.get() {
            gl_context.resize(size);
        }

        if !from_window {
            // If this is a standalone window then we'll also need to resize the window itself
            if let ViewParentingType::Windowed { owned_window } = &*this.parenting.borrow() {
                if let Some(owned_window) = owned_window.load() {
                    owned_window.setContentSize(size);
                }
            }
        }

        Self::reanchor_to_superview_top(this);
        Self::view_did_change_backing_properties(this, notify_host);
        Self::request_frame(this);
    }

    /// Trigger the event immediately and return the event status.
    pub(crate) fn trigger_event(this: ViewRef<Self>, event: Event) -> EventStatus {
        if this.state.closed.get() || !this.attached.get() {
            return EventStatus::Ignored;
        }
        match &event {
            Event::Window(WindowEvent::Focused) => this.ime.focused.set(true),
            Event::Window(WindowEvent::Unfocused | WindowEvent::WillClose) => {
                // Disable synchronously before dispatch; AppKit can reenter
                // insertText/setMarkedText before the adapter's next frame.
                this.ime.focused.set(false);
                this.ime.marked.borrow_mut().clear();
            }
            _ => {}
        }
        let status = this.window_handler.use_handler(|h| h.on_event(event));
        if status.is_none() && this.ready.get() {
            Self::close(this, false);
            return EventStatus::Ignored;
        }
        let status = status.unwrap_or(EventStatus::Ignored);
        this.frame_requested.set(true);
        Self::update_frame_demand(this);
        status
    }

    fn drawable(this: ViewRef<Self>) -> bool {
        if this.state.closed.get()
            || !this.ready.get()
            || !this.attached.get()
            || this.view.isHiddenOrHasHiddenAncestor()
        {
            return false;
        }
        let size = this.view.bounds().size;
        if !size.width.is_finite()
            || !size.height.is_finite()
            || size.width <= 0.0
            || size.height <= 0.0
        {
            return false;
        }
        this.view.window().is_some_and(|window| {
            window.isVisible()
                && !window.isMiniaturized()
                && window.occlusionState().contains(NSWindowOcclusionState::Visible)
        })
    }

    fn stop_frame_driver(this: ViewRef<Self>) {
        if let Some(link) = this.display_link.take() {
            link.invalidate();
        }
        this.frame_timer.take();
        this.deadline_timer.take();
        this.armed_deadline.set(None);
    }

    fn update_frame_demand(this: ViewRef<Self>) {
        if this.state.closed.get() {
            return;
        }
        let demand = this.window_handler.use_handler(|h| h.frame_demand());
        if demand.is_none() && this.ready.get() {
            Self::close(this, false);
            return;
        }
        this.demand.set(demand.unwrap_or(FrameDemand::Idle));
        Self::sync_frame_driver(this);
    }

    fn sync_frame_driver(this: ViewRef<Self>) {
        match pacing(
            this.demand.get(),
            this.frame_requested.get(),
            Self::drawable(this),
            Instant::now(),
        ) {
            Pace::Stopped => Self::stop_frame_driver(this),
            Pace::Refresh => {
                this.deadline_timer.take();
                this.armed_deadline.set(None);
                let link = this.display_link.take();
                let timer = this.frame_timer.take();
                let running = link.is_some() || timer.is_some();
                this.display_link.set(link);
                this.frame_timer.set(timer);
                if running {
                    return;
                }
                let view: &NSView = this.view;
                if view.respondsToSelector(sel!(displayLinkWithTarget:selector:)) {
                    let link = unsafe {
                        view.displayLinkWithTarget_selector(view, sel!(mooseDisplayLinkFired:))
                    };
                    unsafe {
                        link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes)
                    };
                    this.display_link.set(Some(link));
                } else {
                    // Older macOS uses a main-run-loop timer, NOT a CVDisplayLink worker.
                    let hz = this
                        .view
                        .window()
                        .and_then(|w| w.screen())
                        .or_else(|| NSScreen::mainScreen(this.mtm))
                        .filter(|screen| screen.respondsToSelector(sel!(maximumFramesPerSecond)))
                        .map(|screen| screen.maximumFramesPerSecond() as f64);
                    let timer_view = Weak::new(this.view);
                    this.frame_timer.set(TimerHandle::new(
                        frame_interval(hz).as_secs_f64(),
                        move || {
                            if let Some(view) = timer_view.load() {
                                if let Some(inner) = view.inner_ref() {
                                    Self::trigger_frame(inner);
                                }
                            }
                        },
                    ));
                }
            }
            Pace::Deadline(deadline) => {
                if this.armed_deadline.get() == Some(deadline) {
                    return;
                }
                Self::stop_frame_driver(this);
                this.armed_deadline.set(Some(deadline));
                let timer_view = Weak::new(this.view);
                this.deadline_timer.set(TimerHandle::once(
                    deadline.saturating_duration_since(Instant::now()).as_secs_f64(),
                    move || {
                        if let Some(view) = timer_view.load() {
                            if let Some(inner) = view.inner_ref() {
                                // Reject an already queued callback from a replaced/invalidated timer.
                                if deadline_can_fire(
                                    inner.state.closed.get(),
                                    inner.armed_deadline.get(),
                                    deadline,
                                ) {
                                    inner.deadline_timer.take();
                                    inner.armed_deadline.set(None);
                                    Self::request_frame(inner);
                                }
                            }
                        }
                    },
                ));
            }
        }
    }

    pub(crate) fn request_frame(this: ViewRef<Self>) {
        if this.state.closed.get() {
            return;
        }
        this.frame_requested.set(true);
        Self::sync_frame_driver(this);
    }

    pub(crate) fn frame_requester(this: ViewRef<Self>) -> FrameRequester {
        let wake = Arc::clone(&this.wake);
        let id = this.view_id;
        FrameRequester::new(move || {
            if !wake.queue() {
                return;
            }
            let wake = Arc::clone(&wake);
            dispatch2::DispatchQueue::main().exec_async(move || {
                callback("frame request on main queue", (), || {
                    if !wake.drain() {
                        return;
                    }
                    let view = VIEWS
                        .try_with(|views| views.borrow().get(&id).and_then(Weak::load))
                        .ok()
                        .flatten();
                    if let Some(view) = view {
                        if let Some(inner) = view.inner_ref() {
                            Self::request_frame(inner);
                        }
                    }
                });
            });
        })
    }

    fn trigger_frame(this: ViewRef<Self>) {
        if pacing(
            this.demand.get(),
            this.frame_requested.get(),
            Self::drawable(this),
            Instant::now(),
        ) != Pace::Refresh
        {
            Self::sync_frame_driver(this);
            return;
        }
        this.frame_requested.set(false);
        match this.window_handler.use_handler(|h| h.on_frame()) {
            Some(Ok(())) => Self::update_frame_demand(this),
            Some(Err(error)) => {
                Self::close(this, false);
                warn!("Error while rendering frame: {}", error);
            }
            None => Self::close(this, false),
        }
    }

    fn sync_layer(this: ViewRef<Self>) {
        if let Some(layer) = this.view.layer() {
            // AppKit owns backing-layer geometry; wgpu owns drawableSize and device.
            // CGFloat setters are gated on a bindings feature we don't need
            // otherwise. The Objective-C signature is setContentsScale:(CGFloat).
            unsafe {
                let () = msg_send![&*layer, setContentsScale: this.view.backing_scale_factor()];
            }
            layer.setOpaque(true);
            // Use AppKit's resolved window background, not transparent/black, until
            // the first drawable or CPU image is presented. No renderer owns this colour.
            if let Some(class) = objc2::runtime::AnyClass::get(c"NSColor") {
                let colour: Option<Retained<objc2::runtime::AnyObject>> =
                    unsafe { msg_send![class, windowBackgroundColor] };
                if let Some(colour) = colour {
                    unsafe {
                        let cg: *const std::ffi::c_void = msg_send![&*colour, CGColor];
                        let () = msg_send![&*layer, setBackgroundColor: cg];
                    }
                }
            }
        }
    }

    fn reanchor_to_superview_top(this: ViewRef<Self>) {
        let Ok(parenting) = this.parenting.try_borrow() else {
            return;
        };
        if !matches!(*parenting, ViewParentingType::Parented { .. }) {
            return;
        }
        drop(parenting);
        // A standalone content view belongs to AppKit's frame/titlebar layout.
        if let Some(parent) = unsafe { this.view.superview() } {
            let bounds = parent.bounds();
            let frame = this.view.frame();
            let y = top_origin(
                parent.isFlipped(),
                bounds.origin.y,
                bounds.size.height,
                frame.size.height,
            );
            if y.is_finite() && (frame.origin.y - y).abs() > f64::EPSILON {
                this.view.setFrameOrigin(NSPoint::new(frame.origin.x, y));
            }
        }
    }

    fn remove_tracking_area(this: ViewRef<Self>) {
        if let Some(area) = this.tracking_area.take() {
            this.view.removeTrackingArea(&area);
        }
    }

    fn detach_callbacks(this: ViewRef<Self>) {
        let had_focus = this.ime.focused.get();
        this.attached.set(false);
        Self::stop_frame_driver(this);
        this.notification_center_observer.take();
        Self::remove_tracking_area(this);
        Self::release_focus(this);
        this.ime.focused.set(false);
        this.ime.marked.borrow_mut().clear();
        this.cursor_manager.set_is_inside(false);
        if had_focus && !this.state.closed.get() {
            this.window_handler.use_handler(|h| h.on_event(Event::Window(WindowEvent::Unfocused)));
            Self::update_frame_demand(this);
        }
    }

    fn take_focus(this: ViewRef<Self>) {
        if this.state.closed.get() || !this.attached.get() {
            return;
        }
        if let Some(window) = this.view.window() {
            if !window.firstResponder().is_some_and(|responder| this.view.isEqual(Some(&responder)))
            {
                this.previous_responder
                    .set(window.firstResponder().map(|r| Weak::from_retained(&r)));
                window.makeFirstResponder(Some(this.view));
            }
        }
    }

    fn release_focus(this: ViewRef<Self>) {
        let previous = this.previous_responder.take().and_then(|r| r.load());
        if let Some(window) = this.view.window() {
            if window.firstResponder().is_some_and(|responder| this.view.isEqual(Some(&responder)))
            {
                window.makeFirstResponder(previous.as_deref());
            }
        }
    }

    pub(crate) fn set_keyboard_capture(this: ViewRef<Self>, capture: bool) {
        if this.state.closed.get() || this.keyboard_capture.replace(capture) == capture {
            return;
        }
        if capture {
            Self::take_focus(this);
        } else {
            Self::release_focus(this);
        }
    }

    fn apply_size_constraints(this: ViewRef<Self>) {
        let ViewParentingType::Windowed { owned_window } = &*this.parenting.borrow() else {
            return;
        };
        let Some(window) = owned_window.load() else { return };
        let scale_factor = window.backingScaleFactor();

        if let Some(min_size) = this.state.sizing_strategy.min_size() {
            let min_size = min_size.to_logical(scale_factor);
            window.setContentMinSize(NSSize::new(min_size.width, min_size.height));
        }

        if let Some(max_size) = this.state.sizing_strategy.max_size() {
            let max_size = max_size.to_logical(scale_factor);
            window.setContentMaxSize(NSSize::new(max_size.width, max_size.height));
        }
    }
}

impl Drop for BaseviewView {
    fn drop(&mut self) {
        self.state.closed.set(true);
        self.wake.revoke();
        let _ = VIEWS.try_with(|views| views.borrow_mut().remove(&self.view_id));
        if let Some(link) = self.display_link.take() {
            link.invalidate();
        }
        self.frame_timer.take();
        self.deadline_timer.take();
        self.notification_center_observer.take();
        self.window_handler.destroy();
        #[cfg(debug_assertions)]
        {
            let remaining = LIVE_VIEWS.fetch_sub(1, Ordering::Relaxed).saturating_sub(1);
            crate::tracing::debug!("AppKit view deallocated; live views: {}", remaining);
        }
    }
}

impl ViewImpl for BaseviewView {
    fn callbacks_revoked(this: ViewRef<Self>) -> bool {
        this.state.closed.get()
    }
    fn initialization_failed(this: ViewRef<Self>) {
        Self::close(this, true);
    }
    fn resized_by_appkit(this: ViewRef<Self>) {
        if this.resizing.get() {
            return;
        }
        Self::reanchor_to_superview_top(this);
        Self::view_did_change_backing_properties(this, true);
        Self::request_frame(this);
    }
    fn update_layer(this: ViewRef<Self>) {
        Self::sync_layer(this);
        Self::sync_frame_driver(this);
    }
    fn visibility_changed(this: ViewRef<Self>) {
        // A hidden idle editor needs a first frame when AppKit reveals it again.
        Self::request_frame(this);
    }
    fn view_did_move_to_window(this: ViewRef<Self>) {
        let Some(window) = this.view.window() else { return };
        this.attached.set(true);
        Self::view_did_change_backing_properties(this, false);
        Self::update_tracking_areas(this);
        let notifier_view = Weak::new(this.view);
        this.notification_center_observer.set(Some(
            NotificationCenterObserver::register_window_changes(&window, move |notification| {
                if let Some(view) = notifier_view.load() {
                    if let Some(inner) = view.inner_ref() {
                        if !inner.state.closed.get() && inner.attached.get() {
                            Self::handle_notification(inner, notification);
                        }
                    }
                }
            }),
        ));
        Self::request_frame(this);
    }

    fn become_first_responder(this: ViewRef<Self>) -> bool {
        let Some(window) = this.view.window() else {
            return true;
        };

        if window.isKeyWindow() {
            Self::trigger_event(this, Event::Window(WindowEvent::Focused));
        }

        true
    }

    fn resign_first_responder(this: ViewRef<Self>) -> bool {
        Self::trigger_event(this, Event::Window(WindowEvent::Unfocused));
        true
    }

    fn window_should_close(this: ViewRef<Self>) -> bool {
        Self::close(this, false);

        true
    }

    fn window_did_resize(this: ViewRef<Self>) {
        let Some(window) = this.view.window() else { return };

        let size = window.contentRectForFrameRect(window.frame()).size;
        let size = LogicalSize::new(size.width, size.height);

        BaseviewView::resize(this, size.into(), true, true);
    }

    fn view_did_change_backing_properties(this: ViewRef<Self>, notify_host: bool) {
        if this.state.closed.get() {
            return;
        }
        Self::sync_layer(this);
        let current_size = this.view.size();
        let current_scale_factor = this.view.backing_scale_factor();

        let scale_changed = this.state.scale_factor.get() != current_scale_factor;
        if scale_changed {
            Self::apply_size_constraints(this);
        }

        // Only send the event when the window's size has actually changed to be in line with the
        // other platform implementations
        if this.state.scale_factor.get() != current_scale_factor
            || this.state.size.get() != current_size
        {
            let previous = this.state.size.replace(current_size);
            this.state.scale_factor.set(current_scale_factor);
            let new_size = WindowSize::from_logical(current_size, current_scale_factor);

            let result = this.window_handler.use_handler(|h| h.resized(new_size));
            Self::update_frame_demand(this);
            if this.state.closed.get() {
                return;
            }

            if let Some(Err(e)) = result {
                warn!("Window Handler failed to resize: {}", e);
                this.state.size.set(previous);

                Self::resize(this, previous.into(), false, false);
                return;
            }

            if notify_host {
                if let Err(e) = this.host.request_resize(new_size) {
                    warn!("Host failed to resize parent view: {}", e);

                    Self::resize(this, previous.into(), false, false);
                }
            }

            if scale_changed {
                Self::trigger_event(
                    this,
                    Event::Window(WindowEvent::ScaleFactorChanged(current_scale_factor)),
                );
            }
        }
    }

    /// `hitTest:` override that collapses hits on baseview's internal
    /// OpenGL render subview to this NSView.
    ///
    /// `src/gl/gl` attaches an `NSOpenGLView` as a subview of this
    /// view so the GL context is isolated from event handling. The side
    /// effect is that `[NSView hitTest:]` returns the GL subview for
    /// every click inside our frame — `NSOpenGLView` inherits the
    /// default `acceptsFirstMouse:` which returns `NO`, so AppKit treats
    /// the first click in a non-key window as an activation click and
    /// never dispatches `mouseDown:`. That's the "first click dead zone"
    /// symptom reported in baseview#129 / #202 / #169.
    ///
    /// Fix: if the hit lands on our own GL render subview (pointer
    /// equality against the `NSOpenGLView` stored in `GlContext`),
    /// collapse the result to `self`. AppKit then asks US about
    /// `acceptsFirstMouse:` (we return `YES`), and `mouseDown:` is
    /// dispatched on the first click. Hits on any other subview pass
    /// through unchanged — we only redirect our own render child, not
    /// anything the consumer may add.
    ///
    /// No-op without the `opengl` feature: there's no GL subview to
    /// collapse, so the override pass-through is equivalent to the
    /// default implementation.
    fn hit_test(this: ViewRef<'_, Self>, point: NSPoint) -> Option<&NSView> {
        let superclass = NSView::class();

        // SAFETY: Our superclass is NSView
        let super_result: Option<&NSView> =
            unsafe { msg_send![super(this.view, superclass), hitTest: point] };
        let super_result = super_result?;

        #[cfg(feature = "opengl")]
        {
            if let Some(gl_context) = this.gl_context.get() {
                if *super_result == **gl_context.view {
                    return Some(this.view);
                }
            }
        }

        Some(super_result)
    }

    fn view_will_move_to_window(this: ViewRef<Self>, new_window: Option<&NSWindow>) {
        // This may be a temporary detach/reparent, not an editor close.
        // Stop all callbacks before the old window and handler can go away.
        Self::detach_callbacks(this);
        unsafe {
            let () = msg_send![super(this.view, NSView::class()), viewWillMoveToWindow: new_window];
        }
        // Do not become first responder or enable mouseMoved on the DAW window.
        // viewDidMoveToWindow establishes tracking/observers for the new window.
    }

    fn update_tracking_areas(this: ViewRef<Self>) {
        Self::remove_tracking_area(this);
        if this.state.closed.get() || !this.attached.get() {
            return;
        }
        let tracking_area = new_tracking_area(this.view);
        this.view.addTrackingArea(&tracking_area);
        this.tracking_area.set(Some(tracking_area));
    }

    fn mouse_moved(this: ViewRef<Self>, event: &NSEvent) {
        let point = this.view.convertPoint_fromView(event.locationInWindow(), None);

        let position = LogicalPosition { x: point.x, y: point.y };

        Self::trigger_event(
            this,
            Event::Mouse(MouseEvent::CursorMoved {
                position: position.to_physical(this.state.scale_factor.get()),
                modifiers: make_modifiers(event.modifierFlags()),
            }),
        );

        // SAFETY: Our superclass is NSView
        let _: () = unsafe { msg_send![super(this.view, NSView::class()), mouseMoved: event] };
    }

    fn scroll_wheel(this: ViewRef<Self>, event: &NSEvent) {
        let x = event.scrollingDeltaX() as f32;
        let y = event.scrollingDeltaY() as f32;

        let delta = if event.hasPreciseScrollingDeltas() {
            ScrollDelta::Pixels { x, y }
        } else {
            ScrollDelta::Lines { x, y }
        };

        Self::trigger_event(
            this,
            Event::Mouse(MouseEvent::WheelScrolled {
                delta,
                modifiers: make_modifiers(event.modifierFlags()),
            }),
        );
    }

    fn dragging_entered(
        this: ViewRef<Self>, sender: Option<&ProtocolObject<dyn NSDraggingInfo>>,
    ) -> NSDragOperation {
        let modifiers = this.keyboard_state.last_mods();
        let drop_data = get_drop_data(sender);

        let event = MouseEvent::DragEntered {
            position: get_drag_position(sender).to_physical(this.view.backing_scale_factor()),
            modifiers: make_modifiers(modifiers),
            data: drop_data,
        };

        on_event(this, event)
    }

    fn dragging_updated(
        this: ViewRef<Self>, sender: Option<&ProtocolObject<dyn NSDraggingInfo>>,
    ) -> NSDragOperation {
        let modifiers = this.keyboard_state.last_mods();
        let drop_data = get_drop_data(sender);

        let event = MouseEvent::DragMoved {
            position: get_drag_position(sender).to_physical(this.view.backing_scale_factor()),
            modifiers: make_modifiers(modifiers),
            data: drop_data,
        };

        on_event(this, event)
    }

    fn prepare_for_drag_operation(
        _this: ViewRef<Self>, _sender: Option<&ProtocolObject<dyn NSDraggingInfo>>,
    ) -> bool {
        // Always accept drag operation if we get this far
        // This function won't be called unless dragging_entered/updated
        // has returned an acceptable operation
        true
    }

    fn perform_drag_operation(
        this: ViewRef<Self>, sender: Option<&ProtocolObject<dyn NSDraggingInfo>>,
    ) -> bool {
        let modifiers = this.keyboard_state.last_mods();
        let drop_data = get_drop_data(sender);

        let event = MouseEvent::DragDropped {
            position: get_drag_position(sender).to_physical(this.view.backing_scale_factor()),
            modifiers: make_modifiers(modifiers),
            data: drop_data,
        };

        let event_status = Self::trigger_event(this, Event::Mouse(event));

        matches!(event_status, EventStatus::AcceptDrop(_))
    }

    fn dragging_exited(this: ViewRef<Self>, _sender: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
        on_event(this, MouseEvent::DragLeft);
    }

    fn handle_notification(this: ViewRef<Self>, notification: &NSNotification) {
        let Some(window) = this.view.window() else { return };
        // The subject of the notification, in this case an NSWindow object.
        let Some(notification_object) = notification.object().and_then(|o| o.downcast().ok())
        else {
            return;
        };

        // Only trigger focus events if the NSWindow that's being notified about is our window,
        // and if the window's first responder is our NSView.
        if window != notification_object {
            return;
        }

        if &*notification.name() == unsafe { NSWindowWillCloseNotification } {
            Self::close(this, false);
            return;
        }
        Self::view_did_change_backing_properties(this, false);
        Self::request_frame(this);
        if &*notification.name() != unsafe { NSWindowDidBecomeKeyNotification }
            && &*notification.name() != unsafe { NSWindowDidResignKeyNotification }
        {
            return;
        }
        let Some(first_responder) = window.firstResponder() else { return };

        // If the first responder isn't our NSView, the focus events will instead be triggered
        // by the becomeFirstResponder and resignFirstResponder methods on the NSView itself.
        if !this.view.isEqual(Some(&first_responder)) {
            return;
        }

        Self::trigger_event(
            this,
            Event::Window(if window.isKeyWindow() {
                WindowEvent::Focused
            } else {
                WindowEvent::Unfocused
            }),
        );
    }

    fn mouse_down(this: ViewRef<Self>, event: &NSEvent) {
        Self::take_focus(this);
        Self::trigger_event(
            this,
            Event::Mouse(ButtonPressed {
                button: MouseButton::Left,
                modifiers: make_modifiers(event.modifierFlags()),
            }),
        );
    }

    fn mouse_up(this: ViewRef<Self>, event: &NSEvent) {
        Self::trigger_event(
            this,
            Event::Mouse(ButtonReleased {
                button: MouseButton::Left,
                modifiers: make_modifiers(event.modifierFlags()),
            }),
        );
    }

    fn right_mouse_down(this: ViewRef<Self>, event: &NSEvent) {
        Self::take_focus(this);
        Self::trigger_event(
            this,
            Event::Mouse(ButtonPressed {
                button: MouseButton::Right,
                modifiers: make_modifiers(event.modifierFlags()),
            }),
        );
    }

    fn right_mouse_up(this: ViewRef<Self>, event: &NSEvent) {
        Self::trigger_event(
            this,
            Event::Mouse(ButtonReleased {
                button: MouseButton::Right,
                modifiers: make_modifiers(event.modifierFlags()),
            }),
        );
    }

    fn other_mouse_down(this: ViewRef<Self>, event: &NSEvent) {
        Self::take_focus(this);
        Self::trigger_event(
            this,
            Event::Mouse(ButtonPressed {
                button: MouseButton::Middle,
                modifiers: make_modifiers(event.modifierFlags()),
            }),
        );
    }

    fn other_mouse_up(this: ViewRef<Self>, event: &NSEvent) {
        Self::trigger_event(
            this,
            Event::Mouse(ButtonReleased {
                button: MouseButton::Middle,
                modifiers: make_modifiers(event.modifierFlags()),
            }),
        );
    }

    fn mouse_entered(this: ViewRef<Self>) {
        this.cursor_manager.set_is_inside(true);
        Self::trigger_event(this, Event::Mouse(MouseEvent::CursorEntered));
    }

    fn mouse_exited(this: ViewRef<Self>) {
        this.cursor_manager.set_is_inside(false);
        Self::trigger_event(this, Event::Mouse(MouseEvent::CursorLeft));
    }

    fn cursor_update(this: ViewRef<Self>, event: Option<&NSEvent>) -> bool {
        let Some(event) = event else { return false };
        let point = this.view.convertPoint_fromView(event.locationInWindow(), None);
        if NSPointInRect(point, this.view.bounds()) {
            this.cursor_manager.update_to_current_cursor();
            true
        } else {
            false
        }
    }

    fn has_marked_text(this: ViewRef<Self>) -> bool {
        this.ime.enabled() && !this.ime.marked.borrow().is_empty()
    }
    fn marked_range(this: ViewRef<Self>) -> objc2_foundation::NSRange {
        this.ime.marked_range()
    }
    fn selected_range(this: ViewRef<Self>) -> objc2_foundation::NSRange {
        this.ime.selection()
    }
    fn insert_text(
        this: ViewRef<Self>, text: &objc2::runtime::AnyObject,
        replacement: objc2_foundation::NSRange,
    ) {
        if !this.ime.enabled() {
            return;
        }
        let owner = this.ime.configuration.borrow().as_ref().map(|c| c.id.clone());
        let replacement_event = this.ime.replacement(replacement);
        let text = super::ime::text(text);
        this.ime.commit(&text, replacement);
        if let Some(event) = replacement_event {
            Self::trigger_event(this, Event::Ime(event));
        }
        if this.ime.enabled()
            && this.ime.configuration.borrow().as_ref().map(|c| c.id.clone()) == owner
        {
            Self::trigger_event(this, Event::Ime(crate::Ime::Commit(text)));
        }
    }
    fn set_marked_text(
        this: ViewRef<Self>, text: &objc2::runtime::AnyObject, selected: objc2_foundation::NSRange,
        replacement: objc2_foundation::NSRange,
    ) {
        if !this.ime.enabled() {
            return;
        }
        let owner = this.ime.configuration.borrow().as_ref().map(|c| c.id.clone());
        let replacement_event = this.ime.replacement(replacement);
        let event = this.ime.mark(super::ime::text(text), selected, replacement);
        if let Some(replacement) = replacement_event {
            Self::trigger_event(this, Event::Ime(replacement));
        }
        if this.ime.enabled()
            && this.ime.configuration.borrow().as_ref().map(|c| c.id.clone()) == owner
        {
            Self::trigger_event(this, Event::Ime(event));
        }
    }
    fn unmark_text(this: ViewRef<Self>) {
        if !this.ime.enabled() {
            return;
        }
        // Cocoa accepts marked text on unmark. Cancellation uses discardMarkedText.
        let text = std::mem::take(&mut *this.ime.marked.borrow_mut());
        if !text.is_empty() {
            this.ime.commit(&text, objc2_foundation::NSRange::new(crate::ime::NS_NOT_FOUND, 0));
            Self::trigger_event(this, Event::Ime(crate::Ime::Commit(text)));
        }
    }
    fn first_rect(
        this: ViewRef<Self>, _: objc2_foundation::NSRange, actual: *mut objc2_foundation::NSRange,
    ) -> NSRect {
        if !actual.is_null() {
            unsafe {
                actual.write(this.ime.selection());
            }
        }
        let config = this.ime.configuration.borrow().clone();
        let Some(c) = config else {
            return NSRect::ZERO;
        };
        let scale = this.state.scale_factor.get();
        let local = NSRect::new(
            NSPoint::new(c.position.x / scale, c.position.y / scale),
            NSSize::new(c.size.width / scale, c.size.height / scale),
        );
        let in_window = this.view.convertRect_toView(local, None);
        this.view.window().map_or(in_window, |window| window.convertRectToScreen(in_window))
    }
    fn attributed_substring(
        this: ViewRef<Self>, range: objc2_foundation::NSRange,
        actual: *mut objc2_foundation::NSRange,
    ) -> Option<Retained<objc2_foundation::NSAttributedString>> {
        let shadow = this.ime.shadow.borrow().clone()?;
        if range.location >= crate::ime::NS_NOT_FOUND {
            return None;
        }
        let start = crate::ime::utf16_to_byte(&shadow.text, range.location);
        let end =
            crate::ime::utf16_to_byte(&shadow.text, range.location.saturating_add(range.length));
        let text = shadow.text.get(start..end)?;
        if !actual.is_null() {
            unsafe {
                actual.write(objc2_foundation::NSRange::new(
                    shadow.text[..start].encode_utf16().count(),
                    text.encode_utf16().count(),
                ));
            }
        }
        Some(objc2_foundation::NSAttributedString::initWithString(
            objc2_foundation::NSAttributedString::alloc(),
            &NSString::from_str(text),
        ))
    }
    fn do_command(this: ViewRef<Self>, _: objc2::runtime::Sel) {
        let key = this.ime.pending_key.borrow().clone();
        if let Some(key) = key {
            Self::trigger_event(this, Event::Keyboard(key));
        }
    }
    fn key_down(this: ViewRef<Self>, event: &NSEvent) {
        if this.ime.enabled() {
            if let Some(key) = this.keyboard_state.process_native_event(event) {
                if !key.modifiers.intersects(
                    keyboard_types::Modifiers::META | keyboard_types::Modifiers::CONTROL,
                ) {
                    *this.ime.pending_key.borrow_mut() = Some(key);
                    unsafe {
                        let _: () = msg_send![this.view, interpretKeyEvents: &*NSArray::from_slice(&[event])];
                    }
                    this.ime.pending_key.borrow_mut().take();
                    return;
                }
                let status = Self::trigger_event(this, Event::Keyboard(key));
                if status == EventStatus::Ignored {
                    unsafe {
                        let superclass = NSView::class();
                        let _: () = msg_send![super(this.view, superclass), keyDown:event];
                    }
                }
                return;
            }
        }
        if let Some(key_event) = this.keyboard_state.process_native_event(event) {
            let status = Self::trigger_event(this, Event::Keyboard(key_event));

            if let EventStatus::Ignored = status {
                unsafe {
                    let () = msg_send![super(this.view, NSView::class()), keyDown:event];
                }
            }
        }
    }

    fn key_up(this: ViewRef<Self>, event: &NSEvent) {
        if let Some(key_event) = this.keyboard_state.process_native_event(event) {
            let status = Self::trigger_event(this, Event::Keyboard(key_event));

            if let EventStatus::Ignored = status {
                unsafe {
                    let () = msg_send![super(this.view, NSView::class()), keyUp:event];
                }
            }
        }
    }

    fn flags_changed(this: ViewRef<Self>, event: &NSEvent) {
        if let Some(key_event) = this.keyboard_state.process_native_event(event) {
            let status = Self::trigger_event(this, Event::Keyboard(key_event));

            if let EventStatus::Ignored = status {
                unsafe {
                    let () = msg_send![super(this.view, NSView::class()), flagsChanged:event];
                }
            }
        }
    }

    fn display_link_fired(this: ViewRef<Self>) {
        Self::trigger_frame(this);
    }
}

/// Info:
/// https://developer.apple.com/documentation/appkit/nstrackingarea
/// https://developer.apple.com/documentation/appkit/nstrackingarea/options
/// https://developer.apple.com/documentation/appkit/nstrackingareaoptions.
fn new_tracking_area(this: &NSView) -> Retained<NSTrackingArea> {
    let options = NSTrackingAreaOptions::MouseEnteredAndExited
        | NSTrackingAreaOptions::MouseMoved
        | NSTrackingAreaOptions::CursorUpdate
        //| NSTrackingAreaOptions::ActiveInActiveApp
        | NSTrackingAreaOptions::ActiveInKeyWindow
        | NSTrackingAreaOptions::InVisibleRect
        | NSTrackingAreaOptions::EnabledDuringMouseDrag;

    // SAFETY: `this` is of the correct type (NSView)
    unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
            NSTrackingArea::alloc(),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)),
            options,
            Some(this),
            None,
        )
    }
}

fn get_drag_position(sender: Option<&ProtocolObject<dyn NSDraggingInfo>>) -> LogicalPosition<f64> {
    let point = match sender {
        Some(sender) => sender.draggingLocation(),
        None => NSPoint::ZERO,
    };

    LogicalPosition::new(point.x, point.y)
}

fn get_drop_data(sender: Option<&ProtocolObject<dyn NSDraggingInfo>>) -> DropData {
    let Some(sender) = sender else {
        return DropData::None;
    };

    let pasteboard = sender.draggingPasteboard();
    let Some(file_list) = pasteboard.propertyListForType(unsafe { NSFilenamesPboardType }) else {
        return DropData::None;
    };

    let Ok(file_list) = file_list.downcast::<NSArray>() else {
        return DropData::None;
    };

    let files = file_list
        .into_iter()
        .filter_map(|s| s.downcast::<NSString>().ok())
        .map(|s| s.to_string().into())
        .collect();

    DropData::Files(files)
}

fn on_event(this: ViewRef<BaseviewView>, event: MouseEvent) -> NSDragOperation {
    let event_status = BaseviewView::trigger_event(this, Event::Mouse(event));
    match event_status {
        EventStatus::AcceptDrop(DropEffect::Copy) => NSDragOperation::Copy,
        EventStatus::AcceptDrop(DropEffect::Move) => NSDragOperation::Move,
        EventStatus::AcceptDrop(DropEffect::Link) => NSDragOperation::Link,
        EventStatus::AcceptDrop(DropEffect::Scroll) => NSDragOperation::Generic,
        _ => NSDragOperation::None,
    }
}

pub struct WindowHandlerContainer {
    slot: HandlerSlot<Box<dyn WindowHandler>>,
}

impl WindowHandlerContainer {
    pub fn new() -> Self {
        Self {
            slot: HandlerSlot::new(
                |handler| {
                    callback("WillClose", (), || {
                        handler.on_event(Event::Window(WindowEvent::WillClose));
                    });
                },
                |handler| {
                    callback("drop window handler", (), || drop(handler));
                },
            ),
        }
    }
    pub fn use_handler<T>(&self, user: impl FnOnce(&dyn WindowHandler) -> T) -> Option<T> {
        // A panic quarantines the handler, rather than retrying it at refresh rate.
        // The extra Option distinguishes a panic from an absent initial handler.
        match callback("window handler", None, || {
            Some(self.slot.with(|handler| user(handler.as_ref())))
        }) {
            Some(result) => result,
            None => {
                self.slot.close();
                None
            }
        }
    }
    pub fn set(&self, handler: Box<dyn WindowHandler>) {
        self.slot.set(handler);
    }
    pub fn destroy(&self) {
        self.slot.close();
    }
}
