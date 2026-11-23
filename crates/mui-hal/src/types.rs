//! Plain data: descriptors, formats and flags. Names and shapes follow
//! wgpu's, cut to what MUI's renderer uses.

use bitflags::bitflags;

/// The API a device runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Backend {
    Vulkan,
    Metal,
    Dx12,
    D3d11,
    Gl,
    BrowserWebGpu,
    /// No GPU: a placeholder for tests and reports.
    Noop,
}

impl Backend {
    /// The backend's name in reports.
    pub const fn to_str(self) -> &'static str {
        match self {
            Self::Vulkan => "vulkan",
            Self::Metal => "metal",
            Self::Dx12 => "dx12",
            Self::D3d11 => "d3d11",
            Self::Gl => "gl",
            Self::BrowserWebGpu => "webgpu",
            Self::Noop => "noop",
        }
    }
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.to_str())
    }
}

bitflags! {
    /// A set of [`Backend`]s.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct Backends: u32 {
        const VULKAN = 1 << 0;
        const GL = 1 << 1;
        const METAL = 1 << 2;
        const DX12 = 1 << 3;
        const D3D11 = 1 << 4;
        const BROWSER_WEBGPU = 1 << 5;
        /// The backends a native window renders through by default.
        const PRIMARY = Self::VULKAN.bits() | Self::METAL.bits() | Self::DX12.bits() | Self::D3D11.bits() | Self::BROWSER_WEBGPU.bits();
        const SECONDARY = Self::GL.bits();
    }
}

impl Backends {
    /// Backends named in a comma-separated list, as wgpu parses
    /// `WGPU_BACKEND`; unknown names select nothing.
    pub fn from_comma_list(list: &str) -> Self {
        list.split(',')
            .map(|name| match name.trim().to_ascii_lowercase().as_str() {
                "vulkan" | "vk" => Self::VULKAN,
                "dx12" | "d3d12" => Self::DX12,
                "d3d11" => Self::D3D11,
                "metal" | "mtl" => Self::METAL,
                "opengl" | "gles" | "gl" => Self::GL,
                "webgpu" => Self::BROWSER_WEBGPU,
                _ => Self::empty(),
            })
            .fold(Self::empty(), |all, b| all | b)
    }
}

impl From<Backend> for Backends {
    fn from(b: Backend) -> Self {
        match b {
            Backend::Vulkan => Self::VULKAN,
            Backend::Metal => Self::METAL,
            Backend::Dx12 => Self::DX12,
            Backend::D3d11 => Self::D3D11,
            Backend::Gl => Self::GL,
            Backend::BrowserWebGpu => Self::BROWSER_WEBGPU,
            Backend::Noop => Self::empty(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DeviceType {
    #[default]
    Other,
    IntegratedGpu,
    DiscreteGpu,
    VirtualGpu,
    Cpu,
}

/// What an adapter says about itself.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AdapterInfo {
    pub name: String,
    pub vendor: u32,
    pub device: u32,
    pub device_type: DeviceType,
    pub driver: String,
    pub driver_info: String,
    pub backend: Backend,
}

impl AdapterInfo {
    /// An unnamed adapter of `device_type` on `backend`, for tests.
    pub fn new(device_type: DeviceType, backend: Backend) -> Self {
        Self {
            name: String::new(),
            vendor: 0,
            device: 0,
            device_type,
            driver: String::new(),
            driver_info: String::new(),
            backend,
        }
    }
}

/// The limits MUI's renderer checks. A device grants at least
/// [`Limits::downlevel_webgl2_defaults`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Limits {
    pub max_texture_dimension_2d: u32,
    pub max_texture_array_layers: u32,
    pub max_bind_groups: u32,
    pub max_uniform_buffer_binding_size: u64,
    pub min_uniform_buffer_offset_alignment: u32,
    pub max_vertex_attributes: u32,
    pub max_vertex_buffer_array_stride: u32,
    pub max_inter_stage_shader_variables: u32,
    pub max_buffer_size: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_texture_dimension_2d: 8192,
            max_texture_array_layers: 256,
            max_bind_groups: 4,
            max_uniform_buffer_binding_size: 64 << 10,
            min_uniform_buffer_offset_alignment: 256,
            max_vertex_attributes: 16,
            max_vertex_buffer_array_stride: 2048,
            max_inter_stage_shader_variables: 16,
            max_buffer_size: 256 << 20,
        }
    }
}

