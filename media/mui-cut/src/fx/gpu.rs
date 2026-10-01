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
    textures: [wgpu::Texture; 4],
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
        let textures = [0; 4].map(|_| {
            device.create_texture(&wgpu::TextureDescriptor {
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
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        });
        let views = textures
            .each_ref()
            .map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));
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
        self.targets = Some(Targets {
            size,
            textures,
            views,
            binds,
        });
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

    /// Targets and uniform slots for `passes` passes; the frame's header.
    fn begin(&mut self, canvas: &GpuCanvas, frame: &Frame, passes: u64) -> Header {
        let size = canvas.size();
        self.prepare(&canvas.device, size, passes);
        self.staged.clear();
        self.written = 0;
        Header {
            res: size.map(|v| v as f32),
            time: frame.t as f32,
            scale: size[0] as f32 / frame.size[0].max(1) as f32,
            seed: frame.seed,
        }
    }

    /// The scene's stack over `A`, then `overlays` over that (a 3D frame's
    /// flat layers), then the result into `target`; the overlays' quads.
    fn finish(
        &mut self,
        canvas: &mut GpuCanvas,
        assets: &Assets,
        frame: &Frame,
        h: Header,
        overlays: &[&Drawn],
        target: &wgpu::TextureView,
    ) -> Result<Vec<Quad>, String> {
        let mut enc = encoder(canvas);
        let mut out = self.chain(&mut enc, h, A, &frame.effects);
        let mut quads = Vec::new();
        if !overlays.is_empty() {
            // Back into `A`: an overlay's own stack ping-pongs `P` and `Q`.
            if out != A {
                let offset = self.stage(h, 0, &[]);
                let a = self.targets.as_ref().expect("prepared").views[A].clone();
                self.pass(&mut enc, &self.copy, out, &a, offset, true);
                out = A;
            }
            self.submit(canvas, enc);
            quads = self.composite(canvas, assets, frame, h, overlays, false)?;
            enc = encoder(canvas);
        }
        let offset = self.stage(h, 0, &[]);
        self.pass(&mut enc, &self.copy, out, target, offset, true);
        self.submit(canvas, enc);
        Ok(quads)
    }

    /// `layers` over `A`: runs of plain layers painted together, a layer
    /// with effects alone through its stack. `first` paints the frame's
    /// background under the first run and replaces `A`; else they go over
    /// what `A` holds. Their quads.
    fn composite(
        &mut self,
        canvas: &mut GpuCanvas,
        assets: &Assets,
        frame: &Frame,
        h: Header,
        layers: &[&Drawn],
        mut first: bool,
    ) -> Result<Vec<Quad>, String> {
        let view = |i: usize| self.targets.as_ref().expect("prepared").views[i].clone();
        let (lv, av) = (view(L), view(A));
        let clear = Rgba([0; 4]);
        let part = |background: Rgba, layers: Vec<Drawn>| Frame {
            background,
            layers,
            effects: Vec::new(),
            // Painted flat: a 3D frame's overlays are 2D layers.
            view: None,
            ..frame.clone()
        };
        let mut quads = Vec::with_capacity(layers.len());
        let mut plain: Vec<Drawn> = Vec::new();
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
        for &l in layers {
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
        Ok(quads)
    }

    /// A 3D `frame` (the stage's pass into `A`) with the scene's effects,
    /// then its overlay layers flat over that, into `target`; its quads.
    pub(crate) fn draw_3d(
        &mut self,
        canvas: &mut GpuCanvas,
        assets: &Assets,
        frame: &Frame,
        view: &crate::View,
        target: &wgpu::TextureView,
    ) -> Result<Vec<Quad>, String> {
        let overlays: Vec<&Drawn> = frame.layers.iter().filter(|l| l.space.overlay).collect();
        let h = self.begin(
            canvas,
            frame,
            passes(&frame.effects) + 2 + layer_passes(&overlays),
        );
        let a = self.targets.as_ref().expect("prepared").views[A].clone();
        let mut quads = canvas.draw_3d(assets, frame, view, &a)?;
        quads.extend(self.finish(canvas, assets, frame, h, &overlays, target)?);
        Ok(quads)
    }

    /// `frame`'s scene effects over `rgba`, a picture already rendered at
    /// the canvas's size (a Blender frame), then its overlay layers, into
    /// `target`.
    pub(crate) fn plate(
        &mut self,
        canvas: &mut GpuCanvas,
        assets: &Assets,
        frame: &Frame,
        rgba: &[u8],
        target: &wgpu::TextureView,
    ) -> Result<(), String> {
        let [w, h] = canvas.size();
        if rgba.len() != w as usize * h as usize * 4 {
            return Err(format!(
                "a plate of {} bytes is not {w}x{h} RGBA",
                rgba.len()
            ));
        }
        let overlays: Vec<&Drawn> = frame.layers.iter().filter(|l| l.space.overlay).collect();
        let hd = self.begin(
            canvas,
            frame,
            passes(&frame.effects) + 2 + layer_passes(&overlays),
        );
        let a = &self.targets.as_ref().expect("prepared").textures[A];
        canvas.queue.write_texture(
            a.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            a.size(),
        );
        self.finish(canvas, assets, frame, hd, &overlays, target)?;
        Ok(())
    }

    /// Each box of `atlas` through its stack, in place: a 3D frame's layer
    /// with effects, painted with room around it for what they spread. A
    /// box is its corner, size, and pixels per project pixel.
    pub(crate) fn boxes(
        &mut self,
        canvas: &GpuCanvas,
        frame: &Frame,
        atlas: &wgpu::Texture,
        boxes: &[AtlasBox<'_>],
    ) {
        // One size for all (grown, never shrunk), each box at its centre,
        // so a scene of several does not reallocate per box.
        let mut size = self.targets.as_ref().map_or([1, 1], |t| t.size);
        for (_, px, _, _) in boxes {
            size = [size[0].max(px[0]), size[1].max(px[1])];
        }
        let n = boxes.iter().map(|b| passes(b.3)).sum::<u64>() + 1;
        self.prepare(&canvas.device, size, n);
        self.staged.clear();
        self.written = 0;
        let mut enc = encoder(canvas);
        let copy = |enc: &mut wgpu::CommandEncoder,
                    from: &wgpu::Texture,
                    a: [u32; 2],
                    to: &wgpu::Texture,
                    b: [u32; 2],
                    px: [u32; 2]| {
            let (mut src, mut dst) = (from.as_image_copy(), to.as_image_copy());
            src.origin = wgpu::Origin3d {
                x: a[0],
                y: a[1],
                z: 0,
            };
            dst.origin = wgpu::Origin3d {
                x: b[0],
                y: b[1],
                z: 0,
            };
            enc.copy_texture_to_texture(
                src,
                dst,
                wgpu::Extent3d {
                    width: px[0],
                    height: px[1],
                    depth_or_array_layers: 1,
                },
            );
        };
        for &(at, px, k, stack) in boxes {
            let t = self.targets.as_ref().expect("prepared");
            let mid = [(size[0] - px[0]) / 2, (size[1] - px[1]) / 2];
            // `L` clear but for the box: what the stack reads past it is
            // transparent, as round a flat layer.
            drop(enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mui-cut fx clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.views[L],
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
            }));
            copy(&mut enc, atlas, at, &t.textures[L], mid, px);
            let h = Header {
                res: size.map(|v| v as f32),
                time: frame.t as f32,
                scale: k as f32,
                seed: frame.seed,
            };
            let out = self.chain(&mut enc, h, L, stack);
            let t = self.targets.as_ref().expect("prepared");
            copy(&mut enc, &t.textures[out], mid, atlas, at, px);
        }
        self.submit(canvas, enc);
    }

    /// `frame` with its effects into `target`; its quads.
    pub(crate) fn draw(
        &mut self,
        canvas: &mut GpuCanvas,
        assets: &Assets,
        frame: &Frame,
        target: &wgpu::TextureView,
    ) -> Result<Vec<Quad>, String> {
        // A 3D frame drawn flat keeps its overlays over the scene's stack.
        let (top, base): (Vec<&Drawn>, Vec<&Drawn>) = frame
            .layers
            .iter()
            .partition(|l| frame.view.is_some() && l.space.overlay);
        let h = self.begin(
            canvas,
            frame,
            passes(&frame.effects) + 2 + layer_passes(&base) + layer_passes(&top),
        );
        let mut quads = self.composite(canvas, assets, frame, h, &base, true)?;
        quads.extend(self.finish(canvas, assets, frame, h, &top, target)?);
        Ok(quads)
    }
}

