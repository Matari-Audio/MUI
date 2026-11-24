//! Metal resource wrappers (`MTLBuffer`, `MTLTexture`, `MTLSamplerState`,
//! `MTLLibrary`, `MTLRenderPipelineState`, bind groups, and query placeholders).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use mui_shaders::ShaderId;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSRange, NSString};
use objc2_metal::{
    MTLBlendFactor, MTLBlendOperation, MTLBuffer, MTLColorWriteMask, MTLCompileOptions, MTLDevice,
    MTLLanguageVersion, MTLLibrary, MTLPixelFormat, MTLPrimitiveType, MTLRenderPipelineDescriptor,
    MTLRenderPipelineState, MTLResourceOptions, MTLSamplerAddressMode, MTLSamplerDescriptor,
    MTLSamplerMinMagFilter, MTLSamplerState, MTLStorageMode, MTLTexture, MTLTextureDescriptor,
    MTLTextureType, MTLTextureUsage, MTLVertexDescriptor, MTLVertexFormat, MTLVertexStepFunction,
};

use super::{Device, DeviceShared, Error, MapCallback, SendObj, lock};
use crate::binding::{
    BoundKind, ShaderBindingTable, dynamic_offset_bindings, metal_vertex_buffer_index,
    resolve_entry_point, validate_bind_group_entries,
};
use crate::types::{
    AddressMode, BindGroupLayoutEntry, BlendFactor, BlendOperation, BufferDescriptor, BufferUsages,
    ColorTargetState, ColorWrites, FilterMode, PrimitiveTopology, QuerySetDescriptor,
    SamplerDescriptor, TextureDescriptor, TextureFormat, TextureUsages, TextureViewDimension,
    VertexBufferLayout, VertexFormat, VertexStepMode,
};

pub(crate) fn mtl_format(f: TextureFormat) -> MTLPixelFormat {
    match f {
        TextureFormat::R8Unorm => MTLPixelFormat::R8Unorm,
        TextureFormat::Rgba8Unorm => MTLPixelFormat::RGBA8Unorm,
        TextureFormat::Rgba8UnormSrgb => MTLPixelFormat::RGBA8Unorm_sRGB,
        TextureFormat::Bgra8Unorm => MTLPixelFormat::BGRA8Unorm,
        TextureFormat::Bgra8UnormSrgb => MTLPixelFormat::BGRA8Unorm_sRGB,
        TextureFormat::Rgba16Float => MTLPixelFormat::RGBA16Float,
        TextureFormat::Rgba32Float => MTLPixelFormat::RGBA32Float,
    }
}

pub(crate) fn mtl_primitive(t: PrimitiveTopology) -> MTLPrimitiveType {
    match t {
        PrimitiveTopology::TriangleList => MTLPrimitiveType::Triangle,
        PrimitiveTopology::TriangleStrip => MTLPrimitiveType::TriangleStrip,
    }
}

fn mtl_blend_factor(f: BlendFactor) -> MTLBlendFactor {
    match f {
        BlendFactor::Zero => MTLBlendFactor::Zero,
        BlendFactor::One => MTLBlendFactor::One,
        BlendFactor::SrcAlpha => MTLBlendFactor::SourceAlpha,
        BlendFactor::OneMinusSrcAlpha => MTLBlendFactor::OneMinusSourceAlpha,
        BlendFactor::DstAlpha => MTLBlendFactor::DestinationAlpha,
        BlendFactor::OneMinusDstAlpha => MTLBlendFactor::OneMinusDestinationAlpha,
    }
}

fn mtl_blend_op(op: BlendOperation) -> MTLBlendOperation {
    match op {
        BlendOperation::Add => MTLBlendOperation::Add,
    }
}

fn mtl_color_mask(mask: ColorWrites) -> MTLColorWriteMask {
    let mut out = MTLColorWriteMask::None;
    if mask.contains(ColorWrites::RED) {
        out |= MTLColorWriteMask::Red;
    }
    if mask.contains(ColorWrites::GREEN) {
        out |= MTLColorWriteMask::Green;
    }
    if mask.contains(ColorWrites::BLUE) {
        out |= MTLColorWriteMask::Blue;
    }
    if mask.contains(ColorWrites::ALPHA) {
        out |= MTLColorWriteMask::Alpha;
    }
    out
}

