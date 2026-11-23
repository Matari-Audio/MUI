//! Group layers, masks and backdrops through the quad pass, against
//! `vello_cpu` (which isolates every layer, mixes blend modes and blurs a
//! backdrop's replay through a Gaussian filter layer), and checked without
//! a reference where the answer is known: a blurred flat colour is that
//! colour, half opacity of white over black is mid grey.
#![cfg(all(feature = "gpu-quad", feature = "cpu"))]

mod quad_support;
use quad_support::*;

use mui_scene::prelude::*;
use mui_scene::{Layer, ResolvedScene};
use mui_vello::kurbo::Affine;
use mui_vello::quad::QuadRenderer;

fn star(points: usize) -> impl Fn(Size) -> Path + Clone + 'static {
    move |s: Size| {
        let c = Point::new(s.width / 2., s.height / 2.);
        let r = s.width.min(s.height) / 2.;
        Path::polyline(
            (0..2 * points).map(|i| {
                let a = std::f64::consts::PI * i as f64 / points as f64;
                let r = if i % 2 == 0 { r } else { r * 0.5 };
                Point::new(c.x + a.cos() * r, c.y + a.sin() * r)
            }),
            true,
        )
    }
}

/// Stripes and labels: something for a blur to smear.
fn busy() -> Vec<El> {
    let mut kids: Vec<El> = (0..16)
        .map(|i| {
            block(10., H)
                .radius(0.)
                .fill(if i % 2 == 0 {
                    rgba(0.9, 0.5, 0.1, 1.)
                } else {
                    rgba(0.1, 0.4, 0.8, 1.)
                })
                .at(20. * i as f64, 0.)
        })
        .collect();
    kids.push(
        block(W, 30.)
            .radius(0.)
            .fill(Gradient::linear(
                90.,
                [(0., rgba(1., 1., 1., 1.)), (1., rgba(0., 0., 0., 1.))],
            ))
            .at(0., 85.),
    );
    kids
}

/// Two groups at opacity 0.5: one of overlapping opaque and translucent
/// children (a target of its own, or the overlap would blend twice), one
/// of children apart from each other (folded into them).
fn opacity(scale: f64) -> ResolvedScene {
    let overlapping = stack![
        block(80., 60.).radius(0.).fill(rgba(0.9, 0.1, 0.1, 1.)),
        block(80., 60.)
            .radius(10.)
            .fill(rgba(0.1, 0.2, 0.9, 1.))
            .at(40., 30.),
        block(50., 50.)
            .radius(25.)
            .fill(rgba(1., 1., 0.2, 0.6))
            .at(20., 20.),
    ]
    .size(130., 100.)
    .opacity(0.5)
    .at(20., 20.);
    let apart = stack![
        block(40., 40.).radius(0.).fill(rgba(0.9, 0.9, 0.9, 1.)),
        block(40., 40.)
            .radius(8.)
            .fill(rgba(0.2, 0.9, 0.3, 0.8))
            .at(60., 0.),
    ]
    .size(100., 40.)
    .opacity(0.5)
    .at(180., 40.);
    scene_at(ground(vec![overlapping, apart]), scale)
}

/// Each blend mode over a gradient, a translucent tile per mode.
fn blends(scale: f64) -> ResolvedScene {
    use mui_scene::Mix::*;
    let modes = [
        Multiply, Screen, Overlay, Darken, Lighten, ColorDodge, ColorBurn, HardLight, SoftLight,
        Difference, Exclusion, Hue, Saturation, Color, Luminosity,
    ];
    let mut kids = vec![block(W, H).radius(0.).fill(Gradient::linear(
        90.,
        [
            (0., rgba(0.9, 0.2, 0.1, 1.)),
            (0.5, rgba(0.2, 0.8, 0.4, 1.)),
            (1., rgba(0.1, 0.2, 0.9, 1.)),
        ],
    ))];
    for (i, m) in modes.into_iter().enumerate() {
        let (x, y) = (8. + 62. * (i % 5) as f64, 8. + 64. * (i / 5) as f64);
        kids.push(
            block(54., 54.)
                .radius(6.)
                .fill(rgba(0.8, 0.6, 0.3, 0.85))
                .blend(m)
                .at(x, y),
        );
    }
    scene_at(ground(kids), scale)
}

