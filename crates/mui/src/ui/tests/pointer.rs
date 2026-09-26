use super::*;

#[test]
fn a_move_is_inert_only_on_the_same_target_away_from_raw_pointer_readers() {
    let tree = || {
        row([
            block(50., 50.).fill(Role::Field).id("src"),
            block(50., 50.).fill(Role::Field).id("dst"),
            block(50., 50.).fill(Role::Field).tracks_pointer().id("xy"),
        ])
    };
    let mut ui = Ui::new(Theme::DEFAULT);
    for _ in 0..2 {
        ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
    }
    assert!(ui.inert(&at(20., 20., false).into()), "same target");
    assert_eq!(
        ui.local("src"),
        Some(Point::new(20., 20.)),
        "taken as the pointer"
    );
    assert!(!ui.inert(&at(60., 10., false).into()), "a new target");
    assert!(!ui.inert(&at(20., 20., true).into()), "a button");
    let wheel = Input {
        wheel: Vec2::new(0., 1.),
        ..at(20., 20., false).into()
    };
    assert!(!ui.inert(&wheel), "the wheel");

    ui.frame(tree(), None, at(110., 10., false), 0.016).unwrap();
    ui.frame(tree(), None, at(110., 10., false), 0.016).unwrap();
    assert!(
        !ui.inert(&at(120., 20., false).into()),
        "a tracks_pointer node"
    );

    ui.frame(tree(), None, at(10., 10., true), 0.016).unwrap();
    ui.frame(tree(), None, at(10., 10., true), 0.016).unwrap();
    assert!(!ui.inert(&at(12., 12., true).into()), "a held capture");
}

#[test]
fn a_new_hover_target_is_owed_one_more_tree() {
    // Plain surfaces: no hover spring to keep frames coming.
    let tree = || row([block(50., 50.).id("a"), block(50., 50.).id("b")]);
    let mut ui = Ui::new(Theme::DEFAULT);
    ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
    ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
    let f = ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
    assert!(!f.animating, "resting on one target");
    let f = ui.frame(tree(), None, at(60., 10., false), 0.016).unwrap();
    assert!(f.animating, "the tree just built still saw `a` hovered");
    let f = ui.frame(tree(), None, at(60., 10., false), 0.016).unwrap();
    assert!(!f.animating, "and one tree later it has caught up");
}

#[test]
fn an_inert_move_keeps_the_tip_counting_and_a_release_is_not_inert() {
    let tree = || block(40., 40.).fill(Role::Raised).tip("why").id("b");
    let mut ui = Ui::new(Theme::DEFAULT);
    ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
    ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
    assert!(ui.inert(&at(12., 12., false).into()), "the tip counts on");
    let f = ui.frame(tree(), None, at(12., 12., false), 0.6).unwrap();
    assert!(f.tip.is_some());
    ui.frame(tree(), None, at(12., 12., true), 0.016).unwrap();
    ui.frame(tree(), None, at(12., 12., false), 0.016).unwrap();
    assert!(
        !ui.inert(&at(13., 12., false).into()),
        "the release is owed a tree"
    );
}

#[test]
fn a_tip_comes_due_after_half_a_second_of_hover() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let tree = || block(40., 40.).fill(Role::Raised).tip("why").id("b");
    // In a window, not hugging: a float is kept inside the box it floats
    // in, so the room under the surface has to exist.
    let win = || Some(Size::new(240., 300.));
    ui.frame(tree(), win(), at(10., 10., false), 0.016).unwrap();
    let f = ui.frame(tree(), win(), at(10., 10., false), 0.4).unwrap();
    assert!(f.tip.is_none(), "the pointer has not rested long enough");
    assert!(f.animating, "the tooltip deadline keeps idle hosts awake");
    let f = ui.frame(tree(), win(), at(10., 10., false), 0.6).unwrap();
    let (t, at) = f.tip.clone().expect("due");
    assert_eq!(t, "why");
    assert!(at.y > 40., "below the surface");
    assert!(!f.animating, "a settled tooltip does not spin the host");
    assert!(
        f.scene.surfaces().any(|s| s.frame.y > 40.),
        "and floated into the scene"
    );
    let f = ui
        .frame(tree(), win(), PointerInput::default(), 0.016)
        .unwrap();
    assert!(f.tip.is_none(), "gone when the pointer leaves");
}

