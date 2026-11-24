//! Portable shader-binding, vertex-slot, constant-buffer alignment, and
//! backend-selection tables shared by the Metal and D3D11 backends.
//!
//! Compiled and unit-tested on every host (including Linux x86_64) so the
//! flat-register / slot ABI between `mui-shaders`, `metal`, and `d3d11` stays
//! mechanically verified even on cross-compilation hosts.

#![allow(
    dead_code,
    reason = "portable helpers shared across cfg-gated Metal and D3D11 backends and unit-tested on all hosts"
)]

use mui_shaders::{BindingInfo, BindingKind, EntryPointInfo, ShaderId, Stage};

use crate::types::{
    Backends, BindGroupLayoutEntry, BindingType, CompositeAlphaMode, PresentMode, ShaderStages,
    TextureFormat,
};

/// Metal vertex-buffer index offset (`[[buffer(16 + slot)]]`).
///
/// Uniform buffers from `mui-shaders` take consecutive indices starting at
/// `[[buffer(0)]]`. Placing vertex buffers at `16..=30` guarantees zero
/// collision with uniform buffers while staying within Metal's 31-slot vertex
/// buffer table (`0..=30`).
pub const METAL_VERTEX_BUFFER_BASE: u32 = 16;

/// Maximum Metal vertex-buffer slots available above [`METAL_VERTEX_BUFFER_BASE`].
pub const METAL_MAX_VERTEX_SLOTS: u32 = 15;

/// D3D11.1 `VSSetConstantBuffers1` / `PSSetConstantBuffers1` offset & size
/// granularity: 16 shader constants = 256 bytes.
pub const D3D11_CB_OFFSET_ALIGN_BYTES: u64 = 256;

/// Maximum constant-buffer slice in 16-byte shader constants (`64 KiB`).
pub const D3D11_MAX_CB_CONSTANTS: u32 = 4096;

/// HLSL semantic name emitted by naga for WGSL `@location(N)` vertex inputs.
pub const HLSL_VERTEX_SEMANTIC: &str = "LOC";

/// One resolved resource slot for a `(group, binding)` pair inside a pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedSlot {
    pub group: u32,
    pub binding: u32,
    pub kind: BindingKind,
    /// Flat per-class slot index: Metal `[[buffer/texture/sampler(slot)]]` and
    /// D3D11 `b#/t#/s#` register (guaranteed equal by `mui-shaders`).
    pub slot: u32,
    pub visibility: ShaderStages,
}

/// Complete `(group, binding) -> flat slot` mapping for a render pipeline.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShaderBindingTable {
    entries: Vec<ResolvedSlot>,
}

impl ShaderBindingTable {
    /// Builds a binding table by intersecting the pipeline layout's bind group
    /// layouts with the reflected bindings of `vertex_shader` and
    /// `fragment_shader`.
    pub fn from_shaders(
        vertex_shader: ShaderId,
        fragment_shader: ShaderId,
        groups: &[&[BindGroupLayoutEntry]],
    ) -> Result<Self, String> {
        let mut entries = Vec::new();
        let vs_bindings = vertex_shader.artifact().bindings;
        let fs_bindings = fragment_shader.artifact().bindings;

        for (group_idx, layout) in groups.iter().enumerate() {
            let group = u32::try_from(group_idx)
                .map_err(|_| format!("bind group index {group_idx} overflows u32"))?;
            for entry in *layout {
                let vs_match = find_binding(vs_bindings, group, entry.binding);
                let fs_match = find_binding(fs_bindings, group, entry.binding);
                let reflected = match (vs_match, fs_match) {
                    (Some(v), Some(f)) => {
                        if v.kind != f.kind || v.msl_slot != f.msl_slot {
                            return Err(format!(
                                "group {group} binding {} disagrees between {:?} and {:?}",
                                entry.binding, vertex_shader, fragment_shader
                            ));
                        }
                        v
                    }
                    (Some(b), None) | (None, Some(b)) => b,
                    (None, None) => continue,
                };
                if !kind_matches(&entry.ty, reflected.kind) {
                    return Err(format!(
                        "group {group} binding {} kind mismatch: layout={:?} shader={:?}",
                        entry.binding, entry.ty, reflected.kind
                    ));
                }
                debug_assert_eq!(reflected.msl_slot, reflected.hlsl_register);
                entries.push(ResolvedSlot {
                    group,
                    binding: entry.binding,
                    kind: reflected.kind,
                    slot: reflected.msl_slot,
                    visibility: entry.visibility,
                });
            }
        }
        Ok(Self { entries })
    }

