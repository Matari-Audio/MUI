//! MUI core: a styled layout tree in, a z-ordered paint list out.
//!
//! Renderer-independent, `forbid(unsafe_code)`. Layout frames and painted
//! geometry stay separate: a node's outline may be its own rounded frame or
//! the filleted union of its children, and every shell is a true parallel
//! inset of the outline before it.
//!
//! The compact spelling, which says the same thing:
//!
//! ```
//! use mui_core::prelude::*;
//! # let _before =
//! column([row([text("Filter"), spacer(), text("on")])
//!     .align(Align::Center)
//!     .justify(Justify::SpaceBetween)
//!     .width(Len::Px(240.))])
//! # ; let _after =
//! col![row!["Filter", spacer(), "on"].between().w(240)]
//! # ;
//! ```
#![forbid(unsafe_code)]

mod color;
pub mod curve;
mod dsl;
mod element;
mod motion;
mod scene;
mod style;
mod theme;

pub use color::{Color, Mode, Palette, Pigment};
pub use dsl::{caption, label, title, IntoLen, Sugar};
pub use element::{
    canvas, column, grid, leaf, overlay, row, spacer, text, Canvas, Content, Draw, El, Element,
    IntoEl, Kind, Paints, Semantics, State, StateStyle, Styled,
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
pub use style::{
    Cursor, Fill, Fit, Gradient, Image, Mix, Paint, Radius, Role, Shadow, Stroke, Style,
};
pub use theme::{CornerProfile, Theme};

/// Everything a scene file needs, including the spacing tokens as bare
/// names: `.gap(M).pad(L)`.
pub mod prelude {
    pub use crate::Role::*;
    pub use crate::{
        canvas, caption, col, column, grid, label, leaf, overlay, resolve_scene, row, spacer,
        stack, text, title, Align, Color, Cursor, Draw, El, Fill, Fit, Gradient, Image, IntoEl,
        IntoLen, Justify, Kind, Len, Mix, Paints, Radius, Role, SceneSpec, Shadow, Size, State,
        Style, Styled, Sugar, Theme,
    };
    pub use mui_geometry::{Path, Point};
    pub use mui_layout::Spacing;
    pub use mui_layout::SpacingToken::{Xl, Xs, L, M, S};
    /// `n` steps of the theme's spacing unit: `.gap(step(1.5))`, for the
    /// values between `Xs` and `Xl`.
    ///
    /// ```
    /// use mui_core::prelude::*;
    /// let row = row!["a", "b"].gap(step(2.));
    /// assert_eq!(step(2.).resolve(Default::default()), 8.);
    /// ```
    pub fn step(n: f64) -> Spacing {
        Spacing::step(n)
    }
    /// A percentage length: `.width(pct(50.))`.
    pub fn pct(p: f64) -> Len {
        Len::Pct(p)
    }
    /// A fluid length with two stops -- CSS `clamp(min, pct%, max)`. The rail
    /// tracks the window between 64 and 220 px and neither collapses at 240
    /// nor sprawls at 2000:
    ///
    /// ```
    /// use mui_core::prelude::*;
    /// let rail = col![text("Filters")].w(clamp(64., 30., 220.)).id("rail");
    /// let row = row![rail, leaf(0., 0.).grow(1.)];
    /// let scene =
    ///     resolve_scene(&SceneSpec::new(row).offered(Size::new(240., 80.))).unwrap();
    /// assert_eq!(scene.surface("rail").unwrap().frame.size.width, 72.);
    /// ```
    pub fn clamp(min: f64, pct: f64, max: f64) -> Len {
        Len::Clamp { min, pct, max }
    }
}
