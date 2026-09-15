//! Render one View snapshot: SVG clip groups apply to paths and text together.
use mui_core::{container, item, Color, Overflow, ViewState};
use mui_core::{TextStyle, TextSystem};
use mui_geometry::{Affine, Point};
use mui_layout::{Align, Flow};
fn matrix(t: Affine) -> String {
    format!(
        "matrix({} {} {} {} {} {})",
        t.xx, t.yx, t.xy, t.yy, t.tx, t.ty
    )
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut fonts = TextSystem::default();
    fonts.register_font(include_bytes!("../../mui-text/tests/fonts/DejaVuSans.ttf").to_vec())?;
    let ui = container([
        container([
            item("small").text("Gain").pad(5.),
            item("large").text("80%").pad(2.),
        ])
        .id("baseline")
        .align(Align::Baseline)
        .gap(10.),
        container([
            item("above")
                .text("Scroll past this")
                .shrink(0.)
                .width(240.)
                .height(55.)
                .pad(8.),
            item("target")
                .text("Transformed target")
                .shrink(0.)
                .width(240.)
                .height(50.)
                .pad(8.)
                .round(14.)
                .color(Color::PrimarySoft(0))
                .on_tap("target"),
            item("below")
                .text("Nested content")
                .shrink(0.)
                .width(240.)
                .height(120.)
                .pad(8.)
                .color(Color::Panel),
        ])
        .id("inner")
        .layout(Flow::Column)
        .align(Align::Start)
        .width(240.)
        .height(110.)
        .overflow(Overflow::Scroll),
        item("tail")
            .text("Outer content")
            .shrink(0.)
            .width(240.)
            .height(140.)
            .pad(8.)
            .color(Color::Raised),
    ])
    .id("outer")
    .layout(Flow::Column)
    .align(Align::Start)
    .gap(12.)
    .pad(12.)
    .width(264.)
    .height(230.)
    .overflow(Overflow::Scroll)
    .build()?;
    let text = fonts.resolve_with(&ui, |id, _| TextStyle {
        family: "DejaVu Sans".into(),
        size: if id == "large" { 32. } else { 16. },
        line_height: if id == "large" { 42. } else { 24. },
        ..Default::default()
    })?;
    let scene = &text.scene;
    println!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="780" height="370" viewBox="0 0 780 370"><title>MUI baselines and transformed nested viewports</title><rect width="780" height="370" fill="#181c22"/>"##
    );
    for (panel, offset) in [0., 55.].into_iter().enumerate() {
        let mut state = ViewState::default();
        state.set_scroll("inner", Point::new(0., offset))?;
        state.set_transform(
            "outer",
            Affine::rotation(-0.06).then(Affine::translation(40. + panel as f64 * 390., 75.)),
        )?;
        state.set_transform(
            "target",
            Affine::scale(0.94, 1.).then(Affine::rotation(0.03)),
        )?;
        let view = state.resolve(&ui, scene)?;
        assert_eq!(view.item("inner").unwrap().scroll.y, offset);
        let target = view.item("target").unwrap();
        let frame = scene.layout.frame("target").unwrap();
        let point = target
            .transform
            .apply(Point::new(frame.x + 100., frame.y + 25.));
        assert_eq!(view.tap_at(point)?, Some("target"));
        let styles = ui.resolved_styles(None)?;
        println!(
            r##"<text x="{}" y="35" fill="#f0f2f5" font-family="DejaVu Sans" font-size="18">Inner scroll: {} px</text>"##,
            40 + panel * 390,
            offset
        );
        for (index, (id, entry)) in view.items().enumerate() {
            for (c, clip) in entry.clips.iter().enumerate() {
                let f = clip.frame;
                println!(
                    r#"<defs><clipPath id="c{panel}-{index}-{c}" clipPathUnits="userSpaceOnUse"><rect x="{}" y="{}" width="{}" height="{}" transform="{}"/></clipPath></defs><g clip-path="url(#c{panel}-{index}-{c})">"#,
                    f.x,
                    f.y,
                    f.size.width,
                    f.size.height,
                    matrix(clip.transform)
                );
            }
            println!(r#"<g transform="{}">"#, matrix(entry.transform));
            if let Some(fill) = styles[id].fill {
                println!(
                    r#"<path d="{}" fill="rgb({},{},{})"/>"#,
                    scene.surface(id).unwrap().path.to_svg_data()?,
                    fill.0,
                    fill.1,
                    fill.2
                );
            }
            if let Some(p) = text.paragraph(id) {
                let baseline =
                    p.frame().y + f64::from(p.layout().lines().next().unwrap().metrics().baseline);
                // These fixture labels are single-line. Browser text uses the same named font;
                // renderer-independent tests above use Parley's actual glyph metrics.
                println!(
                    r##"<text x="{}" y="{}" font-size="{}" font-family="DejaVu Sans" fill="#f0f2f5">{}</text>"##,
                    p.frame().x,
                    baseline,
                    if id == "large" { 32 } else { 16 },
                    ui.info(id).unwrap().text.as_ref().unwrap()
                );
                if id == "small" || id == "large" {
                    println!(
                        r##"<path d="M{} {} h{}" stroke="#56d9b1" stroke-dasharray="3 3"/>"##,
                        p.frame().x,
                        baseline,
                        p.frame().size.width
                    );
                }
            }
            println!("</g>{}", "</g>".repeat(entry.clips.len()));
        }
        println!(
            r##"<circle cx="{}" cy="{}" r="4" fill="#ffda66"/><text x="{}" y="345" fill="#b8c0cc" font-family="DejaVu Sans" font-size="14">Yellow point: verified target hit</text>"##,
            point.x,
            point.y,
            40 + panel * 390
        );
    }
    println!("</svg>");
    Ok(())
}
