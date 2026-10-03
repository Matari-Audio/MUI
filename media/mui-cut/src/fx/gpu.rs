//! Effects on the GPU: full-frame fragment passes between four textures in
//! the canvas's format. A frame with effects renders its plain layers in
//! runs and each layer with effects alone into `L` (only the renderer writes
//! `L`, so its "already presented" shortcut stays true), runs the layer's
//! stack ping-ponging `P`/`Q`, composites the result over `A`, then runs the
//! scene's stack on `A` and copies it to the target. The uniforms of every
//! pass of a frame sit in one buffer at 256-byte dynamic offsets.
//!
//! `glow`, `light_wrap` and `glass` blur through a pyramid (each level half
//! the last, down and back up, in linear light and dithered) for a wide,
//! soft falloff at any radius; `light_wrap` and `glass` also read `A`, the
//! frame so far: what is under their layer.
//!
//! WGSL modules are plain includes: `prelude.wgsl` (bindings, the `U`
//! header, noise) then the effect's file, which declares its `Params`. Kept
//! off naga_oil/wesl on purpose: `concat!` does it without shipping naga to
//! the browser.
use std::num::NonZeroU64;

use super::{EFFECTS, Fx, LEVELS};
use crate::gpu::GpuCanvas;
use crate::render::Assets;
use crate::{Drawn, Frame, Quad, Rgba};

/// Per-pass uniform slot: the header (32 bytes) and up to 56 floats.
const SLOT: u64 = 256;
/// The pyramid's levels: linear light needs more than 8 bits, or a soft
/// glow's tail comes out in rings. (Rendered to in the browser too, as the
/// shutter's sum is.)
const LINEAR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Bloom's and neon's small-source compensation (`pyramid_up.wgsl`'s
/// `sparse`): a source covering a sixteenth of a level's pixel spreads with
/// about three times the light (and never past its own colour).
const SPARSE: f32 = 0.75;
/// Neon's two tiers, before `intensity`.
const NEON_TIGHT: f32 = 1.2;
const NEON_WIDE: f32 = 1.0;
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
        "glow" => module!("glow.wgsl"),
        "light_wrap" => module!("light_wrap.wgsl"),
        "glass" => module!("glass.wgsl"),
        _ => return None,
    })
}

struct Targets {
    size: [u32; 2],
    textures: [wgpu::Texture; 4],
    views: [wgpu::TextureView; 4],
    /// The pyramid: its levels' sizes, the way down and the way back up
    /// (level 0, half size, comes up into whatever the caller says).
    sizes: Vec<[u32; 2]>,
    down: Vec<wgpu::TextureView>,
    up: Vec<wgpu::TextureView>,
    /// Glass's second pyramid result: its shape blurred over the bevel.
    edge: wgpu::TextureView,
}

pub(crate) struct Passes {
    device: wgpu::Device,
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    effects: Vec<wgpu::RenderPipeline>,
    copy: wgpu::RenderPipeline,
    over: wgpu::RenderPipeline,
    warp: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    /// What an unused input reads: one clear pixel.
    none: wgpu::TextureView,
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
        let input = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mui-cut fx"),
            entries: &[
                input(0),
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
                input(3),
                input(4),
                input(5),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline_as = |name: &str,
                           wgsl: &str,
                           blend: Option<wgpu::BlendState>,
                           format: wgpu::TextureFormat| {
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
        let pipeline = |name: &str, wgsl: &str, blend| pipeline_as(name, wgsl, blend, format);
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
        let warp = pipeline("warp", module!("warp.wgsl"), None);
        let down = pipeline_as("pyramid down", module!("pyramid_down.wgsl"), None, LINEAR);
        let up = pipeline_as("pyramid up", module!("pyramid_up.wgsl"), None, LINEAR);
        let none =
            texture(device, format, [1, 1]).create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mui-cut fx"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let slots = 16;
        Ok(Self {
            device: device.clone(),
            format,
            layout,
            effects,
            copy,
            over,
            warp,
            down,
            up,
            none,
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
        let textures = [0; 4].map(|_| texture(device, self.format, size));
        let views = textures
            .each_ref()
            .map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));
        let mut sizes = Vec::new();
        let mut at = size;
        while sizes.len() < LEVELS as usize && at[0].min(at[1]) > 1 {
            at = at.map(|v| v.div_ceil(2));
            sizes.push(at);
        }
        if sizes.is_empty() {
            sizes.push([1, 1]);
        }
        let level = |s: &[u32; 2]| {
            texture(device, LINEAR, *s).create_view(&wgpu::TextureViewDescriptor::default())
        };
        let down = sizes.iter().map(level).collect();
        let up = sizes.iter().map(level).collect();
        let edge = level(&sizes[0]);
        self.targets = Some(Targets {
            size,
            textures,
            views,
            sizes,
            down,
            up,
            edge,
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

    fn view(&self, i: usize) -> wgpu::TextureView {
        self.targets.as_ref().expect("prepared").views[i].clone()
    }

    /// One full-target pass of `pipeline` into `dst`, reading `src` (up to
    /// four textures: `src`, `aux`, `aux2`, `aux3` in the WGSL).
    fn pass(
        &self,
        enc: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        src: &[&wgpu::TextureView],
        dst: &wgpu::TextureView,
        offset: u32,
        clear: bool,
    ) {
        let input = |i: usize| {
            wgpu::BindingResource::TextureView(src.get(i).copied().unwrap_or(&self.none))
        };
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: input(0),
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
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: input(1),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: input(2),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: input(3),
                },
            ],
        });
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
        pass.set_bind_group(0, &bind, &[offset]);
        pass.draw(0..3, 0..1);
    }

