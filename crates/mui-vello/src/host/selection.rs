//! Driver-independent candidate policy. Device calls stay in the host lifecycle.

pub(super) fn requested_limits(supported: &wgpu::Limits) -> wgpu::Limits {
    let mut requested = vello::required_limits()
        .using_resolution(supported.clone())
        .using_alignment(supported.clone());
    requested.max_storage_buffers_per_shader_stage = supported
        .max_storage_buffers_per_shader_stage
        .max(requested.max_storage_buffers_per_shader_stage);
    requested.max_storage_buffer_binding_size = supported
        .max_storage_buffer_binding_size
        .max(requested.max_storage_buffer_binding_size);
    requested.max_buffer_size = supported.max_buffer_size.max(requested.max_buffer_size);
    requested
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
// retain CPU fallback and MUI's full compute renderer requirements.
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

pub(super) fn validate_renderer_requirements(
    info: &wgpu::AdapterInfo,
    flags: wgpu::DownlevelFlags,
    requested: &wgpu::Limits,
    supported: &wgpu::Limits,
) -> Result<(), String> {
    validate_renderer_requirements_with_warp(
        info,
        flags,
        requested,
        supported,
        cfg!(feature = "test-support")
            && std::env::var("MUI_TEST_ALLOW_WARP").is_ok_and(|value| value == "1"),
    )
}

fn validate_renderer_requirements_with_warp(
    info: &wgpu::AdapterInfo,
    flags: wgpu::DownlevelFlags,
    requested: &wgpu::Limits,
    supported: &wgpu::Limits,
    allow_warp: bool,
) -> Result<(), String> {
    // WARP's compute JIT faults outside Rust error handling; see the captured
    // stack in research/platform-validation-2026-10-08.md.
    if !allow_warp
        && info.backend == wgpu::Backend::Dx12
        && info.device_type == wgpu::DeviceType::Cpu
        && info.vendor == 0x1414
    {
        return Err("WARP compute shader compilation can crash; use CPU rendering".into());
    }
    if !flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS) {
        return Err("Vello requires compute shaders; downlevel GL is unsupported".into());
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

pub(super) fn validate_texture_formats(adapter: &wgpu::Adapter) -> Result<(), String> {
    use wgpu::{TextureFormat as F, TextureUsages as U};
    for (format, needed) in [
        (
            F::Rgba8Unorm,
            U::STORAGE_BINDING
                | U::TEXTURE_BINDING
                | U::RENDER_ATTACHMENT
                | U::COPY_SRC
                | U::COPY_DST,
        ),
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
    fn warp_is_rejected_before_device_creation_without_rejecting_hardware_or_mesa() {
        let limits = wgpu::Limits::default();
        let validate = |info: &wgpu::AdapterInfo| {
            validate_renderer_requirements_with_warp(
                info,
                wgpu::DownlevelFlags::COMPUTE_SHADERS,
                &limits,
                &limits,
                false,
            )
        };
        let mut info = wgpu::AdapterInfo::new(wgpu::DeviceType::Cpu, wgpu::Backend::Dx12);
        info.name = "Microsoft Basic Render Driver".into();
        info.vendor = 0x1414;
        info.device = 0x008c;
        info.driver = "10.0.26100.33438".into();
        assert!(validate(&info).unwrap_err().contains("WARP"));
        assert!(
            validate_renderer_requirements_with_warp(
                &info,
                wgpu::DownlevelFlags::COMPUTE_SHADERS,
                &limits,
                &limits,
                true,
            )
            .is_ok()
        );
        info.backend = wgpu::Backend::Vulkan;
        assert!(validate(&info).is_ok());
        info.backend = wgpu::Backend::Dx12;
        info.device_type = wgpu::DeviceType::DiscreteGpu;
        for vendor in [0x10de, 0x8086, 0x1002, 0x1414] {
            info.vendor = vendor;
            assert!(validate(&info).is_ok());
        }
        info.device_type = wgpu::DeviceType::IntegratedGpu;
        info.vendor = 0x8086;
        assert!(validate(&info).is_ok());
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
    fn required_workgroup_memory_and_storage_are_not_silently_weakened() {
        let required = vello::required_limits();
        let info = wgpu::AdapterInfo::new(wgpu::DeviceType::IntegratedGpu, wgpu::Backend::Dx12);
        for field in [
            "max_storage_buffers_per_shader_stage",
            "max_compute_workgroup_storage_size",
            "max_storage_buffer_binding_size",
            "max_buffer_size",
        ] {
            let mut supported = required.clone();
            match field {
                "max_storage_buffers_per_shader_stage" => {
                    supported.max_storage_buffers_per_shader_stage -= 1;
                }
                "max_compute_workgroup_storage_size" => {
                    supported.max_compute_workgroup_storage_size -= 1;
                }
                "max_storage_buffer_binding_size" => supported.max_storage_buffer_binding_size -= 1,
                "max_buffer_size" => supported.max_buffer_size -= 1,
                _ => unreachable!(),
            }
            let error = validate_renderer_requirements(
                &info,
                wgpu::DownlevelFlags::COMPUTE_SHADERS,
                &requested_limits(&supported),
                &supported,
            )
            .unwrap_err();
            assert!(error.contains(field), "{error}");
        }
        assert!(requested_limits(&required).check_limits(&required));
    }
}