impl Limits {
    /// What WebGL2 guarantees: the floor every adapter must meet.
    pub fn downlevel_webgl2_defaults() -> Self {
        Self {
            max_texture_dimension_2d: 2048,
            max_uniform_buffer_binding_size: 16 << 10,
            max_vertex_buffer_array_stride: 255,
            max_inter_stage_shader_variables: 15,
            ..Self::default()
        }
    }

    /// These limits with the texture size `supported` grants.
    #[must_use]
    pub fn using_resolution(self, supported: Self) -> Self {
        Self {
            max_texture_dimension_2d: supported.max_texture_dimension_2d,
            ..self
        }
    }

    /// These limits with the alignments `supported` grants.
    #[must_use]
    pub fn using_alignment(self, supported: Self) -> Self {
        Self {
            min_uniform_buffer_offset_alignment: supported.min_uniform_buffer_offset_alignment,
            ..self
        }
    }

    /// Every limit here is no stricter than `supported`'s.
    pub fn check_limits(&self, supported: &Self) -> bool {
        let mut ok = true;
        self.check_limits_with_fail_fn(supported, false, |_, _, _| ok = false);
        ok
    }

    /// Calls `fail(name, requested, supported)` for each limit stricter than
    /// `supported`'s; stops at the first when `fatal`.
    pub fn check_limits_with_fail_fn(
        &self,
        supported: &Self,
        fatal: bool,
        mut fail: impl FnMut(&'static str, u64, u64),
    ) {
        let at_most = [
            (
                "max_texture_dimension_2d",
                self.max_texture_dimension_2d,
                supported.max_texture_dimension_2d,
            ),
            (
                "max_texture_array_layers",
                self.max_texture_array_layers,
                supported.max_texture_array_layers,
            ),
            (
                "max_bind_groups",
                self.max_bind_groups,
                supported.max_bind_groups,
            ),
            (
                "max_vertex_attributes",
                self.max_vertex_attributes,
                supported.max_vertex_attributes,
            ),
            (
                "max_vertex_buffer_array_stride",
                self.max_vertex_buffer_array_stride,
                supported.max_vertex_buffer_array_stride,
            ),
            (
                "max_inter_stage_shader_variables",
                self.max_inter_stage_shader_variables,
                supported.max_inter_stage_shader_variables,
            ),
        ]
        .map(|(n, r, s)| (n, u64::from(r), u64::from(s), r > s));
        let rest = [
            (
                "max_buffer_size",
                self.max_buffer_size,
                supported.max_buffer_size,
                self.max_buffer_size > supported.max_buffer_size,
            ),
            (
                "max_uniform_buffer_binding_size",
                self.max_uniform_buffer_binding_size,
                supported.max_uniform_buffer_binding_size,
                self.max_uniform_buffer_binding_size > supported.max_uniform_buffer_binding_size,
            ),
            (
                "min_uniform_buffer_offset_alignment",
                u64::from(self.min_uniform_buffer_offset_alignment),
                u64::from(supported.min_uniform_buffer_offset_alignment),
                self.min_uniform_buffer_offset_alignment
                    < supported.min_uniform_buffer_offset_alignment,
            ),
        ];
        for (name, requested, granted, _) in at_most.into_iter().chain(rest).filter(|l| l.3) {
            fail(name, requested, granted);
            if fatal {
                return;
            }
        }
    }
}

/// What a format allows on an adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureFormatFeatures {
    pub allowed_usages: TextureUsages,
}

bitflags! {
    /// Optional device features MUI asks for.
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub struct Features: u32 {
        const TIMESTAMP_QUERY = 1 << 0;
        const TIMESTAMP_QUERY_INSIDE_ENCODERS = 1 << 1;
        const PIPELINE_CACHE = 1 << 2;
    }
}

#[derive(Clone, Debug, Default)]
pub struct DeviceDescriptor<'a> {
    pub label: Option<&'a str>,
    pub required_features: Features,
    pub required_limits: Limits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureFormat {
    R8Unorm,
    Rgba8Unorm,
    Rgba8UnormSrgb,
    Bgra8Unorm,
    Bgra8UnormSrgb,
    Rgba16Float,
    Rgba32Float,
}

impl TextureFormat {
    /// Bytes per texel.
    pub const fn block_size(self) -> u32 {
        match self {
            Self::R8Unorm => 1,
            Self::Rgba8Unorm | Self::Rgba8UnormSrgb | Self::Bgra8Unorm | Self::Bgra8UnormSrgb => 4,
            Self::Rgba16Float => 8,
            Self::Rgba32Float => 16,
        }
    }

