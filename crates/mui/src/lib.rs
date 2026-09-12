//! Facade crate for plugin/application authors.
#![forbid(unsafe_code)]

pub use mui_core as core;
#[cfg(feature = "egui")]
pub use mui_egui as egui;
pub use mui_geometry as geometry;
pub use mui_layout as layout;
pub use mui_tessellate as tessellate;

pub mod prelude {
    pub use mui_core::{
        resolve_scene, CornerProfile, CornerRule, Radius, SceneSpec, SceneState, Spacing,
        SpacingScale, SpacingToken, SurfaceSource, SurfaceSpec, Theme,
    };
    pub use mui_layout::{column, leaf, overlay, row, Align, Insets, Justify, Node, Size};
}

/// Every `rust` block in the README, compiled and run by `cargo test --doc`.
///
/// `cfg(doctest)` means this exists only while doctests are being collected:
/// the README does not land in the rendered docs and costs nothing in a normal
/// build. Documentation that is not executed is documentation that is wrong,
/// and a colour crate's README is all executable claims.
#[cfg(doctest)]
#[doc = include_str!("../../../README.md")]
struct ReadmeDoctests;
