//! Offscreen targets: a group layer's, a blending layer's snapshot of what
//! is under it, and a backdrop's snapshot and blur. `Rgba8Unorm`,
//! premultiplied, sized to their device box in 64-pixel buckets and pooled
//! (after Rune's layer targets: reused while they fit and are at most twice
//! the area asked for). A group layer's target and a backdrop's blurred one
//! are kept for the next frame, keyed by their entry, and drawn again only
//! when what they show changed; a target no frame used is dropped at the
//! frame's end.

use super::layer::{Compose, Kind, Layers, Run, Surface};
use rustc_hash::FxHashMap;

/// `Rgba8Unorm`: renderable and filterable on every WebGL2 device.
pub const FORMAT: mui_hal::TextureFormat = mui_hal::TextureFormat::Rgba8Unorm;

/// The blur shader.
pub const BLUR_SHADER: &str = mui_shaders::BLUR_WGSL;

/// Uniform slots are this far apart, the alignment every device allows for
/// a dynamic offset; each binds [`SLOT_BYTES`] of it.
pub const SLOT: u64 = 256;
pub const SLOT_BYTES: u64 = 32;

pub struct Target {
    texture: mui_hal::Texture,
    pub view: mui_hal::TextureView,
    /// Bind group 1 of the quad pass, sampling this target.
    pub group: mui_hal::BindGroup,
    /// The device rect it was last drawn for.
    rect: [u32; 4],
    used: bool,
}

impl Target {
    fn fits(&self, texels: [u32; 2]) -> bool {
        let [w, h] = [self.texture.width(), self.texture.height()];
        let [bw, bh] = texels.map(|v| v.div_ceil(64) * 64);
        w >= bw && h >= bh && u64::from(w * h) <= 2 * u64::from(bw * bh)
    }
}

/// The uniform slots of one frame, written in one go before it is submitted.
#[derive(Default)]
pub struct Slots {
    pub bytes: Vec<u8>,
}

impl Slots {
    /// A slot holding `values`; its dynamic offset.
    pub fn push(&mut self, values: [f32; 8]) -> u32 {
        let at = self.bytes.len();
        self.bytes.resize(at + SLOT as usize, 0);
        for (out, v) in self.bytes[at..]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(values)
        {
            *out = v.to_ne_bytes();
        }
        at as u32
    }
}

/// What the passes of one frame draw from.
pub struct Draw<'a> {
    pub runs: &'a [Run],
    /// Bind group 1 of each run.
    pub groups: &'a [mui_hal::BindGroup],
    /// Instances in the buffer, past the last run's start.
    pub count: u32,
    pub instances: mui_hal::BufferSlice<'a>,
    /// Bind group 0, its view at a dynamic offset.
    pub view: &'a mui_hal::BindGroup,
    pub uniforms: &'a mui_hal::Buffer,
    /// The profiling query set, until the frame's first pass takes it.
    pub stamp: std::cell::Cell<Option<&'a mui_hal::QuerySet>>,
}

impl Draw<'_> {
    /// The runs `keep` takes, ending at `upto`, into `pass`; draw calls.
    pub fn runs(
        &self,
        pass: &mut mui_hal::RenderPass<'_>,
        over: &mui_hal::RenderPipeline,
        masks: Option<[&mui_hal::RenderPipeline; 2]>,
        upto: u32,
        keep: impl Fn(&Run) -> bool,
    ) -> usize {
        let mut draws = 0;
        let mut compose_bound = None;
        for (k, r) in self.runs.iter().enumerate() {
            let end = self
                .runs
                .get(k + 1)
                .map_or(self.count, |n| n.start)
                .min(upto);
            if end <= r.start || !keep(r) {
                continue;
            }
            if compose_bound != Some(r.compose) {
                let pipeline = match (r.compose, masks) {
                    (Compose::Atop, Some([atop, _])) => atop,
                    (Compose::DestIn, Some([_, inside])) => inside,
                    _ => over,
                };
                pass.set_pipeline(pipeline);
                compose_bound = Some(r.compose);
            }
            pass.set_bind_group(1, &self.groups[k], &[]);
            pass.draw(0..4, r.start..end);
            draws += 1;
        }
        draws
    }
}

