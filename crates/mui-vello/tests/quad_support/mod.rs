//! What the quad pixel tests share: a WebGL2-limited device, readback,
//! PNG output and the comparison against `vello_cpu`.
#![allow(dead_code, reason = "each test file uses part of it")]

use mui_scene::ResolvedScene;
use mui_scene::prelude::*;
use mui_vello::kurbo::Affine;
use mui_vello::quad::{QuadRenderer, classify};
use std::{sync::mpsc, time::Duration};

pub const W: f64 = 320.;
pub const H: f64 = 200.;
pub const SCALES: [f64; 3] = [1., 1.5, 2.];

pub fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    device_with(|_| wgpu::Limits::downlevel_webgl2_defaults())
}

/// A device with the limits `limits` picks from the adapter's: the
/// compute-shader `GpuRenderer` needs more than WebGL2 allows.
pub fn device_with(
    limits: impl FnOnce(wgpu::Limits) -> wgpu::Limits,
) -> Option<(wgpu::Device, wgpu::Queue)> {
    pollster::block_on(async {
        let adapter =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
                .request_adapter(&wgpu::RequestAdapterOptions::default())
                .await
                .map_err(|e| eprintln!("SKIPPED: no wgpu adapter ({e})"))
                .ok()?;
        eprintln!("adapter: {:?}", adapter.get_info());
        adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: limits(adapter.limits()),
                ..Default::default()
            })
            .await
            .map_err(|e| eprintln!("SKIPPED: no wgpu device ({e})"))
            .ok()
    })
}

