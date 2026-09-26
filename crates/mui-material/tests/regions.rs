use mui_geometry::kurbo::{Point as KPoint, Shape};
use mui_geometry::{bez_path, boundary_distance};
use mui_material::prelude::*;
use mui_scene::SceneError;
fn has(path: &Path, x: f64, y: f64) -> bool {
    bez_path(path, 0.01).unwrap().winding(KPoint::new(x, y)) != 0
}
fn cell(id: &str) -> El {
    stack![].flex(1.).fill(Role::Primary).id(id)
}
fn octagon(s: Size) -> Path {
    let (w, h) = (s.width, s.height);
    let d = w.min(h) * 0.25;
    Path::polyline(
        [
            (d, 0.),
            (w - d, 0.),
            (w, d),
            (w, h - d),
            (w - d, h),
            (d, h),
            (0., h - d),
            (0., d),
        ]
        .map(|(x, y)| Point::new(x, y)),
        true,
    )
}
#[test]
fn octagon_split_nests_and_keeps_exact_padding_and_total_gap() {
    let root = row![
        cell("a"),
        col![cell("b"), cell("c")].flex(1.).inside(2.).id("right")
    ]
    .inside(2.)
    .outline(octagon)
    .w(200.)
    .h(160.)
    .id("root");
    let scene = resolve(&SceneSpec::new(root)).unwrap();
    let a = scene.surface("a").unwrap();
    let right = scene.surface("right").unwrap();
    assert!((right.frame.x - a.frame.right() - 2.).abs() < 0.01);
    assert!(!has(&a.path, 3., 3.));
    assert!(has(&a.path, 20., 80.));
    let root = scene
        .surface("root")
        .unwrap()
        .path
        .flatten(0.01, 10000)
        .unwrap();
    for ring in a.path.flatten(0.01, 10000).unwrap() {
        for p in ring {
            assert!(boundary_distance(p, &root) >= 1.95);
        }
    }
    let b = scene.surface("b").unwrap();
    let c = scene.surface("c").unwrap();
    assert!((c.frame.y - b.frame.bottom() - 2.).abs() < 0.01);
    assert_eq!(scene.layout.frame("b"), Some(b.frame));
    assert!(b.clip_paths().unwrap().iter().any(|p| p.0 == right.path));
}
#[test]
fn inward_ramp_padding_follows_the_actual_sloped_border() {
    for (align, factor) in [
        (BorderAlign::Inside, 1.),
        (BorderAlign::Center, 0.5),
        (BorderAlign::Outside, 0.),
    ] {
        let root = stack![cell("child")]
            .inside(2.)
            .radius(0.)
            .w(200.)
            .h(100.)
            .border_ramp(
                BorderRamp::horizontal((Role::Primary, 20.), (Role::Dim, 1.)).align(align),
            );
        let scene = resolve(&SceneSpec::new(root)).unwrap();
        let p = &scene.surface("child").unwrap().path;
        assert!(!has(p, 20. * factor + 1., 50.));
        assert!(has(p, 20. * factor + 2.5, 50.));
        assert!(!has(p, 200. - factor - 1., 50.));
        assert!(has(p, 200. - factor - 2.5, 50.));
        for x in [50., 100., 150.] {
            let edge = (20. - 19. * x / 200.) * factor;
            assert!(!has(p, x, edge + 1.8));
            assert!(has(p, x, edge + 2.3));
        }
    }
}
#[test]
fn curved_split_has_a_real_gap_and_reverses_direction() {
    for bend in [-0.2, 0.2] {
        let scene = resolve(&SceneSpec::new(
            row![cell("a"), cell("b")]
                .inside(4.)
                .bend(bend)
                .radius(0.)
                .w(200.)
                .h(160.),
        ))
        .unwrap();
        let a = &scene.surface("a").unwrap().path;
        let b = &scene.surface("b").unwrap().path;
        let middle = 100. + bend * 192.;
        assert!(!has(a, middle, 80.) && !has(b, middle, 80.));
        assert!(has(a, middle - 2.2, 80.));
        assert!(has(b, middle + 2.2, 80.));
        let rings = b.flatten(0.01, 10000).unwrap();
        for ring in a.flatten(0.01, 10000).unwrap() {
            for p in ring {
                assert!(boundary_distance(p, &rings) >= 3.9);
            }
        }
    }
}
#[test]
fn holes_survive_and_consumed_regions_disappear() {
    // Wound against the octagon: outlines fill NonZero, like the renderer.
    let ring = |s: Size| {
        let mut p = octagon(s);
        p.commands.extend(
            Path::polyline(
                [(70., 50.), (70., 110.), (130., 110.), (130., 50.)].map(|(x, y)| Point::new(x, y)),
                true,
            )
            .commands,
        );
        p
    };
    let scene = resolve(&SceneSpec::new(
        stack![cell("child")]
            .inside(2.)
            .outline(ring)
            .w(200.)
            .h(160.),
    ))
    .unwrap();
    let p = &scene.surface("child").unwrap().path;
    assert!(!has(p, 100., 80.));
    assert!(!has(p, 69., 80.));
    assert!(has(p, 67., 80.));
    let scene = resolve(&SceneSpec::new(
        stack![cell("child")].inside(100.).w(40.).h(40.),
    ))
    .unwrap();
    assert!(scene.surface("child").is_none());
}
#[test]
fn invalid_padding_bend_and_border_fail_before_publication() {
    // `.inside(pad)` is also the gap, so layout refuses it first.
    for pad in [-1., f64::NAN, f64::INFINITY] {
        assert!(matches!(
            resolve(&SceneSpec::new(
                row![cell("a"), cell("b")].inside(pad).w(100.).h(100.)
            )),
            Err(SceneError::Layout(mui_layout::Error::InvalidValue))
        ));
    }
    for bend in [0.5, f64::NAN] {
        assert!(matches!(
            resolve(&SceneSpec::new(
                row![cell("a"), cell("b")]
                    .inside(2.)
                    .bend(bend)
                    .w(100.)
                    .h(100.)
            )),
            Err(SceneError::Geometry(mui_geometry::Error::InvalidOptions(
                "shape padding/bend"
            )))
        ));
    }
}
#[test]
fn ordinary_alignment_reserves_only_the_inward_share() {
    for (align, factor) in [
        (BorderAlign::Inside, 1.),
        (BorderAlign::Center, 0.5),
        (BorderAlign::Outside, 0.),
    ] {
        let root = stack![cell("child")]
            .inside(2.)
            .stroke(Role::Primary).stroke_width(20.)
            .border_align(align)
            .w(100.)
            .h(100.)
            .radius(0.);
        let scene = resolve(&SceneSpec::new(root)).unwrap();
        let b = mui_geometry::bounds(
            scene
                .surface("child")
                .unwrap()
                .path
                .flatten(0.01, 10000)
                .unwrap()
                .concat(),
        )
        .unwrap();
        assert!((b.x0 - 20. * factor - 2.).abs() < 0.01);
    }
}

