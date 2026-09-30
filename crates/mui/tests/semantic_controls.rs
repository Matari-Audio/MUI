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
    }
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