    /// Encode `stack` over texture `src`, a layer's over `backdrop` (what
    /// is under it); the texture holding the result.
    fn chain(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        h: Header,
        src: usize,
        backdrop: Option<usize>,
        stack: &[Fx],
    ) -> usize {
        let mut cur = src;
        for fx in stack {
            let Some((i, mut params)) = fx.pack() else {
                continue;
            };
            let dst = if cur == P { Q } else { P };
            let (from, to) = (self.view(cur), self.view(dst));
            let under = backdrop.map(|b| self.view(b));
            let under = under.as_ref().unwrap_or(&self.none).clone();
            let num = |k: &str| match fx.values.get(k) {
                Some(super::Val::Num(v)) => *v as f32,
                _ => 0.,
            };
            let px = |k: &str| num(k) * h.scale;
            let srcs: Vec<wgpu::TextureView> = match fx.kind.as_str() {
                "glow" => {
                    let mode = EFFECTS[i]
                        .modes
                        .iter()
                        .position(|m| Some(*m) == fx.mode)
                        .unwrap_or(0);
                    // The pyramid's first-level filter: bloom, outer, inner, neon.
                    let filter = [1., 2., 3., 4.][mode];
                    let n = self.levels();
                    let mut w = spread(px("radius"), num("falloff"), n);
                    if mode == 3 {
                        // Neon, two tiers: a tight, hot halo hugging the
                        // tube (a fifth of the radius, falling off fast)
                        // and the wide one, dimmer, out to `radius`.
                        let tight = spread(px("radius") * 0.2, 0.25, n);
                        w.resize(w.len().max(tight.len()), 0.);
                        for (k, w) in w.iter_mut().enumerate() {
                            *w = NEON_WIDE * *w + NEON_TIGHT * tight.get(k).unwrap_or(&0.);
                        }
                    }
                    // Bloom and neon spread a small source as if it were
                    // bigger, so it reads at any radius.
                    let sparse = if mode == 0 || mode == 3 { SPARSE } else { 0. };
                    let up0 = self.targets.as_ref().expect("prepared").up[0].clone();
                    let pre = [
                        &params[..4],
                        &[filter, num("threshold"), num("knee"), num("tint")],
                    ]
                    .concat();
                    self.pyramid(enc, h, [&from, &self.none.clone()], &pre, &w, sparse, &up0);
                    params.push(mode as f32);
                    vec![from, up0]
                }
                "light_wrap" => {
                    let w = focus(px("radius"), self.levels());
                    let up0 = self.targets.as_ref().expect("prepared").up[0].clone();
                    self.pyramid(enc, h, [&from, &under], &[0., 0., 0., 0., 5.], &w, 0., &up0);
                    vec![from, up0]
                }
                "glass" => {
                    let t = self.targets.as_ref().expect("prepared");
                    let (up0, edge) = (t.up[0].clone(), t.edge.clone());
                    let n = self.levels();
                    let none = self.none.clone();
                    self.pyramid(
                        enc,
                        h,
                        [&from, &none],
                        &[0., 0., 0., 0., 6.],
                        &focus(px("bevel"), n),
                        0.,
                        &edge,
                    );
                    self.pyramid(
                        enc,
                        h,
                        [&under, &none],
                        &[0., 0., 0., 0., 7.],
                        &focus(px("frost"), n),
                        0.,
                        &up0,
                    );
                    vec![from, under, up0, edge]
                }
                _ => {
                    for pass in 0..EFFECTS[i].passes {
                        let dst = if cur == P { Q } else { P };
                        let offset = self.stage(h, pass, &params);
                        let (from, to) = (self.view(cur), self.view(dst));
                        self.pass(enc, &self.effects[i], &[&from], &to, offset, true);
                        cur = dst;
                    }
                    continue;
                }
            };
            let offset = self.stage(h, 0, &params);
            let srcs: Vec<&wgpu::TextureView> = srcs.iter().collect();
            self.pass(enc, &self.effects[i], &srcs, &to, offset, true);
            cur = dst;
        }
        cur
    }

