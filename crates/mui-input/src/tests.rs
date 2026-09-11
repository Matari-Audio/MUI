use super::*;
use mui_geometry::PathCommand;

fn square(x: f64, y: f64, w: f64, h: f64) -> Vec<PathCommand> {
    vec![
        PathCommand::MoveTo(Point::new(x, y)),
        PathCommand::LineTo(Point::new(x + w, y)),
        PathCommand::LineTo(Point::new(x + w, y + h)),
        PathCommand::LineTo(Point::new(x, y + h)),
        PathCommand::Close,
    ]
}

fn path(commands: Vec<PathCommand>) -> Path {
    Path { commands }
}

/// An outer ring with a counter inside it, wound the *other* way -- which is
/// how `mui-geometry` emits a hole and how a typeface draws one.
fn ring() -> Path {
    let mut c = square(0., 0., 100., 100.);
    c.extend(reversed(square(30., 30., 40., 40.)));
    path(c)
}

/// Same points, opposite direction. `Close` has no direction, so only the
/// interior vertices move.
fn reversed(mut c: Vec<PathCommand>) -> Vec<PathCommand> {
    let close = matches!(c.last(), Some(PathCommand::Close));
    if close {
        c.pop();
    }
    c.reverse();
    let mut out: Vec<PathCommand> = c
        .into_iter()
        .enumerate()
        .map(|(i, cmd)| {
            let p = match cmd {
                PathCommand::MoveTo(p) | PathCommand::LineTo(p) => p,
                other => return other,
            };
            if i == 0 {
                PathCommand::MoveTo(p)
            } else {
                PathCommand::LineTo(p)
            }
        })
        .collect();
    if close {
        out.push(PathCommand::Close);
    }
    out
}

fn at(x: f64, y: f64) -> PointerInput {
    PointerInput {
        pos: Some(Point::new(x, y)),
        primary_down: false,
    }
}

fn down(x: f64, y: f64) -> PointerInput {
    PointerInput {
        primary_down: true,
        ..at(x, y)
    }
}

// ---- geometry ----------------------------------------------------------

#[test]
fn a_point_inside_a_counter_is_not_a_hit() {
    let mut hit = Hit::default();
    hit.push("ring", &ring()).unwrap();
    assert_eq!(hit.at(Point::new(10., 50.)), Some("ring"), "on the band");
    assert_eq!(hit.at(Point::new(50., 50.)), None, "inside the hole");
    assert_eq!(hit.at(Point::new(150., 50.)), None, "outside entirely");
}

#[test]
fn same_wound_rings_stack_rather_than_cancel() {
    // Two contours wound the same way -- what you get by concatenating glyph
    // outlines. Even-odd erased the overlap; non-zero fills it, which is the
    // whole reason `Hit::at` counts winding instead of parity.
    let mut c = square(0., 0., 100., 100.);
    c.extend(square(30., 30., 40., 40.));
    let mut hit = Hit::default();
    hit.push("stacked", &path(c)).unwrap();
    assert_eq!(hit.at(Point::new(50., 50.)), Some("stacked"), "the overlap");
    assert_eq!(hit.at(Point::new(10., 50.)), Some("stacked"), "outer only");
}

#[test]
fn the_corner_of_a_rounded_shape_is_outside_its_bounding_box_corner() {
    let mut hit = Hit::default();
    // A capsule is 48 wide, so its ends are half-circles of radius 24.
    hit.push("pill", &Path::capsule(48., 120.).unwrap())
        .unwrap();
    let bounds = Path::capsule(48., 120.)
        .unwrap()
        .flatten(0.01, 250_000)
        .unwrap();
    let corner = bounds
        .iter()
        .flatten()
        .fold(Point::new(f64::MAX, f64::MAX), |a, p| {
            Point::new(a.x.min(p.x), a.y.min(p.y))
        });
    // Just inside the bounding box's top-left corner, but well outside the
    // rounded end. A rect hit test would claim this; the real geometry does not.
    let just_inside = Point::new(corner.x + 1., corner.y + 1.);
    assert_eq!(
        hit.at(just_inside),
        None,
        "bounding box corner claimed a hit"
    );
    // The middle of the same shape does hit, so the test is not vacuous.
    let mid = Point::new(corner.x + 24., corner.y + 60.);
    assert_eq!(hit.at(mid), Some("pill"));
}

#[test]
fn the_last_target_pushed_is_on_top() {
    let mut hit = Hit::default();
    hit.push("under", &path(square(0., 0., 100., 100.)))
        .unwrap();
    hit.push("over", &path(square(50., 50., 100., 100.)))
        .unwrap();
    assert_eq!(hit.at(Point::new(75., 75.)), Some("over"), "overlap");
    assert_eq!(hit.at(Point::new(25., 25.)), Some("under"), "only under");
}

#[test]
fn malformed_geometry_fails_at_registration_not_silently() {
    let mut broken = Path::capsule(48., 120.).unwrap();
    let PathCommand::ArcTo(arc) = &mut broken.commands[1] else {
        panic!("expected an arc");
    };
    arc.radius *= 3.;
    assert!(Hit::default().push("bad", &broken).is_err());
}

// ---- interaction -------------------------------------------------------

