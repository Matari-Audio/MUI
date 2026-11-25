//! Recorded Metal commands and encoding into an `MTLCommandBuffer`.

use std::sync::Arc;

use mui_shaders::BindingKind;
use objc2::runtime::ProtocolObject;
use objc2_metal::{
    MTLBlitCommandEncoder as _, MTLClearColor, MTLCommandBuffer, MTLCommandEncoder as _,
    MTLLoadAction, MTLOrigin, MTLRenderCommandEncoder as _, MTLRenderPassDescriptor,
    MTLScissorRect, MTLSize, MTLStoreAction, MTLViewport,
};

use super::{
    BindGroup, Bound, Buffer, DeviceShared, Error, QuerySet, RenderPipeline, Texture, TextureView,
    mtl_primitive,
};
use crate::binding::{clamp_scissor_rect, metal_vertex_buffer_index};
use crate::shared::{
    resolve_dynamic_offset, validate_buffer_range, validate_submit_buffer, validate_submit_texture,
    validate_texture_region, validate_texture_to_buffer_layout, validate_texture_to_texture,
};
use crate::types::{BufferUsages, Extent3d, Origin3d, ShaderStages, TextureUsages};

pub enum Cmd {
    CopyBuffer {
        src: Buffer,
        src_offset: u64,
        dst: Buffer,
        dst_offset: u64,
        size: u64,
    },
    BufferToTexture {
        src: Buffer,
        offset: u64,
        dst: Texture,
        origin: Origin3d,
        size: Extent3d,
    },
    TextureToBuffer {
        src: Texture,
        mip_level: u32,
        origin: Origin3d,
        dst: Buffer,
        offset: u64,
        bytes_per_row: u32,
        rows_per_image: u32,
        size: Extent3d,
    },
    TextureToTexture {
        src: Texture,
        src_mip: u32,
        src_origin: Origin3d,
        dst: Texture,
        dst_mip: u32,
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
    pub clear: Option<[f32; 4]>,
    pub timestamps: Option<(QuerySet, Option<u32>, Option<u32>)>,
    pub cmds: Vec<PassCmd>,
}

pub enum PassCmd {
    Pipeline(RenderPipeline),
    Group(u32, BindGroup, Vec<u32>),
    Vertex(u32, Buffer, u64),
    Viewport([f32; 6]),
    Scissor([u32; 4]),
    Draw {
        vertices: std::ops::Range<u32>,
        instances: std::ops::Range<u32>,
    },
}

const METAL_MAX_END: u64 = usize::MAX as u64;

fn check_texture(device: &Arc<DeviceShared>, t: &Texture) -> Result<(), Error> {
    validate_submit_texture(
        t.belongs_to(device),
        t.is_retired_swapchain(),
        t.is_valid(),
        "texture",
    )
}

fn check_view(device: &Arc<DeviceShared>, v: &TextureView) -> Result<(), Error> {
    validate_submit_texture(
        v.belongs_to(device),
        v.is_retired_swapchain(),
        v.is_valid(),
        "texture view",
    )
}

fn check_buffer_src(device: &Arc<DeviceShared>, src: &Buffer) -> Result<(), Error> {
    validate_submit_buffer(src.belongs_to(device), src.is_valid(), None)
}

fn check_buffer_write(device: &Arc<DeviceShared>, dst: &Buffer) -> Result<(), Error> {
    validate_submit_buffer(
        dst.belongs_to(device),
        dst.is_valid(),
        Some(dst.is_mapped_or_pending()),
    )
}

fn check_buffer_range(b: &Buffer, offset: u64, size: u64, what: &str) -> Result<(), Error> {
    validate_buffer_range(offset, size, b.size(), METAL_MAX_END, what).map(drop)
}

pub(super) fn validate_cmd(device: &Arc<DeviceShared>, cmd: &Cmd) -> Result<(), Error> {
    match cmd {
        Cmd::CopyBuffer {
            src,
            src_offset,
            dst,
            dst_offset,
            size,
        } => {
            check_buffer_src(device, src)?;
            if !src.usage().contains(BufferUsages::COPY_SRC) {
                return Err(Error::validation("copy_buffer source lacks COPY_SRC"));
            }
            check_buffer_write(device, dst)?;
            if !dst.usage().contains(BufferUsages::COPY_DST) {
                return Err(Error::validation("copy_buffer destination lacks COPY_DST"));
            }
            check_buffer_range(src, *src_offset, *size, "copy_buffer src")?;
            check_buffer_range(dst, *dst_offset, *size, "copy_buffer dst")
        }
        Cmd::BufferToTexture {
            src,
            offset,
            dst,
            origin,
            size,
        } => {
            check_buffer_src(device, src)?;
            if !src.usage().contains(BufferUsages::COPY_SRC) {
                return Err(Error::validation("buffer_to_texture source lacks COPY_SRC"));
            }
            check_texture(device, dst)?;
            if !dst.usage().contains(TextureUsages::COPY_DST) {
                return Err(Error::validation(
                    "buffer_to_texture destination lacks COPY_DST",
                ));
            }
            if size.width == 0 || size.height == 0 {
                return Ok(());
            }
            validate_texture_region(*origin, *size, dst.width(), dst.height(), dst.layers())?;
            let layers = u64::from(size.depth_or_array_layers.max(1));
            let total = u64::from(size.width)
                .checked_mul(u64::from(dst.format().block_size()))
                .and_then(|bpr| bpr.checked_mul(u64::from(size.height)))
                .and_then(|bpi| bpi.checked_mul(layers))
                .ok_or_else(|| Error::validation("buffer_to_texture size overflows u64"))?;
            check_buffer_range(src, *offset, total, "buffer_to_texture")
        }
        Cmd::TextureToBuffer {
            src,
            mip_level,
            origin,
            dst,
            offset,
            bytes_per_row,
            rows_per_image,
            size,
        } => {
            check_texture(device, src)?;
            check_buffer_write(device, dst)?;
            validate_texture_to_buffer_layout(
                *origin,
                *size,
                (src.width(), src.height(), src.layers()),
                *mip_level,
                src.mip_level_count(),
                src.sample_count(),
                src.usage(),
                src.format().block_size(),
                dst.usage(),
                dst.size(),
                *offset,
                *bytes_per_row,
                *rows_per_image,
                METAL_MAX_END,
            )?;
            if size.width > 0 && size.height > 0 {
                let align = u64::from(src.format().block_size());
                if !offset.is_multiple_of(align) {
                    return Err(Error::validation(format!(
                        "texture_to_buffer destination offset ({offset}) must be a multiple of the texture pixel size ({align})"
                    )));
                }
                if !u64::from(*bytes_per_row).is_multiple_of(align) {
                    return Err(Error::validation(format!(
                        "texture_to_buffer bytes_per_row ({bytes_per_row}) must be a multiple of the texture pixel size ({align})"
                    )));
                }
            }
            Ok(())
        }
        Cmd::TextureToTexture {
            src,
            src_mip,
            src_origin,
            dst,
            dst_mip,
            dst_origin,
            size,
        } => {
            check_texture(device, src)?;
            check_texture(device, dst)?;
            validate_texture_to_texture(
                Arc::ptr_eq(&src.0, &dst.0),
                (src.width(), src.height(), src.layers()),
                *src_mip,
                src.mip_level_count(),
                src.sample_count(),
                src.format(),
                src.usage(),
                *src_origin,
                (dst.width(), dst.height(), dst.layers()),
                *dst_mip,
                dst.mip_level_count(),
                dst.sample_count(),
                dst.format(),
                dst.usage(),
                *dst_origin,
                *size,
            )
        }
        Cmd::Timestamp(_, _) | Cmd::Resolve { .. } => Err(Error::validation(
            "timestamp queries are not supported on the native Metal backend",
        )),
        Cmd::Pass(pass) => {
            check_view(device, &pass.target)?;
            if pass.timestamps.is_some() {
                return Err(Error::validation(
                    "timestamp queries are not supported on the native Metal backend",
                ));
            }
            if !pass
                .target
                .0
                .texture
                .usage()
                .contains(TextureUsages::RENDER_ATTACHMENT)
            {
                return Err(Error::validation(
                    "render pass target lacks RENDER_ATTACHMENT",
                ));
            }
            let mut current_pipeline: Option<&RenderPipeline> = None;
            let mut bound_groups: [Option<&BindGroup>; 4] = [None; 4];
            for pc in &pass.cmds {
                match pc {
                    PassCmd::Pipeline(p) => {
                        if !p.belongs_to(device) {
                            return Err(Error::validation(
                                "render pipeline belongs to another device",
                            ));
                        }
                        if !p.is_valid() {
                            return Err(Error::validation("invalid render pipeline"));
                        }
                        current_pipeline = Some(p);
                    }
                    PassCmd::Group(group_idx, g, dynamic_offsets) => {
                        if *group_idx >= 4 {
                            return Err(Error::validation(format!(
                                "bind group index {group_idx} exceeds max_bind_groups (4)"
                            )));
                        }
                        if !g.belongs_to(device) {
                            return Err(Error::validation("bind group belongs to another device"));
                        }
                        if !g.is_valid() {
                            return Err(Error::validation("invalid bind group"));
                        }
                        if dynamic_offsets.len() != g.0.layout.0.dynamic_bindings.len() {
                            return Err(Error::validation(format!(
                                "expected {} dynamic offsets for group {group_idx}, got {}",
                                g.0.layout.0.dynamic_bindings.len(),
                                dynamic_offsets.len()
                            )));
                        }
                        for (binding, bound) in &g.0.entries {
                            match bound {
                                Bound::Buffer {
                                    buffer,
                                    offset,
                                    size,
                                } => {
                                    check_buffer_src(device, buffer)?;
                                    if !buffer.usage().contains(BufferUsages::UNIFORM) {
                                        return Err(Error::validation(
                                            "bound buffer lacks UNIFORM usage",
                                        ));
                                    }
                                    let total_off = resolve_dynamic_offset(
                                        &g.0.layout.0.dynamic_bindings,
                                        *binding,
                                        dynamic_offsets,
                                        *offset,
                                    )?;
                                    check_buffer_range(
                                        buffer,
                                        total_off,
                                        size.unwrap_or(0),
                                        "uniform bind group",
                                    )?;
                                }
                                Bound::View(v) => {
                                    check_view(device, v)?;
                                    if !v.0.texture.usage().contains(TextureUsages::TEXTURE_BINDING)
                                    {
                                        return Err(Error::validation(
                                            "bound texture view lacks TEXTURE_BINDING",
                                        ));
                                    }
                                }
                                Bound::Sampler(s) => {
                                    if !s.belongs_to(device) {
                                        return Err(Error::validation(
                                            "sampler in bind group belongs to another device",
                                        ));
                                    }
                                    if !s.is_valid() {
                                        return Err(Error::validation(
                                            "invalid sampler in bind group",
                                        ));
                                    }
                                }
                            }
                        }
                        bound_groups[*group_idx as usize] = Some(g);
                    }
                    PassCmd::Vertex(slot, b, off) => {
                        let _ = metal_vertex_buffer_index(*slot).map_err(Error::validation)?;
                        check_buffer_src(device, b)?;
                        if !b.usage().contains(BufferUsages::VERTEX) {
                            return Err(Error::validation("vertex buffer lacks VERTEX usage"));
                        }
                        check_buffer_range(b, *off, 0, "vertex buffer")?;
                    }
                    PassCmd::Viewport(_) | PassCmd::Scissor(_) => {}
                    PassCmd::Draw { .. } => {
                        if let Some(pipe) = current_pipeline {
                            for (group_idx, expected_layout) in
                                pipe.0.group_layouts.iter().enumerate()
                            {
                                let Some(bg) = bound_groups.get(group_idx).and_then(|g| *g) else {
                                    return Err(Error::validation(format!(
                                        "draw requires bind group {group_idx} to be bound for active pipeline"
                                    )));
                                };
                                if !bg.0.layout.is_compatible_with(expected_layout) {
                                    return Err(Error::validation(format!(
                                        "bind group {group_idx} layout is incompatible with active pipeline"
                                    )));
                                }
                            }
                        }
                    }
                }
            }
            Ok(())
        }
    }
}

pub(super) fn encode_all(
    cb: &ProtocolObject<dyn MTLCommandBuffer>,
    cmds: &[Cmd],
) -> Result<(), Error> {
    for cmd in cmds {
        match cmd {
            Cmd::CopyBuffer {
                src,
                src_offset,
                dst,
                dst_offset,
                size,
            } => {
                if *size == 0 {
                    continue;
                }
                let s = src
                    .0
                    .raw
                    .as_ref()
                    .ok_or_else(|| Error::validation("copy_buffer source is invalid"))?;
                let d = dst
                    .0
                    .raw
                    .as_ref()
                    .ok_or_else(|| Error::validation("copy_buffer destination is invalid"))?;
                let blit = cb.blitCommandEncoder().ok_or_else(|| {
                    Error::internal("MTLCommandBuffer::blitCommandEncoder returned nil")
                })?;
                // SAFETY: bounds checked by validation; both buffers stay retained in `cmds`.
                unsafe {
                    blit.copyFromBuffer_sourceOffset_toBuffer_destinationOffset_size(
                        &s.0,
                        *src_offset as usize,
                        &d.0,
                        *dst_offset as usize,
                        *size as usize,
                    );
                }
                blit.endEncoding();
            }
            Cmd::BufferToTexture {
                src,
                offset,
                dst,
                origin,
                size,
            } => {
                if size.width == 0 || size.height == 0 {
                    continue;
                }
                let s = src
                    .0
                    .raw
                    .as_ref()
                    .ok_or_else(|| Error::validation("buffer_to_texture source is invalid"))?;
                let d =
                    dst.0.raw.as_ref().ok_or_else(|| {
                        Error::validation("buffer_to_texture destination is invalid")
                    })?;
                let layers = size.depth_or_array_layers.max(1);
                let block = dst.format().block_size();
                let bpr = size.width * block;
                let bpi = bpr * size.height;
                let blit = cb.blitCommandEncoder().ok_or_else(|| {
                    Error::internal("MTLCommandBuffer::blitCommandEncoder returned nil")
                })?;
                for z in 0..layers {
                    let slice_offset = *offset as usize + (z as usize) * (bpi as usize);
                    // SAFETY: packed tightly per slice, retained in `cmds`; `sourceBytesPerImage`
                    // must be `0` for 2D (`depth: 1`) copies per Apple's Metal API spec.
                    unsafe {
                        blit.copyFromBuffer_sourceOffset_sourceBytesPerRow_sourceBytesPerImage_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                            &s.0,
                            slice_offset,
                            bpr as usize,
                            0,
                            MTLSize {
                                width: size.width as usize,
                                height: size.height as usize,
                                depth: 1,
                            },
                            &d.0,
                            (origin.z + z) as usize,
                            0,
                            MTLOrigin {
                                x: origin.x as usize,
                                y: origin.y as usize,
                                z: 0,
                            },
                        );
                    }
                }
                blit.endEncoding();
            }
            Cmd::TextureToBuffer {
                src,
                mip_level,
                origin,
                dst,
                offset,
                bytes_per_row,
                rows_per_image,
                size,
            } => {
                if size.width == 0 || size.height == 0 {
                    continue;
                }
                let s = src
                    .0
                    .raw
                    .as_ref()
                    .ok_or_else(|| Error::validation("texture_to_buffer source is invalid"))?;
                let d =
                    dst.0.raw.as_ref().ok_or_else(|| {
                        Error::validation("texture_to_buffer destination is invalid")
                    })?;
                let layers = size.depth_or_array_layers.max(1);
                let bpi = (*bytes_per_row as usize) * (*rows_per_image as usize);
                let blit = cb.blitCommandEncoder().ok_or_else(|| {
                    Error::internal("MTLCommandBuffer::blitCommandEncoder returned nil")
                })?;
                for z in 0..layers {
                    let dst_off = *offset as usize + (z as usize) * bpi;
                    // SAFETY: valid slice/region/offset, retained in `cmds`; `destinationBytesPerImage`
                    // must be `0` for 2D (`depth: 1`) copies per Apple's Metal API spec.
                    unsafe {
                        blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
                            &s.0,
                            (origin.z + z) as usize,
                            *mip_level as usize,
                            MTLOrigin {
                                x: origin.x as usize,
                                y: origin.y as usize,
                                z: 0,
                            },
                            MTLSize {
                                width: size.width as usize,
                                height: size.height as usize,
                                depth: 1,
                            },
                            &d.0,
                            dst_off,
                            *bytes_per_row as usize,
                            0,
                        );
                    }
                }
                blit.endEncoding();
            }
            Cmd::TextureToTexture {
                src,
                src_mip,
                src_origin,
                dst,
                dst_mip,
                dst_origin,
                size,
            } => {
                if size.width == 0 || size.height == 0 {
                    continue;
                }
                let s = src
                    .0
                    .raw
                    .as_ref()
                    .ok_or_else(|| Error::validation("texture_to_texture source is invalid"))?;
                let d = dst.0.raw.as_ref().ok_or_else(|| {
                    Error::validation("texture_to_texture destination is invalid")
                })?;
                let layers = size.depth_or_array_layers.max(1);
                let blit = cb.blitCommandEncoder().ok_or_else(|| {
                    Error::internal("MTLCommandBuffer::blitCommandEncoder returned nil")
                })?;
                for z in 0..layers {
                    // SAFETY: valid source/dest slices and 2D regions, retained in `cmds`.
                    unsafe {
                        blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                            &s.0,
                            (src_origin.z + z) as usize,
                            *src_mip as usize,
                            MTLOrigin {
                                x: src_origin.x as usize,
                                y: src_origin.y as usize,
                                z: 0,
                            },
                            MTLSize {
                                width: size.width as usize,
                                height: size.height as usize,
                                depth: 1,
                            },
                            &d.0,
                            (dst_origin.z + z) as usize,
                            *dst_mip as usize,
                            MTLOrigin {
                                x: dst_origin.x as usize,
                                y: dst_origin.y as usize,
                                z: 0,
                            },
                        );
                    }
                }
                blit.endEncoding();
            }
            Cmd::Pass(pass) => encode_pass(cb, pass)?,
            Cmd::Timestamp(_, _) | Cmd::Resolve { .. } => {
                return Err(Error::validation(
                    "timestamp queries are not supported on the native Metal backend",
                ));
            }
        }
    }
    Ok(())
}

