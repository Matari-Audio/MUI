//! Linux native presentation backends: X11 MIT-SHM/`PutImage`, Wayland
//! `wl_shm` on a dedicated child `wl_subsurface`, and Vulkan `VkSwapchainKHR`.

pub mod vk_swapchain;
pub mod wayland;
pub mod x11;

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::{DamageRect, PresentError, PresentationBackend};

/// The display name of an Xlib `Display*` (`XDisplayString`), for a private
/// connection to the same server. `None` for XCB handles, which carry no
/// name, or when the process has not loaded `libX11`: `$DISPLAY` then.
///
/// # Safety
/// An Xlib handle's display pointer must be live.
#[expect(unsafe_code, reason = "XDisplayString from the host's loaded libX11")]
unsafe fn x11_display_name(display: RawDisplayHandle) -> Option<String> {
    let RawDisplayHandle::Xlib(handle) = display else {
        return None;
    };
    let display = handle.display?;
    // SAFETY: RTLD_NOLOAD only finds the libX11 the host already loaded.
    let lib = unsafe { libc::dlopen(c"libX11.so.6".as_ptr(), libc::RTLD_NOW | libc::RTLD_NOLOAD) };
    if lib.is_null() {
        return None;
    }
    // SAFETY: the symbol is libX11's `char *XDisplayString(Display *)`.
    unsafe {
        let symbol = libc::dlsym(lib, c"XDisplayString".as_ptr());
        let name = (!symbol.is_null()).then(|| {
            let display_string: unsafe extern "C" fn(*mut std::ffi::c_void) -> *const libc::c_char =
                std::mem::transmute(symbol);
            let name = display_string(display.as_ptr());
            (!name.is_null()).then(|| {
                std::ffi::CStr::from_ptr(name)
                    .to_string_lossy()
                    .into_owned()
            })
        });
        libc::dlclose(lib);
        name.flatten()
    }
}

/// Linux CPU presenter dispatching to direct X11 (`x11rb`) or Wayland (`wl_shm`).
pub enum LinuxCpuPresenter {
    /// Direct X11 MIT-SHM / `PutImage` presenter.
    X11(Box<x11::X11Presenter>),
    /// Direct Wayland `wl_shm` presenter on a dedicated child `wl_subsurface`.
    Wayland(Box<wayland::WaylandShmPresenter>),
}

impl LinuxCpuPresenter {
    /// Creates a Linux CPU presenter for an X11 or Wayland window handle.
    ///
    /// # Safety
    /// Display and window handles must remain valid on the calling window thread
    /// until this presenter is dropped.
    ///
    /// # Errors
    /// Returns [`PresentError`] when the handles are unsupported or native
    /// resource setup fails.
    #[expect(
        unsafe_code,
        reason = "dispatches raw X11/Wayland handles to native backends"
    )]
    pub unsafe fn new(
        display: RawDisplayHandle,
        window: RawWindowHandle,
    ) -> Result<Self, PresentError> {
        // SAFETY: the caller keeps the display live for this call.
        let x11_display = unsafe { x11_display_name(display) };
        match (display, window) {
            (_, RawWindowHandle::Xlib(w)) => {
                let id = u32::try_from(w.window)
                    .map_err(|_| PresentError::UnsupportedHandle("Xlib window id overflows u32"))?;
                Ok(Self::X11(Box::new(x11::X11Presenter::new(
                    id,
                    x11_display.as_deref(),
                )?)))
            }
            (_, RawWindowHandle::Xcb(w)) => Ok(Self::X11(Box::new(x11::X11Presenter::new(
                w.window.get(),
                x11_display.as_deref(),
            )?))),
            (RawDisplayHandle::Wayland(d), RawWindowHandle::Wayland(w)) => {
                // SAFETY: caller upholds live Wayland display and surface lifetime.
                let presenter = unsafe {
                    wayland::WaylandShmPresenter::new(d.display.as_ptr(), w.surface.as_ptr())?
                };
                Ok(Self::Wayland(Box::new(presenter)))
            }
            _ => Err(PresentError::UnsupportedHandle(
                "expected Xlib, Xcb, or Wayland handle on Linux",
            )),
        }
    }

    /// Returns the concrete Linux CPU presentation backend currently in use.
    #[must_use]
    pub fn backend(&self) -> PresentationBackend {
        match self {
            Self::X11(x11) => x11.backend(),
            Self::Wayland(_) => PresentationBackend::LinuxWaylandShm,
        }
    }

    /// Updates the display scale factor (used by Wayland `wp_viewport` destination sizing).
    pub fn set_scale_factor(&mut self, scale: f64) {
        if let Self::Wayland(wl) = self {
            wl.set_scale_factor(scale);
        }
    }

    /// Uploads and presents a single dirty rectangle of `0x00RRGGBB` pixels.
    pub fn present_patch(
        &mut self,
        rect: DamageRect,
        pixels_0rgb: &[u32],
        surface_size: (u32, u32),
    ) -> Result<(), PresentError> {
        match self {
            Self::X11(x11) => x11.present_patch(rect, pixels_0rgb, surface_size),
            Self::Wayland(wl) => wl.present_patch(rect, pixels_0rgb, surface_size),
        }
    }

    /// Uploads and presents a full-surface `0x00RRGGBB` frame with explicit damage rects.
    pub fn present_full_with_damage(
        &mut self,
        pixels_0rgb: &[u32],
        surface_size: (u32, u32),
        damage: &[DamageRect],
    ) -> Result<(), PresentError> {
        match self {
            Self::X11(x11) => x11.present_full_with_damage(pixels_0rgb, surface_size, damage),
            Self::Wayland(wl) => wl.present_full_with_damage(pixels_0rgb, surface_size, damage),
        }
    }

    /// Presents a budgeted `src_size` bitmap scaled to fill `dst_size`.
    pub fn present_scaled(
        &mut self,
        src_size: (u32, u32),
        pixels_0rgb: &[u32],
        dst_size: (u32, u32),
    ) -> Result<(), PresentError> {
        match self {
            Self::X11(x11) => x11.present_scaled(src_size, pixels_0rgb, dst_size),
            Self::Wayland(wl) => wl.present_scaled(src_size, pixels_0rgb, dst_size),
        }
    }
}
