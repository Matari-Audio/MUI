//! Textures the quad pass samples in place: a weld's material, drawn by
//! the weld pass into its pool texture, in the weld's slot of the paint
//! list; and a host texture registered under a key. Neither has a CPU
//! reference (`crate::paint` refuses external material), so the checks are
//! reference-free: the target against the weld's own texture, texel for
//! pixel, edges included (this replaced a comparison with classic Vello).
#![cfg(all(feature = "gpu-quad", feature = "cpu"))]

mod quad_support;
use quad_support::*;

use mui_scene::prelude::*;
use mui_scene::{Image, ResolvedScene};
use mui_vello::effects::{Budget, WeldTextures};
use mui_vello::kurbo::Affine;
use mui_vello::quad::{Error, QuadRenderer, Reason};
use std::sync::Arc;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SIZE: [u32; 2] = [W as u32, H as u32];

/// Two welded plates, with a block under them and, when `over`, a
/// translucent block over them: the weld's slot is between the two.
fn welded(over: bool) -> ResolvedScene {
    let plate = |id: &str| block(80., 60.).radius(12.).fill(Role::Primary).id(id);
    let mut kids = vec![
        block(60., 160.)
            .radius(0.)
            .fill(rgba(0.9, 0.6, 0.1, 1.))
            .at(30., 20.),
        row![plate("a"), plate("b")]
            .gap(16.)
            .weld(Weld::all().reach(24.).blend(60.))
            .id("join")
            .at(40., 40.),
    ];
    if over {
        kids.push(
            block(70., 120.)
                .radius(0.)
                .fill(rgba(0.1, 0.8, 0.9, 0.5))
                .at(120., 30.),
        );
    }
    let root = stack(kids).size(W, H);
    let spec = SceneSpec::new(root).offered(Size::new(W, H));
    mui_scene::resolve(&spec.weld_backend(mui_scene::WeldBackend::AnalyticGpu)).expect("resolves")
}

fn target(device: &wgpu::Device) -> wgpu::Texture {
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
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn at(px: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let i = 4 * (y * width + x) as usize;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

/// The weld's pixels on the target are its texture's, texel for pixel; in
/// its slot, over what was drawn before it and under what comes after.
#[test]
fn a_weld_paints_its_texture_in_its_slot() {
    let Some((device, queue)) = device() else {
        return;
    };
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut quad = QuadRenderer::new(&device, FORMAT);
    let mut pool =
        pollster::block_on(WeldTextures::new(&device, &queue, Budget::default())).expect("pool");
    let out = target(&device);
    let view = out.create_view(&wgpu::TextureViewDescriptor::default());
    let scene = welded(true);
    // Without a pool, the weld is refused by index.
    let slot = scene
        .paint
        .iter()
        .position(|p| p.layer == mui_scene::Layer::External)
        .expect("a weld slot");
    assert_eq!(
        quad.render(&device, &queue, &scene.paint, Affine::IDENTITY, &view, SIZE),
        Err(Error::Unsupported {
            index: slot,
            reason: Reason::External
        })
    );
    quad.render_scene(
        &device,
        &queue,
        &scene,
        &mut pool,
        Affine::IDENTITY,
        &view,
        SIZE,
    )
    .expect("render");
    let px = readback(&device, &queue, &out, SIZE);
    let (_, texture, used) = pool.texture("join").expect("a texture");
    let texels = readback(
        &device,
        &queue,
        texture,
        [texture.width(), texture.height()],
    );
    let b = scene.external_weld("join").expect("the weld").bounds();
    assert_eq!(
        [b.width(), b.height()].map(|v| v as u32),
        used,
        "a texel a pixel at 1x"
    );
    assert_eq!([b.x0.fract(), b.y0.fract()], [0., 0.]);
    // Left of the translucent block: the weld itself, over the orange one.
    let (mut worst, mut covered) = (0, 0);
    for y in 0..used[1] {
        for x in 0..used[0] {
            let (dx, dy) = (b.x0 as u32 + x, b.y0 as u32 + y);
            if dx >= 120 {
                continue;
            }
            let texel = at(&texels, texture.width(), x, y);
            let got = at(&px, SIZE[0], dx, dy);
            if texel[3] < 255 {
                // An antialiased edge: the premultiplied texel over what
                // is under it (the orange block, or the cleared target).
                let orange = (30..90).contains(&dx) && (20..180).contains(&dy);
                let under = if orange {
                    [230., 153., 26., 255.]
                } else {
                    [0.; 4]
                };
                let keep = f64::from(255 - texel[3]) / 255.;
                for c in 0..4 {
                    let want = f64::from(texel[c]) + under[c] * keep;
                    assert!(
                        (f64::from(got[c]) - want).abs() <= 3.,
                        "edge texel {texel:?} at ({dx}, {dy}) channel {c}: {got:?}"
                    );
                }
                continue;
            }
            covered += 1;
            let off = got
                .iter()
                .zip(texel)
                .map(|(a, b)| a.abs_diff(b))
                .max()
                .unwrap_or(0);
            worst = worst.max(off);
        }
    }
    assert!(covered > 2000, "only {covered} opaque weld texels");
    assert!(worst <= 1, "weld pixels differ from its texture by {worst}");
    // Under the translucent block: the block blended over the weld.
    let (x, y) = (150, 70);
    let texel = at(&texels, texture.width(), x - b.x0 as u32, y - b.y0 as u32);
    let got = at(&px, SIZE[0], x, y);
    let want = [0.1_f64, 0.8, 0.9].map(|c| (c * 0.5 * 255.).round());
    for c in 0..3 {
        let blended = want[c] + f64::from(texel[c]) * 0.5;
        assert!(
            (f64::from(got[c]) - blended).abs() <= 2.,
            "channel {c}: {got:?}, the weld {texel:?} under the block"
        );
    }
    if let Some(error) = pollster::block_on(scope.pop()) {
        panic!("wgpu validation: {error}");
    }
}

/// A host texture is sampled where it is, straight alpha premultiplied; an
/// image naming a key that is not registered paints its stand-in, as on
/// the CPU canvas.
#[test]
fn a_host_texture_is_sampled_in_place() {
    let Some((device, queue)) = device() else {
        return;
    };
    let quad = QuadRenderer::new(&device, FORMAT);
    let host = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        host.as_image_copy(),
        &[200, 100, 50, 128].repeat(16),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(16),
            rows_per_image: None,
        },
        host.size(),
    );
    let image = Arc::new(Image::texture(7, 4, 4));
    let root = block(W, H).radius(0.).fill(Fill::Image(image, Fit::Fill));
    let scene = scene_at(root, 1.);
    let mut gpu = Gpu {
        device,
        queue,
        quad,
    };
    let stand_in = gpu_pixels(&mut gpu, &scene.paint, Affine::IDENTITY, SIZE);
    let cpu = cpu_pixels(&scene, Affine::IDENTITY, SIZE);
    assert_eq!(
        at(&stand_in, SIZE[0], 160, 100),
        at(&cpu, SIZE[0], 160, 100)
    );
    gpu.quad.set_texture(7, &host);
    let px = gpu_pixels(&mut gpu, &scene.paint, Affine::IDENTITY, SIZE);
    let got = at(&px, SIZE[0], 160, 100);
    let want = [200, 100, 50].map(|c: u32| ((c * 128) as f64 / 255.).round() as u8);
    for c in 0..3 {
        assert!(got[c].abs_diff(want[c]) <= 1, "{got:?} vs {want:?}");
    }
    assert_eq!(got[3], 128);
}
