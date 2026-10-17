// Copyright 2026 the Vello Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! MUI downstream safety policy for the locked Vello/wgpu shader set.
use std::borrow::Cow;

/// Device limits for the compute pipeline with area antialiasing, as used by MUI.
/// Derive workgroup and binding requirements from the shipped shader metadata;
/// wgpu defaults alone need not cover a new shader release. Buffer capacity is
/// scene-dependent, so retain the default storage/buffer baseline and let the
/// host request additional adapter-supported headroom.
pub fn required_limits() -> wgpu::Limits {
    let shaders = vello_shaders::SHADERS;
    let mut limits = wgpu::Limits::default();
    for shader in [
        shaders.pathtag_reduce,
        shaders.pathtag_reduce2,
        shaders.pathtag_scan1,
        shaders.pathtag_scan_small,
        shaders.pathtag_scan_large,
        shaders.bbox_clear,
        shaders.flatten,
        shaders.draw_reduce,
        shaders.draw_leaf,
        shaders.clip_reduce,
        shaders.clip_leaf,
        shaders.binning,
        shaders.tile_alloc,
        shaders.backdrop_dyn,
        shaders.path_count_setup,
        shaders.path_count,
        shaders.coarse,
        shaders.path_tiling_setup,
        shaders.path_tiling,
        shaders.fine_area,
    ] {
        let storage = shader
            .bindings
            .iter()
            .filter(|binding| {
                matches!(
                    binding,
                    vello_shaders::BindType::Buffer | vello_shaders::BindType::BufReadOnly
                )
            })
            .count() as u32;
        limits.max_storage_buffers_per_shader_stage =
            limits.max_storage_buffers_per_shader_stage.max(storage);
        limits.max_bindings_per_bind_group = limits.max_bindings_per_bind_group.max(
            shader
                .wgsl
                .binding_indices
                .iter()
                .map(|index| u32::from(*index) + 1)
                .max()
                .unwrap_or(0),
        );
        // WebGPU accounts each workgroup variable rounded up to 16 bytes.
        let storage_bytes = shader
            .workgroup_buffers
            .iter()
            .map(|buffer| buffer.size_in_bytes.next_multiple_of(16))
            .sum();
        limits.max_compute_workgroup_storage_size =
            limits.max_compute_workgroup_storage_size.max(storage_bytes);
        let [x, y, z] = shader.workgroup_size;
        limits.max_compute_workgroup_size_x = limits.max_compute_workgroup_size_x.max(x);
        limits.max_compute_workgroup_size_y = limits.max_compute_workgroup_size_y.max(y);
        limits.max_compute_workgroup_size_z = limits.max_compute_workgroup_size_z.max(z);
        limits.max_compute_invocations_per_workgroup =
            limits.max_compute_invocations_per_workgroup.max(x * y * z);
    }
    limits
}

/// Serialize DX12 pipeline creation across Vello and MUI on this plugin image.
/// wgpu 30.0.1's FXC shader cache shares non-atomic ID3DBlob reference counts
/// across compilation threads (gfx-rs/wgpu#10547). The same gate must surround
/// render and compute pipeline creation; locking only Vello workers is too narrow.
/// No guard crosses an await, GPU submission or audio callback.
pub fn with_pipeline_creation<T>(device: &wgpu::Device, create: impl FnOnce() -> T) -> T {
    static DX12_COMPILATION: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = (device.adapter_info().backend == wgpu::Backend::Dx12).then(|| {
        DX12_COMPILATION
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    });
    create()
}

pub(crate) fn prepare_shader<'a>(
    backend: wgpu::Backend,
    label: &str,
    source: Cow<'a, str>,
) -> Cow<'a, str> {
    if label != "vello.tile_alloc" {
        return source;
    }
    // storageBarrier orders storage, not sh_tile_count's workgroup memory.
    // Retain the Metal workaround but also order the final prefix-sum writes.
    assert_eq!(
        source.matches("storageBarrier();").count(),
        2,
        "tile_alloc barriers changed"
    );
    let source = source.replace(
        "storageBarrier();",
        "workgroupBarrier();\n    storageBarrier();",
    );
    if backend != wgpu::Backend::Dx12 {
        return Cow::Owned(source);
    }
    // Vello#1991: optimized FXC can hang with the storage-buffer broadcast.
    // Use its already-declared workgroup scalar on DX12 (FXC and DXC), leaving
    // Metal's historic compiler workaround intact. A dependency change must
    // update this patch deliberately, never silently leave a half-patched shader.
    let write = "paths[drawobj_ix].tiles = offset;";
    let read = "paths[drawobj_ix | (WG_SIZE - 1u)].tiles";
    assert_eq!(source.matches(write).count(), 1, "tile_alloc write changed");
    assert_eq!(
        source.matches(read).count(),
        1,
        "tile_alloc broadcast changed"
    );
    Cow::Owned(
        source
            .replace(write, "sh_tile_offset = offset;")
            .replace(read, "sh_tile_offset"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_allocation_orders_workgroup_writes_and_uses_dx12_shared_broadcast() {
        let source = vello_shaders::SHADERS.tile_alloc.wgsl.code;
        let dx12 = prepare_shader(wgpu::Backend::Dx12, "vello.tile_alloc", source.clone());
        assert!(dx12.contains("sh_tile_offset = offset;"));
        assert!(!dx12.contains("paths[drawobj_ix | (WG_SIZE - 1u)].tiles"));
        assert!(dx12.contains("workgroupBarrier();\n    storageBarrier();"));
        let metal = prepare_shader(wgpu::Backend::Metal, "vello.tile_alloc", source.clone());
        assert!(metal.contains("paths[drawobj_ix | (WG_SIZE - 1u)].tiles"));
        assert!(metal.contains("workgroupBarrier();\n    storageBarrier();"));
        assert_eq!(
            prepare_shader(wgpu::Backend::Dx12, "other", source.clone()),
            source
        );
    }

    #[test]
    fn area_limits_cover_coarse_bindings_and_largest_workgroups() {
        let limits = required_limits();
        let coarse = vello_shaders::SHADERS.coarse;
        assert!(
            limits.max_storage_buffers_per_shader_stage
                >= coarse
                    .bindings
                    .iter()
                    .filter(|binding| matches!(
                        binding,
                        vello_shaders::BindType::Buffer | vello_shaders::BindType::BufReadOnly
                    ))
                    .count() as u32
        );
        assert!(
            limits.max_compute_invocations_per_workgroup
                >= coarse.workgroup_size.iter().product::<u32>()
        );
        assert!(
            limits.max_compute_workgroup_storage_size
                >= coarse
                    .workgroup_buffers
                    .iter()
                    .map(|buffer| buffer.size_in_bytes.next_multiple_of(16))
                    .sum::<u32>()
        );
    }
}
