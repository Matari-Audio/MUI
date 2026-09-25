//! The retained GPU renderer on a real device. Skips, loudly, when the
//! machine has no wgpu adapter at all; a software adapter still runs it.
#![cfg(feature = "gpu-effects")]

use mui_scene::prelude::*;
use mui_scene::{Image, ResolvedScene};
use mui_vello::effects::{Budget, GpuRenderer};
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

async fn hybrid(device: &wgpu::Device, queue: &wgpu::Queue) -> GpuRenderer {
    let format = wgpu::TextureFormat::Rgba8Unorm;
    GpuRenderer::new(device, queue, format, SIZE, Budget::default())
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

/// An unchanged scene into the same target costs no encode, no Vello
/// render and no present: the target already holds the frame. A new target
/// (a swapchain's next image) gets only the present pass.
#[test]
fn a_static_frame_neither_encodes_nor_renders() {
    let Some((device, queue)) = device() else {
        return;
    };
    let (view, other) = (target(&device), target(&device));
    let scene = welded();
    let mut r = pollster::block_on(hybrid(&device, &queue));
    let first = r.render(&scene, Affine::IDENTITY, &view).unwrap();
    assert!(first.encoded_scenes > 0 && first.renders > 0);
    for _ in 0..5 {
        let idle = r.render(&scene, Affine::IDENTITY, &view).unwrap();
        assert_eq!((idle.encoded_scenes, idle.renders), (0, 0));
    }
    let moved = r.render(&scene, Affine::IDENTITY, &other).unwrap();
    assert_eq!((moved.encoded_scenes, moved.renders), (0, 0));
}

fn readable(device: &wgpu::Device) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
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
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// The target's pixels, test-side only: the renderer never reads back.
fn pixels(device: &wgpu::Device, queue: &wgpu::Queue, t: &wgpu::Texture) -> Vec<u8> {
    let row = SIZE[0] * 4;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * SIZE[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        t.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: None,
            },
        },
        t.size(),
    );
    queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let out = buffer.slice(..).get_mapped_range().unwrap().to_vec();
    out
}

fn at(p: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * SIZE[0] + x) * 4) as usize;
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

/// A host texture named by `Image::texture` is painted from the GPU copy,
/// and a redraw of it shows after `set_texture` with the scene unchanged:
/// the spectrogram path, with no readback and no upload.
#[test]
fn a_host_texture_paints_and_refreshes_without_a_new_scene() {
    let Some((device, queue)) = device() else {
        return;
    };
    let out = readable(&device);
    let view = out.create_view(&Default::default());
    let mut r = pollster::block_on(hybrid(&device, &queue));
    let host = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let fill = |rgba: [u8; 4]| {
        queue.write_texture(
            host.as_image_copy(),
            &rgba.repeat(16),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(16),
                rows_per_image: None,
            },
            host.size(),
        )
    };
    let image = Arc::new(Image::texture(7, 4, 4));
    let root = leaf(320., 200.).fill(Fill::Image(image, Fit::Fill)).id("t");
    let scene = resolve_scene(&SceneSpec::new(root).offered(Size::new(320., 200.))).unwrap();
    fill([255, 0, 0, 255]);
    r.set_texture(7, &host);
    r.render(&scene, Affine::IDENTITY, &view).unwrap();
    assert_eq!(at(&pixels(&device, &queue, &out), 160, 100), [255, 0, 0, 255]);
    fill([0, 0, 255, 255]);
    r.set_texture(7, &host);
    let again = r.render(&scene, Affine::IDENTITY, &view).unwrap();
    assert_eq!((again.encoded_scenes, again.renders), (0, 1));
    assert_eq!(at(&pixels(&device, &queue, &out), 160, 100), [0, 0, 255, 255]);
}

/// A backdrop blurs what is under it: over a hard red/blue edge, the
/// pixels either side of it inside the panel mix, and those outside stay.
#[test]
fn a_backdrop_blurs_the_edge_under_it() {
    let Some((device, queue)) = device() else {
        return;
    };
    let out = readable(&device);
    let view = out.create_view(&Default::default());
    let mut r = pollster::block_on(hybrid(&device, &queue));
    let half = |c: Color| leaf(160., 200.).fill(Fill::Color(c));
    let root = stack![
        row![half(Color::srgb(1., 0., 0.)), half(Color::srgb(0., 0., 1.))],
        leaf(320., 100.).backdrop_blur(8.).id("glass"),
    ]
    .id("root");
    let scene = resolve_scene(&SceneSpec::new(root).offered(Size::new(320., 200.))).unwrap();
    r.render(&scene, Affine::IDENTITY, &view).unwrap();
    let p = pixels(&device, &queue, &out);
    let (inside, outside) = (at(&p, 157, 75), at(&p, 157, 25));
    assert!(inside[0] < 250 && inside[2] > 5, "no blur at the edge: {inside:?}");
    assert_eq!(outside, [255, 0, 0, 255], "the blur leaked past the panel");
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