fn mtl_vertex_format(f: VertexFormat) -> MTLVertexFormat {
    match f {
        VertexFormat::Float32x2 => MTLVertexFormat::Float2,
        VertexFormat::Float32x4 => MTLVertexFormat::Float4,
    }
}

fn mtl_filter(f: FilterMode) -> MTLSamplerMinMagFilter {
    match f {
        FilterMode::Nearest => MTLSamplerMinMagFilter::Nearest,
        FilterMode::Linear => MTLSamplerMinMagFilter::Linear,
    }
}

fn mtl_address(a: AddressMode) -> MTLSamplerAddressMode {
    match a {
        AddressMode::ClampToEdge => MTLSamplerAddressMode::ClampToEdge,
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
    pub raw: Option<SendObj<ProtocolObject<dyn MTLBuffer>>>,
    size: u64,
    usage: BufferUsages,
    map_state: Mutex<MapState>,
    next_map_token: AtomicU64,
    last_write_seq: AtomicU64,
}

#[derive(Clone)]
pub struct Buffer(pub(crate) Arc<BufferInner>);

impl Buffer {
    pub fn new(device: &Device, desc: &BufferDescriptor<'_>) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("Metal device is lost"));
        }
        if desc.size > device.limits().max_buffer_size {
            return Err(Error::validation(format!(
                "buffer size {} exceeds Metal max_buffer_size {}",
                desc.size,
                device.limits().max_buffer_size
            )));
        }
        let alloc = usize::try_from(desc.size.max(4))
            .map_err(|_| Error::out_of_memory("buffer size overflows usize"))?;
        let raw = device
            .0
            .raw
            .newBufferWithLength_options(alloc, MTLResourceOptions::StorageModeShared)
            .ok_or_else(|| Error::out_of_memory("MTLDevice::newBufferWithLength returned nil"))?;
        Ok(Self::from_raw(device, raw, desc.size, desc.usage))
    }

    pub(crate) fn from_raw(
        device: &Device,
        raw: Retained<ProtocolObject<dyn MTLBuffer>>,
        size: u64,
        usage: BufferUsages,
    ) -> Self {
        Self(Arc::new(BufferInner {
            device: device.0.clone(),
            raw: Some(SendObj(raw)),
            size,
            usage,
            map_state: Mutex::new(MapState::Unmapped),
            next_map_token: AtomicU64::new(1),
            last_write_seq: AtomicU64::new(0),
        }))
    }

    pub fn invalid(device: &Device, desc: &BufferDescriptor<'_>) -> Self {
        Self(Arc::new(BufferInner {
            device: device.0.clone(),
            raw: None,
            size: desc.size,
            usage: desc.usage,
            map_state: Mutex::new(MapState::Unmapped),
            next_map_token: AtomicU64::new(1),
            last_write_seq: AtomicU64::new(0),
        }))
    }

    pub fn is_valid(&self) -> bool {
        self.0.raw.is_some()
    }

    pub(crate) fn belongs_to(&self, device: &Arc<DeviceShared>) -> bool {
        Arc::ptr_eq(&self.0.device, device)
    }

    pub(crate) fn record_write_seq(&self, seq: u64) {
        self.0.last_write_seq.fetch_max(seq, Ordering::AcqRel);
    }

    pub(crate) fn last_write_seq(&self) -> u64 {
        self.0.last_write_seq.load(Ordering::Acquire)
    }

    pub fn size(&self) -> u64 {
        self.0.size
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
        let mut out = vec![0u8; len];
        if len > 0 {
            // SAFETY: buffer uses `StorageModeShared`, GPU completion waited in `poll`,
            // `state` stays locked through the copy so `unmap()` cannot race mid-copy,
            // and `start + len <= self.0.size <= raw.length()`.
            unsafe {
                let src = raw.contents().as_ptr().cast::<u8>().add(start);
                std::ptr::copy_nonoverlapping(src, out.as_mut_ptr(), len);
            }
        }
        drop(state);
        Ok(out)
    }

    pub fn unmap(&self) {
        *lock(&self.0.map_state) = MapState::Unmapped;
    }
}

