//! Scalar vocabulary: sizes, insets, spacing tokens, alignment and lengths.
//!
//! The units a tree is written in, with no knowledge of the tree itself. A
//! [`Len`] resolves against a parent extent, a [`Spacing`] against the
//! theme scale handed to `resolve_with`, and both are plain data.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}
impl Size {
    pub const ZERO: Self = Self::new(0.0, 0.0);
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
    pub(crate) fn main(self, vertical: bool) -> f64 {
        if vertical { self.height } else { self.width }
    }
    pub(crate) fn cross(self, vertical: bool) -> f64 {
        if vertical { self.width } else { self.height }
    }
    pub(crate) fn axes(main: f64, cross: f64, vertical: bool) -> Self {
        if vertical {
            Self::new(cross, main)
        } else {
            Self::new(main, cross)
        }
    }
    pub(crate) fn valid(self, limit: f64) -> bool {
        [self.width, self.height]
            .iter()
            .all(|n| n.is_finite() && *n >= 0.0 && *n <= limit)
    }
}

/// `(width, height)`: `.min_size((40., 30.))`.
impl From<(f64, f64)> for Size {
    fn from((width, height): (f64, f64)) -> Self {
        Self::new(width, height)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}
impl Insets {
    pub const ZERO: Self = Self::all(0.0);
    pub const fn all(v: f64) -> Self {
        Self::symmetric(v, v)
    }
    pub const fn symmetric(horizontal: f64, vertical: f64) -> Self {
        Self {
            left: horizontal,
            right: horizontal,
            top: vertical,
            bottom: vertical,
        }
    }
    pub fn horizontal(self) -> f64 {
        self.left + self.right
    }
    pub fn vertical(self) -> f64 {
        self.top + self.bottom
    }
    pub(crate) fn valid(self, limit: f64) -> bool {
        [self.left, self.right, self.top, self.bottom]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0 && *v <= limit)
    }
}

/// Cross-axis placement. `Stretch` is the default and means "fill if you are a
/// container, centre if you are content": a column of rows fills its width,
/// a column of labels lines them up down the middle, and neither needs saying.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    Start,
    Center,
    End,
    #[default]
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}
impl Justify {
    pub(crate) fn as_align(self) -> Align {
        match self {
            Self::Start => Align::Start,
            Self::End => Align::End,
            _ => Align::Center,
        }
    }
}

/// A length on one axis. `Auto` is measured, `Px` is fixed, `Pct` is a share
/// of the parent's inner extent -- and counts as `Auto` while the parent is
/// still hugging, since there is nothing to take a share of yet.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Len {
    #[default]
    Auto,
    Px(f64),
    Pct(f64),
    /// CSS `clamp(min, pct%, max)`: a share of the parent that stops at two
    /// pixel bounds. The one length a sidebar wants on a window that is
    /// sometimes 240 px wide and sometimes 2000.
    ///
    /// ```
    /// use mui_layout::{leaf, resolve, row, Len, Size};
    /// let rail = Len::Clamp { min: 64.0, pct: 30.0, max: 220.0 };
    /// let tree = || row([leaf(0., 0.).width(rail).id("rail"), leaf(0., 0.).grow(1.)]);
    /// let at = |w: f64| {
    ///     resolve(&tree(), Some(Size::new(w, 40.)), Default::default())
    ///         .unwrap()
    ///         .frame("rail")
    ///         .unwrap()
    ///         .size
    ///         .width
    /// };
    /// assert_eq!((at(240.), at(200.), at(2000.)), (72., 64., 220.));
    /// ```
    Clamp {
        min: f64,
        pct: f64,
        max: f64,
    },
    /// A share of the nearest ancestor with a definite size on this axis --
    /// CSS `cqw`/`cqh`, without the `container-type` ceremony. `Pct` is a
    /// share of the parent, whatever the parent turned out to be; this is a
    /// share of the box that actually has a size.
    ///
    /// ```
    /// use mui_layout::{leaf, resolve, row, Len, Size};
    /// // The row hugs, so it is no one's container: the bar takes half of
    /// // the 400 px panel above it, not half of the row around it.
    /// let bar = leaf(0., 8.).width(Len::Container(50.)).id("bar");
    /// let tree = row([row([bar])]).width(Len::Px(400.));
    /// let l = resolve(&tree, Some(Size::new(400., 8.)), Default::default()).unwrap();
    /// assert_eq!(l.frame("bar").unwrap().size.width, 200.);
    /// ```
    Container(f64),
}
/// A bare number is pixels: `.width(120)`, `.w(12.5)`.
impl From<f64> for Len {
    fn from(v: f64) -> Self {
        Self::Px(v)
    }
}
impl From<f32> for Len {
    fn from(v: f32) -> Self {
        Self::Px(v.into())
    }
}
impl From<i32> for Len {
    fn from(v: i32) -> Self {
        Self::Px(v.into())
    }
}
impl Len {
    pub(crate) fn px(self) -> Option<f64> {
        match self {
            Self::Px(v) => Some(v),
            _ => None,
        }
    }
    /// `container` is the nearest definite ancestor extent on this axis;
    /// without one, a container share degrades to a share of the parent.
    pub(crate) fn fixed(self, parent: f64, container: Option<f64>) -> Option<f64> {
        match self {
            Self::Auto => None,
            Self::Px(v) => Some(v),
            Self::Pct(p) => Some(parent * p / 100.0),
            Self::Clamp { min, pct, max } => Some((parent * pct / 100.0).clamp(min, max)),
            Self::Container(p) => Some(container.unwrap_or(parent) * p / 100.0),
        }
    }
    pub(crate) fn valid(self, limit: f64) -> bool {
        match self {
            Self::Auto => true,
            Self::Px(v) => v.is_finite() && (0.0..=limit).contains(&v),
            Self::Pct(p) | Self::Container(p) => p.is_finite() && (0.0..=100.0).contains(&p),
            Self::Clamp { min, pct, max } => {
                Self::Pct(pct).valid(limit)
                    && Self::Px(min).valid(limit)
                    && Self::Px(max).valid(limit)
                    && min <= max
            }
        }
    }
}
