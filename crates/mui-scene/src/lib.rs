//! A styled element tree lowered to a resolved, paint-ordered scene.
//!
//! The [`El`] DSL and its `row!`/`col!`/`stack!`/`grid!` sugar build the tree,
//! [`Styled`] and [`Paints`] decorate it, and [`resolve_scene`] walks it once
//! against a [`Theme`] into a z-ordered [`ResolvedScene`] paint list.
//!
//! Renderer-independent, `forbid(unsafe_code)`. Layout frames and painted
//! geometry stay separate: a node's outline may be its own rounded frame or
//! the filleted union of its children, and every shell is a true parallel
//! inset of the outline before it.
//!
//! What this crate is not: it holds no widgets and no state (`mui-widgets`),
//! no event loop or input handling (`mui`), and no rasterizer (`mui-vello`).
//!
//! The compact spelling, which says the same thing:
//!
//! ```
//! use mui_scene::prelude::*;
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

mod regions;
pub use mui_geometry::BorderAlign;
pub use regions::ShapeLayout;
mod border_ramp;
pub use border_ramp::BorderRamp;
mod dsl;
mod element;
mod external;
mod material_weld;
mod scene;
pub use external::{ExternalWeld, WeldBackend};
mod weld_dsl;

pub use mui_weld::{Channel as WeldChannel, Quality as WeldQuality, Weld, WeldCache};

pub use dsl::{caption, label, title, IntoLen, Sugar};
pub use element::{
    canvas, canvas_cached, column, fits, grid, icon, leaf, overlay, row, spacer, text, Canvas,
    CanvasCache, Carve, Content, Draw, El, Element, IntoEl, Kind, Outline, Paints, Semantics,
    State, StateStyle, Styled,
};
pub use mui_geometry::CornerStyle;
pub use mui_layout::{
    Align, Area, Frame, Id, Insets, Justify, Layout, Len, Limits, Match, Node, Pin, Size, Spacing,
    SpacingScale, SpacingToken,
};
pub use mui_motion::{curve, Spring};
pub use mui_style::{
    Color, Corner, Corners, Cursor, Elevation, Fill, Fit, Gradient, GradientKind, Image, Mix, Mode,
    Paint, Palette, Pigment, Radius, Role, Shadow, ShadowKind, Stroke, Style, Theme,
};
pub use mui_text::{Axes, Font, Weight};
pub mod material_symbols;
pub use scene::{
    resolve_scene, resolve_scene_cached, resolve_scene_with, Layer, Painted, ResolvedScene,
    ResolvedSurface, SceneError, SceneSpec, Text, TextCache, TextGlyph,
};

/// Everything a scene file needs, including the spacing tokens as bare
/// names: `.gap(M).pad(L)`.
pub mod prelude {
    pub use crate::Role::*;
    pub use crate::{
        canvas, canvas_cached, caption, col, column, fits, grid, icon, label, leaf, overlay,
        resolve_scene, row, spacer, stack, text, title, weld, weld_morph, Align, Area, Axes,
        BorderAlign, BorderRamp, CanvasCache, Color, Corner, Cursor, Draw, El, Elevation, Fill,
        Fit, Font, Gradient, Id, Image, IntoEl, IntoLen, Justify, Kind, Len, Match, Mix, Paints,
        Pin, Radius, Role, SceneSpec, Shadow, ShapeLayout, Size, State, Style, Styled, Sugar,
        Theme, Weight, Weld, WeldBackend, WeldChannel, WeldQuality,
    };
    pub use mui_geometry::{CornerStyle, Path, Point};
    pub use mui_layout::Spacing;
    pub use mui_layout::SpacingToken::{Xl, Xs, L, M, S};
    /// `n` steps of the theme's spacing unit: `.gap(step(1.5))`, for the
    /// values between `Xs` and `Xl`.
    ///
    /// ```
    /// use mui_scene::prelude::*;
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
    /// A share of the nearest ancestor with a definite size on that axis --
    /// CSS `cqw`/`cqh`, without having to declare the container. The badge
    /// is a fifth of the panel, whatever the hugging row between them does:
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let badge = leaf(0., 12.).w(cq(20.)).id("badge");
    /// let panel = col![row![badge]].w(400.);
    /// let scene = resolve_scene(&SceneSpec::new(panel)).unwrap();
    /// assert_eq!(scene.surface("badge").unwrap().frame.size.width, 80.);
    /// ```
    pub fn cq(p: f64) -> Len {
        Len::Container(p)
    }
    /// A fluid length with two stops -- CSS `clamp(min, pct%, max)`. The rail
    /// tracks the window between 64 and 220 px and neither collapses at 240
    /// nor sprawls at 2000. The percentage is of the parent; for a share of
    /// the nearest sized ancestor instead, see [`cq`]:
    ///
    /// ```
    /// use mui_scene::prelude::*;
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
