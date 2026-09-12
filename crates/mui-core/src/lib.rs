//! MUI core: small intrinsic layout + topology-aware surface composition.
//!
//! The core is renderer-independent and forbids unsafe code. It deliberately
//! separates layout frames from painted geometry: a surface may be a frame,
//! a Boolean merge, or a true parallel inset/outset of another surface.
#![forbid(unsafe_code)]

mod color;
pub mod dsl;
mod scene;
mod theme;
pub use color::{Accent, ColorError, Colors, Mode, Palette, Rgb, Seeds};

pub use mui_layout::{Align, Frame, Insets, Justify, Layout, Limits, Node, Size};
pub use scene::{
    resolve_scene, resolve_scene_measured, CornerRule, Edge, Extension, FrameRadius, ResolvedScene,
    ResolvedSurface, SceneError, SceneSpec, SceneState, SurfaceSource, SurfaceSpec,
};
pub use theme::{CornerProfile, Spacing, SpacingScale, SpacingToken, Theme};