fn one_square() -> Hit {
    let mut hit = Hit::default();
    hit.push("a", &path(square(0., 0., 100., 100.))).unwrap();
    hit.push("b", &path(square(200., 0., 100., 100.))).unwrap();
    hit
}

#[test]
fn press_and_release_on_the_same_target_is_a_click() {
    let (hit, mut ui) = (one_square(), Interaction::new());
    ui.update(&hit, at(50., 50.));
    assert!(ui.get("a").hovered && !ui.get("a").clicked);

    ui.update(&hit, down(50., 50.));
    let r = ui.get("a");
    assert!(r.pressed && r.held && !r.clicked, "{r:?}");

    ui.update(&hit, at(50., 50.));
    let r = ui.get("a");
    assert!(r.clicked && r.released && !r.held, "{r:?}");

    // And it is edge-triggered: the next frame is quiet.
    ui.update(&hit, at(50., 50.));
    assert!(!ui.get("a").clicked && !ui.get("a").released);
}

#[test]
fn a_press_captures_its_target_even_when_the_pointer_leaves() {
    let (hit, mut ui) = (one_square(), Interaction::new());
    ui.update(&hit, down(50., 50.));
    // Straight off the shape, and over a different target.
    ui.update(&hit, down(250., 50.));
    assert!(ui.get("a").held, "capture lost when the pointer left");
    assert!(!ui.get("b").hovered, "another target hovered mid-gesture");
    assert_eq!(ui.held(), Some("a"));
}

#[test]
fn releasing_away_from_the_target_releases_but_does_not_click() {
    let (hit, mut ui) = (one_square(), Interaction::new());
    ui.update(&hit, down(50., 50.));
    ui.update(&hit, down(250., 50.));
    ui.update(&hit, at(250., 50.));
    let r = ui.get("a");
    assert!(r.released && !r.clicked, "{r:?}");
}

#[test]
fn moving_past_the_threshold_turns_a_click_into_a_drag() {
    let (hit, mut ui) = (one_square(), Interaction::new());
    ui.update(&hit, down(50., 50.));
    assert!(!ui.get("a").dragged, "a drag before any movement");

    ui.update(&hit, down(50. + DRAG_THRESHOLD, 50.));
    assert!(!ui.get("a").dragged, "threshold is exclusive");

    ui.update(&hit, down(50. + DRAG_THRESHOLD + 0.1, 50.));
    assert!(ui.get("a").dragged);

    ui.update(&hit, at(50. + DRAG_THRESHOLD + 0.1, 50.));
    assert!(!ui.get("a").clicked, "a drag reported a click on release");
}

#[test]
fn a_wobble_that_returns_home_is_still_a_click() {
    let (hit, mut ui) = (one_square(), Interaction::new());
    ui.update(&hit, down(50., 50.));
    for x in [51., 52., 51., 50.] {
        ui.update(&hit, down(x, 50.));
    }
    ui.update(&hit, at(50., 50.));
    assert!(ui.get("a").clicked, "accumulated travel counted as a drag");
}

#[test]
fn drag_delta_is_per_frame_and_zero_when_not_dragging() {
    let (hit, mut ui) = (one_square(), Interaction::new());
    ui.update(&hit, down(50., 50.));
    ui.update(&hit, down(90., 50.)); // crosses the threshold
    assert_eq!(ui.get("a").drag_delta, Point::new(40., 0.));

    ui.update(&hit, down(95., 60.));
    assert_eq!(
        ui.get("a").drag_delta,
        Point::new(5., 10.),
        "delta measured from the press instead of the last frame"
    );

    assert_eq!(ui.get("b").drag_delta, Point::new(0., 0.));
}

#[test]
fn a_press_on_empty_space_captures_nothing() {
    let (hit, mut ui) = (one_square(), Interaction::new());
    ui.update(&hit, down(150., 50.));
    assert_eq!(ui.held(), None);
    // And a later release over a target must not invent a click.
    ui.update(&hit, at(50., 50.));
    assert!(!ui.get("a").clicked);
}

#[test]
fn losing_the_pointer_mid_gesture_keeps_the_capture() {
    let (hit, mut ui) = (one_square(), Interaction::new());
    ui.update(&hit, down(50., 50.));
    ui.update(
        &hit,
        PointerInput {
            pos: None,
            primary_down: true,
        },
    );
    assert!(ui.get("a").held, "capture dropped when the pointer left");
    assert_eq!(ui.hovered(), None);
    assert_eq!(ui.get("a").drag_delta, Point::new(0., 0.));
}

#[test]
fn an_unknown_id_is_inert_rather_than_a_panic() {
    let (hit, mut ui) = (one_square(), Interaction::new());
    ui.update(&hit, down(50., 50.));
    assert_eq!(ui.get("gone"), Response::default());
}

#[test]
fn a_larger_threshold_is_honoured() {
    let hit = one_square();
    let mut ui = Interaction::new().with_drag_threshold(20.);
    ui.update(&hit, down(50., 50.));
    ui.update(&hit, down(60., 50.));
    assert!(
        !ui.get("a").dragged,
        "10 units dragged under a 20 threshold"
    );
    ui.update(&hit, down(75., 50.));
    assert!(ui.get("a").dragged);
}
