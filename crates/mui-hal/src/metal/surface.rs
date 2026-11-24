//! Metal window surface backed by `CAMetalLayer` attached to an AppKit `NSView`.
//!
//! Follows Apple QuartzCore primary documentation:
//! - `allowsNextDrawableTimeout = true` (default 1s timeout returning `Timeout`
//!   cleanly instead of deadlocking a DAW GUI thread).
//! - `maximumDrawableCount` clamped to `2..=3`.
//! - `displaySyncEnabled` tracks `PresentMode`.
//! - When `presentsWithTransaction` is enabled, `present` commits, waits via
//!   `waitUntilScheduled()`, and calls `MTLDrawable::present()`; otherwise it
//!   calls `MTLCommandBuffer::presentDrawable(&drawable)` before `commit()`.

use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use objc2::rc::autoreleasepool;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::NSView;
use objc2_core_foundation::CGSize;
use objc2_metal::{MTLCommandBuffer, MTLCommandQueue, MTLDrawable, MTLTexture as _};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer, CATransaction};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use super::{Device, Error, SendOnlyObj, Texture, TextureView, lock, mtl_format};
use crate::binding::{
    metal_display_sync_enabled, metal_drawable_count, metal_layer_opaque,
    metal_surface_format_supported,
};
use crate::types::{
    CompositeAlphaMode, SurfaceConfiguration, TextureFormat, TextureUsages, TextureViewDimension,
};

struct LayerState {
    layer: SendOnlyObj<CAMetalLayer>,
    device: Option<Device>,
    format: TextureFormat,
    alpha_mode: CompositeAlphaMode,
    width: u32,
    height: u32,
    usage: TextureUsages,
    live: Arc<AtomicBool>,
    released: bool,
}

pub struct Surface {
    state: Mutex<LayerState>,
}

pub struct SurfaceTexture {
    pub texture: Texture,
    pub view: TextureView,
    drawable: SendOnlyObj<ProtocolObject<dyn CAMetalDrawable>>,
    device: Device,
    live: Arc<AtomicBool>,
}

pub enum Acquired {
    Image(SurfaceTexture),
    Outdated,
    Timeout,
    Lost,
    Failed(Error),
}

impl Surface {
    /// Creates a dedicated `CAMetalLayer` sublayer on `window`'s AppKit `NSView`.
    ///
    /// # Safety
    /// Caller must supply a live `RawWindowHandle::AppKit` on the AppKit main
    /// thread and keep the `NSView` alive until `Surface::release` / `Drop`.
    pub unsafe fn new(_display: RawDisplayHandle, window: RawWindowHandle) -> Result<Self, Error> {
        let RawWindowHandle::AppKit(handle) = window else {
            return Err(Error::validation("expected AppKit handle on macOS"));
        };
        if objc2::MainThreadMarker::new().is_none() {
            return Err(Error::validation(
                "CAMetalLayer surface must be created on the AppKit main thread",
            ));
        }
        let view_ptr: NonNull<NSView> = handle.ns_view.cast();
        // SAFETY: caller guarantees live `NSView`; main thread verified above.
        let view: &NSView = unsafe { view_ptr.as_ref() };
        view.setWantsLayer(true);
        let root = view
            .layer()
            .ok_or_else(|| Error::internal("NSView has no backing CALayer"))?;

        let layer = CAMetalLayer::new();
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        layer.setFrame(root.bounds());
        layer.setContentsScale(root.contentsScale());
        root.addSublayer(&layer);
        CATransaction::commit();

        Ok(Self {
            state: Mutex::new(LayerState {
                layer: SendOnlyObj(layer),
                device: None,
                format: TextureFormat::Bgra8Unorm,
                alpha_mode: CompositeAlphaMode::Opaque,
                width: 0,
                height: 0,
                usage: TextureUsages::RENDER_ATTACHMENT,
                live: Arc::new(AtomicBool::new(true)),
                released: false,
            }),
        })
    }

    pub fn configure(&self, device: &Device, config: &SurfaceConfiguration) -> Result<(), Error> {
        if config.width == 0 || config.height == 0 {
            return Err(Error::validation("surface dimensions must be non-zero"));
        }
        if !metal_surface_format_supported(config.format) {
            return Err(Error::validation(format!(
                "CAMetalLayer does not support pixel format {:?} (supported: Bgra8Unorm, Bgra8UnormSrgb, Rgba16Float)",
                config.format
            )));
        }
        let mut st = lock(&self.state);
        if st.released {
            return Err(Error::validation("surface has been released"));
        }
        // Retire any outstanding drawable from a previous configuration.
        st.live.store(false, Ordering::Release);
        st.live = Arc::new(AtomicBool::new(true));
        let prev_device = st.device.clone();

        let layer = &st.layer.0;
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        if let Some(root) = layer.superlayer() {
            layer.setFrame(root.bounds());
            layer.setContentsScale(root.contentsScale());
        }
        layer.setDevice(Some(&device.0.raw.0));
        layer.setPixelFormat(mtl_format(config.format));
        layer.setFramebufferOnly(config.usage == TextureUsages::RENDER_ATTACHMENT);
        layer.setDrawableSize(CGSize {
            width: f64::from(config.width),
            height: f64::from(config.height),
        });
        layer.setMaximumDrawableCount(metal_drawable_count(config.desired_maximum_frame_latency));
        layer.setDisplaySyncEnabled(metal_display_sync_enabled(config.present_mode));
        layer.setAllowsNextDrawableTimeout(true);
        layer.setOpaque(metal_layer_opaque(config.alpha_mode));
        CATransaction::commit();

        st.device = Some(device.clone());
        st.format = config.format;
        st.alpha_mode = config.alpha_mode;
        st.width = config.width;
        st.height = config.height;
        st.usage = config.usage;
        drop(st);

        if let Some(prev) = prev_device {
            prev.prune_retired_staged();
        }
        device.prune_retired_staged();
        Ok(())
    }

