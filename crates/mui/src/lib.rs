//! Facade crate for plugin/application authors.
#![forbid(unsafe_code)]

pub use mui_core as core;
pub use mui_core::{color, curve, theme};
#[cfg(feature = "egui")]
pub use mui_egui as egui;
pub use mui_geometry as geometry;
pub use mui_layout as layout;
pub use mui_tessellate as tessellate;
#[cfg(feature = "text")]
pub mod text {
    pub use mui_core::paragraph::{parley, Paragraph, TextScene, TextStyle, TextSystem};
    pub use mui_text::*;
}

pub mod prelude {
    pub use mui_core::{
        container, item, resolve_scene, Color, Colors, Contrast, CornerProfile, CornerRule,
        Direction, FrameRadius, Item, ItemInfo, ItemStyle, Mode, Palette, Radius, Rgb, Rounding,
        SceneSpec, SceneState, Seeds, Spacing, SpacingScale, SpacingToken, StyleError,
        SurfaceSource, SurfaceSpec, Theme, Ui, View, ViewState,
    };
    pub use mui_layout::generic::{column, leaf, overlay, row};
    pub use mui_layout::Flow::{Auto, Column, Grid, Overlay, Row};
    pub use mui_layout::Horizontal::{Center, Left, Right};
    pub use mui_layout::Justify::{End, SpaceAround, SpaceBetween, SpaceEvenly, Start};
    pub use mui_layout::SpacingToken::{Xl, Xs, L, M, S};
    pub use mui_layout::Vertical::{Bottom, Middle, Top};
    pub use mui_layout::{Align, Fill, Gap, Hug, Insets, Justify, Overflow, Pad, Size, Sizing};
    pub use mui_layout::{Flow, Horizontal, Track, Vertical};
}
