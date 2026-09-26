//! What a resized `GpuRenderer` actually rasterises, on a real device.
//!
//! `#[ignore]` because it needs a GPU: CI has none, and a test that silently
//! passes on a software fallback would be worse than one that is skipped.
//!
//! ```text
//! cargo test -p mui-preview --test render_resize -- --ignored --nocapture
//! ```
//!
//! The claim under test is the one `host::Gpu::resize` is built on: a
//! renderer built small and resized paints the whole of the bigger target,
//! not just the corner its first offscreen frame covered.

use mui::prelude::*;
use mui::vello::effects::{Budget, GpuRenderer};
use mui::vello::kurbo::Affine;

const SMALL: u32 = 100;
const WIDE: u32 = 400;
const TALL: u32 = 300;

#[test]
#[ignore = "needs a GPU"]
fn a_resized_renderer_paints_its_whole_target() {
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("adapter");
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("device");
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut renderer = pollster::block_on(GpuRenderer::new(
        &device,
        &queue,
        format,
        [SMALL, SMALL],
        Budget::default(),
    ))
    .expect("renderer");
    // What `Gpu::resize` does.
    renderer.resize([WIDE, TALL]).expect("resize");

    let root = block(f64::from(WIDE), f64::from(TALL)).fill(Fill::Color(Color::srgb(1., 1., 1.)));
    let spec = SceneSpec::new(root).offered(Size::new(f64::from(WIDE), f64::from(TALL)));
    let scene = resolve(&spec).expect("scene");
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("resize target"),
        size: wgpu::Extent3d {
            width: WIDE,
            height: TALL,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    renderer
        .render(&scene, Affine::IDENTITY, &view)
        .expect("render");

    // One row is enough, but the copy still has to honour the 256-byte stride.
    let row = (WIDE * 4).next_multiple_of(256);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(row) * u64::from(TALL),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.expect("map"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let mapped = readback.slice(..).get_mapped_range().expect("mapped");
    // Inside the wide target, far outside the small one.
    let at = ((TALL - 8) * row + (WIDE - 8) * 4) as usize;
    assert_eq!(
        mapped[at..at + 4],
        [255, 255, 255, 255],
        "a resized renderer must paint its whole target"
    );
}
