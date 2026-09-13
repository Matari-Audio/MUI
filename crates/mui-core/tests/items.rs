use mui_core::SpacingToken::{L, M, S};
use mui_core::*;
use mui_layout::{Fill, Flow::*, Horizontal::*, Hug, Justify::SpaceEvenly, Track, Vertical::*};
fn box_item(id: &str, w: f64, h: f64) -> Item {
    item(id).width(w).height(h)
}
#[test]
fn tabs_use_one_identity_and_infer_extension_without_changing_tap_targets() {
    for x in [Left, Center, Right] {
        let ui = container([
            container([
                box_item("left", 60., 32.),
                box_item("tab", 80., 32.)
                    .extend_to("panel")
                    .on_tap("select-filter")
                    .color(Color::Raised),
                box_item("right", 60., 32.),
            ])
            .layout(Row)
            .position(x, Middle)
            .width(Fill)
            .pad(M)
            .gap(S),
            box_item("panel", 360., 100.).color(Color::Panel),
        ])
        .layout(Column)
        .gap(L)
        .width(Fill)
        .height(Hug)
        .round(Rounding::separate(12., 8.))
        .merge(["tab", "panel"])
        .build()
        .unwrap()
        .available_width(360.);
        let scene = ui.resolve().unwrap();
        let tab = scene.layout.frame("tab").unwrap();
        let panel = scene.layout.frame("panel").unwrap();
        assert_eq!(scene.surface("tab").unwrap().bounds.unwrap().max.y, panel.y);
        assert_eq!(tab.size, Size::new(80., 32.));
        assert_eq!(
            ui.tap_at(&scene, tab.x + 1., tab.y + 1.),
            Some("select-filter")
        );
        assert_eq!(ui.tap_at(&scene, tab.x + 1., tab.bottom() + 1.), None);
        let outlines = ui.outlines(&scene);
        let merged = outlines
            .iter()
            .find(|(s, _)| s.id.starts_with("@merge:"))
            .unwrap();
        assert_eq!(merged.0.basis.components(), 1);
        for (concave, radius) in [(false, 12.), (true, 8.)] {
            assert!(merged.0.path.commands.iter().any(|c|matches!(c,mui_geometry::PathCommand::ArcTo(a) if (a.sweep<0.)==concave && (a.radius-radius).abs()<1e-6)));
        }
        assert!(!outlines
            .iter()
            .any(|(s, _)| s.id == "tab" || s.id == "panel"));
    }
    for flow in [Row, Column] {
        for reversed in [false, true] {
            let a = box_item("a", 40., 40.).extend_to("b");
            let b = box_item("b", 40., 40.);
            let ui = container(if reversed { [b, a] } else { [a, b] })
                .layout(flow)
                .gap(10.)
                .merge(["a", "b"])
                .build()
                .unwrap();
            assert_eq!(
                ui.outlines(&ui.resolve().unwrap())
                    .last()
                    .unwrap()
                    .0
                    .basis
                    .components(),
                1
            );
        }
    }
    let overlapping = container([
        box_item("a", 40., 40.).extend_to("b"),
        box_item("b", 20., 20.),
    ])
    .layout(Overlay)
    .build()
    .unwrap();
    let resolved = overlapping.resolve().unwrap();
    assert_eq!(resolved.surface("a").unwrap().bounds.unwrap().width(), 40.);
}
#[test]
fn grid_auto_places_equal_tracks_and_respects_explicit_cells_spans_and_alignment() {
    let ui = container((0..4).map(|i| box_item(&format!("i{i}"), 20., 20.)))
        .layout(Grid(3))
        .gap(10.)
        .width(Fill)
        .height(Hug)
        .center()
        .build()
        .unwrap()
        .available_width(320.);
    let l = ui.resolve().unwrap().layout;
    for (id, x, y) in [
        ("i0", 40., 0.),
        ("i1", 150., 0.),
        ("i2", 260., 0.),
        ("i3", 40., 30.),
    ] {
        let f = l.frame(id).unwrap();
        assert_eq!((f.x, f.y), (x, y));
    }
    let ui = container([
        box_item("wide", 20., 20.)
            .cell(1, 1)
            .span(2, 1)
            .place(Right, Bottom),
        box_item("other", 20., 20.).cell(2, 2).place(Left, Top),
    ])
    .layout(Grid(2))
    .columns([Track::Fixed(80.), Track::Fraction(1.)])
    .rows([Track::Fixed(60.), Track::Fixed(40.)])
    .gap(10.)
    .width(200.)
    .height(110.)
    .build()
    .unwrap();
    let l = ui.resolve().unwrap().layout;
    assert_eq!(
        (l.frame("wide").unwrap().x, l.frame("wide").unwrap().y),
        (180., 40.)
    );
    assert_eq!(
        (l.frame("other").unwrap().x, l.frame("other").unwrap().y),
        (90., 70.)
    );
}
#[test]
fn physical_alignment_and_distribution_follow_resolved_flow() {
    for flow in [Row, Column] {
        let ui = container([box_item("a", 20., 20.), box_item("b", 20., 20.)])
            .layout(flow)
            .width(100.)
            .height(100.)
            .pack(SpaceEvenly)
            .build()
            .unwrap();
        let l = ui.resolve().unwrap().layout;
        let a = l.frame("a").unwrap();
        let b = l.frame("b").unwrap();
        assert_eq!(
            if flow == Row { (a.x, b.x) } else { (a.y, b.y) },
            (20., 60.)
        );
    }
    let overlay = container([box_item("child", 20., 20.)])
        .layout(Overlay)
        .center()
        .width(100.)
        .height(80.)
        .build()
        .unwrap();
    let f = overlay.resolve().unwrap().layout.frame("child").unwrap();
    assert_eq!((f.x, f.y), (40., 30.));
    for (width, ax, ay) in [(160., 70., 80.), (70., 30., 50.)] {
        let ui = container([box_item("a", 40., 20.), box_item("b", 40., 20.)])
            .layout(Auto)
            .position(Right, Bottom)
            .gap(10.)
            .width(Fill)
            .height(100.)
            .build()
            .unwrap()
            .available_width(width);
        let l = ui.resolve().unwrap().layout;
        let a = l.frame("a").unwrap();
        assert_eq!((a.x, a.y), (ax, ay));
    }
}
#[test]
fn scoped_text_measurement_and_invalid_item_policies_are_explicit() {
    let component = || container([item("label").text("hello").pad(S).on_tap("hello")]);
    let ui = container([component().scope("a"), component().scope("b")])
        .gap(M)
        .build()
        .unwrap();
    let scene = ui
        .resolve_with(|_, text, _| Ok(Size::new(text.len() as f64 * 8., 20.)))
        .unwrap();
    assert_eq!(
        scene.layout.frame("a/label").unwrap().size,
        Size::new(56., 36.)
    );
    assert_eq!(ui.info("b/label").unwrap().text.as_deref(), Some("hello"));
    for bad in [
        container([]).layout(Grid(0)),
        container([]).layout(Grid(2)).wrap(),
        container([box_item("a", 20., 20.).cell(1, 1)]).layout(Row),
        container([])
            .layout(Grid(2))
            .columns([Track::Fraction(-1.)]),
    ] {
        assert!(bad.build().unwrap().resolve().is_err());
    }
    assert!(item("x")
        .text("bad")
        .children([item("child")])
        .build()
        .is_err());
    assert!(container([item("x"), item("x")]).build().is_err());
    assert!(container([item("x")]).merge(["missing"]).build().is_err());
    let diagonal = container([
        box_item("a", 20., 20.).cell(1, 1).extend_to("b"),
        box_item("b", 20., 20.).cell(2, 2),
    ])
    .layout(Grid(2))
    .gap(10.)
    .build()
    .unwrap();
    assert!(diagonal.resolve().is_err());
}
