//! Owned native presentation layer for MUI.
//!
//! Replaces `softbuffer`, out-of-`WM_PAINT` Win32 `GetDC`/`StretchDIBits`, and
//! macOS `CGImage` layer mosaics with direct, zero-wait native presenters:
//!
//! - **Windows:** retained `WM_PAINT` `CreateDIBSection` CPU presenter; GPU
//!   frames use wgpu's DirectComposition visual on the same `HWND`.
//! - **macOS:** 3-slot `IOSurface` ring bound directly to a dedicated
//!   `CALayer.contents`.
//! - **Linux:** direct `x11rb` MIT-SHM (with `PutImage` fallback), direct
//!   Wayland `wl_shm` double-buffered ring on a dedicated child `wl_subsurface`,
//!   and an owned `ash` `VkSwapchain` (using its own fresh child `wl_subsurface`
//!   on Wayland so `wl_shm` and Vulkan never touch the same `wl_surface`).

#![deny(unsafe_code)]

pub mod sentinel;
pub mod state;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

pub use sentinel::{CrashSentinel, DriverId, SentinelGuard};
pub use state::{
    FallbackReason, GraphicsApi, PresentationBackend, PresentationMode, PresentationState,
};

/// Maximum pixels allowed in a 1:1 CPU software buffer (64 MiB at 4 Bpp).
pub const MAX_CPU_PIXELS: u64 = 16 * 1024 * 1024;

/// Process-wide user software-mode toggle (independent of environment variables).
static USER_SOFTWARE_MODE: AtomicBool = AtomicBool::new(false);

/// Enables or disables user-requested software-only presentation for new windows.
///
/// Useful on drivers (such as proprietary NVIDIA on Linux) that ignore
/// `LIBGL_ALWAYS_SOFTWARE=1`.
pub fn set_user_software_mode(enabled: bool) {
    USER_SOFTWARE_MODE.store(enabled, Ordering::Release);
}

/// Returns whether user-requested software mode is currently enabled.
#[must_use]
pub fn user_software_mode() -> bool {
    USER_SOFTWARE_MODE.load(Ordering::Acquire)
}

/// Returns `true` if CPU-only software rendering is forced via either the user
/// toggle ([`set_user_software_mode`]) or environment variables (`MUI_FORCE_CPU=1`
/// / `true`, or `MUI_RENDERER=cpu`).
#[must_use]
pub fn software_mode_enabled() -> bool {
    user_software_mode()
        || force_cpu_env(
            std::env::var("MUI_FORCE_CPU").ok().as_deref(),
            std::env::var("MUI_RENDERER").ok().as_deref(),
        )
}

fn force_cpu_env(force_cpu: Option<&str>, renderer: Option<&str>) -> bool {
    force_cpu.is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        || renderer.is_some_and(|v| v.eq_ignore_ascii_case("cpu"))
}

/// Damaged rectangle in physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DamageRect {
    /// Left coordinate in physical pixels.
    pub x: u32,
    /// Top coordinate in physical pixels.
    pub y: u32,
    /// Width in physical pixels.
    pub width: u32,
    /// Height in physical pixels.
    pub height: u32,
}

impl DamageRect {
    /// Creates a damage rectangle from `[x, y, width, height]`.
    #[must_use]
    pub const fn from_array([x, y, width, height]: [u32; 4]) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Converts this rectangle to `[x, y, width, height]`.
    #[must_use]
    pub const fn to_array(self) -> [u32; 4] {
        [self.x, self.y, self.width, self.height]
    }

    /// Returns `true` when either dimension is zero.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// Surface pixel format for GPU swapchains and CPU bitmaps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SurfaceFormat {
    /// 8-bit BGRA, linear/non-sRGB UNORM.
    Bgra8Unorm,
    /// 8-bit RGBA, linear/non-sRGB UNORM.
    Rgba8Unorm,
}

/// Swapchain presentation pacing preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PresentMode {
    /// Low-latency tear-free triple-buffered queue (preferred when offered).
    Mailbox,
    /// Standard vsynced FIFO queue.
    Fifo,
    /// Adaptive vsync FIFO that allows tearing on late frames.
    FifoRelaxed,
    /// Uncapped immediate presentation.
    Immediate,
}

impl PresentMode {
    /// Selects the highest-ranked mode present in `offered`.
    #[must_use]
    pub fn choose(offered: &[Self]) -> Self {
        [
            Self::Mailbox,
            Self::Fifo,
            Self::FifoRelaxed,
            Self::Immediate,
        ]
        .into_iter()
        .find(|mode| offered.contains(mode))
        .unwrap_or(Self::Fifo)
    }
}

