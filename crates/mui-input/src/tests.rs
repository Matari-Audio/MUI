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
        ..PointerInput::default()
    }
}

fn down(x: f64, y: f64) -> PointerInput {
    PointerInput {
        buttons: Buttons::PRIMARY,
        ..at(x, y)
    }
}

fn with(mut p: PointerInput, b: Button, mods: Mods) -> PointerInput {
    p.buttons = p.buttons.set(Button::Primary, false).set(b, true);
    p.mods = mods;
    p
}

fn shift() -> Mods {
    Mods {
        shift: true,
        ..Mods::default()
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
    assert_eq!(ui.get("a").drag_delta, Vec2::new(40., 0.));

    ui.update(&hit, down(95., 60.));
    assert_eq!(
        ui.get("a").drag_delta,
        Vec2::new(5., 10.),
        "delta measured from the press instead of the last frame"
    );

    assert_eq!(ui.get("b").drag_delta, Vec2::ZERO);
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
            buttons: Buttons::PRIMARY,
            ..PointerInput::default()
        },
    );
    assert!(ui.get("a").held, "capture dropped when the pointer left");
    assert_eq!(ui.hovered(), None);
    assert_eq!(ui.get("a").drag_delta, Vec2::ZERO);
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

#[test]
fn default_and_new_use_the_normal_drag_threshold() {
    assert_eq!(Interaction::default().threshold, DRAG_THRESHOLD);
    assert_eq!(Interaction::new().threshold, DRAG_THRESHOLD);
}

#[test]
fn invalid_drag_thresholds_are_rejected() {
    for threshold in [-1.0, f64::NEG_INFINITY, f64::INFINITY, f64::NAN] {
        assert!(
            std::panic::catch_unwind(|| Interaction::new().with_drag_threshold(threshold)).is_err(),
            "invalid threshold {threshold:?} was accepted"
        );
    }
}

#[test]
fn zero_threshold_turns_any_nonzero_move_into_a_drag() {
    let (hit, mut ui) = (one_square(), Interaction::new().with_drag_threshold(0.));
    ui.update(&hit, down(50., 50.));
    assert!(!ui.get("a").dragged, "a stationary press is not a drag");

    ui.update(&hit, down(50.1, 50.));
    assert!(ui.get("a").dragged);
}

#[test]
fn cancel_clears_capture_and_edges_but_preserves_threshold() {
    let (hit, mut ui) = (one_square(), Interaction::new().with_drag_threshold(20.));
    ui.update(&hit, down(50., 50.));
    assert!(ui.get("a").pressed && ui.get("a").held);

    ui.cancel();
    assert_eq!(ui.threshold, 20.);
    assert_eq!(ui.get("a"), Response::default());
    assert_eq!(ui.held(), None);
    assert_eq!(ui.hovered(), None);

    ui.update(&hit, at(50., 50.));
    let response = ui.get("a");
    assert!(response.hovered);
    assert!(!response.pressed && !response.released && !response.clicked);
    assert!(!response.held && !response.dragged);
    assert_eq!(response.drag_delta, Vec2::ZERO);

    ui.update(&hit, down(50., 50.));
    ui.update(&hit, down(60., 50.));
    assert!(
        !ui.get("a").dragged,
        "cancel changed the configured threshold"
    );
}

#[test]
fn a_clip_rejects_a_hit_the_renderer_would_not_draw() {
    let mut hit = Hit::default();
    hit.push_clipped(
        "row",
        &path(square(0., 0., 100., 100.)),
        Some(mui_geometry::Rect::new(0., 0., 100., 50.)),
    )
    .unwrap();
    assert_eq!(hit.at(Point::new(50., 25.)), Some("row"), "inside the clip");
    assert_eq!(hit.at(Point::new(50., 75.)), None, "clipped away");
}

#[test]
fn exact_rounded_and_nested_clips_reject_corners() {
    let outer = mui_geometry::RoundedRect::new(mui_geometry::Rect::new(0., 0., 100., 100.), 20.)
        .unwrap()
        .path();
    let inner = mui_geometry::RoundedRect::new(mui_geometry::Rect::new(20., 20., 80., 80.), 15.)
        .unwrap()
        .path();
    let mut hit = Hit::default();
    hit.push_clipped_paths(
        "target",
        &path(square(10., 10., 80., 80.)),
        Some(mui_geometry::Rect::new(0., 0., 100., 100.)),
        Some(&[outer.into(), inner.into()]),
    )
    .unwrap();
    assert_eq!(hit.at(Point::new(21., 21.)), None, "inner rounded clip");
    assert_eq!(
        hit.at(Point::new(30., 30.)),
        Some("target"),
        "inside both clips"
    );
}

