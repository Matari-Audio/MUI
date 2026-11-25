//! Offscreen checks on the first adapter the machine offers. Skipped (with
//! a note) where no device exists, unless `MUI_HAL_REQUIRE_NATIVE=1`.

use super::*;

/// The first adapter, or `None` (test skipped). With `MUI_HAL_REQUIRE_NATIVE=1`
/// (native OS CI) a missing adapter, or one that is not this OS's native
/// backend, fails instead, so native coverage cannot silently vanish.
fn device() -> Option<(Device, Queue)> {
    let require_native = std::env::var("MUI_HAL_REQUIRE_NATIVE").is_ok_and(|v| v == "1");
    let instance = Instance::new(InstanceDescriptor::from_env());
    let adapters = pollster::block_on(instance.enumerate_adapters(Backends::all()));
    let Some(adapter) = adapters.into_iter().next() else {
        assert!(
            !require_native,
            "MUI_HAL_REQUIRE_NATIVE=1 but no adapter ({:?})",
            instance.backend_errors()
        );
        eprintln!("skipped: no adapter ({:?})", instance.backend_errors());
        return None;
    };
    if require_native {
        // The wgpu fallback reports the same graphics API (e.g. Metal), so check the
        // implementation, not the API.
        assert!(
            adapter.is_native(),
            "MUI_HAL_REQUIRE_NATIVE=1 but the adapter is wgpu ({:?})",
            adapter.get_info().backend
        );
    }
    Some(pollster::block_on(adapter.request_device(&DeviceDescriptor::default())).unwrap())
}

