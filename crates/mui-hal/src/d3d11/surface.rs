//! D3D11 window surface wired to `mui-present`'s `DxgiSwapChainPresenter`
//! (`HwndFlipSwapChain` for opaque surfaces; `DirectCompositionPresenter` when
//! `translucent == true`).
//!
//! **Lock ordering invariant:** Always acquire `Surface::state` FIRST, then
//! `DeviceShared::state` SECOND. Never acquire `Surface::state` while holding
//! `DeviceShared::state`. Every DXGI swapchain call (`new`, `resize`,
//! `back_buffer`, `present`, and swapchain teardown) executes while holding
//! `DeviceShared::state` so DXGI's internal use of the immediate
//! `ID3D11DeviceContext` can never race with `Queue::submit`.
//!
//! **Retirement/loss rule:** swapchain generation retirement (`live = false`)
//! is published only under `DeviceShared::state` (via [`SwapState::invalidate`]),
//! and `Device::submit` validates every command, staged or explicit, under that
//! same lock. Likewise `acquire` checks the HAL loss flag under it. So a
//! submit either runs entirely before a retirement/loss or observes it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use mui_present::windows::DxgiSwapChainPresenter;
use mui_present::{PresentError, PresentMode as PMode, SurfaceFormat};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use super::{Device, Error, Texture, TextureView, lock};
use crate::binding::{d3d11_buffer_count, d3d11_surface_translucent};
use crate::types::{
    CompositeAlphaMode, PresentMode, SurfaceConfiguration, TextureFormat, TextureUsages,
    TextureViewDimension,
};

fn to_present_format(f: TextureFormat) -> Result<SurfaceFormat, Error> {
    match f {
        TextureFormat::Bgra8Unorm => Ok(SurfaceFormat::Bgra8Unorm),
        TextureFormat::Rgba8Unorm => Ok(SurfaceFormat::Rgba8Unorm),
        other => Err(Error::validation(format!(
            "D3D11 DXGI surface does not support format {other:?}"
        ))),
    }
}

fn to_present_mode(m: PresentMode) -> PMode {
    match m {
        PresentMode::Immediate | PresentMode::AutoNoVsync => PMode::Immediate,
        PresentMode::Mailbox => PMode::Mailbox,
        PresentMode::FifoRelaxed => PMode::FifoRelaxed,
        PresentMode::Fifo | PresentMode::AutoVsync => PMode::Fifo,
    }
}

fn map_present_err(device: &Device, err: PresentError) -> Error {
    match err {
        PresentError::SurfaceLost => {
            let _ = device.check_device_removed();
            Error::internal(PresentError::SurfaceLost.to_string())
        }
        other => Error::internal(other.to_string()),
    }
}

struct SwapState {
    presenter: DxgiSwapChainPresenter,
    device: Device,
    format: TextureFormat,
    alpha_mode: CompositeAlphaMode,
    usage: TextureUsages,
    live: Arc<AtomicBool>,
}

impl SwapState {
    /// Marks this swapchain generation retired. Takes `DeviceShared::state`
    /// (caller holds `Surface::state`), so it serializes with `Device::submit`.
    fn invalidate(&self) {
        let _ctx_st = lock(&self.device.0.state);
        self.live.store(false, Ordering::Release);
    }

    /// Destroys the swapchain so a new one can be created on the same `HWND`.
    ///
    /// DXGI defers swapchain destruction until the owning immediate context
    /// flushes, and a flip-model `HWND` accepts only one swapchain at a time. So,
    /// under this device's context lock: unbind the back buffer, release the
    /// presenter, *then* flush, so the destruction has happened before any
    /// device (this one or another) creates the next swapchain on the window.
    fn retire(self) {
        let ctx_st = lock(&self.device.0.state);
        ctx_st.clear_render_targets_and_flush();
        drop(self.presenter);
        ctx_st.flush();
    }
}

struct SurfaceState {
    window: RawWindowHandle,
    swap: Option<SwapState>,
    released: bool,
}

// SAFETY: `RawWindowHandle` is an opaque Win32 `HWND` integer handle kept alive
// by `mui_hal::Surface._target`, and `SurfaceState` is mutex-guarded.
unsafe impl Send for SurfaceState {}

pub struct Surface {
    state: Mutex<SurfaceState>,
}

pub struct SurfaceTexture {
    pub texture: Texture,
    pub view: TextureView,
    device: Device,
    live: Arc<AtomicBool>,
}

pub enum Acquired {
    Image(SurfaceTexture),
    Outdated,
    Occluded,
    Lost,
    Failed(Error),
}

