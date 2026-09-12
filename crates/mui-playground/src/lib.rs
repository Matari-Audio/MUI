//! Browser adapter and constrained item DSL. No arbitrary code execution.
#![forbid(unsafe_code)]
mod parser;
use mui::core::ResolvedScene;
use mui::prelude::*;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(
    inline_js = "let context; export function text_width(text) { context ??= document.createElement('canvas').getContext('2d'); context.font = '14px sans-serif'; return context.measureText(text).width; }"
)]
extern "C" {
    fn text_width(text: &str) -> f64;
}
#[cfg(not(target_arch = "wasm32"))]
fn text_width(text: &str) -> f64 {
    text.chars().count() as f64 * 8.
}
struct Preview {
    ui: Ui,
    scene: ResolvedScene,
}
thread_local! {static PREVIEW:RefCell<Option<Preview>>=const {RefCell::new(None)};}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}
fn svg(p: &Preview, hovered: Option<&str>, bounds: bool) -> Result<String, String> {
    let styles = p.ui.resolved_styles(hovered).map_err(|e| e.to_string())?;
    let w = p.scene.layout.size.width.max(1.);
    let h = p.scene.layout.size.height.max(1.);
    let mut out = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" data-hover="{}" aria-label="MUI live preview"><rect width="100%" height="100%" fill="{}"/>"#,
        escape(hovered.unwrap_or("")),
        hex(p.ui.colors().canvas)
    );
    for (surface, _) in p.ui.outlines(&p.scene) {
        let s = styles[surface.id.as_str()];
        if s.fill.is_some() || s.stroke.is_some() {
            out += &format!(
                r#"<path d="{}" fill="{}" stroke="{}" stroke-width="{}"/>"#,
                surface.path.to_svg_data().map_err(|e| e.to_string())?,
                s.fill.map_or("none".into(), hex),
                s.stroke.map_or("none".into(), |s| hex(s.0)),
                s.stroke.map_or(0., |s| s.1)
            );
        }
    }
    for (id, f) in p.scene.layout.frames() {
        if let Some(info) = p.ui.info(id) {
            if let Some(text) = &info.text {
                out += &format!(
                    r#"<text x="{}" y="{}" text-anchor="middle" dominant-baseline="central" font-family="sans-serif" font-size="14" fill="{}">{}</text>"#,
                    f.x + f.size.width / 2.,
                    f.y + f.size.height / 2.,
                    hex(styles[id].text),
                    escape(text)
                );
            }
            if bounds {
                out += &format!(
                    r#"<rect x="{}" y="{}" width="{}" height="{}" fill="none" stroke="{}" stroke-width="0.7" stroke-dasharray="3 3"><title>{}</title></rect>"#,
                    f.x,
                    f.y,
                    f.size.width,
                    f.size.height,
                    hex(p.ui.colors().outline),
                    escape(id)
                );
            }
        }
    }
    out += "</svg>";
    Ok(out)
}
#[wasm_bindgen]
pub fn parameters(source: &str) -> Result<String, JsValue> {
    let (_, params) = parser::parse(source).map_err(|s| JsValue::from_str(&s))?;
    Ok(params
        .iter()
        .map(|p| {
            format!(
                "{}\t{}\t{}\t{}\t{}\n",
                p.start, p.end, p.label, p.value, p.pixels
            )
        })
        .collect())
}
#[wasm_bindgen]
pub fn render(
    source: &str,
    width: f64,
    dark: bool,
    roundness: f64,
    primary: u32,
    bounds: bool,
) -> Result<String, JsValue> {
    let build = || -> Result<Preview, String> {
        let (root, _) = parser::parse(source)?;
        let mut theme = Theme {
            mode: if dark { Mode::Dark } else { Mode::Light },
            corners: mui::core::CornerProfile::new(roundness, roundness),
            ..Default::default()
        };
        theme.palette.light.primary[0] =
            Rgb((primary >> 16) as u8, (primary >> 8) as u8, primary as u8);
        let ui = root
            .build_with(theme)
            .map_err(|e| e.to_string())?
            .available_width(width);
        let scene = ui
            .resolve_with(|_, text, _| Ok(Size::new(text_width(text), 20.)))
            .map_err(|e| e.to_string())?;
        Ok(Preview { ui, scene })
    };
    let preview = build().map_err(|s| JsValue::from_str(&s))?;
    let output = svg(&preview, None, bounds).map_err(|s| JsValue::from_str(&s))?;
    PREVIEW.with(|p| *p.borrow_mut() = Some(preview));
    Ok(output)
}
#[wasm_bindgen]
pub fn hover(x: f64, y: f64, bounds: bool) -> Result<String, JsValue> {
    PREVIEW.with(|p| {
        let p = p.borrow();
        let preview = p
            .as_ref()
            .ok_or_else(|| JsValue::from_str("No valid preview yet"))?;
        let id = preview.ui.hover_at(&preview.scene, x, y);
        svg(preview, id, bounds).map_err(|s| JsValue::from_str(&s))
    })
}
#[wasm_bindgen]
pub fn tap(x: f64, y: f64) -> Option<String> {
    PREVIEW.with(|p| {
        let p = p.borrow();
        let p = p.as_ref()?;
        p.ui.tap_at(&p.scene, x, y).map(str::to_owned)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parsed_program_runs_the_real_layout_and_geometry_and_reports_exact_sliders() {
        let code = r#"container([item("🎛").width(80).height(32).round(20px).extend_to("panel").on_tap("filter"), item("panel").width(200).height(100)]).layout(Column).gap(12).merge(["🎛","panel"])"#;
        let (item, params) = parser::parse(code).unwrap();
        let p = params.iter().find(|p| p.label == "round").unwrap();
        let chars: Vec<_> = code.encode_utf16().collect();
        assert_eq!(String::from_utf16(&chars[p.start..p.end]).unwrap(), "20px");
        let ui = item.build().unwrap();
        let scene = ui.resolve().unwrap();
        assert_eq!(
            scene.surface("🎛").unwrap().bounds.unwrap().max.y,
            scene.layout.frame("panel").unwrap().y
        );
        assert_eq!(ui.info("🎛").unwrap().tap.as_deref(), Some("filter"));
        assert!(svg(&Preview { ui, scene }, None, false)
            .unwrap()
            .contains("<svg"));
        for code in [
            "fetch(\"url\")",
            "item(\"x\").round(NaN)",
            "item(\"x\").width(-1)",
            "container([item(\"x\")]).layout(Grid(0))",
            "item(\"x\").center(12)",
        ] {
            assert!(parser::parse(code).is_err(), "{code}");
        }
    }
}