fn target(device: &Device, w: u32, h: u32) -> Texture {
    device.create_texture(&TextureDescriptor {
        label: None,
        size: Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::RENDER_ATTACHMENT
            | TextureUsages::COPY_SRC
            | TextureUsages::COPY_DST
            | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

/// `texture`'s RGBA8 texels, row-major.
fn read(device: &Device, queue: &Queue, texture: &Texture) -> Vec<u8> {
    let (w, h) = (texture.width(), texture.height());
    let stride = (4 * w).next_multiple_of(256);
    let buffer = device.create_buffer(&BufferDescriptor {
        label: None,
        size: u64::from(stride * h),
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        TexelCopyBufferInfo {
            buffer: &buffer,
            layout: TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(h),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(MapMode::Read, move |r| send.send(r).unwrap());
    device.poll(PollType::Wait).unwrap();
    receive.try_recv().unwrap().unwrap();
    let mapped = buffer.slice(..).get_mapped_range().unwrap();
    mapped
        .chunks(stride as usize)
        .flat_map(|row| &row[..4 * w as usize])
        .copied()
        .collect()
}

#[test]
fn clears_and_reads_back() {
    let Some((device, queue)) = device() else {
        return;
    };
    let texture = target(&device, 7, 5);
    let view = texture.create_view(&TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    drop(encoder.begin_render_pass(&RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(Color {
                    r: 1.,
                    g: 0.,
                    b: 0.,
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
    queue.submit([encoder.finish()]);
    let pixels = read(&device, &queue, &texture);
    assert_eq!(pixels.len(), 7 * 5 * 4);
    assert!(
        pixels.chunks(4).all(|p| p == [255, 0, 0, 255]),
        "{pixels:?}"
    );
    assert_eq!(validation_errors(), 0);
}

/// Uploads a gradient, presents it through `present`'s pipeline onto a
/// larger target, and expects the texels back with clear past the edge.
#[test]
fn present_shader_copies_an_upload() {
    let Some((device, queue)) = device() else {
        return;
    };
    let (w, h) = (9u32, 6u32);
    let source = target(&device, w, h);
    let texels: Vec<u8> = (0..w * h)
        .flat_map(|i| [(i * 4) as u8, (i * 7) as u8, (255 - i) as u8, 255])
        .collect();
    // Padded rows, to exercise the repack.
    let stride = 4 * w + 12;
    let mut padded = vec![0xAA; (stride * h) as usize];
    for y in 0..h as usize {
        let row = 4 * w as usize;
        padded[y * stride as usize..][..row].copy_from_slice(&texels[y * row..][..row]);
    }
    queue.write_texture(
        source.as_image_copy(),
        &padded,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(stride),
            rows_per_image: Some(h),
        },
        source.size(),
    );
    let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: None,
        entries: &[BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: false },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }],
    });
    let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(ShaderModuleDescriptor {
        label: None,
        source: ShaderSource::Mui(ShaderId::Present),
    });
    let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        vertex: VertexState {
            module: &module,
            entry_point: Some("vs"),
            compilation_options: PipelineCompilationOptions,
            buffers: &[],
        },
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: MultisampleState,
        fragment: Some(FragmentState {
            module: &module,
            entry_point: Some("fs_present"),
            compilation_options: PipelineCompilationOptions,
            targets: &[Some(ColorTargetState {
                format: TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let source_view = source.create_view(&TextureViewDescriptor::default());
    let group = device.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[BindGroupEntry {
            binding: 0,
            resource: BindingResource::TextureView(&source_view),
        }],
    });
    let out = target(&device, w + 3, h + 2);
    let out_view = out.create_view(&TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &out_view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color::BLACK),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &group, &[]);
        pass.set_pipeline(&pipeline);
        pass.draw(0..3, 0..1);
    }
    queue.submit([encoder.finish()]);
    // The source and its group may go: the submission holds them.
    drop((group, source_view, source));
    let pixels = read(&device, &queue, &out);
    for y in 0..h + 2 {
        for x in 0..w + 3 {
            let at = ((y * (w + 3) + x) * 4) as usize;
            let want = if x < w && y < h {
                let i = ((y * w + x) * 4) as usize;
                texels[i..i + 4].to_vec()
            } else {
                vec![0; 4]
            };
            assert_eq!(pixels[at..at + 4], want[..], "texel ({x}, {y})");
        }
    }
    assert_eq!(validation_errors(), 0);
}

/// Every shader's pipelines build from its SPIR-V and reflected layout.
#[test]
fn every_shader_builds() {
    let Some((device, _queue)) = device() else {
        return;
    };
    for id in ShaderId::ALL {
        let artifact = id.artifact();
        let mut groups: Vec<Vec<BindGroupLayoutEntry>> = Vec::new();
        for b in artifact.bindings {
            let g = b.group as usize;
            if groups.len() <= g {
                groups.resize(g + 1, Vec::new());
            }
            groups[g].push(BindGroupLayoutEntry {
                binding: b.binding,
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: match b.kind {
                    mui_shaders::BindingKind::UniformBuffer => BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    mui_shaders::BindingKind::Texture2d => BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    mui_shaders::BindingKind::Texture2dArray => BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    mui_shaders::BindingKind::Sampler => {
                        BindingType::Sampler(SamplerBindingType::Filtering)
                    }
                },
                count: None,
            });
        }
        let layouts: Vec<BindGroupLayout> = groups
            .iter()
            .map(|entries| {
                device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                    label: None,
                    entries,
                })
            })
            .collect();
        let refs: Vec<Option<&BindGroupLayout>> = layouts.iter().map(Some).collect();
        let layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &refs,
            immediate_size: 0,
        });
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: None,
            source: ShaderSource::Mui(id),
        });
        let attributes = vertex_attr_array![
            0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4,
            4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4,
            8 => Float32x4, 9 => Float32x4, 10 => Float32x4, 11 => Float32x4,
            12 => Float32x4, 13 => Float32x4, 14 => Float32x4,
        ];
        let instanced = [Some(VertexBufferLayout {
            array_stride: 15 * 16,
            step_mode: VertexStepMode::Instance,
            attributes: &attributes,
        })];
        let vs = artifact
            .entry_points
            .iter()
            .find(|e| e.stage == mui_shaders::Stage::Vertex)
            .unwrap();
        for fs in artifact
            .entry_points
            .iter()
            .filter(|e| e.stage == mui_shaders::Stage::Fragment)
        {
            let _ = device.create_render_pipeline(&RenderPipelineDescriptor {
                label: None,
                layout: Some(&layout),
                vertex: VertexState {
                    module: &module,
                    entry_point: Some(vs.name),
                    compilation_options: PipelineCompilationOptions,
                    buffers: if id == ShaderId::Quad {
                        &instanced
                    } else {
                        &[]
                    },
                },
                primitive: PrimitiveState {
                    topology: PrimitiveTopology::TriangleStrip,
                },
                depth_stencil: None,
                multisample: MultisampleState,
                fragment: Some(FragmentState {
                    module: &module,
                    entry_point: Some(fs.name),
                    compilation_options: PipelineCompilationOptions,
                    targets: &[Some(ColorTargetState {
                        format: TextureFormat::Bgra8Unorm,
                        blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
        }
    }
    assert_eq!(validation_errors(), 0);
}

/// A mapped Xlib window from libX11, loaded at runtime.
#[expect(unsafe_code, reason = "libX11 through dlopen")]
mod xlib {
    use std::ffi::{c_char, c_int, c_ulong, c_void};
    use std::ptr::NonNull;

    type OpenDisplay = unsafe extern "C" fn(*const c_char) -> *mut c_void;
    type DefaultScreen = unsafe extern "C" fn(*mut c_void) -> c_int;
    type RootWindow = unsafe extern "C" fn(*mut c_void, c_int) -> c_ulong;
    type CreateSimpleWindow = unsafe extern "C" fn(
        *mut c_void,
        c_ulong,
        c_int,
        c_int,
        u32,
        u32,
        u32,
        c_ulong,
        c_ulong,
    ) -> c_ulong;
    type WindowOp = unsafe extern "C" fn(*mut c_void, c_ulong) -> c_int;
    type DisplayOp = unsafe extern "C" fn(*mut c_void) -> c_int;
    type ResizeWindow = unsafe extern "C" fn(*mut c_void, c_ulong, u32, u32) -> c_int;
    type XSyncFn = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;

    /// Resizes a [`Window`] after it moved into a surface.
    #[derive(Clone, Copy)]
    pub struct Resizer {
        display: NonNull<c_void>,
        window: c_ulong,
        resize: ResizeWindow,
        sync: XSyncFn,
    }

    impl Resizer {
        pub fn resize(self, w: u32, h: u32) {
            // SAFETY: the window lives in the surface, which outlives every
            // call in the test; the test uses the display from one thread.
            unsafe {
                (self.resize)(self.display.as_ptr(), self.window, w, h);
                (self.sync)(self.display.as_ptr(), 0);
            }
        }
    }

    pub struct Window {
        display: NonNull<c_void>,
        window: c_ulong,
        destroy: WindowOp,
        close: DisplayOp,
        resizer: Resizer,
    }

    // SAFETY: the test touches the display from one thread at a time.
    unsafe impl Send for Window {}
    // SAFETY: as above.
    unsafe impl Sync for Window {}

    fn sym<T>(lib: *mut c_void, name: &std::ffi::CStr) -> Option<T> {
        // SAFETY: `lib` is a live handle; `T` is the symbol's C signature.
        let p = unsafe { libc::dlsym(lib, name.as_ptr()) };
        // SAFETY: a non-null function pointer of type `T`.
        (!p.is_null()).then(|| unsafe { std::mem::transmute_copy(&p) })
    }

    impl Window {
        /// A `w` x `h` window on `$DISPLAY`, or `None` without one.
        pub fn open(w: u32, h: u32) -> Option<Self> {
            std::env::var_os("DISPLAY")?;
            // SAFETY: loading libX11 runs its initialisers only.
            let lib = unsafe { libc::dlopen(c"libX11.so.6".as_ptr(), libc::RTLD_NOW) };
            if lib.is_null() {
                return None;
            }
            let open: OpenDisplay = sym(lib, c"XOpenDisplay")?;
            let screen: DefaultScreen = sym(lib, c"XDefaultScreen")?;
            let root: RootWindow = sym(lib, c"XRootWindow")?;
            let create: CreateSimpleWindow = sym(lib, c"XCreateSimpleWindow")?;
            let map: WindowOp = sym(lib, c"XMapWindow")?;
            let sync: DisplayOp = sym(lib, c"XFlush")?;
            // SAFETY: plain Xlib calls on the display they opened.
            unsafe {
                let display = NonNull::new(open(std::ptr::null()))?;
                let d = display.as_ptr();
                let window = create(d, root(d, screen(d)), 0, 0, w, h, 0, 0, 0);
                map(d, window);
                sync(d);
                Some(Self {
                    display,
                    window,
                    resizer: Resizer {
                        display,
                        window,
                        resize: sym(lib, c"XResizeWindow")?,
                        sync: sym(lib, c"XSync")?,
                    },
                    destroy: sym(lib, c"XDestroyWindow")?,
                    close: sym(lib, c"XCloseDisplay")?,
                })
            }
        }
    }

    impl Window {
        pub const fn resizer(&self) -> Resizer {
            self.resizer
        }
    }

    impl Drop for Window {
        fn drop(&mut self) {
            // SAFETY: the window and display opened in `open`.
            unsafe {
                (self.destroy)(self.display.as_ptr(), self.window);
                (self.close)(self.display.as_ptr());
            }
        }
    }

    impl raw_window_handle::HasWindowHandle for Window {
        fn window_handle(
            &self,
        ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
            let raw = raw_window_handle::XlibWindowHandle::new(self.window);
            // SAFETY: the window lives as long as `self`.
            Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw.into()) })
        }
    }

    impl raw_window_handle::HasDisplayHandle for Window {
        fn display_handle(
            &self,
        ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
            let raw = raw_window_handle::XlibDisplayHandle::new(Some(self.display), 0);
            // SAFETY: the display lives as long as `self`.
            Ok(unsafe { raw_window_handle::DisplayHandle::borrow_raw(raw.into()) })
        }
    }
}

/// Presents frames to a real X11 window through the swapchain, resizing
/// once, with validation clean. Skipped without a display that presents.
#[test]
fn presents_to_an_x11_window() {
    let Some(window) = xlib::Window::open(64, 48) else {
        eprintln!("skipped: no X display");
        return;
    };
    let instance = Instance::new(InstanceDescriptor::from_env());
    let Some(adapter) = pollster::block_on(instance.enumerate_adapters(Backends::all()))
        .into_iter()
        .next()
    else {
        eprintln!("skipped: no adapter");
        return;
    };
    if std::env::var("MUI_HAL_BACKEND").as_deref() != Ok("wgpu") && !cfg!(feature = "wgpu-only") {
        assert!(
            adapter.as_vulkan().is_some(),
            "expected the native Vulkan backend"
        );
    }
    let (device, queue) =
        pollster::block_on(adapter.request_device(&DeviceDescriptor::default())).unwrap();
    let resizer = window.resizer();
    let surface = instance.create_surface(window).unwrap();
    let mut config = SurfaceConfiguration {
        usage: TextureUsages::RENDER_ATTACHMENT,
        format: TextureFormat::Bgra8Unorm,
        width: 64,
        height: 48,
        present_mode: PresentMode::Fifo,
        desired_maximum_frame_latency: 2,
        alpha_mode: CompositeAlphaMode::Opaque,
        view_formats: Vec::new(),
    };
    let scope = device.push_error_scope(ErrorFilter::Validation);
    surface.configure(&device, &config);
    if let Some(e) = pollster::block_on(scope.pop()) {
        panic!("configure failed: {e}");
    }
    let mut presented = 0;
    let mut resized = 0;
    for frame in 0..30 {
        if frame == 6 {
            config.width = 80;
            config.height = 40;
            resizer.resize(80, 40);
            surface.configure(&device, &config);
        }
        let texture = match surface.get_current_texture() {
            CurrentSurfaceTexture::Success(t) | CurrentSurfaceTexture::Suboptimal(t) => t,
            CurrentSurfaceTexture::Outdated => {
                surface.configure(&device, &config);
                continue;
            }
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => continue,
            other @ (CurrentSurfaceTexture::Lost | CurrentSurfaceTexture::Validation) => {
                panic!("acquire failed: {:?}", std::mem::discriminant(&other))
            }
        };
        // The swapchain picks the first UNORM format the surface lists.
        assert!(matches!(
            texture.texture.format(),
            TextureFormat::Bgra8Unorm | TextureFormat::Rgba8Unorm
        ));
        // Native reports the swapchain's choice; wgpu uses the configured one.
        assert_eq!(
            texture.texture.format(),
            surface.format().unwrap_or(config.format)
        );
        // The window manager applies the resize asynchronously, and the
        // swapchain follows the window's current extent: reconfigure until
        // it catches up, within a few frames.
        let size = (texture.texture.width(), texture.texture.height());
        if size != (config.width, config.height) && frame >= 6 {
            queue.present(texture);
            surface.configure(&device, &config);
            continue;
        }
        assert_eq!(size, (config.width, config.height), "frame {frame}");
        resized += u32::from(frame >= 6);
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
        drop(encoder.begin_render_pass(&RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &texture.view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Color {
                        r: f64::from(frame) / 12.,
                        g: 0.5,
                        b: 0.2,
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
        queue.submit([encoder.finish()]);
        queue.present(texture);
        presented += 1;
    }
    device.poll(PollType::Wait).unwrap();
    assert!(presented >= 10, "only {presented} frames presented");
    assert!(resized > 0, "no frame reached the resized extent");
    assert_eq!(validation_errors(), 0);
}

/// Native error scopes: a dropped scope leaves the stack, scopes are per
/// thread, and popping one leaves the others in place.
#[test]
fn error_scopes_are_per_thread_and_drop_safe() {
    let Some((device, _queue)) = device() else {
        return;
    };
    if device.as_vulkan().is_none() {
        eprintln!("skipped: not the native backend");
        return;
    }
    let error = || Error::Validation {
        description: "test".into(),
    };
    // A dropped scope leaves the stack: the next error goes uncaptured.
    let (uncaptured, seen) = std::sync::mpsc::channel();
    let uncaptured = std::sync::Mutex::new(uncaptured);
    device.on_uncaptured_error(Arc::new(move |e| {
        let _ = lock(&uncaptured).send(e.to_string());
    }));
    drop(device.push_error_scope(ErrorFilter::Validation));
    device.report(Error::Validation {
        description: "after drop".into(),
    });
    assert!(seen.try_recv().unwrap().contains("after drop"));
    let outer = device.push_error_scope(ErrorFilter::Validation);
    // Another thread's scope, pushed later, neither catches this thread's
    // error nor is removed when this thread pops.
    let (pushed, popped) = (std::sync::mpsc::channel(), std::sync::mpsc::channel::<()>());
    let other = {
        let device = device.clone();
        std::thread::spawn(move || {
            let scope = device.push_error_scope(ErrorFilter::Validation);
            pushed.0.send(()).unwrap();
            popped.1.recv().unwrap();
            device.report(error());
            pollster::block_on(scope.pop()).is_some()
        })
    };
    pushed.1.recv().unwrap();
    device.report(error());
    assert!(pollster::block_on(outer.pop()).is_some());
    popped.0.send(()).unwrap();
    assert!(other.join().unwrap(), "the other thread's scope survives");
}

/// Task #25 (PR #135 r4671173788): writes to a mapped buffer (both
/// `Queue::write_buffer` and `CommandEncoder` copy submissions) are rejected
/// with a validation error until `Buffer::unmap` runs, matching wgpu.
#[test]
fn rejects_writes_to_mapped_buffer() {
    let Some((device, queue)) = device() else {
        return;
    };
    let dst = device.create_buffer(&BufferDescriptor {
        label: Some("mapped readback"),
        size: 16,
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let src = device.create_buffer(&BufferDescriptor {
        label: Some("copy source"),
        size: 16,
        usage: BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let initial = [1u8, 2, 3, 4];
    let replacement = [9u8, 8, 7, 6];
    queue.write_buffer(&dst, 0, &initial);
    queue.write_buffer(&src, 0, &replacement);
    queue.submit([]);

    // Map `dst` for reading.
    let (send, recv) = std::sync::mpsc::channel();
    dst.slice(..)
        .map_async(MapMode::Read, move |r| send.send(r).unwrap());

    // On native Vulkan, writes while the map is still pending must also fail.
    if device.as_vulkan().is_some() {
        assert!(
            dst.slice(..).get_mapped_range().is_err(),
            "pending map must not expose a mapped range before poll"
        );
        let scope = device.push_error_scope(ErrorFilter::Validation);
        let mut enc = device.create_command_encoder(&CommandEncoderDescriptor::default());
        enc.copy_buffer_to_buffer(&src, 0, &dst, 0, 4);
        queue.submit([enc.finish()]);
        let err = pollster::block_on(scope.pop());
        assert!(
            matches!(err, Some(Error::Validation { .. })),
            "expected validation error writing a pending-map buffer, got {err:?}"
        );
    }

    device.poll(PollType::Wait).unwrap();
    recv.try_recv().unwrap().unwrap();
    {
        let view = dst.slice(0..4).get_mapped_range().unwrap();
        assert_eq!(&*view, &initial);
    }

    // While mapped: a submitted copy writing `dst` is rejected.
    let scope = device.push_error_scope(ErrorFilter::Validation);
    let mut enc = device.create_command_encoder(&CommandEncoderDescriptor::default());
    enc.copy_buffer_to_buffer(&src, 0, &dst, 0, 4);
    queue.submit([enc.finish()]);
    let err = pollster::block_on(scope.pop());
    assert!(
        matches!(err, Some(Error::Validation { .. })),
        "expected validation error submitting a write to a mapped buffer, got {err:?}"
    );

    // While mapped: `queue.write_buffer` targeting `dst` is rejected at write
    // time (before `submit([])` runs) and must not poison later submissions.
    let scope = device.push_error_scope(ErrorFilter::Validation);
    queue.write_buffer(&dst, 0, &[5, 5, 5, 5]);
    let err = pollster::block_on(scope.pop());
    assert!(
        matches!(err, Some(Error::Validation { .. })),
        "expected validation error in write_buffer on a mapped buffer, got {err:?}"
    );
    let scope = device.push_error_scope(ErrorFilter::Validation);
    queue.submit([]);
    assert!(
        pollster::block_on(scope.pop()).is_none(),
        "rejected write_buffer must not poison subsequent submissions"
    );

    // The mapped bytes remain untouched.
    {
        let view = dst.slice(0..4).get_mapped_range().unwrap();
        assert_eq!(&*view, &initial);
    }

    // After unmapping: `get_mapped_range` fails and writes succeed again.
    dst.unmap();
    if device.as_vulkan().is_some() {
        assert!(
            dst.slice(0..4).get_mapped_range().is_err(),
            "unmapped buffer must not return a mapped range"
        );
    }
    let scope = device.push_error_scope(ErrorFilter::Validation);
    let mut enc = device.create_command_encoder(&CommandEncoderDescriptor::default());
    enc.copy_buffer_to_buffer(&src, 0, &dst, 0, 4);
    queue.submit([enc.finish()]);
    assert!(
        pollster::block_on(scope.pop()).is_none(),
        "writing after unmap must succeed"
    );

    let (send, recv) = std::sync::mpsc::channel();
    dst.slice(..)
        .map_async(MapMode::Read, move |r| send.send(r).unwrap());
    device.poll(PollType::Wait).unwrap();
    recv.try_recv().unwrap().unwrap();
    {
        let view = dst.slice(0..4).get_mapped_range().unwrap();
        assert_eq!(&*view, &replacement);
    }
    dst.unmap();
    assert_eq!(validation_errors(), 0);
}

/// Task #26 (PR #135 r4671173807, r4671173953): headless regression proving
/// passes and `create_view` targeting a retired swapchain image are rejected
/// before reaching Vulkan, leaving the image untouched and validation clean.
#[test]
fn rejects_pass_targeting_retired_swapchain_image_headless() {
    let Some((device, queue)) = device() else {
        return;
    };
    let Some(vk_dev) = device.as_vulkan() else {
        eprintln!("skipped: not the native backend");
        return;
    };
    let live = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let vk_tex = vk::Texture::new_with_liveness(
        vk_dev,
        &TextureDescriptor {
            label: Some("simulated swapchain image"),
            size: Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        },
        live.clone(),
    )
    .unwrap();
    let texture = Texture(TextureInner::Vulkan(vk_tex));
    let view = texture.create_view(&TextureViewDescriptor::default());

    // Initial clear to green while the swapchain generation is live.
    let mut enc = device.create_command_encoder(&CommandEncoderDescriptor::default());
    drop(enc.begin_render_pass(&RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(Color {
                    r: 0.,
                    g: 1.,
                    b: 0.,
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
    queue.submit([enc.finish()]);
    device.poll(PollType::Wait).unwrap();

    // Record a pass clearing to red, then retire the swapchain before submit.
    let mut stale = device.create_command_encoder(&CommandEncoderDescriptor::default());
    drop(stale.begin_render_pass(&RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(Color {
                    r: 1.,
                    g: 0.,
                    b: 0.,
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
    live.store(false, std::sync::atomic::Ordering::Release);

    let scope = device.push_error_scope(ErrorFilter::Validation);
    queue.submit([stale.finish()]);
    let err = pollster::block_on(scope.pop());
    assert!(
        matches!(err, Some(Error::Validation { ref description }) if description.contains("retired swapchain")),
        "expected retired-swapchain validation error, got {err:?}"
    );

    // Creating a new view of a retired swapchain image is also rejected cleanly.
    let scope = device.push_error_scope(ErrorFilter::Validation);
    let _invalid_view = texture.create_view(&TextureViewDescriptor::default());
    let err = pollster::block_on(scope.pop());
    assert!(
        matches!(err, Some(Error::Validation { ref description }) if description.contains("retired swapchain")),
        "expected retired-swapchain view validation error, got {err:?}"
    );

    // Re-enable solely to read back our owned test texture and confirm the
    // rejected red clear never ran on the GPU.
    live.store(true, std::sync::atomic::Ordering::Release);
    let pixels = read(&device, &queue, &texture);
    assert!(
        pixels.chunks(4).all(|p| p == [0, 255, 0, 255]),
        "rejected pass must not have rendered: {pixels:?}"
    );
    assert_eq!(validation_errors(), 0);
}

/// Tasks #26 & #27a on a real X11 swapchain: passes and presents for frames
/// held across `Surface::configure` or `Surface::drop` are rejected cleanly
/// without UB or Vulkan validation errors.
#[test]
fn rejects_retired_swapchain_pass_and_stale_present_on_x11() {
    let Some(window) = xlib::Window::open(64, 48) else {
        eprintln!("skipped: no X display");
        return;
    };
    let instance = Instance::new(InstanceDescriptor::from_env());
    let Some(adapter) = pollster::block_on(instance.enumerate_adapters(Backends::all()))
        .into_iter()
        .next()
    else {
        eprintln!("skipped: no adapter");
        return;
    };
    if adapter.as_vulkan().is_none() {
        eprintln!("skipped: not the native backend");
        return;
    }
    let (device, queue) =
        pollster::block_on(adapter.request_device(&DeviceDescriptor::default())).unwrap();
    let surface = instance.create_surface(window).unwrap();
    let config = SurfaceConfiguration {
        usage: TextureUsages::RENDER_ATTACHMENT,
        format: TextureFormat::Bgra8Unorm,
        width: 64,
        height: 48,
        present_mode: PresentMode::Fifo,
        desired_maximum_frame_latency: 2,
        alpha_mode: CompositeAlphaMode::Opaque,
        view_formats: Vec::new(),
    };
    surface.configure(&device, &config);

    // Case 1: Frame and recorded pass held across `Surface::configure`.
    let frame1 = match surface.get_current_texture() {
        CurrentSurfaceTexture::Success(t) | CurrentSurfaceTexture::Suboptimal(t) => t,
        other => panic!("acquire 1 failed: {:?}", std::mem::discriminant(&other)),
    };
    let scope = device.push_error_scope(ErrorFilter::Validation);
    queue.write_texture(
        frame1.texture.as_image_copy(),
        &[0; 4],
        TexelCopyBufferLayout::default(),
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    assert!(matches!(
        pollster::block_on(scope.pop()),
        Some(Error::Validation { .. })
    ));
    let mut enc1 = device.create_command_encoder(&CommandEncoderDescriptor::default());
    drop(enc1.begin_render_pass(&RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(RenderPassColorAttachment {
            view: &frame1.view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(Color::BLACK),
                store: StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    }));
    // Reconfigure while `frame1` and `enc1` are still held (abandoned acquire).
    surface.configure(&device, &config);

    let scope = device.push_error_scope(ErrorFilter::Validation);
    queue.submit([enc1.finish()]);
    assert!(
        matches!(
            pollster::block_on(scope.pop()),
            Some(Error::Validation { .. })
        ),
        "pass targeting pre-reconfigure swapchain must be rejected"
    );

    let scope = device.push_error_scope(ErrorFilter::Validation);
    queue.present(frame1);
    assert!(
        matches!(
            pollster::block_on(scope.pop()),
            Some(Error::Validation { .. })
        ),
        "presenting a frame from a superseded swapchain must be rejected"
    );

    let scope = device.push_error_scope(ErrorFilter::Validation);
    queue.submit([]);
    assert!(pollster::block_on(scope.pop()).is_none());

    // Case 2: Frame and recorded pass held across `Surface::drop`.
    let frame2 = match surface.get_current_texture() {
        CurrentSurfaceTexture::Success(t) | CurrentSurfaceTexture::Suboptimal(t) => t,
        other => panic!("acquire 2 failed: {:?}", std::mem::discriminant(&other)),
    };
    let mut enc2 = device.create_command_encoder(&CommandEncoderDescriptor::default());
    drop(enc2.begin_render_pass(&RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(RenderPassColorAttachment {
            view: &frame2.view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(Color::BLACK),
                store: StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    }));
    drop(surface);

    let scope = device.push_error_scope(ErrorFilter::Validation);
    queue.submit([enc2.finish()]);
    assert!(
        matches!(
            pollster::block_on(scope.pop()),
            Some(Error::Validation { .. })
        ),
        "pass targeting a dropped surface's swapchain must be rejected"
    );

    let scope = device.push_error_scope(ErrorFilter::Validation);
    queue.present(frame2);
    assert!(
        matches!(
            pollster::block_on(scope.pop()),
            Some(Error::Validation { .. })
        ),
        "presenting a frame after Surface::drop must be rejected"
    );

    device.poll(PollType::Wait).unwrap();
    assert_eq!(validation_errors(), 0);
}

/// Task #27a (PR #135 r4671173989): `vulkan::Surface::present` consumes its
/// `SurfaceTexture` by value (making double-present a compile error) and
/// rejects frames held across `release()` + `configure()` (replaced swapchain).
#[test]
fn rejects_replaced_swapchain_frame_on_vulkan_surface() {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

    let Some(window) = xlib::Window::open(64, 48) else {
        eprintln!("skipped: no X display");
        return;
    };
    let instance = Instance::new(InstanceDescriptor::from_env());
    let Some(vk_instance) = instance.vulkan() else {
        eprintln!("skipped: not the native backend");
        return;
    };
    let Some(adapter) = vk_instance.enumerate_adapters().into_iter().next() else {
        eprintln!("skipped: no Vulkan adapter");
        return;
    };
    let vk_dev = adapter
        .request_device(&DeviceDescriptor::default())
        .unwrap();
    let display = window.display_handle().unwrap().as_raw();
    let win = window.window_handle().unwrap().as_raw();
    // SAFETY: `window` outlives `surface` in this test.
    #[expect(unsafe_code, reason = "raw handles kept alive by `window`")]
    let surface = unsafe { vk::Surface::new(vk_instance, display, win) }.unwrap();

    surface.configure(&vk_dev, (64, 48), false).unwrap();
    let vk::Acquired::Image(old_frame) = surface.acquire() else {
        panic!("expected acquired image");
    };
    // Release and reconfigure completely replaces the `Swap` object.
    surface.release();
    surface.configure(&vk_dev, (64, 48), false).unwrap();
    // Acquire a new active frame, then attempt to present the old one:
    // global generation + acquisition token must reject `old_frame`.
    let vk::Acquired::Image(active_frame) = surface.acquire() else {
        panic!("expected active image");
    };
    assert!(
        surface.present(old_frame).is_err(),
        "frame from replaced swapchain must be rejected"
    );
    // The active frame still presents cleanly (and `present` consumes it).
    surface.present(active_frame).unwrap();

    // Reconfiguring after a present must retain the presented render-complete
    // semaphore on the device instead of destroying it prematurely.
    let before = vk_dev.retained_semaphore_count();
    surface.configure(&vk_dev, (64, 48), false).unwrap();
    assert!(
        vk_dev.retained_semaphore_count() > before,
        "presented render-complete semaphore must be retained across reconfigure"
    );

    // Abandoned acquire followed by a second acquire reports Outdated and
    // quiesces cleanly on reconfigure.
    let vk::Acquired::Image(_abandoned) = surface.acquire() else {
        panic!("expected image for abandoned-acquire check");
    };
    assert!(matches!(surface.acquire(), vk::Acquired::Outdated));
    surface.configure(&vk_dev, (64, 48), false).unwrap();
    surface.release();
    drop(surface);
    drop(vk_dev);
    drop(window);
    assert_eq!(validation_errors(), 0);
}

/// Task #27b (PR #135 r4671173907): retained semaphores stay alive on the
/// device and are destroyed cleanly on device teardown with zero validation
/// errors.
#[test]
fn retained_semaphores_are_freed_cleanly_on_device_drop() {
    let Some((device, queue)) = device() else {
        return;
    };
    let Some(vk_dev) = device.as_vulkan() else {
        eprintln!("skipped: not the native backend");
        return;
    };
    #[expect(unsafe_code, reason = "create a raw semaphore to exercise retention")]
    // SAFETY: a plain semaphore creation on a live device; ownership transfers
    // to `vk_dev`'s retained-semaphore list and is freed in `DeviceShared::drop`.
    let sem = unsafe {
        vk_dev
            .raw()
            .create_semaphore(&ash::vk::SemaphoreCreateInfo::default(), None)
    }
    .unwrap();
    assert_eq!(vk_dev.retained_semaphore_count(), 0);
    vk_dev.retain_semaphore_for_test(sem);
    assert_eq!(vk_dev.retained_semaphore_count(), 1);
    drop((device, queue));
    assert_eq!(validation_errors(), 0);
}

/// PR #139 review #3 (`4672677021`): staging `write_buffer(A)` and then
/// calling `map_async(A)` before submitting flushes the staged write so `A`
/// reads the written bytes and unrelated submissions targeting `B` are never
/// blocked while `A` stays mapped.
#[test]
fn staged_write_before_map_async_does_not_block_unrelated_submissions() {
    let Some((device, queue)) = device() else {
        return;
    };
    let buf_a = device.create_buffer(&BufferDescriptor {
        label: Some("staged A"),
        size: 16,
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let buf_b = device.create_buffer(&BufferDescriptor {
        label: Some("unrelated B"),
        size: 16,
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let src_b = device.create_buffer(&BufferDescriptor {
        label: Some("src B"),
        size: 16,
        usage: BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    // Stage a write to A WITHOUT submitting, then immediately map A.
    let bytes_a = [11u8, 22, 33, 44];
    queue.write_buffer(&buf_a, 0, &bytes_a);
    let (tx_a, rx_a) = std::sync::mpsc::channel();
    buf_a
        .slice(..)
        .map_async(MapMode::Read, move |r| tx_a.send(r).unwrap());
    device.poll(PollType::Wait).unwrap();
    rx_a.try_recv().unwrap().unwrap();
    assert_eq!(&*buf_a.slice(0..4).get_mapped_range().unwrap(), &bytes_a);

    // While A remains mapped, unrelated work targeting B must succeed cleanly.
    let bytes_b = [77u8, 88, 99, 100];
    let scope = device.push_error_scope(ErrorFilter::Validation);
    queue.write_buffer(&src_b, 0, &bytes_b);
    let mut enc = device.create_command_encoder(&CommandEncoderDescriptor::default());
    enc.copy_buffer_to_buffer(&src_b, 0, &buf_b, 0, 4);
    queue.submit([enc.finish()]);
    assert!(
        pollster::block_on(scope.pop()).is_none(),
        "unrelated submission targeting B must not fail while A is mapped"
    );

    let (tx_b, rx_b) = std::sync::mpsc::channel();
    buf_b
        .slice(..)
        .map_async(MapMode::Read, move |r| tx_b.send(r).unwrap());
    device.poll(PollType::Wait).unwrap();
    rx_b.try_recv().unwrap().unwrap();
    assert_eq!(&*buf_b.slice(0..4).get_mapped_range().unwrap(), &bytes_b);

    buf_a.unmap();
    buf_b.unmap();
    assert_eq!(validation_errors(), 0);
}

#[test]
fn native_map_bounds_and_rejected_callbacks() {
    let Some((device, _queue)) = device() else {
        return;
    };
    if device.as_vulkan().is_none() {
        return;
    }
    let buffer = device.create_buffer(&BufferDescriptor {
        label: None,
        size: 16,
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let (send, receive) = std::sync::mpsc::channel();
    let ready = send.clone();
    buffer
        .slice(4..12)
        .map_async(MapMode::Read, move |r| ready.send(r).unwrap());
    let scope = device.push_error_scope(ErrorFilter::Validation);
    let duplicate = send.clone();
    buffer
        .slice(..)
        .map_async(MapMode::Read, move |r| duplicate.send(r).unwrap());
    assert!(pollster::block_on(scope.pop()).is_some());
    let scope = device.push_error_scope(ErrorFilter::Validation);
    buffer
        .slice(12..20)
        .map_async(MapMode::Read, move |r| send.send(r).unwrap());
    assert!(pollster::block_on(scope.pop()).is_some());
    assert!(receive.try_recv().is_err());
    device.poll(PollType::Wait).unwrap();
    assert!(receive.try_recv().unwrap().is_ok());
    assert!(receive.try_recv().unwrap().is_err());
    assert!(receive.try_recv().unwrap().is_err());
    assert_eq!(&*buffer.slice(8..12).get_mapped_range().unwrap(), &[0; 4]);
    assert!(buffer.slice(0..8).get_mapped_range().is_err());
    assert!(buffer.slice(8..16).get_mapped_range().is_err());
    assert_eq!(validation_errors(), 0);
}

#[test]
fn native_poll_rechecks_loss_after_each_map_callback() {
    let Some((device, _queue)) = device() else {
        return;
    };
    if device.as_vulkan().is_none() {
        return;
    }
    let desc = BufferDescriptor {
        label: None,
        size: 16,
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    };
    let first = device.create_buffer(&desc);
    let second = device.create_buffer(&desc);
    let destroy = device.clone();
    first.slice(..).map_async(MapMode::Read, move |r| {
        r.unwrap();
        destroy.destroy();
    });
    let (send, receive) = std::sync::mpsc::channel();
    second
        .slice(..)
        .map_async(MapMode::Read, move |r| send.send(r).unwrap());
    device.poll(PollType::Wait).unwrap();
    assert!(receive.try_recv().unwrap().is_err());
    assert!(second.slice(..).get_mapped_range().is_err());
}

#[test]
fn oversized_texture_placeholder_is_rejected_before_upload_arithmetic() {
    let Some((device, queue)) = device() else {
        return;
    };
    if device.as_vulkan().is_none() {
        return;
    }
    let scope = device.push_error_scope(ErrorFilter::Validation);
    let texture = device.create_texture(&TextureDescriptor {
        label: None,
        size: Extent3d {
            width: u32::MAX,
            height: u32::MAX,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::COPY_DST,
        view_formats: &[],
    });
    assert!(pollster::block_on(scope.pop()).is_some());
    let scope = device.push_error_scope(ErrorFilter::Validation);
    queue.write_texture(
        texture.as_image_copy(),
        &[],
        TexelCopyBufferLayout::default(),
        texture.size(),
    );
    assert!(pollster::block_on(scope.pop()).is_some());
    assert_eq!(validation_errors(), 0);
}

#[test]
fn out_of_range_buffer_copy_reports_validation_at_submit() {
    let Some((device, queue)) = device() else {
        return;
    };
    if device.as_vulkan().is_none() {
        return;
    }
    let src = device.create_buffer(&BufferDescriptor {
        label: None,
        size: 16,
        usage: BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let dst = device.create_buffer(&BufferDescriptor {
        label: None,
        size: 16,
        usage: BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    for (src_offset, dst_offset, size) in [(20, 0, None), (u64::MAX, 0, Some(4)), (0, 16, Some(4))]
    {
        let scope = device.push_error_scope(ErrorFilter::Validation);
        let mut enc = device.create_command_encoder(&CommandEncoderDescriptor::default());
        enc.copy_buffer_to_buffer(&src, src_offset, &dst, dst_offset, size);
        queue.submit([enc.finish()]);
        assert!(matches!(
            pollster::block_on(scope.pop()),
            Some(Error::Validation { .. })
        ));
    }
    let scope = device.push_error_scope(ErrorFilter::Validation);
    let mut enc = device.create_command_encoder(&CommandEncoderDescriptor::default());
    enc.copy_buffer_to_buffer(&src, 16, &dst, 16, None);
    queue.submit([enc.finish()]);
    assert!(pollster::block_on(scope.pop()).is_none());
    device.poll(PollType::Wait).unwrap();
    assert_eq!(validation_errors(), 0);
}

#[test]
fn owner_drop_releases_device_captured_by_error_handler_buffer() {
    let Some((device, queue)) = device() else {
        return;
    };
    let Some(native) = device.as_vulkan() else {
        return;
    };
    let weak = Arc::downgrade(&native.0);
    let buffer = device.create_buffer(&BufferDescriptor {
        label: None,
        size: 16,
        usage: BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    device.on_uncaptured_error(Arc::new(move |_| {
        let _ = buffer.size();
    }));
    drop(device);
    assert!(weak.upgrade().is_some());
    drop(queue);
    assert!(weak.upgrade().is_none());
}

#[test]
fn owner_drop_dispatches_only_unpolled_work_done_callbacks() {
    let Some((device, queue)) = device() else {
        return;
    };
    if device.as_vulkan().is_none() {
        return;
    }
    let (send, receive) = std::sync::mpsc::channel();
    let poll_send = send.clone();
    queue.on_submitted_work_done(move || poll_send.send("poll").unwrap());
    device.poll(PollType::Wait).unwrap();
    assert_eq!(receive.try_recv().unwrap(), "poll");

    let captured = device.create_buffer(&BufferDescriptor {
        label: None,
        size: 16,
        usage: BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.on_submitted_work_done(move || {
        drop(captured);
        send.send("drop").unwrap();
    });
    drop(device);
    assert!(receive.try_recv().is_err());
    drop(queue);
    assert_eq!(receive.try_recv().unwrap(), "drop");
    assert_eq!(
        receive.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    );
    assert_eq!(validation_errors(), 0);
}

/// PR #139 review #10 (`4672677036`): `Queue::on_submitted_work_done` never
/// runs its callback inline during registration, even after `Device::destroy`;
/// it dispatches from `Device::poll`.
#[test]
fn on_submitted_work_done_dispatches_via_poll_after_destroy() {
    let Some((device, queue)) = device() else {
        return;
    };
    if device.as_vulkan().is_none() {
        eprintln!("skipped: not the native backend");
        return;
    }
    device.destroy();
    let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = ran.clone();
    queue.on_submitted_work_done(move || {
        flag.store(true, std::sync::atomic::Ordering::Release);
    });
    assert!(
        !ran.load(std::sync::atomic::Ordering::Acquire),
        "on_submitted_work_done must not invoke callback inline during registration"
    );
    let _ = device.poll(PollType::Poll);
    assert!(
        ran.load(std::sync::atomic::Ordering::Acquire),
        "on_submitted_work_done must dispatch during Device::poll"
    );
}

/// Task #27c (PR #135 r4671173885): native `create_*` failures report to
/// per-thread error scopes instead of panicking, and subsequent operations on
/// the returned invalid handles also report cleanly without Vulkan validation
/// errors.
#[test]
fn creation_failures_report_to_error_scopes_without_panicking() {
    let Some((device, queue)) = device() else {
        return;
    };
    if device.as_vulkan().is_none() {
        eprintln!("skipped: not the native backend");
        return;
    }

    // 1. Multi-level texture creation fails into the Validation error scope.
    let scope = device.push_error_scope(ErrorFilter::Validation);
    let bad_tex = device.create_texture(&TextureDescriptor {
        label: Some("too many mips"),
        size: Extent3d {
            width: 8,
            height: 8,
            depth_or_array_layers: 1,
        },
        mip_level_count: 2,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    assert!(
        matches!(
            pollster::block_on(scope.pop()),
            Some(Error::Validation { .. })
        ),
        "mip_level_count > 1 must report Validation instead of panicking"
    );

    // 2. Creating a view of an invalid texture reports Validation without panicking.
    let scope = device.push_error_scope(ErrorFilter::Validation);
    let bad_view = bad_tex.create_view(&TextureViewDescriptor::default());
    assert!(
        matches!(
            pollster::block_on(scope.pop()),
            Some(Error::Validation { .. })
        ),
        "create_view on invalid texture must report Validation"
    );

    // 3. Submitting a pass targeting the invalid view reports Validation.
    let scope = device.push_error_scope(ErrorFilter::Validation);
    let mut enc = device.create_command_encoder(&CommandEncoderDescriptor::default());
    drop(enc.begin_render_pass(&RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(RenderPassColorAttachment {
            view: &bad_view,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(Color::BLACK),
                store: StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    }));
    queue.submit([enc.finish()]);
    assert!(
        matches!(
            pollster::block_on(scope.pop()),
            Some(Error::Validation { .. })
        ),
        "submitting a pass with an invalid view must report Validation"
    );

    // 4. Buffer exceeding `max_buffer_size` reports Validation; writing to it
    //    also reports Validation without panicking.
    let scope = device.push_error_scope(ErrorFilter::Validation);
    let bad_buf = device.create_buffer(&BufferDescriptor {
        label: Some("oversized"),
        size: device.limits().max_buffer_size + 4,
        usage: BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    assert!(
        matches!(
            pollster::block_on(scope.pop()),
            Some(Error::Validation { .. })
        ),
        "oversized create_buffer must report Validation"
    );
    let scope = device.push_error_scope(ErrorFilter::Validation);
    queue.write_buffer(&bad_buf, 0, &[1, 2, 3, 4]);
    assert!(
        matches!(
            pollster::block_on(scope.pop()),
            Some(Error::Validation { .. })
        ),
        "write_buffer on invalid buffer must report Validation"
    );

    // 5. Bind group referencing an invalid view reports Validation.
    let bgl = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
        label: None,
        entries: &[BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: false },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }],
    });
    let scope = device.push_error_scope(ErrorFilter::Validation);
    let _bad_bg = device.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: &bgl,
        entries: &[BindGroupEntry {
            binding: 0,
            resource: BindingResource::TextureView(&bad_view),
        }],
    });
    assert!(
        matches!(
            pollster::block_on(scope.pop()),
            Some(Error::Validation { .. })
        ),
        "create_bind_group with invalid view must report Validation"
    );

    // 6. Render pipeline with a nonexistent entry point reports Validation.
    let pl = device.create_pipeline_layout(&PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&bgl)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(ShaderModuleDescriptor {
        label: None,
        source: ShaderSource::Mui(ShaderId::Present),
    });
    let scope = device.push_error_scope(ErrorFilter::Validation);
    let _bad_pipe = device.create_render_pipeline(&RenderPipelineDescriptor {
        label: None,
        layout: Some(&pl),
        vertex: VertexState {
            module: &module,
            entry_point: Some("no_such_entry"),
            compilation_options: PipelineCompilationOptions,
            buffers: &[],
        },
        primitive: PrimitiveState::default(),
        depth_stencil: None,
        multisample: MultisampleState,
        fragment: Some(FragmentState {
            module: &module,
            entry_point: Some("fs_present"),
            compilation_options: PipelineCompilationOptions,
            targets: &[Some(ColorTargetState::from(TextureFormat::Rgba8Unorm))],
        }),
        multiview_mask: None,
        cache: None,
    });
    assert!(
        matches!(
            pollster::block_on(scope.pop()),
            Some(Error::Validation { .. })
        ),
        "create_render_pipeline with invalid entry point must report Validation"
    );

    // 7. After `device.destroy()`, creation calls report Internal error instead
    //    of panicking, and `PipelineCache::get_data` returns `None`.
    device.destroy();
    let scope = device.push_error_scope(ErrorFilter::Internal);
    let _sampler = device.create_sampler(&SamplerDescriptor::default());
    assert!(matches!(
        pollster::block_on(scope.pop()),
        Some(Error::Internal { .. })
    ));

    let scope = device.push_error_scope(ErrorFilter::Internal);
    let _qs = device.create_query_set(&QuerySetDescriptor {
        label: None,
        ty: QueryType::Timestamp,
        count: 2,
    });
    assert!(matches!(
        pollster::block_on(scope.pop()),
        Some(Error::Internal { .. })
    ));

    let scope = device.push_error_scope(ErrorFilter::Internal);
    #[expect(unsafe_code, reason = "testing create_pipeline_cache with empty data")]
    // SAFETY: `data: None` passes no cached driver blob.
    let bad_cache = unsafe {
        device.create_pipeline_cache(&PipelineCacheDescriptor {
            label: None,
            data: None,
            fallback: true,
        })
    };
    assert!(matches!(
        pollster::block_on(scope.pop()),
        Some(Error::Internal { .. })
    ));
    assert!(bad_cache.get_data().is_none());

    assert_eq!(validation_errors(), 0);
}