    /// Pyramid levels at the targets' size.
    fn levels(&self) -> usize {
        self.targets.as_ref().expect("prepared").sizes.len()
    }

    /// Blur `src` (`[picture, aux]`) through the pyramid, its first level
    /// filtered by `pre` (`pyramid_down.wgsl`'s `Params`), the levels summed
    /// by `weights` (each level's light over its coverage to the power
    /// `sparse`), into `out` (level 0's size).
    #[expect(
        clippy::too_many_arguments,
        reason = "a pyramid takes this many inputs"
    )]
    fn pyramid(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        h: Header,
        src: [&wgpu::TextureView; 2],
        pre: &[f32],
        weights: &[f32],
        sparse: f32,
        out: &wgpu::TextureView,
    ) {
        let t = self.targets.as_ref().expect("prepared");
        let (sizes, down, up) = (t.sizes.clone(), t.down.clone(), t.up.clone());
        let n = weights.len().clamp(1, sizes.len());
        let at = |k: usize| Header {
            res: sizes[k].map(|v| v as f32),
            ..h
        };
        for k in 0..n {
            let params = if k == 0 { pre } else { &[0.; 5] };
            let offset = self.stage(at(k), 0, params);
            let from = if k == 0 {
                src
            } else {
                [&down[k - 1], &self.none]
            };
            self.pass(enc, &self.down, &from, &down[k], offset, true);
        }
        for k in (0..n).rev() {
            let coarser = k + 1 < n;
            let offset = self.stage(
                at(k),
                0,
                &[weights[k], f32::from(u8::from(coarser)), sparse],
            );
            let from = if coarser { &up[k + 1] } else { &self.none };
            let to = if k == 0 { out } else { &up[k] };
            self.pass(enc, &self.up, &[from, &down[k]], to, offset, true);
        }
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
        let mut out = self.chain(&mut enc, h, A, None, &frame.effects);
        let mut quads = Vec::new();
        if !overlays.is_empty() {
            // Back into `A`: an overlay's own stack ping-pongs `P` and `Q`.
            if out != A {
                let offset = self.stage(h, 0, &[]);
                let a = self.targets.as_ref().expect("prepared").views[A].clone();
                self.pass(&mut enc, &self.copy, &[&self.view(out)], &a, offset, true);
                out = A;
            }
            self.submit(canvas, enc);
            quads = self.composite(canvas, assets, frame, h, overlays, false)?;
            enc = encoder(canvas);
        }
        let offset = self.stage(h, 0, &[]);
        self.pass(
            &mut enc,
            &self.copy,
            &[&self.view(out)],
            target,
            offset,
            true,
        );
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
            // Not the first: it goes over `A`, which its backdrops see.
            let beneath = (!*first).then_some(&av);
            let q = canvas.paint(assets, &part(bg, std::mem::take(plain)), beneath, &lv)?;
            let mut enc = encoder(canvas);
            let offset = this.stage(h, 0, &[]);
            this.pass(&mut enc, &this.over, &[&lv], &av, offset, *first);
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
            quads.extend(canvas.paint(assets, &part(clear, vec![l.clone()]), Some(&av), &lv)?);
            let mut enc = encoder(canvas);
            let out = self.chain(&mut enc, h, L, Some(A), &l.effects);
            let offset = self.stage(h, 0, &[]);
            self.pass(&mut enc, &self.over, &[&self.view(out)], &av, offset, false);
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
    /// box is its corner, size, pixels per project pixel, its stack, and
    /// for a backdrop effect the frame behind it with the map from the
    /// box's pixels to that frame's uv (see `warp.wgsl`).
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
        for (_, px, k, stack, behind) in boxes {
            // A backdrop effect reads the frame behind past the box's edges.
            let room = if behind.is_some() {
                2 * ((super::room(stack) * k).ceil() as u32).min(1024)
            } else {
                0
            };
            size = [size[0].max(px[0] + room), size[1].max(px[1] + room)];
        }
        let n = boxes.iter().map(|b| passes(b.3) + 1).sum::<u64>() + 1;
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
        for &(at, px, k, stack, behind) in boxes {
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
            let mut backdrop = None;
            if let Some((frame_behind, m)) = behind {
                // The map is from the box's pixels: from `L`'s, where the
                // box sits at `mid`.
                let [mx, my] = mid.map(|v| v as f32);
                let row = |r: usize| {
                    let [a, b, c] = [m[3 * r], m[3 * r + 1], m[3 * r + 2]];
                    [a, b, c - a * mx - b * my, 0.]
                };
                let params = [row(0), row(1), row(2)].concat();
                let offset = self.stage(h, 0, &params);
                let a = self.view(A);
                self.pass(&mut enc, &self.warp, &[frame_behind], &a, offset, true);
                backdrop = Some(A);
            }
            let out = self.chain(&mut enc, h, L, backdrop, stack);
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
/// project pixel, the stack, and what is behind it (for a backdrop
/// effect): a frame and the 3x3 map, rows first, from a box pixel to that
/// frame's uv, homogeneous.
pub(crate) type AtlasBox<'a> = (
    [u32; 2],
    [u32; 2],
    f64,
    &'a [Fx],
    Option<(&'a wgpu::TextureView, [f32; 9])>,
);

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

/// Pyramid weights, summing to 1, for a glow `r` output pixels wide. Level
/// `k` (`2^(k+1)` pixels across) fades in as `r` passes it, so a keyed
/// radius grows smoothly; `falloff` 0.5 weighs every level alike (the long,
/// soft tail of light scattering), more favours the wide ones, less the
/// near. Below a pixel the whole glow fades out.
fn spread(r: f32, falloff: f32, max: usize) -> Vec<f32> {
    let top = r.max(1.).log2();
    let n = (top.ceil() as usize).clamp(1, max.max(1));
    let ratio = 4f32.powf(falloff - 0.5);
    let w: Vec<f32> = (0..n)
        .map(|k| {
            let fade = if k == 0 {
                1.
            } else {
                (top - k as f32).clamp(0., 1.)
            };
            ratio.powi(k as i32) * fade
        })
        .collect();
    let sum: f32 = w.iter().sum();
    w.iter().map(|v| v / sum * r.clamp(0., 1.)).collect()
}

/// Pyramid weights for a blur about `r` output pixels wide: the two levels
/// either side of it, so it keys smoothly.
fn focus(r: f32, max: usize) -> Vec<f32> {
    let c = (r.max(1.).log2() - 1.).clamp(0., max.max(1) as f32 - 1.);
    let n = (c.floor() as usize + 2).min(max.max(1));
    (0..n)
        .map(|k| (1. - (k as f32 - c).abs()).max(0.))
        .collect()
}

fn texture(device: &wgpu::Device, format: wgpu::TextureFormat, size: [u32; 2]) -> wgpu::Texture {
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
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
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
        let effects = crate::fx::EFFECTS
            .iter()
            .map(|d| (d.name, super::source(d.name).expect("a shader per effect")));
        let more = [
            ("copy", module!("copy.wgsl")),
            ("warp", module!("warp.wgsl")),
            ("pyramid down", module!("pyramid_down.wgsl")),
            ("pyramid up", module!("pyramid_up.wgsl")),
        ];
        for (name, wgsl) in effects.chain(more) {
            let m = naga::front::wgsl::parse_str(wgsl).unwrap_or_else(|e| panic!("{name}: {e}"));
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
                "{name}: U is {size} bytes"
            );
        }
    }
}