#[test]
fn a_tip_lands_where_it_was_measured_even_under_a_padded_root() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let tree = || col([block(40., 40.).fill(Role::Raised).tip("why").id("b")]).pad(L);
    let win = || Some(Size::new(240., 300.));
    let p = at(110., 30., false);
    ui.frame(tree(), win(), p, 0.016).unwrap();
    ui.frame(tree(), win(), p, 0.016).unwrap();
    let f = ui.frame(tree(), win(), p, 0.6).unwrap();
    let (_, at) = f.tip.clone().expect("due");
    let tip = f
        .scene
        .surfaces()
        .find(|s| s.frame.y == at.y)
        .expect("the tip sits where it was measured, not padded away");
    assert_eq!((tip.frame.x, tip.frame.y), (at.x, at.y));
}

#[test]
fn a_press_and_its_release_bracket_the_gesture() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let tree = || block(40., 40.).fill(Role::Raised).id("b");
    ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
    let f = ui.frame(tree(), None, at(10., 10., true), 0.016).unwrap();
    assert_eq!(f.edits, vec![("b".to_owned(), Edit::Begin)]);
    assert_eq!(ui.edit("b"), Some(Edit::Begin));
    let f = ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
    assert_eq!(f.edits, vec![("b".to_owned(), Edit::End)]);
    assert_eq!(ui.edit("b"), Some(Edit::End));
    let f = ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
    assert!(f.edits.is_empty());
}

/// A look declared beside the resting one, applied because the runtime
/// knows which node the pointer is on -- no `ui.state` in the tree.
#[test]
fn a_declared_hover_style_is_applied_while_hovered() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let tree = || {
        block(40., 40.)
            .fill(Role::Raised)
            .on(State::Hover, |s| s.radius(3.))
            .id("b")
    };
    let corner = |ui: &mut Ui, p| {
        ui.frame(tree(), None, p, 0.016)
            .unwrap()
            .scene
            .paint
            .iter()
            .find_map(|p| p.rect.map(mui_geometry::RoundedRect::radius))
            .expect("the box paints a rounded rect")
    };
    let cold = corner(&mut ui, PointerInput::default());
    // Hover long enough that the spring passes the halfway mark.
    let mut warm = cold;
    for _ in 0..12 {
        warm = corner(&mut ui, at(10., 10., false));
    }
    assert_ne!(
        cold, 3.,
        "the resting radius is the theme's, not the hover one"
    );
    assert_eq!(warm, 3., "hovered, the declared radius is what paints");
}

