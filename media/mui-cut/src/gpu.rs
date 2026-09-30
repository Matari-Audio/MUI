//! Frames on the GPU, on one of two Vello engines behind [`GpuCanvas`]:
//! MUI's retained `GpuRenderer` (classic Vello, compute shaders) or
//! `vello_gpu` (sparse strips, plain render passes, also on WebGL2; see
//! `sparse.rs`). Each layer's tree is painted under its affine over the
//! scene background. [`GpuCanvas`] draws into any texture view (the web
//! editor's canvas surface); native [`Offline`] adds the shutter (subframes
//! summed in an `Rgba16Float` target) and a ring of staging buffers, so the
//! GPU renders the next frame while the last one is piped to ffmpeg.
use mui_scene::ResolvedScene;
use mui_scene::prelude::*;
use mui_vello::effects::{Budget, GpuRenderer};
use mui_vello::kurbo::{Affine, Rect};

use crate::render::{Assets, color};
use crate::sparse::Sparse;
use crate::{Frame, Quad, Rgba};

/// Which Vello draws on the GPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    /// Classic Vello through MUI's `GpuRenderer`: compute shaders, so
    /// WebGPU/Vulkan/Metal/DX12 only.
    Classic,
    /// `vello_gpu`: CPU-side strips, GPU raster in render passes.
    Sparse,
}

impl Engine {
    pub fn name(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::Sparse => "vello_gpu",
        }
    }
}

enum Inner {
    /// With the background scene of the last frame (it only changes with
    /// the scene).
    Classic(Box<GpuRenderer>, Option<(Rgba, [u32; 2], ResolvedScene)>),
    Sparse(Box<Sparse>),
}

/// A GPU renderer kept alive across frames.
pub struct GpuCanvas {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    inner: Inner,
    size: [u32; 2],
}

