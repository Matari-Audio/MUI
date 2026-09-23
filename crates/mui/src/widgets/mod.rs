//! The controls MUI ships with, and the presets they sit on.
//!
//! Every widget here is an ordinary styled tree, and every one has the same
//! shape: it takes `&mut Ui`, the id, and whatever it edits, and returns its
//! element with what happened last frame -- `(Control, bool)` for
//! [`button`] (clicked), [`toggle`], [`slider`] and [`knob`] (changed),
//! `(El, bool)` for [`text_input`] (changed), and `(El, Option<edit>)` for
//! [`curve`] and [`bins`], whose edit says which part moved. A [`Control`]
//! is finished with its look (`.variant`, `.size`, `.role`) or dropped
//! straight into a `row![..]`. [`presets`] holds the shapes a panel is made
//! of.
//!
//! A widget reads last frame's gestures and state from the [`Ui`](crate::Ui)
//! it is handed; nothing here owns a frame, a spring table or an event loop,
//! and nothing here rasterizes.

mod bins;
mod controls;
mod curve;
mod grapheme;
pub mod presets;
pub mod visualization;

pub use bins::{bins, bins_hover, BinAxis, BinEdit, Bins};
pub use controls::{button, knob, slider, step, text_input, toggle, Control, Variant};
pub use curve::{curve, CurveEdit};
pub use presets::{card, chip, glass, panel, tile};
