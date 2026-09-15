//! The gallery contents: one `PreviewScene` per thing worth looking at.
//! Adding one is a struct and a line in [`all`].

use mui::geometry::Path;
use mui::prelude::*;

pub trait PreviewScene {
    fn name(&self) -> &'static str;
    /// One line on what the scene is supposed to prove.
    fn about(&self) -> &'static str;
    /// The specimen. Built every frame like everything else, so a control
    /// that moved is simply visible next frame.
    fn specimen(&mut self, ui: &mut Ui) -> El;
    /// Sidebar rows under the description.
    fn controls(&mut self, _ui: &mut Ui) -> Vec<El> {
        Vec::new()
    }
    /// A typed character; `true` if the scene used it.
    fn key(&mut self, _c: char) -> bool {
        false
    }
    /// Geometry drawn over the surface named `key`, in its local space.
    fn overlay(&self) -> Option<(&'static str, Path)> {
        None
    }
}

pub fn all() -> Vec<Box<dyn PreviewScene>> {
    vec![
        Box::new(PillTab),
        Box::new(ConstantThickness),
        Box::new(SegmentedRow),
        Box::new(Widgets::default()),
        Box::new(GlyphAxes::new()),
    ]
}

/// A tab welded to a panel: unioned sharp, filleted after, with a shell
/// derived from the merged outline.
pub struct PillTab;
impl PreviewScene for PillTab {
    fn name(&self) -> &'static str {
        "Pill tab + panel"
    }
    fn about(&self) -> &'static str {
        "Boolean union first, fillets second. The inner shell is a parallel offset of the merged outline."
    }
    fn specimen(&mut self, _: &mut Ui) -> El {
        let control = |id: &str| leaf(28.0, 28.0).pill().fill(Role::Primary).id(id);
        let tab = column([control("plus"), control("phase"), control("warp")])
            .gap(10.0)
            .pad(22.0)
            .min_width(92.0)
            .align(Align::Center)
            .id("tab")
            .shell(12.0, Role::Raised);
        column([tab, leaf(520.0, 230.0).id("panel")])
            .align(Align::Start)
            .id("pill")
            .weld(Role::Surface)
    }
}

/// Three shells off one card. Every gap measures the same all the way round:
/// radius 28 − 12 = 16, exactly, not "child radius = parent radius".
pub struct ConstantThickness;
impl PreviewScene for ConstantThickness {
    fn name(&self) -> &'static str {
        "Constant thickness"
    }
    fn about(&self) -> &'static str {
        "Each ring is a parallel offset of the ring outside it."
    }
    fn specimen(&mut self, _: &mut Ui) -> El {
        leaf(320.0, 220.0)
            .radius(28.0)
            .fill(Role::Raised)
            .shell(12.0, Role::Field)
            .shell(12.0, Role::Raised)
            .shell(12.0, Role::Field)
            .id("card")
    }
}

/// Four flush cells welded into one outline: interior edges vanish, the
/// junctions go concave instead of pinching.
pub struct SegmentedRow;
impl PreviewScene for SegmentedRow {
    fn name(&self) -> &'static str {
        "Segmented row"
    }
    fn about(&self) -> &'static str {
        "Four flush cells unioned into one strip; shared edges are emitted zero times."
    }
    fn specimen(&mut self, _: &mut Ui) -> El {
        row((0..4).map(|i| leaf(70.0, 44.0 + 12.0 * f64::from(i % 2)).id(format!("cell-{i}"))))
            .radius(22.0)
            .id("strip")
            .weld(Role::Raised)
            .shell(8.0, Role::Field)
    }
}

/// The controls a plugin is made of, each a composition of flex shares.
#[derive(Default)]
pub struct Widgets {
    cutoff: f64,
    res: f64,
    gain: f64,
    bypass: bool,
    clicks: usize,
}
impl PreviewScene for Widgets {
    fn name(&self) -> &'static str {
        "Widgets"
    }
    fn about(&self) -> &'static str {
        "Knob, slider, toggle, button. No thumb is placed: it sits where two flex weights put it."
    }
    fn specimen(&mut self, ui: &mut Ui) -> El {
        let (go, clicked) = button(ui, "go", "Trigger");
        self.clicks += usize::from(clicked);
        column([
            row([
                knob(ui, "cutoff", "Cutoff", &mut self.cutoff, 0.0..=1.0, 72.0),
                knob(ui, "res", "Res", &mut self.res, 0.0..=1.0, 72.0),
            ])
            .gap(L)
            .justify(Justify::Center),
            slider(ui, "gain", "Gain", &mut self.gain, -24.0..=6.0),
            row([
                text("Bypass").fill(Role::Dim),
                toggle(ui, "bypass", &mut self.bypass),
                spacer(),
                text(format!("{}×", self.clicks)).fill(Role::Dim),
                go,
            ])
            .gap(S)
            .align(Align::Center),
        ])
        .gap(M)
        .pad(L)
        .width(300.0)
        .radius(20.0)
        .fill(Role::Surface)
        .id("widgets")
    }
}

