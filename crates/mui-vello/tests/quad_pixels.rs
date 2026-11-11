//! The quad pass against `vello_cpu`: the same paint list through
//! [`mui_vello::software::Renderer`] and through [`QuadRenderer`], read back
//! and compared pixel by pixel, at device scales 1, 1.5 and 2.
//!
//! The device is opened with WebGL2's downlevel limits, so the pipeline is
//! validated against them too. Skips, loudly, when the machine has no wgpu
//! adapter at all. On failure the CPU image, the GPU image and a diff are
//! written to a temp directory whose path the failure prints.
#![cfg(all(feature = "gpu-quad", feature = "cpu"))]

use mui_scene::prelude::*;
use mui_scene::{Layer, ResolvedScene, ShadowKind};
use mui_vello::kurbo::Affine;
use mui_vello::quad::{Error, QuadRenderer, Reason};

mod quad_support;
use quad_support::*;

/// No curve anywhere: square fills, strokes with square joins, gradients,
/// shadows of square boxes, square clips and translucent overlap. What is
/// left between the two rasterisers here is the pass itself.
fn square(scale: f64) -> ResolvedScene {
    let (red, blue) = (Color::srgb(0.95, 0.2, 0.1), Color::srgb(0.1, 0.3, 0.95));
    let fills: Vec<Fill> = vec![
        Color::srgb(0.9, 0.3, 0.2).into(),
        Gradient::linear(90., [(0., red), (1., blue)]).into(),
        Gradient::linear(
            30.,
            [(0., red), (0.4, rgba(0.2, 0.9, 0.3, 0.5)), (1., blue)],
        )
        .into(),
        Gradient::radial((0.4, 0.6), 0.7, [(0., red), (1., blue)]).into(),
        Gradient::conic(20., [(0., red), (0.5, blue), (1., red)]).into(),
    ];
    let mut kids: Vec<El> = fills
        .into_iter()
        .enumerate()
        .map(|(i, fill)| {
            block(52., 40.)
                .radius(0.)
                .fill(fill)
                .shadow(Shadow {
                    blur: 2. + 3. * i as f64,
                    dx: 2.,
                    dy: 3.,
                    spread: 0.,
                    kind: ShadowKind::Drop,
                    fill: Fill::Color(rgba(0., 0., 0., 0.5)),
                })
                .at(10. + 60. * i as f64, 10.)
        })
        .collect();
    kids.push(
        block(80., 50.)
            .radius(0.)
            .fill(rgba(0.85, 0.87, 0.9, 0.8))
            .shadow(Shadow::inset(6.))
            .at(14., 76.),
    );
    for (i, c) in [
        rgba(1., 0.2, 0.1, 0.5),
        rgba(0.1, 0.9, 0.3, 0.4),
        rgba(0.2, 0.3, 1., 0.65),
    ]
    .into_iter()
    .enumerate()
    {
        kids.push(
            block(70., 50.)
                .radius(0.)
                .fill(c)
                .at(110. + 25. * i as f64, 70. + 15. * i as f64),
        );
    }
    kids.push(
        stack![
            stack![
                block(200., 30.)
                    .radius(0.)
                    .fill(Color::srgb(0.9, 0.8, 0.2))
                    .at(-30., 10.)
            ]
            .size(60., 50.)
            .radius(0.)
            .clip()
            .at(10., 10.)
        ]
        .size(70., 80.)
        .radius(0.)
        .clip()
        .at(235., 75.),
    );
    scene_at(ground(kids), scale)
}

fn rounded(scale: f64) -> ResolvedScene {
    let mut kids = Vec::new();
    for (i, r) in [0., 2.5, 6., 13.].into_iter().enumerate() {
        let x = 12. + 62. * i as f64;
        kids.push(
            block(50., 34.)
                .radius(r)
                .fill(Color::srgb(0.9, 0.3, 0.2))
                .at(x, 10.),
        );
        // Strokes, including one wider than twice its radius.
        for (j, w) in [1., 2.5, 7.].into_iter().enumerate() {
            kids.push(
                block(50., 30.)
                    .radius(r)
                    .no_fill()
                    .stroke(Color::srgb(0.3, 0.8, 1.))
                    .stroke_width(w)
                    .at(x + 0.3 * j as f64, 54. + 42. * j as f64),
            );
        }
    }
    kids.push(
        block(56., 28.)
            .pill()
            .fill(Color::srgb(0.95, 0.85, 0.2))
            .at(258., 12.),
    );
    kids.push(
        block(28., 70.)
            .pill()
            .fill(Color::srgb(0.2, 0.9, 0.5))
            .stroke(Color::srgb(1., 1., 1.))
            .stroke_width(2.)
            .at(272., 60.),
    );
    scene_at(ground(kids), scale)
}