/// A structural id remains a hit target for gestures, but it does not
/// warm or animate. The semantic child still gets the default look.
#[test]
fn a_named_layout_surface_stays_cold_while_an_interactive_child_warms() {
    let tree = || {
        col([block(40., 40.)
            .fill(Role::Raised)
            .a11y(A11y::Button)
            .focusable()
            .id("child")])
        .size(100., 100.)
        .fill(Role::Background)
        .id("panel")
    };

    let mut ui = Ui::new(Theme::DEFAULT);
    let cold = ui
        .frame(tree(), None, PointerInput::default(), 0.016)
        .unwrap();
    let panel_cold = paint_color(&cold, "panel");
    let child_cold = paint_color(&cold, "child");
    let mut child_warm = child_cold;
    for _ in 0..12 {
        let frame = ui.frame(tree(), None, at(40., 10., false), 0.016).unwrap();
        child_warm = paint_color(&frame, "child");
    }
    assert!(ui.get("child").hovered, "the child is the topmost target");
    assert!(!ui.get("panel").hovered, "the parent is only underneath");
    assert_ne!(child_warm, child_cold, "the control warms");
    // The child loop's final frame also proves that the parent never
    // received an automatic tint while it sat underneath the child.
    let parent_warm = ui
        .scene()
        .map(|scene| {
            scene
                .paint
                .iter()
                .find(|p| &*p.key == "panel")
                .expect("panel paint")
                .paint
                .solid()
        })
        .expect("scene");
    assert_eq!(parent_warm, panel_cold, "the layout stays cold");

    // Resting on the empty part of the named parent remains interactive
    // for hit testing, without starting a useless animation loop.
    let mut ui = Ui::new(Theme::DEFAULT);
    ui.frame(tree(), None, PointerInput::default(), 0.016)
        .unwrap();
    let (panel_warm, animating) = {
        let frame = ui.frame(tree(), None, at(80., 80., false), 0.016).unwrap();
        (paint_color(&frame, "panel"), frame.animating)
    };
    assert!(ui.get("panel").hovered, "the parent still receives the hit");
    assert_eq!(panel_warm, panel_cold);
    assert!(animating, "the new hover is owed one tree");
    let frame = ui.frame(tree(), None, at(80., 80., false), 0.016).unwrap();
    assert!(!frame.animating, "a structural hover has no spring");
}

/// A child rectangle can extend into the transparent corner of a rounded
/// clipped parent, but that corner was never drawn and must not hit.
#[test]
fn a_rounded_clip_rejects_a_child_corner() {
    let tree = || {
        stack([block(40., 40.)
            .fill(Role::Primary)
            .anchor(Align::Start, Align::Start)
            .id("child")])
        .size(100., 100.)
        .fill(Role::Field)
        .radius(20.)
        .clip()
        .id("panel")
    };
    let mut ui = Ui::new(Theme::DEFAULT);
    ui.frame(tree(), None, PointerInput::default(), 0.016)
        .unwrap();
    ui.frame(tree(), None, at(5., 5., false), 0.016).unwrap();
    assert!(
        !ui.get("child").hovered,
        "the child is outside the parent's rounded contour"
    );
    ui.frame(tree(), None, at(25., 25., false), 0.016).unwrap();
    assert!(ui.get("child").hovered, "the child is inside the clip");
}

/// Every nested clipped ancestor contributes its contour. A point in the
/// outer rounded parent but outside the inner one cannot reach a child.
#[test]
fn nested_rounded_clips_intersect_for_hit_testing() {
    let tree = || {
        stack([stack([block(80., 80.)
            .fill(Role::Primary)
            .centered()
            .id("target")])
        .size(60., 60.)
        .fill(Role::Raised)
        .radius(15.)
        .clip()
        .centered()
        .id("inner")])
        .size(100., 100.)
        .fill(Role::Field)
        .radius(20.)
        .clip()
        .id("outer")
    };
    let mut ui = Ui::new(Theme::DEFAULT);
    ui.frame(tree(), None, PointerInput::default(), 0.016)
        .unwrap();
    ui.frame(tree(), None, at(21., 21., false), 0.016).unwrap();
    assert!(
        !ui.get("target").hovered,
        "the outer clip contains this point but the inner clip does not"
    );
    ui.frame(tree(), None, at(30., 30., false), 0.016).unwrap();
    assert!(ui.get("target").hovered, "the point is inside both clips");
}

/// An explicit hover look owns its fill channel. The automatic semantic
/// fallback must not remap that already-resolved color a second time.
#[test]
fn an_explicit_hover_look_is_applied_once() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let tree = || {
        block(40., 40.)
            .fill(Role::Field)
            .a11y(A11y::Button)
            .on(State::Hover, |s| s.fill(Role::Primary))
            .id("button")
    };
    ui.frame(tree(), None, PointerInput::default(), 0.016)
        .unwrap();
    let mut warm;
    let frame = ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
    warm = paint_color(&frame, "button");
    for _ in 0..12 {
        let frame = ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        warm = paint_color(&frame, "button");
    }
    assert!(ui.get("button").hovered);
    assert_eq!(warm, role_color(Role::Primary));
}

