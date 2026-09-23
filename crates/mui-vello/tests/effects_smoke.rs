//! The retained GPU renderers on a real device. Skips, loudly, when the
//! machine has no wgpu adapter at all; a software adapter still runs it.
#![cfg(feature = "gpu-effects")]

use mui_scene::prelude::*;
use mui_scene::{Image, ResolvedScene};
use mui_vello::effects::{Budget, EffectStats, HybridEffects, TiledEffects};
use mui_vello::kurbo::Affine;
use std::sync::Arc;

const SIZE: [u32; 2] = [320, 200];
/// Far enough right that nothing lands on the target.
const AWAY: Affine = Affine::new([1., 0., 0., 1., 5000., 0.]);

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    pollster::block_on(async {
        let adapter = wgpu::Instance::default()
            .request_adapter(&Default::default())
            .await
            .map_err(|e| eprintln!("SKIPPED: no wgpu adapter ({e})"))
            .ok()?;
        adapter
            .request_device(&Default::default())
            .await
            .map_err(|e| eprintln!("SKIPPED: no wgpu device ({e})"))
            .ok()
    })
}

fn target(device: &wgpu::Device) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: SIZE[0],
                height: SIZE[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

fn welded() -> ResolvedScene {
    let plate = |id: &str| leaf(80., 60.).radius(12.).fill(Role::Primary).id(id);
    let root = row![plate("a"), plate("b")]
        .gap(16.)
        .gpu_weld(Weld::all().reach(24.).blend(60.))
        .id("join")
        .anchor(Align::Start, Align::Start);
    resolve_scene(&SceneSpec::new(root).offered(Size::new(320., 200.))).unwrap()
}

fn pictured(v: u8) -> (ResolvedScene, std::sync::Weak<[u8]>) {
    let image = Arc::new(Image::rgba(2, 2, vec![v; 16]).unwrap());
    let buffer = Arc::downgrade(&image.rgba);
    let root = leaf(40., 40.).fill(Fill::Image(image, Fit::Fill)).id("img");
    let scene = resolve_scene(&SceneSpec::new(root).offered(Size::new(40., 40.))).unwrap();
    (scene, buffer)
}

async fn hybrid(device: &wgpu::Device, queue: &wgpu::Queue) -> HybridEffects {
    let format = wgpu::TextureFormat::Rgba8Unorm;
    HybridEffects::new(device, queue, format, SIZE, Budget::default())
        .await
        .unwrap()
}

/// A weld that scrolls offscreen keeps its texture, so scrolling back neither
/// reallocates nor re-renders it. It used to be evicted the frame it left.
#[test]
fn a_weld_scrolled_away_and_back_is_not_rendered_again() {
    let Some((device, queue)) = device() else {
        return;
    };
    let view = target(&device);
    let mut r = pollster::block_on(hybrid(&device, &queue));
    let scene = welded();
    let first = r.render(&scene, Affine::IDENTITY, &view).unwrap();
    assert_eq!((first.texture_allocations, first.effect_draws), (1, 1));
    let away = r.render(&scene, AWAY, &view).unwrap();
    assert_eq!(away.effect_draws, 0);
    let back = r.render(&scene, Affine::IDENTITY, &view).unwrap();
    assert_eq!(
        (back.texture_allocations, back.effect_draws),
        (0, 0),
        "scrolling back rebuilt the weld texture"
    );
}

/// The tiled renderer only admits welds some tile can sample.
#[test]
fn tiles_do_not_render_an_offscreen_weld() {
    let Some((device, queue)) = device() else {
        return;
    };
    let view = target(&device);
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut r = pollster::block_on(TiledEffects::new(
        &device,
        &queue,
        format,
        SIZE,
        Budget::default(),
        64 * 1024 * 1024,
    ))
    .unwrap();
    let scene = welded();
    let away = r.render(&scene, AWAY, &view).unwrap();
    assert_eq!((away.texture_allocations, away.effect_draws), (0, 0));
    let here = r.render(&scene, Affine::IDENTITY, &view).unwrap();
    assert_eq!((here.texture_allocations, here.effect_draws), (1, 1));
    let idle = r.render(&scene, Affine::IDENTITY, &view).unwrap();
    assert_eq!(idle.effect_draws, 0);
    assert_eq!(r.stats().dirty_tiles, 0, "an idle frame redrew tiles");
}

/// Once the app drops an image, nothing on the GPU path keeps its buffer.
/// The global pixmap cache and the atlas ids used to hold a clone each, and
/// each waited for the other to let go: every morph bake leaked one.
#[test]
fn a_dropped_image_buffer_is_freed_on_the_gpu_path() {
    let Some((device, queue)) = device() else {
        return;
    };
    let view = target(&device);
    let mut r = pollster::block_on(hybrid(&device, &queue));
    let (a, gone) = pictured(10);
    r.render(&a, Affine::IDENTITY, &view).unwrap();
    drop(a);
    let (b, kept) = pictured(20);
    r.render(&b, Affine::IDENTITY, &view).unwrap();
    assert!(
        gone.upgrade().is_none(),
        "the renderer kept the app's buffer"
    );
    assert!(kept.upgrade().is_some());
}

/// An unchanged scene into the same target costs no encode and no GPU pass
/// on either retained renderer: the target already holds the frame. A new
/// target gets the pass again, from the retained encoding.
#[test]
fn a_static_frame_neither_encodes_nor_renders() {
    let Some((device, queue)) = device() else {
        return;
    };
    let (view, other) = (target(&device), target(&device));
    let scene = welded();
    let mut whole = pollster::block_on(hybrid(&device, &queue));
    let mut tiles = pollster::block_on(TiledEffects::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        SIZE,
        Budget::default(),
        64 * 1024 * 1024,
    ))
    .unwrap();
    let mut frames: [&mut dyn FnMut(&wgpu::TextureView) -> EffectStats; 2] = [
        &mut |v| whole.render(&scene, Affine::IDENTITY, v).unwrap(),
        &mut |v| tiles.render(&scene, Affine::IDENTITY, v).unwrap(),
    ];
    for frame in &mut frames {
        let first = frame(&view);
        assert!(first.encoded_scenes > 0 && first.renders > 0);
        for _ in 0..5 {
            let idle = frame(&view);
            assert_eq!((idle.encoded_scenes, idle.renders), (0, 0));
        }
        let moved = frame(&other);
        assert_eq!((moved.encoded_scenes, moved.renders), (0, 1));
    }
}

/// A weld gone from the scene gives its texture back after a few frames,
/// without waiting for budget pressure.
#[test]
fn a_removed_weld_frees_its_texture() {
    let Some((device, queue)) = device() else {
        return;
    };
    let view = target(&device);
    let mut r = pollster::block_on(hybrid(&device, &queue));
    let first = r.render(&welded(), Affine::IDENTITY, &view).unwrap();
    assert!(first.resident_texture_bytes > 0);
    let (plain, _) = pictured(1);
    let mut last = first;
    for _ in 0..=mui_vello::effects::ABSENT_FRAMES {
        assert!(last.resident_texture_bytes > 0, "freed too early");
        last = r.render(&plain, Affine::IDENTITY, &view).unwrap();
    }
    assert_eq!(last.resident_texture_bytes, 0, "the weld texture stayed");
}
