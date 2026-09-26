//! Render the actual resolved material paths, with three owner radii.
//! cargo run -p mui-scene --example surfaces > surfaces.svg
use mui_scene::{Layer, prelude::*};
fn card(radius: f64) -> El {
    stack![
        leaf(220., 244.)
            .anchor(Align::Start, Align::Start)
            .offset(8., 8.)
            .inset_surface_of([Id::of("above"), Id::of("main"), Id::of("below")])
            .fill(Field)
            .id("well"),
        leaf(156., 244.)
            .anchor(Align::Start, Align::Start)
            .offset(236., 8.)
            .inset_surface()
            .fill(Field)
            .id("other"),
        leaf(48., 82.)
            .anchor(Align::Start, Align::Start)
            .offset(8., 8.)
            .id("above"),
        leaf(48., 82.)
            .anchor(Align::Start, Align::Start)
            .offset(8., 170.)
            .id("below"),
        leaf(172., 244.)
            .anchor(Align::Start, Align::Start)
            .offset(56., 8.)
            .id("main"),
        leaf(40., 80.)
            .anchor(Align::Start, Align::Start)
            .offset(8., 90.)
            .join_border("body")
            .fill(Primary)
            .focusable()
            .id("title"),
    ]
    .w(400.)
    .h(260.)
    .id("body")
    .fill(Surface)
    .radius((radius, radius * 0.7))
    .surface_layout(8.)
    .border_ramp(BorderRamp::horizontal((Primary, 4.), (Dim, 1.5)).transition(0.35, 0.65))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="300"><defs><linearGradient id="rim"><stop offset="35%" stop-color="#00c9dc"/><stop offset="65%" stop-color="#666"/></linearGradient></defs><rect width="1280" height="300" fill="#111"/>"##
    );
    for (n, radius) in [16., 24., 32.].into_iter().enumerate() {
        let scene = resolve_scene(&SceneSpec::new(card(radius)))?;
        println!(r##"<g transform="translate({},20)">"##, 10 + n * 425);
        for p in &scene.paint {
            let fill = match (p.key.as_ref(), p.layer) {
                ("body", Layer::Fill) => "#242424",
                ("body", Layer::Stroke) => "url(#rim)",
                ("well" | "other", Layer::Fill) => "#111",
                _ => continue,
            };
            println!(r#"<path d="{}" fill="{fill}"/>"#, p.path.to_svg_data()?);
        }
        println!("</g>");
    }
    println!("</svg>");
    Ok(())
}
