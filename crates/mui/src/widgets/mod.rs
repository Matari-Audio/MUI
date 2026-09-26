//! The controls MUI ships with, and the presets they sit on.
//!
//! Every widget here is an ordinary styled tree, and every one has the same
//! shape: `&mut Ui`, the id, the label (controls only), the value it edits
//! by `&mut`, then its range or options. Every one returns a [`Response`]:
//! the element and what happened last frame -- `changed: bool` for
//! [`button`] (clicked), [`toggle`], [`slider`], [`knob`], [`drag_value`],
//! [`text_input`] and [`color_picker`], [`TextEdit`] for [`text_edit`], and
//! `Option<edit>` for [`curve`] and [`bins`], whose edit says which part
//! moved. A control's `el` is a [`Control`], finished with its look
//! (`.variant`, `.size`, `.role`) or dropped straight into a `row![..]`.
//! [`presets`] holds the shapes a panel is made of.
//!
//! A widget reads last frame's gestures and state from the [`Ui`](crate::Ui)
//! it is handed; nothing here owns a frame, a spring table or an event loop,
//! and nothing here rasterizes.

use mui_scene::{El, IntoEl};

/// What a widget hands back: its tree, and what happened to it last frame.
///
/// `C` is `bool` for a value widget (changed, or clicked for a button) and
/// the edit for [`curve`] and [`bins`]. A control's `E` is a [`Control`],
/// which still takes its look; everything else's is an [`El`]. Either drops
/// into a `row![..]` whole, or read `.el` and `.changed`.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::new(Theme::DEFAULT);
/// let mut on = false;
/// let sw = toggle(&mut ui, "bypass", "Bypass", &mut on);
/// assert!(!sw.changed);
/// let strip = row![sw.el.size(Xs), body("Bypass")];
/// ```
#[must_use]
pub struct Response<C = bool, E = El> {
    pub el: E,
    pub changed: C,
}
impl<C, E: IntoEl> IntoEl for Response<C, E> {
    fn into_el(self) -> El {
        self.el.into_el()
    }
}
impl<C, E: IntoEl> From<Response<C, E>> for El {
    fn from(r: Response<C, E>) -> Self {
        r.el.into_el()
    }
}

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
pub use pick::{ColorOpts, color_picker};
pub use presets::{card, chip, glass, panel, tile};
pub use text::{Newline, TextEdit, TextOpts, text_edit, text_input};
