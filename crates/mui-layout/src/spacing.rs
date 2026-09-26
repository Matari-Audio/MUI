//! Spacing tokens: the scale a gap or a pad is stated in.
//!
//! The layout crate (`gap`, `pad`, `resolve_with`) resolves them; the style
//! crate (a shell's thickness, a theme's scale) depends on this crate for them.

/// A step on the theme's spacing scale. `.gap(M)` reads like the CSS it
/// replaces and re-tunes with the theme instead of with a search-and-replace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpacingToken {
    Xs,
    S,
    M,
    L,
    Xl,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpacingScale {
    /// What one [`Spacing::Step`] comes to. The scale's own steps are not
    /// multiples of it: the tokens are tuned, the unit is for the values
    /// between them.
    pub unit: f64,
    pub xs: f64,
    pub s: f64,
    pub m: f64,
    pub l: f64,
    pub xl: f64,
}
impl SpacingScale {
    pub const DEFAULT: Self = Self {
        unit: 4.0,
        xs: 4.0,
        s: 8.0,
        m: 12.0,
        l: 18.0,
        xl: 28.0,
    };
    pub fn get(self, t: SpacingToken) -> f64 {
        match t {
            SpacingToken::Xs => self.xs,
            SpacingToken::S => self.s,
            SpacingToken::M => self.m,
            SpacingToken::L => self.l,
            SpacingToken::Xl => self.xl,
        }
    }
    pub fn is_valid(self) -> bool {
        [self.unit, self.xs, self.s, self.m, self.l, self.xl]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0)
    }
}
impl Default for SpacingScale {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A gap or padding: pixels, or a token resolved against the scale handed to
/// `resolve_with`. Plain `f64` converts, so `.gap(10.0)` still works.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Spacing {
    Px(f64),
    Token(SpacingToken),
    /// `n` times the scale's [`unit`](SpacingScale::unit): the values
    /// between the five tokens, without leaving the theme.
    Step(f64),
}
impl Spacing {
    pub const fn px(v: f64) -> Self {
        Self::Px(v)
    }
    /// `n` units of the theme's spacing grid.
    ///
    /// ```
    /// use mui_layout::{Spacing, SpacingScale};
    /// assert_eq!(Spacing::step(1.5).resolve(SpacingScale::DEFAULT), 6.0);
    /// ```
    pub const fn step(n: f64) -> Self {
        Self::Step(n)
    }
    pub fn resolve(self, scale: SpacingScale) -> f64 {
        match self {
            Self::Px(v) => v,
            Self::Token(t) => scale.get(t),
            Self::Step(n) => n * scale.unit,
        }
    }
}
impl<T: crate::Px> From<T> for Spacing {
    fn from(v: T) -> Self {
        Self::Px(v.px())
    }
}
impl From<SpacingToken> for Spacing {
    fn from(t: SpacingToken) -> Self {
        Self::Token(t)
    }
}

/// What `.pad(..)` takes: one spacing on all four sides (`12`, `M`), an
/// `(x, y)` pair of pixels, or whole [`Insets`](crate::Insets).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pad {
    All(Spacing),
    Insets(crate::Insets),
}
impl<T: crate::Px> From<T> for Pad {
    fn from(v: T) -> Self {
        Self::All(Spacing::Px(v.px()))
    }
}
impl From<Spacing> for Pad {
    fn from(s: Spacing) -> Self {
        Self::All(s)
    }
}
impl From<SpacingToken> for Pad {
    fn from(t: SpacingToken) -> Self {
        Self::All(Spacing::Token(t))
    }
}
impl From<crate::Insets> for Pad {
    fn from(i: crate::Insets) -> Self {
        Self::Insets(i)
    }
}
impl<X: crate::Px, Y: crate::Px> From<(X, Y)> for Pad {
    fn from((x, y): (X, Y)) -> Self {
        Self::Insets(crate::Insets::symmetric(x.px(), y.px()))
    }
}
