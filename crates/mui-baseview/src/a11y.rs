//! Native accessibility for the editor's own window, including embedded views.
//!
//! Activation and actions only touch atomics/channels. Build updates while the
//! model is locked, then publish after releasing it: UIA and NSAccessibility
//! notifications can synchronously reenter the host's window procedure.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use mui::{SemanticAction, Ui};
use mui_access::Publisher;
#[cfg(target_os = "linux")]
use mui_access::accesskit::DeactivationHandler;
use mui_access::accesskit::{
    Action, ActionData, ActionHandler, ActionRequest, ActivationHandler, TreeUpdate,
};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
#[cfg(target_os = "linux")]
mod x11;

#[cfg(target_os = "linux")]
type NativeAdapter = accesskit_unix::Adapter;
#[cfg(target_os = "macos")]
type NativeAdapter = accesskit_macos::SubclassingAdapter;
#[cfg(target_os = "windows")]
type NativeAdapter = accesskit_windows::SubclassingAdapter;

/// Native accessibility adapter, confined to the window's owning thread.
/// Publish notifications outside model locks and mutable UI borrows: they can
/// synchronously reenter the native window procedure.
pub struct NativeAccessibility {
    adapter: NativeAdapter,
    // Match the native window's thread confinement on every platform.
    thread: std::marker::PhantomData<std::rc::Rc<()>>,
    alive: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    asked: Arc<AtomicBool>,
    #[cfg(target_os = "linux")]
    bounds: x11::BoundsPoll,
}

/// The portable accessibility endpoint. It may move to the thread that owns
/// the UI, while [`NativeAccessibility`] stays on the native window thread.
/// Activation and action callbacks only touch shared atomics and a channel.
/// After the native endpoint drops, queued and late requests are ignored,
/// `wants_tree` is false, and `prepare` produces no more updates.
pub struct AccessibilityUi {
    alive: Arc<AtomicBool>,
    actions: Receiver<ActionRequest>,
    asked: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    publisher: Publisher,
}

