//! What the quad pass refuses still reaches the screen: `GpuRenderer`
//! rasterises that frame on `vello_cpu`, presents it, counts it, keeps it
//! while the list stands, and goes back to the GPU once the list changes.
#![cfg(feature = "gpu-quad")]

use mui_scene::prelude::*;
use mui_scene::{ResolvedScene, Vector, VectorCommand};
use mui_vello::effects::{Budget, GpuRenderer};
use mui_vello::kurbo::{Affine, Rect, Shape as _};
use mui_vello::quad::{Reason, classify};
use std::sync::Arc;

/// Rows of 768 bytes: what a readback copy needs (a multiple of 256).
const SIZE: [u32; 2] = [192, 128];

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    pollster::block_on(async {
        let adapter =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
                .request_adapter(&wgpu::RequestAdapterOptions::default())
                .await
                .map_err(|e| eprintln!("SKIPPED: no wgpu adapter ({e})"))
                .ok()?;
        adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|e| eprintln!("SKIPPED: no wgpu device ({e})"))
            .ok()
    })
}

fn scene(root: El, font: Option<Font>) -> ResolvedScene {
    let mut spec = SceneSpec::new(
        stack![root]
            .size(192., 128.)
            .radius(0.)
            .fill(Color::srgb(0.1, 0.12, 0.15)),
    )
    .offered(Size::new(192., 128.));
    spec.font = font;
    mui_scene::resolve(&spec).expect("resolves")
}

fn hack() -> Option<Font> {
    Some(Font::new(epaint_default_fonts::HACK_REGULAR).expect("font"))
}

fn star(s: Size) -> Path {
    Path::default()
        .move_to(Point::new(s.width / 2., 0.))
        .line_to(Point::new(s.width, s.height))
        .line_to(Point::new(0., s.height))
        .close()
}

