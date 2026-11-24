//! Commands as data, and their translation into a Vulkan command buffer.
//!
//! Every image but a swapchain's sits in `GENERAL`. A global memory barrier
//! goes between commands; inside a pass nothing reads what the pass writes,
//! so none is needed there. `ponytail:` coarse sync, tracked per-resource
//! barriers when a profile shows the stalls.

use ash::vk;

use super::{BindGroup, Buffer, Error, QuerySet, RenderPipeline, Texture, TextureView};
use crate::types::{Extent3d, Origin3d};

pub enum Cmd {
    /// A new texture: `UNDEFINED` into `GENERAL`.
    Init(Texture),
    /// A swapchain image acquired: `UNDEFINED` into `GENERAL`.
    Acquire(Texture),
    /// A swapchain image rendered: `GENERAL` into `PRESENT_SRC`.
    Release(Texture),
    CopyBuffer {
        src: Buffer,
        src_offset: u64,
        dst: Buffer,
        dst_offset: u64,
        size: u64,
    },
    /// Tightly packed rows of `size` from `src` at `offset`.
    BufferToTexture {
        src: Buffer,
        offset: u64,
        dst: Texture,
        origin: Origin3d,
        size: Extent3d,
    },
    TextureToBuffer {
        src: Texture,
        origin: Origin3d,
        dst: Buffer,
        offset: u64,
        /// Texels per row in the buffer.
        row_length: u32,
        /// Rows per array layer in the buffer.
        image_height: u32,
        size: Extent3d,
    },
    TextureToTexture {
        src: Texture,
        src_origin: Origin3d,
        dst: Texture,
        dst_origin: Origin3d,
        size: Extent3d,
    },
    Pass(PassRecord),
    Timestamp(QuerySet, u32),
    Resolve {
        set: QuerySet,
        first: u32,
        count: u32,
        dst: Buffer,
        offset: u64,
    },
}

pub struct PassRecord {
    pub target: TextureView,
    /// `None`: keep the target's contents.
    pub clear: Option<[f32; 4]>,
    /// Queries written at the pass's start and end.
    pub timestamps: Option<(QuerySet, Option<u32>, Option<u32>)>,
    pub cmds: Vec<PassCmd>,
}

pub enum PassCmd {
    Pipeline(RenderPipeline),
    Group(u32, BindGroup, Vec<u32>),
    Vertex(u32, Buffer, u64),
    /// x, y, w, h, min depth, max depth.
    Viewport([f32; 6]),
    Scissor([u32; 4]),
    Draw {
        vertices: std::ops::Range<u32>,
        instances: std::ops::Range<u32>,
    },
}

fn check_texture(t: &Texture) -> Result<(), Error> {
    if t.is_retired_swapchain() {
        return Err(Error::validation(
            "cannot submit commands targeting a retired swapchain image",
        ));
    }
    if !t.is_valid() {
        return Err(Error::validation(
            "cannot submit commands referencing an invalid texture",
        ));
    }
    Ok(())
}

fn check_view(v: &TextureView) -> Result<(), Error> {
    if v.is_retired_swapchain() {
        return Err(Error::validation(
            "cannot submit commands targeting a retired swapchain image",
        ));
    }
    if !v.is_valid() {
        return Err(Error::validation(
            "cannot submit commands referencing an invalid texture view",
        ));
    }
    Ok(())
}

fn check_buffer_write(dst: &Buffer) -> Result<(), Error> {
    if !dst.is_valid() {
        return Err(Error::validation(
            "cannot submit commands referencing an invalid buffer",
        ));
    }
    if dst.is_mapped_or_pending() {
        return Err(Error::validation(
            "cannot write to a buffer while it is mapped",
        ));
    }
    Ok(())
}