    /// Looks up the flat slot for `(group, binding)`.
    #[must_use]
    pub fn resolve(&self, group: u32, binding: u32) -> Option<ResolvedSlot> {
        self.entries
            .iter()
            .copied()
            .find(|e| e.group == group && e.binding == binding)
    }

    /// All resolved slots in this table.
    #[must_use]
    pub fn slots(&self) -> &[ResolvedSlot] {
        &self.entries
    }
}

fn find_binding(
    bindings: &'static [BindingInfo],
    group: u32,
    binding: u32,
) -> Option<&'static BindingInfo> {
    bindings
        .iter()
        .find(|b| b.group == group && b.binding == binding)
}

fn kind_matches(ty: &BindingType, kind: BindingKind) -> bool {
    matches!(
        (ty, kind),
        (BindingType::Buffer { .. }, BindingKind::UniformBuffer)
            | (
                BindingType::Texture {
                    view_dimension: crate::types::TextureViewDimension::D2,
                    ..
                },
                BindingKind::Texture2d
            )
            | (
                BindingType::Texture {
                    view_dimension: crate::types::TextureViewDimension::D2Array,
                    ..
                },
                BindingKind::Texture2dArray
            )
            | (BindingType::Sampler(_), BindingKind::Sampler)
    )
}

/// Resolves an entry point in `shader` for `stage`, matching `name` or taking
/// the shader's first entry point of that stage when `name` is `None`.
pub fn resolve_entry_point(
    shader: ShaderId,
    name: Option<&str>,
    stage: Stage,
) -> Result<&'static EntryPointInfo, String> {
    let entries = shader.artifact().entry_points;
    match name {
        Some(n) => entries
            .iter()
            .find(|e| e.stage == stage && e.name == n)
            .ok_or_else(|| format!("shader {shader:?} has no {stage:?} entry point named `{n}`")),
        None => entries
            .iter()
            .find(|e| e.stage == stage)
            .ok_or_else(|| format!("shader {shader:?} has no {stage:?} entry point")),
    }
}

/// Maps a HAL vertex buffer slot (`0..15`) to its Metal `[[buffer(N)]]` index.
pub fn metal_vertex_buffer_index(slot: u32) -> Result<u32, String> {
    if slot >= METAL_MAX_VERTEX_SLOTS {
        return Err(format!(
            "vertex buffer slot {slot} exceeds Metal limit {METAL_MAX_VERTEX_SLOTS}"
        ));
    }
    Ok(METAL_VERTEX_BUFFER_BASE + slot)
}

/// Computes `(first_constant, num_constants)` for D3D11.1
/// `VSSetConstantBuffers1` / `PSSetConstantBuffers1` from byte offset & size.
///
/// D3D11.1 requires both values to be multiples of 16 shader constants (256 bytes).
pub fn d3d11_constant_buffer_range(
    offset_bytes: u64,
    size_bytes: u64,
) -> Result<(u32, u32), String> {
    if !offset_bytes.is_multiple_of(D3D11_CB_OFFSET_ALIGN_BYTES) {
        return Err(format!(
            "D3D11 constant buffer offset {offset_bytes} is not 256-byte aligned"
        ));
    }
    let first_constant = u32::try_from(offset_bytes / 16)
        .map_err(|_| format!("D3D11 constant buffer offset {offset_bytes} overflows u32"))?;
    let aligned_size = size_bytes
        .max(D3D11_CB_OFFSET_ALIGN_BYTES)
        .checked_next_multiple_of(D3D11_CB_OFFSET_ALIGN_BYTES)
        .ok_or_else(|| format!("D3D11 constant buffer size {size_bytes} overflows u64"))?;
    let num_constants = u32::try_from(aligned_size / 16)
        .unwrap_or(D3D11_MAX_CB_CONSTANTS)
        .min(D3D11_MAX_CB_CONSTANTS);
    Ok((first_constant, num_constants))
}

/// Rounds a D3D11 constant buffer's allocation size up to a 256-byte multiple
/// (at least 256 bytes) so both `D3D11CreateBuffer` (16-byte multiple) and
/// `VSSetConstantBuffers1` (256-byte constant slice) always succeed.
///
/// Returns `None` if rounding up overflows `u64`.
#[must_use]
pub const fn d3d11_constant_buffer_byte_width(requested: u64) -> Option<u64> {
    let min = if requested == 0 {
        D3D11_CB_OFFSET_ALIGN_BYTES
    } else {
        requested
    };
    let rem = min % D3D11_CB_OFFSET_ALIGN_BYTES;
    if rem == 0 {
        Some(min)
    } else {
        min.checked_add(D3D11_CB_OFFSET_ALIGN_BYTES - rem)
    }
}

