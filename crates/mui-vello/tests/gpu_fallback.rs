//! What the quad pass refuses still reaches the screen. An entry refused for
//! what it paints (a vector, a bitmap glyph) is rasterised alone on
//! `vello_cpu` and drawn as an image, the frame staying on the GPU; so is a
//! third nested clip, with everything inside it. A list the pass cannot draw
//! around (a backdrop inside such a clip) is rasterised whole:
//! presented, counted, kept while the list stands, and back on the GPU once
//! the list changes.
#![cfg(feature = "gpu-quad")]

use mui_scene::prelude::*;
use mui_scene::{ResolvedScene, Vector, VectorCommand};
use mui_vello::effects::{Budget, GpuRenderer};
use mui_vello::kurbo::{Affine, Rect, Shape as _};
use mui_vello::quad::{Reason, classify};
use std::sync::Arc;

/// Rows of 768 bytes: what a readback copy needs (a multiple of 256).
const SIZE: [u32; 2] = [192, 128];

fn device() -> Option<(mui_hal::Device, mui_hal::Queue)> {
    pollster::block_on(async {
        let adapter = mui_hal::Instance::new(
            mui_hal::InstanceDescriptor::new_without_display_handle_from_env(),
        )
        .request_adapter(&mui_hal::RequestAdapterOptions::default())
        .await
        .map_err(|e| eprintln!("SKIPPED: no wgpu adapter ({e})"))
        .ok()?;
        adapter
            .request_device(&mui_hal::DeviceDescriptor::default())
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

fn star(s: Size) -> Path {
    Path::default()
        .move_to(Point::new(s.width / 2., 0.))
        .line_to(Point::new(s.width, s.height))
        .line_to(Point::new(0., s.height))
        .close()
}

/// Three path clips deep, around `inner`.
fn nested(inner: El) -> El {
    stack![
        stack![stack![inner].size(60., 60.).outline(star).clip()]
            .size(70., 70.)
            .outline(star)
            .clip()
    ]
    .size(80., 80.)
    .outline(star)
    .clip()
    .at(20., 10.)
}

/// One scene per kind the pass refuses, the reason it gives, and whether
/// the whole frame goes to the CPU.
fn refused() -> Vec<(&'static str, ResolvedScene, Reason, bool)> {
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
            false,
        ),
        (
            "bitmap glyph",
            scene(text("\u{E000}").text_size(32.).at(20., 20.), Some(bitmap)),
            Reason::Text,
            false,
        ),
        (
            "nested path clip",
            scene(nested(block(60., 60.).fill(Color::srgb(0., 1., 0.))), None),
            Reason::ClipDepth,
            false,
        ),
        (
            "backdrop in a nested path clip",
            scene(
                nested(
                    block(60., 60.)
                        .backdrop_blur(4.)
                        .fill(Color::srgba(0., 1., 0., 0.5)),
                ),
                None,
            ),
            Reason::ClipDepth,
            true,
        ),
    ]
}

fn readable(device: &mui_hal::Device) -> mui_hal::Texture {
    device.create_texture(&mui_hal::TextureDescriptor {
        label: None,
        size: mui_hal::Extent3d {
            width: SIZE[0],
            height: SIZE[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: mui_hal::TextureDimension::D2,
        format: mui_hal::TextureFormat::Rgba8Unorm,
        usage: mui_hal::TextureUsages::RENDER_ATTACHMENT | mui_hal::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn pixels(device: &mui_hal::Device, queue: &mui_hal::Queue, t: &mui_hal::Texture) -> Vec<u8> {
    let row = SIZE[0] * 4;
    let buffer = device.create_buffer(&mui_hal::BufferDescriptor {
        label: None,
        size: u64::from(row * SIZE[1]),
        usage: mui_hal::BufferUsages::COPY_DST | mui_hal::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&mui_hal::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        t.as_image_copy(),
        mui_hal::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: mui_hal::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: None,
            },
        },
        t.size(),
    );
    queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(mui_hal::MapMode::Read, |_| {});
    device.poll(mui_hal::PollType::wait_indefinitely()).unwrap();
    buffer.slice(..).get_mapped_range().unwrap().to_vec()
}

fn cpu(scene: &ResolvedScene) -> Vec<u8> {
    let mut cpu = mui_vello::software::Renderer::new((SIZE[0], SIZE[1])).expect("cpu");
    cpu.render(scene, Affine::IDENTITY).expect("cpu render");
    cpu.pixels().to_vec()
}

/// The largest channel difference between two RGBA frames.
fn worst(a: &[u8], b: &[u8]) -> Option<u8> {
    a.iter().zip(b).map(|(a, b)| a.abs_diff(*b)).max()
}

#[test]
fn each_refused_kind_is_drawn_on_the_cpu_counted_and_retried_on_the_gpu() {
    let Some((device, queue)) = device() else {
        return;
    };
    let out = readable(&device);
    let view = out.create_view(&mui_hal::TextureViewDescriptor::default());
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
        mui_hal::TextureFormat::Rgba8Unorm,
        SIZE,
        Budget::default(),
    ))
    .expect("renderer");
    let mut whole = 0;
    for (name, refused, reason, whole_frame) in refused() {
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
        if !whole_frame {
            // Patched: the entry alone from the CPU, the frame on the GPU.
            assert_eq!(stats.cpu_patches, 1, "{name}");
            assert!(!r.fallback().active, "{name}");
            assert!(r.quad_frame().drawn, "{name}: the quad pass drew it");
            let off = worst(&pixels(&device, &queue, &out), &cpu(&refused));
            assert!(off <= Some(2), "{name}: off the CPU frame by {off:?}");
            let again = r.render(&refused, Affine::IDENTITY, &view).expect(name);
            assert_eq!(again.renders, 0, "{name}: the patched frame stands");
            continue;
        }
        whole += 1;
        let fallback = r.fallback().clone();
        assert_eq!(fallback.frames, whole, "{name}");
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
        assert_eq!(r.fallback().frames, whole, "{name}");
        // A list the pass draws: back on the GPU, the CPU frame replaced.
        r.render(&plain, Affine::IDENTITY, &view).expect(name);
        assert!(!r.fallback().active, "{name}");
        assert!(r.quad_frame().drawn, "{name}: the quad pass drew it");
        let off = worst(&pixels(&device, &queue, &out), &cpu(&plain));
        assert!(
            off <= Some(2),
            "{name}: the GPU frame after the CPU one is off by {off:?}"
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
    let view = out.create_view(&mui_hal::TextureViewDescriptor::default());
    let mut r = pollster::block_on(GpuRenderer::new(
        &device,
        &queue,
        mui_hal::TextureFormat::Rgba8Unorm,
        SIZE,
        Budget::default(),
    ))
    .expect("renderer");
    let clipped = refused().swap_remove(3).1;
    assert!(classify(&clipped.paint).unsupported(Reason::ClipDepth) > 0);
    let turned = |x: f64| {
        let mut s = scene(
            block(10., 10.).fill(Color::srgb(0., 0., 1.)).at(x, 90.),
            None,
        );
        // The nested clips first, so both frames fall back whole.
        s.paint.splice(0..0, clipped.paint.iter().cloned());
        s
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
