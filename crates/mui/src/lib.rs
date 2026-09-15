//! MUI: a styled tree in, pixels and gestures out.
//!
//! ```
//! use mui::prelude::*;
//! let mut ui = Ui::new(Theme::DEFAULT);
//! let mut cutoff = 0.5;
//! // One frame: build the tree, hand it in with the pointer, draw what comes back.
//! let root = column([slider(&mut ui, "cutoff", "Cutoff", &mut cutoff, 0.0..=1.0)])
//!     .pad(M)
//!     .fill(Role::Surface);
//! let frame = ui.frame(root, Some(Size::new(240.0, 80.0)), PointerInput::default(), 1.0 / 60.0).unwrap();
//! assert!(frame.scene.paint.len() > 3);
//! ```
#![forbid(unsafe_code)]

pub use mui_core as core;
#[cfg(feature = "egui")]
pub use mui_egui as egui;
pub use mui_geometry as geometry;
pub use mui_input as input;
pub use mui_layout as layout;
pub use mui_tessellate as tessellate;
pub use mui_vello as vello;

mod ui;
pub mod widgets;

pub use ui::{Frame, Ui};

pub mod prelude {
    pub use crate::widgets::{button, knob, slider, toggle};
    pub use crate::{Frame, Ui};
    pub use mui_core::prelude::*;
    pub use mui_core::{CornerProfile, Palette, Pigment, Spring};
    pub use mui_input::{PointerInput, Response};
}

/// Every runnable `rust` block in the README, compiled and run by
/// `cargo test --doc`; `rust,ignore` blocks remain illustrative by design.
#[cfg(doctest)]
#[doc = include_str!("../../../README.md")]
struct ReadmeDoctests;
