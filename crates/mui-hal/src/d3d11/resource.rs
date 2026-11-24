//! D3D11 resources (`ID3D11Buffer`, `ID3D11Texture2D`, SRV/RTV, samplers,
//! shaders via precompiled DXBC or `D3DCompile`, input layouts, blend/rasterizer
//! states, and bind groups).

use std::ffi::CString;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use mui_shaders::{EntryPointInfo, ShaderId, Stage};
use windows::Win32::Foundation::{FALSE, TRUE};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{
    D3D_PRIMITIVE_TOPOLOGY, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
    D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP, D3D_SRV_DIMENSION_TEXTURE2D,
    D3D_SRV_DIMENSION_TEXTURE2DARRAY, ID3DBlob,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_FLAG, D3D11_BIND_RENDER_TARGET,
    D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_VERTEX_BUFFER, D3D11_BLEND, D3D11_BLEND_DESC,
    D3D11_BLEND_DEST_ALPHA, D3D11_BLEND_INV_DEST_ALPHA, D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ONE,
    D3D11_BLEND_OP, D3D11_BLEND_OP_ADD, D3D11_BLEND_SRC_ALPHA, D3D11_BLEND_ZERO, D3D11_BUFFER_DESC,
    D3D11_COLOR_WRITE_ENABLE_ALPHA, D3D11_COLOR_WRITE_ENABLE_BLUE, D3D11_COLOR_WRITE_ENABLE_GREEN,
    D3D11_COLOR_WRITE_ENABLE_RED, D3D11_COMPARISON_NEVER, D3D11_CPU_ACCESS_READ, D3D11_CULL_NONE,
    D3D11_FILL_SOLID, D3D11_FILTER_MIN_LINEAR_MAG_MIP_POINT, D3D11_FILTER_MIN_MAG_MIP_LINEAR,
    D3D11_FILTER_MIN_MAG_MIP_POINT, D3D11_FILTER_MIN_POINT_MAG_LINEAR_MIP_POINT,
    D3D11_INPUT_ELEMENT_DESC, D3D11_INPUT_PER_INSTANCE_DATA, D3D11_INPUT_PER_VERTEX_DATA,
    D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_RASTERIZER_DESC,
    D3D11_RENDER_TARGET_BLEND_DESC, D3D11_SAMPLER_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC,
    D3D11_SHADER_RESOURCE_VIEW_DESC_0, D3D11_TEX2D_ARRAY_SRV, D3D11_TEX2D_SRV,
    D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
    ID3D11BlendState, ID3D11Buffer, ID3D11InputLayout, ID3D11PixelShader, ID3D11RasterizerState,
    ID3D11RenderTargetView, ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11Texture2D,
    ID3D11VertexShader,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_R8_UNORM,
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, DXGI_FORMAT_R16G16B16A16_FLOAT,
    DXGI_FORMAT_R32G32_FLOAT, DXGI_FORMAT_R32G32B32A32_FLOAT, DXGI_SAMPLE_DESC,
};
use windows::core::PCSTR;

use super::{ComSend, Device, DeviceShared, Error, MapCallback, lock, win_err};
use crate::binding::{
    BoundKind, HLSL_VERTEX_SEMANTIC, ShaderBindingTable, d3d11_constant_buffer_byte_width,
    dynamic_offset_bindings, resolve_entry_point, validate_bind_group_entries,
};
use crate::types::{
    AddressMode, BindGroupLayoutEntry, BlendFactor, BlendOperation, BufferDescriptor, BufferUsages,
    ColorTargetState, ColorWrites, FilterMode, PrimitiveTopology, QuerySetDescriptor,
    SamplerDescriptor, TextureDescriptor, TextureFormat, TextureUsages, TextureViewDimension,
    VertexBufferLayout, VertexFormat, VertexStepMode,
};

pub(crate) fn dxgi_format(f: TextureFormat) -> DXGI_FORMAT {
    match f {
        TextureFormat::R8Unorm => DXGI_FORMAT_R8_UNORM,
        TextureFormat::Rgba8Unorm => DXGI_FORMAT_R8G8B8A8_UNORM,
        TextureFormat::Rgba8UnormSrgb => DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        TextureFormat::Bgra8Unorm => DXGI_FORMAT_B8G8R8A8_UNORM,
        TextureFormat::Bgra8UnormSrgb => DXGI_FORMAT_B8G8R8A8_UNORM_SRGB,
        TextureFormat::Rgba16Float => DXGI_FORMAT_R16G16B16A16_FLOAT,
        TextureFormat::Rgba32Float => DXGI_FORMAT_R32G32B32A32_FLOAT,
    }
}

pub(crate) fn d3d_topology(t: PrimitiveTopology) -> D3D_PRIMITIVE_TOPOLOGY {
    match t {
        PrimitiveTopology::TriangleList => D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
        PrimitiveTopology::TriangleStrip => D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
    }
}

fn d3d_blend(f: BlendFactor) -> D3D11_BLEND {
    match f {
        BlendFactor::Zero => D3D11_BLEND_ZERO,
        BlendFactor::One => D3D11_BLEND_ONE,
        BlendFactor::SrcAlpha => D3D11_BLEND_SRC_ALPHA,
        BlendFactor::OneMinusSrcAlpha => D3D11_BLEND_INV_SRC_ALPHA,
        BlendFactor::DstAlpha => D3D11_BLEND_DEST_ALPHA,
        BlendFactor::OneMinusDstAlpha => D3D11_BLEND_INV_DEST_ALPHA,
    }
}

fn d3d_blend_op(op: BlendOperation) -> D3D11_BLEND_OP {
    match op {
        BlendOperation::Add => D3D11_BLEND_OP_ADD,
    }
}