#[test]
fn a_drag_released_over_another_target_is_a_drop() {
    let mut hit = Hit::default();
    hit.push("a", &path(square(0., 0., 40., 40.))).unwrap();
    hit.push("b", &path(square(60., 0., 40., 40.))).unwrap();
    let mut i = Interaction::new();
    i.update(&hit, down(20., 20.));
    i.update(&hit, down(80., 20.));
    assert!(i.get("b").drop_target, "b is under the dragged pointer");
    assert!(!i.get("a").drop_target, "the source is not its own target");
    i.update(&hit, at(80., 20.));
    assert_eq!(i.dropped(), Some(("a", "b")));
    assert!(i.get("b").dropped_on);
    assert!(!i.get("a").clicked, "a drag is never a click");
    i.update(&hit, at(80., 20.));
    assert_eq!(i.dropped(), None, "one frame only");
}

// ---- buttons and modifiers ---------------------------------------------

#[test]
fn a_secondary_press_captures_and_names_its_button() {
    let (hit, mut i) = (one_square(), Interaction::new());
    i.update(
        &hit,
        with(down(50., 50.), Button::Secondary, Mods::default()),
    );
    let r = i.get("a");
    assert!(r.pressed && r.held);
    assert_eq!(r.button, Some(Button::Secondary));
    assert_eq!(i.held_button(), Some(Button::Secondary));
    i.update(&hit, at(50., 50.));
    let r = i.get("a");
    assert!(
        r.clicked_with(Button::Secondary),
        "a right click is a click"
    );
    assert!(!r.clicked_with(Button::Primary), "but not a left one");
}

#[test]
fn a_second_button_pressed_mid_gesture_does_not_steal_the_capture() {
    let (hit, mut i) = (one_square(), Interaction::new());
    i.update(&hit, down(50., 50.));
    let both = PointerInput {
        buttons: Buttons::PRIMARY.set(Button::Secondary, true),
        ..down(50., 50.)
    };
    i.update(&hit, both);
    assert_eq!(i.get("a").button, Some(Button::Primary));
    // Letting go of the *other* button ends nothing.
    i.update(&hit, down(50., 50.));
    assert!(i.get("a").held);
    i.update(&hit, at(50., 50.));
    assert!(i.get("a").released);
}

#[test]
fn modifiers_are_reported_at_the_press_and_now() {
    let (hit, mut i) = (one_square(), Interaction::new());
    i.update(&hit, with(down(50., 50.), Button::Primary, shift()));
    i.update(&hit, with(down(60., 50.), Button::Primary, Mods::default()));
    let r = i.get("a");
    assert!(r.press_mods.shift, "the press was a Shift press");
    assert!(!r.mods.shift, "and Shift is gone now");
    assert_eq!(
        i.get("b").press_mods,
        Mods::default(),
        "a target with no gesture claims no modifiers"
    );
}

#[test]
fn shift_drags_fine_and_a_drag_locks_to_its_longest_axis() {
    let (hit, mut i) = (one_square(), Interaction::new());
    i.update(&hit, with(down(50., 50.), Button::Primary, shift()));
    i.update(&hit, with(down(50., 90.), Button::Primary, shift()));
    let r = i.get("a");
    assert_eq!(r.drag_delta.y, 40.0, "the raw delta is untouched");
    assert_eq!(r.drag_fine(FINE_DRAG).y, 4.0, "Shift is the fine modifier");
    assert_eq!(r.drag_total, Vec2::new(0., 40.), "travel since the press");
    assert_eq!(r.drag_axis(), Some(Axis::Y));
    i.update(
        &hit,
        with(down(150., 90.), Button::Primary, Mods::default()),
    );
    let r = i.get("a");
    assert_eq!(r.drag_fine(FINE_DRAG).x, 100.0, "coarse again mid-drag");
    assert_eq!(r.drag_axis(), Some(Axis::X), "x now dominates");
}

#[test]
fn a_capture_follows_its_target_through_a_rename() {
    let mut i = Interaction::new();
    i.update(&one_square(), down(10., 10.));
    assert!(i.get("a").held);
    // The runtime reordered: what was "a" is "slot/1" now.
    i.rename(&|k| (k == "a").then(|| "slot/1".to_owned()));
    assert!(i.get("slot/1").held && !i.get("a").held);
    assert_eq!(i.held(), Some("slot/1"));
}
