//! The gallery contents.
//!
//! One `PreviewScene` per thing worth looking at. Adding a scene is adding a
//! `struct` and one line in [`all`] — deliberately the same shape as
//! `egui_demo_lib`'s `Demo` trait, so the gallery grows without tooling.

use mui_core::{CornerProfile, SceneSpec, Spacing, SurfaceSpec, Theme};
use mui_geometry::Path;
use mui_layout::generic::{column, leaf, row, Node};
use mui_layout::{Align, Size};

/// Something the gallery can draw. The scene is rebuilt on demand rather than
/// stored, so a future file-watching backend can swap the implementation
/// without the app caring.
pub trait PreviewScene {
    fn name(&self) -> &'static str;
    /// One line describing what this scene is supposed to prove.
    fn about(&self) -> &'static str;
    fn spec(&self) -> SceneSpec;

    /// Geometry that is not a resolved surface — a glyph outline, an imported
    /// path — in the same coordinate space as the scene's own surfaces.
    fn overlay(&self) -> Vec<(String, Path)> {
        Vec::new()
    }

    /// The scene's own sidebar controls. Return `true` when something moved
    /// that changes the geometry, and the gallery re-solves the scene.
    ///
    /// Most scenes are fixed specimens with nothing to tune, so the default
    /// draws nothing and rebuilds nothing.
    fn controls(&mut self, ui: &mut crate::ui::Ui<'_>) -> bool {
        let _ = ui;
        false
    }
}

pub fn all() -> Vec<Box<dyn PreviewScene>> {
    vec![
        Box::new(PillTab),
        Box::new(ConstantThickness),
        Box::new(SegmentedRow),
        Box::new(GlyphAxes::new()),
    ]
}

/// The canonical case: a tab welded to a panel, unioned sharp, filleted after,
/// with an inner shell derived from the *merged* outline.
pub struct PillTab;
impl PreviewScene for PillTab {
    fn name(&self) -> &'static str {
        "Pill tab + panel"
    }
    fn about(&self) -> &'static str {
        "Boolean union first, fillets second. The inner shell is a parallel offset of the merged outline, not an independently guessed radius."
    }
    fn spec(&self) -> SceneSpec {
        let controls = column([
            leaf(28.0, 28.0).id("plus"),
            leaf(28.0, 28.0).id("phase"),
            leaf(28.0, 28.0).id("warp"),
        ])
        .id("controls")
        .gap(10.0)
        .align(Align::Center);

        let tab = column([column([controls]).pad(10.0)])
            .id("tab")
            .pad(12.0)
            .min_size(Size::new(92.0, 0.0));

        let root = column([tab, leaf(520.0, 230.0).id("panel")])
            .id("root")
            .align(Align::Start);

        SceneSpec::new(root)
            .theme(Theme {
                corners: CornerProfile::new(28.0, 32.0),
                ..Theme::default()
            })
            .surface(SurfaceSpec::named_frame("panel"))
            .surface(SurfaceSpec::named_frame("tab"))
            .surface(SurfaceSpec::merge("outer", ["panel", "tab"]))
            .surface(SurfaceSpec::inset("pill-shell", "tab", Spacing::px(12.0)))
    }
}

/// Three nested insets off one parent. Every gap should measure the same all
/// the way around; a naive "child radius = parent radius" would pinch.
pub struct ConstantThickness;
impl PreviewScene for ConstantThickness {
    fn name(&self) -> &'static str {
        "Constant thickness nesting"
    }
    fn about(&self) -> &'static str {
        "Each ring is a parallel offset of the ring outside it. Parent radius 28 - inset 12 = child radius 16, exactly."
    }
    fn spec(&self) -> SceneSpec {
        let root = column([leaf(320.0, 220.0).id("card")]).id("root");
        SceneSpec::new(root)
            .theme(Theme {
                corners: CornerProfile::new(28.0, 28.0),
                ..Theme::default()
            })
            .surface(SurfaceSpec::named_frame("card"))
            .surface(SurfaceSpec::inset("ring-1", "card", Spacing::px(12.0)))
            .surface(SurfaceSpec::inset("ring-2", "ring-1", Spacing::px(12.0)))
            .surface(SurfaceSpec::inset("ring-3", "ring-2", Spacing::px(12.0)))
    }
}

/// A row of flush cells merged into one ring: the interior borders must
/// vanish and the junctions must go concave rather than pinching.
pub struct SegmentedRow;
impl PreviewScene for SegmentedRow {
    fn name(&self) -> &'static str {
        "Segmented row"
    }
    fn about(&self) -> &'static str {
        "Four flush cells unioned into a single outline. Shared interior edges are emitted zero times."
    }
    fn spec(&self) -> SceneSpec {
        let cells: Vec<Node> = (0..4)
            .map(|i| leaf(70.0, 44.0).id(format!("cell-{i}")))
            .collect();
        let root = row(cells).gap(0.0);

        let mut spec = SceneSpec::new(root).theme(Theme {
            corners: CornerProfile::new(22.0, 10.0),
            ..Theme::default()
        });
        for i in 0..4 {
            spec = spec.surface(SurfaceSpec::named_frame(format!("cell-{i}")));
        }
        spec.surface(SurfaceSpec::merge(
            "strip",
            ["cell-0", "cell-1", "cell-2", "cell-3"],
        ))
        .surface(SurfaceSpec::inset("strip-shell", "strip", Spacing::px(8.0)))
    }
}