fn d3d_write_mask(mask: ColorWrites) -> u8 {
    let mut out = 0u8;
    if mask.contains(ColorWrites::RED) {
        out |= D3D11_COLOR_WRITE_ENABLE_RED.0 as u8;
    }
    if mask.contains(ColorWrites::GREEN) {
        out |= D3D11_COLOR_WRITE_ENABLE_GREEN.0 as u8;
    }
    if mask.contains(ColorWrites::BLUE) {
        out |= D3D11_COLOR_WRITE_ENABLE_BLUE.0 as u8;
    }
    if mask.contains(ColorWrites::ALPHA) {
        out |= D3D11_COLOR_WRITE_ENABLE_ALPHA.0 as u8;
    }
    out
}

fn d3d_vertex_format(f: VertexFormat) -> DXGI_FORMAT {
    match f {
        VertexFormat::Float32x2 => DXGI_FORMAT_R32G32_FLOAT,
        VertexFormat::Float32x4 => DXGI_FORMAT_R32G32B32A32_FLOAT,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MapState {
    Unmapped,
    Pending(u64),
    Mapped,
}

pub(crate) struct BufferInner {
    device: Arc<DeviceShared>,
    pub raw: Option<ComSend<ID3D11Buffer>>,
    size: u64,
    alloc_size: u32,
    usage: BufferUsages,
    /// CPU shadow copy for `UNIFORM` buffers (so `UpdateSubresource` always
    /// uploads the full constant buffer cleanly on D3D11.0 and D3D11.1) and
    /// for `MAP_READ` buffers (filled on `copy_mapped_range`).
    pub shadow: Mutex<Option<Vec<u8>>>,
    map_state: Mutex<MapState>,
    next_map_token: AtomicU64,
}

#[derive(Clone)]
pub struct Buffer(pub(crate) Arc<BufferInner>);

impl Buffer {
    pub fn new(device: &Device, desc: &BufferDescriptor<'_>) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("D3D11 device is lost"));
        }
        if desc.size > device.limits().max_buffer_size {
            return Err(Error::validation(format!(
                "buffer size {} exceeds D3D11 max_buffer_size {}",
                desc.size,
                device.limits().max_buffer_size
            )));
        }
        let is_staging = desc.usage.contains(BufferUsages::MAP_READ);
        if is_staging
            && !(desc.usage - (BufferUsages::MAP_READ | BufferUsages::COPY_DST)).is_empty()
        {
            return Err(Error::validation(
                "D3D11 MAP_READ buffers may only be combined with COPY_DST",
            ));
        }
        let is_uniform = desc.usage.contains(BufferUsages::UNIFORM);
        let is_vertex = desc.usage.contains(BufferUsages::VERTEX);
        if is_uniform && is_vertex {
            return Err(Error::validation(
                "D3D11 does not allow combining UNIFORM and VERTEX buffer usages",
            ));
        }
        let byte_width = if is_uniform {
            d3d11_constant_buffer_byte_width(desc.size).ok_or_else(|| {
                Error::validation("D3D11 constant buffer size overflows 256-byte alignment")
            })?
        } else {
            desc.size.max(4)
        };
        let byte_width_u32 = u32::try_from(byte_width)
            .map_err(|_| Error::out_of_memory("buffer size exceeds D3D11 u32 ByteWidth"))?;

        let (usage, bind_flags, cpu_access) = if is_staging {
            (D3D11_USAGE_STAGING, 0u32, D3D11_CPU_ACCESS_READ.0 as u32)
        } else {
            let mut bind = D3D11_BIND_FLAG(0);
            if is_uniform {
                bind |= D3D11_BIND_CONSTANT_BUFFER;
            }
            if is_vertex {
                bind |= D3D11_BIND_VERTEX_BUFFER;
            }
            // Staging-free default copy buffer needs a valid bind flag or staging usage;
            // D3D11 permits `BindFlags = 0` on `D3D11_USAGE_STAGING`, while `DEFAULT`
            // copy-only buffers can safely set `D3D11_BIND_VERTEX_BUFFER`.
            if bind.0 == 0 {
                bind = D3D11_BIND_VERTEX_BUFFER;
            }
            (D3D11_USAGE_DEFAULT, bind.0 as u32, 0u32)
        };

        let bdesc = D3D11_BUFFER_DESC {
            ByteWidth: byte_width_u32,
            Usage: usage,
            BindFlags: bind_flags,
            CPUAccessFlags: cpu_access,
            MiscFlags: 0,
            StructureByteStride: 0,
        };
        let mut raw = None;
        // SAFETY: `bdesc` is a valid D3D11 buffer descriptor on a live device.
        unsafe { device.0.raw.CreateBuffer(&bdesc, None, Some(&mut raw)) }
            .map_err(|e| win_err("ID3D11Device::CreateBuffer", &e))?;
        let raw = raw.ok_or_else(|| Error::internal("CreateBuffer returned null"))?;

        let shadow = if is_uniform {
            Some(vec![0u8; byte_width_u32 as usize])
        } else {
            None
        };

        Ok(Self(Arc::new(BufferInner {
            device: device.0.clone(),
            raw: Some(ComSend(raw)),
            size: desc.size,
            alloc_size: byte_width_u32,
            usage: desc.usage,
            shadow: Mutex::new(shadow),
            map_state: Mutex::new(MapState::Unmapped),
            next_map_token: AtomicU64::new(1),
        })))
    }

    pub fn invalid(device: &Device, desc: &BufferDescriptor<'_>) -> Self {
        Self(Arc::new(BufferInner {
            device: device.0.clone(),
            raw: None,
            size: desc.size,
            alloc_size: 0,
            usage: desc.usage,
            shadow: Mutex::new(None),
            map_state: Mutex::new(MapState::Unmapped),
            next_map_token: AtomicU64::new(1),
        }))
    }

    pub fn is_valid(&self) -> bool {
        self.0.raw.is_some()
    }

    pub(crate) fn belongs_to(&self, device: &Arc<DeviceShared>) -> bool {
        Arc::ptr_eq(&self.0.device, device)
    }

    pub fn size(&self) -> u64 {
        self.0.size
    }

    pub fn alloc_size(&self) -> u32 {
        self.0.alloc_size
    }

    pub fn usage(&self) -> BufferUsages {
        self.0.usage
    }

    pub fn is_mapped_or_pending(&self) -> bool {
        *lock(&self.0.map_state) != MapState::Unmapped
    }

    pub(crate) fn try_mark_pending(&self, token: u64) -> bool {
        let mut state = lock(&self.0.map_state);
        if *state != MapState::Unmapped {
            return false;
        }
        *state = MapState::Pending(token);
        true
    }

    pub fn map_async(&self, offset: u64, size: u64, callback: MapCallback) {
        let device = Device(self.0.device.clone());
        if !self.is_valid() || !self.0.usage.contains(BufferUsages::MAP_READ) {
            device.errors().report(crate::Error::Validation {
                description: "map_async requires a valid MAP_READ buffer".into(),
            });
            callback(Err(crate::BufferAsyncError));
            return;
        }
        if offset.checked_add(size).is_none_or(|end| end > self.0.size) {
            device.errors().report(crate::Error::Validation {
                description: "map_async slice exceeds buffer bounds".into(),
            });
            callback(Err(crate::BufferAsyncError));
            return;
        }
        if self.is_mapped_or_pending() {
            callback(Err(crate::BufferAsyncError));
            return;
        }
        let token = self.0.next_map_token.fetch_add(1, Ordering::Relaxed);
        let this = self.clone();
        let wrapped: MapCallback = Box::new(move |res| {
            let mut state = lock(&this.0.map_state);
            let delivered = if *state == MapState::Pending(token) {
                *state = if res.is_ok() {
                    MapState::Mapped
                } else {
                    MapState::Unmapped
                };
                res
            } else {
                Err(crate::BufferAsyncError)
            };
            drop(state);
            callback(delivered);
        });
        if let Err((err, cb)) = device.begin_map_for_buffer(self, token, wrapped) {
            device.errors().report(err.into_hal());
            cb(Err(crate::BufferAsyncError));
        }
    }

    pub fn copy_mapped_range(
        &self,
        offset: u64,
        size: u64,
    ) -> Result<Vec<u8>, crate::BufferAsyncError> {
        let st = lock(&self.0.device.state);
        let state = lock(&self.0.map_state);
        if *state != MapState::Mapped {
            return Err(crate::BufferAsyncError);
        }
        let Some(raw) = &self.0.raw else {
            return Err(crate::BufferAsyncError);
        };
        let end = offset.checked_add(size).ok_or(crate::BufferAsyncError)?;
        if end > self.0.size {
            return Err(crate::BufferAsyncError);
        }
        let start = usize::try_from(offset).map_err(|_| crate::BufferAsyncError)?;
        let len = usize::try_from(size).map_err(|_| crate::BufferAsyncError)?;

        // If `TextureToBuffer` populated our CPU shadow (because D3D11 cannot
        // CopySubresourceRegion from Texture2D directly into a Buffer), read it.
        if let Some(shadow) = lock(&self.0.shadow).as_ref()
            && start + len <= shadow.len()
        {
            let out = shadow[start..start + len].to_vec();
            drop(state);
            drop(st);
            return Ok(out);
        }

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: buffer is `D3D11_USAGE_STAGING` with `D3D11_CPU_ACCESS_READ`,
        // we waited for GPU completion in `poll`, and `state` stays locked through
        // `Map`/`copy_nonoverlapping`/`Unmap` so `unmap()` cannot race mid-copy.
        unsafe {
            st.context
                .Map(&raw.0, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }
        .map_err(|_| crate::BufferAsyncError)?;
        let mut out = vec![0u8; len];
        if len > 0 && !mapped.pData.is_null() {
            // SAFETY: `mapped.pData` is valid for `self.0.size` bytes until `Unmap`.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    mapped.pData.cast::<u8>().add(start),
                    out.as_mut_ptr(),
                    len,
                );
            }
        }
        // SAFETY: pairs with `Map` above.
        unsafe {
            st.context.Unmap(&raw.0, 0);
        }
        drop(state);
        drop(st);
        Ok(out)
    }

    pub fn unmap(&self) {
        *lock(&self.0.map_state) = MapState::Unmapped;
    }
}

