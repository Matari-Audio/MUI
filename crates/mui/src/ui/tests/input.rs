use super::*;

/// The two halves of `.disabled` are one feature: the look it declared
/// for `State::Disabled` is painted, the look it declared for a hover is
/// not, and the pointer sitting on it produces no gesture at all.
#[test]
fn a_disabled_node_paints_its_off_look_and_hits_nothing() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let tree = |off: bool| {
        block(100., 100.)
            .fill(Role::Field)
            .on(State::Hover, |s| s.fill(Role::Primary))
            .on(State::Disabled, |s| s.fill(Role::Dim))
            .when(off, Styled::disabled)
            .focusable()
            .a11y(A11y::Button)
            .id("bypass")
    };
    let fill = |f: &Frame<'_>| {
        f.scene
            .paint
            .iter()
            .find(|p| &*p.key == "bypass")
            .expect("painted")
            .paint
            .solid()
    };
    let pal = Theme::DEFAULT.palette;
    let role = |r: Role| match Fill::from(r).paint(&pal, pal.background()) {
        Some(Paint::Solid(c)) => c,
        _ => panic!("a role paints solid"),
    };
    // Live: two frames, because a gesture reads the previous hit map.
    ui.frame(tree(false), None, at(50., 50., false), 0.016)
        .unwrap();
    for _ in 0..12 {
        ui.frame(tree(false), None, at(50., 50., false), 0.016)
            .unwrap();
    }
    let frame = ui
        .frame(tree(false), None, at(50., 50., false), 0.016)
        .unwrap();
    let lit = fill(&frame);
    assert!(ui.get("bypass").hovered, "live, and under the pointer");
    assert_eq!(lit, role(Role::Primary), "the explicit hover look, once");

    for _ in 0..2 {
        ui.frame(tree(true), None, at(50., 50., true), 0.016)
            .unwrap();
    }
    let off = fill(
        &ui.frame(tree(true), None, at(50., 50., true), 0.016)
            .unwrap(),
    );
    let r = ui.get("bypass");
    assert!(!r.hovered && !r.pressed && !r.held, "no gesture: {r:?}");
    assert_eq!(off, role(Role::Dim), "the disabled look, unlifted");

    // And it is no Tab stop -- nor does it keep a focus it already had.
    ui.frame(tree(true), None, key(Key::Tab), 0.016).unwrap();
    assert!(!ui.focused("bypass"));
    ui.focus("bypass");
    ui.frame(tree(true), None, PointerInput::default(), 0.016)
        .unwrap();
    assert!(!ui.focused("bypass"), "a focus on a dead node is dropped");
}

/// A shortcut is not focus-gated -- except by a field that is typing.
#[test]
fn a_shortcut_fires_unfocused_and_never_while_a_field_has_the_focus() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let mut value = String::new();
    let undo = |ui: &Ui| {
        ui.shortcuts()
            .iter()
            .any(|k| k.key == Key::Function(1) || k.key == Key::Space)
    };
    let tree = |ui: &mut Ui, v: &mut String| widgets::text_input(ui, "f", v).el;

    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, key(Key::Function(1)), 0.016).unwrap();
    assert!(undo(&ui), "nothing is focused, so the shortcut is ours");

    ui.focus("f");
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, key(Key::Space), 0.016).unwrap();
    assert!(!undo(&ui), "the field is typing: the key is its own");
    assert_eq!(ui.keys("f").len(), 1, "and it still gets it");
}

#[test]
fn tab_walks_the_focusable_surfaces_in_scene_order() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let tree = || {
        col([
            block(20., 20.).focusable().id("a"),
            block(20., 20.).id("plain"),
            block(20., 20.).focusable().id("b"),
        ])
    };
    ui.frame(tree(), None, PointerInput::default(), 0.016)
        .unwrap();
    for want in ["a", "b", "a"] {
        ui.frame(tree(), None, key(Key::Tab), 0.016).unwrap();
        assert!(ui.focused(want), "expected {want}");
    }
    ui.frame(tree(), None, key(Key::Escape), 0.016).unwrap();
    assert!(!ui.focused("a") && !ui.focused("b"));
}

