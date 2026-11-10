//! The instanced SDF pass: every ordinary UI primitive as an instance of one
//! quad pipeline, antialiased in the fragment shader, in one draw call. No
//! compute shaders and no storage buffers, so it runs on a GLES 3 / WebGL2
//! class device.
//!
//! This is the first stage of replacing classic Vello. It draws rounded
//! rects (filled or stroked, solid or gradient), analytic drop and inset
//! shadows, and rect clips, under any affine transform, and matches what
//! [`crate::paint`] produces on the `Cpu` canvas. Anything else in a paint
//! list is reported by [`classify`] and refused by [`QuadRenderer::render`];
//! it is never skipped or approximated.

mod build;
#[cfg(test)]
mod tests;

use crate::kurbo::Affine;
use build::{Instance, SLOTS, Walk};
use mui_scene::Painted;

pub use build::MAX_STOPS;

/// The WGSL of the pass, for validation and tooling.
pub const SHADER: &str = include_str!("quad.wgsl");

/// Why the pass cannot draw an entry yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reason {
    /// A glyph run.
    Text,
    /// An image paint.
    Image,
    /// A retained vector paint.
    Vector,
    /// A fill or stroke whose outline is not a rounded rect.
    Path,
    /// A drop shadow whose outline is not a rounded rect: a filter layer.
    PathShadow,
    /// A clip whose outline is not a rounded rect.
    ClipPath,
    /// A rounded or rotated clip inside another: one is all an instance carries.
    ClipDepth,
    /// A gradient with more than [`MAX_STOPS`] stops.
    GradientStops,
    /// A blend layer, its end, or a mask inside one.
    Layer,
    /// A backdrop blur.
    Backdrop,
    /// External GPU material.
    External,
    /// A non-finite transform.
    NonFinite,
}

impl Reason {
    pub const ALL: [Self; 12] = [
        Self::Text,
        Self::Image,
        Self::Vector,
        Self::Path,
        Self::PathShadow,
        Self::ClipPath,
        Self::ClipDepth,
        Self::GradientStops,
        Self::Layer,
        Self::Backdrop,
        Self::External,
        Self::NonFinite,
    ];
}

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, f)
    }
}

/// How much of a paint list the pass can draw.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    /// Entries the pass draws, or accepts as drawing nothing (clip ends, a
    /// zero-area transform).
    pub supported: usize,
    /// Entries it cannot draw, per reason, indexed like [`Reason::ALL`].
    pub unsupported: [usize; Reason::ALL.len()],
}

impl Coverage {
    pub fn unsupported(&self, reason: Reason) -> usize {
        self.unsupported[reason as usize]
    }
    /// Every entry the pass can draw.
    pub fn complete(&self) -> bool {
        self.unsupported.iter().all(|&n| n == 0)
    }
}

/// How much of `paint` the pass can draw, under a caller transform that
/// keeps axes (a scale and a translation, as every host uses). A host that
/// rotates the whole scene turns square clips into general ones, which can
/// classify differently; [`QuadRenderer::render`] checks under the real one.
pub fn classify(paint: &[Painted]) -> Coverage {
    let mut walk = Walk::default();
    walk.begin();
    let mut scratch: Instance = [[0.; 4]; SLOTS];
    let mut coverage = Coverage::default();
    for p in paint {
        match walk.entry(p, Affine::IDENTITY, &mut scratch) {
            Ok(_) => coverage.supported += 1,
            Err(reason) => coverage.unsupported[reason as usize] += 1,
        }
    }
    coverage
}

/// Why [`QuadRenderer::render`] drew nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// Entry `index` of the list is something the pass cannot draw.
    Unsupported { index: usize, reason: Reason },
    /// The target size is zero or the transform is not finite.
    Target,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported { index, reason } => {
                write!(f, "paint entry {index} is unsupported by the quad pass: {reason}")
            }
            Self::Target => f.write_str("quad pass target is empty or its transform is not finite"),
        }
    }
}

impl std::error::Error for Error {}

const STRIDE: u64 = (SLOTS * 16) as u64;

/// The pass: one pipeline, one uniform, one instance buffer that grows and
/// is then reused.
pub struct QuadRenderer {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    instances: wgpu::Buffer,
    /// What the instance buffer holds this frame, as bytes; kept, so a
    /// steady frame allocates nothing.
    bytes: Vec<u8>,
    walk: Walk,
}

impl QuadRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mui quad"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mui quad view"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mui quad"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        const ATTRIBUTES: [wgpu::VertexAttribute; SLOTS] = wgpu::vertex_attr_array![
            0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4,
            4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4,
            8 => Float32x4, 9 => Float32x4, 10 => Float32x4, 11 => Float32x4,
            12 => Float32x4, 13 => Float32x4,
        ];
        // Premultiplied source-over, on encoded values, as `vello_cpu` blends.
        let over = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mui quad"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: STRIDE,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &ATTRIBUTES,
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState {
                        color: over,
                        alpha: over,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mui quad view"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mui quad view"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        Self {
            pipeline,
            uniform,
            bind,
            instances: instance_buffer(device, 256),
            bytes: Vec::new(),
            walk: Walk::default(),
        }
    }

    /// Clear `target` (`size` pixels) to transparent and draw `paint` on it
    /// under `transform`, the transform [`crate::paint`] takes: the device
    /// scale, usually. Builds every instance first, so a list with an
    /// unsupported entry draws nothing and names the first such entry.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        paint: &[Painted],
        transform: Affine,
        target: &wgpu::TextureView,
        size: [u32; 2],
    ) -> Result<(), Error> {
        if size.contains(&0) || !transform.as_coeffs().iter().all(|v| v.is_finite()) {
            return Err(Error::Target);
        }
        let count = self.build(paint, transform)?;
        if self.instances.size() < self.bytes.len() as u64 {
            let want = (count as u64).next_power_of_two();
            self.instances = instance_buffer(device, want);
        }
        if !self.bytes.is_empty() {
            queue.write_buffer(&self.instances, 0, &self.bytes);
        }
        let view = [size[0] as f32, size[1] as f32, 0., 0.];
        let mut uniform = [0u8; 16];
        for (chunk, v) in uniform.chunks_exact_mut(4).zip(view) {
            chunk.copy_from_slice(&v.to_ne_bytes());
        }
        queue.write_buffer(&self.uniform, 0, &uniform);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("mui quad"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mui quad"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if count > 0 {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind, &[]);
                pass.set_vertex_buffer(0, self.instances.slice(..self.bytes.len() as u64));
                pass.draw(0..4, 0..count as u32);
            }
        }
        queue.submit([encoder.finish()]);
        Ok(())
    }

    /// The instance bytes of `paint`, into `self.bytes`; their count.
    fn build(&mut self, paint: &[Painted], transform: Affine) -> Result<usize, Error> {
        self.bytes.clear();
        self.walk.begin();
        let mut one: Instance = [[0.; 4]; SLOTS];
        let mut count = 0;
        for (index, p) in paint.iter().enumerate() {
            match self.walk.entry(p, transform, &mut one) {
                Ok(true) => {
                    count += 1;
                    for v in one.as_flattened() {
                        self.bytes.extend_from_slice(&v.to_ne_bytes());
                    }
                }
                Ok(false) => {}
                Err(reason) => return Err(Error::Unsupported { index, reason }),
            }
        }
        Ok(count)
    }
}

fn instance_buffer(device: &wgpu::Device, count: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("mui quad instances"),
        size: count.max(1) * STRIDE,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
