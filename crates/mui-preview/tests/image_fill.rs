//! An image fill must not take the gallery down with it.
//!
//! `#[ignore]` because it needs a GPU, like `render_resize`:
//!
//! ```text
//! cargo test -p mui-preview --test image_fill -- --ignored --nocapture
//! ```
//!
//! `vello_hybrid` does not `Err` on a pixmap image source, it `panic!`s
//! ("pixmap image sources are not supported by Vello Hybrid"), so the host's
//! `Err(e) => eprintln!` arm never sees it and the window dies. That is
//! exactly what clicking the gallery's Image scene used to do. `mui_vello`'s
//! `Canvas::images()` is what keeps the pixmap away from this backend; this
//! renders an image fill through the real renderer to prove it.

use std::sync::Arc;

use mui::prelude::*;
use vello_hybrid::{RenderSize, RenderTargetConfig, Renderer, Scene, TextureBindings};

const N: u32 = 200;

#[test]
#[ignore = "needs a GPU"]
fn an_image_fill_reaches_the_hybrid_renderer_without_panicking() {
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

    let mut scene = Scene::new(N as u16, N as u16);
    let (mut renderer, mut resources) = Renderer::new(
        &device,
        &RenderTargetConfig {
            format: wgpu::TextureFormat::Rgba8Unorm,
            width: N,
            height: N,
        },
    );
    scene.reset();
    mui::vello::paint(
        &mut mui::vello::Gpu {
            scene: &mut scene,
            resources: &mut resources,
        },
        frame.scene,
        mui::vello::kurbo::Affine::IDENTITY,
    )
    .expect("paint");

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
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    renderer
        .render(
            &scene,
            &mut resources,
            &device,
            &queue,
            &mut encoder,
            &RenderSize {
                width: N,
                height: N,
            },
            &view,
            &TextureBindings::new(),
        )
        .expect("render");
    queue.submit([encoder.finish()]);
}
