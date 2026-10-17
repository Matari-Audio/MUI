//! GPU/CPU oracle plus eager compilation of every Vello AA and MUI pipeline.
use mui_scene::prelude::*;
use mui_vello::{
    effects::{Budget, GpuRenderer},
    kurbo::Affine,
    software,
};
#[expect(dead_code, reason = "shared fixtures also serve gpu_contract")]
mod gpu_support;

fn main() -> gpu_support::Result<()> {
    pollster::block_on(run())
}

async fn run() -> gpu_support::Result<()> {
    let (info, device, queue) = gpu_support::device().await?;
    println!("ADAPTER {info:?}");
    println!(
        "COMPILER {:?}",
        mui_vello::host::instance_descriptor()
            .backend_options
            .dx12
            .shader_compiler
    );
    let dir = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "gpu-pixels".into()),
    );
    std::fs::create_dir_all(&dir)?;
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    // The production renderer uses area AA only. This additional renderer
    // compiles every vendored Vello pipeline, including both MSAA variants.
    eprintln!("STAGE all_pipelines_begin");
    let _all_pipelines = mui_vello::vello::Renderer::new(
        &device,
        mui_vello::vello::RendererOptions {
            antialiasing_support: mui_vello::vello::AaSupport::all(),
            ..Default::default()
        },
    )?;
    eprintln!("STAGE all_pipelines_ready");
    for scale in [1., 1.5, 2.] {
        let size = [
            (f64::from(gpu_support::W) * scale) as u32,
            (f64::from(gpu_support::H) * scale) as u32,
        ];
        // The same resolved paint is sent to independent GPU and CPU rasterizers.
        let scene = mui_scene::resolve(&SceneSpec::new(
            stack![
                block(100., 72.)
                    .radius(13.)
                    .fill(Color::srgb(0.9, 0.2, 0.1))
                    .at(20., 24.),
                block(90., 86.)
                    .radius(22.)
                    .fill(Color::srgb(0.1, 0.65, 0.9))
                    .at(85., 62.),
                stack![
                    block(140., 30.)
                        .radius(8.)
                        .fill(Color::srgb(0.2, 0.9, 0.4))
                        .at(-20., 20.)
                ]
                .size(90., 60.)
                .radius(11.)
                .clip()
                .at(220., 120.),
            ]
            .size(f64::from(gpu_support::W), f64::from(gpu_support::H))
            .radius(0.)
            .fill(Color::srgb(0.04, 0.06, 0.08)),
        ))?;
        let texture = gpu_support::target(&device, size);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        eprintln!("STAGE renderer_begin scale={scale}");
        let mut gpu = GpuRenderer::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            size,
            Budget::default(),
        )
        .await?;
        // Bgra targets construct the other set of MUI present/blur pipelines.
        eprintln!("STAGE rgba_renderer_ready scale={scale}");
        let _bgra = GpuRenderer::new(
            &device,
            &queue,
            wgpu::TextureFormat::Bgra8Unorm,
            size,
            Budget::default(),
        )
        .await?;
        gpu.render(&scene, Affine::scale(scale), &view)?;
        let actual = gpu_support::readback(&device, &queue, &texture, size)?;
        let mut cpu = software::Renderer::new((size[0], size[1]))?;
        cpu.render(&scene, Affine::scale(scale))?;
        compare(&dir, scale, size, &actual, cpu.pixels(), info.backend)?;
    }
    device.poll(wgpu::PollType::wait_indefinitely())?;
    if let Some(error) = scope.pop().await {
        return Err(error.to_string().into());
    }
    Ok(())
}

fn compare(
    dir: &std::path::Path,
    scale: f64,
    size: [u32; 2],
    actual: &[u8],
    reference: &[u8],
    backend: wgpu::Backend,
) -> gpu_support::Result<()> {
    // Explicit per-backend 8-bit channel tolerances. All pixels participate;
    // mean error prevents widespread small errors from hiding behind edge AA.
    let (maximum, mean_limit) = match backend {
        wgpu::Backend::Vulkan | wgpu::Backend::Dx12 | wgpu::Backend::Metal => {
            (if scale == 1.5 { 48 } else { 4 }, 0.01)
        }
        other => return Err(format!("unqualified pixel backend {other:?}").into()),
    };
    let diff: Vec<u8> = actual
        .iter()
        .zip(reference)
        .map(|(a, b)| a.abs_diff(*b))
        .collect();
    let worst = diff.iter().copied().max().unwrap_or(0);
    let mean = diff.iter().map(|v| f64::from(*v)).sum::<f64>() / diff.len() as f64;
    gpu_support::save(&dir.join(format!("gpu-{scale}.png")), size, actual)?;
    gpu_support::save(&dir.join(format!("cpu-{scale}.png")), size, reference)?;
    let (channels, _) = diff.as_chunks::<4>();
    let visible_diff: Vec<u8> = channels
        .iter()
        .flat_map(|p| {
            [
                p[0].saturating_mul(8),
                p[1].saturating_mul(8),
                p[2].saturating_mul(8),
                255,
            ]
        })
        .collect();
    gpu_support::save(&dir.join(format!("diff-{scale}.png")), size, &visible_diff)?;
    println!(
        "PIXELS backend={backend:?} scale={scale} worst={worst}/{maximum} mean={mean}/{mean_limit}"
    );
    if worst > maximum || mean > mean_limit {
        return Err(format!("GPU/CPU pixels exceed {backend:?} tolerance at scale {scale}").into());
    }
    Ok(())
}
