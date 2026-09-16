//! The two claims that hold every control together: a variant is arithmetic on
//! one role, and one theme unit steps every size. They live here rather than
//! beside the code because they need a [`Host`], and the only one is the
//! runtime in `mui` -- a dev-dependency, so it cannot be reached from a
//! `#[cfg(test)]` module inside the library itself.
use mui::prelude::*;

fn face(v: Variant) -> Style {
    let ui = Ui::new(Theme::DEFAULT);
    button(&ui, "b", "Save")
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
        let ui = Ui::new(Theme::DEFAULT);
        let el = button(&ui, "b", "Save").0.variant(v).el();
        el.children()[0].payload().style.fill.clone()
    };
    assert_eq!(ink(Variant::Solid), Fill::Role(Role::Ink));
    assert_eq!(ink(Variant::Ghost), Fill::Role(Role::Primary));
}

/// One `Theme.control` unit, five steps, and pixels as the escape hatch.
#[test]
fn the_size_scale_steps_every_control_off_the_theme_unit() {
    let width = |c: Control| {
        let ui = Ui::new(Theme::DEFAULT);
        let _ = &ui;
        let scene = resolve_scene(&SceneSpec::new(c.el())).expect("resolves");
        scene.surface("sw").expect("the toggle").frame.size.width
    };
    let ui = Ui::new(Theme::DEFAULT);
    let mut on = false;
    let sw = |size: SpacingToken| {
        let mut on = on;
        toggle(&ui, "sw", &mut on).size(size)
    };
    let (xs, m, xl) = (width(sw(Xs)), width(sw(M)), width(sw(Xl)));
    assert!(xs < m && m < xl, "{xs} {m} {xl}");
    // M is ten units of the default 4 px control step.
    assert!((m - 40.).abs() < 0.01, "{m}");
    // Steps are even: Xs and Xl sit two units either side of M.
    assert!(((xl - m) - (m - xs)).abs() < 0.01);
    assert!((width(toggle(&ui, "sw", &mut on).px(64.)) - 64.).abs() < 0.01);
}
