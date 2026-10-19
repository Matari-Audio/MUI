use super::*;
use mui_input::Ime;

fn deliver(ui: &mut Ui, root: El, input: impl Into<Input>) {
    ui.frame(root, None, input, 0.016).unwrap();
}

fn fields(ui: &mut Ui, a: &mut String, b: &mut String) -> El {
    vstack![
        widgets::text_input(ui, "a", a).el,
        widgets::text_input(ui, "b", b).el,
    ]
}

fn ctrl(key: Key) -> KeyPress {
    KeyPress {
        key,
        mods: Mods {
            ctrl: true,
            ..Mods::default()
        },
    }
}

#[test]
fn view_change_drops_same_batch_shortcut_enter_and_text_before_new_focus() {
    let mut ui = Ui::default();
    let (mut a, mut b) = ("first".to_owned(), "second".to_owned());
    let root = fields(&mut ui, &mut a, &mut b);
    deliver(&mut ui, root, Input::default());
    ui.focus("a");
    let root = fields(&mut ui, &mut a, &mut b);
    deliver(
        &mut ui,
        root,
        Input {
            keys: vec![ctrl(Key::Tab), key(Key::Enter).keys.remove(0)],
            text: "outgoing".into(),
            ..Input::default()
        },
    );
    assert!(
        !ui.keys("b").is_empty(),
        "Tab already moved focus at intake"
    );
    ui.discard_input_for_view_change();
    ui.blur();
    ui.focus("b");
    assert!(ui.keys("b").is_empty());
    assert!(ui.shortcuts().is_empty());
    let incoming = widgets::text_edit(&mut ui, "b", &mut b, widgets::TextOpts::default());
    assert!(!incoming.changed.changed);
    assert!(!incoming.changed.submitted);
    assert_eq!(a, "first");
    assert_eq!(b, "second");

    let root = fields(&mut ui, &mut a, &mut b);
    deliver(
        &mut ui,
        root,
        Input {
            text: "fresh".into(),
            ..Input::default()
        },
    );
    assert!(widgets::text_input(&mut ui, "b", &mut b).changed);
    assert!(b.contains("fresh"));
    assert_eq!(a, "first");
}

#[test]
fn view_change_drops_incoming_paste_but_preserves_outbound_copy() {
    let mut ui = Ui::default();
    let (mut a, mut b) = ("first".to_owned(), "second".to_owned());
    let root = fields(&mut ui, &mut a, &mut b);
    deliver(&mut ui, root, Input::default());
    ui.focus("a");
    let root = fields(&mut ui, &mut a, &mut b);
    deliver(
        &mut ui,
        root,
        Input {
            keys: vec![ctrl(Key::Char('v'))],
            clipboard: Some("old paste".into()),
            ..Input::default()
        },
    );
    ui.set_clipboard("outbound copy");
    ui.discard_input_for_view_change();
    ui.focus("b");
    assert!(!widgets::text_input(&mut ui, "b", &mut b).changed);
    assert_eq!(b, "second");
    let root = fields(&mut ui, &mut a, &mut b);
    let frame = ui.frame(root, None, Input::default(), 0.016).unwrap();
    assert_eq!(frame.clipboard.as_deref(), Some("outbound copy"));

    let root = fields(&mut ui, &mut a, &mut b);
    deliver(
        &mut ui,
        root,
        Input {
            keys: vec![ctrl(Key::Char('v'))],
            clipboard: Some("fresh paste".into()),
            ..Input::default()
        },
    );
    assert!(widgets::text_input(&mut ui, "b", &mut b).changed);
    assert!(b.contains("fresh paste"));
    assert!(!b.contains("old paste"));
}