/// Returns whether `CAMetalLayer.pixelFormat` supports `format`.
///
/// Apple's `CAMetalLayer` only accepts `BGRA8Unorm`, `BGRA8Unorm_sRGB`, and
/// `RGBA16Float` among `mui-hal`'s texture formats; passing `RGBA8Unorm*`
/// throws `NSInvalidArgumentException`.
#[must_use]
pub const fn metal_surface_format_supported(format: TextureFormat) -> bool {
    matches!(
        format,
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb | TextureFormat::Rgba16Float
    )
}

/// Clamps a scissor rectangle `(x, y, w, h)` to fit within `(target_w, target_h)`
/// so Metal's `setScissorRect` never throws an assertion exception.
#[must_use]
pub const fn clamp_scissor_rect(
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    target_w: u32,
    target_h: u32,
) -> [u32; 4] {
    let cx = if x > target_w { target_w } else { x };
    let cy = if y > target_h { target_h } else { y };
    let max_w = target_w - cx;
    let max_h = target_h - cy;
    let cw = if w > max_w { max_w } else { w };
    let ch = if h > max_h { max_h } else { h };
    [cx, cy, cw, ch]
}

/// Computes the DXGI DirectComposition swapchain `BufferCount` from
/// `desired_maximum_frame_latency` (`latency + 1`, clamped to `2..=3`).
#[must_use]
pub const fn d3d11_buffer_count(desired_latency: u32) -> u32 {
    let n = desired_latency.saturating_add(1);
    if n < 2 {
        2
    } else if n > 3 {
        3
    } else {
        n
    }
}

/// Returns whether a `CAMetalLayer` should enable `displaySyncEnabled` for
/// `present_mode`.
#[must_use]
pub const fn metal_display_sync_enabled(mode: PresentMode) -> bool {
    !matches!(mode, PresentMode::Immediate | PresentMode::AutoNoVsync)
}

/// Clamps `desired_maximum_frame_latency` to `CAMetalLayer`'s valid drawable
/// pool size (`2..=3`, default 3).
#[must_use]
pub const fn metal_drawable_count(desired_latency: u32) -> usize {
    match desired_latency {
        0..=2 => 2,
        _ => 3,
    }
}

/// Returns whether `CAMetalLayer.opaque` should be set for `alpha_mode`.
#[must_use]
pub const fn metal_layer_opaque(alpha: CompositeAlphaMode) -> bool {
    matches!(alpha, CompositeAlphaMode::Opaque | CompositeAlphaMode::Auto)
}

/// Returns whether a DirectComposition swapchain should request
/// `DXGI_ALPHA_MODE_PREMULTIPLIED` vs `DXGI_ALPHA_MODE_IGNORE`.
#[must_use]
pub const fn d3d11_surface_translucent(alpha: CompositeAlphaMode) -> bool {
    matches!(
        alpha,
        CompositeAlphaMode::PreMultiplied | CompositeAlphaMode::PostMultiplied
    )
}

/// Evaluates whether `InstanceDescriptor::from_env` should enable native
/// backends given `(env_backend, requested_backends, has_wgpu, wgpu_only, default_native_os)`.
///
/// - `MUI_HAL_BACKEND=wgpu` forces `wgpu` whenever `has_wgpu`.
/// - `MUI_HAL_BACKEND=native|vulkan|metal|d3d11` opts into native (unless `wgpu_only`).
/// - When `MUI_HAL_BACKEND` is unset:
///   - Without `wgpu`, native is always enabled.
///   - With `wgpu_only`, native is disabled.
///   - On Linux (`default_native_os == true`), native Vulkan is default.
///   - On macOS/Windows (`default_native_os == false`) with `wgpu`, `wgpu`
///     remains default until OS-CI parity promotes the native backend, except
///     when `WGPU_BACKEND` explicitly requests `d3d11` (which `wgpu` cannot serve).
#[must_use]
pub fn resolve_native_selection(
    env_backend: Option<&str>,
    requested_backends: Backends,
    has_wgpu: bool,
    wgpu_only: bool,
    default_native_os: bool,
) -> bool {
    if !has_wgpu {
        return true;
    }
    if wgpu_only {
        return false;
    }
    if let Some(raw) = env_backend {
        let lower = raw.trim().to_ascii_lowercase();
        if lower == "wgpu" {
            return false;
        }
        if matches!(
            lower.as_str(),
            "native" | "vulkan" | "vk" | "metal" | "mtl" | "d3d11"
        ) {
            return true;
        }
    }
    if default_native_os {
        return true;
    }
    // `WGPU_BACKEND=d3d11` can only be served by native D3D11 (wgpu has no D3D11 backend).
    requested_backends == Backends::D3D11
}

