//! The three claims that hold every control together: a variant is arithmetic
//! on one role, one theme unit steps every size, and a readout says what it
//! was given.
use mui::prelude::*;

fn face(v: Variant) -> Style {
    let mut ui = Ui::new(Theme::DEFAULT);
    button(&mut ui, "b", "Save")
        .0
        .variant(v)
        .el()
        .payload()
        .style
        .clone()
}

/// Each variant is arithmetic on one role: filled, faded, stroked, bare.
#[test]
fn a_variant_paints_the_role_without_naming_a_second_colour() {
    assert_eq!(face(Variant::Solid).fill, Fill::Role(Role::Primary));
    assert_eq!(face(Variant::Soft).fill, Fill::Faded(Role::Primary, 0.18));
    let outline = face(Variant::Outline);
    assert_eq!(outline.fill, Fill::None);
    assert_eq!(
        outline.stroke.map(|s| s.fill),
        Some(Fill::Role(Role::Primary))
    );
    assert_eq!(face(Variant::Ghost).fill, Fill::None);
    // A filled face carries contrast ink; the rest speak as the role.
    let ink = |v: Variant| {
        let mut ui = Ui::new(Theme::DEFAULT);
        let el = button(&mut ui, "b", "Save").0.variant(v).el();
        el.children()[0].payload().style.fill.clone()
    };
    assert_eq!(ink(Variant::Solid), Fill::Role(Role::Ink));
    assert_eq!(ink(Variant::Ghost), Fill::Role(Role::Primary));
}

/// One `Theme.control` unit, five steps, and pixels as the escape hatch.
#[test]
fn the_size_scale_steps_every_control_off_the_theme_unit() {
    let width = |c: Control| {
        let scene = resolve_scene(&SceneSpec::new(c.el())).expect("resolves");
        scene.surface("sw").expect("the toggle").frame.size.width
    };
    let mut ui = Ui::new(Theme::DEFAULT);
    let mut sw = |size: SpacingToken| {
        let mut on = false;
        toggle(&mut ui, "sw", &mut on).0.size(size)
    };
    let (xs, m, xl) = (width(sw(Xs)), width(sw(M)), width(sw(Xl)));
    assert!(xs < m && m < xl, "{xs} {m} {xl}");
    // M is ten units of the default 4 px control step.
    assert!((m - 40.).abs() < 0.01, "{m}");
    // Steps are even: Xs and Xl sit two units either side of M.
    assert!(((xl - m) - (m - xs)).abs() < 0.01);
    assert!((width(sw(M).px(64.)) - 64.).abs() < 0.01);
}

/// The glyph runs a control resolves to, in paint order.
fn runs(c: Control) -> Vec<usize> {
    let spec = SceneSpec::new(c.el()).font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    resolve_scene(&spec)
        .expect("resolves")
        .paint
        .iter()
        .filter_map(|p| p.text.as_ref().map(|t| t.glyphs.len()))
        .collect()
}

/// A readout prints the units the parameter has, and two decimals when the
/// caller says nothing.
#[test]
fn a_control_prints_the_value_text_it_is_given() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let mut hz = 440.0;
    let bare = runs(slider(&mut ui, "cut", "Cutoff", &mut hz, 20.0..=20_000.0).0);
    // Label, then readout: the default is `{value:.2}`, so "440.00".
    assert_eq!(bare, vec!["Cutoff".len(), "440.00".len()], "{bare:?}");
    let with = runs(
        slider(&mut ui, "cut", "Cutoff", &mut hz, 20.0..=20_000.0)
            .0
            .value_text("440 Hz"),
    );
    assert_eq!(with, vec!["Cutoff".len(), "440 Hz".len()], "{with:?}");
    // A knob has no header, so the text lands under the dial.
    let dial = runs(
        knob(&mut ui, "cut", "Cutoff", &mut hz, 20.0..=20_000.0)
            .0
            .value_text("440 Hz"),
    );
    assert_eq!(dial, vec!["440 Hz".len()], "{dial:?}");
}

/// The whole lane is the slider: a press on the track jumps the value
/// there, and a drag sweeps the range across the lane's own width.
#[test]
fn a_slider_takes_a_track_press_and_drags_across_its_width() {
    let mut ui = Ui::new(Theme::DEFAULT);
    let mut v = 0.0;
    let step = |ui: &mut Ui, v: &mut f64, pointer: PointerInput| {
        let tree = column([slider(ui, "s", "S", v, 0.0..=1.0).0.el()]).width(400.);
        ui.frame(tree, None, pointer, 0.016).unwrap();
    };
    let at = |x: f64, y: f64, down: bool| PointerInput {
        pos: Some(Point::new(x, y)),
        buttons: Buttons::default().set(Button::Primary, down),
        ..PointerInput::default()
    };
    step(&mut ui, &mut v, PointerInput::default());
    let lane = ui.scene().unwrap().surface("s").unwrap().frame;
    let y = lane.y + lane.size.height / 2.0;
    // The thumb sits at the left end; half-way along the track is 0.5.
    let grip = lane.size.height * 0.35 / 0.45;
    let half = lane.x + grip / 2.0 + (lane.size.width - grip) / 2.0;
    step(&mut ui, &mut v, at(half, y, true));
    step(&mut ui, &mut v, at(half, y, true));
    assert!((v - 0.5).abs() < 0.02, "the press jumped the value: {v}");
    // A quarter of the travel further is a quarter of the range further.
    let quarter = (lane.size.width - grip) / 4.0;
    step(&mut ui, &mut v, at(half + quarter, y, true));
    step(&mut ui, &mut v, at(half + quarter, y, false));
    assert!((v - 0.75).abs() < 0.02, "the drag spans the lane: {v}");
}

/// Every widget hands back what happened with its element: a key that edits
/// the field says changed, a key that only moves the caret does not, and an
/// arrow on a focused slider does.
#[test]
fn a_widget_reports_a_change_only_when_its_value_moved() {
    let key = |k: Key| Input {
        keys: vec![KeyPress {
            key: k,
            mods: Mods::default(),
        }],
        ..Input::default()
    };
    let mut ui = Ui::new(Theme::DEFAULT);
    let (mut s, mut v) = (String::from("ab"), 0.5);
    // One tree and one frame; the tree reads the keys of the frame before.
    let mut step = |ui: &mut Ui, focus: &str, input: Input| {
        let (field, typed) = text_input(ui, "f", &mut s);
        let (fader, moved) = slider(ui, "s", "S", &mut v, 0.0..=1.0);
        ui.frame(column([field, fader.el()]), None, input, 0.016)
            .unwrap();
        ui.focus(focus);
        (typed, moved)
    };
    assert_eq!(step(&mut ui, "f", Input::default()), (false, false));
    step(&mut ui, "f", key(Key::Left));
    assert_eq!(step(&mut ui, "f", key(Key::Delete)), (false, false));
    assert_eq!(step(&mut ui, "s", key(Key::Right)), (true, false));
    assert_eq!(step(&mut ui, "s", Input::default()), (false, true));
    assert_eq!(step(&mut ui, "s", Input::default()), (false, false));
    assert_eq!(s, "b");
    assert!((v - 0.51).abs() < 1e-12, "{v}");
}
