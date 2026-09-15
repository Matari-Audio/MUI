//! MUI core: a styled layout tree in, a z-ordered paint list out.
//!
//! Renderer-independent, `forbid(unsafe_code)`. Layout frames and painted
//! geometry stay separate: a node's outline may be its own rounded frame or
//! the filleted union of its children, and every shell is a true parallel
//! inset of the outline before it.
#![forbid(unsafe_code)]

mod color;
pub mod curve;
mod element;
mod motion;
mod scene;
mod style;
mod theme;

pub use color::{Color, Mode, Palette, Pigment};
pub use element::{
    canvas, column, grid, leaf, overlay, row, spacer, text, Canvas, Content, Draw, El, Element,
    IntoEl, Styled,
};
pub use motion::Spring;
pub use mui_layout::{
    Align, Frame, Insets, Justify, Layout, Len, Limits, Node, Size, Spacing, SpacingScale,
    SpacingToken,
};
pub use scene::{
    resolve_scene, resolve_scene_with, Layer, Painted, ResolvedScene, ResolvedSurface, SceneError,
    SceneSpec, SceneState, Text, TextCache,
};
pub use style::{Cursor, Fill, Gradient, Paint, Radius, Role, Shadow, Stroke, Style};
pub use theme::{CornerProfile, Theme};

/// Everything a scene file needs, including the spacing tokens as bare
/// names: `.gap(M).pad(L)`.
pub mod prelude {
    pub use crate::Role::*;
    pub use crate::{
        canvas, column, grid, leaf, overlay, resolve_scene, row, spacer, text, Align, Color,
        Cursor, Draw, El, Fill, Gradient, IntoEl, Justify, Len, Radius, Role, SceneSpec, Shadow,
        Size, Style, Styled, Theme,
    };
    pub use mui_geometry::{Path, Point};
    pub use mui_layout::SpacingToken::{Xl, Xs, L, M, S};
    /// A percentage length: `.width(pct(50.))`.
    pub fn pct(p: f64) -> Len {
        Len::Pct(p)
    }
}