/// Ramps of more than four stops, one instance per span: a hue wheel's
/// seven, hard stops, a ramp starting late and ending early, translucent
/// stops, radial ones, and the same on a stroke and in a faded layer.
fn ramps(scale: f64) -> ResolvedScene {
    let hue = |n: usize| {
        (0..n).map(move |i| {
            let t = i as f32 / (n - 1) as f32;
            let c = |o: f32| 0.5 + 0.5 * (6.283 * (t + o)).cos();
            (t, Color::srgb(c(0.), c(0.33), c(0.67)))
        })
    };
    let (red, blue) = (Color::srgb(0.95, 0.2, 0.1), Color::srgb(0.1, 0.3, 0.95));
    let green = Color::srgb(0.2, 0.9, 0.3);
    let fills: Vec<Fill> = vec![
        Gradient::linear(90., hue(7)).into(),
        Gradient::linear(
            30.,
            [
                (0., red),
                (0.4, red),
                (0.4, green),
                (0.6, green),
                (0.6, blue),
                (1., blue),
            ],
        )
        .into(),
        Gradient::linear(
            200.,
            [
                (0.15, red),
                (0.3, green),
                (0.5, rgba(0.1, 0.3, 0.95, 0.4)),
                (0.7, red),
                (0.85, blue),
            ],
        )
        .into(),
        Gradient::radial((0.4, 0.6), 0.7, hue(6)).into(),
        Gradient::radial(
            (0.5, 0.5),
            0.5,
            [
                (0., rgba(1., 1., 1., 0.)),
                (0.2, red),
                (0.4, rgba(0., 0., 0., 0.3)),
                (0.6, green),
                (0.8, blue),
                (1., rgba(0., 0., 0., 0.)),
            ],
        )
        .into(),
    ];
    let mut kids: Vec<El> = fills
        .into_iter()
        .enumerate()
        .map(|(i, fill)| {
            let (x, y) = (12. + 76. * (i % 4) as f64, 14. + 92. * (i / 4) as f64);
            block(66., 78.).radius(9.).fill(fill).at(x, y)
        })
        .collect();
    // No conic: a span picks its colour the same way for every kind, and
    // the pixels next to a conic's centre differ by up to 204 (vello_cpu's
    // angle there), which would hide what this case measures.
    kids.push(
        block(60., 70.)
            .radius(14.)
            .stroke(Gradient::linear(0., hue(8)))
            .stroke_width(5.)
            .at(240., 110.),
    );
    kids.push(
        stack![
            block(60., 70.)
                .radius(6.)
                .fill(Gradient::linear(135., hue(5)))
        ]
        .opacity(0.6)
        .at(91., 110.),
    );
    scene_at(ground(kids), scale)
}

fn gradients(scale: f64) -> ResolvedScene {
    let (red, blue) = (Color::srgb(0.95, 0.2, 0.1), Color::srgb(0.1, 0.3, 0.95));
    let fills: Vec<Fill> = vec![
        Gradient::linear(90., [(0., red), (1., blue)]).into(),
        Gradient::linear(
            45.,
            [(0., red), (0.5, Color::srgb(0.2, 0.9, 0.3)), (1., blue)],
        )
        .into(),
        Gradient::linear(180., [(0.2, red), (0.8, blue)]).into(),
        Gradient::linear(
            300.,
            [
                (0., rgba(1., 1., 1., 0.)),
                (0.3, red),
                (0.6, rgba(0.1, 0.3, 0.95, 0.5)),
                (1., Color::srgb(0.9, 0.9, 0.2)),
            ],
        )
        .into(),
        Gradient::radial((0.5, 0.5), 0.5, [(0., red), (1., blue)]).into(),
        Gradient::radial(
            (0.3, 0.7),
            0.8,
            [(0., rgba(1., 1., 1., 0.9)), (1., rgba(0., 0., 0., 0.))],
        )
        .into(),
        Gradient::conic(0., [(0., red), (1., blue)]).into(),
        Gradient::conic(
            30.,
            [(0., red), (0.5, Color::srgb(0.2, 0.9, 0.3)), (1., blue)],
        )
        .into(),
    ];
    let kids = fills
        .into_iter()
        .enumerate()
        .map(|(i, fill)| {
            let (x, y) = (12. + 76. * (i % 4) as f64, 14. + 92. * (i / 4) as f64);
            block(66., 78.).radius(9.).fill(fill).at(x, y)
        })
        .collect();
    scene_at(ground(kids), scale)
}

