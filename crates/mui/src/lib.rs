//! Facade crate for plugin/application authors.
#![forbid(unsafe_code)]

pub use mui_core as core;
#[cfg(feature = "egui")]
pub use mui_egui as egui;
pub use mui_geometry as geometry;
pub use mui_layout as layout;
pub use mui_tessellate as tessellate;

pub mod prelude {
    pub use mui_core::dsl::{column, flow, leaf, row, stack, NodeExt};
    pub use mui_core::{
        resolve_scene, resolve_scene_measured, Colors, CornerProfile, CornerRule, Edge,
        FrameRadius, Mode, Palette, Rgb, SceneSpec, SceneState, Seeds, Spacing, SpacingScale,
        SpacingToken, SurfaceSource, SurfaceSpec, Theme,
    };
    pub use mui_layout::{Align, Axis, Fill, Gap, Hug, Insets, Justify, Node, Pad, Size, Sizing};
}
