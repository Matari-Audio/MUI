//! Driver-independent candidate policy. Device calls stay in the host lifecycle.

/// What the renderer needs of an adapter: WebGL2-class limits (no compute,
/// no storage), the adapter's own texture size and buffer alignment, and the
/// texture array layers the glyph atlas grows into. The device itself is
/// requested with the adapter's full limits, for apps that borrow it. The quad pass needs 15
/// vertex attributes and inter-stage variables and a 240-byte instance
/// stride, all inside these; every buffer it binds is a uniform.
pub(super) fn requested_limits(supported: &wgpu::Limits) -> wgpu::Limits {
    wgpu::Limits::downlevel_webgl2_defaults()
        .using_resolution(supported.clone())
        .using_alignment(supported.clone())
}

// Adapter type/backend preference adapted from Zed's Apache-2.0
// crates/gpui_wgpu/src/wgpu_context.rs at
// a84689073d296dfd39987bc7dd478e43ef76d83a (sort_adapters).
// Copyright 2022 - 2025 Zed Industries, Inc.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
// Adaptation: stable high-performance ranking without Zed-specific overrides;
// retain CPU fallback.
pub(super) fn adapter_rank(info: &wgpu::AdapterInfo) -> (u8, u8) {
    adapter_rank_for(info, cfg!(windows))
}

fn adapter_rank_for(info: &wgpu::AdapterInfo, windows: bool) -> (u8, u8) {
    let device = match info.device_type {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::Other => 2,
        wgpu::DeviceType::VirtualGpu => 3,
        wgpu::DeviceType::Cpu => 4,
    };
    let backend = match info.backend {
        wgpu::Backend::Vulkan if windows => 1,
        wgpu::Backend::Vulkan | wgpu::Backend::Metal | wgpu::Backend::Dx12 => 0,
        _ => 2,
    };
    (device, backend)
}

/// No downlevel flag is asked for: the pass runs on anything GLES 3 /
/// WebGL2 class.
pub(super) fn validate_renderer_requirements(
    info: &wgpu::AdapterInfo,
    requested: &wgpu::Limits,
    supported: &wgpu::Limits,
) -> Result<(), String> {
    if super::force_cpu() {
        return Err("CPU rendering forced by environment (MUI_FORCE_CPU / MUI_RENDERER)".into());
    }
    validate_renderer_requirements_with_cpu_adapter(
        info,
        requested,
        supported,
        cfg!(feature = "test-support")
            && (std::env::var("MUI_TEST_ALLOW_CPU_ADAPTER").is_ok_and(|value| value == "1")
                || std::env::var("MUI_TEST_ALLOW_WARP").is_ok_and(|value| value == "1")),
    )
}

pub(super) fn validate_renderer_requirements_with_cpu_adapter(
    info: &wgpu::AdapterInfo,
    requested: &wgpu::Limits,
    supported: &wgpu::Limits,
    allow_cpu_adapter: bool,
) -> Result<(), String> {
    // Reject all software/CPU wgpu adapters (Windows DX12 WARP, Linux Vulkan
    // Lavapipe/SwiftShader, GL llvmpipe, etc.) so hosts fall back directly to
    // our fast dirty-rect `vello_cpu` software path instead of running GPU
    // shaders on a CPU emulator or risking driver JIT faults.
    if !allow_cpu_adapter && info.device_type == wgpu::DeviceType::Cpu {
        if info.backend == wgpu::Backend::Dx12 && info.vendor == 0x1414 {
            return Err("WARP shader compilation can crash; use CPU rendering".into());
        }
        return Err(format!(
            "software/CPU adapter ({:?}, {}) rejected; use CPU rendering",
            info.backend, info.name
        ));
    }
    let mut missing = Vec::new();
    requested.check_limits_with_fail_fn(supported, false, |name, requested, supported| {
        missing.push(format!(
            "{name}: requested {requested}, supported {supported}"
        ));
    });
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("renderer limits: {}", missing.join(", ")))
    }
}