/// Validates every resource a command references before it reaches Vulkan.
pub(super) fn validate_cmd(cmd: &Cmd) -> Result<(), Error> {
    match cmd {
        Cmd::Init(t) | Cmd::Acquire(t) | Cmd::Release(t) => check_texture(t)?,
        Cmd::CopyBuffer {
            src,
            src_offset,
            dst,
            dst_offset,
            size,
        } => {
            if !src.is_valid() {
                return Err(Error::validation(
                    "cannot submit commands referencing an invalid buffer",
                ));
            }
            check_buffer_write(dst)?;
            if src_offset
                .checked_add(*size)
                .is_none_or(|end| end > src.size())
                || dst_offset
                    .checked_add(*size)
                    .is_none_or(|end| end > dst.size())
            {
                return Err(Error::validation("copy range exceeds buffer size"));
            }
        }
        Cmd::BufferToTexture { src, dst, .. } => {
            if !src.is_valid() {
                return Err(Error::validation(
                    "cannot submit commands referencing an invalid buffer",
                ));
            }
            check_texture(dst)?;
        }
        Cmd::TextureToBuffer { src, dst, .. } => {
            check_texture(src)?;
            check_buffer_write(dst)?;
        }
        Cmd::TextureToTexture { src, dst, .. } => {
            check_texture(src)?;
            check_texture(dst)?;
        }
        Cmd::Timestamp(set, _) => {
            if !set.is_valid() {
                return Err(Error::validation(
                    "cannot submit commands referencing an invalid query set",
                ));
            }
        }
        Cmd::Resolve { set, dst, .. } => {
            if !set.is_valid() {
                return Err(Error::validation(
                    "cannot submit commands referencing an invalid query set",
                ));
            }
            check_buffer_write(dst)?;
        }
        Cmd::Pass(pass) => {
            check_view(&pass.target)?;
            if let Some((set, _, _)) = &pass.timestamps
                && !set.is_valid()
            {
                return Err(Error::validation(
                    "cannot submit commands referencing an invalid query set",
                ));
            }
            for pc in &pass.cmds {
                match pc {
                    PassCmd::Pipeline(p) => {
                        if !p.is_valid() {
                            return Err(Error::validation(
                                "cannot submit commands referencing an invalid render pipeline",
                            ));
                        }
                    }
                    PassCmd::Group(_, g, _) => {
                        if g.has_retired_swapchain() {
                            return Err(Error::validation(
                                "cannot submit commands targeting a retired swapchain image",
                            ));
                        }
                        if !g.is_valid() {
                            return Err(Error::validation(
                                "cannot submit commands referencing an invalid bind group",
                            ));
                        }
                    }
                    PassCmd::Vertex(_, b, _) => {
                        if !b.is_valid() {
                            return Err(Error::validation(
                                "cannot submit commands referencing an invalid vertex buffer",
                            ));
                        }
                    }
                    PassCmd::Viewport(_) | PassCmd::Scissor(_) | PassCmd::Draw { .. } => {}
                }
            }
        }
    }
    Ok(())
}

pub(super) struct Recorder<'a> {
    d: &'a ash::Device,
    cb: vk::CommandBuffer,
}

const COLOR: vk::ImageSubresourceLayers = vk::ImageSubresourceLayers {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    mip_level: 0,
    base_array_layer: 0,
    layer_count: 1,
};

fn layers(origin: Origin3d, size: Extent3d) -> vk::ImageSubresourceLayers {
    vk::ImageSubresourceLayers {
        base_array_layer: origin.z,
        layer_count: size.depth_or_array_layers.max(1),
        ..COLOR
    }
}

fn offset(o: Origin3d) -> vk::Offset3D {
    vk::Offset3D {
        x: o.x as i32,
        y: o.y as i32,
        z: 0,
    }
}

fn extent(s: Extent3d) -> vk::Extent3D {
    vk::Extent3D {
        width: s.width,
        height: s.height,
        depth: 1,
    }
}

impl<'a> Recorder<'a> {
    pub fn new(d: &'a ash::Device, cb: vk::CommandBuffer) -> Self {
        Self { d, cb }
    }

