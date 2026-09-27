//! The two pieces of `truce_gui::platform` the editor uses, ported so the
//! plugin does not link truce-gui, whose unconditional wgpu 29 would sit in
//! the graph next to MUI's wgpu 30.
//!
//! Ported from truce-gui 6.3.0 `src/platform.rs`
//! (<https://github.com/truce-audio/truce>), licensed
//! `LicenseRef-TruceLicense-1.0`.
use std::sync::atomic::{AtomicU64, Ordering};

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

/// The last scale any host passed to [`HostScale::set`], f64 bits, 0 = never.
///
/// truce's CLAP wrapper builds a new editor on every `gui.create` and tells
/// it the host scale only when the host calls `gui.set_scale`, which hosts
/// that set it once per instance do not repeat, so a reopened editor came
/// back at 1.0 inside a 1.5x frame. (truce's VST3 wrapper replays it.)
// ponytail: one scale per process; per-instance if a host ever mixes scales.
static HOST_SCALE: AtomicU64 = AtomicU64::new(0);

/// What truce's editor tells it about scale, as the scale an embedded
/// window opens with. Feed it `Editor::set_scale_factor` and
/// `set_uses_system_scale`; it remembers the last host scale across editors.
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
            HOST_SCALE.store(factor.to_bits(), Ordering::Relaxed);
        }
    }
    /// The host asks the editor to follow the system scale.
    pub fn set_uses_system(&mut self, yes: bool) {
        self.system = yes;
    }
    /// This editor's host scale, else the last one any editor was given.
    pub fn get(&self) -> Option<f64> {
        self.host.or(match HOST_SCALE.load(Ordering::Relaxed) {
            0 => None,
            bits => Some(f64::from_bits(bits)),
        })
    }
    /// The window's scale override, `None` for the OS scale. Linux: an
    /// embedded editor follows the host's scale, not the desktop's, which a
    /// non-DPI-aware host does not share. Elsewhere the OS reports a
    /// reliable per-window scale.
    pub fn policy(&self) -> Option<f64> {
        let host = self.get();
        editor_window_scale(self.system, host.is_some(), host.unwrap_or(1.0))
    }
}

/// `Some(scale)` to open with that scale override, `None` for the system
/// scale. Linux only: an embedded editor follows the host's content scale
/// (default 1), not `Xft.dpi`, which a non-DPI-aware host does not share.
fn editor_window_scale(
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
