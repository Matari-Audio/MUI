//! Visibility guards for editors embedded in host-owned windows.
//!
//! `should_skip_frame` originated in truce-gui-utils 6.3.0 (MIT/Apache-2.0).
//! The native baseview fork owns macOS anchoring, including flipped parents.
use raw_window_handle::RawWindowHandle;

/// Whether this tick's frame should be skipped: the editor's view is
/// detached from any window, or the host window is not visible (macOS
/// occlusion; Windows hidden or minimized). A hidden window cannot present,
/// and on Windows `on_frame` runs on the host's GUI thread. Always `false`
/// elsewhere.
#[cfg(target_os = "macos")]
#[must_use]
#[expect(unsafe_code, reason = "Objective-C messages to the host's views")]
pub fn should_skip_frame(handle: RawWindowHandle) -> bool {
    use objc2_app_kit::{NSView, NSWindowOcclusionState};

    let RawWindowHandle::AppKit(handle) = handle else {
        return false;
    };
    // SAFETY: baseview supplies its live NSView on the main thread.
    let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    view.window().is_none_or(|window| {
        !window
            .occlusionState()
            .contains(NSWindowOcclusionState::Visible)
    })
}

#[cfg(target_os = "windows")]
#[must_use]
#[expect(unsafe_code, reason = "two Win32 window-state queries")]
pub fn should_skip_frame(handle: RawWindowHandle) -> bool {
    unsafe extern "system" {
        fn IsWindowVisible(hwnd: *mut std::ffi::c_void) -> i32;
        fn IsIconic(hwnd: *mut std::ffi::c_void) -> i32;
    }

    let hwnd = match handle {
        RawWindowHandle::Win32(h) => h.hwnd.get() as *mut std::ffi::c_void,
        _ => return false,
    };
    // SAFETY: both are pure state queries on a window handle baseview
    // owns for the editor's lifetime; no aliasing or threading concerns,
    // and they're called from the GUI thread that owns the HWND.
    unsafe { IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[must_use]
pub fn should_skip_frame(_handle: RawWindowHandle) -> bool {
    false
}

/// Put the pointer at `x`, `y` physical pixels from the top
/// left of `window`'s view, where a drag that hid it began. X11 (through
/// Xlib, loaded at run time as baseview loads it), Windows and macOS; a
/// no-op anywhere else, and wherever the handles are not those.
#[cfg(target_os = "linux")]
#[expect(unsafe_code, reason = "two Xlib calls on baseview's live display")]
pub fn warp_pointer(window: &baseview::WindowContext, x: f64, y: f64, _scale: f64) {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle};
    let target = match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Xlib(h)) => h.window,
        Ok(RawWindowHandle::Xcb(h)) => std::os::raw::c_ulong::from(h.window.get()),
        _ => return,
    };
    let Ok(RawDisplayHandle::Xlib(d)) = window.display_handle().map(|h| h.as_raw()) else {
        return;
    };
    let Some(display) = d.display else { return };
    let Ok(xlib) = x11_dl::xlib::Xlib::open() else {
        return;
    };
    // SAFETY: `display` is baseview's open Xlib display and `target` its
    // window, both alive while the handler that calls this is; XWarpPointer
    // and XFlush only queue and send a request on it, on its own thread.
    unsafe {
        let display = display.as_ptr().cast();
        (xlib.XWarpPointer)(
            display,
            0,
            target,
            0,
            0,
            0,
            0,
            x.round() as i32,
            y.round() as i32,
        );
        (xlib.XFlush)(display);
    }
}

#[cfg(target_os = "windows")]
#[expect(unsafe_code, reason = "two Win32 cursor calls")]
pub fn warp_pointer(window: &baseview::WindowContext, x: f64, y: f64, _scale: f64) {
    use raw_window_handle::HasWindowHandle;
    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }
    unsafe extern "system" {
        fn ClientToScreen(hwnd: *mut std::ffi::c_void, point: *mut Point) -> i32;
        fn SetCursorPos(x: i32, y: i32) -> i32;
    }
    let Ok(RawWindowHandle::Win32(h)) = window.window_handle().map(|h| h.as_raw()) else {
        return;
    };
    let mut at = Point {
        x: x.round() as i32,
        y: y.round() as i32,
    };
    // SAFETY: the HWND is baseview's, alive for the handler's lifetime;
    // `at` is a live POINT. Both calls run on the window's own thread.
    unsafe {
        if ClientToScreen(h.hwnd.get() as *mut std::ffi::c_void, &mut at) != 0 {
            SetCursorPos(at.x, at.y);
        }
    }
}

#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "AppKit conversions and a Quartz warp")]
pub fn warp_pointer(window: &baseview::WindowContext, x: f64, y: f64, scale: f64) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSScreen, NSView};
    use objc2_foundation::NSPoint;
    use raw_window_handle::HasWindowHandle;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGWarpMouseCursorPosition(point: NSPoint) -> i32;
        fn CGAssociateMouseAndMouseCursorPosition(connected: i32) -> i32;
    }
    let Ok(RawWindowHandle::AppKit(h)) = window.window_handle().map(|h| h.as_raw()) else {
        return;
    };
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    // SAFETY: baseview supplies its live NSView on the main thread.
    let view = unsafe { h.ns_view.cast::<NSView>().as_ref() };
    let (Some(window), Some(main)) = (view.window(), NSScreen::screens(mtm).firstObject()) else {
        return;
    };
    let local = NSPoint::new(x / scale, y / scale);
    let on_screen = window.convertPointToScreen(view.convertPoint_toView(local, None));
    // SAFETY: the position is a Core Graphics point and the warp runs on the main thread.
    unsafe {
        CGWarpMouseCursorPosition(NSPoint::new(
            on_screen.x,
            main.frame().size.height - on_screen.y,
        ));
        CGAssociateMouseAndMouseCursorPosition(1);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
pub fn warp_pointer(_window: &baseview::WindowContext, _x: f64, _y: f64, _scale: f64) {}