impl Surface {
    /// Creates a D3D11 surface target for a live Win32 `HWND`.
    ///
    /// # Safety
    /// Caller must supply a live `RawWindowHandle::Win32` and keep the `HWND`
    /// alive until `Surface::release` / `Drop`.
    pub unsafe fn new(_display: RawDisplayHandle, window: RawWindowHandle) -> Result<Self, Error> {
        let RawWindowHandle::Win32(_) = window else {
            return Err(Error::validation("expected Win32 handle on Windows"));
        };
        Ok(Self {
            state: Mutex::new(SurfaceState {
                window,
                swap: None,
                released: false,
            }),
        })
    }

    pub fn configure(&self, device: &Device, config: &SurfaceConfiguration) -> Result<(), Error> {
        if config.width == 0 || config.height == 0 {
            return Err(Error::validation("surface dimensions must be non-zero"));
        }
        let fmt = to_present_format(config.format)?;
        let translucent = d3d11_surface_translucent(config.alpha_mode);
        let pmode = to_present_mode(config.present_mode);
        if device.is_lost() {
            return Err(Error::internal("D3D11 device is lost"));
        }

        // Phase 1: retire active back-buffer handles and drain inflight GPU work
        // outside `self.state` so `poll(true)` never violates `Surface::state ->
        // DeviceShared::state` lock ordering.
        let prev_device = {
            let st = lock(&self.state);
            if st.released {
                return Err(Error::validation("surface has been released"));
            }
            st.swap.as_ref().map(|swap| {
                swap.invalidate();
                swap.device.clone()
            })
        };

        if let Some(prev) = prev_device
            && !Arc::ptr_eq(&prev.0, &device.0)
        {
            prev.prune_retired_staged();
            let _ = prev.poll(true);
        }
        device.prune_retired_staged();
        device.poll(true)?;
        if device.is_lost() {
            return Err(Error::internal("D3D11 device is lost"));
        }

        // Phase 2: acquire `Surface::state` FIRST, then `DeviceShared::state`
        // SECOND for every DXGI swapchain resize, teardown, or creation.
        let mut st = lock(&self.state);
        if st.released {
            return Err(Error::validation("surface has been released"));
        }
        if device.is_lost() {
            return Err(Error::internal("D3D11 device is lost"));
        }

        if let Some(swap) = &mut st.swap {
            swap.invalidate();
            let same_device = Arc::ptr_eq(&swap.device.0, &device.0);
            if same_device && swap.presenter.translucent() == translucent {
                let resize_res = {
                    let ctx_st = lock(&device.0.state);
                    if device.is_lost() {
                        return Err(Error::internal("D3D11 device is lost"));
                    }
                    ctx_st.clear_render_targets_and_flush();
                    swap.presenter.set_present_mode(pmode);
                    swap.presenter.resize((config.width, config.height), fmt)
                };
                match resize_res {
                    Ok(()) => {
                        swap.format = config.format;
                        swap.alpha_mode = config.alpha_mode;
                        swap.usage = config.usage;
                        swap.live = Arc::new(AtomicBool::new(true));
                        return Ok(());
                    }
                    Err(PresentError::SurfaceLost) => {
                        // Drop the lost swapchain under the device context lock
                        // so subsequent `acquire` sees an unconfigured/lost surface.
                        if let Some(dead) = st.swap.take() {
                            dead.retire();
                        }
                        drop(st);
                        return Err(map_present_err(device, PresentError::SurfaceLost));
                    }
                    Err(e) => {
                        drop(st);
                        return Err(map_present_err(device, e));
                    }
                }
            }
            // Different device or translucent mode toggled: tear down the old
            // swapchain while holding its own `DeviceShared::state`.
            if let Some(old) = st.swap.take() {
                old.retire();
            }
        }

        let buffer_count = d3d11_buffer_count(config.desired_maximum_frame_latency);
        let presenter_res = {
            let ctx_st = lock(&device.0.state);
            if device.is_lost() {
                return Err(Error::internal("D3D11 device is lost"));
            }
            ctx_st.clear_render_targets_and_flush();
            // SAFETY: `st.window` is a live Win32 `HWND` owned by `Surface._target`,
            // and `ctx_st` serializes with `device`'s immediate context.
            unsafe {
                DxgiSwapChainPresenter::new(
                    st.window,
                    &device.0.raw,
                    (config.width, config.height),
                    fmt,
                    buffer_count,
                    translucent,
                    pmode,
                )
            }
        };
        // Map errors only after releasing `Surface::state`: a first-time device-loss
        // detection fires the loss callback inline, which may read this surface.
        let presenter = match presenter_res {
            Ok(p) => p,
            Err(e) => {
                drop(st);
                return Err(map_present_err(device, e));
            }
        };

        st.swap = Some(SwapState {
            presenter,
            device: device.clone(),
            format: config.format,
            alpha_mode: config.alpha_mode,
            usage: config.usage,
            live: Arc::new(AtomicBool::new(true)),
        });
        Ok(())
    }