/// A box of the atlas to run a stack over: corner, size, pixels per
/// project pixel, and the stack.
pub(crate) type AtlasBox<'a> = ([u32; 2], [u32; 2], f64, &'a [Fx]);

fn encoder(c: &GpuCanvas) -> wgpu::CommandEncoder {
    c.device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default())
}

/// Uniform slots compositing `layers` takes: each one's stack and its
/// composite, and a run's.
fn layer_passes(layers: &[&Drawn]) -> u64 {
    layers.iter().map(|l| passes(&l.effects) + 2).sum::<u64>() + 2
}

/// Full-frame passes `stack` takes.
fn passes(stack: &[Fx]) -> u64 {
    stack
        .iter()
        .filter_map(|f| super::def(&f.kind))
        .map(|(_, d)| u64::from(d.passes))
        .sum()
}

fn uniforms(device: &wgpu::Device, slots: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("mui-cut fx uniforms"),
        size: slots * SLOT,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    /// WebGL2 (no `BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED`) refuses a uniform
    /// whose size is not a multiple of 16: every effect's `U` must be one,
    /// and fit its slot.
    #[test]
    fn every_effect_uniform_suits_webgl2() {
        for def in crate::fx::EFFECTS {
            let wgsl = super::source(def.name).expect("a shader per effect");
            let m =
                naga::front::wgsl::parse_str(wgsl).unwrap_or_else(|e| panic!("{}: {e}", def.name));
            let mut layout = naga::proc::Layouter::default();
            layout.update(m.to_ctx()).unwrap();
            let (_, u) = m
                .global_variables
                .iter()
                .find(|(_, g)| g.binding.as_ref().is_some_and(|b| b.binding == 2))
                .expect("the uniform");
            let size = layout[u.ty].size;
            assert!(
                size % 16 == 0 && u64::from(size) <= super::SLOT,
                "{}: U is {size} bytes",
                def.name
            );
        }
    }
}