pub(crate) struct TextureInner {
    device: Arc<DeviceShared>,
    pub raw: Option<ComSend<ID3D11Texture2D>>,
    width: u32,
    height: u32,
    layers: u32,
    mip_level_count: u32,
    sample_count: u32,
    format: TextureFormat,
    usage: TextureUsages,
    swapchain_live: Option<Arc<AtomicBool>>,
}

#[derive(Clone)]
pub struct Texture(pub(crate) Arc<TextureInner>);

impl Texture {
    pub fn new(device: &Device, desc: &TextureDescriptor<'_>) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("D3D11 device is lost"));
        }
        if desc.size.width == 0 || desc.size.height == 0 {
            return Err(Error::validation("texture dimensions must be non-zero"));
        }
        if desc.mip_level_count > 1 {
            return Err(Error::validation(
                "native D3D11 textures support only one mip level (mip_level_count == 1)",
            ));
        }
        let layers = desc.size.depth_or_array_layers.max(1);
        let sample_count = desc.sample_count.max(1);
        let mut bind = D3D11_BIND_FLAG(0);
        if desc.usage.contains(TextureUsages::TEXTURE_BINDING) {
            bind |= D3D11_BIND_SHADER_RESOURCE;
        }
        if desc.usage.contains(TextureUsages::RENDER_ATTACHMENT) {
            bind |= D3D11_BIND_RENDER_TARGET;
        }
        let tdesc = D3D11_TEXTURE2D_DESC {
            Width: desc.size.width,
            Height: desc.size.height,
            MipLevels: 1,
            ArraySize: layers,
            Format: dxgi_format(desc.format),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: sample_count,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: bind.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut raw = None;
        // SAFETY: valid 2D texture descriptor on live D3D11 device.
        unsafe { device.0.raw.CreateTexture2D(&tdesc, None, Some(&mut raw)) }
            .map_err(|e| win_err("ID3D11Device::CreateTexture2D", &e))?;
        let raw = raw.ok_or_else(|| Error::internal("CreateTexture2D returned null"))?;
        Ok(Self(Arc::new(TextureInner {
            device: device.0.clone(),
            raw: Some(ComSend(raw)),
            width: desc.size.width,
            height: desc.size.height,
            layers,
            mip_level_count: 1,
            sample_count,
            format: desc.format,
            usage: desc.usage,
            swapchain_live: None,
        })))
    }

