use kurbo::{Point as KPoint, Shape};
use mui_geometry::*;
fn has(path: &Path, x: f64, y: f64) -> bool {
    bez_path(path, 0.01).unwrap().winding(KPoint::new(x, y)) != 0
}
fn rect() -> Path {
    RoundedRect::new(Bounds::new(0., 0., 200., 100.), 0.)
        .unwrap()
        .path()
}
#[test]
fn empty_sets_have_boolean_semantics_in_both_orders() {
    let p = rect();
    let empty = Path::default();
    for (a, b) in [(&p, &empty), (&empty, &p)] {
        for op in [BooleanOp::Union, BooleanOp::Xor] {
            assert!(has(
                &boolean_paths(a, b, op, Default::default(), Default::default()).unwrap(),
                50.,
                50.
            ));
        }
        assert!(boolean_paths(
            a,
            b,
            BooleanOp::Intersection,
            Default::default(),
            Default::default()
        )
        .unwrap()
        .commands
        .is_empty());
    }
    assert!(boolean_paths(
        &empty,
        &p,
        BooleanOp::Difference,
        Default::default(),
        Default::default()
    )
    .unwrap()
    .commands
    .is_empty());
    assert!(has(
        &boolean_paths(
            &p,
            &empty,
            BooleanOp::Difference,
            Default::default(),
            Default::default()
        )
        .unwrap(),
        50.,
        50.
    ));
}
#[test]
fn ramp_alignments_leave_the_correct_interior_and_outside_band() {
    for (align, factor) in [
        (BorderAlign::Inside, 1.),
        (BorderAlign::Center, 0.5),
        (BorderAlign::Outside, 0.),
    ] {
        let border = border_geometry(
            &rect(),
            WidthProfile::horizontal(20., 1., 0., 200.),
            align,
            Default::default(),
            Default::default(),
        )
        .unwrap();
        let padded = inset_path(&border.interior, 2., Default::default())
            .unwrap()
            .path;
        for x in [50., 100., 150.] {
            let edge = (20. - 19. * x / 200.) * factor;
            assert!(!has(&padded, x, edge + 1.8));
            assert!(has(&padded, x, edge + 2.3));
            if align == BorderAlign::Outside {
                assert!(has(&border.band, x, -0.5));
                assert!(!has(&border.band, x, 0.5));
            }
        }
    }
}
#[test]
fn partition_inherits_star_and_hole_and_can_be_split_again() {
    let vertices = (0..10).map(|i| {
        let angle = (i as f64) * std::f64::consts::PI / 5.;
        let radius = if i % 2 == 0 { 90. } else { 45. };
        Point::new(100. + radius * angle.cos(), 100. + radius * angle.sin())
    });
    let star = Path::polyline(vertices, true);
    let hole = RoundedRect::new(Bounds::new(90., 90., 110., 110.), 4.)
        .unwrap()
        .path();
    let ring = boolean_paths(
        &star,
        &hole,
        BooleanOp::Difference,
        Default::default(),
        Default::default(),
    )
    .unwrap();
    let inner = inset_path(&ring, 2., Default::default()).unwrap().path;
    let halves = ShapeSplit::new(SplitAxis::X, 0.5)
        .gap(2.)
        .regions(&inner, Default::default(), Default::default())
        .unwrap();
    let source = ring.flatten(0.01, 10000).unwrap();
    for half in halves {
        assert!(!has(&half, 100., 100.));
        for part in ShapeSplit::new(SplitAxis::Y, 0.5)
            .gap(2.)
            .regions(&half, Default::default(), Default::default())
            .unwrap()
        {
            for ring in part.flatten(0.01, 10000).unwrap() {
                for p in ring {
                    assert!(boundary_distance(p, &source) >= 1.9);
                }
            }
        }
    }
}
#[test]
fn curved_partition_measures_gap_normally_on_both_axes() {
    for axis in [SplitAxis::X, SplitAxis::Y] {
        for bend in [-0.2, 0.2] {
            let parts = ShapeSplit::new(axis, 0.5)
                .gap(4.)
                .bend(bend)
                .regions(&rect(), Default::default(), Default::default())
                .unwrap();
            let rings = parts[1].flatten(0.01, 10000).unwrap();
            for ring in parts[0].flatten(0.01, 10000).unwrap() {
                for p in ring {
                    assert!(boundary_distance(p, &rings) >= 3.85);
                }
            }
        }
    }
}
#[test]
fn geometry_limits_and_invalid_inputs_are_errors() {
    for value in [f64::NAN, f64::INFINITY, -1.] {
        assert!(border_geometry(
            &rect(),
            WidthProfile::uniform(value),
            BorderAlign::Inside,
            Default::default(),
            Default::default()
        )
        .is_err());
        assert!(ShapeSplit::new(SplitAxis::X, 0.5)
            .gap(value)
            .regions(&rect(), Default::default(), Default::default())
            .is_err());
    }
    assert!(ShapeSplit::new(SplitAxis::X, 0.5)
        .bend(0.2)
        .regions(
            &rect(),
            OffsetOptions {
                flatten_tolerance: 1e-12,
                max_points: 100,
                ..Default::default()
            },
            Default::default()
        )
        .is_err());
    assert!(WidthProfile::horizontal(1., 2., -f64::MAX, f64::MAX)
        .at(0.)
        .is_err());
    let invalid = Path::polyline(
        [
            Point::new(0., 0.),
            Point::new(f64::NAN, 1.),
            Point::new(1., 0.),
        ],
        true,
    );
    assert!(boundary_band(&invalid, WidthProfile::uniform(1.), Default::default()).is_err());
    let empty = ShapeSplit::new(SplitAxis::X, 0.5)
        .regions(&Path::default(), Default::default(), Default::default())
        .unwrap();
    assert!(empty.iter().all(|p| p.commands.is_empty()));
}

