//! A window's GPU presentation target, made on the native window's thread
//! (AppKit allows nothing else). On Wayland the GPU draws into a fresh child
//! `wl_subsurface` owned by `mui-present`: Vulkan never attaches to the
//! window's own `wl_surface`, which carries the CPU presenter's `wl_shm`
//! anchor. Elsewhere the surface targets the window itself.
//!
//! The wgpu surface retains its [`Target`] (the child surface, then the
//! window's retained [`PlatformHandle`]), so whichever thread drops the last
//! GPU surface releases both without the window thread waiting for it.

use baseview::PlatformHandle;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};
#[cfg(target_os = "linux")]
use std::sync::Arc;

#[cfg(target_os = "linux")]
type Child = Arc<mui_present::linux::wayland::WaylandSubsurface>;

struct Target {
    /// Before `window`: the child goes while the parent surface is retained.
    #[cfg(target_os = "linux")]
    child: Option<Child>,
    window: PlatformHandle,
}

impl HasWindowHandle for Target {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        #[cfg(target_os = "linux")]
        if let Some(child) = &self.child {
            let surface =
                std::ptr::NonNull::new(child.surface_ptr()).ok_or(HandleError::Unavailable)?;
            let raw = raw_window_handle::WaylandWindowHandle::new(surface);
            #[expect(unsafe_code, reason = "borrows the retained child wl_surface")]
            // SAFETY: `self.child` owns this wl_surface for the borrow's lifetime.
            return Ok(unsafe { WindowHandle::borrow_raw(raw.into()) });
        }
        self.window.window_handle()
    }
}

impl HasDisplayHandle for Target {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.window.display_handle()
    }
}

/// The GPU surface's own layer, kept by the window to follow its size.
#[derive(Clone, Default)]
pub struct Layer {
    /// Before `_window`: the child's drop sends Wayland requests.
    #[cfg(target_os = "linux")]
    child: Option<Child>,
    /// Keeps the display connection and parent surface alive for `child`.
    #[cfg(target_os = "linux")]
    _window: Option<PlatformHandle>,
}

impl Layer {
    /// The window's new logical size: a Wayland child scales its physical
    /// buffer to it through `wp_viewport`. Nothing to do elsewhere.
    pub fn resize(&self, logical: (u32, u32)) {
        #[cfg(target_os = "linux")]
        if let Some(child) = &self.child {
            child.set_logical_size(logical.0, logical.1);
        }
        #[cfg(not(target_os = "linux"))]
        let _ = logical;
    }
}

/// A wgpu surface on `window` (a fresh child on Wayland) sized for `logical`
/// points. Call on the window's thread after its CPU presenter has gone.
pub fn create(
    instance: &wgpu::Instance,
    window: PlatformHandle,
    logical: (u32, u32),
) -> Result<(wgpu::Surface<'static>, Layer), String> {
    #[cfg(target_os = "linux")]
    let child = wayland_child(&window)?;
    let layer = Layer {
        #[cfg(target_os = "linux")]
        _window: child.as_ref().map(|_| window.clone()),
        #[cfg(target_os = "linux")]
        child: child.clone(),
    };
    layer.resize(logical);
    let target = Target {
        #[cfg(target_os = "linux")]
        child,
        window,
    };
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::from(target))
        .map_err(|e| e.to_string())?;
    Ok((surface, layer))
}

#[cfg(target_os = "linux")]
fn wayland_child(window: &PlatformHandle) -> Result<Option<Child>, String> {
    use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
    let display = window.display_handle().map_err(|e| e.to_string())?.as_raw();
    let parent = window.window_handle().map_err(|e| e.to_string())?.as_raw();
    let (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(parent)) = (display, parent)
    else {
        return Ok(None);
    };
    #[expect(
        unsafe_code,
        reason = "borrows the retained Wayland display and surface"
    )]
    // SAFETY: the Target that owns this child keeps `window` (and with it the
    // display connection and parent surface) alive until after the child.
    let child = unsafe {
        mui_present::linux::wayland::WaylandSubsurface::new(
            display.display.as_ptr(),
            parent.surface.as_ptr(),
        )
    }
    .map_err(|e| e.to_string())?;
    Ok(Some(Arc::new(child)))
}
