//! Render the actual resolved material paths, with three owner radii.
//! cargo run -p mui-scene --example surfaces > surfaces.svg
use mui_scene::{Layer, prelude::*};
fn card(radius: f64) -> El {
    stack![
        block(220., 244.)
            .at(8., 8.)
            .inset_surface_of([Id::of("above"), Id::of("main"), Id::of("below")])
            .fill(Role::Field)
            .id("well"),
        block(156., 244.)
            .at(236., 8.)
            .inset_surface()
            .fill(Role::Field)
            .id("other"),
        block(48., 82.).at(8., 8.).id("above"),
        block(48., 82.).at(8., 170.).id("below"),
        block(172., 244.).at(56., 8.).id("main"),
        block(40., 80.)
            .at(8., 90.)
            .join_border("body")
            .fill(Role::Primary)
            .focusable()
            .id("title"),
    ]
    .w(400.)
    .h(260.)
    .id("body")
    .fill(Role::Surface)
    .radius((radius, radius * 0.7))
    .surface_layout(8.)
    .border_ramp(
        BorderRamp::horizontal((Role::Primary, 4.), (Role::Dim, 1.5)).transition(0.35, 0.65),
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="300"><defs><linearGradient id="rim"><stop offset="35%" stop-color="#00c9dc"/><stop offset="65%" stop-color="#666"/></linearGradient></defs><rect width="1280" height="300" fill="#111"/>"##
    );
    for (n, radius) in [16., 24., 32.].into_iter().enumerate() {
        let scene = resolve(&SceneSpec::new(card(radius)))?;
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