/// Errors produced by native presentation backends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresentError {
    /// Requested surface size is zero or exceeds platform/budget bounds.
    InvalidSize((u32, u32)),
    /// Damage rectangle or pixel slice length does not match.
    InvalidPatch,
    /// The supplied raw window/display handle is unsupported on this OS.
    UnsupportedHandle(&'static str),
    /// Swapchain or native surface is no longer valid and must be recreated.
    SurfaceLost,
    /// Swapchain extent/transform is out of date and needs reconfiguration.
    OutOfDate,
    /// No image became available within the timeout; try again next frame.
    Timeout,
    /// A GPU driver previously crashed on this adapter; CPU fallback is required.
    DriverSentinelTripped(DriverId),
    /// Platform API failure with a concise description.
    Platform(String),
}

impl fmt::Display for PresentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSize((w, h)) => write!(f, "invalid presentation surface size {w}x{h}"),
            Self::InvalidPatch => f.write_str("invalid bitmap patch geometry or pixel count"),
            Self::UnsupportedHandle(msg) => write!(f, "unsupported native handle: {msg}"),
            Self::SurfaceLost => f.write_str("native presentation surface lost"),
            Self::OutOfDate => f.write_str("native swapchain out of date"),
            Self::Timeout => f.write_str("no swapchain image became available in time"),
            Self::DriverSentinelTripped(driver) => {
                write!(f, "crash sentinel tripped for GPU driver {driver}")
            }
            Self::Platform(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for PresentError {}

/// Validates a 1:1 CPU target size against zero and [`MAX_CPU_PIXELS`].
pub(crate) fn validate_cpu_size(size: (u32, u32)) -> Result<(), PresentError> {
    if size.0 == 0 || size.1 == 0 || u64::from(size.0) * u64::from(size.1) > MAX_CPU_PIXELS {
        return Err(PresentError::InvalidSize(size));
    }
    Ok(())
}

/// Validates that `rect` fits inside `surface_size` and matches `pixels.len()`.
pub(crate) fn validate_patch(
    rect: DamageRect,
    pixels_len: usize,
    surface_size: (u32, u32),
) -> Result<(), PresentError> {
    validate_cpu_size(surface_size)?;
    let expected = (rect.width as usize).checked_mul(rect.height as usize);
    if rect.is_empty() || !rect_fits(rect, surface_size) || expected != Some(pixels_len) {
        return Err(PresentError::InvalidPatch);
    }
    Ok(())
}

/// Returns `true` when `rect` lies entirely inside `surface_size`.
fn rect_fits(rect: DamageRect, surface_size: (u32, u32)) -> bool {
    rect.x
        .checked_add(rect.width)
        .is_some_and(|r| r <= surface_size.0)
        && rect
            .y
            .checked_add(rect.height)
            .is_some_and(|b| b <= surface_size.1)
}

/// Validates a full-frame upload: size, buffer length and every nonempty
/// damage rect (an empty damage list means "repaint everything").
fn validate_full_frame(
    pixels_len: usize,
    surface_size: (u32, u32),
    damage: &[DamageRect],
) -> Result<(), PresentError> {
    validate_cpu_size(surface_size)?;
    if pixels_len != surface_size.0 as usize * surface_size.1 as usize
        || damage
            .iter()
            .any(|&r| !r.is_empty() && !rect_fits(r, surface_size))
    {
        return Err(PresentError::InvalidPatch);
    }
    Ok(())
}

/// Nearest-neighbor upscale from `src` (`src_size`) into `dst` (`dst_size`),
/// ORing each pixel with `opaque_mask`.
#[cfg_attr(
    not(any(target_os = "linux", target_os = "macos", test)),
    expect(
        dead_code,
        reason = "used by Linux/macOS CPU presenters and unit tests"
    )
)]
pub(crate) fn blit_scaled_0rgb(
    src: &[u32],
    src_size: (u32, u32),
    dst: &mut [u32],
    dst_size: (u32, u32),
    dst_stride_pixels: usize,
    opaque_mask: u32,
) {
    let (sw, sh) = (src_size.0 as usize, src_size.1 as usize);
    let (dw, dh) = (dst_size.0 as usize, dst_size.1 as usize);
    for dy in 0..dh {
        let sy = (dy * sh) / dh;
        let src_row = &src[sy * sw..(sy + 1) * sw];
        let dst_row = &mut dst[dy * dst_stride_pixels..dy * dst_stride_pixels + dw];
        for (dx, out) in dst_row.iter_mut().enumerate() {
            let sx = (dx * sw) / dw;
            *out = opaque_mask | src_row[sx];
        }
    }
}

