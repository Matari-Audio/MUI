use crate::dpi::{LogicalSize, Size};
use objc2::rc::{Retained, Weak};
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSFloatingWindowLevel, NSPasteboard,
    NSPasteboardTypeString, NSScreen, NSView, NSWindow, NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use std::cell::Cell;
use std::rc::Rc;

use crate::platform::macos::view::{BaseviewView, ViewParentingType};
use crate::platform::ParentWindowHandle;
use crate::platform::Result;
use crate::utils::SizingStrategy;
use crate::wrappers::appkit::{callback, create_window, View};
use crate::*;

pub struct WindowHandle {
    mtm: MainThreadMarker,
    view: Weak<View<BaseviewView>>,
    _window: Option<Retained<NSWindow>>,
    state: Rc<WindowSharedState>,
}

impl Drop for WindowHandle {
    fn drop(&mut self) {
        let Some(view) = self.view.load() else { return };
        let Some(view) = view.inner_ref() else { return };

        callback("drop window handle", (), || BaseviewView::close(view, true));
    }
}

impl WindowHandle {
    pub fn create_window(mut init: WindowInitializer) -> Result<Self> {
        callback(
            "create window",
            Err(crate::platform::PlatformError::CreationFailed("panic while creating window")),
            || {
                let mtm = MainThreadMarker::new().ok_or(
                    crate::platform::PlatformError::CreationFailed(
                        "macOS windows require the main thread",
                    ),
                )?;

                if let Some(name) = init.settings.dock.as_ref().and_then(|d| d.output.as_ref()) {
                    if !NSScreen::screens(mtm)
                        .iter()
                        .any(|s| s.localizedName().to_string() == *name)
                    {
                        return Err(crate::platform::PlatformError::CreationFailed(
                            "selected dock display is unavailable",
                        ));
                    }
                }
                // Creates the global NSApplication instance, if it doesn't exist yet
                let _ = NSApplication::sharedApplication(mtm);

                if let Some(parent) = init.settings.parent.take() {
                    return Self::create_window_parented(
                        init,
                        parent.inner.view.into_inner(mtm),
                        mtm,
                    );
                }

                Self::create_window_standalone(init, mtm)
            },
        )
    }

    pub fn create_window_parented(
        init: WindowInitializer, parent_view: Retained<NSView>, mtm: MainThreadMarker,
    ) -> Result<Self> {
        let parenting =
            ViewParentingType::Parented { parent_view: Weak::from_retained(&parent_view) };

        let backing_scale_factor =
            parent_view.window().map(|w| w.backingScaleFactor()).unwrap_or(1.0);
        let final_size = init.settings.size.to_logical(backing_scale_factor);

        let (ns_view, state) = BaseviewView::new(init, parenting, final_size, mtm)?;

        Ok(Self { mtm, state, _window: None, view: Weak::from_retained(&ns_view) })
    }

    pub fn create_window_standalone(
        mut init: WindowInitializer, mtm: MainThreadMarker,
    ) -> Result<Self> {
        let owner_window =
            init.settings.owner.take().and_then(|owner| owner.inner.view.into_inner(mtm).window());
        let window = create_window_with_options(&init.settings, mtm);

        let final_size = window.contentRectForFrameRect(window.frame()).size;
        let final_size = LogicalSize::new(final_size.width, final_size.height);

        let parenting = ViewParentingType::Windowed { owned_window: Weak::from_retained(&window) };

        let (view, state) = BaseviewView::new(init, parenting, final_size, mtm)?;

        if let Some(owner_window) = owner_window {
            // SAFETY: both retained windows are live on the AppKit main thread.
            unsafe {
                owner_window.addChildWindow_ordered(&window, NSWindowOrderingMode::Above);
            }
        }

        Ok(Self { mtm, state, view: Weak::from_retained(&view), _window: Some(window) })
    }

    pub fn run_until_closed(self) -> Result<()> {
        objc2::rc::autoreleasepool(|_| {
            let Some(view) = self.view.load() else {
                return Ok(());
            };
            let Some(view) = view.inner_ref() else {
                return Ok(());
            };

            BaseviewView::show(view);

            let app = NSApplication::sharedApplication(self.mtm);
            app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

            view.lifetime_tied_to_app.set(Some(Weak::from_retained(&app)));
            app.run();
            view.lifetime_tied_to_app.set(None);

            Ok(())
        })
    }

    pub fn is_open(&self) -> bool {
        !self.state.closed.get()
    }

    pub fn is_resizable(&self) -> bool {
        self.state.sizing_strategy.is_resizable()
    }

    pub fn min_size(&self) -> Option<Size> {
        self.state.sizing_strategy.min_size()
    }

    pub fn max_size(&self) -> Option<Size> {
        self.state.sizing_strategy.max_size()
    }

    #[inline]
    pub fn handle_main_thread_callback(&self) {
        // No-op
    }

    pub fn size(&self) -> WindowSize {
        WindowSize::from_logical(self.state.size.get(), self.state.scale_factor.get())
    }

    pub fn resize(&self, size: Size) -> Result<()> {
        objc2::rc::autoreleasepool(|_| {
            let Some(view) = self.view.load() else {
                return Ok(());
            };
            let Some(view) = view.inner_ref() else {
                return Ok(());
            };

            BaseviewView::resize(view, size, false, false);

            Ok(())
        })
    }

