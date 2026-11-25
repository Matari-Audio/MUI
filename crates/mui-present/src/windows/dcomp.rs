//! Windows DXGI 1.2 GPU presenters for `mui-hal`'s D3D11 backend.
//!
//! Provides two flip-model presenters (`DXGI_SWAP_EFFECT_FLIP_DISCARD`,
//! `BufferCount >= 2`):
//! - [`HwndFlipSwapChain`]: default for opaque windows (`translucent == false`),
//!   created directly on the host `HWND` via `IDXGIFactory2::CreateSwapChainForHwnd`
//!   with `DXGI_ALPHA_MODE_IGNORE` (avoids DirectComposition visual-tree lifetime
//!   complexity for embedded DAW child HWNDs).
//! - [`DirectCompositionPresenter`]: reserved for translucent windows
//!   (`translucent == true`), created via `IDXGIFactory2::CreateSwapChainForComposition`
//!   with `DXGI_ALPHA_MODE_PREMULTIPLIED` and attached to a DirectComposition
//!   visual bound to the host `HWND` (`DCompositionCreateDevice` ->
//!   `CreateTargetForHwnd` -> `CreateVisual` -> `SetContent` -> `SetRoot` ->
//!   `Commit`), detaching non-blockingly on drop.
//! - [`DxgiSwapChainPresenter`]: unified wrapper selecting [`HwndFlipSwapChain`]
//!   for opaque surfaces and [`DirectCompositionPresenter`] for translucent ones.

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
    DXGI_ALPHA_MODE, DXGI_ALPHA_MODE_IGNORE, DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT,
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET, DXGI_ERROR_INVALID_CALL,
    DXGI_MWA_NO_ALT_ENTER, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT,
    IDXGIAdapter, IDXGIDevice, IDXGIFactory2, IDXGISwapChain1,
};
use windows::core::Interface;

use crate::{PresentError, PresentMode, PresentationBackend, SurfaceFormat};

/// Minimum buffer count required by DXGI 1.2 flip-model swapchains.
pub const MIN_FLIP_BUFFER_COUNT: u32 = 2;
/// Maximum buffer count clamped for MUI DXGI flip-model swapchains.
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

fn extract_hwnd(window: RawWindowHandle) -> Result<HWND, PresentError> {
    let RawWindowHandle::Win32(handle) = window else {
        return Err(PresentError::UnsupportedHandle(
            "expected Win32 handle on Windows",
        ));
    };
    Ok(HWND(handle.hwnd.get() as *mut _))
}

fn resolve_dxgi(d3d_device: &ID3D11Device) -> Result<(IDXGIDevice, IDXGIFactory2), PresentError> {
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
    Ok((dxgi_device, dxgi_factory))
}

fn flip_swap_chain_desc(
    size: (u32, u32),
    format: SurfaceFormat,
    buffer_count: u32,
    alpha_mode: DXGI_ALPHA_MODE,
) -> DXGI_SWAP_CHAIN_DESC1 {
    DXGI_SWAP_CHAIN_DESC1 {
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
        AlphaMode: alpha_mode,
        Flags: 0,
    }
}

fn get_back_buffer(swap_chain: &IDXGISwapChain1) -> Result<ID3D11Texture2D, PresentError> {
    // SAFETY: flip-model swapchain always exposes back buffer 0.
    unsafe { swap_chain.GetBuffer::<ID3D11Texture2D>(0) }.map_err(|e| {
        let hr = e.code();
        if hr == DXGI_ERROR_DEVICE_REMOVED
            || hr == DXGI_ERROR_DEVICE_RESET
            || hr == DXGI_ERROR_INVALID_CALL
        {
            PresentError::SurfaceLost
        } else {
            PresentError::Platform(format!("IDXGISwapChain1::GetBuffer(0): {e}"))
        }
    })
}

fn resize_swap_chain(
    swap_chain: &IDXGISwapChain1,
    buffer_count: u32,
    size: (u32, u32),
    format: SurfaceFormat,
) -> Result<(), PresentError> {
    if size.0 == 0 || size.1 == 0 {
        return Err(PresentError::InvalidSize(size));
    }
    // SAFETY: caller has released all back-buffer views and texture references
    // and holds external synchronization with the D3D11 immediate context.
    unsafe {
        swap_chain.ResizeBuffers(
            buffer_count,
            size.0,
            size.1,
            dxgi_format(format),
            DXGI_SWAP_CHAIN_FLAG(0),
        )
    }
    .map_err(|e| {
        let hr = e.code();
        if hr == DXGI_ERROR_DEVICE_REMOVED
            || hr == DXGI_ERROR_DEVICE_RESET
            || hr == DXGI_ERROR_INVALID_CALL
        {
            PresentError::SurfaceLost
        } else {
            PresentError::Platform(format!("IDXGISwapChain1::ResizeBuffers: {e}"))
        }
    })
}

