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
    #[cfg(target_os = "linux")]
    active: Arc<AtomicBool>,
    #[cfg(target_os = "linux")]
    bounds: x11::BoundsPoll,
}

/// The portable accessibility endpoint. It may move to the thread that owns
/// the UI, while [`NativeAccessibility`] stays on the native window thread.
/// Activation and action callbacks only touch shared atomics and a channel.
pub struct AccessibilityUi {
    actions: Receiver<ActionRequest>,
    asked: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    publisher: Publisher,
}

struct Asked {
    asked: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
}
impl ActivationHandler for Asked {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.active.store(true, Ordering::Release);
        self.asked.store(true, Ordering::Release);
        // No model lock from a native callback. A full tree follows next tick.
        None
    }
}
struct Actions(Sender<ActionRequest>);
impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        // The receiving editor may already have closed. No window/model is
        // retained by this callback, so reopening gets a fresh action queue.
        let _ = self.0.send(request);
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
        let asked = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicBool::new(false));
        let activation = Asked {
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
            accesskit_unix::Adapter::new(activation, Actions(send), Gone(Arc::clone(&active)))
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
                    Actions(send),
                )
            }
        };
        #[cfg(target_os = "windows")]
        let adapter = {
            let RawWindowHandle::Win32(handle) = handle else {
                return None;
            };
            let hwnd = accesskit_windows::HWND(handle.hwnd.get() as *mut std::ffi::c_void);
            accesskit_windows::SubclassingAdapter::new(hwnd, activation, Actions(send))
        };
        Some((
            Self {
                adapter,
                thread: std::marker::PhantomData,
                #[cfg(target_os = "linux")]
                active: Arc::clone(&active),
                #[cfg(target_os = "linux")]
                bounds: x11::BoundsPoll::default(),
            },
            AccessibilityUi {
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

impl AccessibilityUi {
    /// A native provider asked for a full tree; redraw even an idle UI.
    pub fn wants_tree(&self) -> bool {
        self.asked.load(Ordering::Acquire)
    }

    /// Apply queued requests on the normal UI tick; no native calls occur.
    pub fn apply(&mut self, ui: &mut Ui) -> bool {
        let mut landed = false;
        while let Ok(request) = self.actions.try_recv() {
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
        if !self.active.load(Ordering::Acquire) {
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
        let asked = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicBool::new(false));
        let mut endpoint = AccessibilityUi {
            actions,
            asked: asked.clone(),
            active: active.clone(),
            publisher: Publisher::default(),
        };
        let mut activation = Asked { asked, active };
        let mut callback = Actions(send);
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
        let asked = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicBool::new(false));
        let mut handler = Asked {
            asked: asked.clone(),
            active: active.clone(),
        };
        assert!(handler.request_initial_tree().is_none());
        assert!(asked.load(Ordering::Acquire));
        assert!(active.load(Ordering::Acquire));
        let (send, receive) = channel();
        let mut handler = Actions(send);
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
}
