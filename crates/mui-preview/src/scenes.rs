//! The gallery contents.
//!
//! One `PreviewScene` per thing worth looking at. Adding a scene is adding a
//! `struct` and one line in [`all`] — deliberately the same shape as
//! `egui_demo_lib`'s `Demo` trait, so the gallery grows without tooling.

use mui_core::dsl::{column, row};
use mui_core::{CornerProfile, CornerRule, FrameRadius, SceneSpec, Spacing, SurfaceSpec, Theme};
use mui_geometry::Path;
use mui_layout::{Align, Node, Size};

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
        let controls = column(
            "controls",
            [
                Node::leaf("plus", Size::new(28.0, 28.0)),
                Node::leaf("phase", Size::new(28.0, 28.0)),
                Node::leaf("warp", Size::new(28.0, 28.0)),
            ],
        )
        .gap(10.0)
        .align(Align::Center);

        let tab = column(
            "tab-frame",
            [column("pill-frame", [controls]).padding(10.0)],
        )
        .padding(12.0)
        .min_size(Size::new(92.0, 0.0));

        let root = column(
            "root",
            [tab, Node::leaf("panel-frame", Size::new(520.0, 230.0))],
        )
        .align(Align::Start);

        SceneSpec::new(root)
            .theme(Theme {
                corners: CornerProfile::new(28.0, 32.0),
                ..Theme::default()
            })
            .surface(SurfaceSpec::frame("panel", "panel-frame").radius(FrameRadius::Global))
            .surface(SurfaceSpec::frame("tab", "tab-frame").radius(FrameRadius::Global))
            .surface(SurfaceSpec::merge("outer", ["panel", "tab"]).corners(CornerRule::Global))
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
        let root = column("root", [Node::leaf("card-frame", Size::new(320.0, 220.0))]);
        SceneSpec::new(root)
            .theme(Theme {
                corners: CornerProfile::new(28.0, 28.0),
                ..Theme::default()
            })
            .surface(SurfaceSpec::frame("card", "card-frame").radius(FrameRadius::Global))
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
            .map(|i| Node::leaf(format!("cell-{i}"), Size::new(70.0, 44.0)))
            .collect();
        let root = row("segment-frame", cells).gap(0.0);

        let mut spec = SceneSpec::new(root).theme(Theme {
            corners: CornerProfile::new(22.0, 10.0),
            ..Theme::default()
        });
        for i in 0..4 {
            spec = spec.surface(
                SurfaceSpec::frame(format!("cell-{i}"), format!("cell-{i}"))
                    .radius(FrameRadius::Global),
            );
        }
        spec.surface(
            SurfaceSpec::merge("strip", ["cell-0", "cell-1", "cell-2", "cell-3"])
                .corners(CornerRule::Global),
        )
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
pub struct GlyphAxes {
    font: std::sync::Arc<Vec<u8>>,
    /// Which face actually loaded. Read by the sidebar, which is why it is
    /// carried rather than logged.
    #[allow(dead_code)]
    source: String,
    glyph: char,
    size: f32,
    /// `(tag, min, max, value)` for every axis the loaded face declares.
    axes: Vec<(String, f32, f32, f32)>,
}

impl GlyphAxes {
    /// The canvas the glyph is centred on, and the reason the scene has a
    /// `spec` at all: the glyph lands inside a real MUI surface.
    const CARD: f64 = 320.0;

    pub fn new() -> Self {
        let (font, source) = match std::env::var_os("MUI_PREVIEW_FONT")
            .map(|p| (std::fs::read(&p), p.to_string_lossy().into_owned()))
        {
            Some((Ok(bytes), path)) => (bytes, path),
            Some((Err(e), path)) => (
                epaint_default_fonts::HACK_REGULAR.to_vec(),
                format!("{path}: {e} — using Hack"),
            ),
            None => (
                epaint_default_fonts::HACK_REGULAR.to_vec(),
                "Hack (static) — set MUI_PREVIEW_FONT to a variable font".to_owned(),
            ),
        };
        let axes = mui_text::axes(&font)
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.tag, a.min, a.max, a.default))
            .collect();
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
        SceneSpec::new(column(
            "root",
            [Node::leaf("card-frame", Size::new(Self::CARD, Self::CARD))],
        ))
        .theme(Theme {
            corners: CornerProfile::new(24.0, 24.0),
            ..Theme::default()
        })
        .surface(SurfaceSpec::frame("card", "card-frame").radius(FrameRadius::Global))
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