    fn barrier(&self) {
        let memory = [vk::MemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::MEMORY_WRITE)
            .dst_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE)];
        // SAFETY: `cb` is recording, outside any rendering scope.
        unsafe {
            self.d.cmd_pipeline_barrier(
                self.cb,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &memory,
                &[],
                &[],
            );
        }
    }

    fn transition(&self, t: &Texture, from: vk::ImageLayout, to: vk::ImageLayout) {
        let image = [vk::ImageMemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::MEMORY_WRITE)
            .dst_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE)
            .old_layout(from)
            .new_layout(to)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(t.0.raw)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: vk::REMAINING_ARRAY_LAYERS,
            })];
        // SAFETY: `cb` is recording, outside any rendering scope; `t` is live.
        unsafe {
            self.d.cmd_pipeline_barrier(
                self.cb,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &image,
            );
        }
    }

    pub fn record(&mut self, cmd: &Cmd) {
        let (d, cb) = (self.d, self.cb);
        let general = vk::ImageLayout::GENERAL;
        match cmd {
            Cmd::Init(t) | Cmd::Acquire(t) => {
                self.transition(t, vk::ImageLayout::UNDEFINED, general);
                return;
            }
            Cmd::Release(t) => {
                self.transition(t, general, vk::ImageLayout::PRESENT_SRC_KHR);
                return;
            }
            _ => self.barrier(),
        }
        // SAFETY (every arm): `cb` is recording outside a rendering scope and
        // each handle is held alive by `cmd` until the submission completes.
        match cmd {
            Cmd::Init(_) | Cmd::Acquire(_) | Cmd::Release(_) => {}
            Cmd::CopyBuffer {
                src,
                src_offset,
                dst,
                dst_offset,
                size,
            } => {
                if *size == 0 {
                    return;
                }
                let region = [vk::BufferCopy {
                    src_offset: *src_offset,
                    dst_offset: *dst_offset,
                    size: *size,
                }];
                // SAFETY: see above.
                unsafe { d.cmd_copy_buffer(cb, src.0.raw, dst.0.raw, &region) };
            }
            Cmd::BufferToTexture {
                src,
                offset: at,
                dst,
                origin,
                size,
            } => {
                let region = [vk::BufferImageCopy {
                    buffer_offset: *at,
                    buffer_row_length: 0,
                    buffer_image_height: 0,
                    image_subresource: layers(*origin, *size),
                    image_offset: offset(*origin),
                    image_extent: extent(*size),
                }];
                // SAFETY: see above.
                unsafe { d.cmd_copy_buffer_to_image(cb, src.0.raw, dst.0.raw, general, &region) };
            }
            Cmd::TextureToBuffer {
                src,
                origin,
                dst,
                offset: at,
                row_length,
                image_height,
                size,
            } => {
                let region = [vk::BufferImageCopy {
                    buffer_offset: *at,
                    buffer_row_length: *row_length,
                    buffer_image_height: *image_height,
                    image_subresource: layers(*origin, *size),
                    image_offset: offset(*origin),
                    image_extent: extent(*size),
                }];
                // SAFETY: see above.
                unsafe { d.cmd_copy_image_to_buffer(cb, src.0.raw, general, dst.0.raw, &region) };
            }
            Cmd::TextureToTexture {
                src,
                src_origin,
                dst,
                dst_origin,
                size,
            } => {
                let region = [vk::ImageCopy {
                    src_subresource: layers(*src_origin, *size),
                    src_offset: offset(*src_origin),
                    dst_subresource: layers(*dst_origin, *size),
                    dst_offset: offset(*dst_origin),
                    extent: extent(*size),
                }];
                // SAFETY: see above.
                unsafe {
                    d.cmd_copy_image(cb, src.0.raw, general, dst.0.raw, general, &region);
                }
            }
            Cmd::Timestamp(set, index) => self.timestamp(set, *index),
            Cmd::Resolve {
                set,
                first,
                count,
                dst,
                offset: at,
            } => {
                // SAFETY: see above.
                unsafe {
                    d.cmd_copy_query_pool_results(
                        cb,
                        set.0.raw,
                        *first,
                        *count,
                        dst.0.raw,
                        *at,
                        8,
                        vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT,
                    );
                }
            }
            Cmd::Pass(pass) => self.pass(pass),
        }
    }

    fn timestamp(&self, set: &QuerySet, index: u32) {
        // SAFETY: `cb` is recording outside a rendering scope; `index` is in the pool.
        unsafe {
            self.d.cmd_reset_query_pool(self.cb, set.0.raw, index, 1);
            self.d.cmd_write_timestamp(
                self.cb,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                set.0.raw,
                index,
            );
        }
    }

    fn pass(&self, pass: &PassRecord) {
        let (d, cb) = (self.d, self.cb);
        let texture = &pass.target.0.texture.0;
        let (w, h) = (texture.width, texture.height);
        if let Some((set, Some(start), _)) = &pass.timestamps {
            self.timestamp(set, *start);
        }
        let attachments = [vk::RenderingAttachmentInfo::default()
            .image_view(pass.target.0.raw)
            .image_layout(vk::ImageLayout::GENERAL)
            .load_op(if pass.clear.is_some() {
                vk::AttachmentLoadOp::CLEAR
            } else {
                vk::AttachmentLoadOp::LOAD
            })
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: pass.clear.unwrap_or_default(),
                },
            })];
        let area = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D {
                width: w,
                height: h,
            },
        };
        let info = vk::RenderingInfo::default()
            .render_area(area)
            .layer_count(1)
            .color_attachments(&attachments);
        let full = vk::Viewport {
            x: 0.,
            y: 0.,
            width: w as f32,
            height: h as f32,
            min_depth: 0.,
            max_depth: 1.,
        };
        // SAFETY: the target view is live and in GENERAL; every command below
        // uses handles `pass` holds.
        unsafe {
            d.cmd_begin_rendering(cb, &info);
            d.cmd_set_viewport(cb, 0, &[full]);
            d.cmd_set_scissor(cb, 0, &[area]);
        }
        let mut layout = vk::PipelineLayout::null();
        let mut set_count = 0;
        // Groups set before their pipeline are bound at the next draw.
        let mut groups: Vec<Option<(vk::DescriptorSet, &[u32])>> = Vec::new();
        let mut dirty = false;
        for cmd in &pass.cmds {
            // SAFETY: as above.
            unsafe {
                match cmd {
                    PassCmd::Pipeline(p) => {
                        d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, p.0.raw);
                        layout = p.0.layout.0.raw;
                        set_count = p.0.layout.0.sets.len();
                        dirty = true;
                    }
                    PassCmd::Group(index, group, offsets) => {
                        let i = *index as usize;
                        if groups.len() <= i {
                            groups.resize(i + 1, None);
                        }
                        groups[i] = Some((group.0.set, offsets));
                        dirty = true;
                    }
                    PassCmd::Vertex(slot, buffer, at) => {
                        d.cmd_bind_vertex_buffers(cb, *slot, &[buffer.0.raw], &[*at]);
                    }
                    PassCmd::Viewport([x, y, w, h, near, far]) => {
                        let v = vk::Viewport {
                            x: *x,
                            y: *y,
                            width: *w,
                            height: *h,
                            min_depth: *near,
                            max_depth: *far,
                        };
                        d.cmd_set_viewport(cb, 0, &[v]);
                    }
                    PassCmd::Scissor([x, y, w, h]) => {
                        let r = vk::Rect2D {
                            offset: vk::Offset2D {
                                x: *x as i32,
                                y: *y as i32,
                            },
                            extent: vk::Extent2D {
                                width: *w,
                                height: *h,
                            },
                        };
                        d.cmd_set_scissor(cb, 0, &[r]);
                    }
                    PassCmd::Draw {
                        vertices,
                        instances,
                    } => {
                        if dirty && layout != vk::PipelineLayout::null() {
                            // Only the active layout's slots; the rest stay
                            // recorded for a later pipeline that uses them.
                            for (i, g) in groups.iter().enumerate().take(set_count) {
                                if let Some((set, offsets)) = g {
                                    d.cmd_bind_descriptor_sets(
                                        cb,
                                        vk::PipelineBindPoint::GRAPHICS,
                                        layout,
                                        i as u32,
                                        &[*set],
                                        offsets,
                                    );
                                }
                            }
                            dirty = false;
                        }
                        d.cmd_draw(
                            cb,
                            vertices.len() as u32,
                            instances.len() as u32,
                            vertices.start,
                            instances.start,
                        );
                    }
                }
            }
        }
        // SAFETY: the rendering scope begun above.
        unsafe { d.cmd_end_rendering(cb) };
        if let Some((set, _, Some(end))) = &pass.timestamps {
            self.timestamp(set, *end);
        }
    }

    /// Makes the GPU's writes visible to host reads of mapped buffers.
    pub fn finish(&self) {
        let memory = [vk::MemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::MEMORY_WRITE)
            .dst_access_mask(vk::AccessFlags::HOST_READ)];
        // SAFETY: `cb` is recording, outside any rendering scope.
        unsafe {
            self.d.cmd_pipeline_barrier(
                self.cb,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::HOST,
                vk::DependencyFlags::empty(),
                &memory,
                &[],
                &[],
            );
        }
    }
}
