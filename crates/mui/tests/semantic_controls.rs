//! Complete controls exercised through the same scene and input routes as a host.
use mui::prelude::*;
use mui::scene::Layer;
use std::sync::Arc;

fn key(key: Key) -> Input {
    Input {
        keys: vec![KeyPress {
            key,
            mods: Mods::default(),
        }],
        ..Input::default()
    }
}
fn pointer(pos: Point, button: Button, down: bool) -> Input {
    PointerInput {
        pos: Some(pos),
        buttons: Buttons::default().set(button, down),
        ..PointerInput::default()
    }
    .into()
}
fn centre(ui: &Ui, id: &str) -> Point {
    let f = ui.scene().unwrap().surface(id).unwrap().frame;
    Point::new(f.x + f.size.width / 2.0, f.y + f.size.height / 2.0)
}
fn action_frame(ui: &mut Ui, calls: &mut usize, input: Input, disabled: bool) {
    let el = button(ui, "save", "Save")
        .on_change(|| *calls += 1)
        .when(disabled, Styled::disabled);
    ui.frame(el.into_el(), None, input, 0.016).unwrap();
}

#[test]
fn activations_coalesce_and_do_not_replay_on_idle_frames() {
    let mut ui = Ui::default();
    let mut calls = 0;
    action_frame(&mut ui, &mut calls, Input::default(), false);
    let pos = centre(&ui, "save");
    for expected in 1..=3 {
        action_frame(
            &mut ui,
            &mut calls,
            pointer(pos, Button::Primary, true),
            false,
        );
        action_frame(
            &mut ui,
            &mut calls,
            pointer(pos, Button::Primary, false),
            false,
        );
        action_frame(&mut ui, &mut calls, Input::default(), false);
        assert_eq!(calls, expected);
    }
    ui.focus("save");
    for k in [Key::Enter, Key::Space, Key::Space] {
        action_frame(&mut ui, &mut calls, key(k), false);
        action_frame(&mut ui, &mut calls, Input::default(), false);
    }
    assert_eq!(
        calls, 6,
        "one for each delivered key press, including repeats"
    );
    action_frame(&mut ui, &mut calls, key(Key::Enter), false);
    assert!(ui.request_action(SemanticAction::activate("save")));
    assert!(ui.request_action(SemanticAction::activate("save")));
    action_frame(&mut ui, &mut calls, Input::default(), false);
    assert_eq!(
        calls, 7,
        "key and duplicate semantic requests coalesce in one frame"
    );
    for _ in 0..3 {
        action_frame(&mut ui, &mut calls, Input::default(), false);
    }
    assert_eq!(calls, 7);
}

#[test]
fn interrupted_disabled_and_secondary_actions_never_activate() {
    let mut ui = Ui::default();
    let mut calls = 0;
    action_frame(&mut ui, &mut calls, Input::default(), false);
    let pos = centre(&ui, "save");
    action_frame(
        &mut ui,
        &mut calls,
        pointer(pos, Button::Secondary, true),
        false,
    );
    action_frame(
        &mut ui,
        &mut calls,
        pointer(pos, Button::Secondary, false),
        false,
    );
    action_frame(&mut ui, &mut calls, Input::default(), false);
    assert_eq!(calls, 0, "before next press");
    action_frame(
        &mut ui,
        &mut calls,
        pointer(pos, Button::Primary, true),
        false,
    );
    ui.cancel();
    action_frame(
        &mut ui,
        &mut calls,
        pointer(pos, Button::Primary, false),
        false,
    );
    action_frame(&mut ui, &mut calls, Input::default(), false);
    assert_eq!(calls, 0, "before next press");
    action_frame(
        &mut ui,
        &mut calls,
        pointer(pos, Button::Primary, true),
        false,
    );
    action_frame(
        &mut ui,
        &mut calls,
        pointer(pos, Button::Primary, false),
        true,
    );
    assert_eq!(calls, 0, "disable while held");
    assert!(
        ui.get("save").released,
        "custom controls still see the closing release"
    );
    assert!(
        !ui.get("save").activated(),
        "a disabled release cannot activate"
    );
    assert!(!ui.request_action(SemanticAction::activate("save")));
    ui.focus("save");
    action_frame(&mut ui, &mut calls, key(Key::Enter), true);
    assert_eq!(calls, 0, "after disabled release");
    action_frame(&mut ui, &mut calls, Input::default(), true);
    assert_eq!(calls, 0);
    assert!(!ui.focused("save"));
    action_frame(&mut ui, &mut calls, Input::default(), false);
    assert!(ui.request_action(SemanticAction::activate("save")));
    action_frame(&mut ui, &mut calls, Input::default(), false);
    assert_eq!(calls, 1, "reenabling starts cleanly");
}

