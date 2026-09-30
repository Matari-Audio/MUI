//! Effects on the GPU: full-frame fragment passes between four textures in
//! the canvas's format. A frame with effects renders its plain layers in
//! runs and each layer with effects alone into `L` (only the renderer writes
//! `L`, so its "already presented" shortcut stays true), runs the layer's
//! stack ping-ponging `P`/`Q`, composites the result over `A`, then runs the
//! scene's stack on `A` and copies it to the target. The uniforms of every
//! pass of a frame sit in one buffer at 256-byte dynamic offsets.
//!
//! WGSL modules are plain includes: `prelude.wgsl` (bindings, the `U`
//! header, noise) then the effect's file, which declares its `Params`. Kept
//! off naga_oil/wesl on purpose: `concat!` does it without shipping naga to
//! the browser.
use std::num::NonZeroU64;

use super::{EFFECTS, Fx};
use crate::gpu::GpuCanvas;
use crate::render::Assets;
use crate::{Drawn, Frame, Quad, Rgba};

/// Per-pass uniform slot: the header (32 bytes) and up to 56 floats.
const SLOT: u64 = 256;
const L: usize = 0;
const A: usize = 1;
const P: usize = 2;
const Q: usize = 3;

macro_rules! module {
    ($file:literal) => {
        concat!(include_str!("prelude.wgsl"), include_str!($file))
    };
}

/// The WGSL of an effect, by name.
fn source(name: &str) -> Option<&'static str> {
    Some(match name {
        "grain" => module!("grain.wgsl"),
        "chromatic" => module!("chromatic.wgsl"),
        "crt" => module!("crt.wgsl"),
        "displace" => module!("displace.wgsl"),
        "blur" => module!("blur.wgsl"),
        "directional_blur" => module!("directional_blur.wgsl"),
        "levels" => module!("levels.wgsl"),
        "plasma" => module!("plasma.wgsl"),
        _ => return None,
    })
}

struct Targets {
    size: [u32; 2],
    views: [wgpu::TextureView; 4],
    binds: [wgpu::BindGroup; 4],
}

pub(crate) struct Passes {
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    effects: Vec<wgpu::RenderPipeline>,
    copy: wgpu::RenderPipeline,
    over: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    slots: u64,
    targets: Option<Targets>,
    /// This frame's uniform bytes, a slot per pass, and how many of them
    /// are uploaded.
    staged: Vec<u8>,
    written: usize,
}

/// The header every pass's uniform starts with (`U` in prelude.wgsl).
#[derive(Clone, Copy)]
struct Header {
    res: [f32; 2],
    time: f32,
    scale: f32,
    seed: u32,
}

impl Passes {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Result<Self, String> {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mui-cut fx"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
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
        let pipeline = |name: &str, wgsl: &str, blend: Option<wgpu::BlendState>| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(name),
                source: wgpu::ShaderSource::Wgsl(wgsl.into()),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(name),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
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
        let effects = EFFECTS
            .iter()
            .map(|d| {
                let wgsl = source(d.name).ok_or_else(|| format!("no shader for `{}`", d.name))?;
                Ok(pipeline(d.name, wgsl, None))
            })
            .collect::<Result<_, String>>()?;
        let copy = pipeline("copy", module!("copy.wgsl"), None);
        let over = pipeline(
            "over",
            module!("copy.wgsl"),
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mui-cut fx"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let slots = 16;
        Ok(Self {
            format,
            layout,
            effects,
            copy,
            over,
            sampler,
            uniforms: uniforms(device, slots),
            slots,
            targets: None,
            staged: Vec::new(),
            written: 0,
        })
    }

