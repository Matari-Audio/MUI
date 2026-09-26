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
mod pick;
pub mod presets;
mod text;
pub mod visualization;

pub use bins::{BinAxis, BinEdit, Bins, bins, bins_hover};
pub(crate) use controls::step;
pub use controls::{Control, Variant, button, drag_value, knob, slider, stepped, toggle};
pub use curve::{CurveEdit, curve};
pub use pick::color_picker;
pub use presets::{card, chip, glass, panel, tile};
pub use text::{Newline, TextEdit, TextOpts, text_edit, text_input};
