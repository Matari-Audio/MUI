use mui_geometry::Error::InvalidOptions;
use mui_scene::prelude::*;
use mui_scene::{Layer, Paint, SceneError};

#[test]
fn named_anchor_keeps_the_outline_and_emits_one_vector_border() {
    for width in [240.0, 640.0] {
        let root = row![
            block(20., 40.),
            block(width, 180.).id("header"),
            block(20., 40.)
        ]
        .gap(0.)
        .union(Role::Surface)
        .radius(12.)
        .id("card");
        let plain = resolve(&SceneSpec::new(root.clone())).unwrap();
        let tree = root.border_ramp(
            BorderRamp::horizontal((Role::Primary, 6.), (Role::Dim, 1.))
                .over("header")
                .transition(0.35, 0.65),
        );
        let ramp = resolve(&SceneSpec::new(tree)).unwrap();
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
        assert!(
            ramp.paint
                .iter()
                .all(|p| !matches!(p.paint, Paint::Image { .. }))
        );
    }
}

#[test]
fn invalid_fields_and_missing_anchors_are_errors() {
    for (ramp, why) in [
        (
            BorderRamp::horizontal((Role::Primary, -1.), (Role::Dim, 1.)),
            "border ramp widths/interval",
        ),
        (
            BorderRamp::horizontal((Role::Primary, 6.), (Role::Dim, 1.)).transition(0.5, 0.5),
            "border ramp widths/interval",
        ),
        (
            BorderRamp::horizontal((Role::Primary, 6.), (Role::Dim, 1.)).over("missing"),
            "border ramp descendant missing",
        ),
    ] {
        assert!(matches!(
            resolve(&SceneSpec::new(block(100., 100.).border_ramp(ramp))),
            Err(SceneError::Geometry(InvalidOptions(m))) if m == why
        ));
    }
}
