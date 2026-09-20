use mui_scene::prelude::*;
use mui_scene::{Layer, Paint};

#[test]
fn named_anchor_keeps_the_outline_and_emits_one_vector_border() {
    for width in [240.0, 640.0] {
        let root = row![
            leaf(20., 40.),
            leaf(width, 180.).id("header"),
            leaf(20., 40.)
        ]
        .gap(0.)
        .weld(Surface)
        .radius(12.)
        .id("card");
        let plain = resolve_scene(&SceneSpec::new(root.clone())).unwrap();
        let tree = root.border_ramp(
            BorderRamp::horizontal((Primary, 6.), (Dim, 1.))
                .over("header")
                .transition(0.35, 0.65),
        );
        let ramp = resolve_scene(&SceneSpec::new(tree)).unwrap();
        assert_eq!(
            plain.surface("card").unwrap().path,
            ramp.surface("card").unwrap().path
        );
        assert_eq!(plain.layout, ramp.layout);
        let strokes: Vec<_> = ramp
            .paint
            .iter()
            .filter(|p| p.key.as_ref() == "card" && p.layer == Layer::Stroke)
            .collect();
        assert_eq!(strokes.len(), 1);
        assert_eq!(strokes[0].width, 0.0);
        let Paint::Gradient { stops, .. } = &strokes[0].paint else {
            panic!("vector material");
        };
        let bounds = mui_geometry::Bounds::from_points(
            strokes[0].path.flatten(0.1, 250_000).unwrap().concat(),
        )
        .unwrap();
        let header = ramp.surface("header").unwrap().frame;
        for (stop, fraction) in stops.iter().zip([0.35, 0.65]) {
            let x = bounds.min.x + f64::from(stop.0) * bounds.width();
            assert!((x - header.x - header.size.width * fraction).abs() < 0.001);
        }
        assert!(ramp
            .paint
            .iter()
            .all(|p| !matches!(p.paint, Paint::Image { .. })));
    }
}

#[test]
fn invalid_fields_and_missing_anchors_are_errors() {
    for ramp in [
        BorderRamp::horizontal((Primary, -1.), (Dim, 1.)),
        BorderRamp::horizontal((Primary, 6.), (Dim, 1.)).transition(0.5, 0.5),
        BorderRamp::horizontal((Primary, 6.), (Dim, 1.)).over("missing"),
    ] {
        assert!(resolve_scene(&SceneSpec::new(leaf(100., 100.).border_ramp(ramp))).is_err());
    }
}
