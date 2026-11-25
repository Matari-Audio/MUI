//! Live native surface smoke test: a real window handle, the native backend's
//! swapchain (`CAMetalLayer` on macOS; DXGI flip-model `HwndFlipSwapChain`,
//! then the translucent `DirectCompositionPresenter` path on Windows), a few
//! cleared frames presented, and a reconfigure that switches presenters.
//!
//! `harness = false`: AppKit views must be created on the main thread, which
//! libtest's worker threads are not. Skips without a native adapter unless
//! `MUI_HAL_REQUIRE_NATIVE=1`, which CI sets so a missing device fails.

#[cfg(any(target_os = "macos", target_os = "windows"))]
use mui_hal::*;

fn require_native() -> bool {
    std::env::var("MUI_HAL_REQUIRE_NATIVE").is_ok_and(|v| v == "1")
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn native_device() -> Option<(Instance, Adapter, Device, Queue)> {
    let instance = Instance::new(InstanceDescriptor {
        native: true,
        ..InstanceDescriptor::from_env()
    });
    let adapter = pollster::block_on(instance.enumerate_adapters(native_backends()))
        .into_iter()
        .find(Adapter::is_native);
    let Some(adapter) = adapter else {
        assert!(
            !require_native(),
            "MUI_HAL_REQUIRE_NATIVE=1 but no native adapter ({:?})",
            instance.backend_errors()
        );
        eprintln!("skipped: no native adapter");
        return None;
    };
    eprintln!("adapter: {:?}", adapter.get_info());
    let (device, queue) =
        pollster::block_on(adapter.request_device(&DeviceDescriptor::default())).unwrap();
    device.on_uncaptured_error(std::sync::Arc::new(|e| {
        panic!("uncaptured GPU error during native_surface test: {e}");
    }));
    Some((instance, adapter, device, queue))
}

/// Configures `surface` with `alpha_mode`, then clears and presents frames
/// until three were shown (occluded/timeout frames are retried).
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn present_frames(
    surface: &Surface<'_>,
    adapter: &Adapter,
    device: &Device,
    queue: &Queue,
    alpha_mode: CompositeAlphaMode,
) {
    let caps = surface.get_capabilities(adapter);
    let format = caps.formats[0];
    let config = SurfaceConfiguration {
        usage: TextureUsages::RENDER_ATTACHMENT,
        format,
        width: 64,
        height: 48,
        present_mode: PresentMode::Fifo,
        desired_maximum_frame_latency: 2,
        alpha_mode,
        view_formats: Vec::new(),
    };
    let scope = device.push_error_scope(ErrorFilter::Validation);
    surface.configure(device, &config);
    if let Some(e) = pollster::block_on(scope.pop()) {
        panic!("configure({alpha_mode:?}) failed: {e}");
    }
    let mut presented = 0;
    for _ in 0..60 {
        let frame = match surface.get_current_texture() {
            CurrentSurfaceTexture::Success(t) | CurrentSurfaceTexture::Suboptimal(t) => t,
            CurrentSurfaceTexture::Outdated => {
                surface.configure(device, &config);
                continue;
            }
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => continue,
            other @ (CurrentSurfaceTexture::Lost | CurrentSurfaceTexture::Validation) => {
                panic!("acquire failed: {:?}", std::mem::discriminant(&other))
            }
        };
        assert_eq!(
            (frame.texture.width(), frame.texture.height()),
            (64, 48),
            "{alpha_mode:?}"
        );
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
        drop(encoder.begin_render_pass(&RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &frame.view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color {
                        r: 0.2,
                        g: 0.5,
                        b: 0.8,
                        a: 1.,
                    }),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        }));
        let present_internal = device.push_error_scope(ErrorFilter::Internal);
        let present_validation = device.push_error_scope(ErrorFilter::Validation);
        queue.submit([encoder.finish()]);
        queue.present(frame);
        if let Some(e) = pollster::block_on(present_validation.pop()) {
            panic!("submit/present({alpha_mode:?}) validation error: {e}");
        }
        if let Some(e) = pollster::block_on(present_internal.pop()) {
            panic!("submit/present({alpha_mode:?}) internal error: {e}");
        }
        presented += 1;
        if presented == 3 {
            break;
        }
    }
    assert_eq!(presented, 3, "{alpha_mode:?}: too few frames presented");
    device.poll(PollType::wait_indefinitely()).unwrap();
    assert_eq!(validation_errors(), 0);
}

