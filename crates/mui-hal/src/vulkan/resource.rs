//! Vulkan resources. Each is an `Arc` whose drop destroys the Vulkan
//! object; a submission clones what it records, so the GPU never reads a
//! destroyed object. `ponytail:` one `VkDeviceMemory` per resource and one
//! descriptor pool per bind group: MUI holds hundreds, not thousands.

use std::ffi::CString;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ash::vk;

use super::{Cmd, Device, DeviceShared, Error, MapCallback, err, lock, vk_err};
use crate::types::{
    BindingType, BlendFactor, BlendState, BufferDescriptor, BufferUsages, ColorWrites, FilterMode,
    PrimitiveTopology, QuerySetDescriptor, SamplerDescriptor, ShaderStages, TextureDescriptor,
    TextureFormat, TextureUsages, TextureViewDimension, VertexFormat, VertexStepMode,
};

pub(crate) fn vk_format(f: TextureFormat) -> vk::Format {
    match f {
        TextureFormat::R8Unorm => vk::Format::R8_UNORM,
        TextureFormat::Rgba8Unorm => vk::Format::R8G8B8A8_UNORM,
        TextureFormat::Rgba8UnormSrgb => vk::Format::R8G8B8A8_SRGB,
        TextureFormat::Bgra8Unorm => vk::Format::B8G8R8A8_UNORM,
        TextureFormat::Bgra8UnormSrgb => vk::Format::B8G8R8A8_SRGB,
        TextureFormat::Rgba16Float => vk::Format::R16G16B16A16_SFLOAT,
        TextureFormat::Rgba32Float => vk::Format::R32G32B32A32_SFLOAT,
    }
}

fn stages(s: ShaderStages) -> vk::ShaderStageFlags {
    let mut out = vk::ShaderStageFlags::empty();
    if s.contains(ShaderStages::VERTEX) {
        out |= vk::ShaderStageFlags::VERTEX;
    }
    if s.contains(ShaderStages::FRAGMENT) {
        out |= vk::ShaderStageFlags::FRAGMENT;
    }
    out
}

/// Device memory bound to one object, freed with it.
struct Memory {
    raw: vk::DeviceMemory,
    mapped: *mut u8,
}

