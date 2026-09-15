use mui_core::{container, item};
use mui_layout::{Flow, Hug};
use mui_core::{TextStyle, TextSystem};

#[test]
fn real_font_wrapping_padding_fixed_leaves_and_invalid_styles() {
    let mut text = TextSystem::default();
    assert!(text.register_font(vec![0, 1, 2]).is_err());
    text.register_font(include_bytes!("../../mui-text/tests/fonts/DejaVuSans.ttf").to_vec())
        .unwrap();
    let style = TextStyle {
        family: "DejaVu Sans".into(),
        ..Default::default()
    };
    let build = |width| {
        container([
            item("paragraph")
                .text("Oscillator controls with several words that wrap.")
                .width(width)
                .height(Hug)
                .pad(10.),
            item("fixed")
                .text("שלום hello")
                .width(180.)
                .height(60.)
                .pad(8.),
        ])
        .layout(Flow::Column)
        .gap(4.)
        .build()
        .unwrap()
    };
    let wide = text.resolve(&build(400.), &style).unwrap();
    let narrow = text.resolve(&build(150.), &style).unwrap();
    let p = narrow.paragraph("paragraph").unwrap();
    assert!(p.layout().height() > wide.paragraph("paragraph").unwrap().layout().height());
    assert_eq!(p.frame().size.width, 130.);
    assert_eq!(
        p.frame().x,
        narrow.scene.layout.frame("paragraph").unwrap().x + 10.
    );
    assert!((p.frame().size.height - f64::from(p.layout().height())).abs() < 0.001);
    let fixed = narrow.paragraph("fixed").unwrap();
    assert_eq!(fixed.frame().size.width, 164.);
    assert!(fixed.layout().lines().count() > 0);
    assert!(fixed.layout().width() > 0.);
    for size in [f32::NAN, f32::INFINITY, -1., 0.] {
        assert!(text
            .resolve(
                &build(150.),
                &TextStyle {
                    size,
                    ..style.clone()
                }
            )
            .is_err());
    }
    // A failed resolution cannot mutate the previously returned drawable paragraph.
    assert_eq!(p.frame().size.width, 130.);
}

#[test]
fn mixed_font_sizes_align_actual_first_baselines() {
    let mut text = TextSystem::default();
    text.register_font(include_bytes!("../../mui-text/tests/fonts/DejaVuSans.ttf").to_vec())
        .unwrap();
    let ui = container([
        item("small").text("Gain").pad(7.),
        item("large").text("80%").pad(2.),
    ])
    .align(mui_layout::Align::Baseline)
    .build()
    .unwrap();
    let resolved = text
        .resolve_with(&ui, |id, _| TextStyle {
            family: "DejaVu Sans".into(),
            size: if id == "small" { 14. } else { 32. },
            line_height: if id == "small" { 20. } else { 42. },
            ..Default::default()
        })
        .unwrap();
    let baseline = |id| {
        let p = resolved.paragraph(id).unwrap();
        p.frame().y + f64::from(p.layout().lines().next().unwrap().metrics().baseline)
    };
    assert!((baseline("small") - baseline("large")).abs() < 0.001);
}