pub(crate) struct TextureInner {
    device: Arc<DeviceShared>,
    pub raw: Option<SendObj<ProtocolObject<dyn MTLTexture>>>,
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
            return Err(Error::internal("Metal device is lost"));
        }
        if desc.size.width == 0 || desc.size.height == 0 {
            return Err(Error::validation("texture dimensions must be non-zero"));
        }
        if desc.mip_level_count > 1 {
            return Err(Error::validation(
                "native Metal textures support only one mip level (mip_level_count == 1)",
            ));
        }
        let layers = desc.size.depth_or_array_layers.max(1);
        let sample_count = desc.sample_count.max(1);
        let td = MTLTextureDescriptor::new();
        td.setTextureType(if layers > 1 {
            MTLTextureType::Type2DArray
        } else {
            MTLTextureType::Type2D
        });
        td.setPixelFormat(mtl_format(desc.format));
        // SAFETY: dimensions validated > 0 above and fit in NSUInteger.
        unsafe {
            td.setWidth(desc.size.width as usize);
            td.setHeight(desc.size.height as usize);
            td.setArrayLength(layers as usize);
            td.setMipmapLevelCount(1);
            td.setSampleCount(sample_count as usize);
        }
        td.setStorageMode(MTLStorageMode::Private);
        let mut usage = MTLTextureUsage::Unknown;
        if desc.usage.contains(TextureUsages::TEXTURE_BINDING) {
            usage |= MTLTextureUsage::ShaderRead;
        }
        if desc.usage.contains(TextureUsages::RENDER_ATTACHMENT) {
            usage |= MTLTextureUsage::RenderTarget;
        }
        if usage == MTLTextureUsage::Unknown {
            usage = MTLTextureUsage::ShaderRead;
        }
        td.setUsage(usage);

        let raw = device.0.raw.newTextureWithDescriptor(&td).ok_or_else(|| {
            Error::out_of_memory("MTLDevice::newTextureWithDescriptor returned nil")
        })?;
        Ok(Self(Arc::new(TextureInner {
            device: device.0.clone(),
            raw: Some(SendObj(raw)),
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

    pub(crate) fn from_drawable(
        device: &Device,
        raw: Retained<ProtocolObject<dyn MTLTexture>>,
        width: u32,
        height: u32,
        format: TextureFormat,
        usage: TextureUsages,
        live: Arc<AtomicBool>,
    ) -> Self {
        Self(Arc::new(TextureInner {
            device: device.0.clone(),
            raw: Some(SendObj(raw)),
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
        let want = dimension.unwrap_or(if self.0.layers > 1 {
            TextureViewDimension::D2Array
        } else {
            TextureViewDimension::D2
        });
        let want_type = match want {
            TextureViewDimension::D2 => MTLTextureType::Type2D,
            TextureViewDimension::D2Array => MTLTextureType::Type2DArray,
        };
        let view_tex = if raw.textureType() == want_type {
            raw.clone()
        } else {
            let slice_len = match want {
                TextureViewDimension::D2 => 1,
                TextureViewDimension::D2Array => self.0.layers as usize,
            };
            // SAFETY: valid level `0..1` and slice range (`0..1` for `Type2D`, `0..layers` for `Type2DArray`).
            let v = unsafe {
                raw.newTextureViewWithPixelFormat_textureType_levels_slices(
                    raw.pixelFormat(),
                    want_type,
                    NSRange::new(0, 1),
                    NSRange::new(0, slice_len),
                )
            }
            .ok_or_else(|| {
                Error::internal("MTLTexture::newTextureViewWithPixelFormat returned nil")
            })?;
            SendObj(v)
        };
        Ok(TextureView(Arc::new(TextureViewInner {
            texture: self.clone(),
            raw: Some(view_tex),
        })))
    }
}

pub(crate) struct TextureViewInner {
    pub texture: Texture,
    pub raw: Option<SendObj<ProtocolObject<dyn MTLTexture>>>,
}

#[derive(Clone)]
pub struct TextureView(pub(crate) Arc<TextureViewInner>);

impl TextureView {
    pub fn invalid(texture: Texture) -> Self {
        Self(Arc::new(TextureViewInner { texture, raw: None }))
    }

    pub fn is_valid(&self) -> bool {
        self.0.raw.is_some() && self.0.texture.is_valid()
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
    pub raw: Option<SendObj<ProtocolObject<dyn MTLSamplerState>>>,
}

#[derive(Clone)]
pub struct Sampler(pub(crate) Arc<SamplerInner>);

impl Sampler {
    pub fn new(device: &Device, desc: &SamplerDescriptor<'_>) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("Metal device is lost"));
        }
        let sd = MTLSamplerDescriptor::new();
        sd.setMinFilter(mtl_filter(desc.min_filter));
        sd.setMagFilter(mtl_filter(desc.mag_filter));
        sd.setSAddressMode(mtl_address(desc.address_mode_u));
        sd.setTAddressMode(mtl_address(desc.address_mode_v));
        let raw = device
            .0
            .raw
            .newSamplerStateWithDescriptor(&sd)
            .ok_or_else(|| {
                Error::internal("MTLDevice::newSamplerStateWithDescriptor returned nil")
            })?;
        Ok(Self(Arc::new(SamplerInner {
            device: device.0.clone(),
            raw: Some(SendObj(raw)),
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
                    if !view
                        .0
                        .texture
                        .usage()
                        .contains(TextureUsages::TEXTURE_BINDING)
                    {
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

pub(crate) struct ShaderModuleInner {
    device: Arc<DeviceShared>,
    pub id: ShaderId,
    pub library: Option<SendObj<ProtocolObject<dyn MTLLibrary>>>,
}

#[derive(Clone)]
pub struct ShaderModule(pub(crate) Arc<ShaderModuleInner>);

impl ShaderModule {
    pub fn new(device: &Device, id: ShaderId) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("Metal device is lost"));
        }
        let msl = NSString::from_str(id.artifact().msl);
        let opts = MTLCompileOptions::new();
        opts.setLanguageVersion(MTLLanguageVersion::Version2_3);
        let library = device
            .0
            .raw
            .newLibraryWithSource_options_error(&msl, Some(&opts))
            .map_err(|e| Error::internal(format!("compiling MSL for {id:?}: {e}")))?;
        Ok(Self(Arc::new(ShaderModuleInner {
            device: device.0.clone(),
            id,
            library: Some(SendObj(library)),
        })))
    }

    pub fn invalid(device: &Device, id: ShaderId) -> Self {
        Self(Arc::new(ShaderModuleInner {
            device: device.0.clone(),
            id,
            library: None,
        }))
    }

    pub(crate) fn belongs_to(&self, device: &Arc<DeviceShared>) -> bool {
        Arc::ptr_eq(&self.0.device, device)
    }
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
    pub state: Option<SendObj<ProtocolObject<dyn MTLRenderPipelineState>>>,
    pub topology: PrimitiveTopology,
    pub bindings: ShaderBindingTable,
    pub group_layouts: Vec<BindGroupLayout>,
}

#[derive(Clone)]
pub struct RenderPipeline(pub(crate) Arc<RenderPipelineInner>);

impl RenderPipeline {
    pub fn new(device: &Device, desc: &PipelineDesc<'_>) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("Metal device is lost"));
        }
        if !desc.layout.belongs_to(&device.0)
            || !desc.vertex.0.belongs_to(&device.0)
            || !desc.fragment.0.belongs_to(&device.0)
        {
            return Err(Error::validation(
                "render pipeline layout or shader module belongs to another device",
            ));
        }
        if !desc.layout.0.valid {
            return Err(Error::validation("invalid pipeline layout"));
        }
        let vs_lib = desc
            .vertex
            .0
            .0
            .library
            .as_ref()
            .ok_or_else(|| Error::validation("invalid vertex shader module"))?;
        let fs_lib = desc
            .fragment
            .0
            .0
            .library
            .as_ref()
            .ok_or_else(|| Error::validation("invalid fragment shader module"))?;

        let vs_info = resolve_entry_point(
            desc.vertex.0.0.id,
            desc.vertex.1,
            mui_shaders::Stage::Vertex,
        )
        .map_err(Error::validation)?;
        let fs_info = resolve_entry_point(
            desc.fragment.0.0.id,
            desc.fragment.1,
            mui_shaders::Stage::Fragment,
        )
        .map_err(Error::validation)?;

        let vs_fn = vs_lib
            .newFunctionWithName(&NSString::from_str(vs_info.msl_name))
            .ok_or_else(|| {
                Error::internal(format!(
                    "MTLLibrary missing vertex function `{}`",
                    vs_info.msl_name
                ))
            })?;
        let fs_fn = fs_lib
            .newFunctionWithName(&NSString::from_str(fs_info.msl_name))
            .ok_or_else(|| {
                Error::internal(format!(
                    "MTLLibrary missing fragment function `{}`",
                    fs_info.msl_name
                ))
            })?;

        let group_slices: Vec<&[BindGroupLayoutEntry]> = desc
            .layout
            .0
            .groups
            .iter()
            .map(|g| g.0.entries.as_slice())
            .collect();
        let bindings = ShaderBindingTable::from_shaders(
            desc.vertex.0.0.id,
            desc.fragment.0.0.id,
            &group_slices,
        )
        .map_err(Error::validation)?;

        let pdesc = MTLRenderPipelineDescriptor::new();
        pdesc.setVertexFunction(Some(&vs_fn));
        pdesc.setFragmentFunction(Some(&fs_fn));

        if !desc.buffers.is_empty() {
            let vdesc = MTLVertexDescriptor::vertexDescriptor();
            let layouts = vdesc.layouts();
            let attrs = vdesc.attributes();
            for (slot, buf) in desc.buffers.iter().enumerate() {
                let metal_idx =
                    metal_vertex_buffer_index(slot as u32).map_err(Error::validation)?;
                // SAFETY: `metal_idx` is in `16..31`, within Metal's 31-slot vertex table.
                let layout = unsafe { layouts.objectAtIndexedSubscript(metal_idx as usize) };
                // SAFETY: stride and step rate are valid positive integers.
                unsafe {
                    layout.setStride(buf.array_stride as usize);
                    layout.setStepRate(1);
                }
                layout.setStepFunction(match buf.step_mode {
                    VertexStepMode::Vertex => MTLVertexStepFunction::PerVertex,
                    VertexStepMode::Instance => MTLVertexStepFunction::PerInstance,
                });
                for attr in buf.attributes {
                    if attr.shader_location >= device.limits().max_vertex_attributes {
                        return Err(Error::validation(format!(
                            "vertex attribute shader_location {} exceeds limit {}",
                            attr.shader_location,
                            device.limits().max_vertex_attributes
                        )));
                    }
                    // SAFETY: `shader_location < max_vertex_attributes` (<= 16 < 31).
                    let a =
                        unsafe { attrs.objectAtIndexedSubscript(attr.shader_location as usize) };
                    a.setFormat(mtl_vertex_format(attr.format));
                    // SAFETY: offset and buffer index fit in NSUInteger and match layout above.
                    unsafe {
                        a.setOffset(attr.offset as usize);
                        a.setBufferIndex(metal_idx as usize);
                    }
                }
            }
            pdesc.setVertexDescriptor(Some(&vdesc));
        }

        // SAFETY: color attachment 0 always exists on `MTLRenderPipelineDescriptor`.
        let color0 = unsafe { pdesc.colorAttachments().objectAtIndexedSubscript(0) };
        color0.setPixelFormat(mtl_format(desc.target.format));
        color0.setWriteMask(mtl_color_mask(desc.target.write_mask));
        if let Some(blend) = desc.target.blend {
            color0.setBlendingEnabled(true);
            color0.setSourceRGBBlendFactor(mtl_blend_factor(blend.color.src_factor));
            color0.setDestinationRGBBlendFactor(mtl_blend_factor(blend.color.dst_factor));
            color0.setRgbBlendOperation(mtl_blend_op(blend.color.operation));
            color0.setSourceAlphaBlendFactor(mtl_blend_factor(blend.alpha.src_factor));
            color0.setDestinationAlphaBlendFactor(mtl_blend_factor(blend.alpha.dst_factor));
            color0.setAlphaBlendOperation(mtl_blend_op(blend.alpha.operation));
        } else {
            color0.setBlendingEnabled(false);
        }

        let state = device
            .0
            .raw
            .newRenderPipelineStateWithDescriptor_error(&pdesc)
            .map_err(|e| Error::internal(format!("MTLDevice::newRenderPipelineState: {e}")))?;

        Ok(Self(Arc::new(RenderPipelineInner {
            device: device.0.clone(),
            state: Some(SendObj(state)),
            topology: desc.topology,
            bindings,
            group_layouts: desc.layout.0.groups.clone(),
        })))
    }

    pub fn invalid(device: &Device) -> Self {
        Self(Arc::new(RenderPipelineInner {
            device: device.0.clone(),
            state: None,
            topology: PrimitiveTopology::TriangleList,
            bindings: ShaderBindingTable::default(),
            group_layouts: Vec::new(),
        }))
    }

    pub fn is_valid(&self) -> bool {
        self.0.state.is_some()
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
            "timestamp queries are not supported on the native Metal backend",
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