    pub(crate) fn from_swapchain(
        device: &Device,
        raw: ID3D11Texture2D,
        width: u32,
        height: u32,
        format: TextureFormat,
        usage: TextureUsages,
        live: Arc<AtomicBool>,
    ) -> Self {
        Self(Arc::new(TextureInner {
            device: device.0.clone(),
            raw: Some(ComSend(raw)),
            width,
            height,
            layers: 1,
            mip_level_count: 1,
            sample_count: 1,
            format,
            usage,
            swapchain_live: Some(live),
        }))
    }

    pub fn invalid(device: &Device, desc: &TextureDescriptor<'_>) -> Self {
        Self(Arc::new(TextureInner {
            device: device.0.clone(),
            raw: None,
            width: desc.size.width,
            height: desc.size.height,
            layers: desc.size.depth_or_array_layers.max(1),
            mip_level_count: desc.mip_level_count.max(1),
            sample_count: desc.sample_count.max(1),
            format: desc.format,
            usage: desc.usage,
            swapchain_live: None,
        }))
    }

    pub(crate) fn device(&self) -> Device {
        Device(self.0.device.clone())
    }

    pub fn is_valid(&self) -> bool {
        self.0.raw.is_some()
    }

    pub(crate) fn belongs_to(&self, device: &Arc<DeviceShared>) -> bool {
        Arc::ptr_eq(&self.0.device, device)
    }

    pub fn is_retired_swapchain(&self) -> bool {
        self.0
            .swapchain_live
            .as_ref()
            .is_some_and(|l| !l.load(Ordering::Acquire))
    }

    pub fn width(&self) -> u32 {
        self.0.width
    }

    pub fn height(&self) -> u32 {
        self.0.height
    }

    pub fn layers(&self) -> u32 {
        self.0.layers
    }

    pub fn mip_level_count(&self) -> u32 {
        self.0.mip_level_count
    }

    pub fn sample_count(&self) -> u32 {
        self.0.sample_count
    }

    pub(crate) fn subresource_index(&self, mip_level: u32, array_slice: u32) -> u32 {
        mip_level + array_slice * self.0.mip_level_count
    }

    pub fn format(&self) -> TextureFormat {
        self.0.format
    }

    pub fn usage(&self) -> TextureUsages {
        self.0.usage
    }

    pub fn create_view(
        &self,
        dimension: Option<TextureViewDimension>,
    ) -> Result<TextureView, Error> {
        let Some(raw) = &self.0.raw else {
            return Err(Error::validation(
                "cannot create view of an invalid texture",
            ));
        };
        let dim = dimension.unwrap_or(if self.0.layers > 1 {
            TextureViewDimension::D2Array
        } else {
            TextureViewDimension::D2
        });
        let srv = if self.0.usage.contains(TextureUsages::TEXTURE_BINDING) {
            let desc = match dim {
                TextureViewDimension::D2 => D3D11_SHADER_RESOURCE_VIEW_DESC {
                    Format: dxgi_format(self.0.format),
                    ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
                    Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture2D: D3D11_TEX2D_SRV {
                            MostDetailedMip: 0,
                            MipLevels: 1,
                        },
                    },
                },
                TextureViewDimension::D2Array => D3D11_SHADER_RESOURCE_VIEW_DESC {
                    Format: dxgi_format(self.0.format),
                    ViewDimension: D3D_SRV_DIMENSION_TEXTURE2DARRAY,
                    Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture2DArray: D3D11_TEX2D_ARRAY_SRV {
                            MostDetailedMip: 0,
                            MipLevels: 1,
                            FirstArraySlice: 0,
                            ArraySize: self.0.layers,
                        },
                    },
                },
            };
            let mut out = None;
            // SAFETY: `raw` was created with `D3D11_BIND_SHADER_RESOURCE`.
            unsafe {
                self.0
                    .device
                    .raw
                    .CreateShaderResourceView(&raw.0, Some(&desc), Some(&mut out))
            }
            .map_err(|e| win_err("ID3D11Device::CreateShaderResourceView", &e))?;
            out.map(ComSend)
        } else {
            None
        };

        let rtv = if self.0.usage.contains(TextureUsages::RENDER_ATTACHMENT) {
            let mut out = None;
            // SAFETY: `raw` was created with `D3D11_BIND_RENDER_TARGET`.
            unsafe {
                self.0
                    .device
                    .raw
                    .CreateRenderTargetView(&raw.0, None, Some(&mut out))
            }
            .map_err(|e| win_err("ID3D11Device::CreateRenderTargetView", &e))?;
            out.map(ComSend)
        } else {
            None
        };

        Ok(TextureView(Arc::new(TextureViewInner {
            texture: self.clone(),
            srv,
            rtv,
            valid: true,
        })))
    }
}

