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
    /// With the base scene of the last frame: the background, or a clear
    /// one for the 3D atlas (it only changes with the scene).
    Classic(
        Box<GpuRenderer>,
        Option<(Option<Rgba>, [u32; 2], ResolvedScene)>,
    ),
    Sparse(Box<Sparse>),
}

/// A GPU renderer kept alive across frames.
pub struct GpuCanvas {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    inner: Inner,
    format: wgpu::TextureFormat,
    size: [u32; 2],
    /// The size the Vello engine is set up for: the target's, or the 3D
    /// atlas's.
    vello_size: [u32; 2],
    /// 3D scenes in 3D; off, they draw flat (see [`GpuCanvas::notice`]).
    pub three_d: bool,
    space: Option<Box<crate::gpu3d::Space>>,
    notice: String,
    /// Effect passes, made the first time a frame has effects.
    fx: Option<Box<crate::fx::gpu::Passes>>,
    /// The beauty sample 3D frames draw as ([`mui_stage::Shot::sample`]);
    /// the shutter sets it per subframe.
    pub(crate) sample: Option<u32>,
    /// 3D glass ray traced: 0 the deterministic trace, else path traced,
    /// these many paths per pixel per draw; `None` is raster glass. Set
    /// only on a device with ray queries.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "the web has no ray-traced glass")
    )]
    pub(crate) glass: Option<u32>,
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
            format,
            size,
            vello_size: size,
            three_d: true,
            space: None,
            notice: String::new(),
            fx: None,
            sample: None,
            glass: None,
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
        self.space = None;
        Ok(())
    }
    /// Why the last 3D frame was drawn flat, or empty.
    pub fn notice(&self) -> &str {
        &self.notice
    }
    /// `frame` scaled to fill `target` (at [`GpuCanvas::size`]), effects and
    /// all; the layers' quads in project pixels. Submits its own work.
    pub fn draw(
        &mut self,
        assets: &Assets,
        frame: &Frame,
        target: &wgpu::TextureView,
    ) -> Result<Vec<Quad>, String> {
        // A failed 3D pass keeps its reason for as long as it draws flat.
        if frame.view.is_none() || self.three_d {
            self.notice.clear();
        }
        if let Some(view) = &frame.view {
            if self.three_d {
                // The scene's stack runs on the 3D pass's output, then the
                // overlay layers go over it flat.
                let drawn = if frame.effects.is_empty() && !frame.has_overlays() {
                    self.draw_3d(assets, frame, view, target)
                } else {
                    self.with_fx(|fx, canvas| fx.draw_3d(canvas, assets, frame, view, target))
                };
                match drawn {
                    Ok(quads) => return Ok(quads),
                    Err(e) => {
                        self.three_d = false;
                        self.space = None;
                        self.notice = format!("3D unavailable, drawn flat: {e}");
                    }
                }
            } else if self.notice.is_empty() {
                "3D scenes draw flat on this renderer".clone_into(&mut self.notice);
            }
        }
        if !frame.has_effects() {
            return self.paint(assets, frame, target);
        }
        self.with_fx(|fx, canvas| fx.draw(canvas, assets, frame, target))
    }

    /// `frame`'s scene effects over `rgba`, a straight-RGBA picture at
    /// [`GpuCanvas::size`] rendered elsewhere (a Blender frame), and its
    /// overlay layers over that, into `target`. Submits its own work.
    pub fn draw_plate(
        &mut self,
        assets: &Assets,
        frame: &Frame,
        rgba: &[u8],
        target: &wgpu::TextureView,
    ) -> Result<(), String> {
        self.with_fx(|fx, canvas| fx.plate(canvas, assets, frame, rgba, target))
    }

    /// `f` with the effect passes, made the first time.
    fn with_fx<R>(
        &mut self,
        f: impl FnOnce(&mut crate::fx::gpu::Passes, &mut Self) -> Result<R, String>,
    ) -> Result<R, String> {
        let mut fx = match self.fx.take() {
            Some(fx) => fx,
            None => Box::new(crate::fx::gpu::Passes::new(&self.device, self.format)?),
        };
        let out = f(&mut fx, self);
        self.fx = Some(fx);
        out
    }

    /// The effects' hook: `frame`'s layers and background as the renderer
    /// draws them, ignoring effects.
    pub(crate) fn paint(
        &mut self,
        assets: &Assets,
        frame: &Frame,
        target: &wgpu::TextureView,
    ) -> Result<Vec<Quad>, String> {
        let [fw, fh] = frame.size.map(f64::from);
        let view =
            Affine::scale_non_uniform(f64::from(self.size[0]) / fw, f64::from(self.size[1]) / fh);
        let layers = assets.layers(frame)?;
        let placed: Vec<(&ResolvedScene, Affine)> =
            layers.scenes.iter().map(|(s, p)| (s, view * *p)).collect();
        self.paint_scenes(
            assets,
            &placed,
            Some((frame.background, [fw, fh], view)),
            self.size,
            target,
        )?;
        Ok(layers.all_quads())
    }

    /// Paint `scenes` into `target`, `size` pixels, over `background` (a
    /// colour filling `[w, h]` under an affine) or over nothing.
    pub(crate) fn paint_scenes(
        &mut self,
        assets: &Assets,
        scenes: &[(&ResolvedScene, Affine)],
        background: Option<(Rgba, [f64; 2], Affine)>,
        size: [u32; 2],
        target: &wgpu::TextureView,
    ) -> Result<(), String> {
        if self.vello_size != size {
            match &mut self.inner {
                Inner::Classic(r, _) => r.resize(size).map_err(|e| e.to_string())?,
                Inner::Sparse(s) => s.resize(&self.device, size)?,
            }
            self.vello_size = size;
        }
        let mut failed = None;
        let mut paint = |c: &mut dyn FnMut(&ResolvedScene, Affine) -> Result<(), String>| {
            for (scene, place) in scenes {
                if let Err(e) = c(scene, *place) {
                    failed = Some(e);
                }
            }
        };
        match &mut self.inner {
            Inner::Classic(renderer, base) => {
                let (colour, [fw, fh], view) = match background {
                    Some((c, wh, v)) => (Some(c), wh, v),
                    None => (None, size.map(f64::from), Affine::IDENTITY),
                };
                let key = (colour, [fw as u32, fh as u32]);
                if base.as_ref().is_none_or(|b| (b.0, b.1) != key) {
                    let bg = block(fw, fh).radius(0.);
                    let bg = match colour {
                        Some(c) => bg.fill(color(c)),
                        None => bg,
                    };
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
                if let Some((bg, [fw, fh], view)) = background {
                    sparse.scene.set_transform(view);
                    let [r, g, b, a] = bg.0;
                    sparse
                        .scene
                        .set_paint(mui_vello::peniko::Color::from_rgba8(r, g, b, a));
                    sparse.scene.fill_rect(&Rect::new(0., 0., fw, fh));
                }
                let mut c = sparse.canvas();
                paint(&mut |s, t| {
                    mui_vello::paint(&mut c, s, t).map_err(|e| format!("paint: {e:?}"))
                });
                sparse.render(&self.device, &self.queue, target)?;
            }
        }
        failed.map_or(Ok(()), Err)
    }

    pub(crate) fn draw_3d(
        &mut self,
        assets: &Assets,
        frame: &Frame,
        view: &crate::View,
        target: &wgpu::TextureView,
    ) -> Result<Vec<Quad>, String> {
        if self.space.is_none() {
            let stage = mui_stage::Stage::with_device(
                self.device.clone(),
                self.queue.clone(),
                self.size[0],
                self.size[1],
            )
            .map_err(|e| e.to_string())?;
            self.space = Some(Box::new(crate::gpu3d::Space::new(stage)));
        }
        let mut space = self.space.take().expect("made above");
        let format = self.format;
        let out = space.draw(self, assets, frame, view, target, format);
        self.space = Some(space);
        out
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
    use crate::shutter::{Shutter, texture};
    use crate::yuv::Yuv;

    /// Staging buffers in flight: the GPU can be two frames ahead of the
    /// frame being read back.
    const RING: usize = 3;

    type Mapped = mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>;

    /// Offline frames on the GPU: subframes averaged in linear light, one
    /// readback per output frame through a ring of staging buffers.
    pub struct Offline {
        pub(crate) canvas: GpuCanvas,
        pub assets: Assets,
        pub adapter: String,
        size: [u32; 2],
        stride: u32,
        shutter: Shutter,
        out: wgpu::Texture,
        out_view: wgpu::TextureView,
        ring: Vec<wgpu::Buffer>,
        pending: VecDeque<(usize, wgpu::SubmissionIndex, Mapped)>,
        next: usize,
        yuv: Option<YuvPass>,
        /// Subframes are beauty samples too (see [`Shutter::expose`]).
        pub beauty: bool,
    }

    /// The compute pass from the float sum to 4:2:0 planes.
    struct YuvPass {
        yuv: Yuv,
        pipeline: wgpu::ComputePipeline,
        bind: wgpu::BindGroup,
        planes: wgpu::Buffer,
        /// Bytes per row of either plane.
        stride: u32,
    }

    impl YuvPass {
        fn new(device: &wgpu::Device, yuv: Yuv, [w, h]: [u32; 2], acc: &wgpu::TextureView) -> Self {
            let stride = match yuv {
                Yuv::Nv12 => w.next_multiple_of(4),
                Yuv::P010 => w * 2,
            };
            let planes = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mui-cut yuv"),
                size: u64::from(stride) * u64::from(h + h / 2),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let cfg = wgpu::util::DeviceExt::create_buffer_init(
                device,
                &wgpu::util::BufferInitDescriptor {
                    label: Some("mui-cut yuv cfg"),
                    contents: &[w, h, stride, 0].map(u32::to_le_bytes).concat(),
                    usage: wgpu::BufferUsages::UNIFORM,
                },
            );
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("mui-cut yuv"),
                source: wgpu::ShaderSource::Wgsl(include_str!("yuv.wgsl").into()),
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("mui-cut yuv"),
                layout: None,
                module: &module,
                entry_point: Some(match yuv {
                    Yuv::Nv12 => "nv12",
                    Yuv::P010 => "p010",
                }),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(acc),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: planes.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: cfg.as_entire_binding(),
                    },
                ],
            });
            Self {
                yuv,
                pipeline,
                bind,
                planes,
                stride,
            }
        }

        /// Convert the float sum and copy the planes to `readback`.
        fn encode(
            &self,
            enc: &mut wgpu::CommandEncoder,
            [w, h]: [u32; 2],
            readback: &wgpu::Buffer,
        ) {
            let across = match self.yuv {
                Yuv::Nv12 => w.div_ceil(4),
                Yuv::P010 => w.div_ceil(2),
            };
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind, &[]);
                pass.dispatch_workgroups(across.div_ceil(8), h.div_ceil(2).div_ceil(8), 1);
            }
            enc.copy_buffer_to_buffer(&self.planes, 0, readback, 0, self.planes.size());
        }
    }

    impl Offline {
        /// A headless device, the high-performance adapter if there are two;
        /// frames come back as straight-alpha RGBA.
        pub fn new(size: [u32; 2], engine: Engine) -> Result<Self, String> {
            pollster::block_on(Self::open(size, engine, None, None))
        }

        /// [`Offline::new`] or [`Offline::with_yuv`], and with `glass`
        /// 3D glass path traced at that many paths per pixel per subframe
        /// (see [`GpuCanvas::glass`]) when the adapter has hardware ray
        /// queries; when it has none, raster glass and a warning.
        pub fn open_with(
            size: [u32; 2],
            engine: Engine,
            yuv: Option<Yuv>,
            glass: Option<u32>,
        ) -> Result<Self, String> {
            pollster::block_on(Self::open(size, engine, yuv, glass))
        }

        /// Frames come back as `yuv` planes, converted on the GPU from the
        /// float sum: what an encoder takes, at 1.5 (or 3) bytes a pixel.
        pub fn with_yuv(size: [u32; 2], engine: Engine, yuv: Yuv) -> Result<Self, String> {
            pollster::block_on(Self::open(size, engine, Some(yuv), None))
        }

        async fn open(
            size: [u32; 2],
            engine: Engine,
            yuv: Option<Yuv>,
            glass: Option<u32>,
        ) -> Result<Self, String> {
            let instance = wgpu::Instance::default();
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    ..Default::default()
                })
                .await
                .map_err(|e| format!("no GPU adapter: {e}"))?;
            let info = adapter.get_info();
            let rt = glass.is_some() && mui_stage_rt::supported(&adapter);
            if glass.is_some() && !rt {
                eprintln!(
                    "mui-cut: {} has no hardware ray queries; drawing raster glass",
                    info.name
                );
            }
            let desc = if rt {
                mui_stage_rt::device_descriptor(&adapter)
            } else {
                wgpu::DeviceDescriptor::default()
            };
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    // `MUI_STAGE_TIMING` reads the stage's GPU time.
                    required_features: desc.required_features
                        | (adapter.features() & mui_stage::TIMESTAMPS),
                    ..desc
                })
                .await
                .map_err(|e| e.to_string())?;
            let mut o = Self::build(&device, &queue, size, engine, yuv).await?;
            o.canvas.glass = glass.filter(|_| rt);
            o.adapter = format!("{}, {} ({:?})", engine.name(), info.name, info.backend);
            if rt {
                o.adapter.push_str(", ray-traced glass");
            }
            Ok(o)
        }

        /// Frames of another size (or pixel format) on the same device, its
        /// assets kept: a render of several variants opens one GPU.
        pub fn resize(&mut self, size: [u32; 2], yuv: Option<Yuv>) -> Result<(), String> {
            if !self.pending.is_empty() {
                return Err("resize with frames in flight: finish() first".into());
            }
            let (device, queue) = (self.canvas.device.clone(), self.canvas.queue.clone());
            let engine = self.canvas.engine();
            let mut o = pollster::block_on(Self::build(&device, &queue, size, engine, yuv))?;
            o.assets = std::mem::take(&mut self.assets);
            o.adapter = std::mem::take(&mut self.adapter);
            o.beauty = self.beauty;
            o.canvas.glass = self.canvas.glass;
            *self = o;
            Ok(())
        }

        async fn build(
            device: &wgpu::Device,
            queue: &wgpu::Queue,
            size: [u32; 2],
            engine: Engine,
            yuv: Option<Yuv>,
        ) -> Result<Self, String> {
            use wgpu::TextureFormat as F;
            use wgpu::TextureUsages as U;
            let canvas = GpuCanvas::new(device, queue, F::Rgba8Unorm, size, engine).await?;
            let shutter = Shutter::new(device, size, F::Rgba8Unorm, F::Rgba8Unorm);
            let out = texture(
                device,
                size,
                F::Rgba8Unorm,
                U::RENDER_ATTACHMENT | U::COPY_SRC,
            );
            let out_view = out.create_view(&wgpu::TextureViewDescriptor::default());
            // Rows padded to 256 bytes, as a texture-to-buffer copy wants.
            let stride = (size[0] * 4).next_multiple_of(256);
            let yuv = yuv.map(|y| YuvPass::new(device, y, size, shutter.sum()));
            let bytes = yuv
                .as_ref()
                .map_or(u64::from(stride) * u64::from(size[1]), |y| y.planes.size());
            let ring = (0..RING)
                .map(|_| {
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("mui-cut readback"),
                        size: bytes,
                        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                        mapped_at_creation: false,
                    })
                })
                .collect();
            Ok(Self {
                canvas,
                assets: Assets::default(),
                adapter: String::new(),
                size,
                stride,
                shutter,
                out,
                out_view,
                ring,
                pending: VecDeque::new(),
                next: 0,
                yuv,
                beauty: false,
            })
        }

        /// Render one output frame from its subframes (one for no blur). The
        /// pixels that come back are an earlier frame's, straight-alpha
        /// RGBA, once the ring is full; [`Offline::finish`] drains the rest.
        pub fn push(&mut self, subframes: &[Frame]) -> Result<Option<Vec<u8>>, String> {
            let done = self.make_room()?;
            self.shutter
                .expose(&mut self.canvas, &self.assets, subframes, self.beauty)?;
            self.read_back(done)
        }

        /// [`Offline::push`] for a picture rendered elsewhere (a Blender
        /// frame, straight RGBA at this size): `frame`'s scene effects run
        /// over it.
        pub fn push_plate(
            &mut self,
            frame: &Frame,
            rgba: &[u8],
        ) -> Result<Option<Vec<u8>>, String> {
            let done = self.make_room()?;
            self.shutter
                .expose_plate(&mut self.canvas, &self.assets, frame, rgba)?;
            self.read_back(done)
        }

        /// The web viewport's Beauty preview offline: `frame` refined by
        /// `n` running-mean samples ([`Shutter::expose_sample`]).
        #[cfg(test)]
        pub(crate) fn push_refined(
            &mut self,
            frame: &Frame,
            n: u32,
        ) -> Result<Option<Vec<u8>>, String> {
            let done = self.make_room()?;
            for i in 0..n {
                self.shutter
                    .expose_sample(&mut self.canvas, &self.assets, frame, i)?;
            }
            self.read_back(done)
        }

        /// The oldest frame, once the ring is full.
        fn make_room(&mut self) -> Result<Option<Vec<u8>>, String> {
            if self.pending.len() == RING {
                self.read_oldest().map(Some)
            } else {
                Ok(None)
            }
        }

        /// The sum on its way back; `done` passed through.
        fn read_back(&mut self, done: Option<Vec<u8>>) -> Result<Option<Vec<u8>>, String> {
            let (device, queue) = (&self.canvas.device, &self.canvas.queue);
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            let slot = self.next;
            if let Some(y) = &self.yuv {
                y.encode(&mut enc, self.size, &self.ring[slot]);
            } else {
                self.shutter.resolve(&mut enc, &self.out_view);
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
            }
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
            // RGBA rows, or the rows of both planes (Y, then half as many UV).
            let (row, stride, rows) = match &self.yuv {
                Some(y) => (
                    y.yuv.frame_bytes(self.size) / (self.size[1] as usize * 3 / 2),
                    y.stride,
                    self.size[1] as usize * 3 / 2,
                ),
                None => (
                    self.size[0] as usize * 4,
                    self.stride,
                    self.size[1] as usize,
                ),
            };
            let mut px = Vec::with_capacity(row * rows);
            {
                let view = buf
                    .slice(..)
                    .get_mapped_range()
                    .map_err(|e| e.to_string())?;
                for r in view.chunks(stride as usize).take(rows) {
                    px.extend_from_slice(&r[..row]);
                }
            }
            buf.unmap();
            Ok(px)
        }
    }
}