#[cfg(target_os = "windows")]
mod window {
    use std::num::NonZeroIsize;

    use raw_window_handle::{
        DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle,
        Win32WindowHandle, WindowHandle,
    };
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP, WS_VISIBLE,
    };
    use windows::core::w;

    /// A 64x48 popup of the predefined `STATIC` class (no class to register).
    pub struct Window(HWND);

    // SAFETY: an `HWND` is a plain handle; the window is only destroyed on drop.
    unsafe impl Send for Window {}
    // SAFETY: as above; nothing reads through the handle from Rust.
    unsafe impl Sync for Window {}

    impl Window {
        pub fn open() -> Self {
            // SAFETY: plain Win32 window creation with a predefined class.
            let hwnd = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("STATIC"),
                    w!("mui-hal native surface"),
                    WS_POPUP | WS_VISIBLE,
                    0,
                    0,
                    64,
                    48,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .expect("CreateWindowExW");
            Self(hwnd)
        }
    }

    impl Drop for Window {
        fn drop(&mut self) {
            // SAFETY: `self.0` was created by this thread and is destroyed once.
            let _ = unsafe { DestroyWindow(self.0) };
        }
    }

    impl HasWindowHandle for Window {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let hwnd = NonZeroIsize::new(self.0.0 as isize).ok_or(HandleError::Unavailable)?;
            let raw = RawWindowHandle::Win32(Win32WindowHandle::new(hwnd));
            // SAFETY: the window lives as long as `self`.
            Ok(unsafe { WindowHandle::borrow_raw(raw) })
        }
    }

    impl HasDisplayHandle for Window {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            Ok(DisplayHandle::windows())
        }
    }
}

#[cfg(target_os = "macos")]
mod window {
    use std::ptr::NonNull;

    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::NSView;
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use raw_window_handle::{
        AppKitWindowHandle, DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle,
        RawWindowHandle, WindowHandle,
    };

    /// A windowless 64x48 `NSView`: enough to host a `CAMetalLayer`.
    pub struct Window(Retained<NSView>);

    // SAFETY: the view is created, used and dropped on the main thread only
    // (`harness = false`, single-threaded `main`); the bounds exist because
    // `Instance::create_surface` requires them.
    unsafe impl Send for Window {}
    // SAFETY: as above.
    unsafe impl Sync for Window {}

    impl Window {
        pub fn open() -> Self {
            let mtm = MainThreadMarker::new().expect("must run on the main thread");
            let frame = NSRect::new(NSPoint::new(0., 0.), NSSize::new(64., 48.));
            Self(NSView::initWithFrame(NSView::alloc(mtm), frame))
        }
    }

    impl HasWindowHandle for Window {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let view = NonNull::from(&*self.0).cast();
            let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(view));
            // SAFETY: the view lives as long as `self`.
            Ok(unsafe { WindowHandle::borrow_raw(raw) })
        }
    }

    impl HasDisplayHandle for Window {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            Ok(DisplayHandle::appkit())
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn main() {
    // `cargo nextest` lists tests first; this harness-less binary has none to
    // list (CI runs it via `cargo test`).
    if std::env::args().any(|a| a == "--list") {
        return;
    }
    let Some((instance, adapter, device, queue)) = native_device() else {
        return;
    };
    let surface = instance
        .create_surface(window::Window::open())
        .expect("native create_surface");
    present_frames(
        &surface,
        &adapter,
        &device,
        &queue,
        CompositeAlphaMode::Opaque,
    );
    // On Windows this swaps the HWND flip swapchain for the DirectComposition
    // presenter on the same window, exercising teardown ordering.
    let caps = surface.get_capabilities(&adapter);
    if caps
        .alpha_modes
        .contains(&CompositeAlphaMode::PreMultiplied)
    {
        present_frames(
            &surface,
            &adapter,
            &device,
            &queue,
            CompositeAlphaMode::PreMultiplied,
        );
        present_frames(
            &surface,
            &adapter,
            &device,
            &queue,
            CompositeAlphaMode::Opaque,
        );
    } else if cfg!(target_os = "windows") {
        panic!("D3D11 surfaces must offer PreMultiplied (DirectComposition)");
    }
    drop(surface);
    println!(
        "native surface smoke: ok ({:?})",
        adapter.get_info().backend
    );
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn main() {
    // Linux's live-window check is `presents_to_an_x11_window` (Vulkan).
    let _ = require_native();
}