pub(crate) struct TextureViewInner {
    pub texture: Texture,
    pub srv: Option<ComSend<ID3D11ShaderResourceView>>,
    pub rtv: Option<ComSend<ID3D11RenderTargetView>>,
    pub valid: bool,
}

#[derive(Clone)]
pub struct TextureView(pub(crate) Arc<TextureViewInner>);

impl TextureView {
    pub fn invalid(texture: Texture) -> Self {
        Self(Arc::new(TextureViewInner {
            texture,
            srv: None,
            rtv: None,
            valid: false,
        }))
    }

    pub fn is_valid(&self) -> bool {
        self.0.valid && self.0.texture.is_valid()
    }

    pub(crate) fn belongs_to(&self, device: &Arc<DeviceShared>) -> bool {
        self.0.texture.belongs_to(device)
    }

    pub fn is_retired_swapchain(&self) -> bool {
        self.0.texture.is_retired_swapchain()
    }
}

pub(crate) struct SamplerInner {
    device: Arc<DeviceShared>,
    pub raw: Option<ComSend<ID3D11SamplerState>>,
}

#[derive(Clone)]
pub struct Sampler(pub(crate) Arc<SamplerInner>);

impl Sampler {
    pub fn new(device: &Device, desc: &SamplerDescriptor<'_>) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("D3D11 device is lost"));
        }
        let filter = match (desc.min_filter, desc.mag_filter) {
            (FilterMode::Nearest, FilterMode::Nearest) => D3D11_FILTER_MIN_MAG_MIP_POINT,
            (FilterMode::Nearest, FilterMode::Linear) => {
                D3D11_FILTER_MIN_POINT_MAG_LINEAR_MIP_POINT
            }
            (FilterMode::Linear, FilterMode::Nearest) => D3D11_FILTER_MIN_LINEAR_MAG_MIP_POINT,
            (FilterMode::Linear, FilterMode::Linear) => D3D11_FILTER_MIN_MAG_MIP_LINEAR,
        };
        let address = match (desc.address_mode_u, desc.address_mode_v) {
            (AddressMode::ClampToEdge, AddressMode::ClampToEdge) => D3D11_TEXTURE_ADDRESS_CLAMP,
        };
        let sdesc = D3D11_SAMPLER_DESC {
            Filter: filter,
            AddressU: address,
            AddressV: address,
            AddressW: address,
            MipLODBias: 0.0,
            MaxAnisotropy: 1,
            ComparisonFunc: D3D11_COMPARISON_NEVER,
            BorderColor: [0.0; 4],
            MinLOD: 0.0,
            MaxLOD: f32::MAX,
        };
        let mut raw = None;
        // SAFETY: valid sampler descriptor on live D3D11 device.
        unsafe { device.0.raw.CreateSamplerState(&sdesc, Some(&mut raw)) }
            .map_err(|e| win_err("ID3D11Device::CreateSamplerState", &e))?;
        let raw = raw.ok_or_else(|| Error::internal("CreateSamplerState returned null"))?;
        Ok(Self(Arc::new(SamplerInner {
            device: device.0.clone(),
            raw: Some(ComSend(raw)),
        })))
    }

    pub fn invalid(device: &Device) -> Self {
        Self(Arc::new(SamplerInner {
            device: device.0.clone(),
            raw: None,
        }))
    }

    pub fn is_valid(&self) -> bool {
        self.0.raw.is_some()
    }

    pub(crate) fn belongs_to(&self, device: &Arc<DeviceShared>) -> bool {
        Arc::ptr_eq(&self.0.device, device)
    }
}

pub(crate) struct BindGroupLayoutInner {
    device: Arc<DeviceShared>,
    pub entries: Vec<BindGroupLayoutEntry>,
    pub dynamic_bindings: Vec<u32>,
    pub valid: bool,
}

#[derive(Clone)]
pub struct BindGroupLayout(pub(crate) Arc<BindGroupLayoutInner>);

impl BindGroupLayout {
    pub fn new(device: &Device, entries: &[BindGroupLayoutEntry]) -> Result<Self, Error> {
        let mut sorted = entries.to_vec();
        sorted.sort_by_key(|e| e.binding);
        Ok(Self(Arc::new(BindGroupLayoutInner {
            device: device.0.clone(),
            dynamic_bindings: dynamic_offset_bindings(&sorted),
            entries: sorted,
            valid: true,
        })))
    }

    pub fn invalid(device: &Device) -> Self {
        Self(Arc::new(BindGroupLayoutInner {
            device: device.0.clone(),
            entries: Vec::new(),
            dynamic_bindings: Vec::new(),
            valid: false,
        }))
    }

    pub(crate) fn belongs_to(&self, device: &Arc<DeviceShared>) -> bool {
        Arc::ptr_eq(&self.0.device, device)
    }

    pub(crate) fn is_compatible_with(&self, other: &Self) -> bool {
        if !self.0.valid || !other.0.valid {
            return false;
        }
        Arc::ptr_eq(&self.0, &other.0)
            || (Arc::ptr_eq(&self.0.device, &other.0.device) && self.0.entries == other.0.entries)
    }
}

#[derive(Clone)]
pub enum Bound {
    Buffer {
        buffer: Buffer,
        offset: u64,
        size: Option<u64>,
    },
    View(TextureView),
    Sampler(Sampler),
}

pub(crate) struct BindGroupInner {
    device: Arc<DeviceShared>,
    pub layout: BindGroupLayout,
    pub entries: Vec<(u32, Bound)>,
    pub valid: bool,
}

#[derive(Clone)]
pub struct BindGroup(pub(crate) Arc<BindGroupInner>);