/// The whole of M1 as one widget sees it: Shift is fine, and a
/// secondary click is a click the caller can tell apart.
#[test]
fn shift_drags_a_value_fine_and_a_secondary_click_is_distinguishable() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let tree = || block(40., 40.).fill(Role::Raised).id("b");
    let drag = |ui: &mut Ui, mods: Mods| {
        // The hit map is last frame's, so a press needs a frame to land on.
        ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        let press = PointerInput {
            mods,
            ..at(10., 10., true)
        };
        ui.frame(tree(), None, press, 0.016).unwrap();
        let moved = PointerInput {
            mods,
            ..at(90., 10., true)
        };
        ui.frame(tree(), None, moved, 0.016).unwrap();
        let mut v = 0.0;
        assert!(ui.drag("b", &mut v, 0.0..=1.0, 80.0, false));
        ui.frame(tree(), None, at(90., 10., false), 0.016).unwrap();
        v
    };
    let coarse = drag(&mut ui, Mods::default());
    let fine = drag(
        &mut ui,
        Mods {
            shift: true,
            ..Mods::default()
        },
    );
    assert!((coarse - 1.0).abs() < 1e-9, "80 px is the full span");
    assert!((fine - coarse * FINE_DRAG).abs() < 1e-9, "a tenth of it");

    ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
    let secondary = PointerInput {
        buttons: Buttons::default().set(Button::Secondary, true),
        ..at(10., 10., false)
    };
    ui.frame(tree(), None, secondary, 0.016).unwrap();
    ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
    let r = ui.get("b");
    assert!(r.clicked_with(Button::Secondary), "the reset gesture");
    assert!(!r.clicked_with(Button::Primary));
}

#[test]
fn a_cancelled_gesture_still_ends() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let tree = || block(40., 40.).fill(Role::Raised).id("b");
    ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
    ui.frame(tree(), None, at(10., 10., true), 0.016).unwrap();
    ui.cancel();
    let f = ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
    assert_eq!(f.edits, vec![("b".to_owned(), Edit::End)]);
}

#[test]
fn hover_warms_the_fill_and_a_press_is_reported_next_frame() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let tree = || {
        block(40., 40.)
            .fill(Role::Raised)
            .a11y(A11y::Button)
            .id("b")
    };
    let base = ui
        .frame(tree(), None, PointerInput::default(), 0.016)
        .unwrap()
        .scene
        .paint[0]
        .paint
        .clone();
    ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
    let f = ui.frame(tree(), None, at(10., 10., true), 0.016).unwrap();
    assert!(f.animating);
    assert_ne!(f.scene.paint[0].paint, base);
    let _ = ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
    assert!(ui.get("b").released);
}

#[test]
fn a_gesture_edge_survives_a_frame_that_failed_to_resolve() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let good = || block(40., 40.).fill(Role::Raised).id("b");
    let bad = || good().radius(-1.);
    ui.frame(good(), None, at(10., 10., false), 0.016).unwrap();
    assert!(
        ui.frame(bad(), None, at(10., 10., true), 0.016).is_err(),
        "a negative radius does not resolve"
    );
    let f = ui.frame(good(), None, at(10., 10., true), 0.016).unwrap();
    assert_eq!(f.edits, vec![("b".to_owned(), Edit::Begin)]);
}

#[test]
fn a_degenerate_or_inverted_range_resolves_and_clamps() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let mut v = 1.0;
    let el = widgets::slider(&mut ui, "fixed", "Fixed", &mut v, 1.0..=1.0)
        .el
        .into_el();
    ui.frame(el, None, PointerInput::default(), 0.016)
        .expect("a fixed parameter is still a tree");
    let mut down = 0.5;
    let el = widgets::slider(&mut ui, "down", "Down", &mut down, 1.0..=0.0)
        .el
        .into_el();
    ui.frame(el, None, PointerInput::default(), 0.016)
        .expect("and so is a downward one");
}
