//! The pass's textures, bind groups and instance buffer, and what brings
//! them up to date each frame.

use super::path::WIDTH;
use super::{QuadRenderer, STRIDE};

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
    uniform: &wgpu::Buffer,
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
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(
                    &paths.create_view(&wgpu::TextureViewDescriptor::default()),
                ),
            },
        ],
    })
}

pub fn image_group(
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
                &self.uniform,
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

    /// The bind group of each image run, made once per image buffer and
    /// dropped with it.
    pub(super) fn bind_images(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
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
                &self.uniform,
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