#[test]
fn every_control_has_a_visible_keyboard_focus_ring_without_layout_motion() {
    for mode in [Mode::Dark, Mode::Light] {
        for kind in 0..5 {
            let mut ui = Ui::default();
            let mut theme = Theme::default();
            theme.palette.mode = mode;
            ui.set_theme(theme);
            let (mut value, mut on) = (0.5, false);
            let build = |ui: &mut Ui, value: &mut f64, on: &mut bool| match kind {
                0 => button(ui, "c", "Action").el,
                1 => toggle(ui, "c", "Switch", on).el,
                2 => slider(ui, "c", "Gain", value, 0.0..=1.0).el,
                3 => knob(ui, "c", "Gain", value, 0.0..=1.0).el,
                _ => drag_value(ui, "c", "Gain", value, 0.0..=1.0).el,
            };
            let el = build(&mut ui, &mut value, &mut on).el();
            ui.frame(el.into_el(), None, Input::default(), 0.016)
                .unwrap();
            let before = ui.scene().unwrap().surface("c").unwrap().frame;
            let expected_cursor = match kind {
                0 | 1 => Cursor::Hand,
                3 => Cursor::ResizeV,
                _ => Cursor::ResizeH,
            };
            assert_eq!(
                ui.scene().unwrap().surface("c").unwrap().cursor,
                Some(expected_cursor)
            );
            ui.focus("c");
            for _ in 0..32 {
                let el = build(&mut ui, &mut value, &mut on).el();
                ui.frame(el.into_el(), None, Input::default(), 0.016)
                    .unwrap();
            }
            let scene = ui.scene().unwrap();
            assert!(
                scene
                    .paint
                    .iter()
                    .any(|p| p.key.as_str() == "c" && p.layer == Layer::Stroke),
                "kind {kind}, {mode:?}"
            );
            assert_eq!(
                scene.surface("c").unwrap().frame,
                before,
                "focus preserves hit geometry"
            );
            ui.blur();
            for _ in 0..32 {
                let el = build(&mut ui, &mut value, &mut on).el();
                ui.frame(el.into_el(), None, Input::default(), 0.016)
                    .unwrap();
            }
            assert!(
                !ui.scene()
                    .unwrap()
                    .paint
                    .iter()
                    .any(|p| p.key.as_str() == "c" && p.layer == Layer::Stroke)
            );
        }
    }
}

#[test]
fn explicit_steps_cover_negative_bounds_shift_and_semantic_actions() {
    let mut ui = Ui::default();
    let mut value = -2.0;
    let frame = |ui: &mut Ui, value: &mut f64, input: Input| {
        let el = knob(ui, "n", "Offset", value, -2.0..=2.0).step(1.0);
        ui.frame(el.into_el(), None, input, 0.016).unwrap();
    };
    frame(&mut ui, &mut value, Input::default());
    ui.focus("n");
    for (key, expected) in [
        (Key::Left, -2.0),
        (Key::Right, -1.0),
        (Key::Right, 0.0),
        (Key::End, 2.0),
        (Key::Right, 2.0),
        (Key::Left, 1.0),
        (Key::Home, -2.0),
    ] {
        frame(&mut ui, &mut value, self::key(key));
        frame(&mut ui, &mut value, Input::default());
        assert_eq!(value, expected);
    }
    let mut shifted = key(Key::Up);
    shifted.keys[0].mods.shift = true;
    frame(&mut ui, &mut value, shifted);
    frame(&mut ui, &mut value, Input::default());
    assert_eq!(
        value, -1.0,
        "explicit minimum steps remain usable with Shift"
    );
    assert!(ui.request_action(SemanticAction::increment("n")));
    assert!(ui.request_action(SemanticAction::increment("n")));
    frame(&mut ui, &mut value, Input::default());
    assert_eq!(value, 1.0, "two numeric requests are two steps");
    assert!(ui.request_action(SemanticAction::decrement("n")));
    frame(&mut ui, &mut value, Input::default());
    assert_eq!(value, 0.0);
}

#[test]
fn a_setting_shares_its_label_and_help_with_the_actual_input() {
    let mut ui = Ui::default();
    let mut on = false;
    let make = |ui: &mut Ui, on: &mut bool| {
        setting("sync", "Sync")
            .description("Follow host tempo")
            .toggle(ui, on)
    };
    let el = group("engine", "Engine", [make(&mut ui, &mut on)]);
    ui.frame(el.into_el(), None, Input::default(), 0.016)
        .unwrap();
    let scene = ui.scene().unwrap();
    let input = scene.surface("sync").unwrap();
    assert_eq!(
        input.semantics.as_ref().unwrap().label.as_deref(),
        Some("Sync")
    );
    assert_eq!(input.description.as_deref(), Some("Follow host tempo"));
    let group_id = Id::of("sync").field("setting");
    assert_eq!(input.parent.as_deref(), Some(group_id.as_str()));
    assert_eq!(
        scene.surface(&group_id).unwrap().parent.as_deref(),
        Some("engine")
    );
    assert!(ui.request_action(SemanticAction::activate("sync")));
    let response = make(&mut ui, &mut on);
    assert!(response.changed && on);
}

