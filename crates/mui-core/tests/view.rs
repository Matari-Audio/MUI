use mui_core::{container, item, Color, Overflow, ViewState};
use mui_geometry::{Affine, Point};
use mui_layout::{Align, Flow};

#[test]
fn nested_scroll_clips_transforms_and_wheel_chaining_share_coordinates() {
    let ui = container([
        container([
            item("spacer").width(100.).height(60.),
            item("target")
                .width(100.)
                .height(40.)
                .round(10.)
                .on_tap("hit"),
            item("bottom").width(100.).height(100.),
        ])
        .id("inner")
        .layout(Flow::Column)
        .align(Align::Start)
        .width(100.)
        .height(80.)
        .overflow(Overflow::Scroll),
        item("tail").width(100.).height(120.),
    ])
    .id("outer")
    .layout(Flow::Column)
    .align(Align::Start)
    .width(100.)
    .height(100.)
    .overflow(Overflow::Scroll)
    .build()
    .unwrap();
    let scene = ui.resolve().unwrap();
    let mut state = ViewState::default();
    let transform = Affine::scale(1.5, 2.)
        .then(Affine::rotation(0.3))
        .then(Affine::translation(150., 100.));
    state.set_transform("outer", transform).unwrap();
    state.set_scroll("inner", Point::new(0., 60.)).unwrap();
    let view = state.resolve(&ui, &scene).unwrap();
    let target = view.item("target").unwrap();
    let f = scene.layout.frame("target").unwrap();
    let center = target.transform.apply(Point::new(f.x + 50., f.y + 20.));
    assert_eq!(view.tap_at(center).unwrap(), Some("hit"));
    let cutout = target.transform.apply(Point::new(f.x + 0.1, f.y + 0.1));
    assert_eq!(view.tap_at(cutout).unwrap(), None);
    assert_eq!(view.tap_at(Point::new(f.x + 50., f.y + 20.)).unwrap(), None);
    assert_eq!(target.clips.len(), 2);
    // Inner consumes its last 60 logical pixels, then outer consumes the remaining 20.
    let delta = transform.apply(Point::new(0., 80.)) - transform.apply(Point::ZERO);
    let left = state.scroll_at(&view, center, delta).unwrap();
    assert!(left.length() < 1e-8);
    let view = state.resolve(&ui, &scene).unwrap();
    assert!((view.item("inner").unwrap().scroll.y - 120.).abs() < 1e-8);
    assert!((view.item("outer").unwrap().scroll.y - 20.).abs() < 1e-8);
    let target = view.item("target").unwrap();
    let offscreen = target.transform.apply(Point::new(f.x + 50., f.y + 20.));
    assert!(!target.visible_at(offscreen));
    assert_eq!(view.tap_at(offscreen).unwrap(), None);
    assert!(state.set_transform("outer", Affine::scale(0., 1.)).is_err());
    assert!(state.set_scroll("inner", Point::new(f64::NAN, 0.)).is_err());
    assert!(state
        .scroll_at(&view, center, Point::new(0., f64::INFINITY))
        .is_err());
    state.set_scroll("outer", Point::new(1e9, 1e9)).unwrap();
    let view = state.resolve(&ui, &scene).unwrap();
    assert_eq!(view.item("outer").unwrap().scroll, Point::new(0., 100.));
    // Scrolling never changes the original geometry/layout coordinates.
    assert_eq!(scene.layout.frame("target").unwrap(), f);
}

#[test]
fn painted_siblings_occlude_and_decorative_children_bubble_to_actions() {
    let make = |disabled, decorative| {
        container([
            item("under").width(100.).height(100.).on_tap("under"),
            item("over")
                .width(100.)
                .height(100.)
                .round(20.)
                .color(Color::Panel)
                .disabled(disabled)
                .children([if decorative {
                    item("label").width(40.).height(20.).color(Color::Text)
                } else {
                    item("label").width(40.).height(20.).on_tap("label")
                }]),
        ])
        .layout(Flow::Overlay)
        .build()
        .unwrap()
    };
    let state = ViewState::default();
    for (disabled, decorative, expected) in [
        (false, false, Some("label")),
        (true, false, None),
        (false, true, None),
    ] {
        let ui = make(disabled, decorative);
        let scene = ui.resolve().unwrap();
        let view = state.resolve(&ui, &scene).unwrap();
        let label = scene.layout.frame("label").unwrap();
        assert_eq!(
            view.tap_at(Point::new(label.x + 20., label.y + 10.))
                .unwrap(),
            expected
        );
        // Painted but noninteractive part of the overlay also blocks the older action.
        assert_eq!(view.tap_at(Point::new(50., 70.)).unwrap(), None);
    }
    let ui = item("button")
        .width(100.)
        .height(100.)
        .on_tap("button")
        .children([item("icon").width(20.).height(20.).color(Color::Text)])
        .build()
        .unwrap();
    let scene = ui.resolve().unwrap();
    let icon = scene.layout.frame("icon").unwrap();
    assert_eq!(
        state
            .resolve(&ui, &scene)
            .unwrap()
            .tap_at(Point::new(icon.x + 10., icon.y + 10.))
            .unwrap(),
        Some("button")
    );
}

#[test]
fn text_conversion_preserves_explicit_shrink_in_scroll_containers() {
    for (shrink, expected) in [(0., 100.), (1., 0.)] {
        let ui = container([item("text")
            .shrink(shrink)
            .text("content")
            .width(100.)
            .height(200.)])
        .id("viewport")
        .layout(Flow::Column)
        .width(100.)
        .height(100.)
        .overflow(Overflow::Scroll)
        .build()
        .unwrap();
        let scene = ui
            .resolve_with(|_, _, input| {
                Ok(mui_layout::Size::new(
                    input.known.width.unwrap_or(100.),
                    input.known.height.unwrap_or(200.),
                ))
            })
            .unwrap();
        assert_eq!(
            scene.layout.scroll_limit("viewport").unwrap().height,
            expected
        );
    }
}

#[test]
fn merged_surfaces_require_shared_transforms_and_clips() {
    let ui = container([
        item("a").width(40.).height(40.),
        item("b").width(40.).height(40.),
    ])
    .id("root")
    .merge(["a", "b"])
    .build()
    .unwrap();
    let scene = ui.resolve().unwrap();
    let mut state = ViewState::default();
    state
        .set_transform("root", Affine::translation(10., 20.))
        .unwrap();
    let view = state.resolve(&ui, &scene).unwrap();
    for (surface, _) in ui.outlines(&scene) {
        assert!(view.item(&surface.id).is_some());
    }
    state
        .set_transform("a", Affine::translation(5., 0.))
        .unwrap();
    assert!(state.resolve(&ui, &scene).is_err());
}
