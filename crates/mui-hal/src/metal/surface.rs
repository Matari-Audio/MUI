//! Metal window surface backed by `CAMetalLayer` attached to an AppKit `NSView`.
//!
//! Main-thread safety contract:
//! - `Surface::new` verifies `objc2::MainThreadMarker::new().is_some()` because
//!   `NSView` / `CALayer` sublayer attachment is strictly main-thread-only.
//! - `Surface` remains `Send` so `mui-baseview` can transfer a surface created
//!   on the AppKit main thread to its background render worker.
//! - Every AppKit/CoreAnimation layer mutation in `configure()` and `release()`
//!   / `Drop` runs on the AppKit main thread: inline when already on the main
//!   thread, or via GCD main queue (`dispatch_sync_f` for `configure`,
//!   non-blocking `dispatch_async_f` for `release`/`Drop`) without holding
//!   `Surface::state`.
//! - `CAMetalLayer::nextDrawable()` runs outside `Surface::state` with
//!   `allowsNextDrawableTimeout = true` (1s timeout returning `Timeout`), but
//!   under `Surface::layer_io`, which `configure()` also holds while mutating
//!   layer properties: Apple does not document `nextDrawable()` as safe against
//!   concurrent `device`/`pixelFormat`/`drawableSize` changes, so they never
//!   overlap; `release()`'s main-thread detach takes it too. Lock order:
//!   `Surface::layer_io` -> `Surface::state` (configure takes `layer_io` first so
//!   it never holds `state` while waiting); `acquire` and the detach hold
//!   `layer_io` alone. A configure racing an in-flight `nextDrawable()`
//!   therefore waits at most the 1s drawable timeout.
//! - Presentation uses `MTLCommandBuffer::presentDrawable(&drawable)` followed
//!   by `commit()`, or `commit()` + `waitUntilScheduled()` + `drawable.present()`
//!   when `presentsWithTransaction` is enabled on the `CAMetalLayer`.

use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use objc2::MainThreadMarker;
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

#[repr(C)]
struct DispatchQueueOpaque {
    _private: [u8; 0],
}

type DispatchFunction = extern "C" fn(*mut c_void);

unsafe extern "C" {
    static _dispatch_main_q: DispatchQueueOpaque;
    fn dispatch_sync_f(
        queue: *const DispatchQueueOpaque,
        context: *mut c_void,
        work: DispatchFunction,
    );
    fn dispatch_async_f(
        queue: *const DispatchQueueOpaque,
        context: *mut c_void,
        work: DispatchFunction,
    );
}

/// Runs `f` synchronously on the AppKit main thread.
///
/// Executes inline when already on the main thread; otherwise dispatches onto
/// GCD's `_dispatch_main_q` via `dispatch_sync_f`, catching and rethrowing any
/// panic on the calling thread so unwinding never crosses the C ABI boundary.
fn run_on_main_sync<F, R>(f: F) -> R
where
    F: FnOnce(MainThreadMarker) -> R + Send,
    R: Send,
{
    struct SyncState<F, R> {
        func: Option<F>,
        result: Option<std::thread::Result<R>>,
    }

    extern "C" fn trampoline<F, R>(ctx: *mut c_void)
    where
        F: FnOnce(MainThreadMarker) -> R + Send,
        R: Send,
    {
        // SAFETY: `ctx` points to a live `SyncState<F, R>` on the blocked caller's stack.
        let state = unsafe { &mut *ctx.cast::<SyncState<F, R>>() };
        let res = catch_unwind(AssertUnwindSafe(|| {
            let mtm = MainThreadMarker::new().expect("dispatch_sync_f runs on main thread");
            let func = state.func.take().expect("trampoline invoked once");
            autoreleasepool(|_| func(mtm))
        }));
        state.result = Some(res);
    }

    if let Some(mtm) = MainThreadMarker::new() {
        return f(mtm);
    }

    let mut state = SyncState {
        func: Some(f),
        result: None,
    };
    // SAFETY: `_dispatch_main_q` is a valid static GCD queue; `dispatch_sync_f`
    // does not return until `trampoline` completes, so `&mut state` stays live.
    unsafe {
        dispatch_sync_f(
            std::ptr::addr_of!(_dispatch_main_q),
            (&mut state as *mut SyncState<F, R>).cast::<c_void>(),
            trampoline::<F, R>,
        );
    }
    match state.result.expect("dispatch_sync_f populated result") {
        Ok(v) => v,
        Err(payload) => resume_unwind(payload),
    }
}

