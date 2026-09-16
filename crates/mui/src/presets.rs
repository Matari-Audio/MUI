//! The four shapes every panel is made of, as plain functions.
//!
//! A preset is a `Style` you merge (`.preset(&panel())`, `.base(&card())`) or
//! an `El` you finish (`chip("A").id("chip-A")`). No registry, no trait, no
//! variant table: a preset that needs a variant is a function with an
//! argument.
use mui_core::prelude::*;
use mui_core::Style;

/// The window's own ground: the surface colour and the big corner.
///
/// ```
/// use mui::prelude::*;
/// let editor = col!["Kurv"].pad(M).full().preset(&panel()).clip().id("editor");
/// assert_eq!(editor.children().len(), 1);
/// ```
pub fn panel() -> Style {
    Style {
        fill: Surface.into(),
        radius: 16.0.into(),
        ..Style::default()
    }
}

/// A raised block on the panel, with a shadow to lift it.
///
/// ```
/// use mui::prelude::*;
/// # use mui::core::Style;
/// let mut dialog = col!["Save?"].pad(M).preset(&card());
/// assert_eq!(dialog.style_mut().radius, Radius::Px(12.));
/// ```
pub fn card() -> Style {
    Style {
        fill: Raised.into(),
        radius: 12.0.into(),
        shadow: Some(Shadow::soft(12.0)),
        ..Style::default()
    }
}

/// A small labelled pill: a tab, a tag, a segment.
///
/// ```
/// use mui::prelude::*;
/// let tags = row(["A", "B"].map(chip)).gap(S);
/// assert_eq!(tags.children().len(), 2);
/// ```
pub fn chip(s: &str) -> El {
    row![caption(s)].pad_xy(10.0, 4.0).pill().fill(Field)
}

/// A cell in a bank: centred, padded, raised, rounded. Hands `el` back
/// styled, so the caller keeps saying what is *in* it.
///
/// ```
/// use mui::prelude::*;
/// let cell = tile(col![leaf(40., 40.).pill().fill(Primary), caption("cut")]);
/// assert_eq!(cell.children().len(), 2);
/// ```
pub fn tile(el: El) -> El {
    el.gap(Xs)
        .align(Align::Center)
        .pad(S)
        .radius(10.0)
        .fill(Raised)
}