    pub const fn is_srgb(self) -> bool {
        matches!(self, Self::Rgba8UnormSrgb | Self::Bgra8UnormSrgb)
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub struct TextureUsages: u32 {
        const COPY_SRC = 1 << 0;
        const COPY_DST = 1 << 1;
        const TEXTURE_BINDING = 1 << 2;
        const RENDER_ATTACHMENT = 1 << 4;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub struct BufferUsages: u32 {
        const MAP_READ = 1 << 0;
        const COPY_SRC = 1 << 2;
        const COPY_DST = 1 << 3;
        const VERTEX = 1 << 5;
        const UNIFORM = 1 << 6;
        const QUERY_RESOLVE = 1 << 9;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub struct ShaderStages: u32 {
        const VERTEX = 1 << 0;
        const FRAGMENT = 1 << 1;
        const VERTEX_FRAGMENT = Self::VERTEX.bits() | Self::FRAGMENT.bits();
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct ColorWrites: u32 {
        const RED = 1 << 0;
        const GREEN = 1 << 1;
        const BLUE = 1 << 2;
        const ALPHA = 1 << 3;
        const ALL = 0xF;
    }
}

impl Default for ColorWrites {
    fn default() -> Self {
        Self::ALL
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Extent3d {
    pub width: u32,
    pub height: u32,
    pub depth_or_array_layers: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Origin3d {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}

impl Origin3d {
    pub const ZERO: Self = Self { x: 0, y: 0, z: 0 };
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextureDimension {
    #[default]
    D2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureViewDimension {
    D2,
    D2Array,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextureAspect {
    #[default]
    All,
}

#[derive(Clone, Debug)]
pub struct TextureDescriptor<'a> {
    pub label: Option<&'a str>,
    pub size: Extent3d,
    pub mip_level_count: u32,
    pub sample_count: u32,
    pub dimension: TextureDimension,
    pub format: TextureFormat,
    pub usage: TextureUsages,
    pub view_formats: &'a [TextureFormat],
}

#[derive(Clone, Debug, Default)]
pub struct TextureViewDescriptor<'a> {
    pub label: Option<&'a str>,
    /// `None`: `D2Array` for a texture with more than one layer, else `D2`.
    pub dimension: Option<TextureViewDimension>,
}

#[derive(Clone, Debug)]
pub struct BufferDescriptor<'a> {
    pub label: Option<&'a str>,
    pub size: u64,
    pub usage: BufferUsages,
    pub mapped_at_creation: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FilterMode {
    #[default]
    Nearest,
    Linear,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AddressMode {
    #[default]
    ClampToEdge,
}

#[derive(Clone, Debug, Default)]
pub struct SamplerDescriptor<'a> {
    pub label: Option<&'a str>,
    pub address_mode_u: AddressMode,
    pub address_mode_v: AddressMode,
    pub mag_filter: FilterMode,
    pub min_filter: FilterMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureSampleType {
    Float { filterable: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BufferBindingType {
    Uniform,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SamplerBindingType {
    Filtering,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BindingType {
    Buffer {
        ty: BufferBindingType,
        has_dynamic_offset: bool,
        min_binding_size: Option<std::num::NonZeroU64>,
    },
    Texture {
        sample_type: TextureSampleType,
        view_dimension: TextureViewDimension,
        multisampled: bool,
    },
    Sampler(SamplerBindingType),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BindGroupLayoutEntry {
    pub binding: u32,
    pub visibility: ShaderStages,
    pub ty: BindingType,
    pub count: Option<std::num::NonZeroU32>,
}

#[derive(Clone, Debug)]
pub struct BindGroupLayoutDescriptor<'a> {
    pub label: Option<&'a str>,
    pub entries: &'a [BindGroupLayoutEntry],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VertexFormat {
    Float32x2,
    Float32x4,
}

impl VertexFormat {
    pub const fn size(self) -> u64 {
        match self {
            Self::Float32x2 => 8,
            Self::Float32x4 => 16,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VertexAttribute {
    pub format: VertexFormat,
    pub offset: u64,
    pub shader_location: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VertexStepMode {
    #[default]
    Vertex,
    Instance,
}

#[derive(Clone, Debug)]
pub struct VertexBufferLayout<'a> {
    pub array_stride: u64,
    pub step_mode: VertexStepMode,
    pub attributes: &'a [VertexAttribute],
}

/// `[VertexAttribute; N]` at consecutive offsets, as wgpu's macro builds it.
#[macro_export]
macro_rules! vertex_attr_array {
    ($($location:expr => $format:ident),* $(,)?) => {
        $crate::__attrs([$(($location, $crate::VertexFormat::$format)),*])
    };
}

#[doc(hidden)]
pub const fn __attrs<const N: usize>(list: [(u32, VertexFormat); N]) -> [VertexAttribute; N] {
    let mut out = [VertexAttribute {
        format: VertexFormat::Float32x4,
        offset: 0,
        shader_location: 0,
    }; N];
    let mut offset = 0;
    let mut i = 0;
    while i < N {
        out[i] = VertexAttribute {
            format: list[i].1,
            offset,
            shader_location: list[i].0,
        };
        offset += list[i].1.size();
        i += 1;
    }
    out
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PrimitiveTopology {
    #[default]
    TriangleList,
    TriangleStrip,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PrimitiveState {
    pub topology: PrimitiveTopology,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MultisampleState;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlendFactor {
    Zero,
    One,
    SrcAlpha,
    OneMinusSrcAlpha,
    DstAlpha,
    OneMinusDstAlpha,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlendOperation {
    Add,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlendComponent {
    pub src_factor: BlendFactor,
    pub dst_factor: BlendFactor,
    pub operation: BlendOperation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlendState {
    pub color: BlendComponent,
    pub alpha: BlendComponent,
}

impl BlendState {
    /// Premultiplied source-over.
    pub const PREMULTIPLIED_ALPHA_BLENDING: Self = {
        let c = BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::OneMinusSrcAlpha,
            operation: BlendOperation::Add,
        };
        Self { color: c, alpha: c }
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ColorTargetState {
    pub format: TextureFormat,
    pub blend: Option<BlendState>,
    pub write_mask: ColorWrites,
}

impl From<TextureFormat> for ColorTargetState {
    fn from(format: TextureFormat) -> Self {
        Self {
            format,
            blend: None,
            write_mask: ColorWrites::ALL,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Color {
    pub const TRANSPARENT: Self = Self {
        r: 0.,
        g: 0.,
        b: 0.,
        a: 0.,
    };
    pub const BLACK: Self = Self {
        r: 0.,
        g: 0.,
        b: 0.,
        a: 1.,
    };
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LoadOp<V> {
    Clear(V),
    Load,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StoreOp {
    #[default]
    Store,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Operations<V> {
    pub load: LoadOp<V>,
    pub store: StoreOp,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TexelCopyBufferLayout {
    pub offset: u64,
    pub bytes_per_row: Option<u32>,
    pub rows_per_image: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MapMode {
    Read,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PollType {
    Poll,
    Wait,
}

impl PollType {
    /// wgpu's name for [`PollType::Wait`].
    pub const fn wait_indefinitely() -> Self {
        Self::Wait
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PowerPreference {
    #[default]
    None,
    LowPower,
    HighPerformance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QueryType {
    Timestamp,
}

#[derive(Clone, Debug)]
pub struct QuerySetDescriptor<'a> {
    pub label: Option<&'a str>,
    pub ty: QueryType,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PresentMode {
    AutoVsync,
    AutoNoVsync,
    Fifo,
    FifoRelaxed,
    Immediate,
    Mailbox,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompositeAlphaMode {
    Auto,
    Opaque,
    PreMultiplied,
    PostMultiplied,
    Inherit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceCapabilities {
    pub formats: Vec<TextureFormat>,
    pub present_modes: Vec<PresentMode>,
    pub alpha_modes: Vec<CompositeAlphaMode>,
    pub usages: TextureUsages,
}

/// As wgpu's: nothing offered, but a surface is always renderable.
impl Default for SurfaceCapabilities {
    fn default() -> Self {
        Self {
            formats: Vec::new(),
            present_modes: Vec::new(),
            alpha_modes: Vec::new(),
            usages: TextureUsages::RENDER_ATTACHMENT,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceConfiguration {
    pub usage: TextureUsages,
    pub format: TextureFormat,
    pub width: u32,
    pub height: u32,
    pub present_mode: PresentMode,
    pub desired_maximum_frame_latency: u32,
    pub alpha_mode: CompositeAlphaMode,
    pub view_formats: Vec<TextureFormat>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorFilter {
    Validation,
    OutOfMemory,
    Internal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviceLostReason {
    Unknown,
    Destroyed,
}