fn allocate(
    device: &Device,
    req: vk::MemoryRequirements,
    want: vk::MemoryPropertyFlags,
    prefer: vk::MemoryPropertyFlags,
) -> Result<Memory, Error> {
    let index = device
        .memory_type(req.memory_type_bits, want, prefer)
        .ok_or_else(|| Error::internal(format!("no memory type with {want:?}")))?;
    let info = vk::MemoryAllocateInfo::default()
        .allocation_size(req.size)
        .memory_type_index(index);
    // SAFETY: a plain allocation on a live device.
    let raw = unsafe { device.0.raw.allocate_memory(&info, None) }
        .map_err(|e| vk_err("vkAllocateMemory", e))?;
    let mapped = if want.contains(vk::MemoryPropertyFlags::HOST_VISIBLE) {
        // SAFETY: `raw` is host-visible and mapped once for its lifetime.
        match unsafe {
            device
                .0
                .raw
                .map_memory(raw, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
        } {
            Ok(p) => p.cast(),
            Err(e) => {
                // SAFETY: `raw` was just allocated and is unused.
                unsafe { device.0.raw.free_memory(raw, None) };
                return Err(vk_err("vkMapMemory", e));
            }
        }
    } else {
        std::ptr::null_mut()
    };
    Ok(Memory { raw, mapped })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MapState {
    Unmapped,
    Pending { token: u64, start: u64, end: u64 },
    Mapped { start: u64, end: u64 },
}

pub(crate) struct BufferInner {
    device: Arc<DeviceShared>,
    pub raw: vk::Buffer,
    memory: Memory,
    size: u64,
    pub usage: BufferUsages,
    map_state: Mutex<MapState>,
}

// SAFETY: `memory.mapped` points at memory this buffer owns; access is
// ordered by the map protocol (reads after the GPU's writes complete, and
// writes are rejected while mapped or pending).
unsafe impl Send for BufferInner {}
// SAFETY: as above.
unsafe impl Sync for BufferInner {}

impl Drop for BufferInner {
    fn drop(&mut self) {
        // SAFETY: no submission still holds this buffer (it would hold an Arc).
        unsafe {
            if self.raw != vk::Buffer::null() {
                self.device.raw.destroy_buffer(self.raw, None);
            }
            if self.memory.raw != vk::DeviceMemory::null() {
                self.device.raw.free_memory(self.memory.raw, None);
            }
        }
    }
}

#[derive(Clone)]
pub struct Buffer(pub(crate) Arc<BufferInner>);

impl Buffer {
    pub fn new(device: &Device, desc: &BufferDescriptor<'_>) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("the device is lost"));
        }
        let max_size = device.limits().max_buffer_size;
        if desc.size > max_size {
            return Err(Error::validation(format!(
                "buffer size {} exceeds max_buffer_size {max_size}",
                desc.size
            )));
        }
        let u = desc.usage;
        let mut usage = vk::BufferUsageFlags::TRANSFER_DST;
        if u.contains(BufferUsages::VERTEX) {
            usage |= vk::BufferUsageFlags::VERTEX_BUFFER;
        }
        if u.contains(BufferUsages::UNIFORM) {
            usage |= vk::BufferUsageFlags::UNIFORM_BUFFER;
        }
        if u.contains(BufferUsages::COPY_SRC) {
            usage |= vk::BufferUsageFlags::TRANSFER_SRC;
        }
        let host = u.contains(BufferUsages::MAP_READ);
        Self::with(device, desc.size, usage, host, u)
    }

    pub(crate) fn invalid(device: &Device, desc: &BufferDescriptor<'_>) -> Self {
        Self(Arc::new(BufferInner {
            device: device.0.clone(),
            raw: vk::Buffer::null(),
            memory: Memory {
                raw: vk::DeviceMemory::null(),
                mapped: std::ptr::null_mut(),
            },
            size: desc.size,
            usage: desc.usage,
            map_state: Mutex::new(MapState::Unmapped),
        }))
    }

    /// A host-visible upload chunk.
    pub(crate) fn staging(device: &Device, size: u64) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("the device is lost"));
        }
        Self::with(
            device,
            size,
            vk::BufferUsageFlags::TRANSFER_SRC,
            true,
            BufferUsages::empty(),
        )
    }

    fn with(
        device: &Device,
        size: u64,
        usage: vk::BufferUsageFlags,
        host: bool,
        hal: BufferUsages,
    ) -> Result<Self, Error> {
        let d = &device.0.raw;
        let info = vk::BufferCreateInfo::default()
            .size(size.max(4))
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        // SAFETY: a plain object creation on a live device.
        let raw =
            unsafe { d.create_buffer(&info, None) }.map_err(|e| vk_err("vkCreateBuffer", e))?;
        // SAFETY: `raw` is a live buffer.
        let req = unsafe { d.get_buffer_memory_requirements(raw) };
        let (want, prefer) = if host {
            (
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                vk::MemoryPropertyFlags::HOST_CACHED,
            )
        } else {
            (
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
                vk::MemoryPropertyFlags::empty(),
            )
        };
        let memory = match allocate(device, req, want, prefer) {
            Ok(m) => m,
            Err(e) => {
                // SAFETY: `raw` is unused.
                unsafe { d.destroy_buffer(raw, None) };
                return Err(e);
            }
        };
        // SAFETY: `memory` was allocated for `raw`'s requirements.
        if let Err(e) = unsafe { d.bind_buffer_memory(raw, memory.raw, 0) } {
            // SAFETY: both are unused.
            unsafe {
                d.destroy_buffer(raw, None);
                d.free_memory(memory.raw, None);
            }
            return Err(vk_err("vkBindBufferMemory", e));
        }
        if !memory.mapped.is_null() {
            // Like wgpu, a buffer starts zeroed: `mapped` exposes all of it,
            // including row padding no copy writes.
            // SAFETY: the fresh mapping covers `size` bytes and nothing else
            // references it yet.
            unsafe { std::ptr::write_bytes(memory.mapped, 0, size as usize) };
        }
        Ok(Self(Arc::new(BufferInner {
            device: device.0.clone(),
            raw,
            memory,
            size,
            usage: hal,
            map_state: Mutex::new(MapState::Unmapped),
        })))
    }

    pub fn is_valid(&self) -> bool {
        self.0.raw != vk::Buffer::null()
    }

    pub fn is_mapped_or_pending(&self) -> bool {
        *lock(&self.0.map_state) != MapState::Unmapped
    }

    /// Checks that this buffer is mapped and copies `offset..offset + size`
    /// while holding the map lock, preventing a concurrent `unmap()` + GPU
    /// write from racing with the host read.
    pub fn copy_mapped_range(
        &self,
        offset: u64,
        size: u64,
    ) -> Result<Vec<u8>, crate::BufferAsyncError> {
        let state = lock(&self.0.map_state);
        let MapState::Mapped { start, end } = *state else {
            return Err(crate::BufferAsyncError);
        };
        let read_end = offset.checked_add(size).ok_or(crate::BufferAsyncError)?;
        if offset < start || read_end > end {
            return Err(crate::BufferAsyncError);
        }
        let start = usize::try_from(offset).map_err(|_| crate::BufferAsyncError)?;
        let end = usize::try_from(read_end).map_err(|_| crate::BufferAsyncError)?;
        let bytes = self
            .mapped()
            .get(start..end)
            .ok_or(crate::BufferAsyncError)?;
        Ok(bytes.to_vec())
    }

    pub(crate) fn map_async(&self, start: u64, size: u64, callback: MapCallback) {
        static NEXT_MAP: AtomicU64 = AtomicU64::new(1);
        let device = self.device();
        let end = start.checked_add(size).filter(|end| *end <= self.size());
        let invalid = if !self.is_valid() {
            Some("cannot map an invalid buffer")
        } else if !self.0.usage.contains(BufferUsages::MAP_READ) {
            Some("map_async needs MAP_READ")
        } else if end.is_none() {
            Some("map range exceeds buffer size")
        } else {
            None
        };
        if let Some(description) = invalid {
            device.defer_rejected_map(callback);
            device.errors().report(crate::Error::Validation {
                description: description.into(),
            });
            return;
        }
        let end = end.expect("map range was validated");
        let mut callback = Some(callback);
        let buffer = self.clone();
        let registered = self.device().register_buffer_map(
            self,
            |buf| {
                let mut state = lock(&buf.0.map_state);
                if *state != MapState::Unmapped {
                    return Err(crate::Error::Validation {
                        description: "buffer is already mapped or has a pending map".into(),
                    });
                }
                let token = NEXT_MAP.fetch_add(1, Ordering::Relaxed);
                *state = MapState::Pending { token, start, end };
                Ok(token)
            },
            |token| {
                let cb = callback.take().expect("map callback created once");
                Box::new(move |res| {
                    let outcome = {
                        let mut state = lock(&buffer.0.map_state);
                        match (*state, res) {
                            (
                                MapState::Pending {
                                    token: t,
                                    start,
                                    end,
                                },
                                Ok(()),
                            ) if t == token => {
                                *state = MapState::Mapped { start, end };
                                Ok(())
                            }
                            (MapState::Pending { token: t, .. }, Err(e)) if t == token => {
                                *state = MapState::Unmapped;
                                Err(e)
                            }
                            // `unmap()` ran before this pending map completed.
                            _ => Err(crate::BufferAsyncError),
                        }
                    };
                    cb(outcome);
                })
            },
        );
        if let Err(err) = registered {
            if let Some(cb) = callback.take() {
                device.defer_rejected_map(cb);
            }
            if let Some(err) = err {
                device.errors().report(err);
            }
        }
    }

    pub fn unmap(&self) {
        *lock(&self.0.map_state) = MapState::Unmapped;
    }

    pub fn size(&self) -> u64 {
        self.0.size
    }

    pub fn usage(&self) -> BufferUsages {
        self.0.usage
    }

    /// The mapped bytes of a host-visible buffer. Valid to read once the
    /// GPU work writing them completed.
    pub fn mapped(&self) -> &[u8] {
        assert!(!self.0.memory.mapped.is_null(), "buffer is not MAP_READ");
        // SAFETY: the mapping covers at least `size` bytes for the buffer's life.
        unsafe { std::slice::from_raw_parts(self.0.memory.mapped, self.0.size as usize) }
    }

    /// Staging writes only: the chunk is not in any submission while filled.
    pub(crate) fn mapped_mut(&mut self) -> &mut [u8] {
        assert!(
            !self.0.memory.mapped.is_null(),
            "buffer is not host-visible"
        );
        // SAFETY: the mapping covers `size` bytes; the staging belt hands a
        // chunk's unwritten range to one writer, under the queue lock.
        unsafe { std::slice::from_raw_parts_mut(self.0.memory.mapped, self.0.size as usize) }
    }

    pub(crate) fn device(&self) -> Device {
        Device(self.0.device.clone())
    }

    pub fn is_same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