    /// Textures at `size`, and bind groups over them and the uniforms.
    fn prepare(&mut self, device: &wgpu::Device, size: [u32; 2], passes: u64) {
        let grow = passes > self.slots;
        if grow {
            self.slots = passes.next_power_of_two();
            self.uniforms = uniforms(device, self.slots);
        }
        if !grow && self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let views = [0; 4].map(|_| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("mui-cut fx"),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        });
        let binds = std::array::from_fn(|i| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&views[i]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.uniforms,
                            offset: 0,
                            size: NonZeroU64::new(SLOT),
                        }),
                    },
                ],
            })
        });
        self.targets = Some(Targets { size, views, binds });
    }

    /// Stage one pass's uniform; its dynamic offset.
    fn stage(&mut self, h: Header, pass: u32, params: &[f32]) -> u32 {
        let at = self.staged.len();
        let mut words: Vec<u8> = Vec::with_capacity(SLOT as usize);
        for f in [h.res[0], h.res[1], h.time, h.scale] {
            words.extend(f.to_le_bytes());
        }
        for w in [h.seed, pass, 0, 0] {
            words.extend(w.to_le_bytes());
        }
        for f in params.iter().take(56) {
            words.extend(f.to_le_bytes());
        }
        words.resize(SLOT as usize, 0);
        self.staged.extend(words);
        at as u32
    }

    fn pass(
        &self,
        enc: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        src: usize,
        dst: &wgpu::TextureView,
        offset: u32,
        clear: bool,
    ) {
        let t = self.targets.as_ref().expect("prepared");
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("mui-cut fx"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: dst,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if clear {
                        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &t.binds[src], &[offset]);
        pass.draw(0..3, 0..1);
    }

    /// Encode `stack` over texture `src`; the texture holding the result.
    fn chain(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        h: Header,
        src: usize,
        stack: &[Fx],
    ) -> usize {
        let mut cur = src;
        for fx in stack {
            let Some((i, params)) = fx.pack() else {
                continue;
            };
            for pass in 0..EFFECTS[i].passes {
                let dst = if cur == P { Q } else { P };
                let offset = self.stage(h, pass, &params);
                let view = self.targets.as_ref().expect("prepared").views[dst].clone();
                self.pass(enc, &self.effects[i], cur, &view, offset, true);
                cur = dst;
            }
        }
        cur
    }

    /// Upload the uniforms staged since the last submit, and submit `enc`.
    fn submit(&mut self, canvas: &GpuCanvas, enc: wgpu::CommandEncoder) {
        let from = self.written;
        canvas
            .queue
            .write_buffer(&self.uniforms, from as u64, &self.staged[from..]);
        self.written = self.staged.len();
        canvas.queue.submit([enc.finish()]);
    }

    /// `frame` with its effects into `target`; its quads.
    pub(crate) fn draw(
        &mut self,
        canvas: &mut GpuCanvas,
        assets: &Assets,
        frame: &Frame,
        target: &wgpu::TextureView,
    ) -> Result<Vec<Quad>, String> {
        let size = canvas.size();
        let passes = |s: &[Fx]| -> u64 {
            s.iter()
                .filter_map(|f| super::def(&f.kind))
                .map(|(_, d)| u64::from(d.passes))
                .sum()
        };
        let total = passes(&frame.effects)
            + frame
                .layers
                .iter()
                .map(|l| passes(&l.effects) + 2)
                .sum::<u64>()
            + 2;
        self.prepare(&canvas.device, size, total);
        self.staged.clear();
        self.written = 0;
        let h = Header {
            res: size.map(|v| v as f32),
            time: frame.t as f32,
            scale: size[0] as f32 / frame.size[0].max(1) as f32,
            seed: frame.seed,
        };
        let view = |i: usize| self.targets.as_ref().expect("prepared").views[i].clone();
        let (lv, av) = (view(L), view(A));
        let clear = Rgba([0; 4]);
        let part = |background: Rgba, layers: Vec<Drawn>| Frame {
            background,
            layers,
            effects: Vec::new(),
            ..frame.clone()
        };
        let mut quads = Vec::with_capacity(frame.layers.len());
        let mut plain: Vec<Drawn> = Vec::new();
        let mut first = true;
        let encoder = |c: &GpuCanvas| {
            c.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor::default())
        };
        // A run of plain layers (the first also draws the background) over A.
        let flush = |this: &mut Self,
                     canvas: &mut GpuCanvas,
                     plain: &mut Vec<Drawn>,
                     first: &mut bool|
         -> Result<Vec<Quad>, String> {
            let bg = if *first { frame.background } else { clear };
            let q = canvas.paint(assets, &part(bg, std::mem::take(plain)), &lv)?;
            let mut enc = encoder(canvas);
            let offset = this.stage(h, 0, &[]);
            this.pass(&mut enc, &this.over, L, &av, offset, *first);
            this.submit(canvas, enc);
            *first = false;
            Ok(q)
        };
        for l in &frame.layers {
            if l.effects.is_empty() || l.opacity <= 0. || l.scale == 0. {
                plain.push(l.clone());
                continue;
            }
            if !plain.is_empty() || first {
                quads.extend(flush(self, canvas, &mut plain, &mut first)?);
            }
            quads.extend(canvas.paint(assets, &part(clear, vec![l.clone()]), &lv)?);
            let mut enc = encoder(canvas);
            let out = self.chain(&mut enc, h, L, &l.effects);
            let offset = self.stage(h, 0, &[]);
            self.pass(&mut enc, &self.over, out, &av, offset, false);
            self.submit(canvas, enc);
        }
        if !plain.is_empty() || first {
            quads.extend(flush(self, canvas, &mut plain, &mut first)?);
        }
        let mut enc = encoder(canvas);
        let out = self.chain(&mut enc, h, A, &frame.effects);
        let offset = self.stage(h, 0, &[]);
        self.pass(&mut enc, &self.copy, out, target, offset, true);
        self.submit(canvas, enc);
        Ok(quads)
    }
}

fn uniforms(device: &wgpu::Device, slots: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("mui-cut fx uniforms"),
        size: slots * SLOT,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