/// A rounded clip around a translucent group around a square clip and a
/// multiplying group. (A rounded clip inside a rounded clip is
/// `Reason::ClipDepth`.)
fn nested(scale: f64) -> ResolvedScene {
    let inner = stack![
        block(120., 40.).radius(0.).fill(rgba(0.9, 0.8, 0.2, 1.)),
        block(60., 60.)
            .radius(4.)
            .fill(rgba(0.3, 0.3, 0.9, 1.))
            .blend(mui_scene::Mix::Multiply)
            .at(30., 10.),
    ]
    .size(90., 70.)
    .radius(0.)
    .clip()
    .at(30., 30.);
    let group = stack![
        block(200., 140.).radius(0.).fill(rgba(0.2, 0.7, 0.6, 1.)),
        inner,
        block(60., 30.)
            .radius(6.)
            .fill(rgba(0.9, 0.2, 0.5, 0.8))
            .at(100., 90.),
    ]
    .size(200., 140.)
    .opacity(0.7)
    .at(-20., -10.);
    let outer = stack![group]
        .size(220., 150.)
        .radius(30.)
        .clip()
        .at(50., 30.);
    scene_at(ground(vec![outer]), scale)
}

/// A card whose subtree is tinted by a ramp drawn source-atop it.
fn mask(scale: f64) -> ResolvedScene {
    let card = stack![
        block(120., 30.).radius(6.).fill(rgba(0.9, 0.9, 0.9, 1.)),
        block(60., 60.)
            .radius(30.)
            .fill(rgba(0.2, 0.5, 0.9, 0.7))
            .at(50., 20.),
    ]
    .size(160., 100.)
    .mask(Gradient::linear(
        180.,
        [(0., rgba(1., 0., 0., 0.)), (1., rgba(1., 0., 0., 0.9))],
    ))
    .at(80., 50.);
    scene_at(ground(vec![card]), scale)
}

/// [`mask`] with a six-stop ramp: more stops than one instance carries.
fn mask_ramp(scale: f64) -> ResolvedScene {
    let card = block(160., 100.)
        .radius(8.)
        .fill(rgba(0.9, 0.9, 0.9, 1.))
        .mask(Gradient::linear(
            90.,
            [
                (0., rgba(1., 0., 0., 0.9)),
                (0.2, rgba(1., 1., 0., 0.9)),
                (0.4, rgba(0., 1., 0., 0.9)),
                (0.6, rgba(0., 1., 1., 0.9)),
                (0.8, rgba(0., 0., 1., 0.9)),
                (1., rgba(1., 0., 1., 0.9)),
            ],
        ))
        .at(80., 50.);
    scene_at(ground(vec![card]), scale)
}

/// Frosted panels over stripes: a rounded one and a star, each blur radius
/// in turn, with a translucent tint.
fn backdrops(blur: f64) -> impl Fn(f64) -> ResolvedScene {
    move |scale| {
        let mut kids = busy();
        kids.push(
            block(120., 90.)
                .radius(16.)
                .fill(rgba(1., 1., 1., 0.2))
                .backdrop_blur(blur)
                .at(30., 50.),
        );
        kids.push(
            block(110., 110.)
                .outline(star(5))
                .fill(rgba(0., 0., 0., 0.15))
                .backdrop_blur(blur)
                .at(180., 40.),
        );
        scene_at(ground(kids), scale)
    }
}

/// A modal fading in over a blurred editor: its blur fades with it.
fn fading(scale: f64) -> ResolvedScene {
    let mut kids = busy();
    kids.push(
        stack![
            block(100., 20.)
                .radius(4.)
                .fill(rgba(0.95, 0.95, 0.95, 1.))
                .at(20., 20.)
        ]
        .size(160., 110.)
        .radius(14.)
        .fill(rgba(0.1, 0.1, 0.12, 0.4))
        .shadow(Shadow::soft(8.))
        .backdrop_blur(6.)
        .opacity(0.6)
        .at(80., 45.),
    );
    scene_at(ground(kids), scale)
}

fn gpu() -> Option<Gpu> {
    let (device, queue) = device()?;
    let quad = QuadRenderer::new(&device, mui_hal::TextureFormat::Rgba8Unorm);
    Some(Gpu {
        device,
        queue,
        quad,
    })
}

