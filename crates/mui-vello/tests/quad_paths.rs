//! Arbitrary paths through the quad pass: fills, strokes, clips and shadows
//! of outlines that are not rounded rects, against `vello_cpu` at device
//! scales 1, 1.5 and 2, plus checks that need no reference (no seam where a
//! path meets a rect, nothing in a ring's hole).
//!
//! `vello_cpu` is a sanity check here, not a target. It flattens curves and
//! round joins at 0.25 px, the pass at 0.1 px and its joins and caps are
//! exact circles, so edge pixels of curved outlines differ by up to the area
//! between chord and arc; each case says what else differs.
#![cfg(all(feature = "gpu-quad", feature = "cpu"))]

mod quad_support;
use quad_support::*;

use mui_material::Material;
use mui_scene::prelude::*;
use mui_scene::{Font, ResolvedScene, ShadowKind};
use mui_vello::kurbo::Affine;
use mui_vello::quad::QuadRenderer;
use std::f64::consts::PI;

/// A knob's arcs, as the knob widget draws them: 48-segment polylines
/// stroked with round joins and caps, a translucent track under the value.
fn arc(c: Point, r: f64, from: f64, sweep: f64) -> Path {
    Path::polyline(
        (0..=48).map(|i| {
            let a = from + sweep * f64::from(i) / 48.;
            Point::new(c.x + a.cos() * r, c.y + a.sin() * r)
        }),
        false,
    )
}

fn knobs(scale: f64) -> ResolvedScene {
    let kids = [(0.1, 1.), (0.45, 2.), (0.8, 3.5), (1., 6.)]
        .into_iter()
        .enumerate()
        .map(|(i, (amount, width))| {
            canvas(move |s: Size| {
                let c = Point::new(s.width / 2., s.height / 2.);
                let r = s.width * 0.38;
                let disc = Path::polyline(
                    (0..64).map(|k| {
                        let a = 2. * PI * f64::from(k) / 64.;
                        Point::new(c.x + a.cos() * r * 0.7, c.y + a.sin() * r * 0.7)
                    }),
                    true,
                );
                vec![
                    Draw::stroke(arc(c, r, 0.75 * PI, 1.5 * PI), rgba(1., 1., 1., 0.15), width),
                    Draw::stroke(
                        arc(c, r, 0.75 * PI, 1.5 * PI * amount),
                        rgba(0.3, 0.8, 1., 0.85),
                        width,
                    ),
                    Draw::fill(disc, Color::srgb(0.25, 0.27, 0.3)),
                ]
            })
            .size(70., 70.)
            .at(8. + 76. * i as f64, 20.)
        })
        .collect();
    scene_at(ground(kids), scale)
}

/// A welded body: the union of three panels, concave fillets where they
/// meet, filled, stroked and shelled along the union.
fn union_body(scale: f64) -> ResolvedScene {
    let body = stack![
        block(120., 60.).at(10., 10.),
        block(60., 120.).at(90., 40.),
        block(140., 40.).at(130., 120.),
    ]
    .size(290., 180.)
    .radius(14.)
    .union(Color::srgb(0.3, 0.33, 0.38))
    .shell(4., rgba(0.9, 0.5, 0.2, 0.6))
    .stroke(Color::srgb(0.9, 0.9, 0.95))
    .stroke_width(1.5)
    .at(10., 8.);
    scene_at(ground(vec![body]), scale)
}

/// A carved ring and a panel with a hole cut out of it.
fn carved(scale: f64) -> ResolvedScene {
    let kids = vec![
        block(90., 90.)
            .pill()
            .fill(Color::srgb(0.9, 0.4, 0.2))
            .cut(block(50., 50.).pill().at(20., 20.))
            .at(20., 30.),
        block(150., 110.)
            .radius(12.)
            .fill(Color::srgb(0.2, 0.6, 0.9))
            .cut(block(60., 40.).radius(8.).at(30., 30.))
            .cut(block(30., 30.).pill().at(105., 60.))
            .at(140., 40.),
    ];
    scene_at(ground(kids), scale)
}

