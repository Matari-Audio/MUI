//! The compact spelling: container macros, length and alignment sugar, and
//! the three text roles.
//!
//! ```
//! use mui_core::prelude::*;
//! let bar = row!["Filter", spacer(), col![text("on"), text("off")]]
//!     .gap(M)
//!     .center()
//!     .w(pct(100.));
//! assert_eq!(bar.children().len(), 3);
//! ```
use crate::element::{text, El, Styled};
use mui_layout::{Align, Justify, Len};

/// A length argument: a `Len`, or a bare number in pixels. `Len` lives in
/// `mui-layout`, so `From<i32> for Len` cannot be written here; this trait is
/// the orphan-rule workaround, and it is why `.w(120)` compiles while
/// `.width(120)` still wants `120.`.
// ponytail: no blanket `impl<T: Into<Len>>`, which collides with `i32` under
// coherence; add variants here, or move the `From` impls into mui-layout.
pub trait IntoLen {
    fn into_len(self) -> Len;
}
impl IntoLen for Len {
    fn into_len(self) -> Len {
        self
    }
}
impl IntoLen for f64 {
    fn into_len(self) -> Len {
        Len::Px(self)
    }
}
impl IntoLen for i32 {
    fn into_len(self) -> Len {
        Len::Px(self as f64)
    }
}

/// The short names. Separate from [`Styled`] only because these forward to
/// `Node`'s own builders rather than to the paint style.
pub trait Sugar: Sized {
    /// Width: `.w(120)`, `.w(pct(50.))`.
    fn w(self, len: impl IntoLen) -> Self;
    /// Height.
    fn h(self, len: impl IntoLen) -> Self;
    /// Both: `.square(32)`.
    fn square(self, len: impl IntoLen) -> Self;
    /// Centred on both axes.
    fn center(self) -> Self;
    /// Packed at the start of both axes.
    fn start(self) -> Self;
    /// Packed at the end of both axes.
    fn end(self) -> Self;
    /// Children pushed to the two ends, cross-axis centred.
    fn between(self) -> Self;
    /// All of the parent, both axes: `.w(pct(100.)).h(pct(100.))`.
    fn full(self) -> Self;
}
impl Sugar for El {
    fn w(self, len: impl IntoLen) -> Self {
        self.width(len.into_len())
    }
    fn h(self, len: impl IntoLen) -> Self {
        self.height(len.into_len())
    }
    fn square(self, len: impl IntoLen) -> Self {
        let l = len.into_len();
        self.size(l, l)
    }
    fn center(self) -> Self {
        self.align(Align::Center).justify(Justify::Center)
    }
    fn start(self) -> Self {
        self.align(Align::Start).justify(Justify::Start)
    }
    fn end(self) -> Self {
        self.align(Align::End).justify(Justify::End)
    }
    fn between(self) -> Self {
        self.align(Align::Center).justify(Justify::SpaceBetween)
    }
    fn full(self) -> Self {
        self.w(Len::Pct(100.)).h(Len::Pct(100.))
    }
}

// ponytail: fixed px, because `Theme` carries one text size and no type
// scale; give `Theme` a `TypeScale` and read it in `resolve_scene` when a
// second size has to re-tune with the theme.
/// A heading, 18 px.
pub fn title(s: impl Into<String>) -> El {
    text(s).text_size(18.)
}
/// A field name, 13 px.
pub fn label(s: impl Into<String>) -> El {
    text(s).text_size(13.)
}
/// A footnote, 11 px.
pub fn caption(s: impl Into<String>) -> El {
    text(s).text_size(11.)
}

/// A row of children; every argument goes through [`IntoEl`](crate::IntoEl),
/// so `row!["Filter", spacer()]` works.
#[macro_export]
macro_rules! row {
    ($($child:expr),* $(,)?) => {
        $crate::row(::std::vec![$($crate::IntoEl::into_el($child)),*])
    };
}
/// A column. See [`row!`].
#[macro_export]
macro_rules! col {
    ($($child:expr),* $(,)?) => {
        $crate::column(::std::vec![$($crate::IntoEl::into_el($child)),*])
    };
}
/// An overlay: every child fills the same box, painted in order.
#[macro_export]
macro_rules! stack {
    ($($child:expr),* $(,)?) => {
        $crate::overlay(::std::vec![$($crate::IntoEl::into_el($child)),*])
    };
}
/// A grid: `grid![3; a, b, c, d]`, columns first.
#[macro_export]
macro_rules! grid {
    ($cols:expr $(; $($child:expr),* $(,)?)?) => {
        $crate::grid($cols, ::std::vec![$($($crate::IntoEl::into_el($child)),*)?])
    };
}

#[cfg(test)]
mod dsl_tests {
    use crate::prelude::*;
    use crate::{resolve_scene, Len, SceneSpec};

    #[test]
    fn macros_take_strings_and_els() {
        let r = row!["Filter", spacer(), col![text("a"), text("b")],];
        assert_eq!(r.children().len(), 3);
        assert_eq!(r.children()[2].children().len(), 2);
        assert!(row![].children().is_empty());
        assert!(stack![].children().is_empty());
    }
    #[test]
    fn lengths_and_alignment() {
        assert_eq!(leaf(1., 1.).w(120), leaf(1., 1.).width(Len::Px(120.)));
        assert_eq!(leaf(1., 1.).w(pct(50.)), leaf(1., 1.).width(Len::Pct(50.)));
        assert_eq!(leaf(1., 1.).square(8), leaf(1., 1.).size(8., 8.));
        assert_eq!(leaf(1., 1.).full(), leaf(1., 1.).w(pct(100.)).h(pct(100.)));
        let c = row![].center();
        assert_eq!(c, row![].align(Align::Center).justify(Justify::Center));
    }
    #[test]
    fn grid_wraps_five_cells_into_two_rows() {
        let cell = |i: usize| leaf(20., 20.).id(format!("c{i}"));
        let g = grid![3; cell(0), cell(1), cell(2), cell(3), cell(4)];
        let s = resolve_scene(&SceneSpec::new(g)).unwrap();
        let y = |k: &str| s.layout.frame(k).unwrap().y;
        assert_eq!(y("c0"), y("c2"));
        assert!(y("c3") > y("c0"));
        assert_eq!(y("c3"), y("c4"));
    }
}