#[test]
fn formatted_readouts_reserve_measured_width_and_cache_matches_fresh_layout() {
    let font = Font::new(ttf_inter::REGULAR).unwrap();
    let samples: Arc<[String]> = vec!["iiiiiiii".into(), "WWWW".into()].into();
    let width = |s: &str| {
        resolve(&SceneSpec::new(text(s).id("t")).font(font.clone()))
            .unwrap()
            .surface("t")
            .unwrap()
            .frame
            .size
            .width
    };
    assert!(
        width("WWWW") > width("iiiiiiii"),
        "fewer characters can be wider"
    );
    let mut resolver = Resolver::new();
    let mut ui = Ui::default();
    let mut expected = None;
    for sample in ["i", "WWWW", "ii", "iiiiiiii", "W"] {
        let mut value = 0.5;
        let tree = row([
            drag_value(&mut ui, "readout", "Value", &mut value, 0.0..=1.0)
                .value_text(sample)
                .value_reserve_all(samples.clone())
                .el
                .el(),
            text("neighbour").id("next"),
        ]);
        let spec = SceneSpec::new(tree).font(font.clone());
        let fresh = resolve(&spec).unwrap();
        let cached = resolver.resolve(&spec).unwrap();
        let frame = fresh.surface("readout").unwrap().frame;
        let x = fresh.surface("next").unwrap().frame.x;
        assert_eq!(cached.surface("readout").unwrap().frame, frame);
        assert_eq!(cached.surface("next").unwrap().frame.x, x);
        assert_eq!(*expected.get_or_insert((frame, x)), (frame, x));
        assert_eq!(
            cached
                .surface("readout")
                .unwrap()
                .value_description
                .as_deref(),
            Some(sample)
        );
    }
    // Changing only the reservation must invalidate the intrinsic cache.
    for sample in ["ii", "WWWWWWWW", "i"] {
        let spec =
            SceneSpec::new(text("x").reserve_all(vec![sample.into()]).id("t")).font(font.clone());
        assert_eq!(
            resolver.resolve(&spec).unwrap().surface("t").unwrap().frame,
            resolve(&spec).unwrap().surface("t").unwrap().frame
        );
    }
}

#[test]
fn knob_reservations_keep_the_dial_and_its_neighbour_in_place() {
    let mut ui = Ui::default();
    let mut previous = None;
    for text in ["1 Hz", "20.0 kHz", "99 Hz", "1 Hz"] {
        let mut value = 0.5;
        let el = row![
            knob(&mut ui, "k", "Frequency", &mut value, 0.0..=1.0)
                .value_text(text)
                .value_reserve("20.0 kHz"),
            button(&mut ui, "b", "Reset")
        ];
        ui.frame(el.into_el(), None, Input::default(), 0.016)
            .unwrap();
        let scene = ui.scene().unwrap();
        let frames = (
            scene.surface("k").unwrap().frame,
            scene.surface("b").unwrap().frame,
        );
        assert_eq!(*previous.get_or_insert(frames), frames);
        let dial = frames.0;
        assert!(
            !scene
                .paint
                .iter()
                .any(|paint| paint.key.as_str() == "k/reading" && paint.layer == Layer::Fill),
            "the value layout must not paint over the disc"
        );
        let reading = scene.surface("k/reading").unwrap().frame;
        assert!(reading.x >= dial.x && reading.right() <= dial.right());
        assert!(reading.y >= dial.y && reading.bottom() <= dial.bottom());
        assert!((dial.size.width - dial.size.height).abs() < 0.5, "{dial:?}");
    }
}

#[test]
fn compact_macro_style_knob_keeps_its_value_inside_and_its_name_accessible() {
    let mut ui = Ui::default();
    let mut value = 0.5;
    let control = knob(&mut ui, "compact", "Mix", &mut value, 0.0..=1.0)
        .size(S)
        .value_text("50%")
        .value_reserve("100%");
    ui.frame(control.into_el(), None, Input::default(), 0.016)
        .unwrap();
    let scene = ui.scene().unwrap();
    let dial = scene.surface("compact").unwrap();
    assert!(
        dial.frame.size.width <= 48.,
        "compact dial: {:?}",
        dial.frame
    );
    assert!(scene.surface("compact/reading").is_some());
    assert!(matches!(
        dial.semantics.as_ref().map(|s| &s.role),
        Some(A11y::Slider { .. })
    ));
}