/// The formats the renderer creates, with the usages it creates them with:
/// its frame, image, weld and offscreen textures (`Rgba8Unorm`), the glyph
/// atlas (`R8Unorm`, copied into a larger one as it grows), the path
/// records (`Rgba32Float`, read by `textureLoad`), and the shared session's
/// window format and worker readback bitmap (`Bgra8Unorm`).
pub(super) fn validate_texture_formats(adapter: &wgpu::Adapter) -> Result<(), String> {
    use wgpu::{TextureFormat as F, TextureUsages as U};
    for (format, needed) in [
        (
            F::Rgba8Unorm,
            U::TEXTURE_BINDING | U::RENDER_ATTACHMENT | U::COPY_SRC | U::COPY_DST,
        ),
        (F::R8Unorm, U::TEXTURE_BINDING | U::COPY_SRC | U::COPY_DST),
        (F::Rgba32Float, U::TEXTURE_BINDING | U::COPY_DST),
        (F::Bgra8Unorm, U::RENDER_ATTACHMENT | U::COPY_SRC),
    ] {
        let available = adapter.get_texture_format_features(format);
        if !available.allowed_usages.contains(needed) {
            return Err(format!(
                "{format:?} requires {needed:?}, offers {:?}",
                available.allowed_usages
            ));
        }
    }
    Ok(())
}

pub(super) fn surface_choices(
    caps: &wgpu::SurfaceCapabilities,
    transparency: super::Transparency,
) -> Result<
    (
        wgpu::TextureFormat,
        wgpu::CompositeAlphaMode,
        wgpu::PresentMode,
    ),
    &'static str,