/// One scene per kind the pass refuses, with the reason it gives.
fn refused() -> Vec<(&'static str, ResolvedScene, Reason)> {
    let vector = Arc::new(
        Vector::new(
            6.,
            2.,
            vec![VectorCommand::Fill {
                path: Rect::new(0., 0., 6., 2.).to_path(0.01),
                transform: Affine::IDENTITY,
                brush: mui_vello::peniko::Brush::Solid(mui_vello::peniko::Color::from_rgba8(
                    0, 255, 0, 255,
                )),
                brush_transform: Affine::IDENTITY,
                rule: mui_vello::peniko::Fill::NonZero,
            }],
        )
        .expect("vector"),
    );
    let bitmap = Font::new(&include_bytes!("fixtures/non_apple_sbix.ttf")[..]).expect("sbix");
    vec![
        (
            "vector",
            scene(
                block(60., 20.)
                    .fill(Fill::Vector(vector, Fit::Fill))
                    .at(20., 20.),
                None,
            ),
            Reason::Vector,
        ),
        (
            "bitmap glyph",
            scene(text("\u{E000}").text_size(32.).at(20., 20.), Some(bitmap)),
            Reason::Text,
        ),
        (
            "rotated text",
            scene(
                text("turned").text_size(14.).rotation(0.3).at(20., 40.),
                hack(),
            ),
            Reason::TextTransform,
        ),
        (
            "nested path clip",
            scene(
                stack![
                    stack![block(60., 60.).fill(Color::srgb(0., 1., 0.))]
                        .size(60., 60.)
                        .outline(star)
                        .clip()
                ]
                .size(80., 80.)
                .outline(star)
                .clip()
                .at(20., 10.),
                None,
            ),
            Reason::ClipDepth,
        ),
    ]
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

fn pixels(device: &wgpu::Device, queue: &wgpu::Queue, t: &wgpu::Texture) -> Vec<u8> {
    let row = SIZE[0] * 4;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * SIZE[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
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
    buffer.slice(..).get_mapped_range().unwrap().to_vec()
}

fn cpu(scene: &ResolvedScene) -> Vec<u8> {
    let mut cpu = mui_vello::software::Renderer::new((SIZE[0], SIZE[1])).expect("cpu");
    cpu.render(scene, Affine::IDENTITY).expect("cpu render");
    cpu.pixels().to_vec()
}

#[test]
fn each_refused_kind_is_drawn_on_the_cpu_counted_and_retried_on_the_gpu() {
    let Some((device, queue)) = device() else {
        return;
    };
    let out = readable(&device);
    let view = out.create_view(&wgpu::TextureViewDescriptor::default());
    // Square, on whole pixels: the quad pass and vello_cpu agree on it.
    let plain = scene(
        block(40., 40.)
            .radius(0.)
            .fill(Color::srgb(1., 0., 0.))
            .at(10., 10.),
        None,
    );
    let mut r = pollster::block_on(GpuRenderer::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        SIZE,
        Budget::default(),
    ))
    .expect("renderer");
    for (i, (name, refused, reason)) in refused().into_iter().enumerate() {
        // The bitmap glyph is found only when drawn; the rest up front.
        if reason != Reason::Text {
            assert!(
                classify(&refused.paint).unsupported(reason) > 0,
                "{name}: {:?}",
                classify(&refused.paint)
            );
        }
        let stats = r.render(&refused, Affine::IDENTITY, &view).expect(name);
        assert!(stats.renders > 0, "{name}: nothing drawn");
        let fallback = r.fallback().clone();
        assert_eq!(fallback.frames, i as u64 + 1, "{name}");
        assert_eq!(fallback.last.map(|(_, r)| r), Some(reason), "{name}");
        assert!(fallback.active, "{name}");
        assert_eq!(
            pixels(&device, &queue, &out),
            cpu(&refused),
            "{name}: the presented frame is the CPU frame"
        );
        // The same list again: the CPU frame stands, nothing is redrawn.
        let again = r.render(&refused, Affine::IDENTITY, &view).expect(name);
        assert_eq!(again.renders, 0, "{name}");
        assert!(r.is_current(&refused, Affine::IDENTITY), "{name}");
        assert_eq!(r.fallback().frames, i as u64 + 1, "{name}");
        // A list the pass draws: back on the GPU, the CPU frame replaced.
        r.render(&plain, Affine::IDENTITY, &view).expect(name);
        assert!(!r.fallback().active, "{name}");
        assert!(r.quad_frame().drawn, "{name}: the quad pass drew it");
        let worst = pixels(&device, &queue, &out)
            .iter()
            .zip(cpu(&plain))
            .map(|(a, b)| a.abs_diff(b))
            .max();
        assert!(
            worst <= Some(2),
            "{name}: the GPU frame after the CPU one is off by {worst:?}"
        );
    }
}

/// Two refused frames in a row: the second uploads only the CPU renderer's
/// damage, and still matches a whole CPU render.
#[test]
fn consecutive_refused_frames_upload_their_damage() {
    let Some((device, queue)) = device() else {
        return;
    };
    let out = readable(&device);
    let view = out.create_view(&wgpu::TextureViewDescriptor::default());
    let mut r = pollster::block_on(GpuRenderer::new(
        &device,
        &queue,
        wgpu::TextureFormat::Rgba8Unorm,
        SIZE,
        Budget::default(),
    ))
    .expect("renderer");
    let turned = |x: f64| {
        scene(
            stack![
                text("turned").text_size(14.).rotation(0.3).at(20., 40.),
                block(10., 10.).fill(Color::srgb(0., 0., 1.)).at(x, 90.),
            ],
            hack(),
        )
    };
    r.render(&turned(10.), Affine::IDENTITY, &view).unwrap();
    let stats = r.render(&turned(30.), Affine::IDENTITY, &view).unwrap();
    assert_eq!(r.fallback().frames, 2);
    assert!(
        stats.rendered_pixels < u64::from(SIZE[0] * SIZE[1]),
        "{stats:?}"
    );
    assert_eq!(r.damage().len(), 1);
    assert_eq!(pixels(&device, &queue, &out), cpu(&turned(30.)));
}