#[test]
fn a_still_knob_keeps_its_painted_dial_across_frames() {
    let mut ui = Ui::default();
    let paths = |ui: &mut Ui, mut value: f64| {
        let control = knob(ui, "dial", "Mix", &mut value, 0.0..=1.0);
        ui.frame(control.into_el(), None, Input::default(), 0.016)
            .unwrap();
        let scene = ui.scene().unwrap();
        let drawn = scene
            .paint
            .iter()
            .filter(|p| matches!(p.layer, Layer::Draw(_)));
        drawn.map(|p| Arc::clone(&p.path)).collect::<Vec<_>>()
    };
    let (a, b) = (paths(&mut ui, 0.5), paths(&mut ui, 0.5));
    assert_eq!((a.len(), b.len()), (4, 4), "track, arc, shadow, face");
    assert!(
        a.iter().zip(&b).all(|(a, b)| Arc::ptr_eq(a, b)),
        "redrawn while still"
    );
}

#[test]
fn dragging_from_the_centered_reading_changes_the_knob_value() {
    let mut ui = Ui::default();
    let mut value = 0.;
    let frame = |ui: &mut Ui, value: &mut f64, input| {
        let control = knob(ui, "dial", "Mix", value, 0.0..=1.0)
            .value_text("0%")
            .size(S);
        ui.frame(control.into_el(), None, input, 0.016).unwrap();
    };
    frame(&mut ui, &mut value, Input::default());
    let from = centre(&ui, "dial");
    let to = Point::new(from.x, from.y - 24.);
    for input in [
        pointer(from, Button::Primary, true),
        pointer(to, Button::Primary, true),
        pointer(to, Button::Primary, false),
        Input::default(),
    ] {
        frame(&mut ui, &mut value, input);
    }
    assert!((value - 0.2).abs() < 0.001);
}

#[test]
fn a_drag_value_keeps_its_name_and_help_when_it_becomes_a_text_field() {
    let mut ui = Ui::default();
    let mut value = 120.0;
    let frame = |ui: &mut Ui, value: &mut f64, input: Input| {
        let el = drag_value(ui, "tempo", "Tempo", value, 20.0..=300.0)
            .described("Project beats per minute");
        ui.frame(el.into_el(), None, input, 0.016).unwrap();
    };
    frame(&mut ui, &mut value, Input::default());
    ui.focus("tempo");
    frame(&mut ui, &mut value, key(Key::Enter));
    frame(&mut ui, &mut value, Input::default());
    let field = ui.scene().unwrap().surface("tempo/edit").unwrap();
    assert_eq!(
        field.semantics.as_ref().unwrap().label.as_deref(),
        Some("Tempo")
    );
    assert_eq!(
        field.description.as_deref(),
        Some("Project beats per minute")
    );
    frame(&mut ui, &mut value, key(Key::Escape));
    frame(&mut ui, &mut value, Input::default());
    assert_eq!(value, 120.0);
    assert!(ui.scene().unwrap().surface("tempo").is_some());
}

#[test]
fn settings_attach_help_to_nested_knob_and_slider_targets() {
    for kind in 0..2 {
        let mut ui = Ui::default();
        let mut value = 0.5;
        let response = setting("value", "Value")
            .description("Adjust the value")
            .control(&mut ui, |ui, id, label| {
                if kind == 0 {
                    knob(ui, id, label, &mut value, 0.0..=1.0)
                } else {
                    slider(ui, id, label, &mut value, 0.0..=1.0)
                }
            });
        ui.frame(response.into_el(), None, Input::default(), 0.016)
            .unwrap();
        let target = ui.scene().unwrap().surface("value").unwrap();
        assert_eq!(target.description.as_deref(), Some("Adjust the value"));
        assert_eq!(
            target.semantics.as_ref().unwrap().label.as_deref(),
            Some("Value")
        );
        assert!(matches!(
            target.semantics.as_ref().unwrap().role,
            A11y::Slider { .. }
        ));
    }
}

#[test]
fn setting_does_not_repeat_builtin_knob_or_slider_labels() {
    for (is_knob, control_label, expected_text_layers) in [
        (true, "Value", 2),
        (false, "Value", 2),
        (true, "Fine", 3),
        (false, "Fine", 3),
    ] {
        let mut ui = Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
        let mut value = 0.5;
        let response = setting("value", "Value").control(&mut ui, |ui, id, setting_label| {
            let label = if control_label == "Value" {
                setting_label
            } else {
                control_label
            };
            if is_knob {
                knob(ui, id, label, &mut value, 0.0..=1.0)
            } else {
                slider(ui, id, label, &mut value, 0.0..=1.0)
            }
        });
        ui.frame(response.into_el(), None, Input::default(), 0.016)
            .unwrap();
        let scene = ui.scene().unwrap();
        let input = scene.surface("value").unwrap();
        assert_eq!(
            input.semantics.as_ref().unwrap().label.as_deref(),
            Some(control_label)
        );
        let text_layers = scene
            .paint
            .iter()
            .filter(|paint| paint.layer == Layer::Text)
            .count();
        assert_eq!(
            text_layers, expected_text_layers,
            "matching built-in labels should be hidden without dropping distinct labels or slider readouts"
        );
    }
}

