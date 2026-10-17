//! Shared HWND/GL primitives. Native creation and callback ownership live only
//! in platform::win::native; this module does not create or pump windows.
mod handle;
pub use handle::HWnd;

#[cfg(feature = "opengl")]
mod dc;
#[cfg(feature = "opengl")]
pub use dc::*;
#[cfg(feature = "opengl")]
mod wgl;
#[cfg(feature = "opengl")]
pub use wgl::*;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};

pub trait WindowImpl: 'static {
    fn non_client_create(
        &self, window: HWnd,
    ) -> core::result::Result<(), crate::platform::PlatformError>;

    /// Called during the processing of the WM_CREATE message, but after this type was properly
    /// initialized.
    ///
    /// Note that any messages sent to the window during this function will result in
    /// [`handle_message`] to be called immediately. Implementations must be ready for that.
    ///
    /// If this returns an error, the window creation is canceled.
    fn after_create(
        &self, window: HWnd,
    ) -> core::result::Result<(), crate::platform::PlatformError>;
    unsafe fn handle_message(
        &self, window: HWnd, message_code: u32, w_param: WPARAM, l_param: LPARAM,
    ) -> Option<LRESULT>;
    /// Called during the processing of the WM_DESTROY message, but before any other de-initialization
    /// takes place.
    ///
    /// Note that any messages sent to the window during this function will result in
    /// [`handle_message`] to be called immediately. Implementations must be ready for that.
    ///
    /// This function is not fallible. Any errors will be ignored.
    fn before_destroy(&self, window: HWnd);
}
