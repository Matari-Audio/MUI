//! The instanced SDF pass: every ordinary UI primitive as an instance of one
//! quad pipeline, antialiased in the fragment shader, in one draw call. No
//! compute shaders and no storage buffers, so it runs on a GLES 3 / WebGL2
//! class device.
//!
//! It draws rounded rects (filled or stroked, solid, gradient or image),
//! analytic drop and inset shadows, rect clips and glyph runs from a
//! coverage atlas, under any affine transform for shapes and translation
//! plus uniform scale for text. Anything else in a paint list is reported by
//! [`classify`] and refused by [`QuadRenderer::render`]; it is never skipped
//! or approximated.
//!
//! Bindings: group 0 is the view, the glyph atlas (an `R8Unorm` array, one
//! layer a page) and a linear clamping sampler; group 1 is one image. An
//! image instance needs its image bound, so a run of instances is drawn in
//! one call until an image instance wants a different image than the run
//! already holds: a list with no images, or one image, is one draw call.

mod build;
#[cfg(test)]
mod tests;
mod text;

use crate::Cache;
use crate::kurbo::Affine;
use build::{Drawn, Instance, SLOTS, Walk};
use mui_scene::Painted;
use std::sync::Arc;

pub use build::{MAX_GLYPH_PX, MAX_STOPS};

/// The WGSL of the pass, for validation and tooling.
pub const SHADER: &str = include_str!("quad.wgsl");

/// Why the pass cannot draw an entry yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reason {
    /// A glyph run with a bitmap or colour glyph in it (found when drawn,
    /// not by [`classify`]).
    Text,
    /// A glyph run under a rotation, shear or non-uniform scale.
    TextTransform,
    /// A glyph run larger than [`MAX_GLYPH_PX`] device pixels per em.
    TextSize,
    /// More glyphs in one frame than the atlas budget holds, even emptied
    /// (found when drawn).
    GlyphBudget,
    /// An image paint on a glyph run.
    Image,
    /// An image larger than the device's largest texture (found when
    /// drawn).
    ImageSize,
    /// A host-registered GPU texture: the pass keeps no texture registry.
    ImageTexture,
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
    pub const ALL: [Self; 17] = [
        Self::Text,
        Self::TextTransform,
        Self::TextSize,
        Self::GlyphBudget,
        Self::Image,
        Self::ImageSize,
        Self::ImageTexture,
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
                write!(
                    f,
                    "paint entry {index} is unsupported by the quad pass: {reason}"
                )
            }
            Self::Target => f.write_str("quad pass target is empty or its transform is not finite"),
        }
    }
}

impl std::error::Error for Error {}

/// What the last [`QuadRenderer::render`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Frame {
    /// Whether `render` encoded a render pass at all.
    pub drawn: bool,
    /// Instances drawn.
    pub instances: usize,
    /// Draw calls: one, plus one each time an image instance needs a
    /// different image than its run holds.
    pub draws: usize,
    /// Glyph cells rasterised this frame.
    pub rasterised: u64,
    /// Bytes written to the atlas this frame, cells and clears.
    pub uploaded: usize,
    /// Bytes the glyph atlas texture holds.
    pub atlas_bytes: u64,
}

/// The default glyph atlas budget: four 1024-pixel pages.
pub const ATLAS_BUDGET: usize = 4 << 20;

const STRIDE: u64 = (SLOTS * 16) as u64;

const ATTRIBUTES: [wgpu::VertexAttribute; SLOTS] = wgpu::vertex_attr_array![
    0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4,
    4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4,
    8 => Float32x4, 9 => Float32x4, 10 => Float32x4, 11 => Float32x4,
    12 => Float32x4, 13 => Float32x4,
];

/// The pass: one pipeline, one instance buffer that grows and is then
/// reused, a glyph atlas texture and one texture per image.
pub struct QuadRenderer {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    view_layout: wgpu::BindGroupLayout,
    image_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bind: wgpu::BindGroup,
    /// Group 1 for a run that samples no image.
    no_image: wgpu::BindGroup,
    atlas: wgpu::Texture,
    instances: wgpu::Buffer,
    cpu: Instances,
    groups: Vec<wgpu::BindGroup>,
    frame: Frame,
    /// A profiling hook: when set, the render pass writes its start and end
    /// to queries 0 and 1 of this set (the device needs `TIMESTAMP_QUERY`).
    pub timestamps: Option<wgpu::QuerySet>,
}

/// The CPU side of a frame: the walk, the instance bytes it writes and the
/// glyph atlas, all kept, so a steady frame allocates nothing.
struct Instances {
    bytes: Vec<u8>,
    walk: Walk,
    atlas: text::Atlas,
    /// Fonts, and each image's bind group, keyed on the app's buffer.
    cache: Cache,
    /// Where each draw call starts, and the image it binds.
    runs: Vec<(u32, Option<mui_scene::Image>)>,
}