pub(crate) struct TextureInner {
    device: Arc<DeviceShared>,
    pub raw: vk::Image,
    /// `None` for a swapchain image (owned by its swapchain) or an invalid placeholder.
    memory: Option<vk::DeviceMemory>,
    /// Present for swapchain images; flipped to `false` under the device queue
    /// lock when the swapchain is reconfigured or released.
    swapchain_live: Option<Arc<AtomicBool>>,
    pub format: TextureFormat,
    pub width: u32,
    pub height: u32,
    pub layers: u32,
    pub usage: TextureUsages,
}

impl Drop for TextureInner {
    fn drop(&mut self) {
        if let Some(memory) = self.memory {
            // SAFETY: no submission still holds this image.
            unsafe {
                if self.raw != vk::Image::null() {
                    self.device.raw.destroy_image(self.raw, None);
                }
                self.device.raw.free_memory(memory, None);
            }
        }
    }
}

#[derive(Clone)]
pub struct Texture(pub(crate) Arc<TextureInner>);

impl Texture {
    pub fn new(device: &Device, desc: &TextureDescriptor<'_>) -> Result<Self, Error> {
        Self::new_inner(device, desc, None)
    }

    #[cfg(test)]
    pub(crate) fn new_with_liveness(
        device: &Device,
        desc: &TextureDescriptor<'_>,
        live: Arc<AtomicBool>,
    ) -> Result<Self, Error> {
        Self::new_inner(device, desc, Some(live))
    }