fn present_swap_chain(
    swap_chain: &IDXGISwapChain1,
    sync_interval: u32,
) -> Result<bool, PresentError> {
    // SAFETY: `swap_chain` is a live flip-model swapchain, externally
    // synchronized with the D3D11 immediate context.
    let hr = unsafe { swap_chain.Present(sync_interval, DXGI_PRESENT(0)) };
    if hr == DXGI_STATUS_OCCLUDED {
        return Ok(true);
    }
    if hr == DXGI_ERROR_DEVICE_REMOVED
        || hr == DXGI_ERROR_DEVICE_RESET
        || hr == DXGI_ERROR_INVALID_CALL
    {
        return Err(PresentError::SurfaceLost);
    }
    if hr.is_err() {
        return Err(PresentError::Platform(format!(
            "IDXGISwapChain1::Present failed: 0x{:08x}",
            hr.0 as u32
        )));
    }
    Ok(false)
}

/// Owned Windows HWND flip-model DXGI 1.2 swapchain (`CreateSwapChainForHwnd`).
///
/// Preferred default for opaque surfaces (`DXGI_ALPHA_MODE_IGNORE`,
/// `DXGI_SWAP_EFFECT_FLIP_DISCARD`): avoids DirectComposition visual-tree
/// lifetime complexity on embedded DAW child HWNDs.
pub struct HwndFlipSwapChain {
    swap_chain: IDXGISwapChain1,
    size: (u32, u32),
    format: SurfaceFormat,
    buffer_count: u32,
    sync_interval: u32,
}

// SAFETY: `IDXGISwapChain1` is agile/free-threaded for our externally
// synchronized surface owner (`Surface::state` + `DeviceShared::state` in
// `mui-hal`), and `Drop` performs no thread-affine Win32 message-pump calls.
unsafe impl Send for HwndFlipSwapChain {}

impl HwndFlipSwapChain {
    /// Creates an opaque flip-model DXGI 1.2 swapchain bound directly to `window`'s
    /// `HWND` via `IDXGIFactory2::CreateSwapChainForHwnd`.
    ///
    /// # Safety
    /// `window` must hold a live `RawWindowHandle::Win32` whose `HWND` remains
    /// valid until this presenter is dropped, and the caller must externally
    /// synchronize with `d3d_device`'s immediate context.
    ///
    /// # Errors
    /// Returns [`PresentError`] when `size` is zero, `window` is not Win32, or
    /// DXGI swapchain creation fails.
    pub unsafe fn new(
        window: RawWindowHandle,
        d3d_device: &ID3D11Device,
        size: (u32, u32),
        format: SurfaceFormat,
        buffer_count: u32,
        present_mode: PresentMode,
    ) -> Result<Self, PresentError> {
        let hwnd = extract_hwnd(window)?;
        if size.0 == 0 || size.1 == 0 {
            return Err(PresentError::InvalidSize(size));
        }
        let buffer_count = buffer_count.clamp(MIN_FLIP_BUFFER_COUNT, MAX_FLIP_BUFFER_COUNT);
        let (_dxgi_device, dxgi_factory) = resolve_dxgi(d3d_device)?;
        let desc = flip_swap_chain_desc(size, format, buffer_count, DXGI_ALPHA_MODE_IGNORE);

        // SAFETY: `d3d_device` is a valid `ID3D11Device` (`IUnknown`), `hwnd` is
        // live per caller contract, and `desc` satisfies DXGI 1.2 flip-model
        // HWND invariants (`BufferCount >= 2`, `DXGI_SWAP_EFFECT_FLIP_DISCARD`,
        // `DXGI_ALPHA_MODE_IGNORE`).
        let swap_chain =
            unsafe { dxgi_factory.CreateSwapChainForHwnd(d3d_device, hwnd, &desc, None, None) }
                .map_err(|e| {
                    PresentError::Platform(format!("IDXGIFactory2::CreateSwapChainForHwnd: {e}"))
                })?;

        // Prevent DXGI from intercepting Alt+Enter inside a DAW host window.
        // SAFETY: `hwnd` is live and associated with `dxgi_factory`.
        let _ = unsafe { dxgi_factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER) };

