//! Styled trees and their z-ordered paint lists, using the shared layout solver.
mod element;
mod scene;
mod style;
pub use crate::color::{Color, Mode, Palette, Pigment};
pub use crate::theme::SourceTheme as Theme;
pub use crate::CornerProfile;
pub use element::{column, grid, leaf, overlay, row, spacer, text, Content, El, Element, Styled};
pub use scene::{
    resolve_scene, Layer, Painted, ResolvedScene, ResolvedSurface, SceneError, SceneSpec,
    SceneState,
};
pub use style::{Fill, Gradient, Paint, Radius, Role, Shadow, Stroke, Style};
pub mod prelude {
    pub use super::{
        column, grid, leaf, overlay, resolve_scene, row, spacer, text, Color, El, Fill, Gradient,
        Radius, Role, SceneSpec, Shadow, Styled, Theme,
    };
    pub use mui_layout::generic::Len;
    pub use mui_layout::SpacingToken::{Xl, Xs, L, M, S};
    pub use mui_layout::{Align, Justify, Size};
}
