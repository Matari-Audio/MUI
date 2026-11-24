//! Windows DirectComposition GPU presenter for `mui-hal`'s D3D11 backend.
//!
//! Creates a flip-model DXGI 1.2 swapchain (`DXGI_SWAP_EFFECT_FLIP_DISCARD`,
//! `BufferCount >= 2`) via `IDXGIFactory2::CreateSwapChainForComposition`,
//! attaches it to a DirectComposition visual bound to the host `HWND` via
//! `DCompositionCreateDevice` → `CreateTargetForHwnd` → `CreateVisual` →
//! `SetContent` → `SetRoot` → `Commit`, and tears down non-blockingly on drop.

#![expect(
    unsafe_code,
    reason = "Win32 DirectComposition, DXGI 1.2, and D3D11 COM interop"
)]

use raw_window_handle::RawWindowHandle;
use windows::Win32::Foundation::{DXGI_STATUS_OCCLUDED, FALSE, HWND};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM,
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET, DXGI_PRESENT, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_DISCARD,
    DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter, IDXGIDevice, IDXGIFactory2, IDXGISwapChain1,
};
use windows::core::Interface;

use crate::{PresentError, PresentMode, PresentationBackend, SurfaceFormat};

/// Minimum buffer count required by DXGI 1.2 flip-model composition swapchains.
pub const MIN_FLIP_BUFFER_COUNT: u32 = 2;
/// Maximum buffer count clamped for MUI DirectComposition swapchains.
pub const MAX_FLIP_BUFFER_COUNT: u32 = 3;

fn dxgi_format(format: SurfaceFormat) -> DXGI_FORMAT {
    match format {
        SurfaceFormat::Bgra8Unorm => DXGI_FORMAT_B8G8R8A8_UNORM,
        SurfaceFormat::Rgba8Unorm => DXGI_FORMAT_R8G8B8A8_UNORM,
    }
}

const fn sync_interval_for(mode: PresentMode) -> u32 {
    match mode {
        PresentMode::Immediate => 0,
        PresentMode::Mailbox | PresentMode::Fifo | PresentMode::FifoRelaxed => 1,
    }
}

/// Owned Windows DirectComposition visual + DXGI 1.2 flip-model swapchain.
///
/// Field drop order matters: the swapchain releases first, then the visual and
/// target, then the DirectComposition device after `Drop` detaches the tree
/// and commits without blocking.
pub struct DirectCompositionPresenter {
    swap_chain: IDXGISwapChain1,
    visual: IDCompositionVisual,
    target: IDCompositionTarget,
    dcomp_device: IDCompositionDevice,
    size: (u32, u32),
    format: SurfaceFormat,
    buffer_count: u32,
    translucent: bool,
    sync_interval: u32,
}

// SAFETY: Every COM interface stored here (`IDXGISwapChain1`, `IDCompositionDevice`,
// `IDCompositionTarget`, `IDCompositionVisual`) is agile/free-threaded for our
// externally synchronized surface owner (`Mutex` in `mui-hal`), and `Drop`
// performs no thread-affine Win32 message-pump calls.
unsafe impl Send for DirectCompositionPresenter {}

impl DirectCompositionPresenter {
    /// Creates a DirectComposition swapchain bound to a live Win32 `HWND` and
    /// `ID3D11Device`.
    ///
    /// # Safety
    /// `window` must hold a live `RawWindowHandle::Win32` whose `HWND` remains
    /// valid until this presenter is dropped.
    ///
    /// # Errors
    /// Returns [`PresentError`] when `size` is zero, `window` is not Win32, or
    /// any DXGI / DirectComposition COM call fails.
    pub unsafe fn new(
        window: RawWindowHandle,
        d3d_device: &ID3D11Device,
        size: (u32, u32),
        format: SurfaceFormat,
        buffer_count: u32,
        translucent: bool,
        present_mode: PresentMode,
    ) -> Result<Self, PresentError> {
        let RawWindowHandle::Win32(handle) = window else {
            return Err(PresentError::UnsupportedHandle(
                "expected Win32 handle on Windows",
            ));
        };
        if size.0 == 0 || size.1 == 0 {
            return Err(PresentError::InvalidSize(size));
        }
        let hwnd = HWND(handle.hwnd.get() as *mut _);
        let buffer_count = buffer_count.clamp(MIN_FLIP_BUFFER_COUNT, MAX_FLIP_BUFFER_COUNT);

        let dxgi_device: IDXGIDevice = d3d_device
            .cast()
            .map_err(|e| PresentError::Platform(format!("QueryInterface(IDXGIDevice): {e}")))?;
        // SAFETY: `dxgi_device` is a live COM object backed by `d3d_device`.
        let dxgi_adapter: IDXGIAdapter = unsafe { dxgi_device.GetAdapter() }
            .map_err(|e| PresentError::Platform(format!("IDXGIDevice::GetAdapter: {e}")))?;
        // SAFETY: `dxgi_adapter` has a parent DXGI 1.2 factory on Windows 8+.
        let dxgi_factory: IDXGIFactory2 = unsafe { dxgi_adapter.GetParent() }.map_err(|e| {
            PresentError::Platform(format!("IDXGIAdapter::GetParent(IDXGIFactory2): {e}"))
        })?;

        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: size.0,
            Height: size.1,
            Format: dxgi_format(format),
            Stereo: FALSE,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: buffer_count,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            AlphaMode: if translucent {
                DXGI_ALPHA_MODE_PREMULTIPLIED
            } else {
                DXGI_ALPHA_MODE_IGNORE
            },
            Flags: 0,
        };

