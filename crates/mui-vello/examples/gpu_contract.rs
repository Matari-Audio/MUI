//! Native GPU conformance. Failure to get an adapter is a FAILURE, not a skipped
//! passing test. Software adapters are allowed for correctness, and named.
use mui_vello::{
    effects::{Budget, GpuRenderer},
    kurbo::Affine,
};
mod gpu_support;
fn main() -> gpu_support::Result<()> {
    pollster::block_on(run())
}
async fn run() -> gpu_support::Result<()> {
    let (info, device, queue) = gpu_support::device().await?;
    println!("ADAPTER {info:?}");
    let dir = std::env::args_os()
        .nth(1)
        .map_or_else(|| std::env::temp_dir().join("gpu-contract"), Into::into);
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
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        eprintln!("STAGE renderer_begin scale={scale}");
        let mut renderer = GpuRenderer::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            size,
            Budget::default(),
        )
        .await?;
        eprintln!("STAGE renderer_ready scale={scale}");
        let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let first = renderer.render(&scene, Affine::scale(scale), &view)?;
        assert_eq!(first.effect_draws, 1);
        assert_eq!(first.texture_allocations, 1);
        assert_eq!(first.encoded_scenes, 1);
        eprintln!("STAGE render_submitted scale={scale}");
        let a = gpu_support::readback(&device, &queue, &texture, size)?;
        eprintln!("STAGE readback_ready scale={scale}");
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
        // The quad pass has no encoded scene to keep: a morph tick redraws one
        // damage box from the retained records (it was 0 under Vello, which
        // kept an encoded scene). What it must not do: rebuild a path record,
        // touch the glyph atlas, or redraw the whole frame.
        assert_eq!((changed.encoded_scenes, changed.renders), (1, 1));
        assert!(
            changed.rendered_pixels < u64::from(size[0]) * u64::from(size[1]),
            "a morph tick redrew the whole frame"
        );
        let frame = renderer.quad_frame();
        assert_eq!(
            (frame.paths_built, frame.path_uploaded, frame.uploaded),
            (0, 0, 0),
            "a morph tick rebuilt path or glyph data"
        );
        let b = gpu_support::readback(&device, &queue, &texture, size)?;
        assert_ne!(a, b, "morph did not update pixels");
        // Further ticks reuse the slot: a morph never allocates a texture
        // (or anything else) after the first draw.
        for tick in [0.25, 0.5, 0.75] {
            scene.set_weld_morph("join", tick)?;
            let more = renderer.render(&scene, Affine::scale(scale), &view)?;
            assert_eq!(more.texture_allocations, 0, "morph {tick} allocated");
            assert_eq!(more.uniform_upload_bytes, 16, "morph {tick}");
            assert_eq!(more.effect_draws, 1, "morph {tick}");
            assert_eq!(renderer.quad_frame().paths_built, 0, "morph {tick}");
        }
        scene.set_weld_morph("join", 0.)?;
        renderer.render(&scene, Affine::scale(scale), &view)?;
        assert_eq!(b, gpu_support::readback(&device, &queue, &texture, size)?);
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
    Ok(())
}