#[test]
fn a_slider_reserve_holds_its_lane_and_neighbour_without_a_fixed_width() {
    let mut ui = Ui::default().font(Font::new(ttf_inter::REGULAR).unwrap());
    let samples: Arc<[String]> = vec!["iiiiiiii".into(), "WWWW".into()].into();
    let mut previous = None;
    for value_text in ["i", "WWWW", "iiiiiiii", "ii"] {
        let mut value = 0.5;
        let el = row![
            slider(&mut ui, "s", "Level", &mut value, 0.0..=1.0)
                .value_text(value_text)
                .value_reserve_all(samples.clone()),
            body("next").id("next")
        ];
        ui.frame(el.into_el(), None, Input::default(), 0.016)
            .unwrap();
        let scene = ui.scene().unwrap();
        let geometry = (
            scene.surface("s").unwrap().frame,
            scene.surface("next").unwrap().frame,
        );
        assert_eq!(*previous.get_or_insert(geometry), geometry);
    }
}

#[test]
fn explicit_steps_handle_page_keys_reversed_ranges_and_invalid_overrides() {
    let mut ui = Ui::default();
    let mut value = 2.0;
    let frame = |ui: &mut Ui, value: &mut f64, input: Input, step: f64| {
        let el = knob(ui, "r", "Reversed", value, 2.0..=-2.0).step(step);
        ui.frame(el.into_el(), None, input, 0.016).unwrap();
    };
    frame(&mut ui, &mut value, Input::default(), 1.0);
    ui.focus("r");
    for (k, expected) in [(Key::Right, 1.0), (Key::PageUp, -2.0), (Key::PageDown, 2.0)] {
        frame(&mut ui, &mut value, key(k), 1.0);
        frame(&mut ui, &mut value, Input::default(), 1.0);
        assert_eq!(value, expected);
    }
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        frame(&mut ui, &mut value, Input::default(), invalid);
        assert_eq!(ui.scene().unwrap().surface("r").unwrap().numeric_step, None);
    }
    frame(&mut ui, &mut value, key(Key::Right), f64::NAN);
    frame(&mut ui, &mut value, Input::default(), f64::NAN);
    assert!(
        (value - 1.96).abs() < 1e-10,
        "default signed hundredth restored"
    );
}

#[test]
fn disabled_controls_have_a_visual_cue_in_both_themes() {
    for mode in [Mode::Dark, Mode::Light] {
        let mut ui = Ui::default();
        let mut theme = Theme::default();
        theme.palette.mode = mode;
        ui.set_theme(theme);
        let el = button(&mut ui, "disabled", "Unavailable")
            .disabled()
            .into_el();
        ui.frame(el, None, Input::default(), 0.016).unwrap();
        let scene = ui.scene().unwrap();
        assert!(scene.surface("disabled").unwrap().disabled);
        assert!(scene.paint.iter().any(
            |p| matches!(p.layer, Layer::Blend { opacity, .. } if (opacity - 0.45).abs() < 1e-6)
        ));
    }
}

#[test]
fn supplied_value_text_retains_its_existing_width_floor() {
    let mut ui = Ui::default();
    let mut value = 440.0;
    let tree = drag_value(&mut ui, "f", "Frequency", &mut value, 20.0..=20_000.0)
        .value_text("440 Hz")
        .value_reserve("20.0 kHz")
        .el
        .el();
    let readout = tree.children()[0].payload().extras();
    assert_eq!(readout.reserve.as_deref(), Some("440 Hz"));
    assert_eq!(
        readout.reserve_all.as_deref(),
        Some(&[String::from("20.0 kHz")][..])
    );
}

