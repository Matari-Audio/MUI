//! Popovers, menus, selects and dialogs, driven through real frames: they
//! open, pick, close on an outside press or Escape, and a dialog's dim
//! keeps presses off what is under it.
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

fn escape() -> Input {
    Input {
        keys: vec![KeyPress {
            key: Key::Escape,
            mods: Mods::default(),
        }],
        ..Input::default()
    }
}

fn centre(ui: &Ui, id: &str) -> Point {
    let f = ui.scene().unwrap().surface(id).unwrap().frame;
    Point::new(f.x + f.size.width / 2., f.y + f.size.height / 2.)
}

/// Press and release at `p`, one frame each, then one quiet frame for the
/// tree that reads the click.
fn click(ui: &mut Ui, view: &mut dyn FnMut(&mut Ui) -> El, p: Point) {
    for input in [at(p, true), at(p, false), at(p, false)] {
        let tree = view(ui);
        ui.frame(tree, Some(SIZE), input, 0.016).unwrap();
    }
}

fn idle(ui: &mut Ui, view: &mut dyn FnMut(&mut Ui) -> El, input: Input) {
    let tree = view(ui);
    ui.frame(tree, Some(SIZE), input, 0.016).unwrap();
}

#[test]
fn a_select_opens_picks_and_closes() {
    let mut ui = Ui::default();
    let (wave, moved) = (Cell::new(0), Cell::new(0));
    let mut view = |ui: &mut Ui| {
        let mut w = wave.get();
        let s = select(ui, "wave", &["Saw", "Square", "Sine"], &mut w);
        wave.set(w);
        moved.set(moved.get() + usize::from(s.changed));
        col([s.el]).pad(M)
    };
    idle(&mut ui, &mut view, Input::default());
    assert!(ui.scene().unwrap().surface("wave/list").is_none());
    let button = centre(&ui, "wave");
    click(&mut ui, &mut view, button);
    assert!(ui.scene().unwrap().surface("wave/list").is_some(), "open");
    let sine = centre(&ui, "wave/list/2");
    click(&mut ui, &mut view, sine);
    assert_eq!((wave.get(), moved.get()), (2, 1));
    assert!(ui.scene().unwrap().surface("wave/list").is_none(), "closed");
    // Reopen, then a press anywhere else closes it without a pick.
    click(&mut ui, &mut view, button);
    assert!(ui.scene().unwrap().surface("wave/list").is_some());
    click(&mut ui, &mut view, Point::new(390., 290.));
    assert!(ui.scene().unwrap().surface("wave/list").is_none());
    assert_eq!((wave.get(), moved.get()), (2, 1));
}

#[test]
fn a_menu_reports_its_pick_and_escape_closes_it() {
    let mut ui = Ui::default();
    let (open, picked) = (Cell::new(false), Cell::new(None));
    let mut view = |ui: &mut Ui| {
        let b = button(ui, "file", "File");
        let mut o = open.get() ^ b.changed;
        let b = b.el.into_el();
        let m = menu(ui, "file-menu", "file", &mut o, &["Save", "Load"]);
        open.set(o);
        picked.set(picked.get().or(m.changed));
        let mut root = col([b]).pad(M);
        if let Some(m) = m.el {
            root = root.push(m);
        }
        root
    };
    idle(&mut ui, &mut view, Input::default());
    let file = centre(&ui, "file");
    click(&mut ui, &mut view, file);
    let load = centre(&ui, "file-menu/1");
    click(&mut ui, &mut view, load);
    assert_eq!(picked.get(), Some(1));
    assert!(ui.scene().unwrap().surface("file-menu").is_none());
    click(&mut ui, &mut view, file);
    assert!(ui.scene().unwrap().surface("file-menu").is_some());
    idle(&mut ui, &mut view, escape());
    idle(&mut ui, &mut view, Input::default());
    assert!(ui.scene().unwrap().surface("file-menu").is_none());
}

#[test]
fn a_dialog_dims_the_window_and_takes_its_presses() {
    let mut ui = Ui::default();
    let (open, under) = (Cell::new(true), Cell::new(0));
    let mut view = |ui: &mut Ui| {
        let b = button(ui, "under", "Under");
        under.set(under.get() + usize::from(b.changed));
        let mut root = col([b.el.into_el()]).w(Len::Pct(100.)).h(Len::Pct(100.));
        let mut o = open.get();
        if let Some(d) = dialog(ui, "confirm", &mut o, text("Discard?")) {
            root = root.push(d);
        }
        open.set(o);
        root
    };
    idle(&mut ui, &mut view, Input::default());
    let scrim = ui.scene().unwrap().surface("confirm/scrim").unwrap().frame;
    assert_eq!((scrim.size.width, scrim.size.height), (400., 300.));
    // A press inside the card keeps it open.
    let card = centre(&ui, "confirm");
    click(&mut ui, &mut view, card);
    assert!(open.get());
    // The button is under the dim: the press closes the dialog, not it.
    let b = centre(&ui, "under");
    click(&mut ui, &mut view, b);
    assert!(!open.get());
    assert_eq!(under.get(), 0);
}