/// A shipped button is a real Tab stop and Enter reaches the same action
/// path as a primary click.
#[test]
fn a_button_can_be_focused_and_activated_from_the_keyboard() {
    fn tree(ui: &mut Ui) -> El {
        widgets::button(ui, "button", "Save").el.into_el()
    }

    let mut ui = Ui::new(Theme::DEFAULT);
    let root = tree(&mut ui);
    ui.frame(root, None, PointerInput::default(), 0.016)
        .unwrap();
    let root = tree(&mut ui);
    ui.frame(root, None, key(Key::Tab), 0.016).unwrap();
    assert_eq!(ui.focus_key(), Some("button"));

    // Keys are delivered to the tree on the following frame, exactly as
    // mouse edges are, so the widget sees the same activation boundary.
    let root = tree(&mut ui);
    ui.frame(root, None, key(Key::Enter), 0.016).unwrap();
    let activated = widgets::button(&mut ui, "button", "Save").changed;
    assert!(activated, "Enter activates the focused button");
}

#[test]
fn typing_reaches_the_focused_field() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let mut value = String::new();
    let tree = |ui: &mut Ui, v: &mut String| widgets::text_input(ui, "f", v).el;
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, PointerInput::default(), 0.016)
        .unwrap();
    ui.focus("f");
    let typed = Input {
        text: "hi".into(),
        ..Input::default()
    };
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, typed, 0.016).unwrap();
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, key(Key::Backspace), 0.016).unwrap();
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, PointerInput::default(), 0.016)
        .unwrap();
    assert_eq!(value, "h");
}

/// A field tells a screen reader where its selection is and where each
/// character sits, and a reader's `SetTextSelection` moves it.
#[test]
fn a_field_reports_its_selection_and_carets_and_a_reader_can_select() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let mut value = String::from("héllo");
    let tree = |ui: &mut Ui, v: &mut String| widgets::text_input(ui, "f", v).el;
    let text = |ui: &Ui| match &ui.scene().unwrap().surface("f").unwrap().semantics {
        Some(mui_scene::Semantics {
            role: A11y::TextInput {
                selection, carets, ..
            },
            ..
        }) => (*selection, carets.clone()),
        _ => panic!("not a text input"),
    };
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, Input::default(), 0.016).unwrap();
    ui.focus("f");
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, key(Key::End), 0.016).unwrap();
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, Input::default(), 0.016).unwrap();
    let (sel, carets) = text(&ui);
    assert_eq!(sel, (5, 5), "the caret went to the end");
    assert_eq!(carets.len(), 6, "one per boundary of five characters");
    assert_eq!(carets[0], 8.0, "the text starts inside the padding");
    assert!(carets.windows(2).all(|w| w[1] > w[0]), "{carets:?}");

    assert!(ui.request_action(SemanticAction::set_selection("f", 1, 4)));
    assert!(!ui.request_action(SemanticAction::set_selection("f", 0, 6)));
    let root = tree(&mut ui, &mut value);
    let edits = ui
        .frame(root, None, Input::default(), 0.016)
        .unwrap()
        .edits
        .len();
    assert_eq!(text(&ui).0, (1, 4), "the reader's selection landed");
    assert_eq!(edits, 0, "a selection is not a bracketed edit");
    assert_eq!(value, "héllo");
}

#[test]
fn a_composition_paints_without_editing_the_value_and_the_commit_inserts() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let mut value = "ab".to_owned();
    let tree = |ui: &mut Ui, v: &mut String| widgets::text_input(ui, "f", v).el;
    let ime = |e: mui_input::Ime| Input {
        ime: vec![e],
        ..Input::default()
    };
    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, PointerInput::default(), 0.016)
        .unwrap();
    ui.focus("f");
    ui.set_sel("f", 1, 1);

    let root = tree(&mut ui, &mut value);
    let f = ui
        .frame(
            root,
            None,
            ime(mui_input::Ime::Preedit {
                text: "xy".into(),
                cursor: Some((1, 1)),
            }),
            0.016,
        )
        .unwrap();
    let (at, _) = f.ime.expect("the host is told where the caret is");
    assert!(
        at.x > 0.0,
        "and the caret is in the scene's space, not the field's"
    );
    // The preedit reaches the *next* tree, as every input does here.
    let root = tree(&mut ui, &mut value);
    let f = ui
        .frame(root, None, PointerInput::default(), 0.016)
        .unwrap();
    assert_eq!(value, "ab", "a preedit never touches the value");
    let under = f.scene.surface("/3").expect("underline").frame;
    assert!(under.size.width > 0., "the composing span is underlined");

    let root = tree(&mut ui, &mut value);
    ui.frame(root, None, ime(mui_input::Ime::Commit("xy".into())), 0.016)
        .unwrap();
    let root = tree(&mut ui, &mut value);
    let f = ui
        .frame(root, None, PointerInput::default(), 0.016)
        .unwrap();
    assert_eq!(value, "axyb", "the commit landed at the caret");
    assert!(
        f.scene.surface("/3").is_none(),
        "and the composition, with it the underline, is gone"
    );
}

