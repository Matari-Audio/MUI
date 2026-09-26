//! The compact spelling: container macros and the three text roles.
//!
//! ```
//! use mui_scene::prelude::*;
//! let bar = row!["Filter", spacer(), col![text("on"), text("off")]]
//!     .gap(M)
//!     .center()
//!     .w(pct(100.));
//! assert_eq!(bar.children().len(), 3);
//! ```
use crate::element::{El, TextRole, text};

fn role(s: impl Into<std::sync::Arc<str>>, r: TextRole) -> El {
    let mut e = text(s);
    e.payload_mut().text_role = Some(r);
    e
}
/// A heading: the theme's `type_scale.title`, 18 px by default.
pub fn title(s: impl Into<std::sync::Arc<str>>) -> El {
    role(s, TextRole::Title)
}
/// Body text and field names: `type_scale.body`, 13 px by default.
pub fn body(s: impl Into<std::sync::Arc<str>>) -> El {
    role(s, TextRole::Body)
}
/// A footnote: `type_scale.caption`, 11 px by default.
pub fn caption(s: impl Into<std::sync::Arc<str>>) -> El {
    role(s, TextRole::Caption)
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
        $crate::col(::std::vec![$($crate::IntoEl::into_el($child)),*])
    };
}
/// A stack: every child fills the same box, painted in order.
#[macro_export]
macro_rules! stack {
    ($($child:expr),* $(,)?) => {
        $crate::stack(::std::vec![$($crate::IntoEl::into_el($child)),*])
    };
}
/// A grid: `grid![3; a, b, c, d]`, columns first.
#[macro_export]
macro_rules! grid {
    ($cols:expr $(; $($child:expr),* $(,)?)?) => {
        $crate::grid($cols, ::std::vec![$($($crate::IntoEl::into_el($child)),*)?])
    };
}

/// The first of these that fits the room on offer wins; the rest are not
/// painted and take no space. Declare them widest first.
///
/// ```
/// use mui_scene::prelude::*;
/// let bar = fits![title("Export selection"), text("Export"), block(16., 16.)];
/// assert_eq!(bar.children().len(), 3);
/// ```
#[macro_export]
macro_rules! fits {
    ($($child:expr),* $(,)?) => {
        $crate::fits(::std::vec![$($crate::IntoEl::into_el($child)),*])
    };
}

#[cfg(test)]
mod dsl_tests {
    use crate::prelude::*;
    use crate::{Len, SceneSpec, resolve};

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
        assert_eq!(block(1., 1.).w(120), block(1., 1.).w(Len::Px(120.)));
        assert_eq!(block(1., 1.).w(pct(50.)), block(1., 1.).w(Len::Pct(50.)));
        assert_eq!(block(1., 1.).square(8), block(1., 1.).size(8., 8.));
        assert_eq!(
            block(1., 1.).full(),
            block(1., 1.).w(pct(100.)).h(pct(100.))
        );
        let c = row![].center();
        assert_eq!(c, row![].align(Align::Center).justify(Justify::Center));
    }
    #[test]
    fn grid_wraps_five_cells_into_two_rows() {
        let cell = |i: usize| block(20., 20.).id(format!("c{i}"));
        let g = grid![3; cell(0), cell(1), cell(2), cell(3), cell(4)];
        let s = resolve(&SceneSpec::new(g)).unwrap();
        let y = |k: &str| s.layout.frame(k).unwrap().y;
        assert_eq!(y("c0"), y("c2"));
        assert!(y("c3") > y("c0"));
        assert_eq!(y("c3"), y("c4"));
    }
    #[test]
    fn text_roles_follow_the_theme() {
        let mut th = Theme::default();
        th.type_scale.title = 30.;
        let el = col![title("T").id("t"), caption("c").id("c")];
        let s = resolve(&SceneSpec::new(el).theme(th)).unwrap();
        let t = s.layout.frame("t").unwrap().size.height;
        assert!(t > 2. * s.layout.frame("c").unwrap().size.height, "{t}");
    }
}
