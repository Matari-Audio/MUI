//! cargo run -p mui-scene --example shape_layout > shape-layout.svg
//! SVG uses the resolved contours directly; no independently drawn demo geometry.
use mui_scene::prelude::*;
fn cell(id: &str) -> El {
    stack![].flex(1.).fill(Primary).id(id)
}
fn octagon(s: Size) -> Path {
    let (w, h) = (s.width, s.height);
    let d = w.min(h) * 0.25;
    Path::polyline(
        [
            (d, 0.),
            (w - d, 0.),
            (w, d),
            (w, h - d),
            (w - d, h),
            (d, h),
            (0., h - d),
            (0., d),
        ]
        .map(|(x, y)| Point::new(x, y)),
        true,
    )
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1060" height="350" viewBox="0 0 1060 350"><rect width="1060" height="350" fill="#15191d"/>"##
    );
    let examples = [
        (
            "INHERIT + NEST",
            row![
                cell("a"),
                col![cell("b"), cell("c")].flex(1.).inside(8.).id("nested")
            ]
            .inside(8.),
        ),
        (
            "CURVED SPLIT",
            row![cell("a"), cell("b")].inside(8.).bend(0.2),
        ),
        (
            "20 → 1 BORDER + 8 PADDING",
            row![cell("a"), cell("b")]
                .inside(8.)
                .border_ramp(BorderRamp::horizontal((Primary, 20.), (Dim, 1.))),
        ),
    ];
    for (i, (title, root)) in examples.into_iter().enumerate() {
        let scene = resolve_scene(&SceneSpec::new(
            root.outline(octagon).w(280.).h(230.).id("root"),
        ))?;
        println!(
            r##"<g transform="translate({},65)"><text y="-25" font-family="sans-serif" font-size="14" fill="#dbe1e5">{title}</text>"##,
            35 + i * 350
        );
        for s in scene.surfaces() {
            let fill = match s.key.as_ref() {
                "root" => "#4d606a",
                "nested" => "#26353d",
                "a" => "#70b7ae",
                "b" => "#85a7c6",
                _ => "#ad9ac6",
            };
            println!(
                r#"<path d="{}" fill="{fill}" fill-rule="evenodd"/>"#,
                mui_geometry::bez_path(&s.path, 0.05)?.to_svg()
            );
        }
        println!("</g>");
    }
    println!("</svg>");
    Ok(())
}