    pub fn format(&self) -> Option<TextureFormat> {
        lock(&self.state).swap.as_ref().map(|s| s.format)
    }

    pub fn alpha_mode(&self) -> Option<CompositeAlphaMode> {
        lock(&self.state).swap.as_ref().map(|s| s.alpha_mode)
    }

    pub fn acquire(&self) -> Acquired {
        let st = lock(&self.state);
        if st.released {
            return Acquired::Lost;
        }
        let Some(swap) = &st.swap else {
            return Acquired::Outdated;
        };
        if !swap.live.load(Ordering::Acquire) {
            return Acquired::Outdated;
        }
        // Lock order: `Surface::state` -> `DeviceShared::state`.
        let back_buf = {
            let _ctx_st = lock(&swap.device.0.state);
            if swap.device.is_lost() {
                return Acquired::Lost;
            }
            swap.presenter.back_buffer()
        };
        let back_buf = match back_buf {
            Ok(b) => b,
            Err(PresentError::SurfaceLost) => {
                let dev = swap.device.clone();
                drop(st);
                let _ = dev.check_device_removed();
                return Acquired::Lost;
            }
            Err(PresentError::OutOfDate) => return Acquired::Outdated,
            Err(e) => return Acquired::Failed(Error::internal(e.to_string())),
        };
        let (w, h) = swap.presenter.size();
        let tex = Texture::from_swapchain(
            &swap.device,
            back_buf,
            w,
            h,
            swap.format,
            swap.usage | TextureUsages::RENDER_ATTACHMENT,
            swap.live.clone(),
        );
        let view = match tex.create_view(Some(TextureViewDimension::D2)) {
            Ok(v) => v,
            Err(e) => return Acquired::Failed(e),
        };
        Acquired::Image(SurfaceTexture {
            texture: tex,
            view,
            device: swap.device.clone(),
            live: swap.live.clone(),
        })
    }

    pub fn present(&self, frame: SurfaceTexture) -> Result<(), Error> {
        if !frame.live.load(Ordering::Acquire) {
            return Err(Error::validation(
                "cannot present a back buffer from a reconfigured or released DXGI swapchain",
            ));
        }
        frame.device.flush_staged_before_present()?;
        // Drop back-buffer texture and RTV before presenting so the flip chain
        // holds no extra references, while retaining `live` to verify surface
        // ownership and swapchain generation under `self.state`.
        let SurfaceTexture {
            texture,
            view,
            device,
            live,
        } = frame;
        drop(view);
        drop(texture);

        let present_res = {
            let st = lock(&self.state);
            if st.released {
                return Err(Error::validation(
                    "cannot present to a released DXGI surface",
                ));
            }
            let Some(swap) = &st.swap else {
                return Err(Error::validation(
                    "cannot present to an unconfigured or released surface",
                ));
            };
            if !Arc::ptr_eq(&swap.live, &live) || !live.load(Ordering::Acquire) {
                return Err(Error::validation(
                    "cannot present a back buffer from another surface or replaced DXGI swapchain",
                ));
            }
            // Lock order: `Surface::state` -> `DeviceShared::state`.
            // Clear OM targets + flush AND call `IDXGISwapChain1::Present` under
            // the same context lock so no concurrent `Queue::submit` can race.
            let ctx_st = lock(&swap.device.0.state);
            ctx_st.clear_render_targets_and_flush();
            swap.presenter.present()
        };
        drop(live);
        let res = present_res
            .map(drop)
            .map_err(|e| map_present_err(&device, e));
        let _ = device.poll(false);
        res
    }

    pub fn release(&self) {
        let taken = {
            let mut st = lock(&self.state);
            st.released = true;
            let swap = st.swap.take();
            if let Some(s) = &swap {
                s.invalidate();
            }
            swap
        };
        if let Some(swap) = taken {
            swap.device.prune_retired_staged();
            let _ = swap.device.poll(true);
            swap.retire();
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        self.release();
    }
}