impl BindGroup {
    pub fn new(
        device: &Device,
        layout: &BindGroupLayout,
        entries: Vec<(u32, Bound)>,
    ) -> Result<Self, Error> {
        if !layout.belongs_to(&device.0) {
            return Err(Error::validation(
                "cannot create bind group from a layout belonging to another device",
            ));
        }
        if !layout.0.valid {
            return Err(Error::validation(
                "cannot create bind group from invalid layout",
            ));
        }
        let kinds: Vec<(u32, BoundKind)> = entries
            .iter()
            .map(|(binding, bound)| {
                let kind = match bound {
                    Bound::Buffer { .. } => BoundKind::Buffer,
                    Bound::View(_) => BoundKind::Texture,
                    Bound::Sampler(_) => BoundKind::Sampler,
                };
                (*binding, kind)
            })
            .collect();
        validate_bind_group_entries(&layout.0.entries, &kinds).map_err(Error::validation)?;
        for (_, bound) in &entries {
            match bound {
                Bound::Buffer {
                    buffer,
                    offset,
                    size,
                } => {
                    if !buffer.belongs_to(&device.0) {
                        return Err(Error::validation(
                            "bind group references a buffer from another device",
                        ));
                    }
                    if !buffer.is_valid() {
                        return Err(Error::validation("bind group references an invalid buffer"));
                    }
                    if !buffer.usage().contains(BufferUsages::UNIFORM) {
                        return Err(Error::validation("bind group buffer lacks UNIFORM usage"));
                    }
                    let end = offset.checked_add(size.unwrap_or(0)).ok_or_else(|| {
                        Error::validation("bind group buffer range overflows u64")
                    })?;
                    if end > buffer.size() {
                        return Err(Error::validation(
                            "bind group buffer range exceeds buffer size",
                        ));
                    }
                }
                Bound::View(view) => {
                    if !view.belongs_to(&device.0) {
                        return Err(Error::validation(
                            "bind group references a texture view from another device",
                        ));
                    }
                    if view.is_retired_swapchain() {
                        return Err(Error::validation(
                            "bind group references a retired swapchain image",
                        ));
                    }
                    if !view.is_valid() {
                        return Err(Error::validation(
                            "bind group references an invalid texture view",
                        ));
                    }
                    if view.0.srv.is_none() {
                        return Err(Error::validation(
                            "bind group texture view lacks TEXTURE_BINDING",
                        ));
                    }
                }
                Bound::Sampler(sampler) => {
                    if !sampler.belongs_to(&device.0) {
                        return Err(Error::validation(
                            "bind group references a sampler from another device",
                        ));
                    }
                    if !sampler.is_valid() {
                        return Err(Error::validation(
                            "bind group references an invalid sampler",
                        ));
                    }
                }
            }
        }
        Ok(Self(Arc::new(BindGroupInner {
            device: device.0.clone(),
            layout: layout.clone(),
            entries,
            valid: true,
        })))
    }

    pub fn invalid(layout: &BindGroupLayout) -> Self {
        Self(Arc::new(BindGroupInner {
            device: layout.0.device.clone(),
            layout: layout.clone(),
            entries: Vec::new(),
            valid: false,
        }))
    }

    pub fn is_valid(&self) -> bool {
        self.0.valid
    }

    pub(crate) fn belongs_to(&self, device: &Arc<DeviceShared>) -> bool {
        Arc::ptr_eq(&self.0.device, device)
    }
}

pub(crate) struct PipelineLayoutInner {
    device: Arc<DeviceShared>,
    pub groups: Vec<BindGroupLayout>,
    pub valid: bool,
}

#[derive(Clone)]
pub struct PipelineLayout(pub(crate) Arc<PipelineLayoutInner>);

impl PipelineLayout {
    pub fn new(device: &Device, groups: &[BindGroupLayout]) -> Result<Self, Error> {
        if groups.iter().any(|g| !g.belongs_to(&device.0)) {
            return Err(Error::validation(
                "cannot create pipeline layout from a bind group layout belonging to another device",
            ));
        }
        if groups.iter().any(|g| !g.0.valid) {
            return Err(Error::validation(
                "cannot create pipeline layout from an invalid bind group layout",
            ));
        }
        Ok(Self(Arc::new(PipelineLayoutInner {
            device: device.0.clone(),
            groups: groups.to_vec(),
            valid: true,
        })))
    }

    pub fn invalid(device: &Device) -> Self {
        Self(Arc::new(PipelineLayoutInner {
            device: device.0.clone(),
            groups: Vec::new(),
            valid: false,
        }))
    }

    pub(crate) fn belongs_to(&self, device: &Arc<DeviceShared>) -> bool {
        Arc::ptr_eq(&self.0.device, device)
    }
}

#[derive(Clone)]
pub struct ShaderModule {
    device: Arc<DeviceShared>,
    pub id: ShaderId,
    pub valid: bool,
}

impl ShaderModule {
    pub fn new(device: &Device, id: ShaderId) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("D3D11 device is lost"));
        }
        Ok(Self {
            device: device.0.clone(),
            id,
            valid: true,
        })
    }

    pub fn invalid(device: &Device, id: ShaderId) -> Self {
        Self {
            device: device.0.clone(),
            id,
            valid: false,
        }
    }

    pub(crate) fn belongs_to(&self, device: &Arc<DeviceShared>) -> bool {
        Arc::ptr_eq(&self.device, device)
    }
}