/// Computes the ascending `@binding` indices of dynamic-offset entries inside a
/// `BindGroupLayout`, matching WebGPU's `set_bind_group` dynamic-offset order.
#[must_use]
pub fn dynamic_offset_bindings(entries: &[BindGroupLayoutEntry]) -> Vec<u32> {
    let mut dynamic: Vec<u32> = entries
        .iter()
        .filter_map(|e| match e.ty {
            BindingType::Buffer {
                has_dynamic_offset: true,
                ..
            } => Some(e.binding),
            _ => None,
        })
        .collect();
    dynamic.sort_unstable();
    dynamic
}

/// Coarse resource kind of a concrete bind-group entry for layout validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundKind {
    Buffer,
    Texture,
    Sampler,
}

/// Validates that `entries` provides a complete, duplicate-free, type-matching
/// binding for every entry declared in `layout_entries`.
pub fn validate_bind_group_entries(
    layout_entries: &[BindGroupLayoutEntry],
    entries: &[(u32, BoundKind)],
) -> Result<(), String> {
    // Reject duplicate binding indices in the bind group.
    for (i, (b, _)) in entries.iter().enumerate() {
        if entries[..i].iter().any(|(prev, _)| prev == b) {
            return Err(format!("duplicate bind group entry for binding {b}"));
        }
    }
    // Reject unexpected bindings not present in the layout.
    for (b, _) in entries {
        if !layout_entries.iter().any(|le| le.binding == *b) {
            return Err(format!("bind group entry for undeclared binding {b}"));
        }
    }
    // Verify every layout entry is present with a matching resource kind.
    for le in layout_entries {
        let Some((_, kind)) = entries.iter().find(|(b, _)| *b == le.binding) else {
            return Err(format!(
                "bind group is missing required binding {}",
                le.binding
            ));
        };
        let expected = match le.ty {
            BindingType::Buffer { .. } => BoundKind::Buffer,
            BindingType::Texture { .. } => BoundKind::Texture,
            BindingType::Sampler(_) => BoundKind::Sampler,
        };
        if *kind != expected {
            return Err(format!(
                "bind group binding {} expects {expected:?}, got {kind:?}",
                le.binding
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        BufferBindingType, SamplerBindingType, TextureSampleType, TextureViewDimension,
    };

    fn entry_for(b: &BindingInfo) -> BindGroupLayoutEntry {
        let ty = match b.kind {
            BindingKind::UniformBuffer => BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: b.group == 0 && b.binding == 0,
                min_binding_size: None,
            },
            BindingKind::Texture2d => BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: true },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
            BindingKind::Texture2dArray => BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: true },
                view_dimension: TextureViewDimension::D2Array,
                multisampled: false,
            },
            BindingKind::Sampler => BindingType::Sampler(SamplerBindingType::Filtering),
        };
        BindGroupLayoutEntry {
            binding: b.binding,
            visibility: ShaderStages::VERTEX_FRAGMENT,
            ty,
            count: None,
        }
    }

    #[test]
    fn every_shader_binding_table_resolves_without_slot_collision() {
        for id in ShaderId::ALL {
            let art = id.artifact();
            let max_group = art.bindings.iter().map(|b| b.group).max().unwrap_or(0);
            let mut groups: Vec<Vec<BindGroupLayoutEntry>> =
                vec![Vec::new(); max_group as usize + 1];
            for b in art.bindings {
                groups[b.group as usize].push(entry_for(b));
            }
            let refs: Vec<&[BindGroupLayoutEntry]> = groups.iter().map(Vec::as_slice).collect();
            let table = ShaderBindingTable::from_shaders(id, id, &refs)
                .unwrap_or_else(|e| panic!("{}: {e}", art.name));
            assert_eq!(table.slots().len(), art.bindings.len(), "{}", art.name);

            for b in art.bindings {
                let resolved = table.resolve(b.group, b.binding).unwrap_or_else(|| {
                    panic!("{}: missing ({}, {})", art.name, b.group, b.binding)
                });
                assert_eq!(resolved.slot, b.msl_slot);
                assert_eq!(resolved.slot, b.hlsl_register);
                assert_eq!(resolved.kind, b.kind);
                if b.kind == BindingKind::UniformBuffer {
                    assert!(
                        resolved.slot < METAL_VERTEX_BUFFER_BASE,
                        "{}: uniform slot {} collides with vertex buffer base {}",
                        art.name,
                        resolved.slot,
                        METAL_VERTEX_BUFFER_BASE
                    );
                }
            }
        }
    }

    #[test]
    fn every_shader_entry_point_resolves_msl_and_hlsl_names() {
        for id in ShaderId::ALL {
            let art = id.artifact();
            let vs = resolve_entry_point(id, None, Stage::Vertex).unwrap();
            let fs = resolve_entry_point(id, None, Stage::Fragment).unwrap();
            assert!(!vs.msl_name.is_empty() && !vs.hlsl_name.is_empty());
            assert!(!fs.msl_name.is_empty() && !fs.hlsl_name.is_empty());
            assert!(
                art.msl.contains(vs.msl_name),
                "{}: missing MSL vs",
                art.name
            );
            assert!(
                art.msl.contains(fs.msl_name),
                "{}: missing MSL fs",
                art.name
            );
            assert!(
                art.hlsl.contains(vs.hlsl_name),
                "{}: missing HLSL vs",
                art.name
            );
            assert!(
                art.hlsl.contains(fs.hlsl_name),
                "{}: missing HLSL fs",
                art.name
            );
        }
        assert!(resolve_entry_point(ShaderId::Quad, Some("missing"), Stage::Vertex).is_err());
    }

    #[test]
    fn metal_vertex_slots_never_collide_with_uniform_slots() {
        assert_eq!(metal_vertex_buffer_index(0).unwrap(), 16);
        assert_eq!(metal_vertex_buffer_index(14).unwrap(), 30);
        assert!(metal_vertex_buffer_index(15).is_err());
    }

    #[test]
    fn d3d11_constant_buffer_ranges_obey_16_constant_granularity() {
        assert_eq!(d3d11_constant_buffer_range(0, 16).unwrap(), (0, 16));
        assert_eq!(d3d11_constant_buffer_range(256, 256).unwrap(), (16, 16));
        assert_eq!(d3d11_constant_buffer_range(512, 336).unwrap(), (32, 32));
        assert_eq!(
            d3d11_constant_buffer_range(0, 128 * 1024).unwrap(),
            (0, 4096)
        );
        assert!(d3d11_constant_buffer_range(16, 256).is_err());
        assert_eq!(d3d11_constant_buffer_byte_width(0), Some(256));
        assert_eq!(d3d11_constant_buffer_byte_width(16), Some(256));
        assert_eq!(d3d11_constant_buffer_byte_width(256), Some(256));
        assert_eq!(d3d11_constant_buffer_byte_width(336), Some(512));
        assert_eq!(d3d11_constant_buffer_byte_width(u64::MAX), None);
    }

    #[test]
    fn surface_policy_helpers_match_apple_and_microsoft_invariants() {
        assert!(metal_display_sync_enabled(PresentMode::Fifo));
        assert!(metal_display_sync_enabled(PresentMode::Mailbox));
        assert!(!metal_display_sync_enabled(PresentMode::Immediate));
        assert!(!metal_display_sync_enabled(PresentMode::AutoNoVsync));
        assert_eq!(metal_drawable_count(0), 2);
        assert_eq!(metal_drawable_count(2), 2);
        assert_eq!(metal_drawable_count(3), 3);
        assert_eq!(metal_drawable_count(8), 3);
        assert!(metal_layer_opaque(CompositeAlphaMode::Opaque));
        assert!(!metal_layer_opaque(CompositeAlphaMode::PreMultiplied));
        assert!(metal_surface_format_supported(TextureFormat::Bgra8Unorm));
        assert!(metal_surface_format_supported(
            TextureFormat::Bgra8UnormSrgb
        ));
        assert!(metal_surface_format_supported(TextureFormat::Rgba16Float));
        assert!(!metal_surface_format_supported(TextureFormat::Rgba8Unorm));
        assert!(!metal_surface_format_supported(
            TextureFormat::Rgba8UnormSrgb
        ));
        assert!(!d3d11_surface_translucent(CompositeAlphaMode::Opaque));
        assert!(d3d11_surface_translucent(CompositeAlphaMode::PreMultiplied));
        assert_eq!(d3d11_buffer_count(0), 2);
        assert_eq!(d3d11_buffer_count(1), 2);
        assert_eq!(d3d11_buffer_count(2), 3);
        assert_eq!(d3d11_buffer_count(u32::MAX), 3);
        assert_eq!(
            clamp_scissor_rect(10, 20, 50, 60, 100, 100),
            [10, 20, 50, 60]
        );
        assert_eq!(
            clamp_scissor_rect(80, 90, 50, 60, 100, 100),
            [80, 90, 20, 10]
        );
        assert_eq!(
            clamp_scissor_rect(200, 200, 50, 60, 100, 100),
            [100, 100, 0, 0]
        );
    }

    #[test]
    fn backend_selection_respects_pending_os_ci_default_and_explicit_opt_in() {
        // Linux: native Vulkan by default, wgpu on opt-in.
        assert!(resolve_native_selection(
            None,
            Backends::all(),
            true,
            false,
            true
        ));
        assert!(!resolve_native_selection(
            Some("wgpu"),
            Backends::all(),
            true,
            false,
            true
        ));
        assert!(!resolve_native_selection(
            None,
            Backends::all(),
            true,
            true,
            true
        ));

        // macOS / Windows with `wgpu` enabled: wgpu stays default until OS-CI promotes,
        // while MUI_HAL_BACKEND=metal|d3d11|native or WGPU_BACKEND=d3d11 selects native.
        assert!(!resolve_native_selection(
            None,
            Backends::all(),
            true,
            false,
            false
        ));
        assert!(resolve_native_selection(
            Some("metal"),
            Backends::all(),
            true,
            false,
            false
        ));
        assert!(resolve_native_selection(
            Some("d3d11"),
            Backends::all(),
            true,
            false,
            false
        ));
        assert!(resolve_native_selection(
            Some("native"),
            Backends::all(),
            true,
            false,
            false
        ));
        assert!(resolve_native_selection(
            None,
            Backends::D3D11,
            true,
            false,
            false
        ));
        assert!(!resolve_native_selection(
            Some("wgpu"),
            Backends::D3D11,
            true,
            false,
            false
        ));

        // Standalone native build without `wgpu` feature: native is always selected.
        assert!(resolve_native_selection(
            None,
            Backends::all(),
            false,
            false,
            false
        ));
        assert!(resolve_native_selection(
            Some("wgpu"),
            Backends::all(),
            false,
            false,
            false
        ));
    }

    #[test]
    fn dynamic_offset_bindings_are_sorted_by_binding_number() {
        let entries = [
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: None,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: None,
                },
                count: None,
            },
        ];
        assert_eq!(dynamic_offset_bindings(&entries), vec![0, 3]);
    }

    #[test]
    fn bind_group_completeness_rejects_missing_duplicate_extra_and_mismatched() {
        let layout = [
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
        ];

        assert!(
            validate_bind_group_entries(
                &layout,
                &[
                    (0, BoundKind::Buffer),
                    (1, BoundKind::Texture),
                    (2, BoundKind::Sampler),
                ]
            )
            .is_ok()
        );
        // Missing binding 2.
        assert!(
            validate_bind_group_entries(
                &layout,
                &[(0, BoundKind::Buffer), (1, BoundKind::Texture)]
            )
            .is_err()
        );
        // Duplicate binding 0.
        assert!(
            validate_bind_group_entries(
                &layout,
                &[
                    (0, BoundKind::Buffer),
                    (0, BoundKind::Buffer),
                    (1, BoundKind::Texture),
                    (2, BoundKind::Sampler),
                ]
            )
            .is_err()
        );
        // Extra undeclared binding 3.
        assert!(
            validate_bind_group_entries(
                &layout,
                &[
                    (0, BoundKind::Buffer),
                    (1, BoundKind::Texture),
                    (2, BoundKind::Sampler),
                    (3, BoundKind::Buffer),
                ]
            )
            .is_err()
        );
        // Mismatched resource kind on binding 1.
        assert!(
            validate_bind_group_entries(
                &layout,
                &[
                    (0, BoundKind::Buffer),
                    (1, BoundKind::Sampler),
                    (2, BoundKind::Sampler),
                ]
            )
            .is_err()
        );
    }
}