    fn new_inner(
        device: &Device,
        desc: &TextureDescriptor<'_>,
        swapchain_live: Option<Arc<AtomicBool>>,
    ) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("the device is lost"));
        }
        let d = &device.0.raw;
        let s = desc.size;
        let u = desc.usage;
        if desc.mip_level_count != 1 {
            return Err(Error::validation("native textures have one mip level"));
        }
        if desc.sample_count != 1 {
            return Err(Error::validation(
                "native textures do not support multisampling",
            ));
        }
        if u.is_empty() {
            return Err(Error::validation("texture usage must not be empty"));
        }
        let limits = device.limits();
        if s.width > limits.max_texture_dimension_2d
            || s.height > limits.max_texture_dimension_2d
            || s.depth_or_array_layers > limits.max_texture_array_layers
        {
            return Err(Error::validation("texture dimensions exceed device limits"));
        }
        let mut usage = vk::ImageUsageFlags::empty();
        if u.contains(TextureUsages::COPY_SRC) {
            usage |= vk::ImageUsageFlags::TRANSFER_SRC;
        }
        if u.contains(TextureUsages::COPY_DST) {
            usage |= vk::ImageUsageFlags::TRANSFER_DST;
        }
        if u.contains(TextureUsages::TEXTURE_BINDING) {
            usage |= vk::ImageUsageFlags::SAMPLED;
        }
        if u.contains(TextureUsages::RENDER_ATTACHMENT) {
            usage |= vk::ImageUsageFlags::COLOR_ATTACHMENT;
        }
        let layers = s.depth_or_array_layers.max(1);
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk_format(desc.format))
            .extent(vk::Extent3D {
                width: s.width.max(1),
                height: s.height.max(1),
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(layers)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        // SAFETY: a plain object creation on a live device.
        let raw = unsafe { d.create_image(&info, None) }.map_err(|e| vk_err("vkCreateImage", e))?;
        // SAFETY: `raw` is a live image.
        let req = unsafe { d.get_image_memory_requirements(raw) };
        let memory = match allocate(
            device,
            req,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
            vk::MemoryPropertyFlags::empty(),
        ) {
            Ok(m) => m.raw,
            Err(e) => {
                // SAFETY: `raw` is unused.
                unsafe { d.destroy_image(raw, None) };
                return Err(e);
            }
        };
        // SAFETY: `memory` was allocated for `raw`'s requirements.
        if let Err(e) = unsafe { d.bind_image_memory(raw, memory, 0) } {
            // SAFETY: both are unused.
            unsafe {
                d.destroy_image(raw, None);
                d.free_memory(memory, None);
            }
            return Err(vk_err("vkBindImageMemory", e));
        }
        let texture = Self(Arc::new(TextureInner {
            device: device.0.clone(),
            raw,
            memory: Some(memory),
            swapchain_live,
            format: desc.format,
            width: s.width.max(1),
            height: s.height.max(1),
            layers,
            usage: u,
        }));
        // Into GENERAL, where it stays, before anything uses it.
        device.pend(Cmd::Init(texture.clone()));
        Ok(texture)
    }

    pub(crate) fn invalid(device: &Device, desc: &TextureDescriptor<'_>) -> Self {
        Self(Arc::new(TextureInner {
            device: device.0.clone(),
            raw: vk::Image::null(),
            memory: None,
            swapchain_live: None,
            format: desc.format,
            width: desc.size.width,
            height: desc.size.height,
            layers: desc.size.depth_or_array_layers.max(1),
            usage: desc.usage,
        }))
    }

    /// A swapchain image: not owned, never destroyed here.
    pub(crate) fn external(
        device: &Device,
        raw: vk::Image,
        format: TextureFormat,
        (width, height): (u32, u32),
        live: Arc<AtomicBool>,
    ) -> Self {
        Self(Arc::new(TextureInner {
            device: device.0.clone(),
            raw,
            memory: None,
            swapchain_live: Some(live),
            format,
            width,
            height,
            layers: 1,
            usage: TextureUsages::RENDER_ATTACHMENT,
        }))
    }

    pub(crate) fn device(&self) -> Device {
        Device(self.0.device.clone())
    }

    pub fn is_retired_swapchain(&self) -> bool {
        self.0
            .swapchain_live
            .as_ref()
            .is_some_and(|live| !live.load(Ordering::Acquire))
    }

    pub fn is_valid(&self) -> bool {
        self.0.raw != vk::Image::null() && !self.is_retired_swapchain()
    }

    pub fn format(&self) -> TextureFormat {
        self.0.format
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

    pub fn usage(&self) -> TextureUsages {
        self.0.usage
    }

    pub fn create_view(
        &self,
        dimension: Option<TextureViewDimension>,
    ) -> Result<TextureView, Error> {
        let t = &self.0;
        // Hold the device queue lock across the retirement check and
        // `vkCreateImageView` for swapchain images so `Swap::quiesce` cannot
        // retire and destroy the underlying swapchain image between them.
        let _queue_guard = t.swapchain_live.is_some().then(|| lock(&t.device.queue));
        if self.is_retired_swapchain() {
            return Err(Error::validation(
                "cannot create view of a retired swapchain image",
            ));
        }
        if t.raw == vk::Image::null() {
            return Err(Error::validation(
                "cannot create view of an invalid texture",
            ));
        }
        if self.device().is_lost() {
            return Err(Error::internal("the device is lost"));
        }
        let dimension = dimension.unwrap_or(if t.layers > 1 {
            TextureViewDimension::D2Array
        } else {
            TextureViewDimension::D2
        });
        let (kind, count) = match dimension {
            TextureViewDimension::D2 => (vk::ImageViewType::TYPE_2D, 1),
            TextureViewDimension::D2Array => (vk::ImageViewType::TYPE_2D_ARRAY, t.layers),
        };
        let info = vk::ImageViewCreateInfo::default()
            .image(t.raw)
            .view_type(kind)
            .format(vk_format(t.format))
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: count,
            });
        // SAFETY: `t.raw` is a live image of a matching format; for swapchain
        // images `_queue_guard` serializes against `Swap::quiesce`.
        let raw = unsafe { t.device.raw.create_image_view(&info, None) }
            .map_err(|e| vk_err("vkCreateImageView", e))?;
        Ok(TextureView(Arc::new(ViewInner {
            texture: self.clone(),
            raw,
        })))
    }
}

pub(crate) struct ViewInner {
    pub texture: Texture,
    pub raw: vk::ImageView,
}

impl Drop for ViewInner {
    fn drop(&mut self) {
        if self.raw != vk::ImageView::null() {
            // SAFETY: no submission still holds this view.
            unsafe { self.texture.0.device.raw.destroy_image_view(self.raw, None) };
        }
    }
}

#[derive(Clone)]
pub struct TextureView(pub(crate) Arc<ViewInner>);

impl TextureView {
    pub(crate) fn invalid(texture: Texture) -> Self {
        Self(Arc::new(ViewInner {
            texture,
            raw: vk::ImageView::null(),
        }))
    }

    pub fn is_retired_swapchain(&self) -> bool {
        self.0.texture.is_retired_swapchain()
    }

    pub fn is_valid(&self) -> bool {
        self.0.raw != vk::ImageView::null() && self.0.texture.is_valid()
    }

    pub fn texture(&self) -> &Texture {
        &self.0.texture
    }
}

pub(crate) struct SamplerInner {
    device: Arc<DeviceShared>,
    pub raw: vk::Sampler,
}

impl Drop for SamplerInner {
    fn drop(&mut self) {
        if self.raw != vk::Sampler::null() {
            // SAFETY: no submission still holds this sampler.
            unsafe { self.device.raw.destroy_sampler(self.raw, None) };
        }
    }
}