impl GpuCanvas {
    /// `format` is the target's: a non-sRGB 8-bit RGBA or BGRA.
    pub async fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        size: [u32; 2],
        engine: Engine,
    ) -> Result<Self, String> {
        let inner = match engine {
            Engine::Classic => Inner::Classic(
                Box::new(
                    GpuRenderer::new(device, queue, format, size, Budget::default())
                        .await
                        .map_err(|e| e.to_string())?,
                ),
                None,
            ),
            Engine::Sparse => Inner::Sparse(Box::new(Sparse::new(device, format, size)?)),
        };
        Ok(Self {
            device: device.clone(),
            queue: queue.clone(),
            inner,
            size,
        })
    }
    pub fn engine(&self) -> Engine {
        match self.inner {
            Inner::Classic(..) => Engine::Classic,
            Inner::Sparse(_) => Engine::Sparse,
        }
    }
    pub fn size(&self) -> [u32; 2] {
        self.size
    }
    pub fn resize(&mut self, size: [u32; 2]) -> Result<(), String> {
        self.size = size;
        match &mut self.inner {
            Inner::Classic(r, _) => r.resize(size).map_err(|e| e.to_string()),
            Inner::Sparse(s) => s.resize(&self.device, size),
        }
    }
    /// `frame` scaled to fill `target` (at [`GpuCanvas::size`]); the layers'
    /// quads in project pixels. Submits its own work.
    pub fn draw(
        &mut self,
        assets: &Assets,
        frame: &Frame,
        target: &wgpu::TextureView,
    ) -> Result<Vec<Quad>, String> {
        let [fw, fh] = frame.size.map(f64::from);
        let view =
            Affine::scale_non_uniform(f64::from(self.size[0]) / fw, f64::from(self.size[1]) / fh);
        let layers = assets.layers(frame)?;
        let mut failed = None;
        let mut paint = |c: &mut dyn FnMut(&ResolvedScene, Affine) -> Result<(), String>| {
            for (scene, place) in &layers.scenes {
                if let Err(e) = c(scene, view * *place) {
                    failed = Some(e);
                }
            }
        };
        match &mut self.inner {
            Inner::Classic(renderer, base) => {
                let key = (frame.background, frame.size);
                if base.as_ref().is_none_or(|b| (b.0, b.1) != key) {
                    let bg = block(fw, fh).radius(0.).fill(color(frame.background));
                    let scene = resolve(&SceneSpec::new(bg)).map_err(|e| e.to_string())?;
                    *base = Some((key.0, key.1, scene));
                }
                let base = &base.as_ref().expect("set above").2;
                renderer
                    .render_with_overlay(base, view, target, |c| {
                        paint(&mut |s, t| {
                            mui_vello::paint(c, s, t).map_err(|e| format!("paint: {e:?}"))
                        });
                    })
                    .map_err(|e| e.to_string())?;
            }
            Inner::Sparse(sparse) => {
                sparse.upload(&self.device, &self.queue, assets.images());
                sparse.scene.reset();
                sparse.scene.set_transform(view);
                let [r, g, b, a] = frame.background.0;
                sparse
                    .scene
                    .set_paint(mui_vello::peniko::Color::from_rgba8(r, g, b, a));
                sparse.scene.fill_rect(&Rect::new(0., 0., fw, fh));
                let mut c = sparse.canvas();
                paint(&mut |s, t| {
                    mui_vello::paint(&mut c, s, t).map_err(|e| format!("paint: {e:?}"))
                });
                sparse.render(&self.device, &self.queue, target)?;
            }
        }
        failed.map_or(Ok(layers.quads), Err)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use offline::Offline;

#[cfg(not(target_arch = "wasm32"))]
mod offline {
    use std::collections::VecDeque;
    use std::sync::mpsc;

    use super::{Engine, GpuCanvas};
    use crate::Frame;
    use crate::render::Assets;

    /// Staging buffers in flight: the GPU can be two frames ahead of the
    /// frame being read back.
    const RING: usize = 3;

    type Mapped = mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>;

    /// Offline frames on the GPU: subframes averaged in linear light, one
    /// readback per output frame through a ring of staging buffers.
    pub struct Offline {
        canvas: GpuCanvas,
        pub assets: Assets,
        pub adapter: String,
        size: [u32; 2],
        stride: u32,
        sub: wgpu::TextureView,
        acc: wgpu::TextureView,
        out: wgpu::Texture,
        out_view: wgpu::TextureView,
        accumulate: wgpu::RenderPipeline,
        resolve: wgpu::RenderPipeline,
        sub_bind: wgpu::BindGroup,
        acc_bind: wgpu::BindGroup,
        weight: wgpu::Buffer,
        ring: Vec<wgpu::Buffer>,
        pending: VecDeque<(usize, wgpu::SubmissionIndex, Mapped)>,
        next: usize,
    }

    fn texture(
        device: &wgpu::Device,
        size: [u32; 2],
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
    ) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mui-cut"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    }

    fn pass(
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        bind: &wgpu::BindGroup,
        view: &wgpu::TextureView,
        load: wgpu::LoadOp<wgpu::Color>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("mui-cut shutter"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }

    impl Offline {
        /// A headless device, the high-performance adapter if there are two.
        pub fn new(size: [u32; 2], engine: Engine) -> Result<Self, String> {
            pollster::block_on(Self::open(size, engine))
        }

        async fn open(size: [u32; 2], engine: Engine) -> Result<Self, String> {
            use wgpu::TextureFormat as F;
            use wgpu::TextureUsages as U;
            let instance = wgpu::Instance::default();
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    ..Default::default()
                })
                .await
                .map_err(|e| format!("no GPU adapter: {e}"))?;
            let info = adapter.get_info();
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor::default())
                .await
                .map_err(|e| e.to_string())?;
            let canvas = GpuCanvas::new(&device, &queue, F::Rgba8Unorm, size, engine).await?;
            let sub = texture(
                &device,
                size,
                F::Rgba8Unorm,
                U::RENDER_ATTACHMENT | U::TEXTURE_BINDING,
            );
            let acc = texture(
                &device,
                size,
                F::Rgba16Float,
                U::RENDER_ATTACHMENT | U::TEXTURE_BINDING,
            );
            let out = texture(
                &device,
                size,
                F::Rgba8Unorm,
                U::RENDER_ATTACHMENT | U::COPY_SRC,
            );
            let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
            let (sub, acc, out_view) = (view(&sub), view(&acc), view(&out));

            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("mui-cut shutter"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shutter.wgsl").into()),
            });
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("mui-cut shutter"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let pipeline = |entry: &str, format: F, blend: Option<wgpu::BlendState>| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("vs"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(entry),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview_mask: None,
                    cache: None,
                })
            };
            let add = wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            };
            let accumulate = pipeline(
                "accumulate",
                F::Rgba16Float,
                Some(wgpu::BlendState {
                    color: add,
                    alpha: add,
                }),
            );
            let resolve = pipeline("resolve", F::Rgba8Unorm, None);
            let weight = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mui-cut shutter weight"),
                size: 16,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bind = |v: &wgpu::TextureView| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(v),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: weight.as_entire_binding(),
                        },
                    ],
                })
            };
            let (sub_bind, acc_bind) = (bind(&sub), bind(&acc));
            // Rows padded to 256 bytes, as a texture-to-buffer copy wants.
            let stride = (size[0] * 4).next_multiple_of(256);
            let ring = (0..RING)
                .map(|_| {
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("mui-cut readback"),
                        size: u64::from(stride) * u64::from(size[1]),
                        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                        mapped_at_creation: false,
                    })
                })
                .collect();
            Ok(Self {
                canvas,
                assets: Assets::default(),
                adapter: format!("{}, {} ({:?})", engine.name(), info.name, info.backend),
                size,
                stride,
                sub,
                acc,
                out,
                out_view,
                accumulate,
                resolve,
                sub_bind,
                acc_bind,
                weight,
                ring,
                pending: VecDeque::new(),
                next: 0,
            })
        }

        /// Render one output frame from its subframes (one for no blur). The
        /// pixels that come back are an earlier frame's, straight-alpha
        /// RGBA, once the ring is full; [`Offline::finish`] drains the rest.
        pub fn push(&mut self, subframes: &[Frame]) -> Result<Option<Vec<u8>>, String> {
            let done = if self.pending.len() == RING {
                Some(self.read_oldest()?)
            } else {
                None
            };
            let (device, queue) = (self.canvas.device.clone(), self.canvas.queue.clone());
            let k = 1. / subframes.len().max(1) as f32;
            queue.write_buffer(
                &self.weight,
                0,
                &[k, 0., 0., 0.].map(f32::to_le_bytes).concat(),
            );
            for (i, f) in subframes.iter().enumerate() {
                self.canvas.draw(&self.assets, f, &self.sub)?;
                let mut enc =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                let load = if i == 0 {
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                } else {
                    wgpu::LoadOp::Load
                };
                pass(&mut enc, &self.accumulate, &self.sub_bind, &self.acc, load);
                queue.submit([enc.finish()]);
            }
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            let clear = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
            pass(
                &mut enc,
                &self.resolve,
                &self.acc_bind,
                &self.out_view,
                clear,
            );
            let slot = self.next;
            enc.copy_texture_to_buffer(
                self.out.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &self.ring[slot],
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(self.stride),
                        rows_per_image: Some(self.size[1]),
                    },
                },
                wgpu::Extent3d {
                    width: self.size[0],
                    height: self.size[1],
                    depth_or_array_layers: 1,
                },
            );
            let index = queue.submit([enc.finish()]);
            let (send, recv) = mpsc::sync_channel(1);
            self.ring[slot]
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |r| {
                    let _ = send.send(r);
                });
            self.pending.push_back((slot, index, recv));
            self.next = (slot + 1) % RING;
            Ok(done)
        }

        /// Every frame still in flight, oldest first.
        pub fn finish(&mut self) -> Result<Vec<Vec<u8>>, String> {
            let mut out = Vec::new();
            while !self.pending.is_empty() {
                out.push(self.read_oldest()?);
            }
            Ok(out)
        }

        fn read_oldest(&mut self) -> Result<Vec<u8>, String> {
            let (slot, index, mapped) = self.pending.pop_front().ok_or("nothing in flight")?;
            self.canvas
                .device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(index),
                    timeout: None,
                })
                .map_err(|e| e.to_string())?;
            mapped
                .recv()
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
            let buf = &self.ring[slot];
            let row = self.size[0] as usize * 4;
            let mut px = Vec::with_capacity(row * self.size[1] as usize);
            {
                let view = buf
                    .slice(..)
                    .get_mapped_range()
                    .map_err(|e| e.to_string())?;
                for r in view
                    .chunks(self.stride as usize)
                    .take(self.size[1] as usize)
                {
                    px.extend_from_slice(&r[..row]);
                }
            }
            buf.unmap();
            Ok(px)
        }
    }
}
