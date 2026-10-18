use kurbo::{Point as KPoint, Shape};
use mui_geometry::*;
fn has(path: &Path, x: f64, y: f64) -> bool {
    bez_path(path, 0.01).unwrap().winding(KPoint::new(x, y)) != 0
}
fn rect() -> Path {
    RoundedRect::new(Rect::new(0., 0., 200., 100.), 0.)
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
                &boolean_paths(
                    a,
                    b,
                    op,
                    OffsetOptions::default(),
                    GeometryOptions::default()
                )
                .unwrap(),
                50.,
                50.
            ));
        }
        assert!(
            boolean_paths(
                a,
                b,
                BooleanOp::Intersection,
                OffsetOptions::default(),
                GeometryOptions::default()
            )
            .unwrap()
            .commands
            .is_empty()
        );
    }
    assert!(
        boolean_paths(
            &empty,
            &p,
            BooleanOp::Difference,
            OffsetOptions::default(),
            GeometryOptions::default()
        )
        .unwrap()
        .commands
        .is_empty()
    );
    assert!(has(
        &boolean_paths(
            &p,
            &empty,
            BooleanOp::Difference,
            OffsetOptions::default(),
            GeometryOptions::default()
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
            OffsetOptions::default(),
            GeometryOptions::default(),
        )
        .unwrap();
        let padded = inset_path(&border.interior, 2., OffsetOptions::default())
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
    let hole = RoundedRect::new(Rect::new(90., 90., 110., 110.), 4.)
        .unwrap()
        .path();
    let ring = boolean_paths(
        &star,
        &hole,
        BooleanOp::Difference,
        OffsetOptions::default(),
        GeometryOptions::default(),
    )
    .unwrap();
    let inner = inset_path(&ring, 2., OffsetOptions::default())
        .unwrap()
        .path;
    let halves = ShapeSplit::new(SplitAxis::X, 0.5)
        .gap(2.)
        .regions(&inner, OffsetOptions::default(), GeometryOptions::default())
        .unwrap();
    let source = ring.flatten(0.01, 10000).unwrap();
    for half in halves {
        assert!(!has(&half, 100., 100.));
        for part in ShapeSplit::new(SplitAxis::Y, 0.5)
            .gap(2.)
            .regions(&half, OffsetOptions::default(), GeometryOptions::default())
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
                .regions(
                    &rect(),
                    OffsetOptions::default(),
                    GeometryOptions::default(),
                )
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
        assert!(
            border_geometry(
                &rect(),
                WidthProfile::uniform(value),
                BorderAlign::Inside,
                OffsetOptions::default(),
                GeometryOptions::default()
            )
            .is_err()
        );
        assert!(
            ShapeSplit::new(SplitAxis::X, 0.5)
                .gap(value)
                .regions(
                    &rect(),
                    OffsetOptions::default(),
                    GeometryOptions::default()
                )
                .is_err()
        );
    }
    assert!(
        ShapeSplit::new(SplitAxis::X, 0.5)
            .bend(0.2)
            .regions(
                &rect(),
                OffsetOptions {
                    flatten_tolerance: 1e-12,
                    max_points: 100,
                    ..Default::default()
                },
                GeometryOptions::default()
            )
            .is_err()
    );
    assert!(
        WidthProfile::horizontal(1., 2., -f64::MAX, f64::MAX)
            .at(0.)
            .is_err()
    );
    let invalid = Path::polyline(
        [
            Point::new(0., 0.),
            Point::new(f64::NAN, 1.),
            Point::new(1., 0.),
        ],
        true,
    );
    assert!(
        boundary_band(
            &invalid,
            WidthProfile::uniform(1.),
            OffsetOptions::default()
        )
        .is_err()
    );
    let empty = ShapeSplit::new(SplitAxis::X, 0.5)
        .regions(
            &Path::default(),
            OffsetOptions::default(),
            GeometryOptions::default(),
        )
        .unwrap();
    assert!(empty.iter().all(|p| p.commands.is_empty()));
}

#[test]
fn uniform_offsets_match_sweep_for_holes_and_all_alignments() {
    let o = OffsetOptions::default();
    let g = GeometryOptions::default();
    let hole = RoundedRect::new(Rect::new(60., 30., 80., 40.), 8.)
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

/// The previous solid-piece sweep is an independent filled-set oracle. Keep
/// its duplicated endpoint disks and quarter-circle representation here.
fn endpoint_disk_sweep(outline: &Path, width: WidthProfile, o: OffsetOptions) -> Path {
    let normalized = offset_path(outline, 0., o).unwrap().path;
    let mut path = Path::default();
    for ring in normalized.flatten(o.flatten_tolerance, o.max_points).unwrap() {
        for (a, b) in ring.iter().copied().zip(ring.iter().copied().cycle().skip(1)).take(ring.len()) {
            let d = b - a;
            let len = d.length();
            if len < 1e-9 {
                continue;
            }
            let normal = d.turn_90() / len;
            let mut cuts = vec![0., 1.];
            if d.x.abs() > 1e-9 {
                for x in [width.start, width.end] {
                    let t = (x - a.x) / d.x;
                    if t > 0. && t < 1. {
                        cuts.push(t);
                    }
                }
            }
            cuts.sort_by(f64::total_cmp);
            for ts in cuts.windows(2) {
                let p = a + d * ts[0];
                let q = a + d * ts[1];
                let wp = width.at(p.x).unwrap();
                let wq = width.at(q.x).unwrap();
                if wp.max(wq) == 0. {
                    continue;
                }
                path.commands.extend(Path::polyline([
                    p - normal * wp,
                    q - normal * wq,
                    q + normal * wq,
                    p + normal * wp,
                ], true).commands);
                for (p, w) in [(p, wp), (q, wq)] {
                    if w > 0. {
                        path.commands.extend(RoundedRect::new(
                            Rect::new(p.x - w, p.y - w, p.x + w, p.y + w), w,
                        ).unwrap().path().commands);
                    }
                }
            }
        }
    }
    path
}

#[test]
fn one_join_per_vertex_preserves_the_sweep_fill_and_solid_contours() {
    let o = OffsetOptions::default();
    let hole = RoundedRect::new(Rect::new(60., 30., 80., 40.), 4.).unwrap().path();
    let mut multiple = boolean_paths(&rect(), &hole, BooleanOp::Difference, o, GeometryOptions::default()).unwrap();
    multiple.commands.extend(RoundedRect::new(Rect::new(220., 20., 240., 50.), 3.).unwrap().path().commands);
    let short_edges = Path::polyline([
        Point::new(0., 0.), Point::new(0., 0.), Point::new(5e-10, 0.),
        Point::new(0.0001, 0.), Point::new(0.0001, 0.00004), Point::new(0., 0.00004),
    ], true);
    for (outline, unit) in [(rect(), 1.), (multiple, 1.), (short_edges, 0.000001)] {
        for width in [
            WidthProfile::uniform(unit),
            WidthProfile::horizontal(0., unit, 50. * unit, 150. * unit),
            WidthProfile::horizontal(unit, unit * 0.2, 50. * unit, 150. * unit),
            WidthProfile::uniform(unit * 0.01),
            WidthProfile::uniform(0.),
        ] {
            let reference = bez_path(&endpoint_disk_sweep(&outline, width, o), 0.001 * unit).unwrap();
            let sweep = boundary_band(&outline, width, o).unwrap();
            sweep.validate(o.max_points).unwrap();
            let actual = bez_path(&sweep, 0.001 * unit).unwrap();
            // union_contours must still see solid pieces, never a whole
            // annulus whose inner contour would be filled independently.
            for ring in sweep.flatten(o.flatten_tolerance, o.max_points).unwrap() {
                let area = ring.iter().zip(ring.iter().cycle().skip(1)).take(ring.len())
                    .map(|(a, b)| a.x * b.y - a.y * b.x).sum::<f64>();
                assert!(area > 0., "every sweep piece remains a positive solid contour");
            }
            let merged = union_contours(&sweep, o, GeometryOptions {
                epsilon: (unit * 0.001).min(1e-7),
                ..Default::default()
            }).unwrap();
            let merged = bez_path(&merged, 0.001 * unit).unwrap();
            let outline_bounds = bez_path(&outline, 0.001 * unit).unwrap().bounding_box();
            let interior = outline_bounds.center();
            assert_eq!(merged.winding(interior) != 0, reference.winding(interior) != 0,
                "merging the pieces must not fill the band's interior");
            let bounds = outline_bounds.inflate(unit * 3., unit * 3.);
            for ix in 0..73 {
                for iy in 0..47 {
                    let p = KPoint::new(
                        bounds.x0 + bounds.width() * (f64::from(ix) + 0.37) / 73.,
                        bounds.y0 + bounds.height() * (f64::from(iy) + 0.23) / 47.,
                    );
                    let expected = reference.winding(p) != 0;
                    assert_eq!(actual.winding(p) != 0, expected, "{width:?} at {p:?}");
                }
            }
        }
    }
}

#[test]
fn complex_6468_vertex_sweeps_fit_the_existing_budget_at_both_scales() {
    let outline = Path::polyline((0..6468).map(|i| {
        let angle = f64::from(i) * std::f64::consts::TAU / 6468.;
        let radius = if i % 2 == 0 { 1000. } else { 1000.5 };
        Point::new(radius * angle.cos(), radius * angle.sin())
    }), true);
    for scale in [1., 2.] {
        let o = OffsetOptions { flatten_tolerance: 0.1 / scale, max_points: 100_000, ..Default::default() };
        for width in [WidthProfile::uniform(1.), WidthProfile::horizontal(0.5, 1., -500., 500.)] {
            let reference = endpoint_disk_sweep(&outline, width, o);
            assert!(reference.commands.len() > o.max_points, "the previous representation exceeded this unchanged cap");
            let sweep = boundary_band(&outline, width, o).unwrap();
            sweep.validate(o.max_points).unwrap();
            sweep.flatten(o.flatten_tolerance, o.max_points).unwrap();
        }
    }
    let tiny = OffsetOptions { max_points: 8, ..Default::default() };
    assert_eq!(boundary_band(&rect(), WidthProfile::uniform(1.), tiny), Err(Error::TooManySegments));
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

#[test]
fn filled_path_booleans_preserve_a_hole_touching_the_outer_edge() {
    let mut shape = Path::polyline(
        [
            Point::new(0., 0.),
            Point::new(40., 0.),
            Point::new(40., 40.),
            Point::new(0., 40.),
        ],
        true,
    );
    shape.commands.extend(
        Path::polyline(
            // Wound opposite to the exterior: offset input is nonzero.
            [
                Point::new(0., 20.),
                Point::new(10., 30.),
                Point::new(20., 20.),
                Point::new(10., 10.),
            ],
            true,
        )
        .commands,
    );
    let result = boolean_paths(
        &shape,
        &shape,
        BooleanOp::Intersection,
        OffsetOptions::default(),
        GeometryOptions::default(),
    )
    .unwrap();
    let topology = offset_path(&result, 0., OffsetOptions::default())
        .unwrap()
        .topology;
    assert!((topology.area() - 1400.).abs() < 0.01);
}