#[derive(Clone)]
pub struct Sampler(pub(crate) Arc<SamplerInner>);

impl Sampler {
    pub fn new(device: &Device, desc: &SamplerDescriptor<'_>) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("the device is lost"));
        }
        let filter = |f| match f {
            FilterMode::Nearest => vk::Filter::NEAREST,
            FilterMode::Linear => vk::Filter::LINEAR,
        };
        let info = vk::SamplerCreateInfo::default()
            .mag_filter(filter(desc.mag_filter))
            .min_filter(filter(desc.min_filter))
            .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .max_lod(0.25);
        // SAFETY: a plain object creation on a live device.
        let raw = unsafe { device.0.raw.create_sampler(&info, None) }
            .map_err(|e| vk_err("vkCreateSampler", e))?;
        Ok(Self(Arc::new(SamplerInner {
            device: device.0.clone(),
            raw,
        })))
    }

    pub(crate) fn invalid(device: &Device) -> Self {
        Self(Arc::new(SamplerInner {
            device: device.0.clone(),
            raw: vk::Sampler::null(),
        }))
    }

    pub fn is_valid(&self) -> bool {
        self.0.raw != vk::Sampler::null()
    }
}

fn descriptor_type(ty: &BindingType) -> vk::DescriptorType {
    match ty {
        BindingType::Buffer {
            has_dynamic_offset: true,
            ..
        } => vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC,
        BindingType::Buffer { .. } => vk::DescriptorType::UNIFORM_BUFFER,
        BindingType::Texture { .. } => vk::DescriptorType::SAMPLED_IMAGE,
        BindingType::Sampler(_) => vk::DescriptorType::SAMPLER,
    }
}

pub(crate) struct LayoutInner {
    device: Arc<DeviceShared>,
    pub raw: vk::DescriptorSetLayout,
    /// (binding, type) per entry.
    pub entries: Vec<(u32, vk::DescriptorType)>,
}

impl Drop for LayoutInner {
    fn drop(&mut self) {
        if self.raw != vk::DescriptorSetLayout::null() {
            // SAFETY: groups and pipeline layouts built on it hold this Arc.
            unsafe {
                self.device
                    .raw
                    .destroy_descriptor_set_layout(self.raw, None);
            };
        }
    }
}

#[derive(Clone)]
pub struct BindGroupLayout(pub(crate) Arc<LayoutInner>);

impl BindGroupLayout {
    pub fn new(
        device: &Device,
        entries: &[crate::types::BindGroupLayoutEntry],
    ) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("the device is lost"));
        }
        let bindings: Vec<_> = entries
            .iter()
            .map(|e| {
                vk::DescriptorSetLayoutBinding::default()
                    .binding(e.binding)
                    .descriptor_type(descriptor_type(&e.ty))
                    .descriptor_count(1)
                    .stage_flags(stages(e.visibility))
            })
            .collect();
        let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
        // SAFETY: a plain object creation on a live device.
        let raw = unsafe { device.0.raw.create_descriptor_set_layout(&info, None) }
            .map_err(|e| vk_err("vkCreateDescriptorSetLayout", e))?;
        Ok(Self(Arc::new(LayoutInner {
            device: device.0.clone(),
            raw,
            entries: entries
                .iter()
                .map(|e| (e.binding, descriptor_type(&e.ty)))
                .collect(),
        })))
    }

    pub(crate) fn invalid(device: &Device) -> Self {
        Self(Arc::new(LayoutInner {
            device: device.0.clone(),
            raw: vk::DescriptorSetLayout::null(),
            entries: Vec::new(),
        }))
    }

    pub fn is_valid(&self) -> bool {
        self.0.raw != vk::DescriptorSetLayout::null()
    }
}

/// A resource a bind group points at, held while the group lives.
pub enum Bound {
    Buffer {
        buffer: Buffer,
        offset: u64,
        size: Option<u64>,
    },
    View(TextureView),
    Sampler(Sampler),
}

pub(crate) struct GroupInner {
    _layout: BindGroupLayout,
    pool: vk::DescriptorPool,
    pub set: vk::DescriptorSet,
    _bound: Vec<Bound>,
}

impl Drop for GroupInner {
    fn drop(&mut self) {
        if self.pool != vk::DescriptorPool::null() {
            // SAFETY: no submission still holds this group; freeing the pool frees the set.
            unsafe {
                self._layout
                    .0
                    .device
                    .raw
                    .destroy_descriptor_pool(self.pool, None);
            }
        }
    }
}

#[derive(Clone)]
pub struct BindGroup(pub(crate) Arc<GroupInner>);