impl Instances {
    fn new(side: u16, pages: usize) -> Self {
        Self {
            bytes: Vec::new(),
            walk: Walk::default(),
            atlas: text::Atlas::new(side, pages),
            cache: Cache::default(),
            runs: Vec::new(),
        }
    }

    /// The instance bytes of `paint`, into `self.bytes`; their count. A
    /// frame the atlas cannot hold empties the atlas and tries once more.
    fn build(&mut self, paint: &[Painted], transform: Affine) -> Result<usize, Error> {
        self.cache.tick();
        self.atlas.begin();
        self.atlas.flushed = false;
        match self.list(paint, transform) {
            Err(Error::Unsupported {
                reason: Reason::GlyphBudget,
                ..
            }) => {
                self.atlas.flush();
                self.list(paint, transform)
            }
            done => done,
        }
    }

    fn list(&mut self, paint: &[Painted], transform: Affine) -> Result<usize, Error> {
        self.bytes.clear();
        self.runs.clear();
        self.runs.push((0, None));
        self.walk.begin();
        let mut one: Instance = [[0.; 4]; SLOTS];
        let mut count = 0;
        for (index, p) in paint.iter().enumerate() {
            let refuse = |reason| Error::Unsupported { index, reason };
            match self.walk.entry(p, transform, &mut one).map_err(refuse)? {
                Drawn::Nothing => continue,
                Drawn::One => {}
                Drawn::Image(image) => match self.runs.last_mut() {
                    Some((_, held @ None)) => *held = Some(image.clone()),
                    Some((_, Some(held))) if Arc::ptr_eq(&held.rgba, &image.rgba) => {}
                    _ => self.runs.push((count as u32, Some(image.clone()))),
                },
                Drawn::Text(text) => {
                    let xf = crate::placed(transform, p);
                    count += self
                        .atlas
                        .draw(&mut self.cache, text, xf, &one, &mut self.bytes)
                        .map_err(|f| {
                            refuse(match f {
                                text::Failure::Colour => Reason::Text,
                                text::Failure::Full => Reason::GlyphBudget,
                            })
                        })?;
                    continue;
                }
            }
            count += 1;
            for v in one.as_flattened() {
                self.bytes.extend_from_slice(&v.to_ne_bytes());
            }
        }
        Ok(count)
    }
}