    pub fn suggest_scale_factor(&self, _scale_factor: f64) -> Result<()> {
        // This does not do anything on macOS: all coordinates are already logical
        Ok(())
    }

    pub fn set_keyboard_capture(&self, capture: bool) {
        let Some(view) = self.view.load() else { return };
        let Some(view) = view.inner_ref() else { return };
        callback("keyboard capture", (), || BaseviewView::set_keyboard_capture(view, capture));
    }

    pub fn visibility_control(&self) -> WindowVisibility {
        let view =
            std::sync::Arc::new(dispatch2::MainThreadBound::new(self.view.clone(), self.mtm));
        WindowVisibility::new(move |visible| {
            let view = std::sync::Arc::clone(&view);
            dispatch2::DispatchQueue::main().exec_async(move || {
                let Some(mtm) = MainThreadMarker::new() else {
                    return;
                };
                let Some(view) = view.get(mtm).load() else {
                    return;
                };
                let Some(view) = view.inner_ref() else {
                    return;
                };
                callback("external visibility", (), || {
                    if visible {
                        BaseviewView::show(view);
                    } else {
                        BaseviewView::hide(view);
                    }
                });
            });
        })
    }
    pub fn frame_requester(&self) -> FrameRequester {
        let Some(view) = self.view.load() else {
            return FrameRequester::new(|| {});
        };
        let Some(view) = view.inner_ref() else {
            return FrameRequester::new(|| {});
        };
        BaseviewView::frame_requester(view)
    }

    pub fn set_scale_factor_override(&self, _scale_factor: Option<f64>) -> Result<()> {
        // No-op on macOS: coordinates are logical and the backing scale is authoritative.
        Ok(())
    }

    pub fn set_parent(&self, new_parent: ParentWindowHandle) -> Result<()> {
        objc2::rc::autoreleasepool(|_| {
            let Some(view) = self.view.load() else {
                return Ok(());
            };
            let Some(view) = view.inner_ref() else {
                return Ok(());
            };

            BaseviewView::set_parent(view, new_parent.view.into_inner(view.mtm));

            Ok(())
        })
    }

    pub fn show(&self) -> Result<()> {
        objc2::rc::autoreleasepool(|_| {
            let Some(view) = self.view.load() else {
                return Ok(());
            };
            let Some(view) = view.inner_ref() else {
                return Ok(());
            };

            BaseviewView::show(view);
            Ok(())
        })
    }

    pub fn hide(&self) -> Result<()> {
        objc2::rc::autoreleasepool(|_| {
            let Some(view) = self.view.load() else {
                return Ok(());
            };
            let Some(view) = view.inner_ref() else {
                return Ok(());
            };

            BaseviewView::hide(view);
            Ok(())
        })
    }
}

fn create_window_with_options(
    settings: &WindowSettings, mtm: MainThreadMarker,
) -> Retained<NSWindow> {
    let initial_size = settings.size.to_logical(1.0);
    let window = create_window(initial_size, settings, mtm);
    window.center();

    let final_size = settings.size.to_logical(window.backingScaleFactor());
    if final_size != initial_size {
        window.setContentSize(NSSize::new(final_size.width, final_size.height));
    }

    let title = NSString::from_str(&settings.title);
    window.setTitle(&title);

    if let Some(dock) = &settings.dock {
        let screens = NSScreen::screens(mtm);
        let screen = screens.iter().find(|screen| {
            dock.output.as_ref().is_none_or(|name| screen.localizedName().to_string() == *name)
        });
        if let Some(screen) = screen {
            // macOS has no AppBar work-area contract. This is deliberately an
            // edge overlay; it never changes another application's windows.
            let frame = screen.visibleFrame();
            let height = f64::from(dock.height).min(frame.size.height * 0.5);
            let y = match dock.edge {
                crate::DockEdge::Top => frame.origin.y + frame.size.height - height,
                crate::DockEdge::Bottom => frame.origin.y,
            };
            window.setStyleMask(NSWindowStyleMask::Borderless);
            window.setLevel(NSFloatingWindowLevel);
            window.setFrame_display(
                NSRect::new(NSPoint::new(frame.origin.x, y), NSSize::new(frame.size.width, height)),
                false,
            );
        }
    }

    window
}

pub(crate) struct WindowSharedState {
    pub closed: Cell<bool>,
    pub defer_user_close: bool,
    pub size: Cell<LogicalSize<f64>>,
    pub scale_factor: Cell<f64>,
    pub sizing_strategy: SizingStrategy,
}

impl WindowSharedState {
    pub fn new(
        size: LogicalSize<f64>, scale_factor: f64, sizing_strategy: SizingStrategy,
        defer_user_close: bool,
    ) -> Self {
        Self {
            closed: false.into(),
            defer_user_close,
            size: size.into(),
            scale_factor: scale_factor.into(),
            sizing_strategy,
        }
    }
}

pub fn copy_to_clipboard(string: &str) {
    objc2::rc::autoreleasepool(|_| {
        let pb = NSPasteboard::generalPasteboard();
        let ns_str = NSString::from_str(string);

        pb.clearContents();
        pb.setString_forType(&ns_str, unsafe { NSPasteboardTypeString });
    })
}