impl BindGroup {
    pub fn new(
        device: &Device,
        layout: &BindGroupLayout,
        entries: Vec<(u32, Bound)>,
    ) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("the device is lost"));
        }
        if !layout.is_valid() {
            return Err(Error::validation("bind group layout is invalid"));
        }
        for (_, bound) in &entries {
            match bound {
                Bound::Buffer { buffer, .. } if !buffer.is_valid() => {
                    return Err(Error::validation("bind group references an invalid buffer"));
                }
                Bound::View(view) if view.is_retired_swapchain() => {
                    return Err(Error::validation(
                        "bind group references a retired swapchain image",
                    ));
                }
                Bound::View(view) if !view.is_valid() => {
                    return Err(Error::validation(
                        "bind group references an invalid texture view",
                    ));
                }
                Bound::Sampler(sampler) if !sampler.is_valid() => {
                    return Err(Error::validation(
                        "bind group references an invalid sampler",
                    ));
                }
                _ => {}
            }
        }
        let d = &device.0.raw;
        let mut sizes: Vec<vk::DescriptorPoolSize> = Vec::new();
        for (_, ty) in &layout.0.entries {
            match sizes.iter_mut().find(|s| s.ty == *ty) {
                Some(s) => s.descriptor_count += 1,
                None => sizes.push(vk::DescriptorPoolSize {
                    ty: *ty,
                    descriptor_count: 1,
                }),
            }
        }
        let info = vk::DescriptorPoolCreateInfo::default()
            .max_sets(1)
            .pool_sizes(&sizes);
        // SAFETY: a plain object creation on a live device.
        let pool = unsafe { d.create_descriptor_pool(&info, None) }
            .map_err(|e| vk_err("vkCreateDescriptorPool", e))?;
        let layouts = [layout.0.raw];
        let alloc = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(pool)
            .set_layouts(&layouts);
        // SAFETY: `pool` was sized for exactly this layout.
        let set = match unsafe { d.allocate_descriptor_sets(&alloc) } {
            Ok(sets) => sets[0],
            Err(e) => {
                // SAFETY: `pool` is unused.
                unsafe { d.destroy_descriptor_pool(pool, None) };
                return Err(vk_err("vkAllocateDescriptorSets", e));
            }
        };
        let mut buffers = Vec::with_capacity(entries.len());
        let mut images = Vec::with_capacity(entries.len());
        for (_, bound) in &entries {
            match bound {
                Bound::Buffer {
                    buffer,
                    offset,
                    size,
                } => buffers.push(vk::DescriptorBufferInfo {
                    buffer: buffer.0.raw,
                    offset: *offset,
                    range: size.unwrap_or(vk::WHOLE_SIZE),
                }),
                Bound::View(view) => images.push(vk::DescriptorImageInfo {
                    sampler: vk::Sampler::null(),
                    image_view: view.0.raw,
                    image_layout: vk::ImageLayout::GENERAL,
                }),
                Bound::Sampler(sampler) => images.push(vk::DescriptorImageInfo {
                    sampler: sampler.0.raw,
                    image_view: vk::ImageView::null(),
                    image_layout: vk::ImageLayout::UNDEFINED,
                }),
            }
        }
        let (mut b, mut i) = (0, 0);
        let writes: Vec<vk::WriteDescriptorSet<'_>> = entries
            .iter()
            .map(|(binding, bound)| {
                let ty = layout
                    .0
                    .entries
                    .iter()
                    .find(|(n, _)| n == binding)
                    .map_or(vk::DescriptorType::SAMPLER, |(_, t)| *t);
                let w = vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(*binding)
                    .descriptor_type(ty);
                match bound {
                    Bound::Buffer { .. } => {
                        b += 1;
                        w.buffer_info(&buffers[b - 1..b])
                    }
                    Bound::View(_) | Bound::Sampler(_) => {
                        i += 1;
                        w.image_info(&images[i - 1..i])
                    }
                }
            })
            .collect();
        // SAFETY: every write targets a binding of `set`'s layout with live handles.
        unsafe { d.update_descriptor_sets(&writes, &[]) };
        Ok(Self(Arc::new(GroupInner {
            _layout: layout.clone(),
            pool,
            set,
            _bound: entries.into_iter().map(|(_, b)| b).collect(),
        })))
    }

    pub(crate) fn invalid(layout: &BindGroupLayout) -> Self {
        Self(Arc::new(GroupInner {
            _layout: layout.clone(),
            pool: vk::DescriptorPool::null(),
            set: vk::DescriptorSet::null(),
            _bound: Vec::new(),
        }))
    }

    pub fn has_retired_swapchain(&self) -> bool {
        self.0
            ._bound
            .iter()
            .any(|b| matches!(b, Bound::View(v) if v.is_retired_swapchain()))
    }

    pub fn is_valid(&self) -> bool {
        self.0.set != vk::DescriptorSet::null()
            && self.0._bound.iter().all(|b| match b {
                Bound::Buffer { buffer, .. } => buffer.is_valid(),
                Bound::View(v) => v.is_valid(),
                Bound::Sampler(s) => s.is_valid(),
            })
    }
}

pub(crate) struct PipelineLayoutInner {
    device: Arc<DeviceShared>,
    pub raw: vk::PipelineLayout,
    pub sets: Vec<BindGroupLayout>,
}

impl Drop for PipelineLayoutInner {
    fn drop(&mut self) {
        if self.raw != vk::PipelineLayout::null() {
            // SAFETY: pipelines built on it hold this Arc.
            unsafe { self.device.raw.destroy_pipeline_layout(self.raw, None) };
        }
    }
}

#[derive(Clone)]
pub struct PipelineLayout(pub(crate) Arc<PipelineLayoutInner>);

impl PipelineLayout {
    pub fn new(device: &Device, sets: &[BindGroupLayout]) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("the device is lost"));
        }
        if sets.iter().any(|s| !s.is_valid()) {
            return Err(Error::validation(
                "pipeline layout references an invalid bind group layout",
            ));
        }
        let raws: Vec<_> = sets.iter().map(|s| s.0.raw).collect();
        let info = vk::PipelineLayoutCreateInfo::default().set_layouts(&raws);
        // SAFETY: a plain object creation on a live device.
        let raw = unsafe { device.0.raw.create_pipeline_layout(&info, None) }
            .map_err(|e| vk_err("vkCreatePipelineLayout", e))?;
        Ok(Self(Arc::new(PipelineLayoutInner {
            device: device.0.clone(),
            raw,
            sets: sets.to_vec(),
        })))
    }

    pub(crate) fn invalid(device: &Device) -> Self {
        Self(Arc::new(PipelineLayoutInner {
            device: device.0.clone(),
            raw: vk::PipelineLayout::null(),
            sets: Vec::new(),
        }))
    }

    pub fn is_valid(&self) -> bool {
        self.0.raw != vk::PipelineLayout::null()
    }
}