> {
    if caps.alpha_modes.is_empty()
        || caps.present_modes.is_empty()
        || !caps.usages.contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
    {
        return Err("surface has no alpha/present modes or render attachment usage");
    }
    let format = super::surface_format(&caps.formats).ok_or("no non-sRGB UNORM surface format")?;
    Ok((
        format,
        super::alpha_mode(&caps.alpha_modes, transparency),
        super::present_mode(&caps.present_modes),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_adapters_are_rejected_before_device_creation_while_hardware_is_accepted() {
        let limits = wgpu::Limits::default();
        let validate = |info: &wgpu::AdapterInfo| {
            validate_renderer_requirements_with_cpu_adapter(info, &limits, &limits, false)
        };
        // Windows DX12 WARP is rejected by default and allowed only via explicit test opt-in.
        let mut warp = wgpu::AdapterInfo::new(wgpu::DeviceType::Cpu, wgpu::Backend::Dx12);
        warp.name = "Microsoft Basic Render Driver".into();
        warp.vendor = 0x1414;
        warp.device = 0x008c;
        warp.driver = "10.0.26100.33438".into();
        assert!(validate(&warp).unwrap_err().contains("WARP"));
        assert!(
            validate_renderer_requirements_with_cpu_adapter(&warp, &limits, &limits, true).is_ok()
        );

        // Linux Lavapipe (Vulkan), llvmpipe (GL), and any other DeviceType::Cpu adapter
        // are rejected so hosts fall back directly to dirty-rect vello_cpu software rendering.
        for (backend, name) in [
            (wgpu::Backend::Vulkan, "llvmpipe (LLVM 19.1.0, 256 bits)"),
            (wgpu::Backend::Gl, "llvmpipe"),
            (wgpu::Backend::Metal, "Software"),
        ] {
            let mut cpu = wgpu::AdapterInfo::new(wgpu::DeviceType::Cpu, backend);
            cpu.name = name.into();
            let err = validate(&cpu).unwrap_err();
            assert!(err.contains("CPU"), "{err}");
            assert!(
                validate_renderer_requirements_with_cpu_adapter(&cpu, &limits, &limits, true)
                    .is_ok()
            );
        }

        // Discrete, integrated, virtual, and other GPU adapters remain accepted.
        for kind in [
            wgpu::DeviceType::DiscreteGpu,
            wgpu::DeviceType::IntegratedGpu,
            wgpu::DeviceType::VirtualGpu,
            wgpu::DeviceType::Other,
        ] {
            for (backend, vendor) in [
                (wgpu::Backend::Dx12, 0x10de),
                (wgpu::Backend::Vulkan, 0x1002),
                (wgpu::Backend::Vulkan, 0x8086),
                (wgpu::Backend::Metal, 0x106b),
                (wgpu::Backend::Dx12, 0x1414),
            ] {
                let mut hw = wgpu::AdapterInfo::new(kind, backend);
                hw.vendor = vendor;
                assert!(validate(&hw).is_ok(), "{kind:?} {backend:?} {vendor:#x}");
            }
        }
    }

    #[test]
    fn surface_selection_uses_offered_modes_and_rejects_incompatible_caps() {
        use wgpu::{CompositeAlphaMode as A, PresentMode as P, TextureFormat as F};
        let mut caps = wgpu::SurfaceCapabilities {
            formats: vec![F::Bgra8UnormSrgb, F::Rgba8Unorm],
            present_modes: vec![P::Fifo],
            alpha_modes: vec![A::Inherit],
            ..Default::default()
        };
        let select = |caps: &wgpu::SurfaceCapabilities| {
            surface_choices(caps, super::super::Transparency::Translucent)
        };
        assert_eq!(select(&caps), Ok((F::Rgba8Unorm, A::Inherit, P::Fifo)));
        caps.present_modes.clear();
        assert!(select(&caps).is_err());
        caps.present_modes.push(P::Immediate);
        caps.alpha_modes.clear();
        assert!(select(&caps).is_err());
        caps.alpha_modes.push(A::Opaque);
        caps.usages = wgpu::TextureUsages::COPY_SRC;
        assert!(select(&caps).is_err());
        caps.usages = wgpu::TextureUsages::RENDER_ATTACHMENT;
        caps.formats = vec![F::Bgra8UnormSrgb];
        assert!(select(&caps).is_err());
    }

    #[test]
    fn windows_prefers_dx12_for_intel_and_amd_without_hiding_vulkan_fallback() {
        for vendor in [0x8086, 0x1002, 0x10de] {
            for kind in [
                wgpu::DeviceType::IntegratedGpu,
                wgpu::DeviceType::DiscreteGpu,
            ] {
                let mut dx = wgpu::AdapterInfo::new(kind, wgpu::Backend::Dx12);
                dx.vendor = vendor;
                let mut vk = dx.clone();
                vk.backend = wgpu::Backend::Vulkan;
                assert!(adapter_rank_for(&dx, true) < adapter_rank_for(&vk, true));
                assert_eq!(adapter_rank_for(&dx, false), adapter_rank_for(&vk, false));
            }
        }
    }

    #[test]
    fn a_webgl2_class_adapter_without_compute_is_accepted_and_less_is_not() {
        let info = wgpu::AdapterInfo::new(wgpu::DeviceType::IntegratedGpu, wgpu::Backend::Gl);
        let webgl2 = wgpu::Limits::downlevel_webgl2_defaults();
        let requested = requested_limits(&webgl2);
        assert!(
            validate_renderer_requirements_with_cpu_adapter(&info, &requested, &webgl2, false)
                .is_ok()
        );
        assert_eq!(requested.max_storage_buffers_per_shader_stage, 0);
        assert_eq!(requested.max_compute_invocations_per_workgroup, 0);
        for field in [
            "max_vertex_attributes",
            "max_inter_stage_shader_variables",
            "max_vertex_buffer_array_stride",
            "max_uniform_buffer_binding_size",
        ] {
            let mut supported = webgl2.clone();
            match field {
                "max_vertex_attributes" => supported.max_vertex_attributes -= 1,
                "max_inter_stage_shader_variables" => {
                    supported.max_inter_stage_shader_variables -= 1;
                }
                "max_vertex_buffer_array_stride" => supported.max_vertex_buffer_array_stride -= 1,
                "max_uniform_buffer_binding_size" => supported.max_uniform_buffer_binding_size -= 1,
                _ => unreachable!(),
            }
            let error = validate_renderer_requirements_with_cpu_adapter(
                &info,
                &requested_limits(&supported),
                &supported,
                false,
            )
            .unwrap_err();
            assert!(error.contains(field), "{error}");
        }
    }

    /// What the pass's pipelines actually use fits what is requested.
    #[test]
    fn the_requested_limits_cover_the_quad_pipelines() {
        let requested = requested_limits(&wgpu::Limits::default());
        assert!(crate::quad::INSTANCE_SLOTS as u32 <= requested.max_vertex_attributes);
        assert!(crate::quad::INSTANCE_SLOTS as u32 <= requested.max_inter_stage_shader_variables);
        assert!(
            16 * crate::quad::INSTANCE_SLOTS as u32 <= requested.max_vertex_buffer_array_stride
        );
        assert!(
            mui_weld::boundary::BOUNDARY_BYTES as u64 <= requested.max_uniform_buffer_binding_size
        );
        assert_eq!(
            requested.max_texture_dimension_2d,
            wgpu::Limits::default().max_texture_dimension_2d
        );
    }
}