pub struct Offscreen {
    over: mui_hal::RenderPipeline,
    atop: mui_hal::RenderPipeline,
    inside: mui_hal::RenderPipeline,
    down: mui_hal::RenderPipeline,
    blur: mui_hal::RenderPipeline,
    blur_layout: mui_hal::BindGroupLayout,
    image_layout: mui_hal::BindGroupLayout,
    /// Bound as the snapshot a target's own group does not blend onto.
    blank: mui_hal::TextureView,
    free: Vec<Target>,
    kept: FxHashMap<(usize, bool), Target>,
    /// This frame's target of each surface, and whether it is drawn.
    pub targets: Vec<Option<Target>>,
    draw: Vec<bool>,
}

impl Offscreen {
    pub fn new(
        device: &mui_hal::Device,
        module: &mui_hal::ShaderModule,
        layout: &mui_hal::PipelineLayout,
        image_layout: &mui_hal::BindGroupLayout,
        blank: mui_hal::TextureView,
        cache: Option<&mui_hal::PipelineCache>,
    ) -> Self {
        let over = super::gpu::pipeline(
            device,
            module,
            layout,
            FORMAT,
            Some(super::gpu::OVER),
            cache,
        );
        // Source-atop, premultiplied: colour `s * da + d * (1 - sa)`, and
        // the destination's alpha kept.
        let atop = super::gpu::pipeline(
            device,
            module,
            layout,
            FORMAT,
            Some(mui_hal::BlendState {
                color: mui_hal::BlendComponent {
                    src_factor: mui_hal::BlendFactor::DstAlpha,
                    dst_factor: mui_hal::BlendFactor::OneMinusSrcAlpha,
                    operation: mui_hal::BlendOperation::Add,
                },
                alpha: mui_hal::BlendComponent {
                    src_factor: mui_hal::BlendFactor::Zero,
                    dst_factor: mui_hal::BlendFactor::One,
                    operation: mui_hal::BlendOperation::Add,
                },
            }),
            cache,
        );
        // Destination-in scales premultiplied colour and alpha equally.
        let component = mui_hal::BlendComponent {
            src_factor: mui_hal::BlendFactor::Zero,
            dst_factor: mui_hal::BlendFactor::SrcAlpha,
            operation: mui_hal::BlendOperation::Add,
        };
        let inside = super::gpu::pipeline(
            device,
            module,
            layout,
            FORMAT,
            Some(mui_hal::BlendState {
                color: component,
                alpha: component,
            }),
            cache,
        );
        let blur_module = device.create_shader_module(mui_hal::ShaderModuleDescriptor {
            label: Some("mui quad blur"),
            source: mui_hal::ShaderSource::Mui(mui_hal::ShaderId::Blur),
        });
        let blur_layout = device.create_bind_group_layout(&mui_hal::BindGroupLayoutDescriptor {
            label: Some("mui quad blur"),
            entries: &[
                mui_hal::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: mui_hal::ShaderStages::FRAGMENT,
                    ty: mui_hal::BindingType::Texture {
                        sample_type: mui_hal::TextureSampleType::Float { filterable: false },
                        view_dimension: mui_hal::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                super::gpu::uniform_entry(1, mui_hal::ShaderStages::FRAGMENT),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&mui_hal::PipelineLayoutDescriptor {
            label: Some("mui quad blur"),
            bind_group_layouts: &[Some(&blur_layout)],
            immediate_size: 0,
        });
        let pass = |entry| {
            device.create_render_pipeline(&mui_hal::RenderPipelineDescriptor {
                label: Some("mui quad blur"),
                layout: Some(&pipeline_layout),
                vertex: mui_hal::VertexState {
                    module: &blur_module,
                    entry_point: Some("vs"),
                    compilation_options: mui_hal::PipelineCompilationOptions,
                    buffers: &[],
                },
                primitive: mui_hal::PrimitiveState::default(),
                depth_stencil: None,
                multisample: mui_hal::MultisampleState,
                fragment: Some(mui_hal::FragmentState {
                    module: &blur_module,
                    entry_point: Some(entry),
                    compilation_options: mui_hal::PipelineCompilationOptions,
                    targets: &[Some(mui_hal::ColorTargetState {
                        format: FORMAT,
                        blend: None,
                        write_mask: mui_hal::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache,
            })
        };
        Self {
            over,
            atop,
            inside,
            down: pass("fs_down"),
            blur: pass("fs_blur"),
            blur_layout,
            image_layout: image_layout.clone(),
            blank,
            free: Vec::new(),
            kept: FxHashMap::default(),
            targets: Vec::new(),
            draw: Vec::new(),
        }
    }

    /// Bytes the targets hold now.
    pub fn bytes(&self) -> u64 {
        self.free
            .iter()
            .chain(self.kept.values())
            .chain(self.targets.iter().flatten())
            .map(|t| u64::from(4 * t.texture.width() * t.texture.height()))
            .sum()
    }

    /// A target of at least `texels`, from the pool or new.
    fn take(&mut self, device: &mui_hal::Device, texels: [u32; 2]) -> Target {
        if let Some(k) = self.free.iter().position(|t| t.fits(texels)) {
            let mut t = self.free.swap_remove(k);
            t.used = true;
            return t;
        }
        let [w, h] = texels.map(|v| v.max(1).div_ceil(64) * 64);
        let texture = device.create_texture(&mui_hal::TextureDescriptor {
            label: Some("mui quad offscreen"),
            size: mui_hal::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: mui_hal::TextureDimension::D2,
            format: FORMAT,
            usage: mui_hal::TextureUsages::RENDER_ATTACHMENT
                | mui_hal::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&mui_hal::TextureViewDescriptor::default());
        let group = super::gpu::image_group(device, &self.image_layout, &view, &self.blank);
        Target {
            texture,
            view,
            group,
            rect: [0; 4],
            used: true,
        }
    }

    /// Give every surface of `layers` its target: a kept one where what it
    /// shows did not change, else one to draw.
    pub fn prepare(&mut self, device: &mui_hal::Device, layers: &Layers) {
        self.targets.clear();
        self.draw.clear();
        let mut kept = std::mem::take(&mut self.kept);
        for s in &layers.surfaces {
            let key = (s.entries[0], matches!(s.kind, Kind::Backdrop { .. }));
            let target = match s.kind {
                Kind::Folded => None,
                _ if self.targets.is_empty() => None,
                Kind::Group | Kind::Backdrop { .. } => match kept.remove(&key) {
                    Some(mut t) if !s.dirty && t.rect == s.rect => {
                        t.used = true;
                        self.draw.push(false);
                        self.targets.push(Some(t));
                        continue;
                    }
                    Some(t) if t.fits(s.texels()) => Some(t),
                    Some(t) => {
                        self.free.push(t);
                        Some(self.take(device, s.texels()))
                    }
                    None => Some(self.take(device, s.texels())),
                },
                Kind::Under => Some(self.take(device, s.texels())),
            };
            self.draw.push(target.is_some());
            self.targets.push(target.map(|mut t| {
                t.rect = s.rect;
                t.used = true;
                t
            }));
        }
    }

    /// The group of surface `id`'s target, or of a blending layer's target
    /// and its snapshot.
    pub fn group(
        &self,
        device: &mui_hal::Device,
        id: u16,
        under: Option<u16>,
    ) -> mui_hal::BindGroup {
        let target = |id: u16| self.targets[usize::from(id)].as_ref().expect("a target");
        match under {
            None => target(id).group.clone(),
            Some(u) => super::gpu::image_group(
                device,
                &self.image_layout,
                &target(id).view,
                &target(u).view,
            ),
        }
    }

    /// Encode every surface's passes that `prepare` left to draw; render
    /// passes encoded, and draw calls.
    pub fn encode(
        &mut self,
        device: &mui_hal::Device,
        encoder: &mut mui_hal::CommandEncoder,
        layers: &Layers,
        draw: &Draw<'_>,
        slots: &mut Slots,
    ) -> (usize, usize) {
        let (mut passes, mut draws) = (0, 0);
        for &id in &layers.passes {
            let id = usize::from(id);
            if !self.draw[id] {
                continue;
            }
            let s = &layers.surfaces[id];
            match s.kind {
                Kind::Group => {
                    let view = &self.targets[id].as_ref().expect("a target").view;
                    let mut pass = quad_pass(encoder, view, s, draw, slots);
                    draws += draw.runs(
                        &mut pass,
                        &self.over,
                        Some([&self.atop, &self.inside]),
                        u32::MAX,
                        |r| usize::from(r.surface) == id,
                    );
                    passes += 1;
                }
                Kind::Under => {
                    let view = &self.targets[id].as_ref().expect("a target").view;
                    let mut pass = quad_pass(encoder, view, s, draw, slots);
                    draws += snapshot(&mut pass, s, draw, &self.over, [&self.atop, &self.inside]);
                    passes += 1;
                }
                Kind::Backdrop { scale, sigma } => {
                    draws += self.backdrop(device, encoder, id, s, (scale, sigma), draw, slots);
                    passes += if scale > 1 { 4 } else { 3 };
                }
                Kind::Folded => {}
            }
        }
        (passes, draws)
    }

    /// A backdrop's snapshot, averaged down `scale` times, then blurred
    /// across and down into its kept target. The Gaussian is narrowed by
    /// what the box average and the composite's bilinear tent already add
    /// (variances `(s^2 - 1) / 12 s^2` and `1 / 6` texel squared), so the
    /// three together have the deviation asked for.
    #[expect(
        clippy::too_many_arguments,
        reason = "one frame's state, passed through"
    )]
    fn backdrop(
        &mut self,
        device: &mui_hal::Device,
        encoder: &mut mui_hal::CommandEncoder,
        id: usize,
        s: &Surface,
        (scale, sigma): (u32, f32),
        draw: &Draw<'_>,
        slots: &mut Slots,
    ) -> usize {
        let full = [s.rect[2], s.rect[3]];
        let low = s.texels();
        let snap = self.take(device, full);
        let draws = {
            let mut pass = quad_pass(encoder, &snap.view, s, draw, slots);
            snapshot(&mut pass, s, draw, &self.over, [&self.atop, &self.inside])
        };
        let k = scale as f32;
        let sigma = sigma / k;
        let extra = if scale > 1 {
            (k * k - 1.) / (12. * k * k) + 1. / 6.
        } else {
            0.
        };
        let sigma = (sigma * sigma - extra).max(0.0625).sqrt();
        let radius = (3. * sigma).ceil();
        let mut source = snap;
        let mut used = full;
        let mut spare = Vec::new();
        if scale > 1 {
            let small = self.take(device, low);
            let at = slots.push([used[0] as f32, used[1] as f32, k, k, 0., 0., 0., 0.]);
            self.blur_pass(device, encoder, &self.down, &source, &small, low, at, draw);
            spare.push(std::mem::replace(&mut source, small));
            used = low;
        }
        let across = self.take(device, low);
        let at = slots.push([
            used[0] as f32,
            used[1] as f32,
            1.,
            0.,
            sigma,
            radius,
            0.,
            0.,
        ]);
        self.blur_pass(device, encoder, &self.blur, &source, &across, low, at, draw);
        let out = self.targets[id].take().expect("a target");
        let at = slots.push([low[0] as f32, low[1] as f32, 0., 1., sigma, radius, 0., 0.]);
        self.blur_pass(device, encoder, &self.blur, &across, &out, low, at, draw);
        self.targets[id] = Some(out);
        self.free.extend(spare.into_iter().chain([source, across]));
        draws
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "one pass's state, passed through"
    )]
    fn blur_pass(
        &self,
        device: &mui_hal::Device,
        encoder: &mut mui_hal::CommandEncoder,
        pipeline: &mui_hal::RenderPipeline,
        source: &Target,
        out: &Target,
        texels: [u32; 2],
        at: u32,
        draw: &Draw<'_>,
    ) {
        let group = device.create_bind_group(&mui_hal::BindGroupDescriptor {
            label: Some("mui quad blur"),
            layout: &self.blur_layout,
            entries: &[
                mui_hal::BindGroupEntry {
                    binding: 0,
                    resource: mui_hal::BindingResource::TextureView(&source.view),
                },
                mui_hal::BindGroupEntry {
                    binding: 1,
                    resource: super::gpu::slot(draw.uniforms),
                },
            ],
        });
        let mut pass = begin(encoder, &out.view, draw);
        pass.set_viewport(0., 0., texels[0] as f32, texels[1] as f32, 0., 1.);
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[at]);
        pass.draw(0..3, 0..1);
    }

    /// End the frame: kept targets back under their keys, the rest pooled,
    /// and whatever this frame did not use dropped.
    pub fn finish(&mut self, layers: &Layers) {
        for (s, t) in layers.surfaces.iter().zip(self.targets.drain(..)) {
            let Some(t) = t else { continue };
            match s.kind {
                Kind::Group | Kind::Backdrop { .. } => {
                    let key = (s.entries[0], matches!(s.kind, Kind::Backdrop { .. }));
                    self.kept.insert(key, t);
                }
                _ => self.free.push(t),
            }
        }
        self.free.retain(|t| t.used);
        for t in self.free.iter_mut().chain(self.kept.values_mut()) {
            t.used = false;
        }
    }
}

/// A render pass on `view`, cleared; the frame's first writes its start.
fn begin<'e>(
    encoder: &'e mut mui_hal::CommandEncoder,
    view: &mui_hal::TextureView,
    draw: &Draw<'_>,
) -> mui_hal::RenderPass<'e> {
    let timestamps = draw
        .stamp
        .take()
        .map(|query_set| mui_hal::RenderPassTimestampWrites {
            query_set,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: None,
        });
    encoder.begin_render_pass(&mui_hal::RenderPassDescriptor {
        label: Some("mui quad offscreen"),
        color_attachments: &[Some(mui_hal::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: mui_hal::Operations {
                load: mui_hal::LoadOp::Clear(mui_hal::Color::TRANSPARENT),
                store: mui_hal::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: timestamps,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

/// A pass of the quad pipeline over `s`'s device rect.
fn quad_pass<'e>(
    encoder: &'e mut mui_hal::CommandEncoder,
    view: &mui_hal::TextureView,
    s: &Surface,
    draw: &Draw<'_>,
    slots: &mut Slots,
) -> mui_hal::RenderPass<'e> {
    let [x, y, w, h] = s.rect.map(|v| v as f32);
    let at = slots.push([w, h, x, y, 0., 0., 0., 0.]);
    let mut pass = begin(encoder, view, draw);
    pass.set_viewport(0., 0., w, h, 0., 1.);
    pass.set_bind_group(0, draw.view, &[at]);
    pass.set_vertex_buffer(0, draw.instances);
    pass
}

/// What `s`'s sources drew before it.
fn snapshot(
    pass: &mut mui_hal::RenderPass<'_>,
    s: &Surface,
    draw: &Draw<'_>,
    over: &mui_hal::RenderPipeline,
    masks: [&mui_hal::RenderPipeline; 2],
) -> usize {
    draw.runs(pass, over, Some(masks), s.upto, |r| {
        s.sources.contains(&r.surface)
    })
}
