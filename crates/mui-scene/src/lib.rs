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
//! What this crate is not: it holds no widgets and no state (`mui::widgets`),
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
mod surfaces;
pub use mui_geometry::BorderAlign;
pub use regions::ShapeLayout;
mod border_ramp;
pub use border_ramp::BorderRamp;
mod capture;
mod dsl;
mod element;
mod external;
mod material_weld;
pub use capture::{CaptureError, CaptureLayer, resize_capture};
mod scene;
pub use external::{ExternalWeld, WeldBackend};
mod weld_dsl;

pub use mui_weld::{Channel as WeldChannel, Quality as WeldQuality, Weld, WeldCache};

pub use dsl::{body, caption, title};
pub use element::{
    A11y, Appear, Canvas, CanvasCache, Carve, Content, Draw, El, Element, Extras, IntoEl, Memo,
    Outline, Paints, Semantics, State, StateStyle, Styled, TextRole, block, canvas, canvas_keyed,
    col, fits, grid, icon, row, spacer, stack, text,
};
pub use mui_geometry::CornerStyle;
pub use mui_layout::{
    Align, Area, Frame, Id, Insets, Justify, Layout, Len, Limits, Match, Node, Pin, Size, Spacing,
    SpacingScale, SpacingToken,
};
pub use mui_motion::{Ease, Keys, Spring, curve};
pub use mui_style::{
    Color, Corner, Corners, Cursor, Elevation, Fill, Fit, Gradient, GradientKind, Image, Mix, Mode,
    Paint, Palette, Pigment, Radius, Role, Shadow, ShadowKind, Stroke, Style, Theme, TypeScale,
};
pub use mui_text::{Axes, Font, Weight};
pub use scene::bar;
pub use scene::{
    Layer, Painted, PlacedPath, ResolvedScene, ResolvedSurface, Resolver, SceneError, SceneSpec,
    Text, TextCache, TextGlyph, push_index, resolve,
};

/// Everything a scene file needs, including the spacing tokens as bare
/// names: `.gap(M).pad(L)`.
pub mod prelude {
    pub use crate::{
        A11y, Align, Appear, Area, Axes, BorderAlign, BorderRamp, CanvasCache, Color, Corner,
        Cursor, Draw, Ease, El, Elevation, Fill, Fit, Font, Gradient, Id, Image, IntoEl, Justify,
        Keys, Len, Match, Mix, Paints, Pin, Radius, Resolver, Role, SceneSpec, Shadow,
        ShapeLayout, Size, State, Style, Styled, Theme, Weight, Weld, WeldBackend, WeldChannel,
        WeldQuality, block, body, canvas, canvas_keyed, caption, col, fits, grid, icon, resolve,
        row, spacer, stack, text, title, weld,
    };
    pub use mui_geometry::{CornerStyle, Path, Point};
    pub use mui_layout::Spacing;
    pub use mui_layout::SpacingToken::{L, M, S, Xl, Xs};
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