struct EndEncodingGuard<'a>(&'a ProtocolObject<dyn objc2_metal::MTLRenderCommandEncoder>);

impl Drop for EndEncodingGuard<'_> {
    fn drop(&mut self) {
        self.0.endEncoding();
    }
}

fn encode_pass(cb: &ProtocolObject<dyn MTLCommandBuffer>, pass: &PassRecord) -> Result<(), Error> {
    let target_tex = pass
        .target
        .0
        .raw
        .as_ref()
        .ok_or_else(|| Error::validation("render pass target view is invalid"))?;
    let rp = MTLRenderPassDescriptor::renderPassDescriptor();
    // SAFETY: attachment 0 always exists on `MTLRenderPassDescriptor`.
    let color0 = unsafe { rp.colorAttachments().objectAtIndexedSubscript(0) };
    color0.setTexture(Some(&target_tex.0));
    color0.setStoreAction(MTLStoreAction::Store);
    if let Some([r, g, b, a]) = pass.clear {
        color0.setLoadAction(MTLLoadAction::Clear);
        color0.setClearColor(MTLClearColor {
            red: f64::from(r),
            green: f64::from(g),
            blue: f64::from(b),
            alpha: f64::from(a),
        });
    } else {
        color0.setLoadAction(MTLLoadAction::Load);
    }

    let enc = cb.renderCommandEncoderWithDescriptor(&rp).ok_or_else(|| {
        Error::internal("MTLCommandBuffer::renderCommandEncoderWithDescriptor returned nil")
    })?;
    let _end_guard = EndEncodingGuard(&enc);

    let tw_u32 = pass.target.0.texture.width();
    let th_u32 = pass.target.0.texture.height();
    let tw = tw_u32 as usize;
    let th = th_u32 as usize;
    enc.setViewport(MTLViewport {
        originX: 0.0,
        originY: 0.0,
        width: tw as f64,
        height: th as f64,
        znear: 0.0,
        zfar: 1.0,
    });
    enc.setScissorRect(MTLScissorRect {
        x: 0,
        y: 0,
        width: tw,
        height: th,
    });

    let mut current_pipeline: Option<&RenderPipeline> = None;
    let mut pending_groups: [Option<(&BindGroup, &[u32])>; 4] = [None; 4];

    for cmd in &pass.cmds {
        match cmd {
            PassCmd::Pipeline(pipe) => {
                let state = pipe
                    .0
                    .state
                    .as_ref()
                    .ok_or_else(|| Error::validation("invalid render pipeline state"))?;
                enc.setRenderPipelineState(&state.0);
                current_pipeline = Some(pipe);
                for (group_idx, slot) in pending_groups.iter().enumerate() {
                    if let Some((bg, offsets)) = slot
                        && pipe
                            .0
                            .group_layouts
                            .get(group_idx)
                            .is_some_and(|expected| bg.0.layout.is_compatible_with(expected))
                    {
                        bind_group(&enc, pipe, group_idx as u32, bg, offsets)?;
                    }
                }
            }
            PassCmd::Group(group_idx, bg, offsets) => {
                if let Some(slot) = pending_groups.get_mut(*group_idx as usize) {
                    *slot = Some((bg, offsets.as_slice()));
                }
                if let Some(pipe) = current_pipeline
                    && pipe
                        .0
                        .group_layouts
                        .get(*group_idx as usize)
                        .is_some_and(|expected| bg.0.layout.is_compatible_with(expected))
                {
                    bind_group(&enc, pipe, *group_idx, bg, offsets)?;
                }
            }
            PassCmd::Vertex(slot, buffer, offset) => {
                let metal_idx = metal_vertex_buffer_index(*slot).map_err(Error::validation)?;
                let raw = buffer
                    .0
                    .raw
                    .as_ref()
                    .ok_or_else(|| Error::validation("cannot bind an invalid vertex buffer"))?;
                // SAFETY: `metal_idx` is in `16..31` and `buffer` stays retained in `pass`.
                unsafe {
                    enc.setVertexBuffer_offset_atIndex(
                        Some(&raw.0),
                        *offset as usize,
                        metal_idx as usize,
                    );
                }
            }
            PassCmd::Viewport([x, y, w, h, znear, zfar]) => {
                enc.setViewport(MTLViewport {
                    originX: f64::from(*x),
                    originY: f64::from(*y),
                    width: f64::from(*w),
                    height: f64::from(*h),
                    znear: f64::from(*znear),
                    zfar: f64::from(*zfar),
                });
            }
            PassCmd::Scissor([x, y, w, h]) => {
                let [cx, cy, cw, ch] = clamp_scissor_rect(*x, *y, *w, *h, tw_u32, th_u32);
                enc.setScissorRect(MTLScissorRect {
                    x: cx as usize,
                    y: cy as usize,
                    width: cw as usize,
                    height: ch as usize,
                });
            }
            PassCmd::Draw {
                vertices,
                instances,
            } => {
                let Some(pipe) = current_pipeline else {
                    continue;
                };
                let vcount = vertices.end.saturating_sub(vertices.start);
                let icount = instances.end.saturating_sub(instances.start);
                if vcount == 0 || icount == 0 {
                    continue;
                }
                // SAFETY: pipeline, buffers, and textures are bound and retained.
                unsafe {
                    enc.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(
                        mtl_primitive(pipe.0.topology),
                        vertices.start as usize,
                        vcount as usize,
                        icount as usize,
                        instances.start as usize,
                    );
                }
            }
        }
    }

    Ok(())
}