/// Open polylines with sharp and shallow turns: round joins and caps, the
/// only ones a scene asks for (`one` strokes with `Stroke::new`).
fn polylines(scale: f64) -> ResolvedScene {
    let line = |pts: Vec<(f64, f64)>, w: f64, c: Color| {
        canvas(move |_| {
            vec![Draw::stroke(
                Path::polyline(pts.iter().map(|&(x, y)| Point::new(x, y)), false),
                c,
                w,
            )]
        })
        .size(W, H)
    };
    let zig: Vec<(f64, f64)> = (0..12)
        .map(|i| (20. + 22. * f64::from(i), if i % 2 == 0 { 30. } else { 70. }))
        .collect();
    let wave: Vec<(f64, f64)> = (0..200)
        .map(|i| {
            let x = 10. + 1.5 * f64::from(i);
            (x, 130. + 30. * (f64::from(i) * 0.15).sin() * (f64::from(i) * 0.013).cos())
        })
        .collect();
    let kids = vec![
        line(zig, 5., rgba(1., 0.8, 0.2, 0.7)),
        line(wave, 1.25, Color::srgb(0.4, 1., 0.6)),
        line(vec![(40., 175.), (120., 180.), (60., 186.)], 3., Color::srgb(1., 0.4, 0.5)),
        line(vec![(200., 175.), (300., 175.)], 0.6, Color::srgb(1., 1., 1.)),
    ];
    scene_at(ground(kids), scale)
}

fn star(s: Size) -> Path {
    let c = Point::new(s.width / 2., s.height / 2.);
    let (r, inner) = (s.width.min(s.height) / 2., s.width.min(s.height) / 4.6);
    let pts = (0..10).map(|i| {
        let a = PI * f64::from(i) / 5. - PI / 2.;
        let r = if i % 2 == 0 { r } else { inner };
        Point::new(c.x + a.cos() * r, c.y + a.sin() * r)
    });
    Path::polyline(pts, true)
}

/// Gradients on a path: linear, radial and conic, fitted to its box.
fn gradients(scale: f64) -> ResolvedScene {
    let (red, blue) = (Color::srgb(0.95, 0.2, 0.1), Color::srgb(0.1, 0.3, 0.95));
    let fills: Vec<Fill> = vec![
        Gradient::linear(90., [(0., red), (1., blue)]).into(),
        Gradient::radial((0.5, 0.5), 0.6, [(0., Color::srgb(1., 1., 0.8)), (1., blue)]).into(),
        Gradient::conic(30., [(0., red), (0.5, Color::srgb(0.2, 0.9, 0.3)), (1., red)]).into(),
    ];
    let kids = fills
        .into_iter()
        .enumerate()
        .map(|(i, f)| block(90., 90.).outline(star).fill(f).at(12. + 100. * i as f64, 50.))
        .collect();
    scene_at(ground(kids), scale)
}

static HACK: std::sync::LazyLock<Font> =
    std::sync::LazyLock::new(|| Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());

/// A star clipping rects, a rounded rect and text inside it.
fn clipped(scale: f64) -> ResolvedScene {
    let inside = stack![
        block(300., 30.).radius(0.).fill(Color::srgb(0.9, 0.3, 0.2)).at(-20., 20.),
        block(80., 80.).radius(16.).fill(rgba(0.2, 0.8, 0.4, 0.8)).at(40., 50.),
        text("Cutoff 440 Hz").text_size(16.).fill(Color::srgb(1., 1., 1.)).at(30., 95.),
        // A rounded clip inside the path clip.
        stack![block(200., 20.).fill(Color::srgb(0.9, 0.9, 0.2))]
            .size(60., 40.)
            .radius(12.)
            .clip()
            .at(110., 120.),
    ]
    .size(180., 180.)
    .outline(star)
    .fill(Color::srgb(0.15, 0.2, 0.35))
    .clip()
    .at(70., 10.);
    let mut spec = SceneSpec::new(ground(vec![inside]))
        .offered(Size::new(W, H))
        .scale(scale);
    spec.font = Some(HACK.clone());
    mui_scene::resolve(&spec).expect("resolves")
}

