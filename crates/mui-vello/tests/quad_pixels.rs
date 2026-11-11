//! The quad pass against `vello_cpu`: the same paint list through
//! [`mui_vello::software::Renderer`] and through [`QuadRenderer`], read back
//! and compared pixel by pixel, at device scales 1, 1.5 and 2.
//!
//! The device is opened with WebGL2's downlevel limits, so the pipeline is
//! validated against them too. Skips, loudly, when the machine has no wgpu
//! adapter at all. On failure the CPU image, the GPU image and a diff are
//! written to a temp directory whose path the failure prints.
#![cfg(all(feature = "gpu-quad", feature = "cpu"))]

use mui_scene::prelude::*;
use mui_scene::{Layer, ResolvedScene, ShadowKind};
use mui_vello::kurbo::Affine;
use mui_vello::quad::{Error, QuadRenderer, Reason, classify};
use std::{sync::mpsc, time::Duration};

const W: f64 = 320.;
const H: f64 = 200.;
const SCALES: [f64; 3] = [1., 1.5, 2.];

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
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
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                ..Default::default()
            })
            .await
            .map_err(|e| eprintln!("SKIPPED: no wgpu device ({e})"))
            .ok()
    })
}

fn readback(
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

fn save(path: &std::path::Path, size: [u32; 2], rgba: &[u8]) {
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
struct Limit {
    max: u8,
    mean: f64,
}

/// A scene with no curve in it. Both rasterisers compute exact box-filter
/// area coverage of the same shapes and blend premultiplied sRGB-encoded
/// values; what is left is u8 rounding (measured on RADV: max 2, mean 0.114).
const TIGHT: Limit = Limit {
    max: 2,
    mean: 0.12,
};

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
const fn corners(max: u8, mean: f64) -> Limit {
    Limit { max, mean }
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    quad: QuadRenderer,
}

/// `scene` (resolved at each scale) through both renderers.
fn compare(
    gpu: &mut Gpu,
    failures: &mut Vec<String>,
    name: &str,
    limit: Limit,
    scene: impl Fn(f64) -> ResolvedScene,
) {
    for scale in SCALES {
        let scene = scene(scale);
        let coverage = classify(&scene.paint);
        assert!(coverage.complete(), "{name}: {coverage:?}");
        let size = [(W * scale).round() as u32, (H * scale).round() as u32];
        let transform = Affine::scale(scale);
        let mut cpu = mui_vello::software::Renderer::new((size[0], size[1])).expect("cpu");
        cpu.render(&scene, transform).expect("cpu render");
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
            .render(&gpu.device, &gpu.queue, &scene.paint, transform, &view, size)
            .expect("quad render");
        let actual = readback(&gpu.device, &gpu.queue, &texture, size);
        let reference = cpu.pixels();
        let diff: Vec<u8> = actual
            .iter()
            .zip(reference)
            .map(|(a, b)| a.abs_diff(*b))
            .collect();
        let worst = diff.iter().copied().max().unwrap_or(0);
        let mean = diff.iter().map(|&v| f64::from(v)).sum::<f64>() / diff.len() as f64;
        println!(
            "PIXELS {name} scale={scale} max={worst}/{} mean={mean:.4}/{}",
            limit.max, limit.mean
        );
        if worst > limit.max || mean > limit.mean {
            let dir = std::env::temp_dir().join("mui-quad-pixels");
            std::fs::create_dir_all(&dir).expect("temp dir");
            let file = |kind: &str| dir.join(format!("{name}-{scale}-{kind}.png"));
            save(&file("cpu"), size, reference);
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
            failures.push(format!(
                "{name} at {scale}x: max {worst} > {} or mean {mean:.4} > {}; images in {}",
                limit.max,
                limit.mean,
                dir.display()
            ));
        }
    }
}

fn resolve(root: El, scale: f64) -> ResolvedScene {
    mui_scene::resolve(&SceneSpec::new(root).offered(Size::new(W, H)).scale(scale))
        .expect("resolves")
}

/// The scene's root: a square, opaque ground the size of the target.
fn ground(children: Vec<El>) -> El {
    stack(children)
        .size(W, H)
        .radius(0.)
        .fill(Color::srgb(0.06, 0.07, 0.09))
}

fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
    Color::srgba(r, g, b, a)
}

/// No curve anywhere: square fills, strokes with square joins, gradients,
/// shadows of square boxes, square clips and translucent overlap. What is
/// left between the two rasterisers here is the pass itself.
fn square(scale: f64) -> ResolvedScene {
    let (red, blue) = (Color::srgb(0.95, 0.2, 0.1), Color::srgb(0.1, 0.3, 0.95));
    let fills: Vec<Fill> = vec![
        Color::srgb(0.9, 0.3, 0.2).into(),
        Gradient::linear(90., [(0., red), (1., blue)]).into(),
        Gradient::linear(30., [(0., red), (0.4, rgba(0.2, 0.9, 0.3, 0.5)), (1., blue)]).into(),
        Gradient::radial((0.4, 0.6), 0.7, [(0., red), (1., blue)]).into(),
        Gradient::conic(20., [(0., red), (0.5, blue), (1., red)]).into(),
    ];
    let mut kids: Vec<El> = fills
        .into_iter()
        .enumerate()
        .map(|(i, fill)| {
            block(52., 40.)
                .radius(0.)
                .fill(fill)
                .shadow(Shadow {
                    blur: 2. + 3. * i as f64,
                    dx: 2.,
                    dy: 3.,
                    spread: 0.,
                    kind: ShadowKind::Drop,
                    fill: Fill::Color(rgba(0., 0., 0., 0.5)),
                })
                .at(10. + 60. * i as f64, 10.)
        })
        .collect();
    kids.push(
        block(80., 50.)
            .radius(0.)
            .fill(rgba(0.85, 0.87, 0.9, 0.8))
            .shadow(Shadow::inset(6.))
            .at(14., 76.),
    );
    for (i, c) in [rgba(1., 0.2, 0.1, 0.5), rgba(0.1, 0.9, 0.3, 0.4), rgba(0.2, 0.3, 1., 0.65)]
        .into_iter()
        .enumerate()
    {
        kids.push(block(70., 50.).radius(0.).fill(c).at(110. + 25. * i as f64, 70. + 15. * i as f64));
    }
    kids.push(
        stack![
            stack![block(200., 30.).radius(0.).fill(Color::srgb(0.9, 0.8, 0.2)).at(-30., 10.)]
                .size(60., 50.)
                .radius(0.)
                .clip()
                .at(10., 10.)
        ]
        .size(70., 80.)
        .radius(0.)
        .clip()
        .at(235., 75.),
    );
    resolve(ground(kids), scale)
}

fn rounded(scale: f64) -> ResolvedScene {
    let mut kids = Vec::new();
    for (i, r) in [0., 2.5, 6., 13.].into_iter().enumerate() {
        let x = 12. + 62. * i as f64;
        kids.push(
            block(50., 34.)
                .radius(r)
                .fill(Color::srgb(0.9, 0.3, 0.2))
                .at(x, 10.),
        );
        // Strokes, including one wider than twice its radius.
        for (j, w) in [1., 2.5, 7.].into_iter().enumerate() {
            kids.push(
                block(50., 30.)
                    .radius(r)
                    .no_fill()
                    .stroke(Color::srgb(0.3, 0.8, 1.))
                    .stroke_width(w)
                    .at(x + 0.3 * j as f64, 54. + 42. * j as f64),
            );
        }
    }
    kids.push(
        block(56., 28.)
            .pill()
            .fill(Color::srgb(0.95, 0.85, 0.2))
            .at(258., 12.),
    );
    kids.push(
        block(28., 70.)
            .pill()
            .fill(Color::srgb(0.2, 0.9, 0.5))
            .stroke(Color::srgb(1., 1., 1.))
            .stroke_width(2.)
            .at(272., 60.),
    );
    resolve(ground(kids), scale)
}

fn gradients(scale: f64) -> ResolvedScene {
    let (red, blue) = (Color::srgb(0.95, 0.2, 0.1), Color::srgb(0.1, 0.3, 0.95));
    let fills: Vec<Fill> = vec![
        Gradient::linear(90., [(0., red), (1., blue)]).into(),
        Gradient::linear(45., [(0., red), (0.5, Color::srgb(0.2, 0.9, 0.3)), (1., blue)]).into(),
        Gradient::linear(180., [(0.2, red), (0.8, blue)]).into(),
        Gradient::linear(
            300.,
            [
                (0., rgba(1., 1., 1., 0.)),
                (0.3, red),
                (0.6, rgba(0.1, 0.3, 0.95, 0.5)),
                (1., Color::srgb(0.9, 0.9, 0.2)),
            ],
        )
        .into(),
        Gradient::radial((0.5, 0.5), 0.5, [(0., red), (1., blue)]).into(),
        Gradient::radial((0.3, 0.7), 0.8, [(0., rgba(1., 1., 1., 0.9)), (1., rgba(0., 0., 0., 0.))])
            .into(),
        Gradient::conic(0., [(0., red), (1., blue)]).into(),
        Gradient::conic(30., [(0., red), (0.5, Color::srgb(0.2, 0.9, 0.3)), (1., blue)]).into(),
    ];
    let kids = fills
        .into_iter()
        .enumerate()
        .map(|(i, fill)| {
            let (x, y) = (12. + 76. * (i % 4) as f64, 14. + 92. * (i / 4) as f64);
            block(66., 78.).radius(9.).fill(fill).at(x, y)
        })
        .collect();
    resolve(ground(kids), scale)
}

fn shadows(scale: f64) -> ResolvedScene {
    let card = |blur: f64, dx: f64, dy: f64, spread: f64| {
        block(64., 44.)
            .radius(10.)
            .fill(Color::srgb(0.85, 0.87, 0.9))
            .shadow(Shadow {
                blur,
                dx,
                dy,
                spread,
                kind: ShadowKind::Drop,
                fill: Fill::Color(rgba(0., 0., 0., 0.6)),
            })
    };
    let recess = |blur: f64, dx: f64, dy: f64| {
        block(64., 44.)
            .radius(12.)
            .fill(Color::srgb(0.85, 0.87, 0.9))
            .shadow(Shadow {
                blur,
                dx,
                dy,
                spread: 0.,
                kind: ShadowKind::Inset,
                fill: Fill::Color(rgba(0.1, 0., 0.2, 0.7)),
            })
    };
    let kids = vec![
        card(2., 0., 1., 0.).at(20., 16.),
        card(8., 3., 4., 0.).at(110., 16.),
        card(20., -4., 8., 2.).at(210., 16.),
        recess(3., 0., 2.).at(20., 116.),
        recess(10., 4., -3.).at(110., 116.),
        recess(6., 0., 0.).radius(0.).at(210., 116.),
    ];
    resolve(ground(kids), scale)
}

fn clips(scale: f64) -> ResolvedScene {
    let bar = |y: f64, c: Color| block(300., 22.).radius(6.).fill(c).at(-40., y);
    let inner = stack![
        bar(4., Color::srgb(0.9, 0.3, 0.2)),
        bar(30., Color::srgb(0.2, 0.8, 0.4)),
        bar(56., Color::srgb(0.2, 0.4, 0.95)),
    ]
    .size(150., 90.)
    .radius(18.)
    .clip()
    .at(-10., 10.);
    let middle = stack![inner].size(170., 120.).radius(0.).clip().at(20.5, 15.25);
    let outer = stack![middle].size(150., 150.).radius(0.).clip().at(30., 20.);
    // A square clip inside a rounded one, the other way round.
    let rounded_outside = stack![
        stack![bar(10., Color::srgb(0.9, 0.8, 0.2)), bar(40., Color::srgb(0.8, 0.2, 0.9))]
            .size(60., 60.)
            .radius(0.)
            .clip()
            .at(-10., 0.)
    ]
    .size(80., 80.)
    .radius(24.)
    .clip()
    .at(210., 40.);
    resolve(ground(vec![outer, rounded_outside]), scale)
}

/// Overlapping translucent fills: order and blending.
fn overlap(scale: f64) -> ResolvedScene {
    let kids: Vec<El> = [
        (rgba(1., 0.2, 0.1, 0.5), 30., 30.),
        (rgba(0.1, 0.9, 0.3, 0.4), 80., 60.),
        (rgba(0.2, 0.3, 1., 0.65), 130., 40.),
        (rgba(1., 1., 1., 0.25), 60., 90.),
    ]
    .into_iter()
    .map(|(c, x, y)| block(120., 80.).radius(16.).fill(c).at(x, y))
    .collect();
    // The ground itself translucent, so the target keeps alpha.
    let root = stack(kids).size(W, H).radius(0.).fill(rgba(0.5, 0.5, 0.6, 0.3));
    resolve(root, scale)
}

/// A rotated subtree: its fills, strokes, gradients, shadows and rounded
/// clip turn together.
fn rotated(scale: f64) -> ResolvedScene {
    let panel = stack![
        block(90., 30.)
            .radius(8.)
            .fill(Gradient::linear(90., [(0., Color::srgb(0.9, 0.2, 0.2)), (1., Color::srgb(0.2, 0.2, 0.9))]))
            .at(10., 10.),
        block(60., 40.)
            .radius(0.)
            .fill(Color::srgb(0.2, 0.8, 0.4))
            .stroke(Color::srgb(1., 1., 1.))
            .stroke_width(2.)
            .at(20., 50.),
        block(40., 40.)
            .radius(12.)
            .fill(Color::srgb(0.9, 0.8, 0.3))
            .shadow(Shadow::soft(6.))
            .at(100., 50.),
    ]
    .size(150., 100.)
    .radius(14.)
    .fill(Color::srgb(0.3, 0.32, 0.36))
    .clip()
    .rotation(0.35)
    .at(70., 40.);
    resolve(ground(vec![panel]), scale)
}

/// A subtree under a non-integer scale and a fractional translation, set by
/// hand: the DSL has no subtree scale.
fn scaled(scale: f64) -> ResolvedScene {
    let mut scene = resolve(
        ground(vec![
            block(70., 40.)
                .radius(9.)
                .fill(Color::srgb(0.9, 0.3, 0.2))
                .stroke(Color::srgb(1., 1., 1.))
                .stroke_width(1.5)
                .shadow(Shadow::soft(5.))
                .id("a")
                .at(20., 20.),
            block(60., 60.)
                .radius(30.)
                .fill(Gradient::radial((0.5, 0.5), 0.5, [(0., Color::srgb(1., 1., 1.)), (1., Color::srgb(0.1, 0.4, 0.9))]))
                .id("b")
                .at(110., 30.),
        ]),
        scale,
    );
    let t = Affine::translate((7.3, 4.6)) * Affine::scale_non_uniform(1.37, 1.21);
    for p in &mut scene.paint {
        if matches!(p.key.as_ref(), "a" | "b") {
            p.transform = t * p.transform;
        }
    }
    scene
}

#[test]
fn the_quad_pass_matches_vello_cpu() {
    let Some((device, queue)) = device() else {
        return;
    };
    let quad = QuadRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    let mut gpu = Gpu {
        device,
        queue,
        quad,
    };
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut failures = Vec::new();
    let f = &mut failures;
    compare(&mut gpu, f, "square", TIGHT, square);
    compare(&mut gpu, f, "rounded", corners(60, 0.23), rounded);
    compare(&mut gpu, f, "gradients", corners(53, 0.11), gradients);
    compare(&mut gpu, f, "shadows", corners(51, 0.15), shadows);
    compare(&mut gpu, f, "clips", corners(45, 0.005), clips);
    compare(&mut gpu, f, "overlap", corners(37, 0.18), overlap);
    compare(&mut gpu, f, "rotated", corners(49, 0.03), rotated);
    compare(&mut gpu, f, "scaled", corners(80, 0.09), scaled);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    if let Some(error) = pollster::block_on(scope.pop()) {
        panic!("wgpu validation: {error}");
    }
}

#[test]
fn an_unsupported_entry_is_refused_by_index_and_reason() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut scene = rounded(1.);
    let at = scene
        .paint
        .iter()
        .position(|p| p.layer == Layer::Fill)
        .expect("a fill");
    scene.paint[at].rect = None;
    let mut quad = QuadRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 8,
            height: 8,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let error = quad
        .render(&device, &queue, &scene.paint, Affine::IDENTITY, &view, [8, 8])
        .expect_err("refused");
    assert_eq!(
        error,
        Error::Unsupported {
            index: at,
            reason: Reason::Path
        }
    );
}