fn compile_or_load_dxbc(shader: ShaderId, ep: &EntryPointInfo) -> Result<Vec<u8>, Error> {
    if let Some(dxbc) = ep.dxbc {
        return Ok(dxbc.to_vec());
    }
    let src = shader.artifact().hlsl.as_bytes();
    let entry = CString::new(ep.hlsl_name)
        .map_err(|_| Error::validation("HLSL entry point contains interior NUL"))?;
    let target = match ep.stage {
        Stage::Vertex => c"vs_5_0",
        Stage::Fragment => c"ps_5_0",
    };
    let name = CString::new(shader.artifact().name)
        .map_err(|_| Error::validation("shader name contains interior NUL"))?;
    let mut code: Option<ID3DBlob> = None;
    let mut errs: Option<ID3DBlob> = None;
    // SAFETY: `src`, `name`, `entry`, `target` are valid for the duration of `D3DCompile`.
    let res = unsafe {
        D3DCompile(
            src.as_ptr().cast(),
            src.len(),
            PCSTR(name.as_ptr().cast()),
            None,
            None,
            PCSTR(entry.as_ptr().cast()),
            PCSTR(target.as_ptr().cast()),
            1 << 15, // D3DCOMPILE_OPTIMIZATION_LEVEL3
            0,
            &mut code,
            Some(&mut errs),
        )
    };
    if let Err(e) = res {
        let diag = errs.map_or_else(String::new, |blob| {
            // SAFETY: `blob` is a live `ID3DBlob` returned by `D3DCompile`.
            let ptr = unsafe { blob.GetBufferPointer() }.cast::<u8>();
            // SAFETY: `blob` is a live `ID3DBlob` returned by `D3DCompile`.
            let len = unsafe { blob.GetBufferSize() };
            if ptr.is_null() || len == 0 {
                String::new()
            } else {
                // SAFETY: `ptr` points to `len` valid error-message bytes.
                let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
                String::from_utf8_lossy(slice).into_owned()
            }
        });
        return Err(Error::internal(format!(
            "D3DCompile({:?}, {}): {e}: {diag}",
            shader, ep.hlsl_name
        )));
    }
    let blob =
        code.ok_or_else(|| Error::internal("D3DCompile succeeded with null bytecode blob"))?;
    // SAFETY: `blob` is a valid compiled DXBC buffer.
    let ptr = unsafe { blob.GetBufferPointer() }.cast::<u8>();
    // SAFETY: `blob` is a valid compiled DXBC buffer.
    let len = unsafe { blob.GetBufferSize() };
    if ptr.is_null() || len == 0 {
        return Err(Error::internal("D3DCompile returned empty bytecode blob"));
    }
    // SAFETY: `ptr` points to `len` bytecode bytes owned by `blob`.
    Ok(unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec())
}

pub struct PipelineDesc<'a> {
    pub layout: &'a PipelineLayout,
    pub vertex: (&'a ShaderModule, Option<&'a str>),
    pub fragment: (&'a ShaderModule, Option<&'a str>),
    pub buffers: &'a [VertexBufferLayout<'a>],
    pub topology: PrimitiveTopology,
    pub target: ColorTargetState,
}

pub(crate) struct RenderPipelineInner {
    device: Arc<DeviceShared>,
    pub vs: Option<ComSend<ID3D11VertexShader>>,
    pub ps: Option<ComSend<ID3D11PixelShader>>,
    pub input_layout: Option<ComSend<ID3D11InputLayout>>,
    pub blend: Option<ComSend<ID3D11BlendState>>,
    pub rasterizer: Option<ComSend<ID3D11RasterizerState>>,
    pub vertex_strides: Vec<u32>,
    pub topology: PrimitiveTopology,
    pub bindings: ShaderBindingTable,
    pub group_layouts: Vec<BindGroupLayout>,
}

#[derive(Clone)]
pub struct RenderPipeline(pub(crate) Arc<RenderPipelineInner>);

impl RenderPipeline {
    pub fn new(device: &Device, desc: &PipelineDesc<'_>) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("D3D11 device is lost"));
        }
        if !desc.layout.belongs_to(&device.0)
            || !desc.vertex.0.belongs_to(&device.0)
            || !desc.fragment.0.belongs_to(&device.0)
        {
            return Err(Error::validation(
                "render pipeline layout or shader module belongs to another device",
            ));
        }
        if !desc.layout.0.valid || !desc.vertex.0.valid || !desc.fragment.0.valid {
            return Err(Error::validation(
                "invalid layout or shader module in D3D11 render pipeline",
            ));
        }

        let vs_info = resolve_entry_point(desc.vertex.0.id, desc.vertex.1, Stage::Vertex)
            .map_err(Error::validation)?;
        let fs_info = resolve_entry_point(desc.fragment.0.id, desc.fragment.1, Stage::Fragment)
            .map_err(Error::validation)?;

        let vs_bc = compile_or_load_dxbc(desc.vertex.0.id, vs_info)?;
        let ps_bc = compile_or_load_dxbc(desc.fragment.0.id, fs_info)?;

        let group_slices: Vec<&[BindGroupLayoutEntry]> = desc
            .layout
            .0
            .groups
            .iter()
            .map(|g| g.0.entries.as_slice())
            .collect();
        let bindings =
            ShaderBindingTable::from_shaders(desc.vertex.0.id, desc.fragment.0.id, &group_slices)
                .map_err(Error::validation)?;

        let mut vs = None;
        // SAFETY: `vs_bc` is valid SM 5.0 DXBC bytecode.
        unsafe { device.0.raw.CreateVertexShader(&vs_bc, None, Some(&mut vs)) }
            .map_err(|e| win_err("ID3D11Device::CreateVertexShader", &e))?;
        let vs = vs.ok_or_else(|| Error::internal("CreateVertexShader returned null"))?;

        let mut ps = None;
        // SAFETY: `ps_bc` is valid SM 5.0 DXBC bytecode.
        unsafe { device.0.raw.CreatePixelShader(&ps_bc, None, Some(&mut ps)) }
            .map_err(|e| win_err("ID3D11Device::CreatePixelShader", &e))?;
        let ps = ps.ok_or_else(|| Error::internal("CreatePixelShader returned null"))?;