pub(crate) struct ModuleInner {
    device: Arc<DeviceShared>,
    pub raw: vk::ShaderModule,
}

impl Drop for ModuleInner {
    fn drop(&mut self) {
        if self.raw != vk::ShaderModule::null() {
            // SAFETY: pipelines keep no reference to a module once created.
            unsafe { self.device.raw.destroy_shader_module(self.raw, None) };
        }
    }
}

#[derive(Clone)]
pub struct ShaderModule(pub(crate) Arc<ModuleInner>);

impl ShaderModule {
    pub fn new(device: &Device, shader: mui_shaders::ShaderId) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("the device is lost"));
        }
        let info = vk::ShaderModuleCreateInfo::default().code(shader.artifact().spirv);
        // SAFETY: the words are SPIR-V naga emitted for Vulkan 1.1+.
        let raw = unsafe { device.0.raw.create_shader_module(&info, None) }
            .map_err(|e| vk_err("vkCreateShaderModule", e))?;
        Ok(Self(Arc::new(ModuleInner {
            device: device.0.clone(),
            raw,
        })))
    }

    pub(crate) fn invalid(device: &Device) -> Self {
        Self(Arc::new(ModuleInner {
            device: device.0.clone(),
            raw: vk::ShaderModule::null(),
        }))
    }

    pub fn is_valid(&self) -> bool {
        self.0.raw != vk::ShaderModule::null()
    }
}

pub(crate) struct CacheInner {
    device: Arc<DeviceShared>,
    pub raw: vk::PipelineCache,
}

impl Drop for CacheInner {
    fn drop(&mut self) {
        if self.raw != vk::PipelineCache::null() {
            // SAFETY: pipeline creation holds no cache past its call.
            unsafe { self.device.raw.destroy_pipeline_cache(self.raw, None) };
        }
    }
}

#[derive(Clone)]
pub struct PipelineCache(pub(crate) Arc<CacheInner>);

impl PipelineCache {
    /// A cache primed with `data` when the driver accepts it (it checks the
    /// header), else empty.
    pub fn new(device: &Device, data: Option<&[u8]>) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("the device is lost"));
        }
        let d = &device.0.raw;
        let make = |bytes: &[u8]| {
            let info = vk::PipelineCacheCreateInfo::default().initial_data(bytes);
            // SAFETY: the driver validates the blob's header against itself.
            unsafe { d.create_pipeline_cache(&info, None) }
        };
        let raw = match data.map(make) {
            Some(Ok(raw)) => raw,
            _ => make(&[]).map_err(|e| vk_err("vkCreatePipelineCache", e))?,
        };
        Ok(Self(Arc::new(CacheInner {
            device: device.0.clone(),
            raw,
        })))
    }

    pub(crate) fn invalid(device: &Device) -> Self {
        Self(Arc::new(CacheInner {
            device: device.0.clone(),
            raw: vk::PipelineCache::null(),
        }))
    }

    pub fn is_valid(&self) -> bool {
        self.0.raw != vk::PipelineCache::null()
    }

    pub fn get_data(&self) -> Option<Vec<u8>> {
        if !self.is_valid() {
            return None;
        }
        // SAFETY: a plain query on a live cache.
        unsafe { self.0.device.raw.get_pipeline_cache_data(self.0.raw).ok() }
    }
}

pub(crate) struct PipelineInner {
    device: Arc<DeviceShared>,
    pub raw: vk::Pipeline,
    pub layout: PipelineLayout,
}

impl Drop for PipelineInner {
    fn drop(&mut self) {
        if self.raw != vk::Pipeline::null() {
            // SAFETY: no submission still holds this pipeline.
            unsafe { self.device.raw.destroy_pipeline(self.raw, None) };
        }
    }
}

#[derive(Clone)]
pub struct RenderPipeline(pub(crate) Arc<PipelineInner>);

/// What a render pipeline is built from, backend-neutral.
pub struct PipelineDesc<'a> {
    pub layout: &'a PipelineLayout,
    pub vertex: (&'a ShaderModule, &'a str),
    pub fragment: (&'a ShaderModule, &'a str),
    pub buffers: &'a [crate::types::VertexBufferLayout<'a>],
    pub topology: PrimitiveTopology,
    pub target: crate::types::ColorTargetState,
    pub cache: Option<&'a PipelineCache>,
}

fn factor(f: BlendFactor) -> vk::BlendFactor {
    match f {
        BlendFactor::Zero => vk::BlendFactor::ZERO,
        BlendFactor::One => vk::BlendFactor::ONE,
        BlendFactor::SrcAlpha => vk::BlendFactor::SRC_ALPHA,
        BlendFactor::OneMinusSrcAlpha => vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
        BlendFactor::DstAlpha => vk::BlendFactor::DST_ALPHA,
        BlendFactor::OneMinusDstAlpha => vk::BlendFactor::ONE_MINUS_DST_ALPHA,
    }
}

fn blend(state: Option<BlendState>, mask: ColorWrites) -> vk::PipelineColorBlendAttachmentState {
    let base = vk::PipelineColorBlendAttachmentState::default()
        .color_write_mask(vk::ColorComponentFlags::from_raw(mask.bits()));
    match state {
        None => base,
        Some(b) => base
            .blend_enable(true)
            .src_color_blend_factor(factor(b.color.src_factor))
            .dst_color_blend_factor(factor(b.color.dst_factor))
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(factor(b.alpha.src_factor))
            .dst_alpha_blend_factor(factor(b.alpha.dst_factor))
            .alpha_blend_op(vk::BlendOp::ADD),
    }
}

