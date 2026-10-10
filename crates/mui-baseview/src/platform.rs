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
#[expect(
    unexpected_cfgs,
    reason = "objc 0.2's msg_send! tests the retired `cargo-clippy` feature"
)]
pub fn should_skip_frame(handle: RawWindowHandle) -> bool {
    use objc::{msg_send, sel, sel_impl};

    let view_ptr = match handle {
        RawWindowHandle::AppKit(h) => h.ns_view.as_ptr(),
        _ => return false,
    };

    // SAFETY: `view_ptr` is baseview's live, non-null NSView; `window` and
    // `occlusionState` are plain AppKit queries on the thread that owns it.
    unsafe {
        let view = view_ptr.cast::<objc::runtime::Object>();
        let window: *mut objc::runtime::Object = msg_send![view, window];
        if window.is_null() {
            // Detached from any window - nothing to present into.
            return true;
        }
        // `NSWindowOcclusionStateVisible` == 1 << 1. Bit clear => the
        // window is not visible (minimized or fully covered).
        let state: u64 = msg_send![window, occlusionState];
        state & (1 << 1) == 0
    }
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