#[test]
fn a_long_value_scrolls_under_the_clip_instead_of_wrapping() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let mut value = "x".repeat(60);
    let win = Some(Size::new(200., 60.));
    let tree = |ui: &mut Ui, v: &mut String| widgets::text_input(ui, "f", v).el;
    let root = tree(&mut ui, &mut value);
    ui.frame(root, win, PointerInput::default(), 0.016).unwrap();
    ui.set_sel("f", 60, 60);
    let root = tree(&mut ui, &mut value);
    let f = ui.frame(root, win, PointerInput::default(), 0.016).unwrap();
    // The value, the caret and the field: unnamed children are keyed by
    // their slot under the root.
    let field = f.scene.surface("f").expect("field").frame;
    let text = f.scene.surface("/1").expect("value").frame;
    let caret = f.scene.surface("/2").expect("caret").frame;
    assert!(
        text.size.height < 2. * ui.theme.text,
        "one line, not wrapped: {text:?}"
    );
    assert!(
        caret.right() <= field.right() && caret.x >= field.x,
        "the caret stayed in the field: {caret:?} in {field:?}"
    );
}

#[test]
fn a_selection_is_extended_by_shift_and_deleted_as_one() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let mut value = String::from("hello");
    let run = |ui: &mut Ui, v: &mut String, input: Input| {
        let root = widgets::text_input(ui, "f", v).el;
        ui.frame(root, None, input, 0.016).unwrap();
    };
    run(&mut ui, &mut value, Input::default());
    ui.focus("f");
    let shift = |k| Input {
        keys: vec![KeyPress {
            key: k,
            mods: Mods {
                shift: true,
                ..Mods::default()
            },
        }],
        ..Input::default()
    };
    run(&mut ui, &mut value, shift(Key::Right));
    run(&mut ui, &mut value, shift(Key::Right));
    run(&mut ui, &mut value, key(Key::Backspace));
    run(&mut ui, &mut value, Input::default());
    assert_eq!(value, "llo", "two characters selected, one Backspace");
}

#[test]
fn copy_asks_the_host_for_the_clipboard_and_paste_takes_it_back() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let mut value = String::from("hi");
    let run = |ui: &mut Ui, v: &mut String, input: Input| {
        let root = widgets::text_input(ui, "f", v).el;
        ui.frame(root, None, input, 0.016)
            .unwrap()
            .clipboard
            .clone()
    };
    run(&mut ui, &mut value, Input::default());
    ui.focus("f");
    let ctrl = |c: char, clipboard: Option<String>| Input {
        keys: vec![KeyPress {
            key: Key::Char(c),
            mods: Mods {
                ctrl: true,
                ..Mods::default()
            },
        }],
        clipboard,
        ..Input::default()
    };
    run(&mut ui, &mut value, ctrl('a', None));
    run(&mut ui, &mut value, ctrl('c', None));
    let out = run(&mut ui, &mut value, Input::default());
    assert_eq!(out.as_deref(), Some("hi"), "the copy reached the frame");
    run(&mut ui, &mut value, ctrl('v', Some("yo".into())));
    run(&mut ui, &mut value, Input::default());
    assert_eq!(value, "yo", "and a paste replaced the selection");
}

