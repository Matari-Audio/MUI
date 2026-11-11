//! The pass's textures, bind groups and instance buffer, and what brings
//! them up to date each frame.

use super::path::WIDTH;
use super::{QuadRenderer, STRIDE};
use std::num::NonZeroU64;

/// Premultiplied source-over, on encoded values, as `vello_cpu` blends.
pub const OVER: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

const ATTRIBUTES: [wgpu::VertexAttribute; super::build::SLOTS] = wgpu::vertex_attr_array![
    0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x4,
    4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4,
    8 => Float32x4, 9 => Float32x4, 10 => Float32x4, 11 => Float32x4,
    12 => Float32x4, 13 => Float32x4, 14 => Float32x4,
];

/// A uniform bound at a dynamic offset, [`super::offscreen::SLOT_BYTES`] of it.
pub fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: true,
            min_binding_size: NonZeroU64::new(super::offscreen::SLOT_BYTES),
        },
        count: None,
    }
}

/// One slot of `uniforms`, its offset given when the group is set.
pub fn slot(uniforms: &wgpu::Buffer) -> wgpu::BindingResource<'_> {
    wgpu::BindingResource::Buffer(wgpu::BufferBinding {
        buffer: uniforms,
        offset: 0,
        size: NonZeroU64::new(super::offscreen::SLOT_BYTES),
    })
}

pub fn uniforms(device: &wgpu::Device, slots: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("mui quad uniforms"),
        size: slots.max(1) * super::offscreen::SLOT,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// The bind group layouts of the pass: the view's (group 0) and a run's
/// texture and the snapshot it may blend onto (group 1).
pub fn layouts(device: &wgpu::Device) -> (wgpu::BindGroupLayout, wgpu::BindGroupLayout) {
    let texture = |binding, view_dimension, filterable| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension,
            multisampled: false,
        },
        count: None,
    };
    let view = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("mui quad view"),
        entries: &[
            uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
            texture(1, wgpu::TextureViewDimension::D2Array, true),
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            texture(3, wgpu::TextureViewDimension::D2, false),
        ],
    });
    let image = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("mui quad image"),
        entries: &[
            texture(0, wgpu::TextureViewDimension::D2, true),
            texture(1, wgpu::TextureViewDimension::D2, false),
        ],
    });
    (view, image)
}

/// The quad pipeline into `format`, blending with `blend` (`None`: the
/// unblended clear).
pub fn pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("mui quad"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
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
            module,
            entry_point: Some("fs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Array layers for `pages` atlas pages. At least two: a one-layer array
/// can be taken for a plain 2D texture on GL.
pub fn atlas_layers(pages: usize) -> u32 {
    pages.max(2) as u32
}

pub fn atlas_texture(device: &wgpu::Device, side: u32, layers: u32) -> wgpu::Texture {
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

pub fn image_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
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

/// The path records: `rows` rows of [`WIDTH`] texels.
pub fn path_texture(device: &wgpu::Device, rows: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mui quad paths"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: rows.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

pub fn view_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    atlas: &wgpu::Texture,
    paths: &wgpu::Texture,
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
                resource: slot(uniforms),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(
                    &paths.create_view(&wgpu::TextureViewDescriptor::default()),
                ),
            },
        ],
    })
}

/// Group 1: `view` sampled, over `under`.
pub fn image_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    under: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("mui quad image"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(under),
            },
        ],
    })
}

pub fn instance_buffer(device: &wgpu::Device, count: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("mui quad instances"),
        size: count.max(1) * STRIDE,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl QuadRenderer {
    /// Bring the atlas texture up to date with what the frame rasterised:
    /// grown by a page, zeroed after a flush, and each new or evicted cell
    /// written.
    pub(super) fn upload_glyphs(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
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
                &self.uniforms,
                &self.atlas,
                &self.paths,
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

    /// Bind group 1 of every run: an image's, made once per image buffer
    /// and dropped with it; a host texture's or a weld's, made once per
    /// texture; an offscreen target's.
    pub(super) fn bind_runs(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        use super::layer::Bind;
        self.groups.clear();
        let cache = &mut self.cpu.cache;
        cache.sweep();
        let blank = &self.blank;
        let layout = &self.image_layout;
        let offscreen = self.offscreen.as_ref();
        let mut seen = Vec::new();
        for run in &self.cpu.layers.runs {
            let group = match &run.bind {
                Bind::None => self.no_image.clone(),
                Bind::Image(image) => {
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
                    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                    let group = image_group(device, layout, &view, blank);
                    cache.remember(&image.rgba, crate::Stored::Texture(group.clone()));
                    group
                }
                Bind::Host(key) => {
                    let host = self
                        .hosts
                        .get_mut(key)
                        .expect("the walk names a registered key");
                    host.group
                        .get_or_insert_with(|| image_group(device, layout, &host.view, blank))
                        .clone()
                }
                Bind::Weld(id, texture) => {
                    seen.push(*id);
                    self.welds
                        .entry(*id)
                        .or_insert_with(|| {
                            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                            image_group(device, layout, &view, blank)
                        })
                        .clone()
                }
                Bind::Surface(id) => offscreen.expect("targets").group(device, *id, None),
                Bind::Blend(id, under) => {
                    offscreen.expect("targets").group(device, *id, Some(*under))
                }
            };
            self.groups.push(group);
        }
        // A weld's group lives while a frame draws it.
        self.welds.retain(|id, _| seen.contains(id));
    }

    /// Bring the path texture up to date with the records the frame built:
    /// grown, and every new record written.
    pub(super) fn upload_paths(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let store = &mut self.cpu.walk.paths;
        let mut uploaded = 0;
        if store.rows() > self.paths.height() {
            // Every record again, into a texture with room for them all.
            self.paths = path_texture(device, store.rows());
            store.dirty.clear();
            store.dirty.push((0, store.texels.len() as u32));
            self.bind = view_group(
                device,
                &self.view_layout,
                &self.uniforms,
                &self.atlas,
                &self.paths,
                &self.sampler,
            );
        }
        let store = &mut self.cpu.walk.paths;
        for (at, len) in store.dirty.drain(..) {
            // A range, cut at row ends.
            let (mut at, end) = (at, at + len);
            while at < end {
                let x = at % WIDTH;
                let n = (WIDTH - x).min(end - at);
                let rows = if x == 0 { (end - at) / WIDTH } else { 0 };
                let (w, h) = if rows > 1 { (WIDTH, rows) } else { (n, 1) };
                let texels = &store.texels[at as usize..(at + w * h) as usize];
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.paths,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x,
                            y: at / WIDTH,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &texels
                        .as_flattened()
                        .iter()
                        .flat_map(|v| v.to_ne_bytes())
                        .collect::<Vec<u8>>(),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(16 * w),
                        rows_per_image: Some(h),
                    },
                    wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                );
                uploaded += 16 * (w * h) as usize;
                at += w * h;
            }
        }
        self.frame.paths_built = store.built;
        self.frame.path_uploaded = uploaded;
        self.frame.path_bytes = 16 * u64::from(WIDTH * self.paths.height());
    }
}