        // SAFETY: `d3d_device` is a valid `ID3D11Device` (`IUnknown`), `desc`
        // satisfies DXGI 1.2 flip-model composition invariants (`BufferCount >= 2`,
        // `DXGI_SWAP_EFFECT_FLIP_DISCARD`, `DXGI_SCALING_STRETCH`).
        let swap_chain =
            unsafe { dxgi_factory.CreateSwapChainForComposition(d3d_device, &desc, None) }
                .map_err(|e| {
                    PresentError::Platform(format!(
                        "IDXGIFactory2::CreateSwapChainForComposition: {e}"
                    ))
                })?;

        // SAFETY: `dxgi_device` is the live DXGI device associated with `d3d_device`.
        let dcomp_device: IDCompositionDevice =
            unsafe { DCompositionCreateDevice(&dxgi_device) }
                .map_err(|e| PresentError::Platform(format!("DCompositionCreateDevice: {e}")))?;
        // SAFETY: caller guarantees `hwnd` is live; `topmost = false` keeps normal Z-order.
        let target = unsafe { dcomp_device.CreateTargetForHwnd(hwnd, false) }.map_err(|e| {
            PresentError::Platform(format!("IDCompositionDevice::CreateTargetForHwnd: {e}"))
        })?;
        // SAFETY: creates a visual owned by `dcomp_device`.
        let visual = unsafe { dcomp_device.CreateVisual() }.map_err(|e| {
            PresentError::Platform(format!("IDCompositionDevice::CreateVisual: {e}"))
        })?;
        // SAFETY: `swap_chain` is a composition swapchain created for this device.
        unsafe { visual.SetContent(&swap_chain) }
            .map_err(|e| PresentError::Platform(format!("IDCompositionVisual::SetContent: {e}")))?;
        // SAFETY: attaches `visual` as the root of `target`.
        unsafe { target.SetRoot(&visual) }
            .map_err(|e| PresentError::Platform(format!("IDCompositionTarget::SetRoot: {e}")))?;
        // SAFETY: atomically submits the visual tree to DWM.
        unsafe { dcomp_device.Commit() }
            .map_err(|e| PresentError::Platform(format!("IDCompositionDevice::Commit: {e}")))?;

