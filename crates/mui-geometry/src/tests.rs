use crate::math::{point_segment_distance, signed_area};
use crate::*;
use std::f64::consts::PI;
fn r(x: f64, y: f64, w: f64, h: f64) -> PlacedShape {
    Polygon::rectangle(x, y, w, h).unwrap().into()
}
#[test]
fn rectangle_rejects_nonfinite_derived_endpoints() {
    assert_eq!(
        Polygon::rectangle(1e308, 0., 1e308, 1.),
        Err(Error::NonFinite)
    );
    assert_eq!(
        Polygon::rectangle(0., 1e308, 1., 1e308),
        Err(Error::NonFinite)
    );
}
#[test]
fn rectangle_rejects_rounded_away_extent_but_accepts_next_ulp() {
    let base = 1e16;
    assert_eq!(
        Polygon::rectangle(base, 0., 1., 1.),
        Err(Error::DegenerateRing)
    );
    assert!(Polygon::rectangle(base, 0., 2., 1.).is_ok());
    assert_eq!(
        Polygon::rectangle(0., base, 1., 1.),
        Err(Error::DegenerateRing)
    );
    assert!(Polygon::rectangle(0., base, 1., 2.).is_ok());
}
fn u(p: &[PlacedShape]) -> Topology {
    union(p, GeometryOptions::default()).unwrap()
}
fn rounded(p: &[PlacedShape]) -> RoundedShape {
    fillet(&u(p), Fillet::default()).unwrap()
}
fn concaves(s: &RoundedShape) -> usize {
    s.corners.iter().flatten().filter(|c| c.concave).count()
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-5, "{a} != {b}");
}
#[test]
fn empty_is_valid() {
    assert!(u(&[]).rings().is_empty());
}
#[test]
fn identical_shapes() {
    let a = r(0., 0., 100., 100.);
    let t = u(&[a.clone(), a]);
    assert_eq!(t.components(), 1);
    near(t.area(), 10000.);
    assert_eq!(t.vertex_count(), 4);
}
#[test]
fn containment() {
    let t = u(&[r(0., 0., 100., 100.), r(20., 20., 30., 30.)]);
    near(t.area(), 10000.);
    assert_eq!(t.vertex_count(), 4);
}
#[test]
fn separate() {
    let t = u(&[r(0., 0., 100., 100.), r(200., 0., 40., 40.)]);
    assert_eq!(t.components(), 2);
}
#[test]
fn full_shared_edge() {
    let t = u(&[r(0., 0., 100., 100.), r(0., -50., 100., 50.)]);
    assert_eq!(t.vertex_count(), 4);
    near(t.area(), 15000.);
}
#[test]
fn left_shoulder_is_one() {
    assert_eq!(
        concaves(&rounded(&[r(0., 50., 200., 100.), r(0., 0., 60., 50.)])),
        1
    );
}
#[test]
fn centered_shoulders_are_two() {
    assert_eq!(
        concaves(&rounded(&[r(0., 50., 200., 100.), r(70., 0., 60., 50.)])),
        2
    );
}
#[test]
fn zero_radius_returns_lines() {
    let s = fillet(
        &u(&[r(0., 0., 100., 100.)]),
        Fillet {
            convex_radius: 0.,
            concave_radius: 0.,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(s.corners.iter().flatten().all(|c| c.arc.is_none()));
}
#[test]
fn only_concave() {
    let s = fillet(
        &u(&[r(0., 50., 200., 100.), r(0., 0., 60., 50.)]),
        Fillet {
            convex_radius: 0.,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(s
        .corners
        .iter()
        .flatten()
        .filter(|c| !c.concave)
        .all(|c| c.effective_radius == 0.));
}
#[test]
fn hole_classification_uses_material() {
    let outer = Polygon::rectangle(0., 0., 200., 200.)
        .unwrap()
        .with_hole(Polygon::rectangle(50., 50., 100., 100.).unwrap().exterior);
    let t = u(&[outer.into()]);
    assert_eq!(t.rings().len(), 2);
    near(t.area(), 30000.);
    assert_eq!(concaves(&fillet(&t, Fillet::default()).unwrap()), 4);
}
#[test]
fn a_shape_can_fill_another_shapes_hole() {
    let a = Polygon::rectangle(0., 0., 100., 100.)
        .unwrap()
        .with_hole(Polygon::rectangle(25., 25., 50., 50.).unwrap().exterior);
    let t = u(&[a.into(), r(25., 25., 50., 50.)]);
    near(t.area(), 10000.);
    assert_eq!(t.rings().len(), 1);
}
#[test]
fn overlapping_holes_are_subtractions_not_xor() {
    let a = Polygon::rectangle(0., 0., 100., 100.)
        .unwrap()
        .with_hole(Polygon::rectangle(10., 10., 40., 40.).unwrap().exterior)
        .with_hole(Polygon::rectangle(30., 10., 40., 40.).unwrap().exterior);
    near(u(&[a.into()]).area(), 7600.);
}
#[test]
fn reflection_does_not_cancel_union() {
    let a = r(0., 0., 100., 100.);
    let b = r(0., 0., 100., 100.)
        .transformed(Affine::scale(-1., 1.).then(Affine::translation(100., 0.)));
    near(u(&[a, b]).area(), 10000.);
}
#[test]
fn arbitrary_rotation() {
    let a = r(0., 0., 300., 120.);
    let b = r(30., -50., 100., 80.)
        .transformed(Affine::rotation_about(PI * 0.2, Point::new(80., -10.)));
    let s = rounded(&[a, b]);
    assert!(!s.path.flatten(0.1, 100000).unwrap().is_empty());
}
#[test]
fn radius_shrinks_at_tiny_step() {
    let s = rounded(&[r(0., 50., 200., 100.), r(0., 0., 199.9, 50.)]);
    let c = s.corners.iter().flatten().find(|c| c.concave).unwrap();
    assert!(c.effective_radius < 0.05);
}
#[test]
fn nonincident_boundary_limits_radius() {
    // Narrow notch: local edge lengths alone are not a sufficient safety test.
    let p = Polygon::new(vec![
        Point::new(0., 0.),
        Point::new(100., 0.),
        Point::new(100., 100.),
        Point::new(51., 100.),
        Point::new(51., 10.),
        Point::new(49., 10.),
        Point::new(49., 100.),
        Point::new(0., 100.),
    ]);
    let t = u(&[p.into()]);
    let s = fillet(
        &t,
        Fillet {
            convex_radius: 200.,
            concave_radius: 200.,
            ..Default::default()
        },
    )
    .unwrap();
    for (ri, ring) in s.corners.iter().enumerate() {
        for (i, c) in ring.iter().enumerate() {
            for (rj, r) in t.rings().iter().enumerate() {
                for (j, &q) in r.points().iter().enumerate() {
                    let k = (j + 1) % r.points().len();
                    if ri != rj || (j != i && k != i) {
                        assert!(
                            c.trim
                                <= point_segment_distance(c.vertex, q, r.points()[k]) * 0.49 + 1e-7
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn arcs_are_tangent() {
    let s = rounded(&[r(0., 0., 100., 100.)]);
    for c in s.corners.iter().flatten() {
        let a = c.arc.unwrap();
        near((c.start - a.center).length(), a.radius);
        near((c.end - a.center).length(), a.radius);
        near((c.vertex - c.start).dot(c.start - a.center), 0.);
    }
}
#[test]
fn shared_edge_budget() {
    let s = fillet(
        &u(&[r(0., 0., 25., 80.)]),
        Fillet {
            convex_radius: 500.,
            ..Default::default()
        },
    )
    .unwrap();
    for ring in &s.corners {
        for i in 0..ring.len() {
            let a = &ring[i];
            let b = &ring[(i + 1) % ring.len()];
            assert!(a.trim + b.trim <= a.vertex.distance(b.vertex) * 0.98 + 1e-8);
        }
    }
}
#[test]
fn negative_options_fail() {
    assert!(fillet(
        &u(&[r(0., 0., 10., 10.)]),
        Fillet {
            convex_radius: -1.,
            ..Default::default()
        }
    )
    .is_err());
}
#[test]
fn invalid_values_fail() {
    assert!(union(
        &[r(0., 0., 20., 20.).transformed(Affine::translation(f64::NAN, 0.))],
        Default::default()
    )
    .is_err());
}
#[test]
fn intersection_and_difference() {
    let a = [r(0., 0., 100., 100.)];
    let b = [r(50., 0., 100., 100.)];
    near(
        boolean(&a, &b, BooleanOp::Intersection, Default::default())
            .unwrap()
            .area(),
        5000.,
    );
    near(
        boolean(&a, &b, BooleanOp::Difference, Default::default())
            .unwrap()
            .area(),
        5000.,
    );
    near(
        boolean(&a, &b, BooleanOp::Xor, Default::default())
            .unwrap()
            .area(),
        10000.,
    );
}
#[test]
fn capsule_is_closed_and_has_area() {
    let p = Path::capsule(40., 100.).unwrap();
    let rings = p.flatten(0.01, 10000).unwrap();
    let expected = 40. * 60. + PI * 400.;
    assert!((signed_area(&rings[0]) - expected).abs() < 2.);
}
#[test]
fn flatten_budget_is_respected() {
    assert!(matches!(
        Path::capsule(100., 200.).unwrap().flatten(0.00001, 10),
        Err(Error::TooManySegments)
    ));
}
#[test]
fn randomized_rectangles_have_finite_safe_arcs() {
    let mut seed = 42_u64;
    let mut rand = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        ((seed >> 32) as f64) / (u32::MAX as f64)
    };
    for _ in 0..400 {
        let x = rand() * 400. - 100.;
        let y = rand() * 300. - 100.;
        let a = rand() * TAU;
        let b = r(x, y, 20. + rand() * 150., 20. + rand() * 150.)
            .transformed(Affine::rotation_about(a, Point::new(x, y)));
        let t = u(&[r(0., 0., 250., 130.), b]);
        let s = fillet(&t, Default::default()).unwrap();
        assert!(s.path.flatten(0.2, 100000).is_ok());
        for c in s.corners.iter().flatten() {
            assert!(c.effective_radius.is_finite() && c.effective_radius >= 0.);
            assert!(c.start.finite() && c.end.finite());
        }
    }
}
const TAU: f64 = PI * 2.;

fn rr(w: f64, h: f64, r: f64) -> RoundedRect {
    RoundedRect::new(
        Bounds {
            min: Point::ZERO,
            max: Point::new(w, h),
        },
        r,
    )
    .unwrap()
}
#[test]
fn concentric_inset_preserves_arc_centers() {
    let outer = rr(92., 170., 28.);
    let inner = outer.inset(12.).unwrap().shape.unwrap();
    near(inner.radius(), 16.);
    let a = outer.path();
    let b = inner.path();
    let arcs = |p: Path| {
        p.commands
            .into_iter()
            .filter_map(|c| {
                if let PathCommand::ArcTo(a) = c {
                    Some(a)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };
    for (a, b) in arcs(a).into_iter().zip(arcs(b)) {
        near(a.center.distance(b.center), 0.);
        for i in 0..=100 {
            near(
                a.point_at(i as f64 / 100.)
                    .distance(b.point_at(i as f64 / 100.)),
                12.,
            );
        }
    }
}
#[test]
fn proportional_is_not_concentric() {
    let p = rr(92., 170., 28.);
    let b = p.inset(12.).unwrap().shape.unwrap().bounds();
    let c = NestedRadius::Proportional { scale: 1. }
        .resolve(p, b)
        .unwrap()
        .shape
        .unwrap();
    assert!((c.radius() - 16.).abs() > 1.);
}
#[test]
fn negative_inset_is_error() {
    assert!(rr(30., 80., 10.).inset(-2.).is_err());
}
#[test]
fn collapsed_inset_is_none() {
    assert!(rr(30., 80., 10.).inset(15.).unwrap().shape.is_none());
}
#[test]
fn vanished_convex_arc_is_reported() {
    let i = rr(80., 100., 8.).inset(12.).unwrap();
    assert!(i.corner_collapsed);
    assert_eq!(i.shape.unwrap().radius(), 0.);
}
#[test]
fn concave_arc_grows_while_convex_shrinks() {
    assert_eq!(inset_arc_radius(28., 12., false).unwrap(), Some(16.));
    assert_eq!(inset_arc_radius(28., 12., true).unwrap(), Some(40.));
}
#[test]
fn stroked_gap_counts_both_half_strokes() {
    near(inset_for_stroked_gap(12., 2., 4.).unwrap(), 15.);
}
#[test]
fn rectangle_offset_matches_analytic() {
    let p = rr(180., 120., 28.).path();
    let i = inset_path(&p, 12., OffsetOptions::default()).unwrap();
    let b = i.topology.bounds().unwrap();
    near(b.min.x, 12.);
    near(b.min.y, 12.);
    near(b.max.x, 168.);
    near(b.max.y, 108.);
    let expected = (180. - 24.) * (120. - 24.) - (4. - PI) * 16_f64.powi(2);
    assert!(
        (i.topology.area() - expected).abs() < 8.,
        "{} vs {}",
        i.topology.area(),
        expected
    );
}
#[test]
fn general_offset_distance_is_measured() {
    let t = u(&[r(0., 80., 300., 170.), r(100., 0., 90., 90.)]);
    let s = fillet(
        &t,
        Fillet {
            convex_radius: 24.,
            concave_radius: 32.,
            ..Default::default()
        },
    )
    .unwrap();
    let source = s.path.flatten(0.001, 50000).unwrap();
    let i = inset_path(
        &s.path,
        8.,
        OffsetOptions {
            flatten_tolerance: 0.02,
            ..Default::default()
        },
    )
    .unwrap();
    for p in i.path.flatten(0.1, 50000).unwrap().into_iter().flatten() {
        let d = boundary_distance(p, &source);
        assert!((d - 8.).abs() < 0.08, "distance {d}");
    }
}
#[test]
fn erosion_can_split_a_narrow_neck() {
    let t = u(&[
        r(0., 0., 60., 60.),
        r(100., 0., 60., 60.),
        r(60., 25., 40., 10.),
    ]);
    let i = inset_path(&t.to_path(), 6., Default::default()).unwrap();
    assert_eq!(i.topology.components(), 2);
    assert!(i.counts_changed);
}
#[test]
fn erosion_can_empty_a_shape() {
    let i = inset_path(&rr(20., 20., 4.).path(), 20., Default::default()).unwrap();
    assert_eq!(i.topology.components(), 0);
    assert!(i.counts_changed);
}
#[test]
fn holes_expand_on_inset() {
    let t = u(&[Polygon::rectangle(0., 0., 100., 100.)
        .unwrap()
        .with_hole(Polygon::rectangle(40., 40., 20., 20.).unwrap().exterior)
        .into()]);
    let i = inset_path(&t.to_path(), 5., Default::default()).unwrap();
    let hole = i
        .topology
        .rings()
        .iter()
        .find(|r| r.kind() == RingKind::Hole)
        .unwrap();
    assert!(hole.signed_area().abs() > 400.);
    assert!(i.topology.area() < t.area());
}
#[test]
fn offset_is_repeatable() {
    let p = rr(92., 170., 28.).path();
    assert_eq!(
        inset_path(&p, 12., Default::default()).unwrap(),
        inset_path(&p, 12., Default::default()).unwrap()
    );
}
#[test]
fn invalid_offset_inputs_rejected() {
    assert!(inset_path(&rr(100., 100., 20.).path(), f64::NAN, Default::default()).is_err());
    assert!(inset_path(
        &rr(100., 100., 20.).path(),
        5.,
        OffsetOptions {
            flatten_tolerance: 0.,
            ..Default::default()
        }
    )
    .is_err());
}
#[test]
fn randomized_analytic_insets() {
    let mut seed = 451_u64;
    for _ in 0..500 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let t = (seed >> 32) as f64 / u32::MAX as f64;
        let w = 40. + t * 200.;
        let h = 100. + t * 300.;
        let r = 0.4 * w;
        let d = 0.2 * r;
        let p = rr(w, h, r);
        let q = p.inset(d).unwrap().shape.unwrap();
        near(q.radius() + d, p.radius());
        near(q.bounds().width() + 2. * d, w);
        near(q.bounds().height() + 2. * d, h);
    }
}

#[test]
fn self_intersection_rejected() {
    let p = Polygon::new(vec![
        Point::new(0., 0.),
        Point::new(70., 90.),
        Point::new(0., 80.),
        Point::new(100., 0.),
        Point::new(100., 100.),
    ]);
    assert!(matches!(
        union(&[p.into()], Default::default()),
        Err(Error::SelfIntersection)
    ));
}
#[test]
fn tight_offset_quality_is_rejected_not_faked() {
    assert!(inset_path(
        &rr(100000., 100000., 20000.).path(),
        10000.,
        OffsetOptions {
            flatten_tolerance: 0.00001,
            ..Default::default()
        }
    )
    .is_err());
}

#[test]
fn svg_serialization_rejects_invalid_arc() {
    let p = Path {
        commands: vec![
            PathCommand::MoveTo(Point::ZERO),
            PathCommand::ArcTo(Arc {
                center: Point::ZERO,
                radius: 1.,
                start_angle: 0.,
                sweep: f64::INFINITY,
                to: Point::ZERO,
            }),
            PathCommand::Close,
        ],
    };
    assert!(p.to_svg_data().is_err());
}
#[test]
fn core_geometry_can_be_owned_across_threads() {
    fn check<T: Send + Sync>() {}
    check::<Topology>();
    check::<RoundedShape>();
    check::<InsetShape>();
}
#[test]
fn randomized_inset_boundary_clearance() {
    let mut seed = 61_u64;
    let mut rand = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (seed >> 32) as f64 / u32::MAX as f64
    };
    for _ in 0..200 {
        let t = u(&[
            r(0., 0., 200., 120.),
            r(
                -30. + rand() * 240.,
                -70. + rand() * 180.,
                40. + rand() * 90.,
                50. + rand() * 80.,
            )
            .transformed(Affine::rotation(rand() * PI)),
        ]);
        let p = fillet(&t, Default::default()).unwrap().path;
        let source = p.flatten(0.002, 50000).unwrap();
        let d = 2. + rand() * 8.;
        let inner = inset_path(&p, d, Default::default()).unwrap();
        for q in inner
            .path
            .flatten(0.1, 50000)
            .unwrap()
            .into_iter()
            .flatten()
        {
            let actual = boundary_distance(q, &source);
            assert!((actual - d).abs() < 0.25, "distance {actual} expected {d}");
        }
    }
}

/// A squircle is the same corner drawn fuller: it never leaves the box the
/// circular corner sat in, and at 45 degrees it stands measurably further
/// out than the arc it replaced.
#[test]
fn a_squircle_corner_stays_in_the_box_and_bulges_past_the_arc() {
    let b = Bounds::new(0., 0., 100., 100.);
    let round = RoundedRect::new(b, 25.).unwrap().path();
    let squircle = CornerStyle::Squircle.shape(&round);
    // The corner's 45-degree point is the extreme of `x + y` toward (0, 0).
    let diagonal = |p: &Path| {
        p.flatten(0.01, 100_000)
            .unwrap()
            .into_iter()
            .flatten()
            .inspect(|q| {
                assert!(
                    q.x >= b.min.x - 1e-9
                        && q.y >= b.min.y - 1e-9
                        && q.x <= b.max.x + 1e-9
                        && q.y <= b.max.y + 1e-9,
                    "{q:?} left the rounded rect's own box"
                );
            })
            .fold(f64::INFINITY, |lo, q| lo.min(q.x + q.y))
    };
    // The arc sits at 50 - r*sqrt(2); the superellipse at 50 - r*2^0.75.
    assert!((diagonal(&round) - (50. - 25. * 2f64.sqrt())).abs() < 0.05);
    assert!((diagonal(&squircle) - (50. - 25. * 2f64.powf(0.75))).abs() < 0.05);
}
