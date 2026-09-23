//! The controls MUI ships with, and the presets they sit on.
//!
//! Every widget here is an ordinary styled tree: [`button`], [`knob`],
//! [`toggle`], [`slider`], [`text_input`], [`curve`] and [`bins`] return a
//! [`Control`] or an `El` that the caller finishes and drops into a
//! `row![..]`, and
//! [`presets`] holds the four shapes a panel is made of.
//!
//! What this crate is not: it is not the runtime. A widget reads last frame's
//! gestures and state through the [`Host`] trait, which `mui::Ui` implements;
//! nothing here owns a frame, a spring table or an event loop, and nothing
//! here rasterizes.
#![forbid(unsafe_code)]

use std::ops::RangeInclusive;

use mui_geometry::Point;
use mui_input::{KeyPress, Response};
use mui_scene::{ResolvedScene, Spring, Theme};

mod bins;
mod curve;
mod grapheme;
pub mod presets;
pub mod visualization;
mod widgets;

pub use bins::{bins, bins_hover, BinAxis, BinEdit, Bins};
pub use curve::{curve, CurveEdit};
pub use presets::{card, chip, glass, panel, tile};
pub use widgets::{button, knob, slider, step, text_input, toggle, Control, Variant};

/// What a widget needs from the runtime that hosts it: last frame's gesture
/// and focus state by key, the theme to size and colour against, a text
/// measure, and the sinks a field writes back to. `mui::Ui` is the
/// implementation; the trait exists so a control is a function of state and
/// not of the event loop.
pub trait Host {
    /// The theme every control sizes and colours itself against.
    fn theme(&self) -> &Theme;
    /// Last frame's resolved scene, for a widget that has to measure the box
    /// it was given.
    fn scene(&self) -> Option<&ResolvedScene>;
    /// Last frame's gesture on `id`.
    fn get(&self, id: &str) -> Response;
    /// Which shape of the canvas `id` the pointer is on, by the tag its
    /// `Draw` carried, latched for the length of a gesture.
    fn tag(&self, id: &str) -> Option<&str>;
    /// Hover and press amounts for `id`, 0..1 and spring-smoothed.
    fn state(&self, id: &str) -> (f64, f64);
    /// Apply a drag on `id` to `value` across `range`, `px` pixels for the
    /// full span. Returns whether it changed.
    fn drag(
        &self,
        id: &str,
        value: &mut f64,
        range: RangeInclusive<f64>,
        px: f64,
        vertical: bool,
    ) -> bool;
    /// A spring-smoothed value chasing `target`, so an outside change glides.
    fn tween_with(&mut self, id: &str, target: f64, spring: Spring) -> f64;
    /// Whether `id` holds the keyboard focus.
    fn focused(&self, id: &str) -> bool;
    /// Whether the last press on `id` was the second of a double click.
    fn double_click(&self, id: &str) -> bool;
    /// The pointer relative to `id`'s frame origin, if both exist.
    fn local(&self, id: &str) -> Option<Point>;
    /// The keys this frame, if `id` is focused; empty otherwise.
    fn keys(&self, id: &str) -> &[KeyPress];
    /// The text typed this frame, if `id` is focused.
    fn text(&self, id: &str) -> &str;
    /// `id`'s selection anchor and caret, in characters.
    fn sel(&self, id: &str) -> (usize, usize);
    /// Store `id`'s selection for the next frame.
    fn set_sel(&mut self, id: &str, anchor: usize, caret: usize);
    /// The clipboard the host handed in because a paste key arrived.
    fn pasted(&self) -> Option<&str>;
    /// Ask the host to put `s` on the clipboard.
    fn set_clipboard(&mut self, s: String);
    /// The text the input method is composing, and its cursor in bytes.
    fn preedit(&self) -> Option<(&str, Option<(usize, usize)>)>;
    /// Tell the host where this field's caret is, in the field's own space.
    fn set_ime_caret(&mut self, id: &str, at: Point, height: f64);
    /// The character index in `s` nearest `x`, in the scene's font.
    fn hit(&self, s: &str, size: f64, x: f64) -> usize;
    /// Where the caret sits `byte` bytes into `s`: the inverse of [`Host::hit`].
    fn caret_x(&self, s: &str, size: f64, byte: usize) -> f64;
    /// Whether the caret is on this frame.
    fn blink(&self) -> bool;
}