#[test]
fn view_change_discards_queued_native_edits_without_erasing_editor_state() {
    let mut ui = Ui::default();
    let (mut a, mut b) = ("first".to_owned(), "second".to_owned());
    let root = fields(&mut ui, &mut a, &mut b);
    deliver(&mut ui, root, Input::default());
    ui.focus("a");
    ui.set_sel("a", 1, 3);
    let root = fields(&mut ui, &mut a, &mut b);
    deliver(
        &mut ui,
        root,
        Input {
            ime: vec![
                Ime::Selection(0..1),
                Ime::Commit("stale".into()),
                Ime::Preedit {
                    text: "marked".into(),
                    cursor: Some((1, 2)),
                },
            ],
            ..Input::default()
        },
    );
    assert!(ui.preedit().is_some());
    assert!(!ui.nodes["a"].text_edit.as_ref().unwrap().native.is_empty());
    let selection = ui.sel("a");
    let state = node(&mut ui.nodes, "a");
    state.text_scroll = Some(12.0);
    state.stash = Some(Box::new(42_u32));
    ui.discard_input_for_view_change();
    assert_eq!(ui.focus_key(), Some("a"), "caller owns focus policy");
    assert!(ui.preedit().is_none());
    assert!(ui.ime_caret.is_none());
    assert!(ui.take_native_text("a").is_empty());
    assert_eq!(ui.sel("a"), selection);
    assert_eq!(ui.nodes["a"].text_scroll, Some(12.0));
    assert_eq!(
        ui.nodes["a"].stash.as_ref().unwrap().downcast_ref::<u32>(),
        Some(&42)
    );
    ui.focus("b");
    assert!(!widgets::text_input(&mut ui, "b", &mut b).changed);
    ui.focus("a");
    assert!(!widgets::text_input(&mut ui, "a", &mut a).changed);
    assert_eq!(a, "first");
    assert_eq!(b, "second");
}

fn slider() -> El {
    block(120., 40.).id("gain").focusable().a11y(A11y::Slider {
        value: 0.5,
        min: 0.0,
        max: 1.0,
    })
}

fn capture(ui: &mut Ui) {
    deliver(ui, slider(), Input::default());
    let edits = ui
        .frame(slider(), None, at(20., 20., true), 0.016)
        .unwrap()
        .edits
        .to_vec();
    assert_eq!(edits, vec![("gain".into(), Edit::Begin)]);
    assert!(ui.get("gain").held);
    ui.start_drag("gain", String::from("outgoing drag"));
    assert!(ui.dragging::<String>().is_some());
}

#[test]
fn view_change_cancels_capture_drag_and_owes_end_once_on_next_frame() {
    let mut ui = Ui::default();
    capture(&mut ui);
    ui.discard_input_for_view_change();
    ui.discard_input_for_view_change();
    assert!(!ui.get("gain").held);
    assert!(ui.dragging::<String>().is_none());
    let edits = ui
        .frame(slider(), None, at(20., 20., true), 0.016)
        .unwrap()
        .edits
        .to_vec();
    assert_eq!(edits, vec![("gain".into(), Edit::End)]);
    let edits = ui
        .frame(slider(), None, at(20., 20., false), 0.016)
        .unwrap()
        .edits
        .to_vec();
    assert!(edits.is_empty());
    assert!(ui.close().is_empty());
}

#[test]
fn view_change_owes_end_once_when_closed_without_another_frame() {
    let mut ui = Ui::default();
    capture(&mut ui);
    ui.discard_input_for_view_change();
    ui.discard_input_for_view_change();
    assert_eq!(ui.close(), vec![("gain".into(), Edit::End)]);
    assert!(ui.close().is_empty());
}

#[test]
fn view_change_drops_wheel_and_press_but_preserves_host_actions() {
    let mut ui = Ui::default();
    deliver(&mut ui, slider(), Input::default());
    let input = Input {
        pointer: at(20., 20., true),
        wheel: Vec2::new(0., 15.),
        ..Input::default()
    };
    deliver(&mut ui, slider(), input);
    assert!(ui.press_at.is_some());
    assert_ne!(ui.wheel, Vec2::ZERO);
    assert!(ui.request_action(SemanticAction::set_value("gain", 0.75)));
    ui.discard_input_for_view_change();
    assert!(ui.press_at.is_none());
    assert_eq!(ui.wheel, Vec2::ZERO);
    assert!(ui.wheel_claims.is_empty());
    let mut value = 0.5;
    assert!(ui.drag("gain", &mut value, 0.0..=1.0, 120., true));
    assert_eq!(value, 0.75, "queued host action survived the barrier");
}

#[test]
fn ordinary_cancel_keeps_delivered_text_and_keys() {
    let mut ui = Ui::default();
    let mut value = String::new();
    let root = widgets::text_input(&mut ui, "a", &mut value).el;
    deliver(&mut ui, root, Input::default());
    ui.focus("a");
    let root = widgets::text_input(&mut ui, "a", &mut value).el;
    deliver(
        &mut ui,
        root,
        Input {
            text: "normal".into(),
            keys: key(Key::Enter).keys,
            ..Input::default()
        },
    );
    ui.cancel();
    assert_eq!(ui.text("a"), "normal");
    assert_eq!(ui.keys("a")[0].key, Key::Enter);
    let response = widgets::text_edit(&mut ui, "a", &mut value, widgets::TextOpts::default());
    assert!(response.changed.changed);
    assert!(response.changed.submitted);
    assert_eq!(value, "normal");
}
