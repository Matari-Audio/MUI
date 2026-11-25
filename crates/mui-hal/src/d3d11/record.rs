//! Recorded D3D11 commands and execution on `ID3D11DeviceContext`.

use std::sync::Arc;

use mui_shaders::BindingKind;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BOX, D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_READ,
    D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING, D3D11_VIEWPORT, ID3D11Buffer, ID3D11Device,
    ID3D11DeviceContext, ID3D11DeviceContext1, ID3D11RenderTargetView, ID3D11SamplerState,
    ID3D11ShaderResourceView,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;

use super::{
    BindGroup, Bound, Buffer, DeviceShared, Error, QuerySet, RenderPipeline, Texture, TextureView,
    d3d_topology, dxgi_format, lock, win_err,
};
use crate::binding::{d3d11_constant_buffer_byte_width, d3d11_constant_buffer_range};
use crate::shared::{
    resolve_dynamic_offset, validate_buffer_range, validate_submit_buffer, validate_submit_texture,
    validate_texture_region, validate_texture_to_buffer_layout, validate_texture_to_texture,
};
use crate::types::{BufferUsages, Extent3d, Origin3d, ShaderStages, TextureUsages};

pub enum Cmd {
    WriteBuffer {
        dst: Buffer,
        offset: u64,
        data: Vec<u8>,
    },
    WriteTexture {
        dst: Texture,
        origin: Origin3d,
        size: Extent3d,
        data: Vec<u8>,
    },
    CopyBuffer {
        src: Buffer,
        src_offset: u64,
        dst: Buffer,
        dst_offset: u64,
        size: u64,
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

const MAX_VERTEX_SLOTS: u32 = 16;
const D3D11_MAX_END: u64 = u32::MAX as u64;

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
    validate_buffer_range(offset, size, b.size(), D3D11_MAX_END, what).map(drop)
}

pub(super) fn validate_cmd(device: &Arc<DeviceShared>, cmd: &Cmd) -> Result<(), Error> {
    match cmd {
        Cmd::WriteBuffer { dst, offset, data } => {
            check_buffer_write(device, dst)?;
            if !dst.usage().contains(BufferUsages::COPY_DST) {
                return Err(Error::validation("write_buffer destination lacks COPY_DST"));
            }
            check_buffer_range(dst, *offset, data.len() as u64, "write_buffer")
        }
        Cmd::WriteTexture {
            dst,
            origin,
            size,
            data,
        } => {
            check_texture(device, dst)?;
            if !dst.usage().contains(TextureUsages::COPY_DST) {
                return Err(Error::validation(
                    "write_texture destination lacks COPY_DST",
                ));
            }
            if size.width == 0 || size.height == 0 {
                return Ok(());
            }
            validate_texture_region(*origin, *size, dst.width(), dst.height(), dst.layers())?;
            let layers = size.depth_or_array_layers.max(1) as usize;
            let expected = (size.width as usize)
                .checked_mul(dst.format().block_size() as usize)
                .and_then(|bpr| bpr.checked_mul(size.height as usize))
                .and_then(|bpi| bpi.checked_mul(layers))
                .ok_or_else(|| Error::validation("write_texture byte length overflows"))?;
            if data.len() < expected {
                return Err(Error::validation("write_texture data buffer too small"));
            }
            Ok(())
        }
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
                D3D11_MAX_END,
            )
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
            "timestamp queries are not supported on the native D3D11 backend",
        )),
        Cmd::Pass(pass) => {
            check_view(device, &pass.target)?;
            if pass.timestamps.is_some() {
                return Err(Error::validation(
                    "timestamp queries are not supported on the native D3D11 backend",
                ));
            }
            if pass.target.0.rtv.is_none() {
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
                                    if total_off > 0 {
                                        let slice_bytes = size.unwrap_or_else(|| {
                                            buffer.size().saturating_sub(total_off)
                                        });
                                        let _ = d3d11_constant_buffer_range(total_off, slice_bytes)
                                            .map_err(Error::validation)?;
                                    }
                                }
                                Bound::View(v) => {
                                    check_view(device, v)?;
                                    if v.0.srv.is_none() {
                                        return Err(Error::validation(
                                            "texture view lacks TEXTURE_BINDING",
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
                        if *slot >= MAX_VERTEX_SLOTS {
                            return Err(Error::validation(format!(
                                "vertex buffer slot {slot} exceeds D3D11 limit {MAX_VERTEX_SLOTS}"
                            )));
                        }
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

/// Re-uploads a `UNIFORM` buffer's whole CPU shadow after a write.
///
/// Deliberately never a partial `UpdateSubresource1` box: partial constant-buffer
/// updates are only defined under `ConstantBufferPartialUpdate` with
/// `D3D11_COPY_NO_OVERWRITE`/`DISCARD` semantics, which would race in-flight
/// draws reading the same buffer. Full-buffer `UpdateSubresource(.., None, ..)`
/// is valid on every D3D11.0+ driver, the runtime versions it against queued
/// draws, and uniform buffers are small (<= 64 KiB), so the copy is cheap.
fn upload_constant_buffer(ctx: &ID3D11DeviceContext, raw: &ID3D11Buffer, shadow: &[u8]) {
    // SAFETY: `shadow.len()` equals the buffer's `ByteWidth` (both `alloc_size`),
    // and a `None` box with a full-size source is valid for constant buffers.
    unsafe {
        ctx.UpdateSubresource(raw, 0, None, shadow.as_ptr().cast(), 0, 0);
    }
}

pub(super) fn execute_all(
    device: &ID3D11Device,
    ctx: &ID3D11DeviceContext,
    ctx1: Option<&ID3D11DeviceContext1>,
    cb_offsetting: bool,
    cmds: &[Cmd],
) -> Result<(), Error> {
    for cmd in cmds {
        match cmd {
            Cmd::WriteBuffer { dst, offset, data } => {
                if data.is_empty() {
                    continue;
                }
                let raw = dst
                    .0
                    .raw
                    .as_ref()
                    .ok_or_else(|| Error::validation("cannot write to an invalid buffer"))?;
                if dst.usage().contains(BufferUsages::UNIFORM) {
                    let mut shadow_guard = lock(&dst.0.shadow);
                    let shadow =
                        shadow_guard.get_or_insert_with(|| vec![0u8; dst.alloc_size() as usize]);
                    let start = usize::try_from(*offset)
                        .map_err(|_| Error::validation("write_buffer offset overflows usize"))?;
                    let end = start
                        .checked_add(data.len())
                        .ok_or_else(|| Error::validation("write_buffer end overflows usize"))?;
                    if end > shadow.len() {
                        return Err(Error::validation(
                            "write_buffer exceeds uniform buffer size",
                        ));
                    }
                    shadow[start..end].copy_from_slice(data);
                    upload_constant_buffer(ctx, &raw.0, shadow);
                } else {
                    if dst.usage().contains(BufferUsages::MAP_READ)
                        && let Some(shadow) = lock(&dst.0.shadow).as_mut()
                    {
                        let start = usize::try_from(*offset).map_err(|_| {
                            Error::validation("write_buffer offset overflows usize")
                        })?;
                        let end = start
                            .checked_add(data.len())
                            .ok_or_else(|| Error::validation("write_buffer end overflows usize"))?;
                        if end > shadow.len() {
                            return Err(Error::validation(
                                "write_buffer exceeds MAP_READ shadow size",
                            ));
                        }
                        shadow[start..end].copy_from_slice(data);
                    }
                    let bx = D3D11_BOX {
                        left: *offset as u32,
                        top: 0,
                        front: 0,
                        right: (*offset as u32) + (data.len() as u32),
                        bottom: 1,
                        back: 1,
                    };
                    // SAFETY: partial vertex/copy buffer update with valid byte range.
                    unsafe {
                        ctx.UpdateSubresource(&raw.0, 0, Some(&bx), data.as_ptr().cast(), 0, 0);
                    }
                }
            }
            Cmd::WriteTexture {
                dst,
                origin,
                size,
                data,
            } => {
                if size.width == 0 || size.height == 0 {
                    continue;
                }
                let raw = dst
                    .0
                    .raw
                    .as_ref()
                    .ok_or_else(|| Error::validation("cannot write to an invalid texture"))?;
                let layers = size.depth_or_array_layers.max(1);
                let block = dst.format().block_size();
                let bpr = size.width * block;
                let bpi = bpr * size.height;
                for z in 0..layers {
                    let subresource = dst.subresource_index(0, origin.z + z);
                    let bx = D3D11_BOX {
                        left: origin.x,
                        top: origin.y,
                        front: 0,
                        right: origin.x + size.width,
                        bottom: origin.y + size.height,
                        back: 1,
                    };
                    let slice_start = (z as usize) * (bpi as usize);
                    // SAFETY: `data` holds `layers * bpi` tightly packed bytes.
                    unsafe {
                        ctx.UpdateSubresource(
                            &raw.0,
                            subresource,
                            Some(&bx),
                            data[slice_start..].as_ptr().cast(),
                            bpr,
                            bpi,
                        );
                    }
                }
            }
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
                if dst.usage().contains(BufferUsages::UNIFORM) {
                    copy_buffer_into_uniform(
                        device,
                        ctx,
                        src,
                        *src_offset,
                        dst,
                        *dst_offset,
                        *size,
                    )?;
                    continue;
                }
                let has_map_read_shadow =
                    dst.usage().contains(BufferUsages::MAP_READ) && lock(&dst.0.shadow).is_some();
                if has_map_read_shadow {
                    let bytes = read_buffer_slice(device, ctx, src, *src_offset, *size)?;
                    let dst_start = usize::try_from(*dst_offset)
                        .map_err(|_| Error::validation("copy_buffer dst_offset overflows usize"))?;
                    let dst_end = dst_start.checked_add(bytes.len()).ok_or_else(|| {
                        Error::validation("copy_buffer dst range overflows usize")
                    })?;
                    if let Some(shadow) = lock(&dst.0.shadow).as_mut() {
                        if dst_end > shadow.len() {
                            return Err(Error::validation(
                                "copy_buffer exceeds MAP_READ shadow size",
                            ));
                        }
                        shadow[dst_start..dst_end].copy_from_slice(&bytes);
                    }
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
                let bx = D3D11_BOX {
                    left: *src_offset as u32,
                    top: 0,
                    front: 0,
                    right: (*src_offset + *size) as u32,
                    bottom: 1,
                    back: 1,
                };
                // SAFETY: valid buffer-to-buffer copy range.
                unsafe {
                    ctx.CopySubresourceRegion(
                        &d.0,
                        0,
                        *dst_offset as u32,
                        0,
                        0,
                        &s.0,
                        0,
                        Some(&bx),
                    );
                }
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
                copy_texture_to_staging_buffer(
                    device,
                    ctx,
                    src,
                    *mip_level,
                    *origin,
                    dst,
                    *offset,
                    *bytes_per_row,
                    *rows_per_image,
                    *size,
                )?;
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
                let is_msaa = src.sample_count() > 1;
                let bx = D3D11_BOX {
                    left: src_origin.x,
                    top: src_origin.y,
                    front: 0,
                    right: src_origin.x + size.width,
                    bottom: src_origin.y + size.height,
                    back: 1,
                };
                for z in 0..layers {
                    let dst_sub = dst.subresource_index(*dst_mip, dst_origin.z + z);
                    let src_sub = src.subresource_index(*src_mip, src_origin.z + z);
                    // SAFETY: valid 2D region copy between textures of matching format and sample count;
                    // D3D11 requires `pSrcBox = None` and zero x/y offsets for multisampled copies.
                    unsafe {
                        ctx.CopySubresourceRegion(
                            &d.0,
                            dst_sub,
                            if is_msaa { 0 } else { dst_origin.x },
                            if is_msaa { 0 } else { dst_origin.y },
                            0,
                            &s.0,
                            src_sub,
                            if is_msaa { None } else { Some(&bx) },
                        );
                    }
                }
            }
            Cmd::Pass(pass) => execute_pass(device, ctx, ctx1, cb_offsetting, pass)?,
            Cmd::Timestamp(_, _) | Cmd::Resolve { .. } => {
                return Err(Error::validation(
                    "timestamp queries are not supported on the native D3D11 backend",
                ));
            }
        }
    }
    Ok(())
}

/// Reads `size` bytes at `src_offset` from `src`, preferring its CPU shadow
/// when present and otherwise staging through a temporary `D3D11_USAGE_STAGING`
/// buffer.
fn read_buffer_slice(
    device: &ID3D11Device,
    ctx: &ID3D11DeviceContext,
    src: &Buffer,
    src_offset: u64,
    size: u64,
) -> Result<Vec<u8>, Error> {
    let len =
        usize::try_from(size).map_err(|_| Error::validation("copy_buffer size overflows usize"))?;
    let src_start = usize::try_from(src_offset)
        .map_err(|_| Error::validation("copy_buffer src_offset overflows usize"))?;

    if let Some(src_shadow) = lock(&src.0.shadow).as_ref()
        && src_start
            .checked_add(len)
            .is_some_and(|e| e <= src_shadow.len())
    {
        return Ok(src_shadow[src_start..src_start + len].to_vec());
    }

    let s = src
        .0
        .raw
        .as_ref()
        .ok_or_else(|| Error::validation("copy_buffer source is invalid"))?;
    let len_u32 = u32::try_from(size)
        .map_err(|_| Error::validation("copy_buffer size exceeds D3D11 u32 limit"))?;
    let sdesc = D3D11_BUFFER_DESC {
        ByteWidth: len_u32.max(4),
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let mut stage = None;
    // SAFETY: valid staging buffer descriptor on live D3D11 device.
    unsafe { device.CreateBuffer(&sdesc, None, Some(&mut stage)) }
        .map_err(|e| win_err("ID3D11Device::CreateBuffer(copy stage)", &e))?;
    let stage = stage.ok_or_else(|| Error::internal("CreateBuffer(stage) returned null"))?;
    let bx = D3D11_BOX {
        left: src_offset as u32,
        top: 0,
        front: 0,
        right: (src_offset + size) as u32,
        bottom: 1,
        back: 1,
    };
    // SAFETY: copies validated range into temporary staging buffer.
    unsafe {
        ctx.CopySubresourceRegion(&stage, 0, 0, 0, 0, &s.0, 0, Some(&bx));
    }
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: maps temporary `D3D11_USAGE_STAGING` buffer for CPU read.
    unsafe { ctx.Map(&stage, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }
        .map_err(|e| win_err("ID3D11DeviceContext::Map(copy stage)", &e))?;
    let mut buf = vec![0u8; len];
    if !mapped.pData.is_null() {
        // SAFETY: `mapped.pData` holds at least `len` bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(mapped.pData.cast::<u8>(), buf.as_mut_ptr(), len);
        }
    }
    // SAFETY: pairs with `Map` above.
    unsafe {
        ctx.Unmap(&stage, 0);
    }
    Ok(buf)
}

fn copy_buffer_into_uniform(
    device: &ID3D11Device,
    ctx: &ID3D11DeviceContext,
    src: &Buffer,
    src_offset: u64,
    dst: &Buffer,
    dst_offset: u64,
    size: u64,
) -> Result<(), Error> {
    let dst_start = usize::try_from(dst_offset)
        .map_err(|_| Error::validation("copy_buffer dst_offset overflows usize"))?;
    let bytes = read_buffer_slice(device, ctx, src, src_offset, size)?;

    let d = dst
        .0
        .raw
        .as_ref()
        .ok_or_else(|| Error::validation("copy_buffer destination is invalid"))?;
    let mut shadow_guard = lock(&dst.0.shadow);
    let shadow = shadow_guard.get_or_insert_with(|| vec![0u8; dst.alloc_size() as usize]);
    let dst_end = dst_start
        .checked_add(bytes.len())
        .ok_or_else(|| Error::validation("copy_buffer dst range overflows usize"))?;
    if dst_end > shadow.len() {
        return Err(Error::validation("copy_buffer exceeds uniform buffer size"));
    }
    shadow[dst_start..dst_end].copy_from_slice(&bytes);
    upload_constant_buffer(ctx, &d.0, shadow);
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors TextureToBuffer command fields directly"
)]
fn copy_texture_to_staging_buffer(
    device: &ID3D11Device,
    ctx: &ID3D11DeviceContext,
    src: &Texture,
    mip_level: u32,
    origin: Origin3d,
    dst: &Buffer,
    dst_offset: u64,
    bytes_per_row: u32,
    rows_per_image: u32,
    size: Extent3d,
) -> Result<(), Error> {
    if size.width == 0 || size.height == 0 {
        return Ok(());
    }
    let raw_src = src
        .0
        .raw
        .as_ref()
        .ok_or_else(|| Error::validation("texture_to_buffer source is invalid"))?;
    let raw_dst = dst
        .0
        .raw
        .as_ref()
        .ok_or_else(|| Error::validation("texture_to_buffer destination is invalid"))?;
    let layers = size.depth_or_array_layers.max(1);
    let block = src.format().block_size();
    let copy_row_bytes = (size.width as usize)
        .checked_mul(block as usize)
        .ok_or_else(|| Error::validation("texture_to_buffer row byte width overflows"))?;
    let dst_base = usize::try_from(dst_offset)
        .map_err(|_| Error::validation("texture_to_buffer dst_offset overflows usize"))?;
    let dst_bpr = bytes_per_row as usize;
    let dst_bpi = dst_bpr
        .checked_mul(rows_per_image as usize)
        .ok_or_else(|| Error::validation("texture_to_buffer bytes_per_image overflows usize"))?;

    let stage_desc = D3D11_TEXTURE2D_DESC {
        Width: size.width,
        Height: size.height,
        MipLevels: 1,
        ArraySize: layers,
        Format: dxgi_format(src.format()),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut stage = None;
    // SAFETY: valid staging 2D texture descriptor on live device.
    unsafe { device.CreateTexture2D(&stage_desc, None, Some(&mut stage)) }
        .map_err(|e| win_err("ID3D11Device::CreateTexture2D(staging readback)", &e))?;
    let stage = stage.ok_or_else(|| Error::internal("CreateTexture2D(staging) returned null"))?;

    let src_box = D3D11_BOX {
        left: origin.x,
        top: origin.y,
        front: 0,
        right: origin.x + size.width,
        bottom: origin.y + size.height,
        back: 1,
    };
    for z in 0..layers {
        let src_sub = src.subresource_index(mip_level, origin.z + z);
        // SAFETY: copies GPU texture slice into CPU-readable staging texture.
        unsafe {
            ctx.CopySubresourceRegion(&stage, z, 0, 0, 0, &raw_src.0, src_sub, Some(&src_box));
        }
    }

    let is_map_read = dst.usage().contains(BufferUsages::MAP_READ);
    let is_uniform = dst.usage().contains(BufferUsages::UNIFORM);
    let dst_len = usize::try_from(dst.size())
        .map_err(|_| Error::validation("texture_to_buffer dst size overflows usize"))?;

    let copy_into = |target: &mut [u8]| -> Result<Vec<(usize, usize)>, Error> {
        let mut written_ranges: Vec<(usize, usize)> = Vec::new();
        for z in 0..layers {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            // SAFETY: maps staging texture for CPU read.
            unsafe { ctx.Map(&stage, z, D3D11_MAP_READ, 0, Some(&mut mapped)) }
                .map_err(|e| win_err("ID3D11DeviceContext::Map(staging texture)", &e))?;
            let src_pitch = mapped.RowPitch as usize;
            let slice_res = (|| -> Result<(), Error> {
                if mapped.pData.is_null() {
                    return Err(Error::internal(
                        "ID3D11DeviceContext::Map returned null pData",
                    ));
                }
                let slice_base = (z as usize)
                    .checked_mul(dst_bpi)
                    .and_then(|o| dst_base.checked_add(o))
                    .ok_or_else(|| {
                        Error::validation("texture_to_buffer destination slice offset overflows")
                    })?;
                for y in 0..size.height as usize {
                    let row_off = y.checked_mul(dst_bpr).ok_or_else(|| {
                        Error::validation("texture_to_buffer destination row offset overflows")
                    })?;
                    let dst_at = slice_base.checked_add(row_off).ok_or_else(|| {
                        Error::validation("texture_to_buffer destination offset overflows")
                    })?;
                    let dst_end = dst_at.checked_add(copy_row_bytes).ok_or_else(|| {
                        Error::validation("texture_to_buffer destination end overflows")
                    })?;
                    let src_at = y.checked_mul(src_pitch).ok_or_else(|| {
                        Error::validation("texture_to_buffer source row offset overflows")
                    })?;
                    if dst_end > target.len() {
                        return Err(Error::validation(
                            "texture_to_buffer destination range exceeds buffer size",
                        ));
                    }
                    // SAFETY: `mapped.pData` has `size.height` rows of `src_pitch >= copy_row_bytes`
                    // bytes, and `dst_at..dst_end` is validated within `target`.
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            mapped.pData.cast::<u8>().add(src_at),
                            target.as_mut_ptr().add(dst_at),
                            copy_row_bytes,
                        );
                    }
                    if let Some(last) = written_ranges.last_mut()
                        && last.1 == dst_at
                    {
                        last.1 = dst_end;
                    } else {
                        written_ranges.push((dst_at, dst_end));
                    }
                }
                Ok(())
            })();
            // SAFETY: pairs with `Map` above on every exit path.
            unsafe {
                ctx.Unmap(&stage, z);
            }
            slice_res?;
        }
        Ok(written_ranges)
    };

    if is_map_read {
        let mut shadow_guard = lock(&dst.0.shadow);
        let mut next_shadow = if let Some(existing) = shadow_guard.as_ref() {
            existing.clone()
        } else {
            let mut init = vec![0u8; dst_len];
            if dst_len > 0 {
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                // SAFETY: `raw_dst` is `D3D11_USAGE_STAGING` with `D3D11_CPU_ACCESS_READ`.
                unsafe { ctx.Map(&raw_dst.0, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }
                    .map_err(|e| win_err("ID3D11DeviceContext::Map(MAP_READ shadow init)", &e))?;
                if !mapped.pData.is_null() {
                    // SAFETY: `mapped.pData` holds at least `dst_len` bytes until `Unmap`.
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            mapped.pData.cast::<u8>(),
                            init.as_mut_ptr(),
                            dst_len,
                        );
                    }
                }
                // SAFETY: pairs with `Map` above.
                unsafe {
                    ctx.Unmap(&raw_dst.0, 0);
                }
            }
            init
        };
        let _ = copy_into(&mut next_shadow)?;
        *shadow_guard = Some(next_shadow);
    } else if is_uniform {
        let mut shadow_guard = lock(&dst.0.shadow);
        let mut next_shadow = shadow_guard
            .as_ref()
            .cloned()
            .unwrap_or_else(|| vec![0u8; dst.alloc_size() as usize]);
        let _ = copy_into(&mut next_shadow)?;
        // SAFETY: full constant-buffer upload commits only after every layer succeeds.
        unsafe {
            ctx.UpdateSubresource(&raw_dst.0, 0, None, next_shadow.as_ptr().cast(), 0, 0);
        }
        *shadow_guard = Some(next_shadow);
    } else {
        let mut tmp = vec![0u8; dst_len];
        let ranges = copy_into(&mut tmp)?;
        for (start, end) in ranges {
            if start < end {
                let bx = D3D11_BOX {
                    left: u32::try_from(start).map_err(|_| {
                        Error::validation("texture_to_buffer dst offset exceeds u32")
                    })?,
                    top: 0,
                    front: 0,
                    right: u32::try_from(end)
                        .map_err(|_| Error::validation("texture_to_buffer dst end exceeds u32"))?,
                    bottom: 1,
                    back: 1,
                };
                // SAFETY: uploads only written pixel range into `D3D11_USAGE_DEFAULT` buffer.
                unsafe {
                    ctx.UpdateSubresource(
                        &raw_dst.0,
                        0,
                        Some(&bx),
                        tmp[start..end].as_ptr().cast(),
                        0,
                        0,
                    );
                }
            }
        }
    }
    Ok(())
}

/// Guarantees SRV slots and OM render targets are unbound on every exit path
/// of `execute_pass`, preventing SRV/RTV hazard warnings and lingering back-buffer
/// COM references before `IDXGISwapChain::ResizeBuffers`.
struct PassCleanupGuard<'a>(&'a ID3D11DeviceContext);

impl Drop for PassCleanupGuard<'_> {
    fn drop(&mut self) {
        let null_srvs: [Option<ID3D11ShaderResourceView>; 8] = Default::default();
        // SAFETY: clearing shader resource slots and OM render target on live context.
        unsafe {
            self.0.VSSetShaderResources(0, Some(&null_srvs));
            self.0.PSSetShaderResources(0, Some(&null_srvs));
            self.0.OMSetRenderTargets(None, None);
        }
    }
}

fn execute_pass(
    device: &ID3D11Device,
    ctx: &ID3D11DeviceContext,
    ctx1: Option<&ID3D11DeviceContext1>,
    cb_offsetting: bool,
    pass: &PassRecord,
) -> Result<(), Error> {
    let rtv = pass
        .target
        .0
        .rtv
        .as_ref()
        .ok_or_else(|| Error::validation("render pass target lacks RENDER_ATTACHMENT"))?;

    let rtvs: [Option<ID3D11RenderTargetView>; 1] = [Some(rtv.0.clone())];
    // SAFETY: binding a single live RTV with no depth/stencil view.
    unsafe {
        ctx.OMSetRenderTargets(Some(&rtvs), None);
    }
    let _cleanup = PassCleanupGuard(ctx);

    if let Some(clear) = pass.clear {
        // SAFETY: clearing the bound RTV.
        unsafe {
            ctx.ClearRenderTargetView(&rtv.0, &clear);
        }
    }

    let tw = pass.target.0.texture.width();
    let th = pass.target.0.texture.height();
    let default_vp = [D3D11_VIEWPORT {
        TopLeftX: 0.0,
        TopLeftY: 0.0,
        Width: tw as f32,
        Height: th as f32,
        MinDepth: 0.0,
        MaxDepth: 1.0,
    }];
    let default_scissor = [RECT {
        left: 0,
        top: 0,
        right: tw.min(i32::MAX as u32) as i32,
        bottom: th.min(i32::MAX as u32) as i32,
    }];
    // SAFETY: setting default full-target viewport and scissor rect.
    unsafe {
        ctx.RSSetViewports(Some(&default_vp));
        ctx.RSSetScissorRects(Some(&default_scissor));
    }

    let mut current_pipeline: Option<&RenderPipeline> = None;
    let mut pending_groups: [Option<(&BindGroup, &[u32])>; 4] = [None; 4];
    let mut pending_vertices: [Option<(&Buffer, u64)>; MAX_VERTEX_SLOTS as usize] =
        [None; MAX_VERTEX_SLOTS as usize];

    for cmd in &pass.cmds {
        match cmd {
            PassCmd::Pipeline(pipe) => {
                let vs =
                    pipe.0.vs.as_ref().ok_or_else(|| {
                        Error::validation("invalid render pipeline vertex shader")
                    })?;
                let ps = pipe
                    .0
                    .ps
                    .as_ref()
                    .ok_or_else(|| Error::validation("invalid render pipeline pixel shader"))?;
                let blend = pipe
                    .0
                    .blend
                    .as_ref()
                    .ok_or_else(|| Error::validation("invalid render pipeline blend state"))?;
                let rast =
                    pipe.0.rasterizer.as_ref().ok_or_else(|| {
                        Error::validation("invalid render pipeline rasterizer state")
                    })?;
                // SAFETY: all pipeline COM objects are live and retained in `pass`.
                unsafe {
                    ctx.VSSetShader(&vs.0, None);
                    ctx.PSSetShader(&ps.0, None);
                    if let Some(il) = &pipe.0.input_layout {
                        ctx.IASetInputLayout(&il.0);
                    } else {
                        ctx.IASetInputLayout(None);
                    }
                    ctx.IASetPrimitiveTopology(d3d_topology(pipe.0.topology));
                    ctx.OMSetBlendState(&blend.0, None, 0xFFFF_FFFF);
                    ctx.RSSetState(&rast.0);
                }
                current_pipeline = Some(pipe);
                for (g, slot) in pending_groups.iter().enumerate() {
                    if let Some((bg, offs)) = slot
                        && pipe
                            .0
                            .group_layouts
                            .get(g)
                            .is_some_and(|expected| bg.0.layout.is_compatible_with(expected))
                    {
                        bind_group(device, ctx, ctx1, cb_offsetting, pipe, g as u32, bg, offs)?;
                    }
                }
                for (slot, v) in pending_vertices.iter().enumerate() {
                    if let Some((buf, off)) = v {
                        bind_vertex_buffer(ctx, pipe, slot as u32, buf, *off)?;
                    }
                }
            }
            PassCmd::Group(group_idx, bg, offs) => {
                if let Some(slot) = pending_groups.get_mut(*group_idx as usize) {
                    *slot = Some((bg, offs.as_slice()));
                }
                if let Some(pipe) = current_pipeline
                    && pipe
                        .0
                        .group_layouts
                        .get(*group_idx as usize)
                        .is_some_and(|expected| bg.0.layout.is_compatible_with(expected))
                {
                    bind_group(device, ctx, ctx1, cb_offsetting, pipe, *group_idx, bg, offs)?;
                }
            }
            PassCmd::Vertex(slot, buf, off) => {
                if let Some(entry) = pending_vertices.get_mut(*slot as usize) {
                    *entry = Some((buf, *off));
                }
                if let Some(pipe) = current_pipeline {
                    bind_vertex_buffer(ctx, pipe, *slot, buf, *off)?;
                }
            }
            PassCmd::Viewport([x, y, w, h, min_d, max_d]) => {
                let vp = [D3D11_VIEWPORT {
                    TopLeftX: *x,
                    TopLeftY: *y,
                    Width: *w,
                    Height: *h,
                    MinDepth: *min_d,
                    MaxDepth: *max_d,
                }];
                // SAFETY: valid D3D11 viewport array of length 1.
                unsafe {
                    ctx.RSSetViewports(Some(&vp));
                }
            }
            PassCmd::Scissor([x, y, w, h]) => {
                let rect = [RECT {
                    left: (*x).min(i32::MAX as u32) as i32,
                    top: (*y).min(i32::MAX as u32) as i32,
                    right: x.saturating_add(*w).min(i32::MAX as u32) as i32,
                    bottom: y.saturating_add(*h).min(i32::MAX as u32) as i32,
                }];
                // SAFETY: valid scissor RECT array of length 1.
                unsafe {
                    ctx.RSSetScissorRects(Some(&rect));
                }
            }
            PassCmd::Draw {
                vertices,
                instances,
            } => {
                if current_pipeline.is_none() {
                    continue;
                }
                let vcount = vertices.end.saturating_sub(vertices.start);
                let icount = instances.end.saturating_sub(instances.start);
                if vcount == 0 || icount == 0 {
                    continue;
                }
                // SAFETY: pipeline, RTV, and resources are bound on the immediate context.
                unsafe {
                    if instances.start == 0 && icount == 1 {
                        ctx.Draw(vcount, vertices.start);
                    } else {
                        ctx.DrawInstanced(vcount, icount, vertices.start, instances.start);
                    }
                }
            }
        }
    }

    Ok(())
}

fn bind_vertex_buffer(
    ctx: &ID3D11DeviceContext,
    pipe: &RenderPipeline,
    slot: u32,
    buf: &Buffer,
    offset: u64,
) -> Result<(), Error> {
    let Some(&stride) = pipe.0.vertex_strides.get(slot as usize) else {
        return Ok(());
    };
    let raw = buf
        .0
        .raw
        .as_ref()
        .ok_or_else(|| Error::validation("cannot bind an invalid vertex buffer"))?;
    let bufs: [Option<ID3D11Buffer>; 1] = [Some(raw.0.clone())];
    let strides = [stride];
    let offsets = [offset as u32];
    // SAFETY: valid single vertex buffer slot, stride, and offset.
    unsafe {
        ctx.IASetVertexBuffers(
            slot,
            1,
            Some(bufs.as_ptr()),
            Some(strides.as_ptr()),
            Some(offsets.as_ptr()),
        );
    }
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "passes device/context state plus pipeline, group index, bind group, and offsets"
)]
fn bind_group(
    device: &ID3D11Device,
    ctx: &ID3D11DeviceContext,
    ctx1: Option<&ID3D11DeviceContext1>,
    cb_offsetting: bool,
    pipe: &RenderPipeline,
    group_idx: u32,
    bg: &BindGroup,
    dynamic_offsets: &[u32],
) -> Result<(), Error> {
    for (binding, bound) in &bg.0.entries {
        let Some(resolved) = pipe.0.bindings.resolve(group_idx, *binding) else {
            continue;
        };
        let reg = resolved.slot;
        let vis = resolved.visibility;
        match (resolved.kind, bound) {
            (
                BindingKind::UniformBuffer,
                Bound::Buffer {
                    buffer,
                    offset,
                    size,
                },
            ) => {
                let total_offset = resolve_dynamic_offset(
                    &bg.0.layout.0.dynamic_bindings,
                    *binding,
                    dynamic_offsets,
                    *offset,
                )?;
                let raw =
                    buffer.0.raw.as_ref().ok_or_else(|| {
                        Error::validation("cannot bind an invalid uniform buffer")
                    })?;
                if total_offset > 0 {
                    let slice_bytes =
                        size.unwrap_or_else(|| buffer.size().saturating_sub(total_offset));
                    let (first_c, num_c) = d3d11_constant_buffer_range(total_offset, slice_bytes)
                        .map_err(Error::validation)?;
                    if let Some(c1) = ctx1.filter(|_| cb_offsetting) {
                        let bufs: [Option<ID3D11Buffer>; 1] = [Some(raw.0.clone())];
                        let first = [first_c];
                        let num = [num_c];
                        // SAFETY: `first` and `num` are 16-constant (256-byte) aligned per D3D11.1 spec.
                        unsafe {
                            if vis.contains(ShaderStages::VERTEX) {
                                c1.VSSetConstantBuffers1(
                                    reg,
                                    1,
                                    Some(bufs.as_ptr()),
                                    Some(first.as_ptr()),
                                    Some(num.as_ptr()),
                                );
                            }
                            if vis.contains(ShaderStages::FRAGMENT) {
                                c1.PSSetConstantBuffers1(
                                    reg,
                                    1,
                                    Some(bufs.as_ptr()),
                                    Some(first.as_ptr()),
                                    Some(num.as_ptr()),
                                );
                            }
                        }
                    } else {
                        let byte_width = d3d11_constant_buffer_byte_width(slice_bytes)
                            .and_then(|n| u32::try_from(n).ok())
                            .ok_or_else(|| {
                                Error::validation(
                                    "D3D11 constant buffer fallback slice overflows u32",
                                )
                            })?;
                        let mut slice_buf = vec![0u8; byte_width as usize];
                        if let Some(shadow) = lock(&buffer.0.shadow).as_ref() {
                            let start = usize::try_from(total_offset).unwrap_or(shadow.len());
                            if start < shadow.len() {
                                let avail = (shadow.len() - start).min(slice_buf.len());
                                slice_buf[..avail].copy_from_slice(&shadow[start..start + avail]);
                            }
                        }
                        let bdesc = D3D11_BUFFER_DESC {
                            ByteWidth: byte_width,
                            Usage: D3D11_USAGE_DEFAULT,
                            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                            CPUAccessFlags: 0,
                            MiscFlags: 0,
                            StructureByteStride: 0,
                        };
                        let init = D3D11_SUBRESOURCE_DATA {
                            pSysMem: slice_buf.as_ptr().cast(),
                            SysMemPitch: 0,
                            SysMemSlicePitch: 0,
                        };
                        let mut staged_cb = None;
                        // SAFETY: `bdesc` and `init` describe a valid constant buffer of `byte_width` bytes.
                        unsafe { device.CreateBuffer(&bdesc, Some(&init), Some(&mut staged_cb)) }
                            .map_err(|e| {
                            win_err("ID3D11Device::CreateBuffer(CB offset fallback)", &e)
                        })?;
                        let staged_cb = staged_cb.ok_or_else(|| {
                            Error::internal("CreateBuffer(CB offset fallback) returned null")
                        })?;
                        let bufs: [Option<ID3D11Buffer>; 1] = [Some(staged_cb)];
                        // SAFETY: binding staged constant buffer at slot `reg` with offset 0.
                        unsafe {
                            if vis.contains(ShaderStages::VERTEX) {
                                ctx.VSSetConstantBuffers(reg, Some(&bufs));
                            }
                            if vis.contains(ShaderStages::FRAGMENT) {
                                ctx.PSSetConstantBuffers(reg, Some(&bufs));
                            }
                        }
                    }
                } else {
                    let bufs: [Option<ID3D11Buffer>; 1] = [Some(raw.0.clone())];
                    // SAFETY: binding constant buffer at slot `reg` with offset 0.
                    unsafe {
                        if vis.contains(ShaderStages::VERTEX) {
                            ctx.VSSetConstantBuffers(reg, Some(&bufs));
                        }
                        if vis.contains(ShaderStages::FRAGMENT) {
                            ctx.PSSetConstantBuffers(reg, Some(&bufs));
                        }
                    }
                }
            }
            (BindingKind::Texture2d | BindingKind::Texture2dArray, Bound::View(view)) => {
                let srv = view
                    .0
                    .srv
                    .as_ref()
                    .ok_or_else(|| Error::validation("texture view lacks TEXTURE_BINDING"))?;
                let srvs: [Option<ID3D11ShaderResourceView>; 1] = [Some(srv.0.clone())];
                // SAFETY: binding live SRV at slot `reg`.
                unsafe {
                    if vis.contains(ShaderStages::VERTEX) {
                        ctx.VSSetShaderResources(reg, Some(&srvs));
                    }
                    if vis.contains(ShaderStages::FRAGMENT) {
                        ctx.PSSetShaderResources(reg, Some(&srvs));
                    }
                }
            }
            (BindingKind::Sampler, Bound::Sampler(sampler)) => {
                let raw = sampler
                    .0
                    .raw
                    .as_ref()
                    .ok_or_else(|| Error::validation("cannot bind an invalid sampler"))?;
                let samps: [Option<ID3D11SamplerState>; 1] = [Some(raw.0.clone())];
                // SAFETY: binding live sampler state at slot `reg`.
                unsafe {
                    if vis.contains(ShaderStages::VERTEX) {
                        ctx.VSSetSamplers(reg, Some(&samps));
                    }
                    if vis.contains(ShaderStages::FRAGMENT) {
                        ctx.PSSetSamplers(reg, Some(&samps));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}
