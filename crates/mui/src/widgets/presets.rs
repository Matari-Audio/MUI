//! The four shapes every panel is made of, as plain functions.
//!
//! A preset is a `Style` you merge (`.preset(panel())`, `.base(card())`) or
//! an `El` you finish (`chip("A").id("chip-A")`). No registry, no trait, no
//! variant table: a preset that needs a variant is a function with an
//! argument.
use mui_scene::prelude::*;
use mui_scene::Style;

/// The window's own ground: the surface colour and the big corner.
///
/// ```
/// use mui::prelude::*;
/// let editor = col!["Kurv"].pad(M).full().preset(panel()).clip().id("editor");
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
/// # use mui::scene::Style;
/// let mut dialog = col!["Save?"].pad(M).preset(card());
/// assert_eq!(dialog.style_mut().radius, Radius::Px(12.));
/// ```
pub fn card() -> Style {
    Style {
        fill: Raised.into(),
        radius: 12.0.into(),
        shadow: vec![Shadow::soft(12.0)],
        ..Style::default()
    }
}

/// Glass, at plugin scale: a translucent fill, a bright one-pixel top edge
/// and a soft inner floor. There is no backdrop blur behind it and there
/// will not be -- neither Vello backend can sample what it is over -- and at
/// this size those three layers are what the look actually is.
///
/// The top edge is an inset shadow with a half-pixel feather: an inset
/// shadow fades over its blur, so a zero-blur one would paint nothing.
///
/// ```
/// use mui::prelude::*;
/// let mut overlay = col!["Preset browser"].pad(M).preset(glass());
/// assert_eq!(overlay.style_mut().shadow.len(), 2);
/// ```
pub fn glass() -> Style {
    let edge = |l: f32, a: f32| Fill::Color(Color::oklcha(l, 0.0, 0.0, a));
    Style {
        fill: edge(1.0, 0.10),
        radius: 14.0.into(),
        shadow: vec![
            Shadow {
                blur: 0.5,
                dy: 1.0,
                fill: edge(1.0, 0.35),
                ..Shadow::inset(0.5)
            },
            Shadow {
                blur: 6.0,
                dy: -3.0,
                fill: edge(0.0, 0.25),
                ..Shadow::inset(6.0)
            },
        ],
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