/// Per case, measured on RADV, worst over the three scales: every case has
/// rounded corners, which the pass covers as exact circles where
/// `vello_cpu` covers its flattened polygon (see `corners`); that is each
/// `max`. Beyond corners:
/// - opacity: the overlapping group composites its target once, as
///   `vello_cpu`'s layer does; the apart group folds its opacity into its
///   children. Interiors agree to a level.
/// - blends: `vello_cpu` mixes eight of the modes in u8 arithmetic, the
///   pass in f32 (Vello's GPU formulas); interiors differ by a level or two
///   (mean 0.30).
/// - nested: a clip's edge is multiplied into each instance in the layer,
///   as `vello_cpu`'s non-layer clip is.
/// - mask: the ramp is snapped to `vello_cpu`'s lookup table, drawn
///   source-atop with fixed-function blending; a level where u8 rounding
///   lands apart.
/// - backdrops: `vello_cpu` blurs the replay at full resolution with an
///   exact Gaussian; the pass averages it down (by 1, 2 and 4 for these
///   radii at 1x) and blurs that, so interiors differ by a level or two.
/// - fading: the same, through a group target at opacity 0.6.
#[test]
fn layers_match_vello_cpu() {
    let Some(mut gpu) = gpu() else {
        return;
    };
    let scope = gpu
        .device
        .push_error_scope(mui_hal::ErrorFilter::Validation);
    let mut failures = Vec::new();
    let f = &mut failures;
    compare(&mut gpu, f, "opacity", corners(25, 0.08), opacity);
    compare(&mut gpu, f, "blends", corners(32, 0.31), blends);
    compare(&mut gpu, f, "nested", corners(27, 0.24), nested);
    compare(&mut gpu, f, "mask", corners(49, 0.04), mask);
    // Mean 0.021 on Vulkan, 0.072 on DX12 WARP (same max): its gradient
    // interpolation rounds differently across the six-stop ramp.
    compare(&mut gpu, f, "mask ramp", corners(49, 0.08), mask_ramp);
    for (blur, limit) in [
        (2., corners(23, 0.09)),
        (6., corners(25, 0.21)),
        (12., corners(24, 0.24)),
    ] {
        compare(
            &mut gpu,
            f,
            &format!("backdrop{blur}"),
            limit,
            backdrops(blur),
        );
    }
    compare(&mut gpu, f, "fading", corners(28, 0.28), fading);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    if let Some(error) = pollster::block_on(scope.pop()) {
        panic!("wgpu validation: {error}");
    }
}

