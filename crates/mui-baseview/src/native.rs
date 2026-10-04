//! Native bridges for hosts that retain their own `Driver` and window handler.
//!
//! Construct accessibility in the pre-show window builder. Its native endpoint
//! stays on the window thread; its UI endpoint may move to the model thread.
//! Apply actions and prepare trees with the UI, then release all model locks
//! and mutable borrows before publishing or changing native focus/IME state.
//! Queue reentrant events rather than dropping them while a frame is borrowed.

pub use crate::a11y::{AccessibilityUi, NativeAccessibility};
pub use mui_access::accesskit::TreeUpdate;

/// Translate a native composition event without losing UTF-8 text ranges.
pub fn ime_event(event: &baseview::Ime) -> mui::prelude::Ime {
    match event {
        baseview::Ime::Selection(range) => mui::prelude::Ime::Selection(range.clone()),
        baseview::Ime::Enabled => mui::prelude::Ime::Enabled,
        baseview::Ime::Preedit { text, cursor } => mui::prelude::Ime::Preedit {
            text: text.clone(),
            cursor: *cursor,
        },
        baseview::Ime::Commit(text) => mui::prelude::Ime::Commit(text.clone()),
        baseview::Ime::Disabled => mui::prelude::Ime::Disabled,
    }
}

/// Convert a driver's text state and caret geometry to native physical pixels.
/// Pass `Driver::ui_scale()` so both DPI and the UI's own zoom are included.
pub fn ime_configuration(
    mut config: mui::host::ImeConfiguration,
    scale: f64,
) -> mui::host::ImeConfiguration {
    config.area.0.x *= scale;
    config.area.0.y *= scale;
    config.area.1.width *= scale;
    config.area.1.height *= scale;
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_ime_helpers_keep_unicode_ranges_and_convert_the_caret_once() {
        let config = mui::host::ImeConfiguration {
            id: "field".into(),
            area: (
                mui::prelude::Point::new(10., 20.),
                mui::scene::Size::new(1., 16.),
            ),
            text: "a😀é".into(),
            selection: 1..5,
            marked: Some(1..5),
        };
        let physical = ime_configuration(config.clone(), 1.5);
        assert_eq!(
            physical.area,
            (
                mui::prelude::Point::new(15., 30.),
                mui::scene::Size::new(1.5, 24.)
            )
        );
        assert_eq!(
            (physical.text, physical.selection, physical.marked),
            (config.text, config.selection, config.marked)
        );
        assert!(
            matches!(ime_event(&baseview::Ime::Selection(1..5)), mui::prelude::Ime::Selection(r) if r == (1..5))
        );
        assert!(
            matches!(ime_event(&baseview::Ime::Preedit { text: "😀".into(), cursor: Some((0, 4)) }), mui::prelude::Ime::Preedit { text, cursor: Some((0, 4)) } if text == "😀")
        );
        assert!(
            matches!(ime_event(&baseview::Ime::Commit("é".into())), mui::prelude::Ime::Commit(text) if text == "é")
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
                    let (native, endpoint) =
                        unsafe { NativeAccessibility::new(cx.window_handle().unwrap().as_raw()) }
                            .unwrap();
                    send.send(endpoint).unwrap();
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
            let mut endpoint = receive.recv().unwrap();
            window.show().unwrap();
            window.close();
            assert_eq!(closed.load(Ordering::Acquire), cycle);
            let mut ui = mui::Ui::default();
            assert!(!endpoint.apply(&mut ui));
            assert!(endpoint.prepare(&ui).is_none());
        }
    }
}
