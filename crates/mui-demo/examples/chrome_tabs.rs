//! Export real resolved geometry: cargo run -p mui-demo --example chrome_tabs > docs/chrome-tabs.svg
use mui_core::{resolve_scene, Edge, Mode, Rgb, SceneSpec, SurfaceSpec, Theme};
use mui_layout::{Fill, Hug, Justify, Node, Size};
fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="1080" height="550" viewBox="0 0 1080 550"><title>Cross-column tab attachment, light and dark</title>"#
    );
    for (row, mode) in [Mode::Light, Mode::Dark].into_iter().enumerate() {
        let theme = Theme {
            mode,
            ..Default::default()
        };
        let colors = theme.colors()?;
        println!(
            r#"<rect y="{}" width="1080" height="275" fill="{}"/>"#,
            row * 275,
            hex(colors.canvas)
        );
        for (col, (name, align)) in [
            ("Left", Justify::Start),
            ("Center", Justify::Center),
            ("Right", Justify::End),
        ]
        .into_iter()
        .enumerate()
        {
            let root = Node::column(
                "root",
                [
                    Node::row("header", [Node::leaf("tab-frame", Size::new(80., 32.))])
                        .padding_xy(0., 12.)
                        .width(Fill)
                        .justify(align),
                    Node::column("lower", [Node::leaf("panel-frame", Size::new(320., 120.))]),
                ],
            )
            .gap(20.)
            .width(Fill)
            .height(Hug);
            let scene = resolve_scene(
                &SceneSpec::new(root)
                    .available_width(320.)
                    .theme(theme)
                    .surface(
                        SurfaceSpec::frame("tab", "tab-frame")
                            .extend_to(Edge::Bottom, "panel-frame"),
                    )
                    .surface(SurfaceSpec::frame("panel", "panel-frame"))
                    .surface(SurfaceSpec::merge("shell", ["tab", "panel"])),
            )?;
            let tab = scene.layout.frame("tab-frame").unwrap();
            println!(
                r#"<g transform="translate({},{})" font-family="sans-serif" fill="{}"><text y="0" font-size="16">{} · {:?}</text><g transform="translate(0,14)"><path d="{}" fill="{}" stroke="{}"/><rect x="{}" y="{}" width="80" height="32" fill="none" stroke="{}" stroke-dasharray="4 3"/><text x="160" y="125" text-anchor="middle" font-size="13">20px gap + 12px parent padding</text><text x="160" y="147" text-anchor="middle" font-size="13">Dashed box = unchanged content frame</text></g></g>"#,
                20 + col * 360,
                28 + row * 275,
                hex(colors.text),
                name,
                mode,
                scene.surface("shell").unwrap().path.to_svg_data()?,
                hex(colors.raised),
                hex(colors.outline),
                tab.x,
                tab.y,
                hex(colors.primary[0].fill)
            );
        }
    }
    println!("</svg>");
    Ok(())
}