        Ok(Self {
            swap_chain,
            visual,
            target,
            dcomp_device,
            size,
            format,
            buffer_count,
            translucent,
            sync_interval: sync_interval_for(present_mode),
        })
    }

    /// Always returns [`PresentationBackend::WinDirectComposition`].
    #[must_use]
    pub const fn backend(&self) -> PresentationBackend {
        PresentationBackend::WinDirectComposition
    }

    /// Current swapchain dimensions in physical pixels.
    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Current swapchain pixel format.
    #[must_use]
    pub const fn format(&self) -> SurfaceFormat {
        self.format
    }

    /// Whether the swapchain composites premultiplied per-pixel alpha with DWM.
    #[must_use]
    pub const fn translucent(&self) -> bool {
        self.translucent
    }

    /// Configured flip-model buffer count (`2..=3`).
    #[must_use]
    pub const fn buffer_count(&self) -> u32 {
        self.buffer_count
    }

    /// Updates presentation pacing interval from `present_mode`.
    pub const fn set_present_mode(&mut self, present_mode: PresentMode) {
        self.sync_interval = sync_interval_for(present_mode);
    }

    /// Underlying DXGI 1.2 composition swapchain.
    #[must_use]
    pub const fn swap_chain(&self) -> &IDXGISwapChain1 {
        &self.swap_chain
    }

    /// Fetches back buffer `0` (`ID3D11Texture2D`) for the current frame.
    ///
    /// # Errors
    /// Returns [`PresentError::SurfaceLost`] or [`PresentError::Platform`] if
    /// `IDXGISwapChain::GetBuffer` fails.
    pub fn back_buffer(&self) -> Result<ID3D11Texture2D, PresentError> {
        // SAFETY: flip-model composition swapchain always exposes back buffer 0.
        unsafe { self.swap_chain.GetBuffer::<ID3D11Texture2D>(0) }.map_err(|e| {
            let hr = e.code();
            if hr == DXGI_ERROR_DEVICE_REMOVED || hr == DXGI_ERROR_DEVICE_RESET {
                PresentError::SurfaceLost
            } else {
                PresentError::Platform(format!("IDXGISwapChain1::GetBuffer(0): {e}"))
            }
        })
    }

    /// Resizes the swapchain back buffers and commits the DirectComposition tree.
    ///
    /// **Caller invariant:** Every outstanding `ID3D11Texture2D` and
    /// `ID3D11RenderTargetView` referencing the swapchain back buffer MUST be
    /// dropped and unbound from `ID3D11DeviceContext` before calling `resize`,
    /// otherwise DXGI returns `DXGI_ERROR_INVALID_CALL`.
    ///
    /// # Errors
    /// Returns [`PresentError`] when `size` is zero or `ResizeBuffers`/`Commit` fails.
    pub fn resize(&mut self, size: (u32, u32), format: SurfaceFormat) -> Result<(), PresentError> {
        if size.0 == 0 || size.1 == 0 {
            return Err(PresentError::InvalidSize(size));
        }
        // SAFETY: caller has released all back-buffer views and texture references.
        unsafe {
            self.swap_chain.ResizeBuffers(
                self.buffer_count,
                size.0,
                size.1,
                dxgi_format(format),
                DXGI_SWAP_CHAIN_FLAG(0),
            )
        }
        .map_err(|e| {
            let hr = e.code();
            if hr == DXGI_ERROR_DEVICE_REMOVED || hr == DXGI_ERROR_DEVICE_RESET {
                PresentError::SurfaceLost
            } else {
                PresentError::Platform(format!("IDXGISwapChain1::ResizeBuffers: {e}"))
            }
        })?;
        self.size = size;
        self.format = format;
        self.commit()
    }

    /// Commits pending DirectComposition visual-tree state to DWM.
    ///
    /// # Errors
    /// Returns [`PresentError::Platform`] if `IDCompositionDevice::Commit` fails.
    pub fn commit(&self) -> Result<(), PresentError> {
        // SAFETY: `dcomp_device` is live.
        unsafe { self.dcomp_device.Commit() }
            .map_err(|e| PresentError::Platform(format!("IDCompositionDevice::Commit: {e}")))
    }

    /// Presents the current back buffer and commits the composition visual.
    ///
    /// Returns `Ok(true)` when DWM reports `DXGI_STATUS_OCCLUDED`, `Ok(false)`
    /// on a normal present.
    ///
    /// # Errors
    /// Returns [`PresentError::SurfaceLost`] on device removal/reset, or
    /// [`PresentError::Platform`] on other DXGI failures.
    pub fn present(&self) -> Result<bool, PresentError> {
        // SAFETY: `swap_chain` is a live flip-model composition swapchain.
        let hr = unsafe { self.swap_chain.Present(self.sync_interval, DXGI_PRESENT(0)) };
        if hr == DXGI_STATUS_OCCLUDED {
            return Ok(true);
        }
        if hr == DXGI_ERROR_DEVICE_REMOVED || hr == DXGI_ERROR_DEVICE_RESET {
            return Err(PresentError::SurfaceLost);
        }
        if hr.is_err() {
            return Err(PresentError::Platform(format!(
                "IDXGISwapChain1::Present failed: 0x{:08x}",
                hr.0 as u32
            )));
        }
        self.commit()?;
        Ok(false)
    }
}

impl Drop for DirectCompositionPresenter {
    fn drop(&mut self) {
        // Detach the swapchain and root visual and commit non-blockingly so DWM
        // releases the composition surface immediately on window/GPU teardown.
        // SAFETY: all COM pointers are still live until fields drop after `drop()`.
        unsafe {
            let _ = self.visual.SetContent(None);
            let _ = self.target.SetRoot(None);
            let _ = self.dcomp_device.Commit();
        }
    }
}
