//! Facade crate for plugin/application authors.
#![forbid(unsafe_code)]

pub use mui_core as core;
#[cfg(feature = "egui")]
pub use mui_egui as egui;
pub use mui_geometry as geometry;
pub use mui_layout as layout;
pub use mui_tessellate as tessellate;
#[cfg(feature = "text")]
pub use mui_text as text;

pub mod prelude {
    pub use mui_core::{
        container, item, Color, Colors, Contrast, Direction, Item, ItemInfo, ItemStyle, Mode,
        Palette, Rgb, Rounding, Seeds, Spacing, SpacingScale, SpacingToken, StyleError, Theme, Ui,
        View, ViewState,
    };
    pub use mui_layout::Flow::{Auto, Column, Grid, Overlay, Row};
    pub use mui_layout::Horizontal::{Center, Left, Right};
    pub use mui_layout::Justify::{End, SpaceAround, SpaceBetween, SpaceEvenly, Start};
    pub use mui_layout::SpacingToken::{Xl, Xs, L, M, S};
    pub use mui_layout::Vertical::{Bottom, Middle, Top};
    pub use mui_layout::{Align, Fill, Gap, Hug, Insets, Justify, Overflow, Pad, Size, Sizing};
    pub use mui_layout::{Flow, Horizontal, Track, Vertical};
}
