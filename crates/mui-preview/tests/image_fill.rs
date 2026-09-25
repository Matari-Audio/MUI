//! An image fill must not take the gallery down with it.
//!
//! `#[ignore]` because it needs a GPU, like `render_resize`:
//!
//! ```text
//! cargo test -p mui-preview --test image_fill -- --ignored --nocapture
//! ```
//!
//! Clicking the gallery's Image scene once panicked the GPU path, which
//! could not sample a pixmap. This renders an image fill through the real
//! renderer and reads a pixel back to prove the image, not a grey stand-in,
//! is what lands.

use std::sync::Arc;

use mui::prelude::*;
use mui::vello::effects::{Budget, GpuRenderer};

const N: u32 = 200;

fn render() -> [u8; 4] {
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("adapter");
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("device");

    let px = vec![
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
    ];
    let img = Arc::new(Image::rgba(2, 2, px).expect("2x2 rgba"));
    // Both fits: `Contain` also pushes a clip, which is the arm that has to
    // stay balanced when the image paint is swapped for its stand-in.
    let root = column([
        leaf(100.0, 60.0)
            .pill()
            .fill(Fill::Image(img.clone(), Fit::Cover)),
        leaf(100.0, 60.0)
            .radius(8.0)
            .fill(Fill::Image(img, Fit::Contain)),
    ]);
    let mut ui = Ui::new(Theme::DEFAULT);
    let frame = ui
        .frame(
            root,
            Some(Size::new(f64::from(N), f64::from(N))),
            Input::default(),
            1.0 / 60.0,
        )
        .expect("frame");

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("image fill target"),
        size: wgpu::Extent3d {
            width: N,
            height: N,
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
    let mut renderer = pollster::block_on(GpuRenderer::new(
        &device,
        &queue,
        texture.format(),
        [N, N],
        Budget::default(),
    ))
    .expect("renderer");
    renderer
        .render(frame.scene, mui::vello::kurbo::Affine::IDENTITY, &view)
        .expect("render");

    let row = (N * 4).next_multiple_of(256);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(row) * u64::from(N),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width: N,
            height: N,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.expect("map"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let mapped = readback.slice(..).get_mapped_range().expect("mapped");
    // Just inside the first (Cover) pill's top-left cap. Bilinear sampling
    // blends a 2x2 image into a gradient everywhere but the corners, so this
    // is where the red texel is still red -- or where the stand-in is.
    let at = (6 * row + 74 * 4) as usize;
    let px = [mapped[at], mapped[at + 1], mapped[at + 2], mapped[at + 3]];
    drop(mapped);
    readback.unmap();
    px
}

#[test]
#[ignore = "needs a GPU"]
fn an_image_fill_lands_on_the_pixel() {
    let [r, g, b, a] = render();
    assert!(
        r > 200 && g < 60 && b < 60 && a == 255,
        "expected red, got {:?}",
        [r, g, b, a]
    );
}
