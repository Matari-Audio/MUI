//! baseview's window as a wgpu surface. Both speak raw-window-handle 0.6;
//! moose-baseview fills the Win32 HINSTANCE Vulkan needs.

use raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle, RawDisplayHandle};

/// Keep the native connection alive with the same representation as the surface.
/// On X11, `WindowContext` exposes Xlib but `PlatformHandle` exposes XCB.
#[derive(Debug)]
pub struct Display {
    raw: RawDisplayHandle,
    _owner: baseview::PlatformHandle,
}

impl Display {
    pub fn new(window: &baseview::WindowContext) -> Result<Self, HandleError> {
        Ok(Self {
            raw: window.display_handle()?.as_raw(),
            _owner: window.platform_handle(),
        })
    }
}

// SAFETY: baseview's Send + Sync PlatformHandle retains the same X11
// connection, opened with XInitThreads. AppKit and Windows display handles
// contain no pointers. No thread-bound window operations are exposed here.
#[expect(unsafe_code, reason = "owned native display connection is thread-safe")]
unsafe impl Send for Display {}
// SAFETY: the connection is retained and thread-safe as described above.
#[expect(unsafe_code, reason = "owned native display connection is thread-safe")]
unsafe impl Sync for Display {}

impl HasDisplayHandle for Display {
    #[expect(unsafe_code, reason = "borrows the retained native display connection")]
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        // SAFETY: _owner keeps the connection backing raw alive for this borrow.
        Ok(unsafe { DisplayHandle::borrow_raw(self.raw) })
    }
}

/// # Safety
/// The window must outlive the returned surface.
#[expect(
    unsafe_code,
    reason = "wgpu takes raw native handles only through an unsafe constructor"
)]
pub unsafe fn create(
    instance: &wgpu::Instance,
    window: &baseview::WindowContext,
) -> Option<wgpu::Surface<'static>> {
    // SAFETY: both handles are read from the live `window`, and this
    // function's own contract makes the caller keep that window alive for as
    // long as the returned surface.
    unsafe {
        let target = wgpu::SurfaceTargetUnsafe::from_display_and_window(window, window).ok()?;
        instance.create_surface_unsafe(target).ok()
    }
}
