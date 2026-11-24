//! D3D11 window surface wired to `mui-present`'s `DirectCompositionPresenter`.
//!
//! Enforces Microsoft Learn's DXGI 1.2 `ResizeBuffers` contract:
//! - Before resizing, all cached back-buffer `Texture` / `TextureView` (`RTV`)
//!   handles are retired and dropped, and `ID3D11DeviceContext::OMSetRenderTargets`
//!   is cleared + flushed so zero COM references linger on back buffer `0`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use mui_present::windows::DirectCompositionPresenter;
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
            "D3D11 DirectComposition surface does not support format {other:?}"
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

struct SwapState {
    presenter: DirectCompositionPresenter,
    device: Device,
    format: TextureFormat,
    alpha_mode: CompositeAlphaMode,
    usage: TextureUsages,
    live: Arc<AtomicBool>,
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

        // Retire any active back-buffer handles and snapshot the previous device
        // so we can drain inflight GPU submissions OUTSIDE `self.state` before
        // `IDXGISwapChain::ResizeBuffers` or swapchain teardown.
        let prev_device = {
            let st = lock(&self.state);
            if st.released {
                return Err(Error::validation("surface has been released"));
            }
            st.swap.as_ref().map(|swap| {
                swap.live.store(false, Ordering::Release);
                swap.device.clone()
            })
        };

        if let Some(prev) = prev_device
            && !Arc::ptr_eq(&prev.0, &device.0)
        {
            prev.prune_retired_staged();
            let _ = prev.poll(true);
            prev.clear_render_targets_and_flush();
        }
        // Drop any staged writes targeting retired swapchain images and wait
        // for inflight submissions on `device` so zero COM refs to back buffer
        // `0` / RTV linger before `ResizeBuffers`.
        device.prune_retired_staged();
        let _ = device.poll(true);
        device.clear_render_targets_and_flush();

        let mut st = lock(&self.state);
        if st.released {
            return Err(Error::validation("surface has been released"));
        }

        if let Some(swap) = &mut st.swap {
            swap.live.store(false, Ordering::Release);
            let same_device = Arc::ptr_eq(&swap.device.0, &device.0);
            if same_device && swap.presenter.translucent() == translucent {
                swap.presenter.set_present_mode(pmode);
                swap.presenter
                    .resize((config.width, config.height), fmt)
                    .map_err(|e| Error::internal(e.to_string()))?;
                swap.format = config.format;
                swap.alpha_mode = config.alpha_mode;
                swap.usage = config.usage;
                swap.live = Arc::new(AtomicBool::new(true));
                return Ok(());
            }
            st.swap = None;
        }

        let buffer_count = d3d11_buffer_count(config.desired_maximum_frame_latency);
        // SAFETY: `st.window` is a live Win32 `HWND` owned by `Surface._target`.
        let presenter = unsafe {
            DirectCompositionPresenter::new(
                st.window,
                &device.0.raw,
                (config.width, config.height),
                fmt,
                buffer_count,
                translucent,
                pmode,
            )
        }
        .map_err(|e| Error::internal(e.to_string()))?;

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
        let back_buf = match swap.presenter.back_buffer() {
            Ok(b) => b,
            Err(PresentError::SurfaceLost) => return Acquired::Lost,
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
                "cannot present a back buffer from a reconfigured or released DirectComposition swapchain",
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
        device.clear_render_targets_and_flush();

        let res = {
            let st = lock(&self.state);
            if st.released {
                return Err(Error::validation(
                    "cannot present to a released DirectComposition surface",
                ));
            }
            let Some(swap) = &st.swap else {
                return Err(Error::validation(
                    "cannot present to an unconfigured or released surface",
                ));
            };
            if !Arc::ptr_eq(&swap.live, &live) || !live.load(Ordering::Acquire) {
                return Err(Error::validation(
                    "cannot present a back buffer from another surface or replaced DirectComposition swapchain",
                ));
            }
            swap.presenter
                .present()
                .map(drop)
                .map_err(|e| Error::internal(e.to_string()))
        };
        drop(live);
        let _ = device.poll(false);
        res
    }

    pub fn release(&self) {
        let taken = {
            let mut st = lock(&self.state);
            st.released = true;
            let swap = st.swap.take();
            if let Some(s) = &swap {
                s.live.store(false, Ordering::Release);
            }
            swap
        };
        if let Some(swap) = taken {
            swap.device.prune_retired_staged();
            let _ = swap.device.poll(true);
            swap.device.clear_render_targets_and_flush();
            drop(swap);
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        self.release();
    }
}