        let semantic = CString::new(HLSL_VERTEX_SEMANTIC).expect("LOC has no NUL");
        let mut elements = Vec::new();
        let mut vertex_strides = Vec::with_capacity(desc.buffers.len());
        for (slot, buf) in desc.buffers.iter().enumerate() {
            vertex_strides.push(buf.array_stride as u32);
            let (slot_class, step_rate) = match buf.step_mode {
                VertexStepMode::Vertex => (D3D11_INPUT_PER_VERTEX_DATA, 0),
                VertexStepMode::Instance => (D3D11_INPUT_PER_INSTANCE_DATA, 1),
            };
            for attr in buf.attributes {
                elements.push(D3D11_INPUT_ELEMENT_DESC {
                    SemanticName: PCSTR(semantic.as_ptr().cast()),
                    SemanticIndex: attr.shader_location,
                    Format: d3d_vertex_format(attr.format),
                    InputSlot: slot as u32,
                    AlignedByteOffset: attr.offset as u32,
                    InputSlotClass: slot_class,
                    InstanceDataStepRate: step_rate,
                });
            }
        }
        let input_layout = if elements.is_empty() {
            None
        } else {
            let mut il = None;
            // SAFETY: `semantic` outlives `CreateInputLayout`; `vs_bc` matches input signature.
            unsafe {
                device
                    .0
                    .raw
                    .CreateInputLayout(&elements, &vs_bc, Some(&mut il))
            }
            .map_err(|e| win_err("ID3D11Device::CreateInputLayout", &e))?;
            il.map(ComSend)
        };

        let rt_blend = if let Some(b) = desc.target.blend {
            D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: TRUE,
                SrcBlend: d3d_blend(b.color.src_factor),
                DestBlend: d3d_blend(b.color.dst_factor),
                BlendOp: d3d_blend_op(b.color.operation),
                SrcBlendAlpha: d3d_blend(b.alpha.src_factor),
                DestBlendAlpha: d3d_blend(b.alpha.dst_factor),
                BlendOpAlpha: d3d_blend_op(b.alpha.operation),
                RenderTargetWriteMask: d3d_write_mask(desc.target.write_mask),
            }
        } else {
            D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: FALSE,
                SrcBlend: D3D11_BLEND_ONE,
                DestBlend: D3D11_BLEND_ZERO,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_ZERO,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: d3d_write_mask(desc.target.write_mask),
            }
        };
        let mut blend_desc = D3D11_BLEND_DESC {
            AlphaToCoverageEnable: FALSE,
            IndependentBlendEnable: FALSE,
            RenderTarget: [D3D11_RENDER_TARGET_BLEND_DESC::default(); 8],
        };
        blend_desc.RenderTarget[0] = rt_blend;
        let mut blend = None;
        // SAFETY: valid blend state descriptor on live D3D11 device.
        unsafe { device.0.raw.CreateBlendState(&blend_desc, Some(&mut blend)) }
            .map_err(|e| win_err("ID3D11Device::CreateBlendState", &e))?;
        let blend = blend.ok_or_else(|| Error::internal("CreateBlendState returned null"))?;

        let rast_desc = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            FrontCounterClockwise: TRUE,
            DepthBias: 0,
            DepthBiasClamp: 0.0,
            SlopeScaledDepthBias: 0.0,
            DepthClipEnable: TRUE,
            ScissorEnable: TRUE,
            MultisampleEnable: FALSE,
            AntialiasedLineEnable: FALSE,
        };
        let mut rasterizer = None;
        // SAFETY: valid rasterizer descriptor on live D3D11 device.
        unsafe {
            device
                .0
                .raw
                .CreateRasterizerState(&rast_desc, Some(&mut rasterizer))
        }
        .map_err(|e| win_err("ID3D11Device::CreateRasterizerState", &e))?;
        let rasterizer =
            rasterizer.ok_or_else(|| Error::internal("CreateRasterizerState returned null"))?;

        Ok(Self(Arc::new(RenderPipelineInner {
            device: device.0.clone(),
            vs: Some(ComSend(vs)),
            ps: Some(ComSend(ps)),
            input_layout,
            blend: Some(ComSend(blend)),
            rasterizer: Some(ComSend(rasterizer)),
            vertex_strides,
            topology: desc.topology,
            bindings,
            group_layouts: desc.layout.0.groups.clone(),
        })))
    }

    pub fn invalid(device: &Device) -> Self {
        Self(Arc::new(RenderPipelineInner {
            device: device.0.clone(),
            vs: None,
            ps: None,
            input_layout: None,
            blend: None,
            rasterizer: None,
            vertex_strides: Vec::new(),
            topology: PrimitiveTopology::TriangleList,
            bindings: ShaderBindingTable::default(),
            group_layouts: Vec::new(),
        }))
    }

    pub fn is_valid(&self) -> bool {
        self.0.vs.is_some() && self.0.ps.is_some()
    }

    pub(crate) fn belongs_to(&self, device: &Arc<DeviceShared>) -> bool {
        Arc::ptr_eq(&self.0.device, device)
    }
}

#[derive(Clone)]
pub struct QuerySet {
    pub count: u32,
}

impl QuerySet {
    pub fn new(_device: &Device, _desc: &QuerySetDescriptor<'_>) -> Result<Self, Error> {
        Err(Error::validation(
            "timestamp queries are not supported on the native D3D11 backend",
        ))
    }

    pub fn invalid(_device: &Device) -> Self {
        Self { count: 0 }
    }
}

#[derive(Clone)]
pub struct PipelineCache;

impl PipelineCache {
    pub fn new(_device: &Device, _data: Option<&[u8]>) -> Result<Self, Error> {
        Ok(Self)
    }

    pub fn invalid(_device: &Device) -> Self {
        Self
    }

    pub fn get_data(&self) -> Option<Vec<u8>> {
        None
    }
}
