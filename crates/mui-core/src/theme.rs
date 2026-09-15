#![forbid(unsafe_code)]

use crate::color::Palette;
use mui_layout::SpacingScale;

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub corners: CornerProfile,
    pub spacing: SpacingScale,
    pub palette: Palette,
    pub stroke_width: f64,
    /// Default text size in pixels.
    pub text: f64,
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
        text: 14.0,
    };

    pub fn valid(self) -> bool {
        self.corners.valid()
            && self.spacing.valid()
            && self.palette.valid()
            && self.stroke_width.is_finite()
            && self.stroke_width >= 0.0
            && self.text.is_finite()
            && self.text > 0.0
    }
}