fn shadows(scale: f64) -> ResolvedScene {
    let card = |blur: f64, dx: f64, dy: f64, spread: f64| {
        block(64., 44.)
            .radius(10.)
            .fill(Color::srgb(0.85, 0.87, 0.9))
            .shadow(Shadow {
                blur,
                dx,
                dy,
                spread,
                kind: ShadowKind::Drop,
                fill: Fill::Color(rgba(0., 0., 0., 0.6)),
            })
    };
    let recess = |blur: f64, dx: f64, dy: f64| {
        block(64., 44.)
            .radius(12.)
            .fill(Color::srgb(0.85, 0.87, 0.9))
            .shadow(Shadow {
                blur,
                dx,
                dy,
                spread: 0.,
                kind: ShadowKind::Inset,
                fill: Fill::Color(rgba(0.1, 0., 0.2, 0.7)),
            })
    };
    let kids = vec![
        card(2., 0., 1., 0.).at(20., 16.),
        card(8., 3., 4., 0.).at(110., 16.),
        card(20., -4., 8., 2.).at(210., 16.),
        recess(3., 0., 2.).at(20., 116.),
        recess(10., 4., -3.).at(110., 116.),
        recess(6., 0., 0.).radius(0.).at(210., 116.),
    ];
    scene_at(ground(kids), scale)
}

fn clips(scale: f64) -> ResolvedScene {
    let bar = |y: f64, c: Color| block(300., 22.).radius(6.).fill(c).at(-40., y);
    let inner = stack![
        bar(4., Color::srgb(0.9, 0.3, 0.2)),
        bar(30., Color::srgb(0.2, 0.8, 0.4)),
        bar(56., Color::srgb(0.2, 0.4, 0.95)),
    ]
    .size(150., 90.)
    .radius(18.)
    .clip()
    .at(-10., 10.);
    let middle = stack![inner]
        .size(170., 120.)
        .radius(0.)
        .clip()
        .at(20.5, 15.25);
    let outer = stack![middle]
        .size(150., 150.)
        .radius(0.)
        .clip()
        .at(30., 20.);
    // A square clip inside a rounded one, the other way round.
    let rounded_outside = stack![
        stack![
            bar(10., Color::srgb(0.9, 0.8, 0.2)),
            bar(40., Color::srgb(0.8, 0.2, 0.9))
        ]
        .size(60., 60.)
        .radius(0.)
        .clip()
        .at(-10., 0.)
    ]
    .size(80., 80.)
    .radius(24.)
    .clip()
    .at(210., 40.);
    scene_at(ground(vec![outer, rounded_outside]), scale)
}

/// Overlapping translucent fills: order and blending.
fn overlap(scale: f64) -> ResolvedScene {
    let kids: Vec<El> = [
        (rgba(1., 0.2, 0.1, 0.5), 30., 30.),
        (rgba(0.1, 0.9, 0.3, 0.4), 80., 60.),
        (rgba(0.2, 0.3, 1., 0.65), 130., 40.),
        (rgba(1., 1., 1., 0.25), 60., 90.),
    ]
    .into_iter()
    .map(|(c, x, y)| block(120., 80.).radius(16.).fill(c).at(x, y))
    .collect();
    // The ground itself translucent, so the target keeps alpha.
    let root = stack(kids)
        .size(W, H)
        .radius(0.)
        .fill(rgba(0.5, 0.5, 0.6, 0.3));
    scene_at(root, scale)
}