/// A glyph as geometry, re-derived from the font at whatever axis position
/// the sliders are at. Point `MUI_PREVIEW_FONT` at a variable font for axes.
type Axis = (String, f64, f64, f64);

pub struct GlyphAxes {
    font: Vec<u8>,
    source: String,
    glyph: char,
    size: f64,
    /// `(tag, min, max, value)` per axis the face declares.
    axes: Vec<Axis>,
}
impl GlyphAxes {
    const CARD: f64 = 320.0;

    fn axes(font: &[u8]) -> Result<Vec<Axis>, mui_text::Error> {
        mui_text::axes(font).map(|a| {
            a.into_iter()
                .map(|a| (a.tag, a.min.into(), a.max.into(), a.default.into()))
                .collect()
        })
    }
    fn load_font(
        requested: Option<(String, std::io::Result<Vec<u8>>)>,
    ) -> (Vec<u8>, String, Vec<Axis>) {
        let fallback = |source| {
            let font = epaint_default_fonts::HACK_REGULAR.to_vec();
            let axes = Self::axes(&font).expect("bundled Hack font must be valid");
            (font, source, axes)
        };
        match requested {
            Some((path, Ok(font))) => match Self::axes(&font) {
                Ok(axes) => (font, path, axes),
                Err(e) => fallback(format!("{path}: {e} — using Hack")),
            },
            Some((path, Err(e))) => fallback(format!("{path}: {e} — using Hack")),
            None => fallback("Hack (static) — set MUI_PREVIEW_FONT for axes".to_owned()),
        }
    }
    pub fn new() -> Self {
        let requested = std::env::var_os("MUI_PREVIEW_FONT")
            .map(|p| (p.to_string_lossy().into_owned(), std::fs::read(&p)));
        let (font, source, axes) = Self::load_font(requested);
        Self {
            font,
            source,
            glyph: 'a',
            size: 220.0,
            axes,
        }
    }
}
impl PreviewScene for GlyphAxes {
    fn name(&self) -> &'static str {
        "Glyph axes"
    }
    fn about(&self) -> &'static str {
        "A variable-font outline rebuilt as geometry at every axis position. Type to change the glyph."
    }
    fn specimen(&mut self, _: &mut Ui) -> El {
        leaf(Self::CARD, Self::CARD)
            .radius(24.0)
            .fill(Role::Raised)
            .id("glyph-card")
    }
    fn controls(&mut self, ui: &mut Ui) -> Vec<El> {
        let mut rows = vec![
            text(self.source.clone()).fill(Role::Dim),
            slider(ui, "size", "size", &mut self.size, 24.0..=400.0),
        ];
        if self.axes.is_empty() {
            rows.push(text("no variation axes").fill(Role::Dim));
        }
        for (tag, min, max, value) in &mut self.axes {
            rows.push(slider(ui, tag, tag, value, *min..=*max));
        }
        rows
    }
    fn key(&mut self, c: char) -> bool {
        let printable = !c.is_control();
        if printable {
            self.glyph = c;
        }
        printable
    }
    fn overlay(&self) -> Option<(&'static str, Path)> {
        let settings: Vec<(&str, f32)> = self
            .axes
            .iter()
            .map(|(t, _, _, v)| (t.as_str(), *v as f32))
            .collect();
        let path = mui_text::glyph_path(&self.font, self.glyph, self.size, &settings, 0.05).ok()?;
        let b = mui::geometry::Bounds::from_points(path.flatten(0.05, 250_000).ok()?.concat())?;
        let centre = mui::geometry::Point::new(
            Self::CARD / 2.0 - (b.min.x + b.max.x) / 2.0,
            Self::CARD / 2.0 - (b.min.y + b.max.y) / 2.0,
        );
        Some(("glyph-card", path.rigid_transform(centre, 0.0).ok()?))
    }
}

#[cfg(test)]
mod tests {
    use super::GlyphAxes;

    #[test]
    fn malformed_readable_font_falls_back_with_parse_diagnostic() {
        let (font, source, _) =
            GlyphAxes::load_font(Some(("broken.ttf".to_owned(), Ok(Vec::new()))));
        assert_eq!(font.as_slice(), epaint_default_fonts::HACK_REGULAR);
        assert!(source.starts_with("broken.ttf: ") && source.ends_with(" — using Hack"));
    }
}