/// A glyph as geometry, not as a texture.
///
/// The outline is re-derived from the font at whatever axis position the
/// sliders are at, then flattened and tessellated by the same code that draws
/// every other surface. This is what makes an icon animation — Material
/// Symbols going from `FILL` 0 to 1, say — a geometry change rather than a
/// stream of new glyph atlas entries.
///
/// Point `MUI_PREVIEW_FONT` at a variable font to get its axes as sliders.
/// Without one it falls back to a static face, where the sliders correctly do
/// nothing because the design space is a single point.
type AxisSetting = (String, f32, f32, f32);

pub struct GlyphAxes {
    font: std::sync::Arc<Vec<u8>>,
    /// Which face actually loaded. Named in the sidebar, because a slider
    /// that does nothing is only explicable once you know the face is static.
    source: String,
    glyph: char,
    size: f32,
    /// `(tag, min, max, value)` for every axis the loaded face declares.
    axes: Vec<AxisSetting>,
}

impl GlyphAxes {
    /// The canvas the glyph is centred on, and the reason the scene has a
    /// `spec` at all: the glyph lands inside a real MUI surface.
    const CARD: f64 = 320.0;

    fn axes(font: &[u8]) -> Result<Vec<AxisSetting>, mui_text::Error> {
        mui_text::axes(font).map(|axes| {
            axes.into_iter()
                .map(|a| (a.tag, a.min, a.max, a.default))
                .collect()
        })
    }

    fn load_font(
        requested: Option<(String, Result<Vec<u8>, std::io::Error>)>,
    ) -> (Vec<u8>, String, Vec<AxisSetting>) {
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
            None => fallback("Hack (static) — set MUI_PREVIEW_FONT to a variable font".to_owned()),
        }
    }

    pub fn new() -> Self {
        let requested = std::env::var_os("MUI_PREVIEW_FONT").map(|p| {
            let path = p.to_string_lossy().into_owned();
            (path, std::fs::read(&p))
        });
        let (font, source, axes) = Self::load_font(requested);
        Self {
            font: std::sync::Arc::new(font),
            source,
            glyph: 'a',
            size: 220.0,
            axes,
        }
    }
}

impl Default for GlyphAxes {
    fn default() -> Self {
        Self::new()
    }
}

impl PreviewScene for GlyphAxes {
    fn name(&self) -> &'static str {
        "Glyph axes"
    }
    fn about(&self) -> &'static str {
        "A variable-font outline rebuilt as MUI geometry at every axis position. \
         Drag an axis: the segment count in the status bar moves with it, because \
         the shape is re-solved rather than re-rasterised."
    }

    fn spec(&self) -> SceneSpec {
        SceneSpec::new(column([leaf(Self::CARD, Self::CARD).id("card")]).id("root"))
            .theme(Theme {
                corners: CornerProfile::new(24.0, 24.0),
                ..Theme::default()
            })
            .surface(SurfaceSpec::named_frame("card"))
    }

    fn controls(&mut self, ui: &mut crate::ui::Ui<'_>) -> bool {
        ui.note(&self.source);
        let mut changed = ui.char_field(&mut self.glyph, "glyph");
        changed |= ui.slider(&mut self.size, 24.0..=400.0, "size");
        if self.axes.is_empty() {
            ui.note("no variation axes");
        }
        for (tag, min, max, value) in &mut self.axes {
            changed |= ui.slider(value, *min..=*max, tag);
        }
        changed
    }

    fn overlay(&self) -> Vec<(String, Path)> {
        let settings: Vec<(&str, f32)> = self
            .axes
            .iter()
            .map(|(tag, _, _, value)| (tag.as_str(), *value))
            .collect();
        // Centred on the card, sitting on a baseline three quarters down —
        // close enough to optically centred for a glyph with a descender.
        let Ok(path) =
            mui_text::glyph_path(&self.font, self.glyph, self.size as f64, &settings, 0.05)
        else {
            return Vec::new();
        };
        let Some(bounds) = mui_geometry::Bounds::from_points(
            path.flatten(0.05, 250_000).unwrap_or_default().concat(),
        ) else {
            return Vec::new();
        };
        let centre = mui_geometry::Point::new(
            Self::CARD / 2.0 - (bounds.min.x + bounds.max.x) / 2.0,
            Self::CARD / 2.0 - (bounds.min.y + bounds.max.y) / 2.0,
        );
        match path.rigid_transform(centre, 0.0) {
            Ok(path) => vec![(format!("glyph {:?}", self.glyph), path)],
            Err(_) => Vec::new(),
        }
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
        assert!(source.starts_with("broken.ttf: "));
        assert!(source.ends_with(" — using Hack"));
    }
}
