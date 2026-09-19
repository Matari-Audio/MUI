use super::*;
use mui_input::{Button, Buttons};
use mui_scene::prelude::*;

fn gain() -> El {
    leaf(120.0, 40.0).id("gain").focusable().role(Kind::Slider {
        value: 0.5,
        min: 0.0,
        max: 1.0,
    })
}
fn step(ui: &mut Ui, tree: El, pos: Option<Point>, buttons: Buttons) -> Vec<(String, Edit)> {
    ui.frame(
        tree,
        None,
        PointerInput {
            pos,
            buttons,
            ..Default::default()
        },
        0.016,
    )
    .unwrap()
    .edits
}
fn idle(ui: &mut Ui, tree: El) -> Vec<(String, Edit)> {
    step(ui, tree, None, Buttons::default())
}
fn press(ui: &mut Ui) -> Vec<(String, Edit)> {
    step(ui, gain(), Some(Point::new(20.0, 20.0)), Buttons::PRIMARY)
}

#[test]
fn rounded_clip_rejects_an_invisible_child_corner() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let tree = || {
        stack![leaf(100.0, 100.0).id("child")]
            .square(100.0)
            .pill()
            .clip()
    };
    idle(&mut ui, tree());
    step(
        &mut ui,
        tree(),
        Some(Point::new(2.0, 2.0)),
        Buttons::PRIMARY,
    );
    assert!(!ui.get("child").pressed);
    ui.cancel();
    step(
        &mut ui,
        tree(),
        Some(Point::new(50.0, 50.0)),
        Buttons::PRIMARY,
    );
    assert!(ui.get("child").pressed);
}

#[test]
fn removing_a_held_control_ends_its_gesture_in_that_frame() {
    let mut ui = Ui::new(Theme::DEFAULT);
    idle(&mut ui, gain());
    assert_eq!(press(&mut ui), vec![("gain".into(), Edit::Begin)]);
    let edits = step(
        &mut ui,
        leaf(120.0, 40.0),
        Some(Point::new(20.0, 20.0)),
        Buttons::PRIMARY,
    );
    assert_eq!(edits, vec![("gain".into(), Edit::End)]);
    assert!(!ui.get("gain").held);
    assert!(ui.focus_key().is_none());
}

#[test]
fn disabling_a_held_control_ends_it_without_waiting_for_another_frame() {
    let mut ui = Ui::new(Theme::DEFAULT);
    idle(&mut ui, gain());
    press(&mut ui);
    let edits = step(
        &mut ui,
        gain().disabled(true),
        Some(Point::new(20.0, 20.0)),
        Buttons::PRIMARY,
    );
    assert_eq!(edits, vec![("gain".into(), Edit::End)]);
    assert!(!ui.get("gain").held);
}

#[test]
fn repeated_cancel_does_not_erase_the_owed_end() {
    let mut ui = Ui::new(Theme::DEFAULT);
    idle(&mut ui, gain());
    press(&mut ui);
    ui.cancel();
    ui.cancel();
    assert_eq!(ui.close(), vec![("gain".into(), Edit::End)]);
    assert!(ui.close().is_empty());
}

#[test]
fn close_balances_capture_without_another_frame() {
    let mut ui = Ui::new(Theme::DEFAULT);
    idle(&mut ui, gain());
    press(&mut ui);
    assert_eq!(ui.close(), vec![("gain".into(), Edit::End)]);
    assert!(ui.close().is_empty());
}

#[test]
fn invalid_time_does_not_poison_the_runtime() {
    let mut ui = Ui::new(Theme::DEFAULT);
    idle(&mut ui, gain());
    let before = ui.time;
    for dt in [f64::NAN, f64::INFINITY, -0.001] {
        assert!(matches!(
            ui.frame(gain(), None, Input::default(), dt),
            Err(SceneError::InvalidFrameDelta)
        ));
        assert_eq!(ui.time, before);
    }
    idle(&mut ui, gain());
    assert!(ui.time.is_finite());
}

