//! Surface creation stays on the native window's thread. Vulkan accepts that
//! window's own X11 connection; the shared native instance needs no display.

/// # Safety
/// The window must outlive the returned surface. Call only on its window thread,
/// after it is viewable and has nonzero physical dimensions (AppKit layer work
/// must never run on the background initializer).
#[expect(unsafe_code, reason = "wgpu raw native surface constructor")]
pub unsafe fn create(
    instance: &wgpu::Instance,
    window: &baseview::WindowContext,
) -> Option<wgpu::Surface<'static>> {
    // SAFETY: both handles are borrowed from the live window. The caller owns
    // its surface-before-window teardown and native-thread obligations.
    unsafe {
        let target = wgpu::SurfaceTargetUnsafe::from_display_and_window(window, window).ok()?;
        instance.create_surface_unsafe(target).ok()
    }
}