struct Asked {
    alive: Arc<AtomicBool>,
    asked: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
}
impl ActivationHandler for Asked {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        if !self.alive.load(Ordering::Acquire) {
            return None;
        }
        self.active.store(true, Ordering::Release);
        self.asked.store(true, Ordering::Release);
        // No model lock from a native callback. A full tree follows next tick.
        None
    }
}
struct Actions(Sender<ActionRequest>, Arc<AtomicBool>);
impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        // The receiving editor may already have closed. No window/model is
        // retained by this callback, so reopening gets a fresh action queue.
        if self.1.load(Ordering::Acquire) {
            let _ = self.0.send(request);
        }
    }
}
#[cfg(target_os = "linux")]
struct Gone(Arc<AtomicBool>);
#[cfg(target_os = "linux")]
impl DeactivationHandler for Gone {
    fn deactivate_accessibility(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl NativeAccessibility {
    /// Attach accessibility before showing the native window.
    ///
    /// The UI endpoint can be moved to another thread; send its prepared tree
    /// updates back to this native endpoint for publication.
    ///
    /// # Safety
    /// The handle must identify the caller's live native window. Call on its
    /// owning thread, with no model lock or mutable UI
    /// borrow held. Keep this endpoint on that thread and drop it before the
    /// native window is destroyed, including on `WindowEvent::WillClose`.
    #[expect(
        unsafe_code,
        reason = "native adapter requires the caller's window lifetime contract"
    )]
    pub unsafe fn new(handle: RawWindowHandle) -> Option<(Self, AccessibilityUi)> {
        let (send, actions) = channel();
        let alive = Arc::new(AtomicBool::new(true));
        let asked = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicBool::new(false));
        let activation = Asked {
            alive: Arc::clone(&alive),
            asked: Arc::clone(&asked),
            active: Arc::clone(&active),
        };
        #[cfg(target_os = "linux")]
        let adapter = {
            if !matches!(
                handle,
                RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_) | RawWindowHandle::Wayland(_)
            ) {
                return None;
            }
            accesskit_unix::Adapter::new(
                activation,
                Actions(send, Arc::clone(&alive)),
                Gone(Arc::clone(&active)),
            )
        };
        #[cfg(target_os = "macos")]
        let adapter = {
            let RawWindowHandle::AppKit(handle) = handle else {
                return None;
            };
            // SAFETY: the builder supplies baseview's live NSView on the main
            // thread, before focus/show. The adapter retains it until teardown.
            #[expect(unsafe_code, reason = "attach AccessKit to baseview's live NSView")]
            unsafe {
                accesskit_macos::SubclassingAdapter::new(
                    handle.ns_view.as_ptr(),
                    activation,
                    Actions(send, Arc::clone(&alive)),
                )
            }
        };
        #[cfg(target_os = "windows")]
        let adapter = {
            let RawWindowHandle::Win32(handle) = handle else {
                return None;
            };
            let hwnd = accesskit_windows::HWND(handle.hwnd.get() as *mut std::ffi::c_void);
            accesskit_windows::SubclassingAdapter::new(
                hwnd,
                activation,
                Actions(send, Arc::clone(&alive)),
            )
        };
        Some((
            Self {
                adapter,
                thread: std::marker::PhantomData,
                alive: Arc::clone(&alive),
                active: Arc::clone(&active),
                asked: Arc::clone(&asked),
                #[cfg(target_os = "linux")]
                bounds: x11::BoundsPoll::default(),
            },
            AccessibilityUi {
                alive,
                actions,
                asked,
                active,
                publisher: Publisher::default(),
            },
        ))
    }

    /// Called outside the model lock, since native events can reenter.
    pub fn focus(&mut self, focused: bool) {
        #[cfg(target_os = "linux")]
        self.adapter.update_window_focus_state(focused);
        #[cfg(target_os = "macos")]
        if let Some(events) = self.adapter.update_view_focus_state(focused) {
            events.raise();
        }
        // The Windows subclass tracks WM_SETFOCUS/KILLFOCUS and menu/resize
        // modal loops itself, including dropping its borrow before raising.
        #[cfg(target_os = "windows")]
        let _ = focused;
    }

    /// Track the editor's origin, using its original live display connection.
    /// No requests occur while accessibility is inactive; other platforms' native
    /// adapters track their own bounds.
    ///
    /// # Safety
    /// Both handles must be borrowed from the same live native window/display
    /// for this call, on its owning thread, outside model locks/UI borrows.
    #[expect(
        unsafe_code,
        reason = "bounds query borrows the caller's native display"
    )]
    pub unsafe fn update_bounds(&mut self, display: RawDisplayHandle, window: RawWindowHandle) {
        #[cfg(target_os = "linux")]
        {
            if !self.active.load(Ordering::Acquire) {
                self.bounds.pause();
                return;
            }
            let bounds = self.bounds.update(std::time::Instant::now(), || {
                // SAFETY: caller keeps this original connection and window live.
                unsafe { x11::query(display, window) }
            });
            if let Some(bounds) = bounds {
                self.adapter
                    .set_root_window_bounds(bounds.outer, bounds.inner);
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = (display, window);
    }

    /// Publish after releasing the model lock. AccessKit's adapter borrow is
    /// gone before `raise`, permitting nested native accessibility queries.
    pub fn publish(&mut self, update: TreeUpdate) {
        #[cfg(target_os = "linux")]
        self.adapter.update_if_active(|| update);
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(events) = self.adapter.update_if_active(|| update) {
            events.raise();
        }
    }
}

impl Drop for NativeAccessibility {
    fn drop(&mut self) {
        // Invalidate before the adapter unregisters: its final callbacks may
        // reenter, while a panel's portable endpoint still lives elsewhere.
        self.alive.store(false, Ordering::Release);
        self.active.store(false, Ordering::Release);
        self.asked.store(false, Ordering::Release);
    }
}

impl AccessibilityUi {
    /// A native provider asked for a full tree; redraw even an idle UI.
    pub fn wants_tree(&self) -> bool {
        self.alive.load(Ordering::Acquire) && self.asked.load(Ordering::Acquire)
    }

    /// Apply queued requests on the normal UI tick; no native calls occur.
    pub fn apply(&mut self, ui: &mut Ui) -> bool {
        let mut landed = false;
        while let Ok(request) = self.actions.try_recv() {
            if !self.alive.load(Ordering::Acquire) {
                continue;
            }
            if let Some(action) = ui.scene().and_then(|scene| semantic(scene, &request)) {
                let affinity = if let Some(ActionData::SetTextSelection(selection)) = &request.data
                {
                    ui.scene().and_then(|scene| {
                        Some((
                            mui_access::surface_of(scene, request.target_node)?
                                .key
                                .to_string(),
                            mui_access::selection_is_upstream(
                                scene,
                                request.target_node,
                                &selection.focus,
                            )?,
                        ))
                    })
                } else {
                    None
                };
                if ui.request_action(action) {
                    if let Some((key, upstream)) = affinity {
                        ui.set_text_selection_affinity(key, upstream);
                    }
                    landed = true;
                }
            }
        }
        landed
    }

    /// Prepare under the model lock without calling the native adapter.
    pub fn prepare(&mut self, ui: &Ui) -> Option<TreeUpdate> {
        if !self.alive.load(Ordering::Acquire) || !self.active.load(Ordering::Acquire) {
            return None;
        }
        let scene = ui.scene()?;
        if self.asked.swap(false, Ordering::AcqRel) {
            self.publisher.reset();
        }
        Some(
            self.publisher
                .update(scene, ui.focus_key(), ui.scale().unwrap_or(1.0)),
        )
    }
}

fn semantic(scene: &mui::scene::ResolvedScene, request: &ActionRequest) -> Option<SemanticAction> {
    if request.target_tree != mui_access::accesskit::TreeId::ROOT {
        return None;
    }
    let surface = mui_access::surface_of(scene, request.target_node)?;
    if surface.disabled {
        return None;
    }
    let role = surface.semantics.as_ref().map(|s| &s.role);
    let key = surface.key.to_string();
    Some(match (request.action, &request.data) {
        (Action::Focus, _) if surface.focusable => SemanticAction::focus(key),
        (Action::Click, _)
            if matches!(
                role,
                Some(mui_access::A11y::Button | mui_access::A11y::Toggle { .. })
            ) =>
        {
            SemanticAction::activate(key)
        }
        (Action::Increment, _) if matches!(role, Some(mui_access::A11y::Slider { .. })) => {
            SemanticAction::increment(key)
        }
        (Action::Decrement, _) if matches!(role, Some(mui_access::A11y::Slider { .. })) => {
            SemanticAction::decrement(key)
        }
        (Action::SetValue, Some(ActionData::NumericValue(value)))
            if value.is_finite() && matches!(role, Some(mui_access::A11y::Slider { .. })) =>
        {
            SemanticAction::set_value(key, *value)
        }
        (Action::SetTextSelection, Some(ActionData::SetTextSelection(selection))) => {
            let (anchor, focus) = mui_access::selection_of(scene, request.target_node, selection)?;
            SemanticAction::set_selection(key, anchor, focus)
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui::scene::prelude::*;
    use mui_access::accesskit::TreeId;

    #[test]
    fn portable_endpoint_accepts_callbacks_during_a_model_borrow_and_disconnects_on_drop() {
        fn assert_send<T: Send>() {}
        struct View;
        impl crate::View for View {
            fn build(&mut self, _: &mut Ui, _: &mui::prelude::Input) -> mui::prelude::El {
                block(10., 10.)
                    .focusable()
                    .a11y(mui_access::A11y::Button)
                    .id("field")
            }
            fn changed(&mut self) -> bool {
                false
            }
            fn request_resize(&mut self, _: u32, _: u32) -> bool {
                false
            }
        }
        assert_send::<crate::native::AccessibilityUi>();
        let shared = Arc::new(std::sync::Mutex::new(crate::Shared {
            ui: Ui::default(),
            view: View,
        }));
        let mut handler = crate::Handler::new(shared.clone(), Arc::default(), (100, 100), 1.);
        handler.step();
        let (send, actions) = channel();
        let alive = Arc::new(AtomicBool::new(true));
        let asked = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicBool::new(false));
        let mut endpoint = AccessibilityUi {
            alive: Arc::clone(&alive),
            actions,
            asked: asked.clone(),
            active: active.clone(),
            publisher: Publisher::default(),
        };
        let mut activation = Asked {
            alive: Arc::clone(&alive),
            asked,
            active,
        };
        let mut callback = Actions(send, Arc::clone(&alive));
        let request = || ActionRequest {
            action: Action::Focus,
            target_tree: TreeId::ROOT,
            target_node: mui_access::node_id("field"),
            data: None,
        };
        // These synchronous callbacks must not borrow/lock the model, even
        // when delivered while the editor already holds it.
        let mut model = crate::lock(&shared);
        assert!(activation.request_initial_tree().is_none());
        callback.do_action(request());
        assert!(endpoint.wants_tree());
        assert!(endpoint.apply(&mut model.ui));
        let first = endpoint.prepare(&model.ui).unwrap();
        assert!(!endpoint.wants_tree());
        assert!(!first.nodes.is_empty());
        // Reentrant activation after prepare requests another complete tree.
        assert!(activation.request_initial_tree().is_none());
        assert!(!endpoint.prepare(&model.ui).unwrap().nodes.is_empty());
        drop(model);
        handler.driver.redraw();
        handler.step();
        assert_eq!(crate::lock(&shared).ui.focus_key(), Some("field"));
        drop(endpoint);
        // Pending native callbacks neither retain the model nor reach a new
        // window's endpoint after its predecessor has closed.
        callback.do_action(request());
        assert_eq!(Arc::strong_count(&shared), 2);
    }

    #[test]
    fn activation_and_actions_never_need_the_model_or_native_window() {
        let alive = Arc::new(AtomicBool::new(true));
        let asked = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicBool::new(false));
        let mut handler = Asked {
            alive: Arc::clone(&alive),
            asked: asked.clone(),
            active: active.clone(),
        };
        assert!(handler.request_initial_tree().is_none());
        assert!(asked.load(Ordering::Acquire));
        assert!(active.load(Ordering::Acquire));
        let (send, receive) = channel();
        let mut handler = Actions(send, Arc::clone(&alive));
        handler.do_action(ActionRequest {
            action: Action::Focus,
            target_tree: TreeId::ROOT,
            target_node: mui_access::node_id("field"),
            data: None,
        });
        assert_eq!(
            receive.try_recv().unwrap().target_node,
            mui_access::node_id("field")
        );
        drop(receive);
        // Teardown disconnects pending native providers without a model cycle.
        handler.do_action(ActionRequest {
            action: Action::Focus,
            target_tree: TreeId::ROOT,
            target_node: mui_access::node_id("field"),
            data: None,
        });
    }

    #[test]
    fn actions_reject_stale_disabled_wrong_role_and_nonfinite_requests() {
        let scene = resolve(&SceneSpec::new(row![
            block(10., 10.).focusable().id("group"),
            block(10., 10.)
                .a11y(mui_access::A11y::Button)
                .disabled()
                .id("off"),
            block(10., 10.)
                .a11y(mui_access::A11y::Slider {
                    value: 0.,
                    min: 0.,
                    max: 1.
                })
                .id("gain"),
        ]))
        .unwrap();
        let request = |key, action, data| ActionRequest {
            action,
            data,
            target_tree: TreeId::ROOT,
            target_node: mui_access::node_id(key),
        };
        assert_eq!(
            semantic(&scene, &request("group", Action::Focus, None)),
            Some(SemanticAction::focus("group"))
        );
        assert!(semantic(&scene, &request("group", Action::Click, None)).is_none());
        assert!(semantic(&scene, &request("off", Action::Click, None)).is_none());
        assert!(semantic(&scene, &request("removed", Action::Click, None)).is_none());
        assert!(
            semantic(
                &scene,
                &request(
                    "gain",
                    Action::SetValue,
                    Some(ActionData::NumericValue(f64::NAN))
                )
            )
            .is_none()
        );
        assert_eq!(
            semantic(&scene, &request("gain", Action::Increment, None)),
            Some(SemanticAction::increment("gain"))
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs a native X11 display (run under Xvfb)"]
    fn portable_endpoint_survives_two_native_window_teardowns() {
        use baseview::{
            Event, EventStatus, HandlerError, Window, WindowContext, WindowEvent, WindowHandler,
            WindowSettings, WindowSize,
        };
        use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
        use std::cell::RefCell;
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
            mpsc::channel,
        };
        struct Probe {
            native: RefCell<Option<NativeAccessibility>>,
            closed: Arc<AtomicUsize>,
            cx: WindowContext,
        }
        impl WindowHandler for Probe {
            fn on_frame(&self) -> Result<(), HandlerError> {
                if let Some(native) = self.native.borrow_mut().as_mut() {
                    // SAFETY: both handles borrow our live context on this
                    // native callback thread. No model is borrowed here.
                    #[expect(unsafe_code, reason = "native bridge lifetime regression")]
                    unsafe {
                        native.update_bounds(
                            self.cx.display_handle().unwrap().as_raw(),
                            self.cx.window_handle().unwrap().as_raw(),
                        );
                    }
                    native.focus(self.cx.has_focus());
                }
                Ok(())
            }
            fn resized(&self, _: WindowSize) -> Result<(), HandlerError> {
                Ok(())
            }
            fn on_event(&self, event: Event) -> EventStatus {
                if matches!(event, Event::Window(WindowEvent::WillClose)) {
                    drop(self.native.borrow_mut().take());
                    self.closed.fetch_add(1, Ordering::Release);
                }
                EventStatus::Captured
            }
        }
        struct View;
        impl crate::View for View {
            fn build(&mut self, _: &mut Ui, _: &mui::prelude::Input) -> mui::prelude::El {
                block(10., 10.)
                    .focusable()
                    .a11y(mui_access::A11y::Button)
                    .id("field")
            }
            fn changed(&mut self) -> bool {
                false
            }
            fn request_resize(&mut self, _: u32, _: u32) -> bool {
                false
            }
        }
        let closed = Arc::new(AtomicUsize::new(0));
        for cycle in 1..=2 {
            let (send, receive) = channel();
            let native_closed = closed.clone();
            let window = Window::create(
                WindowSettings::new()
                    .with_title("MUI accessibility bridge lifetime")
                    .with_size(baseview::dpi::LogicalSize::new(120., 100.)),
                move |cx| {
                    // SAFETY: attach in the owning thread's pre-show builder;
                    // Probe drops the adapter before native teardown/context.
                    #[expect(unsafe_code, reason = "native bridge lifetime regression")]
                    let (native, mut endpoint) =
                        unsafe { NativeAccessibility::new(cx.window_handle().unwrap().as_raw()) }
                            .unwrap();
                    // Retain the actual provider's flags; test callback objects
                    // use the same production handler types after native teardown.
                    let activation = Asked {
                        alive: endpoint.alive.clone(),
                        asked: endpoint.asked.clone(),
                        active: endpoint.active.clone(),
                    };
                    let (requests, actions) = channel();
                    endpoint.actions = actions;
                    let callback = Actions(requests, endpoint.alive.clone());
                    send.send((endpoint, activation, callback)).unwrap();
                    Ok(Probe {
                        native: RefCell::new(Some(native)),
                        closed: native_closed,
                        cx,
                    })
                },
            )
            .unwrap();
            // The UI endpoint moves from the native callback thread to this
            // model thread, then remains valid after its native peer closes.
            let (mut endpoint, mut activation, mut callback) = receive.recv().unwrap();
            let shared = Arc::new(std::sync::Mutex::new(crate::Shared {
                ui: Ui::default(),
                view: View,
            }));
            let mut handler = crate::Handler::new(shared.clone(), Arc::default(), (100, 100), 1.);
            handler.step();
            let request = || ActionRequest {
                action: Action::Focus,
                target_tree: TreeId::ROOT,
                target_node: mui_access::node_id("field"),
                data: None,
            };
            assert!(activation.request_initial_tree().is_none());
            assert!(endpoint.wants_tree());
            assert!(
                !endpoint
                    .prepare(&crate::lock(&shared).ui)
                    .unwrap()
                    .nodes
                    .is_empty()
            );
            callback.do_action(request());
            window.show().unwrap();
            window.close();
            assert_eq!(closed.load(Ordering::Acquire), cycle);
            // Late provider callbacks cannot reactivate this retained endpoint,
            // and the focus request queued before close must also be discarded.
            assert!(activation.request_initial_tree().is_none());
            callback.do_action(request());
            assert!(!endpoint.alive.load(Ordering::Acquire));
            assert!(!endpoint.active.load(Ordering::Acquire));
            assert!(!endpoint.asked.load(Ordering::Acquire));
            assert!(!endpoint.wants_tree());
            {
                let mut model = crate::lock(&shared);
                assert!(model.ui.scene().is_some());
                assert!(!endpoint.apply(&mut model.ui));
                assert!(endpoint.prepare(&model.ui).is_none());
            }
            handler.driver.redraw();
            handler.step();
            assert_eq!(crate::lock(&shared).ui.focus_key(), None);
        }
    }
}