/// A rotated subtree: its fills, strokes, gradients, shadows and rounded
/// clip turn together.
fn rotated(scale: f64) -> ResolvedScene {
    let panel = stack![
        block(90., 30.)
            .radius(8.)
            .fill(Gradient::linear(
                90.,
                [
                    (0., Color::srgb(0.9, 0.2, 0.2)),
                    (1., Color::srgb(0.2, 0.2, 0.9))
                ]
            ))
            .at(10., 10.),
        block(60., 40.)
            .radius(0.)
            .fill(Color::srgb(0.2, 0.8, 0.4))
            .stroke(Color::srgb(1., 1., 1.))
            .stroke_width(2.)
            .at(20., 50.),
        block(40., 40.)
            .radius(12.)
            .fill(Color::srgb(0.9, 0.8, 0.3))
            .shadow(Shadow::soft(6.))
            .at(100., 50.),
    ]
    .size(150., 100.)
    .radius(14.)
    .fill(Color::srgb(0.3, 0.32, 0.36))
    .clip()
    .rotation(0.35)
    .at(70., 40.);
    scene_at(ground(vec![panel]), scale)
}

/// A subtree under a non-integer scale and a fractional translation, set by
/// hand: the DSL has no subtree scale.
fn scaled(scale: f64) -> ResolvedScene {
    let mut scene = scene_at(
        ground(vec![
            block(70., 40.)
                .radius(9.)
                .fill(Color::srgb(0.9, 0.3, 0.2))
                .stroke(Color::srgb(1., 1., 1.))
                .stroke_width(1.5)
                .shadow(Shadow::soft(5.))
                .id("a")
                .at(20., 20.),
            block(60., 60.)
                .radius(30.)
                .fill(Gradient::radial(
                    (0.5, 0.5),
                    0.5,
                    [
                        (0., Color::srgb(1., 1., 1.)),
                        (1., Color::srgb(0.1, 0.4, 0.9)),
                    ],
                ))
                .id("b")
                .at(110., 30.),
        ]),
        scale,
    );
    let t = Affine::translate((7.3, 4.6)) * Affine::scale_non_uniform(1.37, 1.21);
    for p in &mut scene.paint {
        if matches!(p.key.as_ref(), "a" | "b") {
            p.transform = t * p.transform;
        }
    }
    scene
}

#[test]
fn the_quad_pass_matches_vello_cpu() {
    let Some((device, queue)) = device() else {
        return;
    };
    let quad = QuadRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    let mut gpu = Gpu {
        device,
        queue,
        quad,
    };
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut failures = Vec::new();
    let f = &mut failures;
    compare(&mut gpu, f, "square", TIGHT, square);
    compare(&mut gpu, f, "rounded", corners(60, 0.23), rounded);
    compare(&mut gpu, f, "gradients", corners(53, 0.11), gradients);
    // Measured 54 / 0.130: rounded corners' antialiasing, as in
    // `gradients`; inside the shapes the spans match vello_cpu exactly.
    compare(&mut gpu, f, "ramps", corners(55, 0.14), ramps);
    compare(&mut gpu, f, "shadows", corners(51, 0.15), shadows);
    compare(&mut gpu, f, "clips", corners(45, 0.005), clips);
    compare(&mut gpu, f, "overlap", corners(37, 0.18), overlap);
    compare(&mut gpu, f, "rotated", corners(49, 0.03), rotated);
    compare(&mut gpu, f, "scaled", corners(80, 0.09), scaled);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    if let Some(error) = pollster::block_on(scope.pop()) {
        panic!("wgpu validation: {error}");
    }
}

#[test]
fn an_unsupported_entry_is_refused_by_index_and_reason() {
    let Some((device, queue)) = device() else {
        return;
    };
    let mut scene = rounded(1.);
    let at = scene
        .paint
        .iter()
        .position(|p| p.layer == Layer::Fill)
        .expect("a fill");
    // Drawn from its path from now on; a path the geometry kernel refuses
    // (a line with no start) is not.
    scene.paint[at].rect = None;
    scene.paint[at].path = std::sync::Arc::new(mui_geometry::Path {
        commands: vec![mui_geometry::PathCommand::LineTo(Point::new(1., 1.))],
    });
    let mut quad = QuadRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 8,
            height: 8,
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
    let error = quad
        .render(
            &device,
            &queue,
            &scene.paint,
            Affine::IDENTITY,
            &view,
            [8, 8],
        )
        .expect_err("refused");
    assert_eq!(
        error,
        Error::Unsupported {
            index: at,
            reason: Reason::Path
        }
    );
}
