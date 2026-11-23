//! Conversions between this crate's types and wgpu's, for the `wgpu`
//! passthrough backend.

use crate::types::*;

pub fn backend(b: wgpu::Backend) -> Backend {
    match b {
        wgpu::Backend::Vulkan => Backend::Vulkan,
        wgpu::Backend::Metal => Backend::Metal,
        wgpu::Backend::Dx12 => Backend::Dx12,
        wgpu::Backend::Gl => Backend::Gl,
        wgpu::Backend::BrowserWebGpu => Backend::BrowserWebGpu,
        _ => Backend::Noop,
    }
}

const BACKENDS: [(Backends, wgpu::Backends); 5] = [
    (Backends::VULKAN, wgpu::Backends::VULKAN),
    (Backends::GL, wgpu::Backends::GL),
    (Backends::METAL, wgpu::Backends::METAL),
    (Backends::DX12, wgpu::Backends::DX12),
    (Backends::BROWSER_WEBGPU, wgpu::Backends::BROWSER_WEBGPU),
];

pub fn backends(b: Backends) -> wgpu::Backends {
    BACKENDS
        .iter()
        .filter(|(ours, _)| b.contains(*ours))
        .fold(wgpu::Backends::empty(), |out, (_, theirs)| out | *theirs)
}

pub fn from_backends(b: wgpu::Backends) -> Backends {
    BACKENDS
        .iter()
        .filter(|(_, theirs)| b.contains(*theirs))
        .fold(Backends::empty(), |out, (ours, _)| out | *ours)
}

pub fn adapter_info(i: &wgpu::AdapterInfo) -> AdapterInfo {
    AdapterInfo {
        name: i.name.clone(),
        vendor: i.vendor,
        device: i.device,
        device_type: match i.device_type {
            wgpu::DeviceType::IntegratedGpu => DeviceType::IntegratedGpu,
            wgpu::DeviceType::DiscreteGpu => DeviceType::DiscreteGpu,
            wgpu::DeviceType::VirtualGpu => DeviceType::VirtualGpu,
            wgpu::DeviceType::Cpu => DeviceType::Cpu,
            wgpu::DeviceType::Other => DeviceType::Other,
        },
        driver: i.driver.clone(),
        driver_info: i.driver_info.clone(),
        backend: backend(i.backend),
    }
}

pub fn limits(l: &wgpu::Limits) -> Limits {
    Limits {
        max_texture_dimension_2d: l.max_texture_dimension_2d,
        max_texture_array_layers: l.max_texture_array_layers,
        max_bind_groups: l.max_bind_groups,
        max_uniform_buffer_binding_size: l.max_uniform_buffer_binding_size,
        min_uniform_buffer_offset_alignment: l.min_uniform_buffer_offset_alignment,
        max_vertex_attributes: l.max_vertex_attributes,
        max_vertex_buffer_array_stride: l.max_vertex_buffer_array_stride,
        max_inter_stage_shader_variables: l.max_inter_stage_shader_variables,
        max_buffer_size: l.max_buffer_size,
    }
}

/// `l` over WebGL2's floor, which every adapter meets.
pub fn to_limits(l: &Limits) -> wgpu::Limits {
    wgpu::Limits {
        max_texture_dimension_2d: l.max_texture_dimension_2d,
        max_texture_array_layers: l.max_texture_array_layers,
        max_bind_groups: l.max_bind_groups,
        max_uniform_buffer_binding_size: l.max_uniform_buffer_binding_size,
        min_uniform_buffer_offset_alignment: l.min_uniform_buffer_offset_alignment,
        max_vertex_attributes: l.max_vertex_attributes,
        max_vertex_buffer_array_stride: l.max_vertex_buffer_array_stride,
        max_inter_stage_shader_variables: l.max_inter_stage_shader_variables,
        max_buffer_size: l.max_buffer_size,
        ..wgpu::Limits::downlevel_webgl2_defaults()
    }
}

