//! The gallery contents.
//!
//! One `PreviewScene` per thing worth looking at. Adding a scene is adding a
//! `struct` and one line in [`all`] — deliberately the same shape as
//! `egui_demo_lib`'s `Demo` trait, so the gallery grows without tooling.

use mui_core::dsl::{column, row};
use mui_core::{CornerProfile, CornerRule, FrameRadius, SceneSpec, Spacing, SurfaceSpec, Theme};
use mui_layout::{Align, Node, Size};

/// Something the gallery can draw. The scene is rebuilt on demand rather than
/// stored, so a future file-watching backend can swap the implementation
/// without the app caring.
pub trait PreviewScene {
    fn name(&self) -> &'static str;
    /// One line describing what this scene is supposed to prove.
    fn about(&self) -> &'static str;
    fn spec(&self) -> SceneSpec;
}

pub fn all() -> Vec<Box<dyn PreviewScene>> {
    vec![
        Box::new(PillTab),
        Box::new(ConstantThickness),
        Box::new(SegmentedRow),
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