#[test]
fn filling_knobs_take_square_shares_of_their_cells() {
    let mut ui = Ui::default();
    let (mut a, mut b) = (0.5, 0.5);
    let hero = knob(&mut ui, "hero", "Depth", &mut a, 0.0..=1.0).fill_cell();
    let small = knob(&mut ui, "small", "Mix", &mut b, 0.0..=1.0).fill_cell();
    let panel = col![stack![hero].grow(2), stack![small].grow(1)];
    ui.frame(
        panel.into_el(),
        Some(Size::new(400., 400.)),
        Input::default(),
        0.016,
    )
    .unwrap();
    let cells = ui.scene().unwrap().surface("hero").unwrap().frame;
    // The same, with the flex on the knobs themselves.
    let hero = knob(&mut ui, "hero", "Depth", &mut a, 0.0..=1.0)
        .fill_cell()
        .grow(2);
    let small = knob(&mut ui, "small", "Mix", &mut b, 0.0..=1.0)
        .fill_cell()
        .grow(1);
    ui.frame(
        col![hero, small].into_el(),
        Some(Size::new(400., 400.)),
        Input::default(),
        0.016,
    )
    .unwrap();
    assert_eq!(
        ui.scene().unwrap().surface("hero").unwrap().frame.size,
        cells.size
    );
    let scene = ui.scene().unwrap();
    let (hero, small) = (
        scene.surface("hero").unwrap().frame,
        scene.surface("small").unwrap().frame,
    );
    for f in [hero, small] {
        assert!((f.size.width - f.size.height).abs() < 0.5, "square: {f:?}");
    }
    assert!(
        (hero.size.width / small.size.width - 2.0).abs() < 0.01,
        "2:1 dials: {hero:?} {small:?}"
    );
    assert!(
        hero.size.width > 100.,
        "the dial grew past its token: {hero:?}"
    );
    // Across a row the face's height offer would be every knob's basis, so
    // the weights are `flex` (basis 0): the widths, and the dials, go 2:1.
    let hero = knob(&mut ui, "hero", "Depth", &mut a, 0.0..=1.0)
        .fill_cell()
        .flex(2);
    let small = knob(&mut ui, "small", "Mix", &mut b, 0.0..=1.0)
        .fill_cell()
        .flex(1);
    ui.frame(
        row![hero, small].into_el(),
        Some(Size::new(300., 400.)),
        Input::default(),
        0.016,
    )
    .unwrap();
    let scene = ui.scene().unwrap();
    let (hero, small) = (
        scene.surface("hero").unwrap().frame,
        scene.surface("small").unwrap().frame,
    );
    assert!(
        (hero.size.width / small.size.width - 2.0).abs() < 0.01,
        "2:1 across a row: {hero:?} {small:?}"
    );
}

#[test]
fn a_knob_look_change_repaints_a_still_knob() {
    let mut ui = Ui::default();
    let paths = |ui: &mut Ui, look: Knob| {
        let mut value = 0.5;
        let control = knob(ui, "dial", "Mix", &mut value, 0.0..=1.0).look(look);
        ui.frame(control.into_el(), None, Input::default(), 0.016)
            .unwrap();
        let scene = ui.scene().unwrap();
        scene
            .paint
            .iter()
            .filter(|p| matches!(p.layer, Layer::Draw(_)))
            .map(|p| Arc::clone(&p.path))
            .collect::<Vec<_>>()
    };
    let a = paths(&mut ui, Knob::smooth());
    let b = paths(&mut ui, Knob::k());
    let c = paths(&mut ui, Knob::k());
    assert!(
        !Arc::ptr_eq(&a[0], &b[0]),
        "a new style must not reuse the old paths"
    );
    assert!(
        b.iter().zip(&c).all(|(b, c)| Arc::ptr_eq(b, c)),
        "redrawn while still"
    );
}

#[test]
fn each_knob_preset_paints_its_parts() {
    let mut ui = Ui::default();
    let draws = |ui: &mut Ui, look: Knob| {
        let mut value = 0.5;
        let control = knob(ui, "dial", "Mix", &mut value, 0.0..=1.0).look(look);
        ui.frame(control.into_el(), None, Input::default(), 0.016)
            .unwrap();
        let scene = ui.scene().unwrap();
        scene
            .paint
            .iter()
            .filter(|p| matches!(p.layer, Layer::Draw(_)))
            .map(|p| p.cap)
            .collect::<Vec<_>>()
    };
    let smooth = draws(&mut ui, Knob::default());
    assert_eq!(smooth.len(), 4, "track, arc, face shadow, face: {smooth:?}");
    assert!(smooth[..2].iter().all(|c| *c == Cap::Butt), "square-cut");
    let k = draws(&mut ui, Knob::k());
    assert_eq!(k.len(), 4, "track, arc, face, pointer line: {k:?}");
    assert!(k.iter().all(|c| *c == Cap::Round));
    assert_eq!(draws(&mut ui, Knob::buffr()).len(), 4, "the dot");
    assert!(
        draws(&mut ui, Knob::drama()).len() <= 3,
        "no shadow, no pointer"
    );
}

#[test]
fn icon_buttons_are_named_and_tipped_and_text_buttons_take_icons() {
    let mut ui = Ui::default();
    for _ in 0..2 {
        let el = row![
            icon_button(&mut ui, "lock", "Keep on preset load", sym::LOCK).size(S),
            button(&mut ui, "save", "Save")
                .icon(sym::SAVE)
                .icon_end(sym::ARROW_FORWARD),
        ];
        ui.frame(el.into_el(), None, Input::default(), 0.016)
            .unwrap();
    }
    let scene = ui.scene().unwrap();
    let lock = scene.surface("lock").unwrap();
    assert_eq!(
        lock.semantics.as_ref().unwrap().label.as_deref(),
        Some("Keep on preset load")
    );
    assert!(matches!(
        lock.semantics.as_ref().unwrap().role,
        A11y::Button
    ));
    assert!((lock.frame.size.width - lock.frame.size.height).abs() < 0.5);
    let save = scene.surface("save").unwrap();
    assert_eq!(
        save.semantics.as_ref().unwrap().label.as_deref(),
        Some("Save")
    );
    let plain = {
        let mut ui = Ui::default();
        let el = button(&mut ui, "save", "Save");
        ui.frame(el.into_el(), None, Input::default(), 0.016)
            .unwrap();
        ui.scene().unwrap().surface("save").unwrap().frame
    };
    assert!(
        save.frame.size.width > plain.size.width + 20.,
        "two icons beside the label"
    );
}