pub fn features(f: wgpu::Features) -> Features {
    let mut out = Features::empty();
    for (theirs, ours) in [
        (wgpu::Features::TIMESTAMP_QUERY, Features::TIMESTAMP_QUERY),
        (
            wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
            Features::TIMESTAMP_QUERY_INSIDE_ENCODERS,
        ),
        (wgpu::Features::PIPELINE_CACHE, Features::PIPELINE_CACHE),
    ] {
        if f.contains(theirs) {
            out |= ours;
        }
    }
    out
}

pub fn to_features(f: Features) -> wgpu::Features {
    let mut out = wgpu::Features::empty();
    if f.contains(Features::TIMESTAMP_QUERY) {
        out |= wgpu::Features::TIMESTAMP_QUERY;
    }
    if f.contains(Features::TIMESTAMP_QUERY_INSIDE_ENCODERS) {
        out |= wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    }
    if f.contains(Features::PIPELINE_CACHE) {
        out |= wgpu::Features::PIPELINE_CACHE;
    }
    out
}

pub fn format(f: TextureFormat) -> wgpu::TextureFormat {
    match f {
        TextureFormat::R8Unorm => wgpu::TextureFormat::R8Unorm,
        TextureFormat::Rgba8Unorm => wgpu::TextureFormat::Rgba8Unorm,
        TextureFormat::Rgba8UnormSrgb => wgpu::TextureFormat::Rgba8UnormSrgb,
        TextureFormat::Bgra8Unorm => wgpu::TextureFormat::Bgra8Unorm,
        TextureFormat::Bgra8UnormSrgb => wgpu::TextureFormat::Bgra8UnormSrgb,
        TextureFormat::Rgba16Float => wgpu::TextureFormat::Rgba16Float,
        TextureFormat::Rgba32Float => wgpu::TextureFormat::Rgba32Float,
    }
}

pub fn from_format(f: wgpu::TextureFormat) -> Option<TextureFormat> {
    Some(match f {
        wgpu::TextureFormat::R8Unorm => TextureFormat::R8Unorm,
        wgpu::TextureFormat::Rgba8Unorm => TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb => TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Bgra8Unorm => TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb => TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba16Float => TextureFormat::Rgba16Float,
        wgpu::TextureFormat::Rgba32Float => TextureFormat::Rgba32Float,
        _ => return None,
    })
}

pub fn texture_usages(u: TextureUsages) -> wgpu::TextureUsages {
    wgpu::TextureUsages::from_bits_truncate(u.bits())
}

pub fn from_texture_usages(u: wgpu::TextureUsages) -> TextureUsages {
    TextureUsages::from_bits_truncate(u.bits())
}

pub fn buffer_usages(u: BufferUsages) -> wgpu::BufferUsages {
    wgpu::BufferUsages::from_bits_truncate(u.bits())
}

pub fn from_buffer_usages(u: wgpu::BufferUsages) -> BufferUsages {
    BufferUsages::from_bits_truncate(u.bits())
}

pub fn stages(s: ShaderStages) -> wgpu::ShaderStages {
    let mut out = wgpu::ShaderStages::NONE;
    if s.contains(ShaderStages::VERTEX) {
        out |= wgpu::ShaderStages::VERTEX;
    }
    if s.contains(ShaderStages::FRAGMENT) {
        out |= wgpu::ShaderStages::FRAGMENT;
    }
    out
}

pub fn view_dimension(d: TextureViewDimension) -> wgpu::TextureViewDimension {
    match d {
        TextureViewDimension::D2 => wgpu::TextureViewDimension::D2,
        TextureViewDimension::D2Array => wgpu::TextureViewDimension::D2Array,
    }
}