/// The pixel at (`x`, `y`) of an RGBA buffer `width` wide.
fn at(px: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let i = 4 * (y * width + x) as usize;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

/// Alpha masks reveal the original backdrop and ignore mask RGB on both renderers.
#[test]
fn alpha_mask_reveals_backdrop_without_tinting() {
    let Some(mut gpu) = gpu() else {
        return;
    };
    let size = sized(1.);
    for alpha in [0., 0.5, 1.] {
        let mut colours = Vec::new();
        for ink in [0., 1.] {
            let root = stack![
                stack![block(160., 120.).radius(0.).fill(rgba(1., 0., 0., 1.))]
                    .size(100., 100.)
                    .radius(0.)
                    .alpha_mask(rgba(ink, ink, ink, alpha))
                    .at(50., 50.)
            ]
            .size(W, H)
            .radius(0.)
            .fill(rgba(0., 0., 1., 1.));
            let scene = scene_at(root, 1.);
            let pixels = [
                cpu_pixels(&scene, Affine::IDENTITY, size),
                gpu_pixels(&mut gpu, &scene.paint, Affine::IDENTITY, size),
            ];
            for (backend, px) in pixels.iter().enumerate() {
                let got = at(px, size[0], 75, 75);
                let expected = [alpha * 255., 0., (1. - alpha) * 255., 255.];
                for (got, expected) in got.into_iter().zip(expected) {
                    assert!(
                        (f32::from(got) - expected).abs() <= 1.,
                        "backend {backend}, alpha {alpha}, mask RGB {ink}: {got} vs {expected}"
                    );
                }
                assert_eq!(
                    at(px, size[0], 175, 75),
                    [0, 0, 255, 255],
                    "overflow must be clipped before masking on backend {backend}"
                );
            }
            colours.push(pixels);
        }
        assert_eq!(
            colours[0], colours[1],
            "mask RGB must not tint, alpha {alpha}"
        );
    }
    let ramp = scene_at(
        stack![
            block(100., 100.)
                .radius(0.)
                .fill(rgba(1., 0., 0., 1.))
                .alpha_mask(Gradient::linear(
                    180.,
                    [(0., rgba(0., 0., 0., 1.)), (1., rgba(0., 0., 0., 0.))]
                ))
                .at(50., 50.)
        ]
        .size(W, H)
        .radius(0.)
        .fill(rgba(0., 0., 1., 1.)),
        1.,
    );
    for px in [
        cpu_pixels(&ramp, Affine::IDENTITY, size),
        gpu_pixels(&mut gpu, &ramp.paint, Affine::IDENTITY, size),
    ] {
        let top = at(&px, size[0], 100, 55);
        let middle = at(&px, size[0], 100, 100);
        let bottom = at(&px, size[0], 100, 145);
        assert!(
            top[0] > middle[0] && middle[0] > bottom[0],
            "smooth alpha fade"
        );
        assert!(
            top[2] < middle[2] && middle[2] < bottom[2],
            "backdrop revealed"
        );
        assert_eq!(middle[1], 0, "mask must not introduce another colour");
    }
    let texture = gpu.device.create_texture(&mui_hal::TextureDescriptor {
        label: Some("retained alpha mask"),
        size: mui_hal::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: mui_hal::TextureDimension::D2,
        format: mui_hal::TextureFormat::Rgba8Unorm,
        usage: mui_hal::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&mui_hal::TextureViewDescriptor::default());
    for frame in 0..2 {
        gpu.quad
            .render(
                &gpu.device,
                &gpu.queue,
                &ramp.paint,
                Affine::IDENTITY,
                &view,
                size,
            )
            .expect("retained alpha mask renders");
        if frame == 0 {
            assert!(gpu.quad.frame().passes > 0, "the mask was rendered");
        } else {
            assert_eq!(
                gpu.quad.frame().passes,
                0,
                "unchanged mask must reuse its target"
            );
        }
    }
}

/// Every long-ramp span is opaque, so the source must survive all instances.
#[test]
fn alpha_masks_with_long_ramps_preserve_opaque_and_transparent_samples() {
    let Some(mut gpu) = gpu() else {
        return;
    };
    let size = sized(1.);
    for alphas in [[1., 1., 1., 1., 1.], [1., 0.5, 0., 0.5, 1.]] {
        for angle in [90., 180.] {
            let stops = alphas
                .into_iter()
                .enumerate()
                .map(|(i, a)| (i as f32 / 4., rgba(0., 0., 0., a)));
            let scene = scene_at(
                stack![
                    block(100., 100.)
                        .radius(0.)
                        .fill(rgba(1., 0., 0., 1.))
                        .alpha_mask(Gradient::linear(angle, stops))
                        .at(50., 50.)
                ]
                .size(W, H)
                .radius(0.)
                .fill(rgba(0., 0., 1., 1.)),
                1.,
            );
            let cpu = cpu_pixels(&scene, Affine::IDENTITY, size);
            let quad = gpu_pixels(&mut gpu, &scene.paint, Affine::IDENTITY, size);
            for y in 51..149 {
                for x in 51..149 {
                    let (a, b) = (at(&cpu, size[0], x, y), at(&quad, size[0], x, y));
                    assert!(
                        a.into_iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 2),
                        "long alpha ramp at {x},{y}, angle {angle}: CPU {a:?}, quad {b:?}"
                    );
                    if alphas == [1.; 5] {
                        assert_eq!(b, [255, 0, 0, 255]);
                    }
                }
            }
        }
    }
}

/// A node's own paint is clipped as well as its children.
#[test]
fn alpha_mask_clips_own_canvas_and_shadow() {
    let Some(mut gpu) = gpu() else {
        return;
    };
    let size = sized(1.);
    for alpha in [0., 1.] {
        let scene = scene_at(
            stack![
                canvas(|_| vec![Draw::fill(
                    Path::polyline(
                        [
                            Point::ZERO,
                            Point::new(80., 0.),
                            Point::new(80., 40.),
                            Point::new(0., 40.)
                        ],
                        true
                    ),
                    rgba(1., 0., 0., 1.)
                )])
                .size(40., 40.)
                .radius(0.)
                .shadow(Shadow::soft(8.))
                .alpha_mask(rgba(0., 0., 0., alpha))
                .at(50., 50.)
            ]
            .size(W, H)
            .radius(0.)
            .fill(rgba(0., 0., 1., 1.)),
            1.,
        );
        for (backend, px) in [
            cpu_pixels(&scene, Affine::IDENTITY, size),
            gpu_pixels(&mut gpu, &scene.paint, Affine::IDENTITY, size),
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(
                at(px, size[0], 65, 65),
                if alpha == 0. {
                    [0, 0, 255, 255]
                } else {
                    [255, 0, 0, 255]
                },
                "backend {backend}"
            );
            for (x, y) in [(45, 65), (100, 65), (65, 95)] {
                assert_eq!(
                    at(px, size[0], x, y),
                    [0, 0, 255, 255],
                    "canvas/shadow outside masked outline on backend {backend}, {x},{y}"
                );
            }
        }
    }
}

/// No reference needed: a blurred flat colour is that colour, at every
/// radius and scale; half of opaque white over black is mid grey whether
/// the group folds (one child) or takes a target (two overlapping); a
/// group at opacity 0 draws nothing.
#[test]
fn known_answers() {
    let Some(mut gpu) = gpu() else {
        return;
    };
    let flat = rgba(0.3, 0.55, 0.8, 1.);
    for blur in [1., 5., 20.] {
        for scale in SCALES {
            let root = stack![
                block(140., 90.)
                    .radius(12.)
                    .fill(rgba(0., 0., 0., 0.))
                    .backdrop_blur(blur)
                    .at(90., 50.)
            ]
            .size(W, H)
            .radius(0.)
            .fill(flat);
            let scene = scene_at(root, scale);
            let size = sized(scale);
            let px = gpu_pixels(&mut gpu, &scene.paint, Affine::scale(scale), size);
            let want = at(&px, size[0], 2, 2);
            for y in (0..size[1]).step_by(3) {
                for x in (0..size[0]).step_by(3) {
                    let got = at(&px, size[0], x, y);
                    let off = got.iter().zip(want).map(|(a, b)| a.abs_diff(b)).max();
                    assert!(
                        off <= Some(1),
                        "blur {blur} at {scale}x, ({x}, {y}): {got:?} vs {want:?}"
                    );
                }
            }
        }
    }
    let white = rgba(1., 1., 1., 1.);
    let half = |kids: Vec<El>, opacity: f32| {
        let root = stack![stack(kids).size(100., 100.).opacity(opacity).at(50., 50.)]
            .size(W, H)
            .radius(0.)
            .fill(rgba(0., 0., 0., 1.));
        scene_at(root, 1.)
    };
    let one = vec![block(100., 100.).radius(0.).fill(white)];
    let two = vec![
        block(100., 100.).radius(0.).fill(white),
        block(50., 50.).radius(0.).fill(white).at(25., 25.),
    ];
    let size = sized(1.);
    // Folded: the frame's pass alone. Isolated: the group's target first.
    for (name, kids, passes) in [("folded", one, 1), ("isolated", two, 2)] {
        let px = gpu_pixels(&mut gpu, &half(kids, 0.5).paint, Affine::IDENTITY, size);
        for (x, y) in [(60, 60), (100, 100), (140, 140)] {
            let got = at(&px, size[0], x, y);
            assert!(
                (127..=128).contains(&got[0])
                    && got[0] == got[1]
                    && got[1] == got[2]
                    && got[3] == 255,
                "{name} at ({x}, {y}): {got:?}"
            );
        }
        assert_eq!(
            gpu.quad.frame().passes,
            passes,
            "{name}: {:?}",
            gpu.quad.frame()
        );
    }
    let gone = half(vec![block(100., 100.).radius(0.).fill(white)], 0.);
    let px = gpu_pixels(&mut gpu, &gone.paint, Affine::IDENTITY, size);
    assert_eq!(at(&px, size[0], 100, 100), [0, 0, 0, 255]);
    assert_eq!(gpu.quad.frame().passes, 1, "{:?}", gpu.quad.frame());
}

/// The layer entries these scenes produce are the ones under test.
#[test]
fn the_scenes_hold_the_layers_under_test() {
    let count =
        |s: &ResolvedScene, f: fn(&Layer) -> bool| s.paint.iter().filter(|p| f(&p.layer)).count();
    assert_eq!(count(&opacity(1.), |l| matches!(l, Layer::Blend { .. })), 2);
    assert_eq!(count(&blends(1.), |l| matches!(l, Layer::Blend { .. })), 15);
    assert_eq!(count(&mask(1.), |l| *l == Layer::Mask), 1);
    assert_eq!(count(&backdrops(4.)(1.), |l| *l == Layer::Backdrop), 2);
    assert_eq!(count(&fading(1.), |l| *l == Layer::Backdrop), 1);
}
