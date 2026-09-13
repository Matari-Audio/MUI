use mui_core::*;
use mui_layout::{Flow, Size};
#[test]
fn clickable_hover_is_automatic_theme_aware_and_contrast_checked() {
    for mode in [Mode::Light, Mode::Dark] {
        let theme = Theme {
            mode,
            ..Default::default()
        };
        let colors = theme.colors().unwrap();
        let ui = container([
            item("click")
                .width(80.)
                .height(40.)
                .on_tap("click")
                .color(Color::Panel),
            item("transparent")
                .width(80.)
                .height(40.)
                .on_tap("transparent"),
            item("hover-only").width(80.).height(40.).hoverable(),
            item("static").width(80.).height(40.),
        ])
        .build_with(theme)
        .unwrap();
        let scene = ui.resolve().unwrap();
        assert_eq!(ui.hover_at(&scene, 1., 1.), Some("click"));
        assert_eq!(ui.tap_at(&scene, 1., 1.), Some("click"));
        assert_eq!(ui.hover_at(&scene, 161., 1.), Some("hover-only"));
        assert_eq!(ui.tap_at(&scene, 161., 1.), None);
        assert_eq!(ui.hover_at(&scene, 241., 1.), None);
        let normal = ui.styles(&colors, None).unwrap();
        for id in ["click", "transparent", "hover-only"] {
            let hovered = ui.styles(&colors, Some(id)).unwrap();
            let a = normal[id];
            let b = hovered[id];
            assert_ne!(a.background, b.background);
            assert_eq!(
                b.background.luminance() > a.background.luminance(),
                mode == Mode::Dark
            );
            assert!(b.text.contrast(b.background) >= 4.5);
            assert_eq!(hovered["static"], normal["static"]);
        }
        assert_eq!(ui.styles(&colors, Some("static")).unwrap(), normal);
        assert_eq!(ui.styles(&colors, Some("missing")).unwrap(), normal);
        for base in [Rgb::BLACK, Rgb::WHITE, Rgb(250, 40, 60)] {
            assert_ne!(base.hovered(mode), base);
        }
        assert_eq!(
            scene.layout.frame("click").unwrap().size,
            Size::new(80., 40.)
        );
    }
}
#[test]
fn merged_hover_preserves_shared_color_inheritance_and_supports_overrides() {
    let ui = container([
        item("tab")
            .width(80.)
            .height(32.)
            .on_tap("tab")
            .extend_to("panel")
            .color(Color::Raised),
        item("panel")
            .width(200.)
            .height(100.)
            .children([item("label").width(20.).height(20.)]),
    ])
    .layout(Flow::Column)
    .gap(12.)
    .merge(["tab", "panel"])
    .build()
    .unwrap();
    let c = ui.theme().colors().unwrap();
    let scene = ui.resolve().unwrap();
    let normal = ui.styles(&c, None).unwrap();
    let hover = ui.styles(&c, Some("tab")).unwrap();
    assert_ne!(normal["panel"].background, hover["panel"].background);
    assert_eq!(hover["tab"].background, hover["panel"].background);
    assert_eq!(hover["label"].background, hover["panel"].background);
    assert!(hover["label"].text.contrast(hover["label"].background) >= 4.5);
    for (outline, _) in ui.outlines(&scene) {
        assert!(hover.contains_key(outline.id.as_str()));
    }
    let tab = scene.layout.frame("tab").unwrap();
    assert_eq!(ui.hover_at(&scene, tab.x + 1., tab.bottom() + 1.), None);
    let ui = item("override")
        .width(20.)
        .height(20.)
        .on_tap("x")
        .hover_color(Color::Custom(Rgb::WHITE))
        .stroke(Color::Custom(Rgb::WHITE), 1.)
        .build()
        .unwrap();
    let styles = ui.styles(&c, Some("override")).unwrap();
    let s = styles["override"];
    assert_eq!(s.fill, Some(Rgb::WHITE));
    assert!(s.text.contrast(s.background) >= 4.5);
    assert!(s.stroke.unwrap().0.contrast(s.background) >= 3.);
}
