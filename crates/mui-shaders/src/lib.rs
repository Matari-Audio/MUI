//! MUI's renderer shaders, lowered from WGSL at build time (see `build.rs`)
//! to the three native targets `mui-hal` binds: SPIR-V for Vulkan 1.1, MSL 2.3
//! for Metal and HLSL Shader Model 5.0 for D3D11. Nothing here parses or
//! translates at runtime; every artifact is a `'static` slice.
//!
//! # Binding ABI
//!
//! - Vulkan: descriptor set = WGSL `@group`, binding = WGSL `@binding`.
//! - Metal and D3D11: per shader, resources in (group, binding) order take
//!   consecutive indices per class (buffers, textures, samplers, each from 0).
//!   [`BindingInfo::msl_slot`] is the `[[buffer(n)]]`/`[[texture(n)]]`/
//!   `[[sampler(n)]]` index and [`BindingInfo::hlsl_register`] the `b#`/`t#`/
//!   `s#` register (space 0); the two are always equal.
//! - Clip space: the SPIR-V flips Y itself, so all three backends take WGSL's
//!   clip space with an ordinary positive-height viewport.
//! - Entry point names can be renamed by a backend to dodge reserved words:
//!   use [`EntryPointInfo::msl_name`] / [`EntryPointInfo::hlsl_name`] there.
#![no_std]

/// SPIR-V version the modules target, (major, minor).
pub const SPIRV_VERSION: (u8, u8) = (1, 3);
/// Metal Shading Language version, (major, minor).
pub const MSL_VERSION: (u8, u8) = (2, 3);
/// D3D shader model; compile entry points as `vs_5_0` / `ps_5_0`.
pub const HLSL_SHADER_MODEL: (u8, u8) = (5, 0);

/// WGSL source for the quad shader.
pub const QUAD_WGSL: &str = include_str!("../wgsl/quad.wgsl");
/// WGSL source for the blur shader.
pub const BLUR_WGSL: &str = include_str!("../wgsl/blur.wgsl");
/// WGSL source for the weld shader.
pub const WELD_WGSL: &str = include_str!("../wgsl/weld.wgsl");
/// WGSL source for the present shader.
pub const PRESENT_WGSL: &str = include_str!("../wgsl/present.wgsl");

/// One of the renderer's WGSL modules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShaderId {
    /// Instanced SDF quads, paths and glyphs.
    Quad,
    /// Downsample and separable blur for backdrops.
    Blur,
    /// Analytic material welding.
    Weld,
    /// Full-screen present to the surface.
    Present,
}

impl ShaderId {
    /// Every shader, in [`artifacts`] order.
    pub const ALL: [ShaderId; 4] = [Self::Quad, Self::Blur, Self::Weld, Self::Present];

    /// This shader's compiled artifact.
    pub fn artifact(self) -> &'static ShaderArtifact {
        &ARTIFACTS[self as usize]
    }
}

/// A pipeline stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    /// Vertex processing.
    Vertex,
    /// Fragment processing.
    Fragment,
}

/// What a binding holds, which also fixes its Metal/D3D11 index class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BindingKind {
    /// `var<uniform>`: Metal `buffer`, D3D11 `b#` constant buffer.
    UniformBuffer,
    /// `texture_2d<f32>`: Metal `texture`, D3D11 `t#`.
    Texture2d,
    /// `texture_2d_array<f32>`: Metal `texture`, D3D11 `t#`.
    Texture2dArray,
    /// Filtering `sampler`: Metal `sampler`, D3D11 `s#`.
    Sampler,
}

impl BindingKind {
    /// The D3D11 register class letter: `b`, `t` or `s`.
    pub const fn hlsl_register_class(self) -> char {
        match self {
            Self::UniformBuffer => 'b',
            Self::Texture2d | Self::Texture2dArray => 't',
            Self::Sampler => 's',
        }
    }
}

/// An entry point and its name in each backend's source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryPointInfo {
    /// The WGSL (and SPIR-V `OpEntryPoint`) name.
    pub name: &'static str,
    pub stage: Stage,
    /// The function name in [`ShaderArtifact::msl`].
    pub msl_name: &'static str,
    /// The function name in [`ShaderArtifact::hlsl`], for `D3DCompile`.
    pub hlsl_name: &'static str,
    /// SM 5.0 DXBC (`vs_5_0` / `ps_5_0`) compiled by FXC. Built on Windows
    /// for Windows; `None` elsewhere, where `mui-hal` compiles
    /// [`ShaderArtifact::hlsl`] at runtime.
    pub dxbc: Option<&'static [u8]>,
}

/// Member of a shader uniform structure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UniformMember {
    /// Beginning of the member relative to its containing structure.
    pub offset: u32,
    /// Distance between consecutive elements of an array member, in bytes.
    pub array_stride: Option<u32>,
}

/// Layout of a shader uniform structure in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UniformLayout {
    /// Total size, including padding.
    pub size: u32,
    /// Top-level members in shader declaration order.
    pub members: &'static [UniformMember],
}

/// One resource binding, with its slot on every backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindingInfo {
    /// WGSL `@group`, the Vulkan descriptor set.
    pub group: u32,
    /// WGSL `@binding`, the Vulkan binding.
    pub binding: u32,
    /// The WGSL variable name.
    pub name: &'static str,
    pub kind: BindingKind,
    /// Metal argument index within [`BindingKind`]'s class.
    pub msl_slot: u32,
    /// D3D11 register number within [`BindingKind::hlsl_register_class`], space 0.
    pub hlsl_register: u32,
    /// Byte layout for uniform buffers.
    pub uniform_layout: Option<UniformLayout>,
}

/// One shader lowered to every target, with its reflection.
#[derive(Clone, Copy, Debug)]
pub struct ShaderArtifact {
    /// Stable module identifier.
    pub id: ShaderId,
    /// File stem of the shader source.
    pub name: &'static str,
    /// SPIR-V words, starting with the magic `0x07230203`.
    pub spirv: &'static [u32],
    /// MSL source holding every entry point.
    pub msl: &'static str,
    /// HLSL SM 5.0 source holding every entry point.
    pub hlsl: &'static str,
    /// Entry points in WGSL declaration order.
    pub entry_points: &'static [EntryPointInfo],
    /// Bindings sorted by (group, binding).
    pub bindings: &'static [BindingInfo],
}

impl ShaderArtifact {
    /// The entry point named `name` in WGSL.
    pub fn entry_point(&self, name: &str) -> Option<&'static EntryPointInfo> {
        self.entry_points.iter().find(|e| e.name == name)
    }

    /// The binding whose WGSL variable is `name`.
    pub fn binding(&self, name: &str) -> Option<&'static BindingInfo> {
        self.bindings.iter().find(|b| b.name == name)
    }
}

/// Every shader, indexed by `ShaderId as usize`.
pub fn artifacts() -> &'static [ShaderArtifact] {
    &ARTIFACTS
}

include!(concat!(env!("OUT_DIR"), "/shaders.rs"));

#[cfg(test)]
mod tests;
