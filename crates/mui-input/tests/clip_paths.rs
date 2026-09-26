use mui_geometry::{Bounds, Path, Point, RoundedRect};
use mui_input::Hit;

#[test]
fn all_clips_must_accept_the_point() {
    let square = RoundedRect::new(Bounds::new(0.0, 0.0, 100.0, 100.0), 0.0)
        .unwrap()
        .path();
    let circle = RoundedRect::new(Bounds::new(0.0, 0.0, 100.0, 100.0), 50.0)
        .unwrap()
        .path();
    let inner = RoundedRect::new(Bounds::new(0.0, 0.0, 60.0, 100.0), 0.0)
        .unwrap()
        .path();
    let mut hit = Hit::default();
    hit.push_tagged_paths(
        "control",
        "body",
        &square,
        None,
        Some(&[circle.into(), inner.into()]),
    )
    .unwrap();
    assert_eq!(hit.at(Point::new(2.0, 2.0)), None);
    assert_eq!(hit.at(Point::new(80.0, 50.0)), None);
    assert_eq!(
        hit.at_tagged(Point::new(50.0, 50.0)),
        Some(("control", Some("body")))
    );
}

#[test]
fn a_clip_hole_does_not_respond() {
    let outer = [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)];
    let hole = [(30.0, 30.0), (30.0, 70.0), (70.0, 70.0), (70.0, 30.0)];
    let square = Path::polyline(outer.map(|(x, y)| Point::new(x, y)), true);
    let mut ring = square.clone();
    ring.commands
        .extend(Path::polyline(hole.map(|(x, y)| Point::new(x, y)), true).commands);
    let mut hit = Hit::default();
    hit.push_clipped_paths("control", &square, None, Some(&[ring.into()]))
        .unwrap();
    assert_eq!(hit.at(Point::new(50.0, 50.0)), None);
    assert_eq!(hit.at(Point::new(10.0, 10.0)), Some("control"));
}

#[test]
fn malformed_clip_cannot_publish_a_partial_target() {
    let square = RoundedRect::new(Bounds::new(0.0, 0.0, 10.0, 10.0), 0.0)
        .unwrap()
        .path();
    let mut invalid = square.clone();
    invalid.commands[0] = mui_geometry::PathCommand::MoveTo(Point::new(f64::NAN, 0.0));
    let mut hit = Hit::default();
    assert!(
        hit.push_clipped_paths("bad", &square, None, Some(&[invalid.into()]))
            .is_err()
    );
    assert!(hit.is_empty());
}

#[test]
fn nonfinite_pointer_coordinates_are_inert() {
    let mut hit = Hit::default();
    hit.push(
        "box",
        &RoundedRect::new(Bounds::new(0.0, 0.0, 10.0, 10.0), 0.0)
            .unwrap()
            .path(),
    )
    .unwrap();
    assert_eq!(hit.at(Point::new(f64::NAN, 5.0)), None);
    assert_eq!(hit.at(Point::new(5.0, f64::INFINITY)), None);
}
