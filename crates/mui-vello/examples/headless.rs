//! Render a MUI scene to a PNG with no window, no egui and no tessellator.
//!
//!     cargo run -p mui-vello --example headless -- /tmp/pill.png
//!
//! This is the whole stack end to end: intrinsic layout resolves the frames,
//! the surface graph unions and fillets them, [`mui_vello::bez_path`] hands the
//! result to Vello, and Vello antialiases it analytically. The only reason it
//! writes a file instead of opening a window is that a window is a separate
//! problem -- the pixels above are the same pixels a surface would get.

use mui_core::dsl::column;
use mui_core::{
    resolve_scene, CornerProfile, CornerRule, FrameRadius, SceneSpec, Spacing, SurfaceSpec, Theme,
};
use mui_layout::{Align, Node, Size};
use vello_common::kurbo::Affine;
use vello_common::peniko::color::palette::css;
use vello_common::peniko::color::AlphaColor;
use vello_hybrid::{RenderSize, RenderTargetConfig, Renderer, Scene, TextureBindings};

const WIDTH: u16 = 640;
const HEIGHT: u16 = 360;

/// The gallery's canonical scene: a tab welded to a panel, unioned sharp and
/// filleted after, with an inner shell offset from the *merged* outline.
fn spec() -> SceneSpec {
    let controls = column(
        "controls",
        [
            Node::leaf("plus", Size::new(28.0, 28.0)),
            Node::leaf("phase", Size::new(28.0, 28.0)),
        ],
    )
    .gap(10.0)
    .align(Align::Center);

    let tab = column(
        "tab-frame",
        [column("pill-frame", [controls]).padding(10.0)],
    )
    .padding(12.0)
    .min_size(Size::new(92.0, 0.0));

    let root = column(
        "root",
        [tab, Node::leaf("panel-frame", Size::new(420.0, 180.0))],
    )
    .align(Align::Start);

    SceneSpec::new(root)
        .theme(Theme {
            corners: CornerProfile::new(28.0, 32.0),
            ..Theme::default()
        })
        .surface(SurfaceSpec::frame("panel", "panel-frame").radius(FrameRadius::Global))
        .surface(SurfaceSpec::frame("tab", "tab-frame").radius(FrameRadius::Global))
        .surface(SurfaceSpec::merge("outer", ["panel", "tab"]).corners(CornerRule::Global))
        .surface(SurfaceSpec::inset("pill-shell", "tab", Spacing::px(12.0)))
}

/// Later surfaces sit on top, so the merged outline paints first and the shell
/// cut out of it paints last.
fn ink(id: &str) -> AlphaColor<vello_common::peniko::color::Srgb> {
    match id {
        "outer" => AlphaColor::new([0.13, 0.14, 0.17, 1.0]),
        "pill-shell" => AlphaColor::new([0.35, 0.72, 0.98, 1.0]),
        "glyph" => AlphaColor::new([0.96, 0.96, 0.97, 1.0]),
        _ => css::TRANSPARENT,
    }
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or("mui-vello.png".into());
    let resolved = resolve_scene(&spec()).expect("scene resolves");

    let mut scene = Scene::new(WIDTH, HEIGHT);
    scene.set_transform(Affine::translate((32.0, 32.0)));
    for (id, surface) in resolved.surfaces() {
        let paint = ink(id);
        if paint.components[3] == 0.0 {
            continue; // An intermediate surface: real geometry, not meant to be seen.
        }
        let path = mui_vello::bez_path(&surface.path, mui_vello::ARC_TOLERANCE)
            .unwrap_or_else(|e| panic!("{id}: {e}"));
        scene.set_paint(paint);
        scene.fill_path(&path);
    }

    // A glyph is geometry here, not an atlas texture: same path pipeline, same
    // fill rule, and its counter must come out as a hole.
    let glyph = mui_text::glyph_path(
        epaint_default_fonts::HACK_REGULAR,
        'a',
        170.0,
        &[],
        mui_vello::ARC_TOLERANCE,
    )
    .expect("glyph outline");
    scene.set_transform(Affine::translate((470.0, 230.0)));
    scene.set_paint(ink("glyph"));
    scene.fill_path(&mui_vello::bez_path(&glyph, mui_vello::ARC_TOLERANCE).expect("glyph path"));

    let pixels = pollster::block_on(rasterise(&scene));
    let file = std::fs::File::create(&out).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), WIDTH.into(), HEIGHT.into());
    enc.set_color(png::ColorType::Rgba);
    enc.write_header()
        .expect("png header")
        .write_image_data(&pixels)
        .expect("png data");
    println!("wrote {out} ({WIDTH}x{HEIGHT})");
}

/// Straight out of `vello_hybrid`'s own `render_to_file` example: headless
/// device, render to a texture, copy back. Nothing MUI-specific happens here.
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
