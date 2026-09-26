//! The two pieces of `truce_gui::platform` the editor uses, ported so the
//! plugin does not link truce-gui, whose unconditional wgpu 29 would sit in
//! the graph next to MUI's wgpu 30.
//!
//! Ported from truce-gui 6.3.0 `src/platform.rs`
//! (<https://github.com/truce-audio/truce>), licensed
//! `LicenseRef-TruceLicense-1.0`.
use raw_window_handle::{HasRawWindowHandle, RawWindowHandle as Rwh};
use truce_core::editor::RawWindowHandle;

/// Truce's parent handle as baseview's raw-window-handle 0.5.
pub(crate) struct ParentWindow(pub RawWindowHandle);

// SAFETY: the handle is the host's live parent window, which the host keeps
// alive for as long as the editor is open; this only re-types it.
#[expect(unsafe_code, reason = "HasRawWindowHandle is an unsafe trait")]
unsafe impl HasRawWindowHandle for ParentWindow {
    fn raw_window_handle(&self) -> Rwh {
        match self.0 {
            RawWindowHandle::AppKit(ptr) => {
                let mut handle = raw_window_handle::AppKitWindowHandle::empty();
                handle.ns_view = ptr;
                Rwh::AppKit(handle)
            }
            RawWindowHandle::UiKit(ptr) => {
                let mut handle = raw_window_handle::UiKitWindowHandle::empty();
                handle.ui_view = ptr;
                Rwh::UiKit(handle)
            }
            RawWindowHandle::Win32(ptr) => {
                let mut handle = raw_window_handle::Win32WindowHandle::empty();
                handle.hwnd = ptr;
                Rwh::Win32(handle)
            }
            RawWindowHandle::X11(window_id) => {
                let mut handle = raw_window_handle::XlibWindowHandle::empty();
                // rwh 0.5's field is c_ulong: u32 on Windows, where an XID
                // never reaches.
                #[cfg_attr(
                    windows,
                    expect(clippy::cast_possible_truncation, reason = "c_ulong is u32")
                )]
                {
                    handle.window = window_id as _;
                }
                Rwh::Xlib(handle)
            }
        }
    }
}

/// `Some(scale)` to open with `ScaleFactor(scale)`, `None` for the system
/// scale. Linux only: an embedded editor follows the host's content scale
/// (default 1), not `Xft.dpi`, which a non-DPI-aware host does not share.
pub(crate) fn editor_window_scale(
    uses_system_scale: bool,
    host_scale_set: bool,
    host_scale: f64,
) -> Option<f64> {
    if !cfg!(target_os = "linux") || uses_system_scale {
        None
    } else if host_scale_set && host_scale.is_finite() && host_scale > 0.0 {
        Some(host_scale)
    } else {
        Some(1.0)
    }
}
