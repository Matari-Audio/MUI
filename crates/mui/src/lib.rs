//! Facade crate for plugin/application authors.
#![forbid(unsafe_code)]

pub use mui_core as core;
#[cfg(feature = "egui")]
pub use mui_egui as egui;
pub use mui_geometry as geometry;
pub use mui_layout as layout;
pub use mui_tessellate as tessellate;

pub mod prelude {
    pub use mui_core::dsl::{column, leaf, row, stack, NodeExt};
    pub use mui_core::{
        resolve_scene, CornerProfile, CornerRule, FrameRadius, SceneSpec, SceneState, Spacing,
        SpacingScale, SpacingToken, SurfaceSource, SurfaceSpec, Theme,
    };
    pub use mui_layout::{Align, Insets, Justify, Node, Size};
}