#[test]
fn a_live_readout_swaps_its_glyphs_without_resolving_again() {
    use std::cell::Cell;
    use std::rc::Rc;

    let walks = Rc::new(Cell::new(0));
    let (w, seen) = (walks.clone(), walks.clone());
    let tree = move || {
        let w = w.clone();
        row![
            text("0.0").reserve("-88.8").id("gain"),
            canvas(move |_| {
                w.set(w.get() + 1);
                Vec::new()
            })
            .size(10., 10.)
        ]
    };
    let mut ui =
        Ui::new(Theme::DEFAULT).font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    ui.frame(tree(), Some(Size::new(300., 40.)), Input::default(), 0.016)
        .unwrap();
    assert_eq!(seen.get(), 1, "the one resolve");

    let glyphs = |ui: &Ui| {
        let t = ui
            .scene()
            .unwrap()
            .paint
            .iter()
            .find(|p| p.layer == mui_scene::Layer::Text);
        t.unwrap().text.clone().unwrap().glyphs
    };
    let (before, frame) = (
        glyphs(&ui),
        ui.scene().unwrap().surface("gain").unwrap().frame,
    );
    let hit_geometry = ui.scene().unwrap().clone();
    for i in 0..32 {
        ui.set_text("gain", format!("-{i}.5")).unwrap();
    }
    assert_eq!(seen.get(), 1, "32 readouts, still one layout resolve");
    assert_ne!(glyphs(&ui), before, "and the glyphs did change");
    assert_eq!(ui.scene().unwrap().surface("gain").unwrap().frame, frame);
    assert!(same_hit_geometry(&hit_geometry, ui.scene().unwrap()));
}

#[test]
fn set_text_says_so_when_there_is_nothing_to_set() {
    let mut ui =
        Ui::new(Theme::DEFAULT).font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    assert!(matches!(
        ui.set_text("gain", "1"),
        Err(SceneError::NoTextLayer)
    ));
    let tree = row![block(20., 20.).id("box")];
    ui.frame(tree, Some(Size::new(80., 40.)), Input::default(), 0.016)
        .unwrap();
    assert!(matches!(
        ui.set_text("box", "1"),
        Err(SceneError::NoTextLayer)
    ));
    assert!(matches!(
        ui.set_text("nope", "1"),
        Err(SceneError::NoTextLayer)
    ));
}

#[test]
fn fallback_text_input_caret_uses_the_fallback_advance() {
    let ui = Ui::new(Theme::DEFAULT)
        .font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap())
        .fallback_font(Font::new(epaint_default_fonts::NOTO_EMOJI_REGULAR).unwrap());
    let value = "A😀";
    let carets = ui.carets(value, 16.);
    let [_, (_, before), (_, end)] = carets[..] else {
        panic!("{carets:?}");
    };
    assert!(end > before, "fallback glyph has no caret advance");
    assert_eq!(ui.hit(value, 16., end), value.chars().count());
}

/// The unnamed root is decoration like any unnamed node: it does not
/// hover as `""`. Clicking the empty background still drops the focus,
/// because a press that lands on no target does -- not because the root
/// happened to be one.
#[test]
fn empty_background_is_no_target_and_a_press_on_it_drops_the_focus() {
    let tree = || {
        // The field sits centred along the top: x 80..120, y 0..20.
        col![block(40., 20.).focusable().id("field")]
            .size(200., 200.)
            .fill(Role::Background)
    };
    let mut ui = Ui::new(Theme::DEFAULT);
    ui.frame(tree(), None, Input::default(), 0.016).unwrap();
    ui.frame(tree(), None, at(100., 10., true), 0.016).unwrap();
    assert!(ui.focused("field"), "a press on the field focuses it");
    ui.frame(tree(), None, at(100., 10., false), 0.016).unwrap();
    ui.frame(tree(), None, at(150., 150., false), 0.016)
        .unwrap();
    assert_eq!(ui.interaction.hovered(), None, "the root is no target");
    assert!(ui.focused("field"), "hovering the background keeps it");
    ui.frame(tree(), None, at(150., 150., true), 0.016).unwrap();
    assert!(!ui.focused("field"), "a press on nothing drops it");
    assert_eq!(ui.interaction.held(), None, "and captures nothing");

    // A press that starts on the field and a second button going down
    // elsewhere mid-gesture is still the field's gesture.
    ui.frame(tree(), None, at(100., 10., false), 0.016).unwrap();
    ui.frame(tree(), None, at(100., 10., true), 0.016).unwrap();
    let both = PointerInput {
        pos: Some(Point::new(150., 150.)),
        buttons: Buttons::PRIMARY.set(Button::Secondary, true),
        ..PointerInput::default()
    };
    ui.frame(tree(), None, both, 0.016).unwrap();
    assert!(ui.focused("field"), "held, the field keeps the focus");
}

