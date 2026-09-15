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

/// Every runnable `rust` block in the README, compiled and run by
/// `cargo test --doc`; `rust,ignore` blocks remain illustrative by design.
///
/// `cfg(doctest)` means this exists only while doctests are being collected:
/// the README does not land in the rendered docs and costs nothing in a normal
/// build. Executable documentation is checked by the test suite, while ignored
/// snippets are kept for deliberately partial or conceptual examples.
#[cfg(doctest)]
#[doc = include_str!("../../../README.md")]
struct ReadmeDoctests;
