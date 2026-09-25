//! baseview's raw-window-handle 0.5 as a wgpu surface. truce-gui carries
//! the same bridge, but typed against its own wgpu; MUI renders on another.
use raw_window_handle::{
    HasRawDisplayHandle, HasRawWindowHandle, RawDisplayHandle, RawWindowHandle,
};
use wgpu::rwh;

/// # Safety
/// The window must outlive the returned surface.
#[allow(unsafe_code)]
pub unsafe fn create(
    instance: &wgpu::Instance,
    window: &baseview::Window,
) -> Option<wgpu::Surface<'static>> {
    let (display, window) = match (window.raw_display_handle(), window.raw_window_handle()) {
        #[cfg(target_os = "linux")]
        (RawDisplayHandle::Xlib(d), RawWindowHandle::Xlib(w)) => (
            rwh::RawDisplayHandle::Xlib(rwh::XlibDisplayHandle::new(
                std::ptr::NonNull::new(d.display),
                d.screen,
            )),
            rwh::RawWindowHandle::Xlib(rwh::XlibWindowHandle::new(w.window)),
        ),
        #[cfg(target_os = "macos")]
        (_, RawWindowHandle::AppKit(w)) => (
            rwh::RawDisplayHandle::AppKit(rwh::AppKitDisplayHandle::new()),
            rwh::RawWindowHandle::AppKit(rwh::AppKitWindowHandle::new(std::ptr::NonNull::new(
                w.ns_view,
            )?)),
        ),
        #[cfg(target_os = "windows")]
        (_, RawWindowHandle::Win32(w)) => {
            let mut win32 =
                rwh::Win32WindowHandle::new(std::num::NonZeroIsize::new(w.hwnd as isize)?);
            // Vulkan's `vkCreateWin32SurfaceKHR` rejects a null HINSTANCE,
            // and baseview leaves it null.
            unsafe extern "system" {
                fn GetModuleHandleW(name: *const u16) -> isize;
            }
            win32.hinstance =
                std::num::NonZeroIsize::new(unsafe { GetModuleHandleW(std::ptr::null()) });
            (
                rwh::RawDisplayHandle::Windows(rwh::WindowsDisplayHandle::new()),
                rwh::RawWindowHandle::Win32(win32),
            )
        }
        _ => return None,
    };
    unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(display),
            raw_window_handle: window,
        })
    }
    .ok()
}
