//! What a MUI tree looks like, as data: colours, roles, fills and the
//! [`Theme`] they resolve against.
//!
//! Everything here is inert. A [`Fill`] is a description until [`Fill::paint`]
//! is asked with a [`Palette`] and the colour underneath, and a [`Style`] is a
//! record of intent until a scene walk applies it. Nothing in this crate
//! builds a tree, measures a box or paints a pixel.
//!
//! It is not the element DSL (that is `mui-scene`), not a renderer (that is
//! `mui-vello`), and not the layout solver (that is `mui-layout`). It shares
//! only the [`Spacing`](mui_layout::Spacing) vocabulary with the solver,
//! and reads it from the solver crate, which knows nothing of style.
#![forbid(unsafe_code)]

mod color;
mod style;
mod theme;

pub use color::{Color, Mode, Palette, Pigment};
pub use style::{
    Cursor, Elevation, Fill, Fit, Gradient, GradientKind, Image, Mix, Paint, Radius, Role, Shadow,
    ShadowKind, Stroke, Style,
};
pub use theme::{Corner, Corners, Theme, TypeScale};