/// Drop shadows of custom outlines.
fn shadows(scale: f64) -> ResolvedScene {
    let kids = [3., 8.]
        .into_iter()
        .enumerate()
        .map(|(i, blur)| {
            block(100., 100.)
                .outline(star)
                .fill(Color::srgb(0.85, 0.87, 0.9))
                .shadow(Shadow {
                    blur,
                    dx: 4.,
                    dy: 6.,
                    spread: 0.,
                    kind: ShadowKind::Drop,
                    fill: Fill::Color(rgba(0., 0., 0., 0.7)),
                })
                .at(30. + 150. * i as f64, 40.)
        })
        .collect();
    scene_at(ground(kids), scale)
}

/// A rotated subtree of paths, and a subtree under a fractional,
/// non-uniform scale set by hand (the DSL has no subtree scale).
fn turned(scale: f64) -> ResolvedScene {
    let mut scene = scene_at(
        ground(vec![
            stack![
                block(70., 70.).outline(star).fill(Color::srgb(0.9, 0.6, 0.2)).at(10., 10.),
                canvas(|s: Size| {
                    let c = Point::new(s.width / 2., s.height / 2.);
                    vec![Draw::stroke(arc(c, 25., 0.75 * PI, 1.2 * PI), rgba(0.3, 0.8, 1., 0.9), 3.)]
                })
                .size(70., 70.)
                .at(80., 10.),
            ]
            .size(160., 90.)
            .rotation(0.4)
            .at(20., 40.),
            block(60., 60.)
                .outline(star)
                .fill(Color::srgb(0.3, 0.9, 0.5))
                .stroke(Color::srgb(1., 1., 1.))
                .stroke_width(1.5)
                .id("scaled")
                .at(190., 40.),
        ]),
        scale,
    );
    let t = Affine::translate((7.3, 4.6)) * Affine::scale_non_uniform(1.37, 1.21);
    for p in &mut scene.paint {
        if p.key.as_ref().ends_with("scaled") {
            p.transform = t * p.transform;
        }
    }
    scene
}

fn gpu() -> Option<Gpu> {
    let (device, queue) = device()?;
    let quad = QuadRenderer::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    Some(Gpu {
        device,
        queue,
        quad,
    })
}

#[test]
fn paths_match_vello_cpu() {
    let Some(mut gpu) = gpu() else {
        return;
    };
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut failures = Vec::new();
    let f = &mut failures;
    let g = &mut gpu;
    // Each limit is the case's measured worst over the three scales on
    // RADV, rounded up; means likewise.
    //
    // Knob arcs: polylines, so only the round joins and caps are curves.
    // The pass's are circles; `vello_cpu`'s are kurbo's flattened offset
    // arcs, which sit up to a quarter pixel off (its caps are not even
    // symmetric about the segment). Measured 50.
    compare(g, f, "path-knobs", corners(52, 0.03), knobs);
    // A welded union: a fill of fillets and arcs (flattened at 0.1 px here,
    // 0.25 px there), a shell ring and a stroke with round joins. Measured
    // 50, on the stroke's curved stretches.
    compare(g, f, "path-union", corners(52, 0.08), union_body);
    // Fills only: both take exact box-filter area of nearly the same
    // polygons. Measured 1.
    compare(g, f, "path-carved", TIGHT, carved);
    // Round joins and caps at sharp turns, and a hairpin; the worst pixels
    // sit on cap tips, where the pass's circle is the closer one (checked
    // by hand: 0.45 of a pixel against an exact 0.42, `vello_cpu` 0.27).
    // Measured 112.
    compare(g, f, "path-polylines", corners(114, 0.04), polylines);
    // Gradients on star fills: the fill and the ramp both match. Measured 1.
    compare(g, f, "path-gradients", TIGHT, gradients);
    // A star clipping rects, a rounded rect, a rounded clip and text: the
    // path clip multiplies in exactly; what is left is the rounded rect's
    // and the glyphs' own differences (see quad_pixels and quad_media).
    // Measured 45.
    compare(g, f, "path-clipped", corners(46, 0.04), clipped);
    // Drop shadows of stars: rows blurred analytically, against
    // `vello_cpu`'s Gaussian filter layer. Measured 2, mean 0.037.
    compare(g, f, "path-shadows", corners(3, 0.04), shadows);
    // A rotated star and arc, and a star filled and stroked under a
    // fractional non-uniform scale: the arc's and stroke's joins as above.
    // Measured 73.
    compare(g, f, "path-turned", corners(74, 0.01), turned);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    if let Some(error) = pollster::block_on(scope.pop()) {
        panic!("wgpu validation: {error}");
    }
}

