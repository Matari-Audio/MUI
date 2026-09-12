#![forbid(unsafe_code)]

use crate::color::Palette;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerProfile {
    pub convex: f64,
    pub concave: f64,
}
impl CornerProfile {
    pub const DEFAULT: Self = Self::new(18.0, 14.0);

    pub const fn new(convex: f64, concave: f64) -> Self {
        Self { convex, concave }
    }
    pub fn valid(self) -> bool {
        self.convex.is_finite()
            && self.concave.is_finite()
            && self.convex >= 0.0
            && self.concave >= 0.0
    }
    pub fn scaled(self, scale: f64) -> Option<Self> {
        if !scale.is_finite() || scale < 0.0 {
            return None;
        }
        Some(Self::new(self.convex * scale, self.concave * scale))
    }
}
impl Default for CornerProfile {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpacingToken {
    Xs,
    S,
    M,
    L,
    Xl,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpacingScale {
    pub xs: f64,
    pub s: f64,
    pub m: f64,
    pub l: f64,
    pub xl: f64,
}
impl Default for SpacingScale {
    fn default() -> Self {
        Self::DEFAULT
    }
}
impl SpacingScale {
    pub const DEFAULT: Self = Self {
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
    pub fn valid(self) -> bool {
        [self.xs, self.s, self.m, self.l, self.xl]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spacing {
    Px(f64),
    Token(SpacingToken),
}
impl Spacing {
    pub const fn px(v: f64) -> Self {
        Self::Px(v)
    }
    pub const fn xs() -> Self {
        Self::Token(SpacingToken::Xs)
    }
    pub const fn s() -> Self {
        Self::Token(SpacingToken::S)
    }
    pub const fn m() -> Self {
        Self::Token(SpacingToken::M)
    }
    pub const fn l() -> Self {
        Self::Token(SpacingToken::L)
    }
    pub const fn xl() -> Self {
        Self::Token(SpacingToken::Xl)
    }
    pub fn resolve(self, theme: &Theme) -> Option<f64> {
        let v = match self {
            Self::Px(v) => v,
            Self::Token(t) => theme.spacing.get(t),
        };
        (v.is_finite() && v >= 0.0).then_some(v)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub corners: CornerProfile,
    pub spacing: SpacingScale,
    pub palette: Palette,
    pub stroke_width: f64,
}
impl Default for Theme {
    fn default() -> Self {
        Self::DEFAULT
    }
}
impl Theme {
    /// Every default, as a `const`, so an application's theme file can state
    /// what differs and spread the rest:
    ///
    /// ```
    /// # use mui_core::{Palette, Pigment, Theme};
    /// pub const SKIN: Theme = Theme {
    ///     palette: Palette {
    ///         primary: Pigment::new(242.0, 0.131),
    ///         ..Palette::NEUTRAL
    ///     },
    ///     ..Theme::DEFAULT
    /// };
    /// ```
    ///
    /// `Default::default()` is not usable in a `const`, and a theme that has
    /// to restate every field to be one is a theme that drifts from the
    /// crate's defaults silently.
    pub const DEFAULT: Self = Self {
        corners: CornerProfile::DEFAULT,
        spacing: SpacingScale::DEFAULT,
        palette: Palette::NEUTRAL,
        stroke_width: 1.5,
    };

    pub fn valid(self) -> bool {
        self.corners.valid()
            && self.spacing.valid()
            && self.palette.valid()
            && self.stroke_width.is_finite()
            && self.stroke_width >= 0.0
    }
}
