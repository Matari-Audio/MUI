//! Safe standalone top-level window host and direct AccessKit integration over
//! our owned `moose-baseview` layer (X11, native Wayland `xdg_toplevel`, Win32,
//! macOS Cocoa). Keeps standalone binaries like `mui-preview` free of `unsafe`
//! and free of `winit` / `accesskit_winit`.
use std::sync::Arc;

use baseview::dpi::LogicalSize;
use baseview::{HandlerError, Window, WindowContext, WindowHandler, WindowSettings};
#[cfg(target_os = "linux")]
use raw_window_handle::HasDisplayHandle;
use raw_window_handle::HasWindowHandle;

use crate::a11y::{AccessibilityUi, NativeAccessibility};

/// Build standalone top-level [`WindowSettings`] with `title` and logical
/// `size`, preferring native Wayland (`xdg_toplevel`) on Linux whenever
/// `WAYLAND_DISPLAY` is set unless `MUI_LINUX_BACKEND=x11` forces X11.
pub fn settings(title: &str, size: (u32, u32)) -> WindowSettings {
    let settings = WindowSettings::new()
        .with_title(title)
        .with_size(LogicalSize::new(f64::from(size.0), f64::from(size.1)));
    #[cfg(target_os = "linux")]
    let settings = if prefer_wayland() {
        settings.with_wayland(true)
    } else {
        settings
    };
    settings
}

#[cfg(target_os = "linux")]
fn prefer_wayland() -> bool {
    if std::env::var("MUI_LINUX_BACKEND").is_ok_and(|v| v.eq_ignore_ascii_case("x11")) {
        return false;
    }
    if std::env::var("MUI_LINUX_BACKEND").is_ok_and(|v| v.eq_ignore_ascii_case("wayland")) {
        return true;
    }
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// Run a standalone top-level window until closed.
///
/// Marks the process as standalone (Windows per-monitor DPI awareness), creates
/// the window on the native backend (Wayland `xdg_toplevel` or X11 on Linux,
/// Win32 on Windows, Cocoa on macOS), and blocks until the window closes.
/// Never call this from a plugin.
pub fn run_window<H: WindowHandler>(
    settings: WindowSettings,
    handler: impl FnOnce(WindowContext) -> Result<H, HandlerError> + Send + 'static,
) -> Result<(), String> {
    // SAFETY: standalone application entrypoint; never called inside a plugin host.
    #[expect(unsafe_code, reason = "baseview's standalone-process DPI opt-in")]
    unsafe {
        baseview::assume_standalone_in_process();
    }
    Window::create(settings, handler)
        .and_then(Window::run_until_closed)
        .map_err(|error| error.to_string())
}

/// Combined native + UI AccessKit bridge for single-threaded standalone windows
/// (`!Send`, confined to the window's owning thread). Drops its native endpoint
/// safely before its window context.
pub struct StandaloneAccessibility {
    native: NativeAccessibility,
    ui: AccessibilityUi,
}

impl StandaloneAccessibility {
    /// Attach AccessKit (`accesskit_unix` / `accesskit_macos` /
    /// `accesskit_windows`) inside the `Window::create` builder closure.
    /// Screen-reader activation and actions wake `cx` immediately so settled
    /// 0 Hz windows respond without polling.
    pub fn attach(cx: &WindowContext) -> Option<Self> {
        let handle = cx.window_handle().ok()?.as_raw();
        let waker = cx.waker();
        #[expect(
            unsafe_code,
            reason = "attach direct AccessKit adapter to live baseview window on owning thread"
        )]
        // SAFETY: `cx` owns this live window on its owning thread; callers keep
        // `StandaloneAccessibility` inside their `WindowHandler` and drop it
        // on `WillClose` / handler drop (before `cx` drops).
        let (mut native, ui) = unsafe {
            NativeAccessibility::new_with_wake(
                handle,
                Arc::new(move || {
                    waker.request_redraw();
                }),
            )
        }?;
        native.focus(cx.has_focus());
        Some(Self { native, ui })
    }

    /// Notify the native accessibility provider of window focus changes.
    pub fn focus(&mut self, focused: bool) {
        self.native.focus(focused);
    }

    /// Refresh screen bounds on Linux X11 while accessibility is active.
    pub fn update_bounds(&mut self, cx: &WindowContext) {
        #[cfg(target_os = "linux")]
        if let (Ok(display), Ok(handle)) = (cx.display_handle(), cx.window_handle()) {
            // SAFETY: both handles borrow `cx`'s live window on its owning thread.
            #[expect(unsafe_code, reason = "native bounds query on owning thread")]
            unsafe {
                self.native.update_bounds(display.as_raw(), handle.as_raw());
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = cx;
    }

    /// Whether a screen reader requested a full initial tree.
    pub fn wants_tree(&self) -> bool {
        self.ui.wants_tree()
    }

    /// Apply queued screen-reader actions to `ui`. Returns true if any landed.
    pub fn apply(&mut self, ui: &mut mui::Ui) -> bool {
        self.ui.apply(ui)
    }

    /// Publish an updated accessibility tree when a screen reader is active.
    pub fn publish(&mut self, ui: &mui::Ui) {
        if let Some(update) = self.ui.prepare(ui) {
            self.native.publish(update);
        }
    }
}