pub fn readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    size: [u32; 2],
) -> Vec<u8> {
    let stride = (size[0] * 4).next_multiple_of(256);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(stride) * u64::from(size[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(size[1]),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let (send, recv) = mpsc::sync_channel(1);
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = send.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    recv.recv_timeout(Duration::from_secs(10))
        .expect("mapped")
        .expect("map");
    let mapped = buffer.slice(..).get_mapped_range().expect("mapped range");
    let mut pixels = Vec::with_capacity(size[0] as usize * size[1] as usize * 4);
    for row in mapped.chunks(stride as usize).take(size[1] as usize) {
        pixels.extend_from_slice(&row[..size[0] as usize * 4]);
    }
    pixels
}

pub fn save(path: &std::path::Path, size: [u32; 2], rgba: &[u8]) {
    let file = std::io::BufWriter::new(std::fs::File::create(path).expect("create png"));
    let mut encoder = png::Encoder::new(file, size[0], size[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(rgba))
        .expect("write png");
}

/// The largest per-channel difference allowed, and the mean over every
/// channel of every pixel.
#[derive(Clone, Copy)]
pub struct Limit {
    pub max: u8,
    pub mean: f64,
}

/// A scene with no curve in it. Both rasterisers compute exact box-filter
/// area coverage of the same shapes and blend premultiplied sRGB-encoded
/// values; what is left is u8 rounding (measured on RADV: max 2, mean 0.114).
pub const TIGHT: Limit = Limit { max: 2, mean: 0.12 };

/// A scene with rounded corners. The pass covers the exact circle;
/// `vello_cpu` covers the polygon its flattener makes of the arc, whose
/// chords sit up to 0.25 device px inside it. A corner pixel cut by a chord
/// therefore differs by up to about a quarter of its area (64 levels), and
/// a stroke narrower than twice its radius adds kurbo's inner-offset loop,
/// counted twice by the non-zero rule. Matching that would mean fitting
/// the reference's chords rather than the shape, so these cases take a
/// looser maximum instead, set just above what each measured; the mean
/// stays low because only corner pixels move. Each `max` is the case's
/// measured worst over the three scales, rounded up.
pub const fn corners(max: u8, mean: f64) -> Limit {
    Limit { max, mean }
}

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub quad: QuadRenderer,
}

/// `paint` through the quad pass onto a fresh `size` target, read back.
pub fn gpu_pixels(
    gpu: &mut Gpu,
    paint: &[mui_scene::Painted],
    transform: Affine,
    size: [u32; 2],
) -> Vec<u8> {
    try_gpu_pixels(gpu, paint, transform, size).expect("quad render")
}

pub fn try_gpu_pixels(
    gpu: &mut Gpu,
    paint: &[mui_scene::Painted],
    transform: Affine,
    size: [u32; 2],
) -> Result<Vec<u8>, mui_vello::quad::Error> {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    gpu.quad
        .render(&gpu.device, &gpu.queue, paint, transform, &view, size)?;
    Ok(readback(&gpu.device, &gpu.queue, &texture, size))
}

/// `scene` on the `Cpu` canvas as it paints a scene it has painted before:
/// the second frame, which takes glyphs from its atlas.
pub fn cpu_pixels(scene: &ResolvedScene, transform: Affine, size: [u32; 2]) -> Vec<u8> {
    let mut cpu = mui_vello::software::Renderer::new((size[0], size[1])).expect("cpu");
    cpu.render(scene, transform).expect("cpu render");
    cpu.invalidate();
    cpu.render(scene, transform).expect("cpu render");
    cpu.pixels().to_vec()
}

/// The target size at `scale`.
pub fn sized(scale: f64) -> [u32; 2] {
    [(W * scale).round() as u32, (H * scale).round() as u32]
}

/// Where the images of a case go.
pub fn out_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("mui-quad-pixels");
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// `scene` (resolved at each scale) through both renderers. With `keep`,
/// both images are written out whatever the result.
pub fn compare(
    gpu: &mut Gpu,
    failures: &mut Vec<String>,
    name: &str,
    limit: Limit,
    scene: impl Fn(f64) -> ResolvedScene,
) {
    compare_keep(gpu, failures, name, limit, false, scene);
}

pub fn compare_keep(
    gpu: &mut Gpu,
    failures: &mut Vec<String>,
    name: &str,
    limit: Limit,
    keep: bool,
    scene: impl Fn(f64) -> ResolvedScene,
) {
    for scale in SCALES {
        let scene = scene(scale);
        let coverage = classify(&scene.paint);
        assert!(coverage.complete(), "{name}: {coverage:?}");
        let size = sized(scale);
        let transform = Affine::scale(scale);
        let actual = gpu_pixels(gpu, &scene.paint, transform, size);
        let reference = cpu_pixels(&scene, transform, size);
        let diff: Vec<u8> = actual
            .iter()
            .zip(&reference)
            .map(|(a, b)| a.abs_diff(*b))
            .collect();
        let worst = diff.iter().copied().max().unwrap_or(0);
        let mean = diff.iter().map(|&v| f64::from(v)).sum::<f64>() / diff.len() as f64;
        println!(
            "PIXELS {name} scale={scale} max={worst}/{} mean={mean:.4}/{}",
            limit.max, limit.mean
        );
        let failed = worst > limit.max || mean > limit.mean;
        if failed || keep {
            let dir = out_dir();
            let file = |kind: &str| dir.join(format!("{name}-{scale}-{kind}.png"));
            save(&file("cpu"), size, &reference);
            save(&file("gpu"), size, &actual);
            let (channels, _) = diff.as_chunks::<4>();
            let visible: Vec<u8> = channels
                .iter()
                .flat_map(|p| {
                    let m = p.iter().copied().max().unwrap_or(0).saturating_mul(16);
                    [m, m, m, 255]
                })
                .collect();
            save(&file("diff"), size, &visible);
        }
        if failed {
            failures.push(format!(
                "{name} at {scale}x: max {worst} > {} or mean {mean:.4} > {}; images in {}",
                limit.max,
                limit.mean,
                out_dir().display()
            ));
        }
    }
}

pub fn scene_at(root: El, scale: f64) -> ResolvedScene {
    mui_scene::resolve(&SceneSpec::new(root).offered(Size::new(W, H)).scale(scale))
        .expect("resolves")
}

/// The scene's root: a square, opaque ground the size of the target.
pub fn ground(children: Vec<El>) -> El {
    stack(children)
        .size(W, H)
        .radius(0.)
        .fill(Color::srgb(0.06, 0.07, 0.09))
}

pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
    Color::srgba(r, g, b, a)
}
