//! Render a MUI scene to a PNG with no window, no egui and no tessellator.
//!
//!     cargo run -p mui-vello --example headless -- /tmp/pill.png
//!
//! The whole stack end to end: a styled tree resolves to frames, welds and
//! shells become filleted outlines, [`mui_vello::paint`] hands the z-ordered
//! paint list to Vello, and Vello antialiases it analytically. It writes a
//! file instead of opening a window only because a window is a separate
//! problem: a surface would get the same pixels.

use mui_core::styled::prelude::*;
use mui_core::CornerProfile;
use vello_common::kurbo::Affine;
use vello_hybrid::{RenderSize, RenderTargetConfig, Renderer, Scene, TextureBindings};

const WIDTH: u16 = 640;
const HEIGHT: u16 = 360;

fn spec() -> SceneSpec {
    let control = |id: &str| leaf(28.0, 28.0).pill().fill(Role::Primary).id(id);
    let tab = column([control("plus"), control("phase"), control("warp")])
        .gap(10.0)
        .pad(22.0)
        .min_width(92.0)
        .align(Align::Center)
        .id("tab")
        .shell(12.0, Role::Raised);
    let panel = row([text("welded").text_size(22.0)])
        .size(520.0, 230.0)
        .pad(L)
        .id("panel");
    let root = column([tab, panel])
        .align(Align::Start)
        .id("root")
        .weld(Role::Surface);
    SceneSpec::new(root)
        .theme(Theme {
            corners: CornerProfile::new(28.0, 32.0),
            ..Theme::DEFAULT
        })
        .font(epaint_default_fonts::HACK_REGULAR.to_vec())
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or("mui-vello.png".into());
    let resolved = resolve_scene(&spec()).expect("scene resolves");
    let mut scene = Scene::new(WIDTH, HEIGHT);
    mui_vello::paint(&mut scene, &resolved, Affine::translate((32.0, 32.0))).expect("paints");
    let rgba = pollster::block_on(rasterise(&scene));
    let file = std::fs::File::create(&out).expect("create output");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), WIDTH.into(), HEIGHT.into());
    enc.set_color(png::ColorType::Rgba);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&rgba))
        .expect("write png");
    println!(
        "wrote {out}: {} surfaces, {} paint ops",
        resolved.keys.len(),
        resolved.paint.len()
    );
}

async fn rasterise(scene: &Scene) -> Vec<u8> {
    let instance = wgpu::Instance::default();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions::default())
        .await
        .expect("no GPU adapter -- this example needs one, the library does not");
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor::default())
        .await
        .expect("device");

    let size = wgpu::Extent3d {
        width: WIDTH.into(),
        height: HEIGHT.into(),
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("mui target"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let (mut renderer, mut resources) = Renderer::new(
        &device,
        &RenderTargetConfig {
            format: texture.format(),
            width: WIDTH.into(),
            height: HEIGHT.into(),
        },
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    renderer
        .render(
            scene,
            &mut resources,
            &device,
            &queue,
            &mut encoder,
            &RenderSize {
                width: WIDTH.into(),
                height: HEIGHT.into(),
            },
            &view,
            &TextureBindings::new(),
        )
        .expect("render");

    // Readback rows are padded to 256 bytes, so the copy cannot be one memcpy.
    let row = (u32::from(WIDTH) * 4).next_multiple_of(256);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(row) * u64::from(HEIGHT),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: None,
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);

    buffer.slice(..).map_async(wgpu::MapMode::Read, |r| {
        r.expect("map readback buffer");
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");

    let stride = usize::from(WIDTH) * 4;
    let mut out = Vec::with_capacity(stride * usize::from(HEIGHT));
    for line in buffer
        .slice(..)
        .get_mapped_range()
        .chunks_exact(row as usize)
    {
        out.extend_from_slice(&line[..stride]);
    }
    buffer.unmap();
    out
}
