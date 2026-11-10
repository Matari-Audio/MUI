#![cfg(feature = "gpu-effects")]
//! First-open, changed, full-redraw and resized frames must survive GPU overflow.
use mui_scene::prelude::*;
use mui_vello::{
    effects::{Budget, GpuRenderer},
    kurbo::Affine,
};
#[expect(
    dead_code,
    reason = "shared GPU example helpers include an unused fixture"
)]
#[path = "../examples/gpu_support/mod.rs"]
mod gpu_support;
fn scene(hidden: usize, alpha: f32, color: f32) -> mui_scene::ResolvedScene {
    let root = stack((0..hidden).map(|_| {
        let mut node = block(1180., 760.).radius(0.).fill(Color::srgb(1., 0., 0.));
        for _ in 0..22 {
            node = stack([node]).size(1180., 760.).opacity(alpha);
        }
        node
    }))
    .size(1180., 760.)
    .radius(0.)
    .fill(Color::srgb(color, 0.3, 0.4));
    mui_scene::resolve(&SceneSpec::new(root)).unwrap()
}
#[test]
fn nested_layers_recover_and_present_complete_frames() {
    pollster::block_on(run()).unwrap();
}
async fn run() -> gpu_support::Result<()> {
    let (info, device, queue) = match gpu_support::device().await {
        Ok(device) => device,
        Err(error) => {
            eprintln!("SKIPPED: no GPU device ({error})");
            return Ok(());
        }
    };
    println!("ADAPTER {} {:?}", info.name, info.backend);
    for alpha in [0., 0.9] {
        let size = [1180, 760];
        let target = gpu_support::target(&device, size);
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        // A fresh renderer AND target prevent a stale cached frame from passing.
        let mut renderer = GpuRenderer::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            size,
            Budget::default(),
        )
        .await?;
        let initial = scene(1, alpha, 0.2);
        let first = renderer.render(&initial, Affine::IDENTITY, &view)?;
        assert_eq!(first.encoded_scenes, 1);
        assert_eq!(
            first.rendered_pixels,
            u64::from(size[0]) * u64::from(size[1])
        );
        let before = gpu_support::readback(&device, &queue, &target, size)?;
        assert!(
            before.as_chunks::<4>().0.iter().all(|p| p[3] == 255),
            "first frame must be opaque"
        );
        assert!(
            before
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[..3] != [0, 0, 0]),
            "first frame must contain visible color"
        );
        let next = scene(1, alpha, 0.7);
        renderer.render(&next, Affine::IDENTITY, &view)?;
        let changed = gpu_support::readback(&device, &queue, &target, size)?;
        assert_ne!(
            before, changed,
            "a failed render must not reuse the previous frame"
        );
        renderer.invalidate();
        let full = renderer.render(&next, Affine::IDENTITY, &view)?;
        assert_eq!(
            full.encoded_scenes, 1,
            "native expose must force full encoding"
        );
        assert_eq!(
            full.rendered_pixels,
            u64::from(size[0]) * u64::from(size[1])
        );
        assert_eq!(
            changed,
            gpu_support::readback(&device, &queue, &target, size)?
        );
        let resized = [590, 380];
        renderer.resize(resized)?;
        let target = gpu_support::target(&device, resized);
        let resized_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let frame = renderer.render(&next, Affine::scale(0.5), &resized_view)?;
        assert_eq!(frame.encoded_scenes, 1);
        assert_eq!(
            frame.rendered_pixels,
            u64::from(resized[0]) * u64::from(resized[1])
        );
        let pixels = gpu_support::readback(&device, &queue, &target, resized)?;
        assert!(
            pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255),
            "resized first frame must be opaque"
        );
        println!(
            "PASS alpha={alpha} first_opaque={} changed=true full_redraw=true resized_opaque={}",
            size[0] * size[1],
            resized[0] * resized[1]
        );
    }
    Ok(())
}

#[test]
fn cpu_coarse_stages_preserve_download_and_render_contract() {
    pollster::block_on(async {
        let (_, device, queue) = match gpu_support::device().await {
            Ok(device) => device,
            Err(error) => {
                eprintln!("SKIPPED: no GPU device ({error})");
                return;
            }
        };
        let size = [64, 64];
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("CPU coarse output"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let mut renderer = vello::Renderer::new(
            &device,
            vello::RendererOptions {
                use_cpu: true,
                ..vello::RendererOptions::default()
            },
        )
        .unwrap();
        let mut scene = vello::Scene::new();
        scene.fill(
            vello::peniko::Fill::NonZero,
            Affine::IDENTITY,
            vello::peniko::Color::WHITE,
            None,
            &mui_vello::kurbo::Rect::new(0., 0., 64., 64.),
        );
        renderer
            .render_to_texture(
                &device,
                &queue,
                &scene,
                &target.create_view(&wgpu::TextureViewDescriptor::default()),
                &vello::RenderParams {
                    base_color: vello::peniko::Color::BLACK,
                    width: size[0],
                    height: size[1],
                    antialiasing_method: vello::AaConfig::Area,
                },
            )
            .unwrap();
        let pixels = gpu_support::readback(&device, &queue, &target, size).unwrap();
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [255; 4]),
            "CPU coarse stages must render every pixel"
        );
    });
}
