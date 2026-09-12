use crate::math::{point_segment_distance, signed_area};
use crate::*;
use std::f64::consts::PI;
fn rect(x: f64, y: f64, w: f64, h: f64) -> PlacedShape {
    Polygon::rectangle(x, y, w, h).unwrap().into()
}
fn merged(shapes: &[PlacedShape]) -> Topology {
    union(shapes, GeometryOptions::default()).unwrap()
}
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
fn near(a: f64, b: f64, tolerance: f64) {
    assert!(
        (a - b).abs() <= tolerance,
        "actual={a}, expected={b}, tolerance={tolerance}"
    );
}
#[test]
fn boolean_sets_have_concrete_areas_and_components() {
    let cases = [
        ("empty", vec![], 0., 0),
        (
            "duplicate",
            vec![rect(0., 0., 100., 100.), rect(0., 0., 100., 100.)],
            10000.,
            1,
        ),
        (
            "containment",
            vec![rect(0., 0., 100., 100.), rect(20., 20., 30., 30.)],
            10000.,
            1,
        ),
        (
            "separate",
            vec![rect(0., 0., 100., 100.), rect(200., 0., 40., 40.)],
            11600.,
            2,
        ),
        (
            "touching",
            vec![rect(0., 0., 100., 100.), rect(0., -50., 100., 50.)],
            15000.,
            1,
        ),
    ];
    for (name, shapes, area, components) in cases {
        let t = merged(&shapes);
        near(t.area(), area, 1e-5);
        assert_eq!(t.components(), components, "{name}");
    }
    for (op, area) in [
        (BooleanOp::Union, 15000.),
        (BooleanOp::Intersection, 5000.),
        (BooleanOp::Difference, 5000.),
        (BooleanOp::Xor, 10000.),
    ] {
        let t = boolean(
            &[rect(0., 0., 100., 100.)],
            &[rect(50., 0., 100., 100.)],
            op,
            GeometryOptions::default(),
        )
        .unwrap();
        near(t.area(), area, 1e-5);
    }
}
#[test]
fn holes_subtract_material_and_can_be_filled() {
    let shape = Polygon::rectangle(0., 0., 100., 100.)
        .unwrap()
        .with_hole(Polygon::rectangle(10., 10., 40., 40.).unwrap().exterior)
        .with_hole(Polygon::rectangle(30., 10., 40., 40.).unwrap().exterior);
    near(merged(&[shape.into()]).area(), 7600., 1e-5);
    let ring = Polygon::rectangle(0., 0., 100., 100.)
        .unwrap()
        .with_hole(Polygon::rectangle(25., 25., 50., 50.).unwrap().exterior);
    let t = merged(&[ring.clone().into()]);
    assert_eq!(t.rings().len(), 2);
    near(t.area(), 7500., 1e-5);
    let rounded = fillet(&t, CornerStyle::default()).unwrap();
    assert_eq!(
        rounded
            .corners
            .iter()
            .flatten()
            .filter(|c| c.concave)
            .count(),
        4
    );
    let t = merged(&[ring.into(), rect(25., 25., 50., 50.)]);
    assert_eq!(t.rings().len(), 1);
    near(t.area(), 10000., 1e-5);
}
#[test]
fn cleanup_roundtrip_never_loses_a_surviving_component() {
    for sliver in [0., 10.] {
        let options = GeometryOptions {
            epsilon: 0.00001,
            ..Default::default()
        };
        let t = boolean(
            &[rect(0., 0., 1., 1.), rect(10., 0., 1., 1.)],
            &[rect(sliver, 0., 0.999995, 1.)],
            BooleanOp::Difference,
            options,
        )
        .unwrap();
        assert_eq!(t.components(), 1);
        assert_eq!(t.polygons().len(), 1);
        let roundtrip = union(&t.placed_shapes(), options).unwrap();
        near(roundtrip.area(), 1., 1e-5);
        assert_eq!(roundtrip.components(), t.components());
    }
}
#[test]
fn fillets_make_expected_shoulders_and_remain_tangent_in_narrow_geometry() {
    for (x, width, expected) in [(0., 60., 1), (70., 60., 2), (0., 199.9, 1)] {
        let t = merged(&[rect(0., 50., 200., 100.), rect(x, 0., width, 50.)]);
        for (convex, concave) in [(0., 0.), (0., 32.), (24., 32.), (500., 500.)] {
            let shape = fillet(
                &t,
                CornerStyle {
                    convex_radius: convex,
                    concave_radius: concave,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                shape.corners.iter().flatten().filter(|c| c.concave).count(),
                expected
            );
            shape.path.validate(10000).unwrap();
            for c in shape.corners.iter().flatten() {
                if let Some(a) = c.arc {
                    near(c.start.distance(a.center), a.radius, 1e-6);
                    near(c.end.distance(a.center), a.radius, 1e-6);
                    near((c.vertex - c.start).dot(c.start - a.center), 0., 1e-6);
                }
                if width > 199. && c.concave {
                    assert!(c.effective_radius < 0.05);
                }
            }
        }
    }
    let notch = Polygon::new(vec![
        Point::new(0., 0.),
        Point::new(100., 0.),
        Point::new(100., 100.),
        Point::new(51., 100.),
        Point::new(51., 10.),
        Point::new(49., 10.),
        Point::new(49., 100.),
        Point::new(0., 100.),
    ]);
    let t = merged(&[notch.into()]);
    let rounded = fillet(
        &t,
        CornerStyle {
            convex_radius: 200.,
            concave_radius: 200.,
            ..Default::default()
        },
    )
    .unwrap();
    for (ri, ring) in rounded.corners.iter().enumerate() {
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
fn parallel_offsets_have_measured_thickness_and_correct_collapse() {
    for (w, h, r, d) in [
        (92., 170., 28., 12.),
        (100., 80., 8., 12.),
        (30., 30., 15., 3.),
    ] {
        let outer = rr(w, h, r);
        let child = outer.inset(d).unwrap().shape.unwrap();
        near(child.radius(), (r - d).max(0.), 1e-8);
        near(child.bounds().min.x, d, 1e-8);
        let source = outer.path().flatten(0.002, 50000).unwrap();
        let inset = inset_path(
            &outer.path(),
            d,
            OffsetOptions {
                flatten_tolerance: 0.01,
                ..Default::default()
            },
        )
        .unwrap();
        for p in inset.path.flatten(0.01, 50000).unwrap().iter().flatten() {
            near(boundary_distance(*p, &source), d, 0.05);
        }
        let out = outer.outset(d).unwrap();
        near(out.radius(), r + d, 1e-8);
    }
    let t = merged(&[rect(0., 50., 200., 100.), rect(0., 0., 60., 50.)]);
    let path = fillet(&t, CornerStyle::default()).unwrap().path;
    let original = path.flatten(0.002, 50000).unwrap();
    let inset = inset_path(&path, 6., OffsetOptions::default()).unwrap();
    for p in inset.path.flatten(0.05, 50000).unwrap().iter().flatten() {
        near(boundary_distance(*p, &original), 6., 0.1);
    }
    assert!(rr(20., 20., 4.).inset(10.).unwrap().shape.is_none());
    assert!(rr(80., 100., 8.).inset(12.).unwrap().corner_collapsed);
    assert_eq!(inset_arc_radius(28., 12., true).unwrap(), Some(40.));
    assert_eq!(inset_arc_radius(28., 12., false).unwrap(), Some(16.));
    near(inset_for_stroked_gap(12., 2., 4.).unwrap(), 15., 1e-8);
}
#[test]
fn erosion_splits_necks_expands_holes_and_disappears() {
    let t = merged(&[
        rect(0., 0., 40., 40.),
        rect(60., 0., 40., 40.),
        rect(40., 15., 20., 10.),
    ]);
    let inset = inset_path(&t.to_path(), 6., Default::default()).unwrap();
    assert_eq!(inset.topology.components(), 2);
    assert!(inset.counts_changed);
    assert!(
        inset_path(&rr(20., 20., 4.).path(), 20., Default::default())
            .unwrap()
            .topology
            .rings()
            .is_empty()
    );
    let p = Polygon::rectangle(0., 0., 100., 100.)
        .unwrap()
        .with_hole(Polygon::rectangle(40., 40., 20., 20.).unwrap().exterior);
    let t = merged(&[p.into()]);
    let inset = inset_path(&t.to_path(), 5., Default::default()).unwrap();
    assert!(
        inset
            .topology
            .rings()
            .iter()
            .find(|r| r.kind() == RingKind::Hole)
            .unwrap()
            .signed_area()
            .abs()
            > 400.
    );
    assert_eq!(
        inset,
        inset_path(&t.to_path(), 5., Default::default()).unwrap()
    );
}
#[test]
fn invalid_inputs_and_work_budgets_are_errors() {
    let path = rr(100., 100., 20.).path();
    for value in [f64::NAN, f64::INFINITY, -1.] {
        assert!(inset_path(&path, value, Default::default()).is_err());
        assert!(fillet(
            &merged(&[rect(0., 0., 10., 10.)]),
            CornerStyle {
                convex_radius: value,
                ..Default::default()
            }
        )
        .is_err());
    }
    assert!(path.flatten(0.00001, 10).is_err());
    assert!(inset_path(
        &path,
        1000.,
        OffsetOptions {
            flatten_tolerance: 0.001,
            ..Default::default()
        }
    )
    .is_err());
    let crossing = Polygon::new(vec![
        Point::new(0., 0.),
        Point::new(10., 10.),
        Point::new(0., 10.),
        Point::new(10., 0.),
    ]);
    assert!(union(&[crossing.into()], Default::default()).is_err());
    assert!(union(
        &[rect(0., 0., 10., 10.)],
        GeometryOptions {
            max_vertices: 3,
            ..Default::default()
        }
    )
    .is_err());
    let bad = Path {
        commands: vec![
            PathCommand::ArcTo(Arc {
                center: Point::ZERO,
                radius: 1.,
                start_angle: 0.,
                sweep: PI,
                to: Point::ZERO,
            }),
            PathCommand::Close,
        ],
    };
    assert!(bad.to_svg_data().is_err());
    assert!(bad.flatten(0.1, 100).is_err());
}
#[test]
fn transforms_and_capsules_preserve_area_and_valid_arcs() {
    let a = rect(0., 0., 100., 100.);
    let b = a
        .clone()
        .transformed(Affine::scale(-1., 1.).then(Affine::translation(100., 0.)));
    near(merged(&[a, b]).area(), 10000., 1e-5);
    for i in 0..32 {
        let angle = i as f64 * PI / 16.;
        let t = merged(&[rect(0., 0., 100., 40.).transformed(Affine::rotation(angle))]);
        near(t.area(), 4000., 1e-4);
        fillet(&t, Default::default())
            .unwrap()
            .path
            .validate(1000)
            .unwrap();
    }
    let path = Path::capsule(40., 100.).unwrap();
    let expected = 40. * 60. + PI * 20. * 20.;
    for angle in [0., 0.4, PI] {
        let p = path.rigid_transform(Point::new(100., -50.), angle).unwrap();
        let rings = p.flatten(0.002, 10000).unwrap();
        near(signed_area(&rings[0]), expected, 0.3);
    }
}