/// "1.2k", "-3 dB": a parser in the domain's own units.
fn units(s: &str) -> Option<f64> {
    let s = s.trim();
    let s = s
        .strip_suffix("dB")
        .or_else(|| s.strip_suffix("Hz"))
        .unwrap_or(s)
        .trim();
    match s.strip_suffix('k') {
        Some(k) => k.trim().parse::<f64>().ok().map(|k| k * 1000.0),
        None => s.parse().ok(),
    }
}

#[test]
fn domains_round_trip_typed_units() {
    let hz = Domain::new(20.0..=20_000.0)
        .format(|v| {
            if v >= 1000.0 {
                format!("{:.1}k", v / 1000.0)
            } else {
                format!("{v:.0}")
            }
        })
        .parse(units);
    for v in [20.0, 440.0, 1200.0, 20_000.0] {
        assert_eq!(hz.parsed(&hz.formatted(v)), Some(v));
    }
    assert_eq!(hz.parsed("1.2k"), Some(1200.0));
    let db = Domain::new(-24.0..=24.0).parse(units);
    assert_eq!(db.parsed("-3 dB"), Some(-3.0));
    assert_eq!(db.parsed("-30 dB"), Some(-24.0));
    assert_eq!(db.parsed("quiet"), None);
}

fn text_input(s: &str) -> Input {
    Input {
        text: s.into(),
        ..Input::default()
    }
}

#[test]
fn typing_into_a_knob_lands_normalised_and_escape_cancels() {
    // A normalised 0..1 knob over -24..24 dB, typed in dB.
    let domain = Domain::new(0.0..=1.0)
        .format(|n| format!("{:.1} dB", n * 48.0 - 24.0))
        .parse(|s| units(s).map(|db| (db + 24.0) / 48.0));
    let mut ui = Ui::default();
    let mut value = 0.5;
    let frame = |ui: &mut Ui, value: &mut f64, input: Input| {
        let el = knob(ui, "gain", "Gain", value, domain.clone());
        let changed = el.changed;
        ui.frame(el.into_el(), None, input, 0.016).unwrap();
        changed
    };
    frame(&mut ui, &mut value, Input::default());
    assert_eq!(
        ui.scene()
            .unwrap()
            .surface("gain")
            .unwrap()
            .value_description
            .as_deref(),
        Some("0.0 dB"),
        "the formatter speaks for the reading"
    );
    ui.focus("gain");
    frame(&mut ui, &mut value, key(Key::Enter));
    frame(&mut ui, &mut value, Input::default());
    assert!(
        ui.scene().unwrap().surface("gain/edit").is_some(),
        "Enter opens a field"
    );
    frame(&mut ui, &mut value, text_input("12 dB"));
    frame(&mut ui, &mut value, Input::default());
    frame(&mut ui, &mut value, key(Key::Escape));
    frame(&mut ui, &mut value, Input::default());
    assert_eq!(value, 0.5, "Escape leaves the value alone");
    assert!(ui.scene().unwrap().surface("gain/edit").is_none());

    ui.focus("gain");
    frame(&mut ui, &mut value, key(Key::Enter));
    frame(&mut ui, &mut value, text_input("12 dB"));
    frame(&mut ui, &mut value, key(Key::Enter));
    // A widget reads last frame's input: the next build takes the Enter.
    assert!(
        frame(&mut ui, &mut value, Input::default()),
        "a typed value is an edit"
    );
    assert!(
        (value - 0.75).abs() < 1e-12,
        "12 dB is 0.75 of -24..24: {value}"
    );
}

#[test]
fn a_knob_without_a_parser_never_opens_a_field() {
    let mut ui = Ui::default();
    let mut value = 0.5;
    let frame = |ui: &mut Ui, value: &mut f64, input: Input| {
        let el = knob(ui, "k", "Mix", value, 0.0..=1.0);
        ui.frame(el.into_el(), None, input, 0.016).unwrap();
    };
    frame(&mut ui, &mut value, Input::default());
    ui.focus("k");
    frame(&mut ui, &mut value, key(Key::Enter));
    frame(&mut ui, &mut value, Input::default());
    assert!(ui.scene().unwrap().surface("k/edit").is_none());
}