#[test]
fn morph_resize_and_cached_resolution_agree() {
    let mut cache = mui_scene::Resolver::default();
    let mut previous = None;
    for (pad, width, scale, bend) in [(2., 200., 1., 0.), (8., 240., 2., 0.2), (2., 200., 1., 0.)] {
        let root = row![cell("a"), cell("b")]
            .inside(pad)
            .bend(bend)
            .outline(octagon)
            .w(width)
            .h(160.)
            .border_ramp(BorderRamp::horizontal(
                (Role::Primary, pad * 2.),
                (Role::Dim, 1.),
            ));
        let mut spec = SceneSpec::new(root);
        spec.device_scale = Some(scale);
        let fresh = resolve(&spec).unwrap();
        let cached = cache.resolve(&spec).unwrap().clone();
        let again = cache.resolve(&spec).unwrap().clone();
        assert_eq!(
            fresh.surface("a").unwrap().path,
            cached.surface("a").unwrap().path
        );
        assert_eq!(
            cached.surface("a").unwrap().path,
            again.surface("a").unwrap().path
        );
        if let Some(p) = previous {
            assert_ne!(p, cached.surface("a").unwrap().path);
        }
        previous = Some(cached.surface("a").unwrap().path.clone());
    }
}

#[test]
fn shaped_canvases_clip_their_own_draws() {
    let child = canvas(|size| {
        vec![Draw::fill(
            Path::polyline(
                [
                    (0., 0.),
                    (size.width, 0.),
                    (size.width, size.height),
                    (0., size.height),
                ]
                .map(|(x, y)| Point::new(x, y)),
                true,
            ),
            Role::Primary,
        )]
    })
    .flex(1.)
    .id("canvas");
    let scene = resolve(&SceneSpec::new(
        stack![child].inside(2.).outline(octagon).w(200.).h(160.),
    ))
    .unwrap();
    let draw = scene
        .paint
        .iter()
        .position(|p| p.key.as_ref() == "canvas" && matches!(p.layer, mui_scene::Layer::Draw(_)))
        .unwrap();
    assert_eq!(scene.paint[draw - 1].layer, mui_scene::Layer::Clip);
    assert_eq!(
        scene.paint[draw - 1].path,
        scene.surface("canvas").unwrap().path
    );
    assert_eq!(scene.paint[draw + 1].layer, mui_scene::Layer::Unclip);
}

#[test]
fn vector_weld_is_partitioned_after_the_union() {
    let root = row![
        stack![].w(100.).h(60.).id("a"),
        stack![].w(100.).h(120.).id("b")
    ]
    .align(Align::Center)
    .union(Role::Surface)
    .radius((0., 0.))
    .inside(2.)
    .w(200.)
    .h(120.)
    .id("root");
    let scene = resolve(&SceneSpec::new(root)).unwrap();
    let root = scene
        .surface("root")
        .unwrap()
        .path
        .flatten(0.01, 10000)
        .unwrap();
    for id in ["a", "b"] {
        for r in scene
            .surface(id)
            .unwrap()
            .path
            .flatten(0.01, 10000)
            .unwrap()
        {
            for p in r {
                assert!(boundary_distance(p, &root) >= 1.9);
            }
        }
    }
}

#[test]
fn outward_child_borders_preserve_the_allocated_gap() {
    for alignment in [BorderAlign::Outside, BorderAlign::Center] {
        let scene = resolve(&SceneSpec::new(
            row![
                cell("a").stroke(Role::Primary).stroke_width(10.).border_align(alignment),
                cell("b").stroke(Role::Primary).stroke_width(10.).border_align(alignment),
            ]
            .inside(2.)
            .radius(0.)
            .w(200.)
            .h(100.),
        ))
        .unwrap();
        let a = scene.surface("a").unwrap().frame;
        let b = scene.surface("b").unwrap().frame;
        let out = if alignment == BorderAlign::Outside {
            10.
        } else {
            5.
        };
        assert!((a.x - out - 2.).abs() < 0.01);
        assert!((b.x - a.right() - out * 2. - 2.).abs() < 0.01);
    }
}
