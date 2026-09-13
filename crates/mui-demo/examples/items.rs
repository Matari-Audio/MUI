//! Export the unified item example with real resolved outlines and grid layout.
use mui::prelude::*;
fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="860" height="360" viewBox="0 0 860 360"><title>MUI items: row, inferred join and grid controls</title>"#
    );
    let hovering = std::env::args().any(|arg| arg == "--hover");
    for (i, mode) in [Mode::Light, Mode::Dark].into_iter().enumerate() {
        let theme = Theme {
            mode,
            ..Default::default()
        };
        let colors = theme.colors()?;
        let ui = container([
            container([
                item("osc").text("Oscillator").pad(S).on_tap("select-osc"),
                item("filter")
                    .text("Filter")
                    .pad(S)
                    .extend_to("panel")
                    .color(Color::Raised)
                    .on_tap("select-filter"),
                item("fx").text("Effects").pad(S).on_tap("select-fx"),
            ])
            .layout(Row)
            .center()
            .gap(S)
            .pad(M)
            .width(Fill),
            item("panel")
                .layout(Grid(3))
                .pad(L)
                .gap(M)
                .width(Fill)
                .children([
                    item("cutoff")
                        .width(88.)
                        .height(88.)
                        .color(Color::PrimarySoft(0)),
                    item("resonance")
                        .width(88.)
                        .height(88.)
                        .color(Color::PrimarySoft(1)),
                    item("drive")
                        .width(88.)
                        .height(88.)
                        .color(Color::PrimarySoft(2)),
                ]),
        ])
        .layout(Column)
        .width(Fill)
        .height(Hug)
        .gap(M)
        .round(Rounding::separate(M, S))
        .merge(["filter", "panel"])
        .build_with(theme)?
        .available_width(380.);
        // Deliberate schematic text metrics for this SVG example only.
        let resolved = ui.resolve_with(|_, text, _| Ok(Size::new(text.len() as f64 * 8., 20.)))?;
        let f = resolved.layout.frame("filter").unwrap();
        let hovered = if hovering {
            ui.hover_at(&resolved, f.x + 1., f.y + 1.)
        } else {
            None
        };
        let styles = ui.styles(&colors, hovered)?;
        println!(
            r#"<rect x="{}" width="430" height="360" fill="{}"/><g transform="translate({},30)" font-family="sans-serif" font-size="14" fill="{}"><text x="0" y="0">{:?} · {}</text><g transform="translate(0,22)">"#,
            i * 430,
            hex(colors.canvas),
            25 + i * 430,
            hex(colors.text),
            mode,
            if hovering {
                "hover on Filter"
            } else {
                "normal"
            }
        );
        for (surface, _) in ui.outlines(&resolved) {
            if let Some(color) = styles[surface.id.as_str()].fill {
                println!(
                    r#"<path d="{}" fill="{}"/>"#,
                    surface.path.to_svg_data()?,
                    hex(color)
                );
            }
        }
        for (id, f) in resolved.layout.frames() {
            if let Some(text) = ui.info(id).and_then(|info| info.text.as_deref()) {
                println!(
                    r#"<text x="{}" y="{}" text-anchor="middle" fill="{}">{}</text>"#,
                    f.x + f.size.width / 2.,
                    f.y + f.size.height / 2. + 5.,
                    hex(styles[id].text),
                    text
                );
            }
        }
        println!(
            r#"<rect data-hover-target="filter" x="{}" y="{}" width="{}" height="{}" fill="none"/>"#,
            f.x, f.y, f.size.width, f.size.height
        );
        for (id, label, color_index) in [
            ("cutoff", "Cutoff", 0),
            ("resonance", "Resonance", 1),
            ("drive", "Drive", 2),
        ] {
            let f = resolved.layout.frame(id).unwrap();
            println!(
                r#"<text x="{}" y="{}" text-anchor="middle" fill="{}">{}</text>"#,
                f.x + 44.,
                f.y + 49.,
                hex(colors.primary[color_index].on_soft),
                label
            );
        }
        println!(
            r#"<text x="190" y="258" text-anchor="middle" fill="{}">Row: centered content · Grid: 3 equal tracks</text><text x="190" y="282" text-anchor="middle" fill="{}">Filter joins panel through padding and gap</text></g></g>"#,
            hex(colors.muted),
            hex(colors.muted)
        );
    }
    println!("</svg>");
    Ok(())
}