    pub fn format(&self) -> Option<TextureFormat> {
        let st = lock(&self.state);
        st.device.as_ref().map(|_| st.format)
    }

    pub fn alpha_mode(&self) -> Option<CompositeAlphaMode> {
        let st = lock(&self.state);
        st.device.as_ref().map(|_| st.alpha_mode)
    }

    pub fn acquire(&self) -> Acquired {
        let st = lock(&self.state);
        if st.released {
            return Acquired::Lost;
        }
        let Some(device) = st.device.clone() else {
            return Acquired::Outdated;
        };
        if st.width == 0 || st.height == 0 {
            return Acquired::Outdated;
        }
        let format = st.format;
        let usage = st.usage;
        let live = st.live.clone();
        // Clone the retained `CAMetalLayer` and drop `self.state` before calling
        // `nextDrawable()` (which may block up to 1s) so `configure()`/`release()`
        // never stall behind it.
        let layer = st.layer.clone();
        drop(st);

        let drawable = autoreleasepool(|_| layer.nextDrawable());

        // Reacquire `self.state` after `nextDrawable()` to verify the surface
        // was neither released nor reconfigured while waiting for a drawable.
        {
            let st = lock(&self.state);
            if st.released {
                return Acquired::Lost;
            }
            if !Arc::ptr_eq(&st.live, &live) || !live.load(Ordering::Acquire) {
                return Acquired::Outdated;
            }
        }

        let Some(drawable) = drawable else {
            return Acquired::Timeout;
        };
        let raw_tex = drawable.texture();
        let width = raw_tex.width() as u32;
        let height = raw_tex.height() as u32;
        let texture =
            Texture::from_drawable(&device, raw_tex, width, height, format, usage, live.clone());
        let view = match texture.create_view(Some(TextureViewDimension::D2)) {
            Ok(v) => v,
            Err(e) => return Acquired::Failed(e),
        };
        Acquired::Image(SurfaceTexture {
            texture,
            view,
            drawable: SendOnlyObj(drawable),
            device,
            live,
        })
    }

    pub fn present(&self, frame: &SurfaceTexture) -> Result<(), Error> {
        if !frame.live.load(Ordering::Acquire) {
            return Err(Error::validation(
                "cannot present a drawable from a reconfigured or released CAMetalLayer",
            ));
        }
        let presents_with_transaction = {
            let st = lock(&self.state);
            if st.released {
                return Err(Error::validation(
                    "cannot present to a released CAMetalLayer",
                ));
            }
            if !Arc::ptr_eq(&st.live, &frame.live) || !frame.live.load(Ordering::Acquire) {
                return Err(Error::validation(
                    "cannot present a drawable from a different surface or replaced CAMetalLayer configuration",
                ));
            }
            st.layer.presentsWithTransaction()
        };
        frame.device.flush_staged_before_present()?;
        let res = autoreleasepool(|_| -> Result<(), Error> {
            let cb = frame
                .device
                .0
                .command_queue
                .commandBuffer()
                .ok_or_else(|| Error::internal("MTLCommandQueue::commandBuffer returned nil"))?;
            let drawable: &ProtocolObject<dyn MTLDrawable> =
                ProtocolObject::from_ref(&*frame.drawable.0);
            if presents_with_transaction {
                cb.commit();
                cb.waitUntilScheduled();
                drawable.present();
            } else {
                cb.presentDrawable(drawable);
                cb.commit();
            }
            Ok(())
        });
        // Non-blocking reap of completed earlier submissions so drawables do not
        // accumulate in `inflight` when callers omit explicit `device.poll()`.
        let _ = frame.device.poll(false);
        res
    }

    pub fn release(&self) {
        let prev_device = {
            let mut st = lock(&self.state);
            if st.released {
                return;
            }
            st.released = true;
            st.live.store(false, Ordering::Release);
            let dev = st.device.take();
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            st.layer.removeFromSuperlayer();
            CATransaction::commit();
            dev
        };
        if let Some(dev) = prev_device {
            dev.prune_retired_staged();
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        self.release();
    }
}
