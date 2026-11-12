//! Tabs, toasts, a table and tree rows, driven through real frames.
use std::cell::Cell;

use mui::prelude::*;

const SIZE: Size = Size::new(400., 300.);

fn at(p: Point, down: bool) -> Input {
    Input {
        pointer: PointerInput {
            pos: Some(p),
            buttons: Buttons::default().set(Button::Primary, down),
            ..PointerInput::default()
        },
        ..Input::default()
    }
}

fn key(k: Key) -> Input {
    Input {
        keys: vec![KeyPress {
            key: k,
            mods: Mods::default(),
        }],
        ..Input::default()
    }
}

fn centre(ui: &Ui, id: &str) -> Point {
    let f = ui.scene().unwrap().surface(id).unwrap().frame;
    Point::new(f.x + f.size.width / 2., f.y + f.size.height / 2.)
}

fn frame(ui: &mut Ui, view: &mut dyn FnMut(&mut Ui) -> El, input: Input, dt: f64) {
    let tree = view(ui);
    ui.frame(tree, Some(SIZE), input, dt).unwrap();
}

fn click(ui: &mut Ui, view: &mut dyn FnMut(&mut Ui) -> El, p: Point) {
    for input in [at(p, true), at(p, false), at(p, false)] {
        frame(ui, view, input, 0.016);
    }
}

#[test]
fn tabs_pick_by_press_and_by_arrow_keys() {
    let mut ui = Ui::default();
    let page = Cell::new(0);
    let mut view = |ui: &mut Ui| {
        let mut p = page.get();
        let t = tabs(ui, "pages", &["Osc", "Filter", "FX"], &mut p);
        page.set(p);
        col([t.el]).w(Len::Pct(100.0)).h(Len::Pct(100.0))
    };
    frame(&mut ui, &mut view, Input::default(), 0.016);
    let p = centre(&ui, "pages/2");
    click(&mut ui, &mut view, p);
    assert_eq!(page.get(), 2);
    ui.focus("pages/2");
    frame(&mut ui, &mut view, key(Key::Right), 0.016);
    frame(&mut ui, &mut view, Input::default(), 0.016);
    assert_eq!(page.get(), 0, "Right wraps past the last tab");
    assert!(ui.focused("pages/0"), "focus follows the selection");
    frame(&mut ui, &mut view, key(Key::Left), 0.016);
    frame(&mut ui, &mut view, Input::default(), 0.016);
    assert_eq!(page.get(), 2);
}

#[test]
fn a_toast_shows_lets_presses_through_and_goes_after_its_time() {
    let mut ui = Ui::default();
    let (pressed, fire) = (Cell::new(0), Cell::new(true));
    let mut view = |ui: &mut Ui| {
        if fire.replace(false) {
            toast(ui, "Preset saved");
        }
        let go = button(ui, "go", "Go");
        if go.changed {
            pressed.set(pressed.get() + 1);
        }
        // Under the full-window toast layer, clear of its card.
        let mut root = col([stack([]).h(100.0), go.el.into_el()])
            .w(Len::Pct(100.0))
            .h(Len::Pct(100.0));
        if let Some(t) = toasts(ui) {
            root = root.push(t);
        }
        root
    };
    frame(&mut ui, &mut view, Input::default(), 0.016);
    assert!(ui.scene().unwrap().surface("mui/toasts/0").is_some());
    // The layer covers the window, but only its cards take presses.
    let p = centre(&ui, "go");
    click(&mut ui, &mut view, p);
    assert_eq!(pressed.get(), 1, "the toast layer swallowed a press");
    // Up through its time, gone after it.
    frame(&mut ui, &mut view, Input::default(), TOAST_SECS - 0.5);
    assert!(ui.scene().unwrap().surface("mui/toasts/0").is_some());
    frame(&mut ui, &mut view, Input::default(), 1.0);
    frame(&mut ui, &mut view, Input::default(), 0.016);
    assert!(ui.scene().unwrap().surface("mui/toasts/0").is_none());
}

#[test]
fn a_press_dismisses_a_toast() {
    let mut ui = Ui::default();
    let fire = Cell::new(true);
    let mut view = |ui: &mut Ui| {
        if fire.replace(false) {
            toast(ui, "Saved");
        }
        let mut root = col([]).w(Len::Pct(100.0)).h(Len::Pct(100.0));
        if let Some(t) = toasts(ui) {
            root = root.push(t);
        }
        root
    };
    frame(&mut ui, &mut view, Input::default(), 0.016);
    let p = centre(&ui, "mui/toasts/0");
    click(&mut ui, &mut view, p);
    frame(&mut ui, &mut view, Input::default(), 0.016);
    assert!(ui.scene().unwrap().surface("mui/toasts/0").is_none());
}

#[test]
fn a_table_builds_its_header_and_only_the_visible_rows() {
    let mut ui = Ui::default();
    let mut state = ListState::uniform(0..10_000u64, 24.0);
    let built = Cell::new(0);
    let cols = [
        Column {
            title: "Name",
            width: 160.0,
        },
        Column {
            title: "Size",
            width: 80.0,
        },
    ];
    let t = table(
        &mut ui,
        "files",
        &mut state,
        ListOptions::new(240.0, 228.0),
        &cols,
        |_, item, c| {
            built.set(built.get() + 1);
            text(format!("{}:{c}", item.index))
        },
    );
    assert_eq!(
        t.changed.visible,
        0..9,
        "200 px of 24 px rows under a 28 px header"
    );
    assert!(
        built.get() < 40,
        "built {} cells for 10 000 rows",
        built.get()
    );
    ui.frame(t.el, Some(SIZE), Input::default(), 0.016).unwrap();
    assert!(ui.scene().unwrap().surface("files").is_some());
}

#[test]
fn a_tree_row_flips_its_branch() {
    let mut ui = Ui::default();
    let open = Cell::new(false);
    let mut view = |ui: &mut Ui| {
        let mut o = open.get();
        let r = tree_row(ui, "bank/drums", "Drums", 1, Some(&mut o));
        open.set(o);
        let mut root = col([r.el]).w(Len::Pct(100.0)).h(Len::Pct(100.0));
        if open.get() {
            root = root.push(tree_row(ui, "bank/drums/kick", "Kick", 2, None).el);
        }
        root
    };
    frame(&mut ui, &mut view, Input::default(), 0.016);
    let p = centre(&ui, "bank/drums/toggle");
    click(&mut ui, &mut view, p);
    assert!(open.get());
    frame(&mut ui, &mut view, Input::default(), 0.016);
    let (branch, leaf) = {
        let s = ui.scene().unwrap();
        (
            s.surface("bank/drums").unwrap().frame,
            s.surface("bank/drums/kick").unwrap().frame,
        )
    };
    assert!(leaf.y > branch.y);
}
