//! MUI core: small intrinsic layout + topology-aware surface composition.
//!
//! The core is renderer-independent and forbids unsafe code. It deliberately
//! separates layout frames from painted geometry: a surface may be a frame,
//! a Boolean merge, or a true parallel inset/outset of another surface.
#![forbid(unsafe_code)]

mod color;
pub mod curve;
mod item;
mod style;
mod view;
pub use item::{container, item, Color, Direction, Item, ItemInfo, Rounding, Ui};
pub use style::{ItemStyle, StyleError};
pub use view::{Clip, View, ViewItem, ViewState};
pub mod dsl;
mod scene;
mod theme;
pub use color::{Accent, ColorError, Colors, Mode, Palette, Rgb, Seeds};

pub use mui_layout::{Align, Frame, Insets, Justify, Layout, Limits, Node, Overflow, Size};
pub use scene::{
    resolve_scene, resolve_scene_measured, resolve_scene_measured_with_baseline, CornerRule, Edge,
    Extension, FrameRadius, ResolvedScene, ResolvedSurface, SceneError, SceneSpec, SceneState,
    SurfaceSource, SurfaceSpec,
};
pub use theme::{Contrast, CornerProfile, Spacing, SpacingScale, SpacingToken, Theme};
