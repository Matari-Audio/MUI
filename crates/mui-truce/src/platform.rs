//! The two pieces of `truce_gui::platform` the editor uses, ported so the
//! plugin does not link truce-gui, whose unconditional wgpu 29 would sit in
//! the graph next to MUI's wgpu 30.
//!
//! Ported from truce-gui 6.3.0 `src/platform.rs`
//! (<https://github.com/truce-audio/truce>), licensed
//! `LicenseRef-TruceLicense-1.0`.
use raw_window_handle as rwh;
use truce_core::editor::RawWindowHandle;

/// Truce's parent handle as the raw-window-handle 0.6 baseview takes: what
/// [`mui_baseview::open`] takes. A null handle is `Unavailable`.
pub struct ParentWindow(pub RawWindowHandle);

impl rwh::HasWindowHandle for ParentWindow {
    fn window_handle(&self) -> Result<rwh::WindowHandle<'_>, rwh::HandleError> {
        let null = || rwh::HandleError::Unavailable;
        let raw = match self.0 {
            RawWindowHandle::AppKit(ptr) => rwh::RawWindowHandle::AppKit(
                rwh::AppKitWindowHandle::new(std::ptr::NonNull::new(ptr).ok_or_else(null)?),
            ),
            RawWindowHandle::UiKit(ptr) => rwh::RawWindowHandle::UiKit(
                rwh::UiKitWindowHandle::new(std::ptr::NonNull::new(ptr).ok_or_else(null)?),
            ),
            RawWindowHandle::Win32(ptr) => {
                rwh::RawWindowHandle::Win32(rwh::Win32WindowHandle::new(
                    std::num::NonZeroIsize::new(ptr as isize).ok_or_else(null)?,
                ))
            }
            // An XID is 29 bits; baseview takes an Xlib or an XCB parent.
            RawWindowHandle::X11(id) => rwh::RawWindowHandle::Xlib(rwh::XlibWindowHandle::new(
                std::os::raw::c_ulong::try_from(id)
                    .ok()
                    .filter(|&id| id != 0)
                    .ok_or_else(null)?,
            )),
        };
        // SAFETY: the handle is the host's live parent window, which the host
        // keeps alive for as long as the editor is open; this only re-types it.
        #[expect(unsafe_code, reason = "borrow_raw is how a raw handle is lent")]
        Ok(unsafe { rwh::WindowHandle::borrow_raw(raw) })
    }
}

/// The scale of one editor, retained across that editor's close/reopen.
/// Feed it `Editor::set_scale_factor` and `set_uses_system_scale`.
/// A framework that recreates editors must replay its per-plugin scale;
/// borrowing a different editor's scale is never a safe fallback.
#[derive(Clone, Copy, Debug, Default)]
pub struct HostScale {
    host: Option<f64>,
    system: bool,
}

impl HostScale {
    /// The host's content scale; ignored unless finite and positive.
    pub fn set(&mut self, factor: f64) {
        if factor.is_finite() && factor > 0.0 {
            self.host = Some(factor);
        }
    }
    /// The host asks the editor to follow the system scale.
    pub fn set_uses_system(&mut self, yes: bool) {
        self.system = yes;
    }
    /// This editor's host scale; `None` until a valid value was supplied.
    pub fn get(&self) -> Option<f64> {
        self.host
    }
    /// The window's scale override, `None` for the OS scale. Linux: an
    /// embedded editor follows the host's scale, not the desktop's, which a
    /// non-DPI-aware host does not share. Elsewhere the OS reports a
    /// reliable per-window scale.
    pub fn policy(&self) -> Option<f64> {
        let host = self.get();
        editor_window_scale(cfg!(target_os = "macos"), self.system, host)
    }
}

/// macOS always follows AppKit's backing scale. Windows and X11 use the
/// host's explicit content scale, defaulting to 1 on X11 only. The same
/// policy applies before and after native creation.
fn editor_window_scale(macos: bool, uses_system_scale: bool, host: Option<f64>) -> Option<f64> {
    if macos || uses_system_scale {
        None
    } else {
        host.or_else(|| cfg!(target_os = "linux").then_some(1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_scale_never_overrides_appkit_or_system_scale() {
        assert_eq!(editor_window_scale(true, false, Some(2.0)), None);
        assert_eq!(editor_window_scale(false, true, Some(2.0)), None);
        assert_eq!(editor_window_scale(false, false, Some(2.0)), Some(2.0));
    }

    #[test]
    fn invalid_scale_does_not_replace_the_last_valid_scale() {
        let mut scale = HostScale::default();
        scale.set(1.5);
        for factor in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            scale.set(factor);
            assert_eq!(scale.get(), Some(1.5));
        }
    }
}