#[test]
fn drag_value_types_in_its_domain_units() {
    // Strict: only "<n> bpm" reads, so a field that opened with a plain
    // number would not round-trip.
    let domain = Domain::new(20.0..=300.0)
        .format(|v| format!("{v:.0} bpm"))
        .parse(|s| s.trim().strip_suffix(" bpm")?.parse().ok());
    let mut ui = Ui::default();
    let mut value = 120.4;
    let frame = |ui: &mut Ui, value: &mut f64, input: Input| {
        let el = drag_value(ui, "tempo", "Tempo", value, domain.clone());
        ui.frame(el.into_el(), None, input, 0.016).unwrap();
    };
    frame(&mut ui, &mut value, Input::default());
    ui.focus("tempo");
    for input in [
        key(Key::Enter),
        Input::default(),
        key(Key::Enter),
        Input::default(),
    ] {
        frame(&mut ui, &mut value, input);
    }
    assert_eq!(
        value, 120.4,
        "an untouched formatted draft preserves the precise value"
    );
    ui.focus("tempo");
    for input in [
        key(Key::Enter),
        Input::default(),
        text_input("140 bpm"),
        key(Key::Enter),
        Input::default(),
    ] {
        frame(&mut ui, &mut value, input);
    }
    assert_eq!(value, 140.0);
}

#[test]
fn value_align_centres_a_reading_in_its_reserve() {
    let painted = |align: TextAlign| {
        let mut ui = Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
        let mut v = 0.5;
        let el = slider(&mut ui, "s", "Mix", &mut v, 0.0..=1.0)
            .value_text("1")
            .value_reserve("100.00 %")
            .value_align(align);
        ui.frame(el.into_el(), None, Input::default(), 0.016)
            .unwrap();
        let scene = ui.scene().unwrap();
        let frame = scene.surface("s/reading").unwrap().frame;
        let x = scene
            .paint
            .iter()
            .filter(|p| p.key.as_str() == "s/reading")
            .find_map(|p| p.text.as_ref().map(|t| t.origin.x));
        (frame, x.expect("the reading painted"))
    };
    let (start, centre) = (painted(TextAlign::Start), painted(TextAlign::Center));
    assert_eq!(start.0, centre.0, "the reserve keeps the frame");
    assert!(centre.1 > start.1 + 1.0, "the reading moved inside it");
}

#[test]
fn nonfinite_tip_delays_preserve_the_policy() {
    let mut ui = Ui::default();
    for policy in [TipPolicy::Off, TipPolicy::After(0.25)] {
        ui.set_tips(policy);
        for delay in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            ui.set_tips(TipPolicy::After(delay));
            assert_eq!(ui.tips(), policy);
        }
    }
}

#[test]
fn removing_a_knob_parser_cancels_its_draft() {
    let mut ui = Ui::default();
    let mut value = 0.5;
    let domain = Domain::new(0.0..=1.0).parse(|s| s.parse().ok());
    let frame = |ui: &mut Ui, value: &mut f64, domain: Domain, input| {
        let el = knob(ui, "k", "Knob", value, domain);
        ui.frame(el.into_el(), None, input, 0.016).unwrap();
    };
    frame(&mut ui, &mut value, domain.clone(), Input::default());
    ui.focus("k");
    for input in [
        key(Key::Enter),
        Input::default(),
        text_input("0.8"),
        Input::default(),
    ] {
        frame(&mut ui, &mut value, domain.clone(), input);
    }
    assert!(ui.scene().unwrap().surface("k/edit").is_some());
    frame(&mut ui, &mut value, Domain::new(0.0..=1.0), key(Key::Enter));
    frame(
        &mut ui,
        &mut value,
        Domain::new(0.0..=1.0),
        Input::default(),
    );
    assert_eq!(value, 0.5);
    assert!(!ui.focused("k/edit"));
    assert!(ui.scene().unwrap().surface("k/edit").is_none());
}

#[test]
fn knob_readouts_honor_reserve_and_alignment() {
    for shown in ["0", "0 dB"] {
        let painted = |align| {
            let mut ui = Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
            let mut value = 0.5;
            let el = knob(&mut ui, "k", "Gain", &mut value, 0.0..=1.0)
                .el
                .hide_label()
                .value_text(shown)
                .value_reserve("-100.00 dB")
                .value_align(align);
            ui.frame(el.into_el(), None, Input::default(), 0.016)
                .unwrap();
            let scene = ui.scene().unwrap();
            let frame = scene.surface("k/reading").unwrap().frame;
            let x = scene
                .paint
                .iter()
                .find_map(|p| p.text.as_ref().map(|t| t.origin.x))
                .unwrap();
            (frame, x)
        };
        let (start, end) = (painted(TextAlign::Start), painted(TextAlign::End));
        assert_eq!(start.0, end.0);
        assert!(
            end.1 > start.1 + 1.,
            "alignment moves {shown} within the reserve"
        );
    }
}