#[test]
fn a_long_select_scrolls_inside_the_window_to_its_last_option() {
    let mut ui = Ui::default();
    let names: Vec<String> = (0..30).map(|i| format!("Option {i}")).collect();
    let picked = Cell::new(0);
    let mut view = |ui: &mut Ui| {
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut i = picked.get();
        let s = select(ui, "long", &names, &mut i);
        picked.set(i);
        col([s.el]).pad(M)
    };
    idle(&mut ui, &mut view, Input::default());
    let button = centre(&ui, "long");
    click(&mut ui, &mut view, button);
    let list = ui.scene().unwrap().surface("long/list").unwrap().frame;
    assert!(
        list.y >= 0. && list.y + list.size.height <= 300.,
        "{list:?}"
    );
    // Wheel the list down until its last option is inside it, then pick it.
    let over = Point::new(
        list.x + list.size.width / 2.,
        list.y + list.size.height / 2.,
    );
    for _ in 0..40 {
        let mut input = at(over, false);
        input.wheel = Vec2::new(0., 60.);
        idle(&mut ui, &mut view, input);
    }
    let last = centre(&ui, "long/list/29");
    assert!(
        last.y > list.y && last.y < list.y + list.size.height,
        "{last:?} not in {list:?}"
    );
    click(&mut ui, &mut view, last);
    assert_eq!(picked.get(), 29);
}

#[test]
fn a_dialog_holds_the_keyboard_and_gives_focus_back() {
    let mut ui = Ui::default();
    let (open, name) = (Cell::new(false), std::cell::RefCell::new(String::new()));
    let mut view = |ui: &mut Ui| {
        let field = text_input(ui, "name", &mut name.borrow_mut()).el.into_el();
        let mut root = col([field]).w(Len::Pct(100.)).h(Len::Pct(100.));
        let mut o = open.get();
        let body = col![
            button(ui, "ok", "OK").el.into_el(),
            button(ui, "no", "No").el.into_el()
        ];
        if let Some(d) = dialog(ui, "ask", &mut o, body) {
            root = root.push(d);
        }
        open.set(o);
        root
    };
    idle(&mut ui, &mut view, Input::default());
    ui.focus("name");
    let typed = |s: &str| Input {
        text: s.into(),
        ..Input::default()
    };
    idle(&mut ui, &mut view, typed("a"));
    idle(&mut ui, &mut view, Input::default());
    assert_eq!(*name.borrow(), "a");
    open.set(true);
    idle(&mut ui, &mut view, Input::default());
    idle(&mut ui, &mut view, typed("b"));
    idle(&mut ui, &mut view, Input::default());
    assert_eq!(
        *name.borrow(),
        "a",
        "typing reached the field under the dialog"
    );
    assert!(ui.focused("ok"), "focus did not move into the dialog");
    // Tab cycles the dialog's two buttons, never the field.
    let tab = || Input {
        keys: vec![KeyPress {
            key: Key::Tab,
            mods: Mods::default(),
        }],
        ..Input::default()
    };
    for _ in 0..3 {
        idle(&mut ui, &mut view, tab());
        assert!(!ui.focused("name"));
    }
    open.set(false);
    idle(&mut ui, &mut view, Input::default());
    idle(&mut ui, &mut view, Input::default());
    assert!(ui.focused("name"), "focus did not come back");
}

#[test]
fn tab_through_a_long_select_keeps_the_focused_row_in_view() {
    let mut ui = Ui::default();
    let names: Vec<String> = (0..30).map(|i| format!("Option {i}")).collect();
    let picked = Cell::new(0);
    let mut view = |ui: &mut Ui| {
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut i = picked.get();
        let s = select(ui, "long", &names, &mut i);
        picked.set(i);
        col([s.el]).pad(M)
    };
    idle(&mut ui, &mut view, Input::default());
    let button = centre(&ui, "long");
    click(&mut ui, &mut view, button);
    let list = ui.scene().unwrap().surface("long/list").unwrap().frame;
    let tab = || Input {
        keys: vec![KeyPress {
            key: Key::Tab,
            mods: Mods::default(),
        }],
        ..Input::default()
    };
    // The button, then every option in turn.
    for n in 0..20 {
        idle(&mut ui, &mut view, tab());
        idle(&mut ui, &mut view, Input::default());
        let Some(row) = (0..30).find(|i| ui.focused(format!("long/list/{i}"))) else {
            assert_eq!(n, 0, "focus left the list");
            continue;
        };
        let f = ui
            .scene()
            .unwrap()
            .surface(&format!("long/list/{row}"))
            .unwrap()
            .frame;
        assert!(
            f.y >= list.y - 0.5 && f.y + f.size.height <= list.y + list.size.height + 0.5,
            "row {row} at {f:?} outside {list:?}"
        );
    }
}

#[test]
fn stacked_dialogs_give_focus_back_in_order() {
    let mut ui = Ui::default();
    let (outer, inner) = (Cell::new(false), Cell::new(false));
    let mut view = |ui: &mut Ui| {
        let field = button(ui, "field", "Field").el.into_el();
        let mut root = col([field]).w(Len::Pct(100.)).h(Len::Pct(100.));
        let (mut o, mut i) = (outer.get(), inner.get());
        let a = button(ui, "a", "A").el.into_el();
        if let Some(d) = dialog(ui, "outer", &mut o, col![a]) {
            root = root.push(d);
        }
        let b = button(ui, "b", "B").el.into_el();
        if let Some(d) = dialog(ui, "inner", &mut i, col![b]) {
            root = root.push(d);
        }
        outer.set(o);
        inner.set(i);
        root
    };
    idle(&mut ui, &mut view, Input::default());
    ui.focus("field");
    outer.set(true);
    idle(&mut ui, &mut view, Input::default());
    idle(&mut ui, &mut view, Input::default());
    assert!(ui.focused("a"));
    inner.set(true);
    idle(&mut ui, &mut view, Input::default());
    idle(&mut ui, &mut view, Input::default());
    assert!(ui.focused("b"));
    inner.set(false);
    idle(&mut ui, &mut view, Input::default());
    assert!(
        ui.focused("a"),
        "closing the inner dialog did not return to the outer"
    );
    outer.set(false);
    idle(&mut ui, &mut view, Input::default());
    assert!(ui.focused("field"), "the background lost its focus");
}