pub fn layout_entry(e: &BindGroupLayoutEntry) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding: e.binding,
        visibility: stages(e.visibility),
        ty: match e.ty {
            BindingType::Buffer {
                has_dynamic_offset,
                min_binding_size,
                ..
            } => wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset,
                min_binding_size,
            },
            BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable },
                view_dimension: d,
                multisampled,
            } => wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable },
                view_dimension: view_dimension(d),
                multisampled,
            },
            BindingType::Sampler(SamplerBindingType::Filtering) => {
                wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)
            }
        },
        count: e.count,
    }
}

fn factor(f: BlendFactor) -> wgpu::BlendFactor {
    match f {
        BlendFactor::Zero => wgpu::BlendFactor::Zero,
        BlendFactor::One => wgpu::BlendFactor::One,
        BlendFactor::SrcAlpha => wgpu::BlendFactor::SrcAlpha,
        BlendFactor::OneMinusSrcAlpha => wgpu::BlendFactor::OneMinusSrcAlpha,
        BlendFactor::DstAlpha => wgpu::BlendFactor::DstAlpha,
        BlendFactor::OneMinusDstAlpha => wgpu::BlendFactor::OneMinusDstAlpha,
    }
}

fn component(c: BlendComponent) -> wgpu::BlendComponent {
    wgpu::BlendComponent {
        src_factor: factor(c.src_factor),
        dst_factor: factor(c.dst_factor),
        operation: wgpu::BlendOperation::Add,
    }
}

pub fn target(t: &ColorTargetState) -> wgpu::ColorTargetState {
    wgpu::ColorTargetState {
        format: format(t.format),
        blend: t.blend.map(|b| wgpu::BlendState {
            color: component(b.color),
            alpha: component(b.alpha),
        }),
        write_mask: wgpu::ColorWrites::from_bits_truncate(t.write_mask.bits()),
    }
}

pub fn attribute(a: &VertexAttribute) -> wgpu::VertexAttribute {
    wgpu::VertexAttribute {
        format: match a.format {
            VertexFormat::Float32x2 => wgpu::VertexFormat::Float32x2,
            VertexFormat::Float32x4 => wgpu::VertexFormat::Float32x4,
        },
        offset: a.offset,
        shader_location: a.shader_location,
    }
}

pub fn step_mode(s: VertexStepMode) -> wgpu::VertexStepMode {
    match s {
        VertexStepMode::Vertex => wgpu::VertexStepMode::Vertex,
        VertexStepMode::Instance => wgpu::VertexStepMode::Instance,
    }
}

pub fn topology(t: PrimitiveTopology) -> wgpu::PrimitiveTopology {
    match t {
        PrimitiveTopology::TriangleList => wgpu::PrimitiveTopology::TriangleList,
        PrimitiveTopology::TriangleStrip => wgpu::PrimitiveTopology::TriangleStrip,
    }
}

pub fn extent(e: Extent3d) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: e.width,
        height: e.height,
        depth_or_array_layers: e.depth_or_array_layers,
    }
}

pub fn origin(o: Origin3d) -> wgpu::Origin3d {
    wgpu::Origin3d {
        x: o.x,
        y: o.y,
        z: o.z,
    }
}

pub fn buffer_layout(l: TexelCopyBufferLayout) -> wgpu::TexelCopyBufferLayout {
    wgpu::TexelCopyBufferLayout {
        offset: l.offset,
        bytes_per_row: l.bytes_per_row,
        rows_per_image: l.rows_per_image,
    }
}