impl QuadRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self::with_atlas_budget(device, format, ATLAS_BUDGET)
    }

    /// A pass whose glyph atlas holds at most `budget` bytes: square
    /// single-channel pages of up to 1024 pixels, as many as fit, at least
    /// one. Glyphs unused for 64 frames are evicted; a frame that needs more
    /// than fits empties the atlas and rasterises again.
    pub fn with_atlas_budget(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        budget: usize,
    ) -> Self {
        let limits = device.limits();
        let mut side = 1024.min(limits.max_texture_dimension_2d);
        while side > 64 && (side * side) as usize > budget {
            side /= 2;
        }
        let pages =
            (budget / (side * side) as usize).clamp(1, limits.max_texture_array_layers as usize);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mui quad"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let texture = |binding, view_dimension| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension,
                multisampled: false,
            },
            count: None,
        };
        let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mui quad view"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture(1, wgpu::TextureViewDimension::D2Array),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mui quad image"),
            entries: &[texture(0, wgpu::TextureViewDimension::D2)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mui quad"),
            bind_group_layouts: &[Some(&view_layout), Some(&image_layout)],
            immediate_size: 0,
        });
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mui quad image"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let atlas = atlas_texture(device, u32::from(side as u16), 1);
        let bind = view_group(device, &view_layout, &uniform, &atlas, &sampler);
        let no_image = image_group(device, &image_layout, &image_texture(device, 1, 1));
        let mut cpu = Instances::new(side as u16, pages);
        cpu.walk.max_image = limits.max_texture_dimension_2d;
        Self {
            pipeline,
            uniform,
            view_layout,
            image_layout,
            sampler,
            bind,
            no_image,
            atlas,
            instances: instance_buffer(device, 256),
            cpu,
            groups: Vec::new(),
            frame: Frame::default(),
            timestamps: None,
        }
    }

    /// What the last frame drew, rasterised and uploaded.
    pub fn frame(&self) -> Frame {
        self.frame
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
        let count = self.cpu.build(paint, transform)?;
        self.upload_glyphs(device, queue);
        self.bind_images(device, queue);
        if self.instances.size() < self.cpu.bytes.len() as u64 {
            let want = (count as u64).next_power_of_two();
            self.instances = instance_buffer(device, want);
        }
        if !self.cpu.bytes.is_empty() {
            queue.write_buffer(&self.instances, 0, &self.cpu.bytes);
        }
        let view = [size[0] as f32, size[1] as f32, 0., 0.];
        queue.write_buffer(&self.uniform, 0, view.map(f32::to_ne_bytes).as_flattened());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("mui quad"),
        });
        let mut draws = 0;
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
                timestamp_writes: self.timestamps.as_ref().map(|query_set| {
                    wgpu::RenderPassTimestampWrites {
                        query_set,
                        beginning_of_pass_write_index: Some(0),
                        end_of_pass_write_index: Some(1),
                    }
                }),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if count > 0 {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind, &[]);
                pass.set_vertex_buffer(0, self.instances.slice(..self.cpu.bytes.len() as u64));
                let mut groups = self.groups.iter();
                let runs = &self.cpu.runs;
                for (i, (start, image)) in runs.iter().enumerate() {
                    let end = runs.get(i + 1).map_or(count as u32, |r| r.0);
                    let group = match image {
                        Some(_) => groups.next().expect("a group per image run"),
                        None => &self.no_image,
                    };
                    if end > *start {
                        pass.set_bind_group(1, group, &[]);
                        pass.draw(0..4, *start..end);
                        draws += 1;
                    }
                }
            }
        }
        queue.submit([encoder.finish()]);
        self.frame.drawn = true;
        self.frame.instances = count;
        self.frame.draws = draws;
        Ok(())
    }

    /// Bring the atlas texture up to date with what the frame rasterised:
    /// grown by a page, zeroed after a flush, and each new or evicted cell
    /// written.
    fn upload_glyphs(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let atlas = &mut self.cpu.atlas;
        let before = atlas.rasterised;
        atlas.rasterise();
        let side = u32::from(atlas.side());
        let layers = atlas_layers(atlas.pages());
        let grow = layers > self.atlas.depth_or_array_layers();
        if atlas.flushed || grow {
            let next = atlas_texture(device, side, layers);
            if !atlas.flushed {
                // The old pages, copied before this frame's cells land.
                let mut encoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
                encoder.copy_texture_to_texture(
                    self.atlas.as_image_copy(),
                    next.as_image_copy(),
                    self.atlas.size(),
                );
                queue.submit([encoder.finish()]);
            }
            self.atlas = next;
            self.bind = view_group(
                device,
                &self.view_layout,
                &self.uniform,
                &self.atlas,
                &self.sampler,
            );
        }
        let mut uploaded = 0;
        for u in &atlas.uploads {
            let [x, y, w, h] = u.rect;
            let len = (w * h) as usize;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: u.page },
                    aspect: wgpu::TextureAspect::All,
                },
                &atlas.bytes[u.at..u.at + len],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
            uploaded += len;
        }
        self.frame.rasterised = atlas.rasterised - before;
        self.frame.uploaded = uploaded;
        self.frame.atlas_bytes = u64::from(side * side * self.atlas.depth_or_array_layers());
    }

    /// The bind group of each image run, made once per image buffer and
    /// dropped with it.
    fn bind_images(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.groups.clear();
        let cache = &mut self.cpu.cache;
        cache.sweep();
        for image in self.cpu.runs.iter().filter_map(|r| r.1.as_ref()) {
            if let Some(crate::Stored::Texture(group)) = cache.find(&image.rgba) {
                self.groups.push(group.clone());
                continue;
            }
            // The walk only hands over images `premultiply` takes.
            let Some(pixels) = crate::premultiply(image) else {
                unreachable!("the walk refuses an image it cannot hold");
            };
            let texture = image_texture(device, image.width, image.height);
            queue.write_texture(
                texture.as_image_copy(),
                pixels.data_as_u8_slice(),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * image.width),
                    rows_per_image: Some(image.height),
                },
                texture.size(),
            );
            let group = image_group(device, &self.image_layout, &texture);
            cache.remember(&image.rgba, crate::Stored::Texture(group.clone()));
            self.groups.push(group);
        }
    }
}

/// Array layers for `pages` atlas pages. At least two: a one-layer array
/// can be taken for a plain 2D texture on GL.
fn atlas_layers(pages: usize) -> u32 {
    pages.max(2) as u32
}

fn atlas_texture(device: &wgpu::Device, side: u32, layers: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mui quad glyphs"),
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: atlas_layers(layers as usize),
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn image_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mui quad image"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn view_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    atlas: &wgpu::Texture,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    let view = atlas.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mui quad view"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn image_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture: &wgpu::Texture,
) -> wgpu::BindGroup {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mui quad image"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&view),
        }],
    })
}

fn instance_buffer(device: &wgpu::Device, count: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("mui quad instances"),
        size: count.max(1) * STRIDE,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