/// Runs `f` on the AppKit main thread without blocking the caller when invoked
/// from a background thread (used by `Surface::release` / `Drop`).
fn run_on_main_async<F>(f: F)
where
    F: FnOnce(MainThreadMarker) + Send + 'static,
{
    extern "C" fn trampoline<F>(ctx: *mut c_void)
    where
        F: FnOnce(MainThreadMarker) + Send + 'static,
    {
        // SAFETY: `ctx` was produced by `Box::into_raw(Box::new(f))` below.
        let func = unsafe { Box::from_raw(ctx.cast::<F>()) };
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let mtm = MainThreadMarker::new().expect("dispatch_async_f runs on main thread");
            autoreleasepool(|_| func(mtm));
        }));
    }

    if let Some(mtm) = MainThreadMarker::new() {
        f(mtm);
        return;
    }

    let raw = Box::into_raw(Box::new(f)).cast::<c_void>();
    // SAFETY: `_dispatch_main_q` is a valid static GCD queue and `trampoline`
    // reclaims `raw` exactly once.
    unsafe {
        dispatch_async_f(std::ptr::addr_of!(_dispatch_main_q), raw, trampoline::<F>);
    }
}

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
    /// Serializes `nextDrawable()` with `configure()`'s layer mutations and
    /// `release()`'s detach (an `Arc` so the async detach can outlive `self`).
    layer_io: Arc<Mutex<()>>,
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
        if MainThreadMarker::new().is_none() {
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
            layer_io: Arc::new(Mutex::new(())),
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

        // Retire any outstanding drawable and hide the previous device while
        // reconfiguring, without holding `self.state` across the main-queue sync.
        let (prev_device, config_live) = {
            let mut st = lock(&self.state);
            if st.released {
                return Err(Error::validation("surface has been released"));
            }
            st.live.store(false, Ordering::Release);
            let prev_device = st.device.take();
            let next_live = Arc::new(AtomicBool::new(true));
            st.live = next_live.clone();
            (prev_device, next_live)
        };

        let raw_device = device.0.raw.clone();
        let pixel_format = mtl_format(config.format);
        let framebuffer_only = config.usage == TextureUsages::RENDER_ATTACHMENT;
        let drawable_size = CGSize {
            width: f64::from(config.width),
            height: f64::from(config.height),
        };
        let max_drawables = metal_drawable_count(config.desired_maximum_frame_latency);
        let display_sync = metal_display_sync_enabled(config.present_mode);
        let opaque = metal_layer_opaque(config.alpha_mode);

        // AppKit / CoreAnimation layer mutations run strictly on the main thread.
        // Lock `self.state` on the main thread so verifying `config_live`,
        // mutating `CAMetalLayer`, and updating stored metadata are atomic with
        // respect to concurrent `configure`/`release` calls, without ever
        // holding `self.state` across `dispatch_sync_f`.
        let apply_res: Result<(), Error> = run_on_main_sync(|_mtm| {
            let _io = lock(&self.layer_io);
            let mut st = lock(&self.state);
            if st.released {
                return Err(Error::validation("surface has been released"));
            }
            if !Arc::ptr_eq(&st.live, &config_live) {
                return Ok(());
            }
            let layer: &CAMetalLayer = &st.layer;
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            if let Some(root) = layer.superlayer() {
                layer.setFrame(root.bounds());
                layer.setContentsScale(root.contentsScale());
            }
            layer.setDevice(Some(&**raw_device));
            layer.setPixelFormat(pixel_format);
            layer.setFramebufferOnly(framebuffer_only);
            layer.setDrawableSize(drawable_size);
            layer.setMaximumDrawableCount(max_drawables);
            layer.setDisplaySyncEnabled(display_sync);
            layer.setAllowsNextDrawableTimeout(true);
            layer.setOpaque(opaque);
            CATransaction::commit();
            st.device = Some(device.clone());
            st.format = config.format;
            st.alpha_mode = config.alpha_mode;
            st.width = config.width;
            st.height = config.height;
            st.usage = config.usage;
            Ok(())
        });

        if let Some(prev) = prev_device {
            prev.prune_retired_staged();
        }
        device.prune_retired_staged();
        apply_res
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
        // `nextDrawable()` (which may block up to 1s) so `release()` and metadata
        // reads never stall behind it; only layer mutation waits (`layer_io`).
        let layer = st.layer.clone();
        drop(st);

        let drawable = {
            let _io = lock(&self.layer_io);
            autoreleasepool(|_| layer.nextDrawable())
        };

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
        let _ = frame.device.poll(false);
        res
    }

    pub fn release(&self) {
        let (prev_device, layer) = {
            let mut st = lock(&self.state);
            if st.released {
                return;
            }
            st.released = true;
            st.live.store(false, Ordering::Release);
            (st.device.take(), st.layer.clone())
        };
        // Detach the `CAMetalLayer` on the AppKit main thread without blocking
        // background destructors.
        let layer_io = self.layer_io.clone();
        run_on_main_async(move |_mtm| {
            let layer = layer;
            let _io = lock(&layer_io);
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            layer.removeFromSuperlayer();
            CATransaction::commit();
        });
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