/// Enter on a button and an arrow on a slider are edits a plugin host
/// has to see bracketed, on the frame the tree applied them.
#[test]
fn keyboard_edits_are_bracketed_and_a_slider_steps() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let mut v = 0.5;
    let mut clicks = 0;
    let mut tree = |ui: &mut Ui| {
        let widgets::Response {
            el: b,
            changed: clicked,
        } = widgets::button(ui, "b", "Go");
        clicks += usize::from(clicked);
        let s = widgets::slider(ui, "s", "S", &mut v, 0.0..=1.0).el;
        col([b.el(), s.el()]).width(200.)
    };
    let root = tree(&mut ui);
    ui.frame(root, None, Input::default(), 0.016).unwrap();
    ui.focus("b");
    let root = tree(&mut ui);
    ui.frame(root, None, key(Key::Enter), 0.016).unwrap();
    let root = tree(&mut ui);
    let f = ui.frame(root, None, Input::default(), 0.016).unwrap();
    assert_eq!(
        f.edits,
        [("b".into(), Edit::Begin), ("b".into(), Edit::End)]
    );
    ui.focus("s");
    let shifted = |k| Input {
        keys: vec![KeyPress {
            key: k,
            mods: Mods {
                shift: true,
                ..Mods::default()
            },
        }],
        ..Input::default()
    };
    for input in [key(Key::Right), shifted(Key::Left), key(Key::PageDown)] {
        let root = tree(&mut ui);
        ui.frame(root, None, input, 0.016).unwrap();
    }
    let root = tree(&mut ui);
    let f = ui.frame(root, None, key(Key::End), 0.016).unwrap();
    assert_eq!(
        f.edits,
        [("s".into(), Edit::Begin), ("s".into(), Edit::End)]
    );
    assert_eq!(clicks, 1);
    assert!((v - 0.409).abs() < 1e-9, "+0.01, -0.001, -0.1: {v}");
    let mut tree = |ui: &mut Ui| {
        widgets::slider(ui, "s", "S", &mut v, 0.0..=1.0)
            .el
            .into_el()
    };
    let root = tree(&mut ui);
    ui.frame(root, None, Input::default(), 0.016).unwrap();
    assert_eq!(v, 1.0, "End goes to the end");
}

/// A platform's Increment takes the arrow keys' step, as a `SetValue`.
#[test]
fn increment_and_decrement_step_like_the_arrow_keys() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let mut v = 0.5;
    let root = widgets::slider(&mut ui, "s", "S", &mut v, 0.0..=1.0)
        .el
        .into_el();
    ui.frame(root, None, Input::default(), 0.016).unwrap();
    assert!(ui.request_action(SemanticAction::increment("s")));
    assert!(ui.request_action(SemanticAction::increment("s")));
    assert!(ui.request_action(SemanticAction::decrement("s")));
    let _ = widgets::slider(&mut ui, "s", "S", &mut v, 0.0..=1.0);
    assert!((v - 0.51).abs() < 1e-12, "+0.01 +0.01 -0.01: {v}");
    let f = ui
        .frame(block(1., 1.), None, Input::default(), 0.016)
        .unwrap();
    assert_eq!(
        f.edits,
        [("s".into(), Edit::Begin), ("s".into(), Edit::End)]
    );
    assert!(!ui.request_action(SemanticAction::decrement("absent")));
}

/// A click in a field scrolled to its tail lands where the text was
/// painted, not where it would be unscrolled.
#[test]
fn a_click_in_a_scrolled_field_lands_on_the_painted_character() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let mut value = "x".repeat(60);
    let win = Some(Size::new(200., 60.));
    let tree = |ui: &mut Ui, v: &mut String| widgets::text_input(ui, "f", v).el;
    let root = tree(&mut ui, &mut value);
    ui.frame(root, win, PointerInput::default(), 0.016).unwrap();
    ui.set_sel("f", 60, 60);
    let root = tree(&mut ui, &mut value);
    let f = ui.frame(root, win, PointerInput::default(), 0.016).unwrap();
    let field = f.scene.surface("f").unwrap().frame;
    let (x, y) = (field.right() - 10., field.y + field.size.height / 2.);
    let root = tree(&mut ui, &mut value);
    ui.frame(root, win, at(x, y, true), 0.016).unwrap();
    tree(&mut ui, &mut value);
    let (_, caret) = ui.sel("f");
    assert!(caret >= 58, "the tail was under the pointer, got {caret}");
}
