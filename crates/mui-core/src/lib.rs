//! MUI core: a styled layout tree in, a z-ordered paint list out.
//!
//! Renderer-independent, `forbid(unsafe_code)`. Layout frames and painted
//! geometry stay separate: a node's outline may be its own rounded frame or
//! the filleted union of its children, and every shell is a true parallel
//! inset of the outline before it.
#![forbid(unsafe_code)]

mod color;
mod element;
mod motion;
mod scene;
mod style;
mod theme;

pub use color::{Color, Mode, Palette, Pigment};
pub use element::{column, grid, leaf, overlay, row, spacer, text, Content, El, Element, Styled};
pub use motion::Spring;
pub use mui_layout::{
    Align, Frame, Insets, Justify, Layout, Len, Limits, Node, Size, Spacing, SpacingScale,
    SpacingToken,
};
pub use scene::{
    resolve_scene, Layer, Painted, ResolvedScene, ResolvedSurface, SceneError, SceneSpec,
    SceneState,
};
pub use style::{Fill, Gradient, Paint, Radius, Role, Shadow, Stroke, Style};
pub use theme::{CornerProfile, Theme};

/// Everything a scene file needs, including the spacing tokens as bare
/// names: `.gap(M).pad(L)`.
pub mod prelude {
    pub use crate::{
        column, grid, leaf, overlay, resolve_scene, row, spacer, text, Align, Color, El, Fill,
        Gradient, Justify, Len, Radius, Role, SceneSpec, Shadow, Size, Styled, Theme,
    };
    pub use mui_layout::SpacingToken::{Xl, Xs, L, M, S};
}