#[test]
fn semantic_set_value_is_clamped_and_bracketed() {
    let mut ui = Ui::new(Theme::DEFAULT);
    idle(&mut ui, gain());
    assert!(ui.request_action(SemanticAction::set_value("gain", 5.0)));
    let mut value = 0.5;
    assert!(ui.drag("gain", &mut value, 0.0..=1.0, 120.0, true));
    assert_eq!(value, 1.0);
    assert_eq!(
        idle(&mut ui, gain()),
        vec![("gain".into(), Edit::Begin), ("gain".into(), Edit::End)]
    );
    assert_eq!(
        ui.edits_for("gain").collect::<Vec<_>>(),
        vec![Edit::Begin, Edit::End]
    );
    assert!(idle(&mut ui, gain()).is_empty());
}

#[test]
fn incompatible_disabled_and_nonfinite_actions_are_rejected() {
    let mut ui = Ui::new(Theme::DEFAULT);
    idle(&mut ui, gain());
    assert!(!ui.request_action(SemanticAction::activate("gain")));
    assert!(!ui.request_action(SemanticAction::set_value("absent", 0.8)));
    assert!(!ui.request_action(SemanticAction::set_value("gain", f64::NAN)));
    idle(&mut ui, gain().disabled(true));
    assert!(!ui.request_action(SemanticAction::set_value("gain", 0.8)));
    assert!(!ui.request_action(SemanticAction::focus("gain")));
}

#[test]
fn semantic_activation_does_not_synthesize_pointer_capture() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let tree = || leaf(100.0, 40.0).id("go").role(Kind::Button);
    idle(&mut ui, tree());
    assert!(ui.request_action(SemanticAction::activate("go")));
    assert!(ui.get("go").clicked_with(Button::Primary));
    assert!(!ui.get("go").held);
    assert!(ui.pointer.pos.is_none());
    idle(&mut ui, tree());
    assert!(!ui.get("go").clicked);
}

#[test]
fn failed_layout_does_not_replay_a_consumed_activation() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let tree = || leaf(100.0, 40.0).id("go").role(Kind::Button);
    idle(&mut ui, tree());
    assert!(ui.request_action(SemanticAction::activate("go")));
    assert!(ui.get("go").clicked);
    assert!(ui
        .frame(leaf(-1.0, 10.0), None, Input::default(), 0.016)
        .is_err());
    assert!(!ui.get("go").clicked);
    assert_eq!(
        ui.close(),
        vec![("go".into(), Edit::Begin), ("go".into(), Edit::End)]
    );
}

#[test]
fn transient_tweens_and_removed_field_state_do_not_accumulate() {
    let mut ui = Ui::new(Theme::DEFAULT);
    for i in 0..256 {
        ui.tween(&format!("temporary-{i}"), i as f64);
        idle(&mut ui, leaf(20.0, 20.0));
        assert!(ui.motion.len() <= 1);
    }
    ui.sel.insert("gone".into(), (3, 8));
    ui.scrolls.insert("gone".into(), [5.0, 9.0]);
    idle(&mut ui, leaf(20.0, 20.0));
    assert!(ui.motion.is_empty());
    assert!(ui.sel.is_empty());
    assert!(ui.scrolls.is_empty());
}

#[test]
fn model_identity_survives_reorder_and_display_rename() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let a = Id::of("osc").entity(42).field("gain");
    let b = Id::of("osc").entity(77).field("gain");
    let control = |id: &Id, name: &str| leaf(60.0, 30.0).id(id).label(name).focusable();
    idle(&mut ui, row![control(&a, "Saw"), control(&b, "Sine")]);
    ui.focus(a.as_str());
    ui.tween(a.as_str(), 0.4);
    idle(&mut ui, row![control(&b, "Sine"), control(&a, "Renamed")]);
    assert_eq!(ui.focus_key(), Some(a.as_str()));
    assert_eq!(ui.tween(a.as_str(), 0.4), 0.4);
    assert_eq!(
        ui.scene().unwrap().surface(a.as_str()).unwrap().frame.x,
        60.0
    );
}