pub fn color(c: Color) -> wgpu::Color {
    wgpu::Color {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
}

pub fn filter(f: FilterMode) -> wgpu::FilterMode {
    match f {
        FilterMode::Nearest => wgpu::FilterMode::Nearest,
        FilterMode::Linear => wgpu::FilterMode::Linear,
    }
}

pub fn error(e: wgpu::Error) -> crate::Error {
    match e {
        wgpu::Error::OutOfMemory { source } => crate::Error::OutOfMemory {
            source: source.to_string(),
        },
        wgpu::Error::Validation { description, .. } => crate::Error::Validation { description },
        wgpu::Error::Internal { description, .. } => crate::Error::Internal { description },
    }
}

pub fn error_filter(f: ErrorFilter) -> wgpu::ErrorFilter {
    match f {
        ErrorFilter::Validation => wgpu::ErrorFilter::Validation,
        ErrorFilter::OutOfMemory => wgpu::ErrorFilter::OutOfMemory,
        ErrorFilter::Internal => wgpu::ErrorFilter::Internal,
    }
}

pub fn present_mode(m: PresentMode) -> wgpu::PresentMode {
    match m {
        PresentMode::AutoVsync => wgpu::PresentMode::AutoVsync,
        PresentMode::AutoNoVsync => wgpu::PresentMode::AutoNoVsync,
        PresentMode::Fifo => wgpu::PresentMode::Fifo,
        PresentMode::FifoRelaxed => wgpu::PresentMode::FifoRelaxed,
        PresentMode::Immediate => wgpu::PresentMode::Immediate,
        PresentMode::Mailbox => wgpu::PresentMode::Mailbox,
    }
}

pub fn from_present_mode(m: wgpu::PresentMode) -> PresentMode {
    match m {
        wgpu::PresentMode::AutoVsync => PresentMode::AutoVsync,
        wgpu::PresentMode::AutoNoVsync => PresentMode::AutoNoVsync,
        wgpu::PresentMode::Fifo => PresentMode::Fifo,
        wgpu::PresentMode::FifoRelaxed => PresentMode::FifoRelaxed,
        wgpu::PresentMode::Immediate => PresentMode::Immediate,
        wgpu::PresentMode::Mailbox => PresentMode::Mailbox,
    }
}

pub fn alpha_mode(m: CompositeAlphaMode) -> wgpu::CompositeAlphaMode {
    match m {
        CompositeAlphaMode::Auto => wgpu::CompositeAlphaMode::Auto,
        CompositeAlphaMode::Opaque => wgpu::CompositeAlphaMode::Opaque,
        CompositeAlphaMode::PreMultiplied => wgpu::CompositeAlphaMode::PreMultiplied,
        CompositeAlphaMode::PostMultiplied => wgpu::CompositeAlphaMode::PostMultiplied,
        CompositeAlphaMode::Inherit => wgpu::CompositeAlphaMode::Inherit,
    }
}

pub fn from_alpha_mode(m: wgpu::CompositeAlphaMode) -> CompositeAlphaMode {
    match m {
        wgpu::CompositeAlphaMode::Auto => CompositeAlphaMode::Auto,
        wgpu::CompositeAlphaMode::Opaque => CompositeAlphaMode::Opaque,
        wgpu::CompositeAlphaMode::PreMultiplied => CompositeAlphaMode::PreMultiplied,
        wgpu::CompositeAlphaMode::PostMultiplied => CompositeAlphaMode::PostMultiplied,
        wgpu::CompositeAlphaMode::Inherit => CompositeAlphaMode::Inherit,
    }
}

pub fn surface_config(c: &SurfaceConfiguration) -> wgpu::SurfaceConfiguration {
    wgpu::SurfaceConfiguration {
        usage: texture_usages(c.usage),
        format: format(c.format),
        width: c.width,
        height: c.height,
        present_mode: present_mode(c.present_mode),
        desired_maximum_frame_latency: c.desired_maximum_frame_latency,
        alpha_mode: alpha_mode(c.alpha_mode),
        view_formats: c.view_formats.iter().copied().map(format).collect(),
        color_space: wgpu::SurfaceColorSpace::Auto,
    }
}

pub fn shader_source(id: mui_shaders::ShaderId) -> &'static str {
    match id {
        mui_shaders::ShaderId::Quad => mui_shaders::QUAD_WGSL,
        mui_shaders::ShaderId::Blur => mui_shaders::BLUR_WGSL,
        mui_shaders::ShaderId::Weld => mui_shaders::WELD_WGSL,
        mui_shaders::ShaderId::Present => mui_shaders::PRESENT_WGSL,
    }
}