/// Owned native CPU pixel presenter for a live OS window.
///
/// Accepts packed `0x00RRGGBB` pixel patches (identical to MUI's internal
/// software transport), retains the full frame on the native side, and delivers
/// dirty rectangles via the optimal zero-`softbuffer` OS mechanism.
///
/// Dropping [`CpuPresenter`] tears down native presentation objects immediately
/// without blocking on compositor callbacks or background threads.
pub struct CpuPresenter {
    inner: PlatformCpuPresenter,
}

#[cfg(target_os = "linux")]
type PlatformCpuPresenter = linux::LinuxCpuPresenter;

#[cfg(target_os = "macos")]
type PlatformCpuPresenter = macos::IoSurfacePresenter;

#[cfg(target_os = "windows")]
type PlatformCpuPresenter = windows::DibWmPaintPresenter;

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
struct PlatformCpuPresenter;

impl CpuPresenter {
    /// Creates a native CPU presenter attached to the live window handles.
    ///
    /// # Safety
    /// The caller must invoke this on the window's owning thread and ensure
    /// that the underlying native display and window handles remain live until
    /// this [`CpuPresenter`] is dropped.
    ///
    /// # Errors
    /// Returns [`PresentError`] when the handle kind is unsupported or native
    /// presentation initialization fails.
    #[expect(
        unsafe_code,
        reason = "borrows raw OS window/display handles from caller"
    )]
    pub unsafe fn new(
        display: RawDisplayHandle,
        window: RawWindowHandle,
    ) -> Result<Self, PresentError> {
        #[cfg(target_os = "linux")]
        // SAFETY: forwarded from this function's contract.
        let inner = unsafe { linux::LinuxCpuPresenter::new(display, window)? };
        #[cfg(target_os = "macos")]
        let inner = {
            let _ = display;
            // SAFETY: forwarded from this function's contract.
            unsafe { macos::IoSurfacePresenter::new(window)? }
        };
        #[cfg(target_os = "windows")]
        let inner = {
            let _ = display;
            // SAFETY: forwarded from this function's contract.
            unsafe { windows::DibWmPaintPresenter::new(window)? }
        };
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            let _ = (display, window);
            Err(PresentError::UnsupportedHandle(
                "no native CPU presenter on this target",
            ))
        }
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        Ok(Self { inner })
    }

    /// Reports which native backend is backing this presenter.
    #[must_use]
    pub fn backend(&self) -> PresentationBackend {
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        {
            self.inner.backend()
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            let _ = &self.inner;
            PresentationBackend::Offscreen
        }
    }

    /// Updates the display scale factor (used by Wayland `wp_viewport` destination sizing).
    pub fn set_scale_factor(&mut self, scale: f64) {
        #[cfg(target_os = "linux")]
        self.inner.set_scale_factor(scale);
        #[cfg(not(target_os = "linux"))]
        let _ = scale;
    }

    /// Uploads and presents a single dirty rectangle of `0x00RRGGBB` pixels.
    ///
    /// # Errors
    /// Returns [`PresentError`] if the dimensions or patch bounds are invalid
    /// or the platform presentation call fails.
    pub fn present_patch(
        &mut self,
        rect: DamageRect,
        pixels_0rgb: &[u32],
        surface_size: (u32, u32),
    ) -> Result<(), PresentError> {
        validate_patch(rect, pixels_0rgb.len(), surface_size)?;
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        {
            self.inner.present_patch(rect, pixels_0rgb, surface_size)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            let _ = (rect, pixels_0rgb, surface_size);
            Err(PresentError::UnsupportedHandle("unsupported target"))
        }
    }

    /// Uploads and presents a full-surface `0x00RRGGBB` frame with explicit damage rects.
    ///
    /// # Errors
    /// Returns [`PresentError`] if dimensions or buffer length are invalid or
    /// the platform presentation call fails.
    pub fn present_full_with_damage(
        &mut self,
        pixels_0rgb: &[u32],
        surface_size: (u32, u32),
        damage: &[DamageRect],
    ) -> Result<(), PresentError> {
        validate_full_frame(pixels_0rgb.len(), surface_size, damage)?;
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        {
            self.inner
                .present_full_with_damage(pixels_0rgb, surface_size, damage)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            let _ = (pixels_0rgb, surface_size, damage);
            Err(PresentError::UnsupportedHandle("unsupported target"))
        }
    }

    /// Presents a budgeted `src_size` bitmap stretched to fill `dst_size` physical pixels.
    ///
    /// # Errors
    /// Returns [`PresentError`] if dimensions or pixel counts are invalid or
    /// the platform presentation call fails.
    pub fn present_scaled(
        &mut self,
        src_size: (u32, u32),
        pixels_0rgb: &[u32],
        dst_size: (u32, u32),
    ) -> Result<(), PresentError> {
        if src_size.0 == 0
            || src_size.1 == 0
            || dst_size.0 == 0
            || dst_size.1 == 0
            || pixels_0rgb.len() != src_size.0 as usize * src_size.1 as usize
        {
            return Err(PresentError::InvalidPatch);
        }
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        {
            self.inner.present_scaled(src_size, pixels_0rgb, dst_size)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            let _ = (src_size, pixels_0rgb, dst_size);
            Err(PresentError::UnsupportedHandle("unsupported target"))
        }
    }

    /// Fills the entire surface with one solid `0x00RRGGBB` color (startup / over-budget).
    ///
    /// # Errors
    /// Returns [`PresentError`] if `surface_size` is empty or presentation fails.
    pub fn fill_solid(
        &mut self,
        rgb_0rgb: u32,
        surface_size: (u32, u32),
    ) -> Result<(), PresentError> {
        if surface_size.0 == 0 || surface_size.1 == 0 {
            return Err(PresentError::InvalidSize(surface_size));
        }
        let rgb = rgb_0rgb & 0x00ff_ffff;
        if u64::from(surface_size.0) * u64::from(surface_size.1) <= MAX_CPU_PIXELS {
            let pixels = vec![rgb; surface_size.0 as usize * surface_size.1 as usize];
            self.present_full_with_damage(
                &pixels,
                surface_size,
                &[DamageRect::from_array([
                    0,
                    0,
                    surface_size.0,
                    surface_size.1,
                ])],
            )
        } else {
            // Over 16 Mi pixels: present a tiny 1x1 solid buffer scaled to fill.
            self.present_scaled((1, 1), &[rgb], surface_size)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn present_mode_ranks_mailbox_then_fifo_before_immediate() {
        assert_eq!(
            PresentMode::choose(&[
                PresentMode::Immediate,
                PresentMode::Fifo,
                PresentMode::Mailbox
            ]),
            PresentMode::Mailbox
        );
        assert_eq!(
            PresentMode::choose(&[PresentMode::Immediate, PresentMode::Fifo]),
            PresentMode::Fifo
        );
        assert_eq!(
            PresentMode::choose(&[PresentMode::Immediate, PresentMode::FifoRelaxed]),
            PresentMode::FifoRelaxed
        );
        assert_eq!(
            PresentMode::choose(&[PresentMode::Immediate]),
            PresentMode::Immediate
        );
        assert_eq!(PresentMode::choose(&[]), PresentMode::Fifo);
    }

    #[test]
    fn force_cpu_env_and_user_toggle_work_independently() {
        assert!(!force_cpu_env(None, None));
        assert!(!force_cpu_env(Some("0"), Some("gpu")));
        assert!(force_cpu_env(Some("1"), None));
        assert!(force_cpu_env(Some("TRUE"), None));
        assert!(force_cpu_env(None, Some("cpu")));
        assert!(force_cpu_env(None, Some("CPU")));

        let prev = user_software_mode();
        set_user_software_mode(true);
        assert!(user_software_mode());
        assert!(software_mode_enabled());
        set_user_software_mode(prev);
    }

    #[test]
    fn full_frame_rejects_out_of_bounds_damage() {
        let r = DamageRect::from_array;
        assert!(validate_full_frame(16, (4, 4), &[]).is_ok());
        assert!(validate_full_frame(16, (4, 4), &[r([0, 0, 4, 4]), r([3, 3, 0, 9])]).is_ok());
        assert!(validate_full_frame(16, (4, 4), &[r([3, 3, 2, 1])]).is_err());
        assert!(validate_full_frame(16, (4, 4), &[r([u32::MAX, 0, 2, 1])]).is_err());
        assert!(validate_full_frame(15, (4, 4), &[]).is_err());
    }

    #[test]
    fn scaled_blit_samples_corners_and_applies_opaque_mask() {
        let src = [0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0x00ff_ffff];
        let mut dst = vec![0u32; 16];
        blit_scaled_0rgb(&src, (2, 2), &mut dst, (4, 4), 4, 0xff00_0000);
        assert_eq!(dst[0], 0xffff_0000);
        assert_eq!(dst[3], 0xff00_ff00);
        assert_eq!(dst[12], 0xff00_00ff);
        assert_eq!(dst[15], 0xffff_ffff);
    }
}
