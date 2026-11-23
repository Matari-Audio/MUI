//! Offscreen checks on the first adapter the machine offers. Skipped (with
//! a note) where no Vulkan 1.3 device exists.

use super::*;

fn device() -> Option<(Device, Queue)> {
    let instance = Instance::new(InstanceDescriptor::from_env());
    let adapters = pollster::block_on(instance.enumerate_adapters(Backends::all()));
    let Some(adapter) = adapters.into_iter().next() else {
        eprintln!("skipped: no adapter ({:?})", instance.backend_errors());
        return None;
    };
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
    if std::env::var("MUI_HAL_BACKEND").as_deref() != Ok("wgpu") {
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