fn bind_group(
    enc: &ProtocolObject<dyn objc2_metal::MTLRenderCommandEncoder>,
    pipe: &RenderPipeline,
    group_idx: u32,
    bg: &BindGroup,
    dynamic_offsets: &[u32],
) -> Result<(), Error> {
    for (binding, bound) in &bg.0.entries {
        let Some(resolved) = pipe.0.bindings.resolve(group_idx, *binding) else {
            continue;
        };
        let slot = resolved.slot as usize;
        let vis = resolved.visibility;
        match (resolved.kind, bound) {
            (BindingKind::UniformBuffer, Bound::Buffer { buffer, offset, .. }) => {
                let total_offset = resolve_dynamic_offset(
                    &bg.0.layout.0.dynamic_bindings,
                    *binding,
                    dynamic_offsets,
                    *offset,
                )? as usize;
                let raw =
                    buffer.0.raw.as_ref().ok_or_else(|| {
                        Error::validation("cannot bind an invalid uniform buffer")
                    })?;
                // SAFETY: slot matches `mui-shaders` MSL reflection and buffer stays retained.
                unsafe {
                    if vis.contains(ShaderStages::VERTEX) {
                        enc.setVertexBuffer_offset_atIndex(Some(&raw.0), total_offset, slot);
                    }
                    if vis.contains(ShaderStages::FRAGMENT) {
                        enc.setFragmentBuffer_offset_atIndex(Some(&raw.0), total_offset, slot);
                    }
                }
            }
            (BindingKind::Texture2d | BindingKind::Texture2dArray, Bound::View(view)) => {
                let raw = view
                    .0
                    .raw
                    .as_ref()
                    .ok_or_else(|| Error::validation("cannot bind an invalid texture view"))?;
                // SAFETY: slot matches `mui-shaders` MSL reflection and view stays retained.
                unsafe {
                    if vis.contains(ShaderStages::VERTEX) {
                        enc.setVertexTexture_atIndex(Some(&raw.0), slot);
                    }
                    if vis.contains(ShaderStages::FRAGMENT) {
                        enc.setFragmentTexture_atIndex(Some(&raw.0), slot);
                    }
                }
            }
            (BindingKind::Sampler, Bound::Sampler(sampler)) => {
                let raw = sampler
                    .0
                    .raw
                    .as_ref()
                    .ok_or_else(|| Error::validation("cannot bind an invalid sampler"))?;
                // SAFETY: slot matches `mui-shaders` MSL reflection and sampler stays retained.
                unsafe {
                    if vis.contains(ShaderStages::VERTEX) {
                        enc.setVertexSamplerState_atIndex(Some(&raw.0), slot);
                    }
                    if vis.contains(ShaderStages::FRAGMENT) {
                        enc.setFragmentSamplerState_atIndex(Some(&raw.0), slot);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}
