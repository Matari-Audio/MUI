//! MUI: a styled tree in, pixels and gestures out.
//!
//! ```
//! use mui::prelude::*;
//! let mut ui = Ui::new(Theme::DEFAULT);
//! let mut cutoff = 0.5;
//! // One frame: build the tree, hand it in with the input, draw what comes back.
//! let root = col![
//!     label("Filter"),
//!     slider(&mut ui, "cutoff", "Cutoff", &mut cutoff, 0.0..=1.0).0,
//! ]
//! .gap(S)
//! .pad(M)
//! .fill(Surface);
//! let frame = ui.frame(root, Some(Size::new(240.0, 96.0)), Input::default(), 1.0 / 60.0).unwrap();
//! assert!(frame.scene.paint.len() > 3);
//! // frame.cursor is what to set; frame.tip is the tooltip that came due;
//! // frame.animating says whether to schedule another frame; frame.edits is
//! // every gesture that began or ended, and frame.clipboard is what a copy
//! // wants put on the system clipboard.
//! ```
#![forbid(unsafe_code)]

pub use mui_geometry as geometry;
pub use mui_input as input;
pub use mui_layout as layout;
pub use mui_motion as motion;
pub use mui_scene as scene;
pub use mui_vello as vello;

mod actions;
mod ui;
pub mod widgets;
pub use actions::SemanticAction;
pub use widgets::presets;

pub use ui::{Edit, Frame, Ui};

pub mod prelude {
    pub use crate::widgets::presets::{card, chip, glass, panel, tile};
    pub use crate::widgets::{
        bins, bins_hover, button, color_picker, curve, knob, slider, text_input, toggle, BinAxis,
        BinEdit, Bins, ColorFormat, ColorPicker, Control, CurveEdit, PickerShape, Variant,
    };
    pub use crate::{Edit, Frame, SemanticAction, Ui};
    pub use mui_input::{
        Axis, Button, Buttons, Ime, Input, Key, KeyPress, Mods, PointerInput, Response, FINE_DRAG,
    };
    pub use mui_scene::prelude::*;
    pub use mui_scene::{Corners, Mode, Palette, Pigment, SpacingToken, Spring};
}

/// Every runnable `rust` block in the README, compiled and run by
/// `cargo test --doc`; `rust,ignore` blocks remain illustrative by design.
#[cfg(doctest)]
#[doc = include_str!("../../../README.md")]
struct ReadmeDoctests;