/// `kids` alone on a transparent target.
fn alone(gpu: &mut Gpu, kids: Vec<El>, scale: f64) -> Vec<u8> {
    let scene = scene_at(stack(kids).size(W, H), scale);
    gpu_pixels(gpu, &scene.paint, Affine::scale(scale), sized(scale))
}

/// A path fill and a rect meeting along an edge turned off the pixel grid:
/// their coverages add up to one wherever both reach, so together they
/// leave no seam and no overlap.
#[test]
fn a_path_abutting_a_rect_leaves_no_seam() {
    let Some(mut gpu) = gpu() else {
        return;
    };
    let wedge = |s: Size| {
        Path::polyline(
            [(0., 0.), (s.width, 0.), (s.width, s.height), (s.width * 0.3, s.height)]
                .map(|(x, y)| Point::new(x, y)),
            true,
        )
    };
    let white = Color::srgb(1., 1., 1.);
    // One of the pair, in the same turned frame.
    // One of the pair painted, the other laid out unpainted; or, with
    // neither, their union as one rect.
    let turned = |kids: Vec<El>| row(kids).gap(0.).rotation(0.3).at(90., 50.);
    let paint = |el: El, on: bool| if on { el.fill(white) } else { el.no_fill() };
    let half = |path: bool| {
        turned(vec![
            paint(block(60., 70.).outline(wedge), path),
            paint(block(50., 70.).radius(0.), !path),
        ])
    };
    for scale in SCALES {
        let path = alone(&mut gpu, vec![half(true)], scale);
        let rect = alone(&mut gpu, vec![half(false)], scale);
        let whole = alone(
            &mut gpu,
            vec![turned(vec![block(110., 70.).radius(0.).fill(white)])],
            scale,
        );
        // Inside the union, away from its own edges (where the rect pass's
        // corner is a product of two edges, not an exact area), the seam
        // must vanish: what one leaves uncovered the other covers.
        let w = sized(scale)[0] as usize;
        let mut shared = 0;
        for (i, ((a, b), u)) in path
            .chunks(4)
            .zip(rect.chunks(4))
            .zip(whole.chunks(4))
            .enumerate()
        {
            if b[3] > 0 && u[3] == 255 {
                shared += usize::from(a[3] > 0);
                let sum = u16::from(a[3]) + u16::from(b[3]);
                let (x, y) = (i % w, i / w);
                assert!(
                    sum.abs_diff(u16::from(u[3])) <= 1,
                    "scale {scale}, {x},{y}: {} + {} against {}",
                    a[3],
                    b[3],
                    u[3]
                );
            }
        }
        assert!(shared as f64 > 60. * scale, "scale {scale}: {shared} shared pixels");
    }
}

/// A ring's hole is empty and its band is solid.
#[test]
fn a_closed_ring_leaves_its_hole_empty() {
    let Some(mut gpu) = gpu() else {
        return;
    };
    for scale in SCALES {
        let ring = alone(
            &mut gpu,
            vec![
                block(90., 90.)
                    .pill()
                    .fill(Color::srgb(1., 1., 1.))
                    .cut(block(50., 50.).pill().at(20., 20.))
                    .at(40., 40.),
            ],
            scale,
        );
        let [w, _] = sized(scale);
        let alpha = |x: f64, y: f64| {
            let (x, y) = ((x * scale) as usize, (y * scale) as usize);
            ring[(y * w as usize + x) * 4 + 3]
        };
        for (x, y) in [(85., 85.), (70., 85.), (85., 100.), (100., 70.)] {
            assert_eq!(alpha(x, y), 0, "scale {scale}: hole at {x},{y}");
        }
        for (x, y) in [(50., 85.), (85., 50.), (120., 85.), (85., 120.)] {
            assert_eq!(alpha(x, y), 255, "scale {scale}: band at {x},{y}");
        }
    }
}
