use super::*;
use kurbo::{ParamCurve as _, PathEl};
use mui_geometry::{Path, PathCommand};

fn capsule() -> Path {
    Path::capsule(48., 120.).unwrap()
}

#[test]
fn a_capsule_keeps_its_extent_through_the_conversion() {
    let path = capsule();
    let flat = path.flatten(0.01, 250_000).unwrap();
    let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    for p in flat.iter().flatten() {
        lo = (lo.0.min(p.x), lo.1.min(p.y));
        hi = (hi.0.max(p.x), hi.1.max(p.y));
    }
    let b = bez_path(&path, ARC_TOLERANCE).unwrap().bounding_box();
    for (a, b) in [(lo.0, b.x0), (lo.1, b.y0), (hi.0, b.x1), (hi.1, b.y1)] {
        assert!((a - b).abs() < 0.05, "extent drifted: {a} vs {b}");
    }
}

#[test]
fn an_arc_lands_on_the_tangent_point_mui_recorded() {
    let path = capsule();
    let bez = bez_path(&path, ARC_TOLERANCE).unwrap();
    for command in &path.commands {
        let PathCommand::ArcTo(arc) = *command else {
            continue;
        };
        let hit = bez.segments().any(|s| {
            let e = s.eval(1.);
            (e.x - arc.to.x).hypot(e.y - arc.to.y) < 1e-9
        });
        assert!(hit, "no segment ends at {:?}", arc.to);
    }
}

#[test]
fn arcs_become_curves_not_a_polyline() {
    let bez = bez_path(&capsule(), ARC_TOLERANCE).unwrap();
    assert!(
        bez.elements()
            .iter()
            .any(|e| matches!(e, PathEl::CurveTo(..))),
        "a rounded shape flattened to line segments"
    );
}

#[test]
fn a_nonsense_tolerance_is_refused_rather_than_hung_on() {
    for t in [0., -1., f64::NAN, f64::INFINITY] {
        assert!(bez_path(&capsule(), t).is_err(), "accepted tolerance {t}");
    }
}

#[test]
fn a_malformed_arc_is_caught_at_the_seam() {
    let mut path = capsule();
    let PathCommand::ArcTo(arc) = &mut path.commands[1] else {
        panic!("expected the capsule's first arc at index 1");
    };
    arc.radius *= 2.;
    assert!(bez_path(&path, ARC_TOLERANCE).is_err());
}