impl RenderPipeline {
    pub fn new(device: &Device, desc: &PipelineDesc<'_>) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("the device is lost"));
        }
        if !desc.layout.is_valid() {
            return Err(Error::validation("render pipeline layout is invalid"));
        }
        if !desc.vertex.0.is_valid() || !desc.fragment.0.is_valid() {
            return Err(Error::validation(
                "render pipeline references an invalid shader module",
            ));
        }
        if let Some(cache) = desc.cache
            && !cache.is_valid()
        {
            return Err(Error::validation(
                "render pipeline references an invalid pipeline cache",
            ));
        }
        let name = |s: &str| {
            CString::new(s).map_err(|e| Error::validation(err("entry point name", e).to_string()))
        };
        let (vs, fs) = (name(desc.vertex.1)?, name(desc.fragment.1)?);
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(desc.vertex.0.0.raw)
                .name(&vs),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(desc.fragment.0.0.raw)
                .name(&fs),
        ];
        let mut bindings = Vec::new();
        let mut attributes = Vec::new();
        for (i, b) in desc.buffers.iter().enumerate() {
            bindings.push(
                vk::VertexInputBindingDescription::default()
                    .binding(i as u32)
                    .stride(b.array_stride as u32)
                    .input_rate(match b.step_mode {
                        VertexStepMode::Vertex => vk::VertexInputRate::VERTEX,
                        VertexStepMode::Instance => vk::VertexInputRate::INSTANCE,
                    }),
            );
            for a in b.attributes {
                attributes.push(
                    vk::VertexInputAttributeDescription::default()
                        .location(a.shader_location)
                        .binding(i as u32)
                        .format(match a.format {
                            VertexFormat::Float32x2 => vk::Format::R32G32_SFLOAT,
                            VertexFormat::Float32x4 => vk::Format::R32G32B32A32_SFLOAT,
                        })
                        .offset(a.offset as u32),
                );
            }
        }
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&bindings)
            .vertex_attribute_descriptions(&attributes);
        let assembly =
            vk::PipelineInputAssemblyStateCreateInfo::default().topology(match desc.topology {
                PrimitiveTopology::TriangleList => vk::PrimitiveTopology::TRIANGLE_LIST,
                PrimitiveTopology::TriangleStrip => vk::PrimitiveTopology::TRIANGLE_STRIP,
            });
        let viewport = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let attachments = [blend(desc.target.blend, desc.target.write_mask)];
        let color = vk::PipelineColorBlendStateCreateInfo::default().attachments(&attachments);
        let dynamic = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic);
        let formats = [vk_format(desc.target.format)];
        let mut rendering =
            vk::PipelineRenderingCreateInfo::default().color_attachment_formats(&formats);
        let info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&multisample)
            .color_blend_state(&color)
            .dynamic_state(&dynamic)
            .layout(desc.layout.0.raw)
            .push_next(&mut rendering);
        let cache = desc.cache.map_or(vk::PipelineCache::null(), |c| c.0.raw);
        // SAFETY: every handle and array `info` points at outlives the call;
        // dynamic rendering is enabled on the device.
        let raw = unsafe { device.0.raw.create_graphics_pipelines(cache, &[info], None) }
            .map_err(|(_, e)| vk_err("vkCreateGraphicsPipelines", e))?[0];
        Ok(Self(Arc::new(PipelineInner {
            device: device.0.clone(),
            raw,
            layout: desc.layout.clone(),
        })))
    }

    pub(crate) fn invalid(device: &Device, layout: &PipelineLayout) -> Self {
        Self(Arc::new(PipelineInner {
            device: device.0.clone(),
            raw: vk::Pipeline::null(),
            layout: layout.clone(),
        }))
    }

    pub fn is_valid(&self) -> bool {
        self.0.raw != vk::Pipeline::null()
    }
}

pub(crate) struct QueryInner {
    device: Arc<DeviceShared>,
    pub raw: vk::QueryPool,
}

impl Drop for QueryInner {
    fn drop(&mut self) {
        if self.raw != vk::QueryPool::null() {
            // SAFETY: no submission still holds this pool.
            unsafe { self.device.raw.destroy_query_pool(self.raw, None) };
        }
    }
}

#[derive(Clone)]
pub struct QuerySet(pub(crate) Arc<QueryInner>);

impl QuerySet {
    pub fn new(device: &Device, desc: &QuerySetDescriptor<'_>) -> Result<Self, Error> {
        if device.is_lost() {
            return Err(Error::internal("the device is lost"));
        }
        let info = vk::QueryPoolCreateInfo::default()
            .query_type(vk::QueryType::TIMESTAMP)
            .query_count(desc.count.max(1));
        // SAFETY: a plain object creation on a live device.
        let raw = unsafe { device.0.raw.create_query_pool(&info, None) }
            .map_err(|e| vk_err("vkCreateQueryPool", e))?;
        Ok(Self(Arc::new(QueryInner {
            device: device.0.clone(),
            raw,
        })))
    }

    pub(crate) fn invalid(device: &Device) -> Self {
        Self(Arc::new(QueryInner {
            device: device.0.clone(),
            raw: vk::QueryPool::null(),
        }))
    }

    pub fn is_valid(&self) -> bool {
        self.0.raw != vk::QueryPool::null()
    }
}
