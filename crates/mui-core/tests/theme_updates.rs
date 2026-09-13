use mui_core::*;
use mui_layout::{Flow, SpacingToken::M};
#[test]
fn live_theme_updates_recompute_tokens_and_preserve_ids_actions_and_overrides() {
    let mut ui = container([
        item("token")
            .width(80.)
            .height(40.)
            .pad(M)
            .round(M)
            .on_tap("token")
            .color(Color::Panel),
        item("global").width(80.).height(40.).color(Color::Raised),
        item("fixed").width(80.).height(40.).round(5.),
    ])
    .layout(Flow::Row)
    .gap(M)
    .build()
    .unwrap()
    .offered(320., 60.);
    let original = ui.resolve().unwrap();
    let before = ui.resolved_styles(Some("token")).unwrap()["token"];
    let mut theme = *ui.theme();
    theme.mode = Mode::Light;
    theme.spacing.m = 18.;
    theme.corners = CornerProfile::new(9., 7.);
    theme.hover_shift = 0.12;
    let next = ui.set_theme(theme).unwrap();
    assert_eq!(ui.theme(), &theme);
    assert_eq!(
        next.surface("token")
            .unwrap()
            .analytic_rect
            .unwrap()
            .radius(),
        18.
    );
    assert_eq!(
        next.surface("global")
            .unwrap()
            .analytic_rect
            .unwrap()
            .radius(),
        9.
    );
    assert_eq!(
        next.surface("fixed")
            .unwrap()
            .analytic_rect
            .unwrap()
            .radius(),
        5.
    );
    assert_ne!(
        original.layout.frame("global").unwrap().x,
        next.layout.frame("global").unwrap().x
    );
    assert_eq!(ui.info("token").unwrap().tap.as_deref(), Some("token"));
    assert_ne!(
        before.background,
        ui.resolved_styles(Some("token")).unwrap()["token"].background
    );
    let snapshot = ui.resolve().unwrap();
    let colors = *ui.colors();
    for mut bad in [theme; 2].into_iter().enumerate() {
        if bad.0 == 0 {
            bad.1.spacing.m = 100.;
        } else {
            bad.1.contrast.text = 2.;
        }
        assert!(ui.set_theme(bad.1).is_err());
        assert_eq!(ui.theme(), &theme);
        assert_eq!(ui.resolve().unwrap(), snapshot);
        assert_eq!(*ui.colors(), colors);
    }
}
#[test]
fn measured_theme_changes_and_stronger_contrast_are_checked_before_publication() {
    let mut ui = item("text")
        .text("hello")
        .pad(M)
        .on_tap("hello")
        .build()
        .unwrap();
    let mut theme = *ui.theme();
    theme.mode = Mode::Light;
    theme.spacing.m = 20.;
    theme.contrast = Contrast::AAA;
    assert!(ui.set_theme(theme).is_err()); // The host must supply text measurement.
    let scene = ui
        .set_theme_with(theme, |_, text, _| {
            Ok(Size::new(text.len() as f64 * 8., 20.))
        })
        .unwrap();
    assert_eq!(scene.layout.size, Size::new(80., 60.));
    for hover in [None, Some("text")] {
        let s = ui.resolved_styles(hover).unwrap()["text"];
        assert!(s.text.contrast(s.background) >= 7.);
    }
    let mut impossible = item("gray")
        .width(40.)
        .height(40.)
        .color(Color::Custom(Rgb(118, 118, 118)))
        .build()
        .unwrap();
    let old = *impossible.theme();
    assert!(impossible.set_theme(theme).is_err());
    assert_eq!(*impossible.theme(), old);
}
