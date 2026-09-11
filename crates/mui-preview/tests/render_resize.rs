//! What a resized `Scene` actually rasterises, on a real device.
//!
//! `#[ignore]` because it needs a GPU: CI has none, and a test that silently
//! passes on a software fallback would be worse than one that is skipped.
//!
//! ```text
//! cargo test -p mui-preview --test render_resize -- --ignored --nocapture
//! ```
//!
//! The claim under test is the one `host::Gpu::resize` is built on.
//! `Renderer::render` takes a `RenderSize` per call, which makes it look as
//! though the `Scene` needs no resizing of its own -- but `Scene::reset` calls
//! `viewport_state.reset(self.width, self.height)`, so the scene keeps clipping
//! to whatever it was constructed at. This renders the same rectangle twice,
//! once through each path, and reads the far corner back.

use vello_common::kurbo::Rect;
use vello_common::peniko::color::{AlphaColor, Srgb};
use vello_hybrid::{RenderSize, RenderTargetConfig, Renderer, Scene, TextureBindings};

const SMALL: u16 = 100;
const WIDE: u16 = 400;
const TALL: u16 = 300;

/// The far corner of a full-bleed white rectangle drawn into `scene`, as RGBA.
/// Black means the scene clipped the rectangle away before it reached the
/// rasteriser.
fn far_corner(device: &wgpu::Device, queue: &wgpu::Queue, scene: &mut Scene) -> [u8; 4] {
    scene.set_paint(AlphaColor::<Srgb>::new([1., 1., 1., 1.]));
    scene.fill_rect(&Rect::new(0., 0., WIDE as f64, TALL as f64));

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("resize target"),
        size: wgpu::Extent3d {
            width: WIDE as u32,
            height: TALL as u32,
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
    let (mut renderer, mut resources) = Renderer::new(
        device,
        &RenderTargetConfig {
            format: texture.format(),
            width: WIDE as u32,
            height: TALL as u32,
        },
    );
    let mut encoder = device.create_command_encoder(&Default::default());
    renderer
        .render(
            scene,
            &mut resources,
            device,
            queue,
            &mut encoder,
            &RenderSize {
                width: WIDE as u32,
                height: TALL as u32,
            },
            &view,
            &TextureBindings::new(),
        )
        .expect("render");

    // One row is enough, but the copy still has to honour the 256-byte stride.
    let row = (WIDE as u32 * 4).next_multiple_of(256);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(row) * u64::from(TALL),
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
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width: WIDE as u32,
            height: TALL as u32,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    readback.slice(..).map_async(wgpu::MapMode::Read, |r| {
        r.expect("map");
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll");
    let mapped = readback.slice(..).get_mapped_range();
    // Inside the wide scene, far outside the small one.
    let (x, y) = (WIDE as u32 - 8, TALL as u32 - 8);
    let at = (y * row + x * 4) as usize;
    let pixel = [mapped[at], mapped[at + 1], mapped[at + 2], mapped[at + 3]];
    drop(mapped);
    readback.unmap();
    pixel
}

#[test]
#[ignore = "needs a GPU"]
fn a_scene_clips_to_its_own_size_until_it_is_resized() {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("adapter");
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("device");

    // What `Gpu::resize` does.
    let mut resized = Scene::new(SMALL, SMALL);
    resized.reset_and_resize(WIDE, TALL);
    assert_eq!(
        far_corner(&device, &queue, &mut resized),
        [255, 255, 255, 255],
        "a resized scene must paint its whole surface"
    );

    // What it would do if `RenderSize` alone were enough. This is the arm that
    // makes the assertion above mean something.
    let mut stale = Scene::new(SMALL, SMALL);
    stale.reset();
    assert_eq!(
        far_corner(&device, &queue, &mut stale),
        [0, 0, 0, 0],
        "a scene that was never resized must still clip to its old size"
    );
}
