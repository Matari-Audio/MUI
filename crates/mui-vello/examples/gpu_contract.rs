//! Native GPU conformance. Failure to get an adapter is a FAILURE, not a skipped
//! passing test. Software adapters are allowed for correctness, and named.
use mui_vello::{
    effects::{Budget, HybridEffects},
    kurbo::Affine,
};
#[allow(dead_code)]
mod gpu_support;
fn main() -> gpu_support::Result<()> {
    pollster::block_on(run())
}
async fn run() -> gpu_support::Result<()> {
    let (info, device, queue) = gpu_support::device().await?;
    println!("ADAPTER {info:?}");
    let dir = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "gpu-contract".into()),
    );
    std::fs::create_dir_all(&dir)?;
    for scale in [1., 1.5, 2.] {
        let size = [
            (f64::from(gpu_support::W) * scale) as u32,
            (f64::from(gpu_support::H) * scale) as u32,
        ];
        let mut scene = gpu_support::fixture(scale);
        let layout = scene.layout.clone();
        let paint = scene.paint.clone();
        let texture = gpu_support::target(&device, size);
        let view = texture.create_view(&Default::default());
        let mut renderer = HybridEffects::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            size,
            Budget::default(),
        )
        .await?;
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let first = renderer.render(&scene, Affine::scale(scale), &view)?;
        assert_eq!(first.effect_draws, 1);
        assert_eq!(first.texture_allocations, 1);
        assert_eq!(first.encoded_scenes, 1);
        let a = gpu_support::readback(&device, &queue, &texture, size)?;
        let second = renderer.render(&scene, Affine::scale(scale), &view)?;
        assert_eq!(second.effect_draws, 0);
        assert_eq!(second.uniform_upload_bytes, 0);
        assert_eq!(second.texture_allocations, 0);
        assert_eq!(second.encoded_scenes, 0);
        assert_eq!(
            a,
            gpu_support::readback(&device, &queue, &texture, size)?,
            "retained frame changed pixels"
        );
        scene.set_weld_morph("join", 0.)?;
        assert_eq!(scene.layout, layout);
        assert_eq!(scene.paint, paint);
        let changed = renderer.render(&scene, Affine::scale(scale), &view)?;
        assert_eq!(changed.uniform_upload_bytes, 16);
        assert_eq!(changed.effect_draws, 1);
        assert_eq!(changed.texture_allocations, 0);
        assert_eq!(changed.encoded_scenes, 0);
        let b = gpu_support::readback(&device, &queue, &texture, size)?;
        assert_ne!(a, b, "morph did not update pixels");
        let f = scene.surface("front").expect("front").frame;
        let white = gpu_support::pixel(
            &b,
            size,
            ((f.x + 12.) * scale) as u32,
            ((f.y + 12.) * scale) as u32,
        );
        assert!(
            white.iter().all(|v| *v >= 254),
            "GPU content covered the later floating control: {white:?}"
        );
        assert_eq!(
            gpu_support::pixel(&b, size, size[0] - 4, 4),
            [0, 0, 0, 255],
            "outside the clip/background changed"
        );
        if let Some(error) = error_scope.pop().await {
            return Err(error.to_string().into());
        }
        gpu_support::save(&dir.join(format!("merged-{scale}.png")), size, &a)?;
        gpu_support::save(&dir.join(format!("separate-{scale}.png")), size, &b)?;
        println!("PASS scale={scale} static-reuse morph-16B no-relayout paint-order pixels");
    }
    tile_contract(&device, &queue).await?;
    Ok(())
}

async fn tile_contract(device: &wgpu::Device, queue: &wgpu::Queue) -> gpu_support::Result<()> {
    use mui_scene::prelude::*;
    use mui_vello::effects::TiledEffects;
    let make = |scale: f64| {
        let group = stack![
            leaf(120., 90.)
                .fill(Primary)
                .radius(20.)
                .border(Ink, 2.)
                .id("a"),
            leaf(110., 70.)
                .fill(Secondary)
                .radius(15.)
                .border(Ink, 4.)
                .offset(80., 20.)
                .id("b")
        ]
        .size(190., 100.)
        .gpu_weld(Weld::crisp().blend(60.))
        .id("join");
        let root = stack![
            group.anchor(Align::Start, Align::Start).offset(40., 50.),
            leaf(80., 24.)
                .fill(Ink)
                .float()
                .offset(100., 100.)
                .id("front")
        ]
        .size(900., 500.)
        .fill(Surface)
        .opacity(0.8);
        resolve_scene(&SceneSpec::new(root).scale(scale)).unwrap()
    };
    for scale in [1., 1.5, 2.] {
        let mut scene = make(scale);
        let size = [(900. * scale) as u32, (500. * scale) as u32];
        let a = gpu_support::target(device, size);
        let b = gpu_support::target(device, size);
        let mut full = HybridEffects::new(
            device,
            queue,
            wgpu::TextureFormat::Rgba8Unorm,
            size,
            Budget::default(),
        )
        .await?;
        let mut tiles = TiledEffects::new(
            device,
            queue,
            wgpu::TextureFormat::Rgba8Unorm,
            size,
            Budget::default(),
            64 * 1024 * 1024,
        )
        .await?;
        for progress in [1., 1., 0.4, 0.] {
            scene.set_weld_morph("join", progress)?;
            full.render(
                &scene,
                Affine::scale(scale),
                &a.create_view(&Default::default()),
            )?;
            tiles.render(
                &scene,
                Affine::scale(scale),
                &b.create_view(&Default::default()),
            )?;
            let x = gpu_support::readback(device, queue, &a, size)?;
            let y = gpu_support::readback(device, queue, &b, size)?;
            let max = x
                .iter()
                .zip(&y)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap_or(0);
            assert!(
                max <= 2,
                "tile/premul/guard-band parity failed at {scale}: {max}/255"
            );
        }
        tiles.render(
            &scene,
            Affine::scale(scale),
            &b.create_view(&Default::default()),
        )?;
        assert_eq!(tiles.stats().dirty_tiles, 0, "idle tile raster");
        scene.set_weld_morph("join", 0.8)?;
        tiles.render(
            &scene,
            Affine::scale(scale),
            &b.create_view(&Default::default()),
        )?;
        assert!(
            tiles.stats().dirty_tiles > 0 && tiles.stats().dirty_tiles < tiles.stats().total_tiles,
            "local effect invalidated the whole editor"
        );
        println!("PASS tile reference/parity/idle/local morph scale={scale}");
    }
    Ok(())
}
