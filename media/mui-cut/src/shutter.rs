//! The shutter on the GPU, for both targets: subframes drawn through a
//! [`GpuCanvas`] and added, weighted 1/N, into an `Rgba16Float` sum in
//! linear light (`shutter.wgsl`), then resolved to straight-alpha sRGB in
//! any 8-bit target: the CLI's readback texture or the browser's export
//! canvas.
use crate::Frame;
use crate::gpu::GpuCanvas;
use crate::render::Assets;

pub(crate) fn texture(
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
    blend_pass(encoder, pipeline, bind, view, load, 1.);
}

/// [`pass`] with a blend constant.
fn blend_pass(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    bind: &wgpu::BindGroup,
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
    k: f64,
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
    pass.set_blend_constant(wgpu::Color {
        r: k,
        g: k,
        b: k,
        a: k,
    });
    pass.draw(0..3, 0..1);
}

pub struct Shutter {
    size: [u32; 2],
    sub: wgpu::TextureView,
    acc: wgpu::TextureView,
    accumulate: wgpu::RenderPipeline,
    /// A running mean: the sum moves `k` of the way to the new sample.
    average: wgpu::RenderPipeline,
    resolve: wgpu::RenderPipeline,
    sub_bind: wgpu::BindGroup,
    acc_bind: wgpu::BindGroup,
    weight: wgpu::Buffer,
}

impl Shutter {
    /// Subframes drawn at `size` by a canvas of `canvas_format`, resolved
    /// into targets of `out_format`.
    pub fn new(
        device: &wgpu::Device,
        size: [u32; 2],
        canvas_format: wgpu::TextureFormat,
        out_format: wgpu::TextureFormat,
    ) -> Self {
        use wgpu::TextureFormat as F;
        use wgpu::TextureUsages as U;
        let usage = U::RENDER_ATTACHMENT | U::TEXTURE_BINDING;
        let view = |t: wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let sub = view(texture(device, size, canvas_format, usage));
        let acc = view(texture(device, size, F::Rgba16Float, usage));
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
        let mean = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Constant,
            dst_factor: wgpu::BlendFactor::OneMinusConstant,
            operation: wgpu::BlendOperation::Add,
        };
        let average = pipeline(
            "accumulate",
            F::Rgba16Float,
            Some(wgpu::BlendState {
                color: mean,
                alpha: mean,
            }),
        );
        let resolve = pipeline("resolve", out_format, None);
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
        Self {
            size,
            sub,
            acc,
            accumulate,
            average,
            resolve,
            sub_bind,
            acc_bind,
            weight,
        }
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    /// The float sum, for passes that read it (the encoder's YUV).
    pub fn sum(&self) -> &wgpu::TextureView {
        &self.acc
    }

    /// Draw every subframe through `canvas` into the sum; with `beauty`,
    /// subframe `i` is also beauty sample `i` of a 3D scene (jittered
    /// pixel, lens, area lights). Submits.
    pub fn expose(
        &self,
        canvas: &mut GpuCanvas,
        assets: &Assets,
        subframes: &[Frame],
        beauty: bool,
    ) -> Result<(), String> {
        let (device, queue) = (canvas.device.clone(), canvas.queue.clone());
        let k = 1. / subframes.len().max(1) as f32;
        queue.write_buffer(
            &self.weight,
            0,
            &[k, 0., 0., 0.].map(f32::to_le_bytes).concat(),
        );
        for (i, f) in subframes.iter().enumerate() {
            canvas.sample = beauty.then_some(i as u32);
            let drawn = canvas.draw(assets, f, &self.sub);
            canvas.sample = None;
            drawn?;
            // An export never quietly flattens a 3D shot.
            if f.view.is_some() && !canvas.notice().is_empty() {
                return Err(canvas.notice().to_owned());
            }
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            let load = if i == 0 {
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
            } else {
                wgpu::LoadOp::Load
            };
            pass(&mut enc, &self.accumulate, &self.sub_bind, &self.acc, load);
            queue.submit([enc.finish()]);
        }
        Ok(())
    }

    /// Beauty sample `i` of `frame` into a running mean of the ones before
    /// (sample 0 starts it): a paused viewport refining. Submits.
    pub fn expose_sample(
        &self,
        canvas: &mut GpuCanvas,
        assets: &Assets,
        frame: &Frame,
        i: u32,
    ) -> Result<Vec<crate::Quad>, String> {
        canvas.sample = Some(i);
        let quads = canvas.draw(assets, frame, &self.sub);
        canvas.sample = None;
        let quads = quads?;
        canvas.queue.write_buffer(
            &self.weight,
            0,
            &[1f32, 0., 0., 0.].map(f32::to_le_bytes).concat(),
        );
        let mut enc = canvas
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let load = if i == 0 {
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
        } else {
            wgpu::LoadOp::Load
        };
        let k = 1. / f64::from(i + 1);
        blend_pass(&mut enc, &self.average, &self.sub_bind, &self.acc, load, k);
        canvas.queue.submit([enc.finish()]);
        Ok(quads)
    }

    /// [`Shutter::expose`] of one picture rendered elsewhere, `frame`'s
    /// scene effects over it. Submits.
    pub fn expose_plate(
        &self,
        canvas: &mut GpuCanvas,
        frame: &Frame,
        rgba: &[u8],
    ) -> Result<(), String> {
        let queue = canvas.queue.clone();
        queue.write_buffer(
            &self.weight,
            0,
            &[1f32, 0., 0., 0.].map(f32::to_le_bytes).concat(),
        );
        canvas.draw_plate(frame, rgba, &self.sub)?;
        let mut enc = canvas
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let clear = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        pass(&mut enc, &self.accumulate, &self.sub_bind, &self.acc, clear);
        queue.submit([enc.finish()]);
        Ok(())
    }

    /// The sum as straight-alpha sRGB into `target`.
    pub fn resolve(&self, enc: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let clear = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        pass(enc, &self.resolve, &self.acc_bind, target, clear);
    }
}