#[test]
fn uniform_offsets_match_sweep_for_holes_and_all_alignments() {
    let o = OffsetOptions::default();
    let g = GeometryOptions::default();
    let hole = RoundedRect::new(Bounds::new(60., 30., 80., 40.), 8.)
        .unwrap()
        .path();
    let outline = boolean_paths(&rect(), &hole, BooleanOp::Difference, o, g).unwrap();
    for align in [
        BorderAlign::Inside,
        BorderAlign::Center,
        BorderAlign::Outside,
    ] {
        let width = 4.;
        let fast = border_geometry(&outline, WidthProfile::uniform(width), align, o, g).unwrap();
        let radius = if align == BorderAlign::Center {
            width / 2.
        } else {
            width
        };
        let sweep = union_contours(
            &boundary_band(&outline, WidthProfile::uniform(radius), o).unwrap(),
            o,
            g,
        )
        .unwrap();
        let reference = match align {
            BorderAlign::Inside => {
                boolean_paths(&sweep, &outline, BooleanOp::Intersection, o, g).unwrap()
            }
            BorderAlign::Center => sweep,
            BorderAlign::Outside => {
                boolean_paths(&sweep, &outline, BooleanOp::Difference, o, g).unwrap()
            }
        };
        for x in (-5..206).step_by(3) {
            for y in (-5..106).step_by(3) {
                let (x, y) = (f64::from(x) + 0.37, f64::from(y) + 0.37);
                assert_eq!(
                    has(&fast.band, x, y),
                    has(&reference, x, y),
                    "{align:?} at {x},{y}"
                );
                assert_eq!(
                    has(&fast.interior, x, y),
                    has(&outline, x, y) && !has(&reference, x, y)
                );
            }
        }
    }
}

#[test]
fn overlapping_sweep_pieces_use_bounded_batches_but_keep_the_output_limit() {
    let square = |x: f64| {
        Path::polyline(
            [
                Point::new(x, 0.),
                Point::new(x + 4., 0.),
                Point::new(x + 4., 4.),
                Point::new(x, 4.),
            ],
            true,
        )
    };
    let mut sweep = Path::default();
    for _ in 0..200 {
        sweep.commands.extend(square(0.).commands);
    }
    let geometry = GeometryOptions {
        max_vertices: 64,
        ..GeometryOptions::default()
    };
    let joined = union_contours(&sweep, OffsetOptions::default(), geometry).unwrap();
    assert_eq!(joined.flatten(0.1, 1000).unwrap().len(), 1);
    let mut disjoint = Path::default();
    for n in 0..100 {
        disjoint.commands.extend(square(f64::from(n) * 8.).commands);
    }
    assert!(union_contours(&disjoint, OffsetOptions::default(), geometry).is_err());
}