        Ok(Self {
            swap_chain,
            size,
            format,
            buffer_count,
            sync_interval: sync_interval_for(present_mode),
        })
    }

    /// Always returns [`PresentationBackend::WinDxgiFlip`].
    #[must_use]
    pub const fn backend(&self) -> PresentationBackend {
        PresentationBackend::WinDxgiFlip
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

    /// Configured flip-model buffer count (`2..=3`).
    #[must_use]
    pub const fn buffer_count(&self) -> u32 {
        self.buffer_count
    }

    /// Updates presentation pacing interval from `present_mode`.
    pub const fn set_present_mode(&mut self, present_mode: PresentMode) {
        self.sync_interval = sync_interval_for(present_mode);
    }

    /// Underlying DXGI 1.2 HWND swapchain.
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
        get_back_buffer(&self.swap_chain)
    }

    /// Resizes the swapchain back buffers.
    ///
    /// **Caller invariant:** Every outstanding `ID3D11Texture2D` and
    /// `ID3D11RenderTargetView` referencing the swapchain back buffer MUST be
    /// dropped and unbound from `ID3D11DeviceContext` before calling `resize`,
    /// and the caller must hold external synchronization with the immediate
    /// context.
    ///
    /// # Errors
    /// Returns [`PresentError::InvalidSize`], [`PresentError::SurfaceLost`], or
    /// [`PresentError::Platform`].
    pub fn resize(&mut self, size: (u32, u32), format: SurfaceFormat) -> Result<(), PresentError> {
        resize_swap_chain(&self.swap_chain, self.buffer_count, size, format)?;
        self.size = size;
        self.format = format;
        Ok(())
    }

    /// Presents the current back buffer.
    ///
    /// Returns `Ok(true)` when DWM reports `DXGI_STATUS_OCCLUDED`, `Ok(false)`
    /// on a normal present.
    ///
    /// # Errors
    /// Returns [`PresentError::SurfaceLost`] on device removal/reset/invalid call,
    /// or [`PresentError::Platform`] on other DXGI failures.
    pub fn present(&self) -> Result<bool, PresentError> {
        present_swap_chain(&self.swap_chain, self.sync_interval)
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
// externally synchronized surface owner (`Surface::state` + `DeviceShared::state`
// in `mui-hal`), and `Drop` performs no thread-affine Win32 message-pump calls.
unsafe impl Send for DirectCompositionPresenter {}

impl DirectCompositionPresenter {
    /// Creates a DirectComposition swapchain bound to a live Win32 `HWND` and
    /// `ID3D11Device`.
    ///
    /// # Safety
    /// `window` must hold a live `RawWindowHandle::Win32` whose `HWND` remains
    /// valid until this presenter is dropped, and the caller must externally
    /// synchronize with `d3d_device`'s immediate context.
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
        let hwnd = extract_hwnd(window)?;
        if size.0 == 0 || size.1 == 0 {
            return Err(PresentError::InvalidSize(size));
        }
        let buffer_count = buffer_count.clamp(MIN_FLIP_BUFFER_COUNT, MAX_FLIP_BUFFER_COUNT);
        let (dxgi_device, dxgi_factory) = resolve_dxgi(d3d_device)?;
        let alpha_mode = if translucent {
            DXGI_ALPHA_MODE_PREMULTIPLIED
        } else {
            DXGI_ALPHA_MODE_IGNORE
        };
        let desc = flip_swap_chain_desc(size, format, buffer_count, alpha_mode);

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
        get_back_buffer(&self.swap_chain)
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
        resize_swap_chain(&self.swap_chain, self.buffer_count, size, format)?;
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
    /// Returns [`PresentError::SurfaceLost`] on device removal/reset/invalid call,
    /// or [`PresentError::Platform`] on other DXGI failures.
    pub fn present(&self) -> Result<bool, PresentError> {
        let occluded = present_swap_chain(&self.swap_chain, self.sync_interval)?;
        if occluded {
            return Ok(true);
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

/// Unified Windows DXGI 1.2 flip-model presenter:
/// - `translucent == false` -> [`HwndFlipSwapChain`] (`CreateSwapChainForHwnd`)
/// - `translucent == true` -> [`DirectCompositionPresenter`] (`CreateSwapChainForComposition`)
pub enum DxgiSwapChainPresenter {
    /// Opaque HWND flip-model swapchain (default).
    Hwnd(HwndFlipSwapChain),
    /// Translucent DirectComposition swapchain (`DXGI_ALPHA_MODE_PREMULTIPLIED`).
    DirectComposition(DirectCompositionPresenter),
}

impl DxgiSwapChainPresenter {
    /// Creates an HWND flip swapchain for opaque surfaces or a DirectComposition
    /// presenter when `translucent` is `true`.
    ///
    /// # Safety
    /// `window` must hold a live `RawWindowHandle::Win32` whose `HWND` remains
    /// valid until this presenter is dropped, and the caller must externally
    /// synchronize with `d3d_device`'s immediate context.
    ///
    /// # Errors
    /// Returns [`PresentError`] on invalid handle/size or DXGI/DComp failure.
    pub unsafe fn new(
        window: RawWindowHandle,
        d3d_device: &ID3D11Device,
        size: (u32, u32),
        format: SurfaceFormat,
        buffer_count: u32,
        translucent: bool,
        present_mode: PresentMode,
    ) -> Result<Self, PresentError> {
        if translucent {
            // SAFETY: forwarded from this function's contract.
            let dcomp = unsafe {
                DirectCompositionPresenter::new(
                    window,
                    d3d_device,
                    size,
                    format,
                    buffer_count,
                    true,
                    present_mode,
                )?
            };
            Ok(Self::DirectComposition(dcomp))
        } else {
            // SAFETY: forwarded from this function's contract.
            let hwnd = unsafe {
                HwndFlipSwapChain::new(
                    window,
                    d3d_device,
                    size,
                    format,
                    buffer_count,
                    present_mode,
                )?
            };
            Ok(Self::Hwnd(hwnd))
        }
    }

    #[must_use]
    pub const fn backend(&self) -> PresentationBackend {
        match self {
            Self::Hwnd(p) => p.backend(),
            Self::DirectComposition(p) => p.backend(),
        }
    }

    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        match self {
            Self::Hwnd(p) => p.size(),
            Self::DirectComposition(p) => p.size(),
        }
    }

    #[must_use]
    pub const fn format(&self) -> SurfaceFormat {
        match self {
            Self::Hwnd(p) => p.format(),
            Self::DirectComposition(p) => p.format(),
        }
    }

    #[must_use]
    pub const fn translucent(&self) -> bool {
        match self {
            Self::Hwnd(_) => false,
            Self::DirectComposition(p) => p.translucent(),
        }
    }

    pub const fn set_present_mode(&mut self, present_mode: PresentMode) {
        match self {
            Self::Hwnd(p) => p.set_present_mode(present_mode),
            Self::DirectComposition(p) => p.set_present_mode(present_mode),
        }
    }

    /// Fetches back buffer `0` (`ID3D11Texture2D`) for the current frame.
    ///
    /// # Errors
    /// Returns [`PresentError::SurfaceLost`] or [`PresentError::Platform`].
    pub fn back_buffer(&self) -> Result<ID3D11Texture2D, PresentError> {
        match self {
            Self::Hwnd(p) => p.back_buffer(),
            Self::DirectComposition(p) => p.back_buffer(),
        }
    }

    /// Resizes the swapchain back buffers.
    ///
    /// # Errors
    /// Returns [`PresentError::InvalidSize`], [`PresentError::SurfaceLost`], or
    /// [`PresentError::Platform`].
    pub fn resize(&mut self, size: (u32, u32), format: SurfaceFormat) -> Result<(), PresentError> {
        match self {
            Self::Hwnd(p) => p.resize(size, format),
            Self::DirectComposition(p) => p.resize(size, format),
        }
    }

    /// Presents the current back buffer (`Ok(true)` if `DXGI_STATUS_OCCLUDED`).
    ///
    /// # Errors
    /// Returns [`PresentError::SurfaceLost`] or [`PresentError::Platform`].
    pub fn present(&self) -> Result<bool, PresentError> {
        match self {
            Self::Hwnd(p) => p.present(),
            Self::DirectComposition(p) => p.present(),
        }
    }
}
