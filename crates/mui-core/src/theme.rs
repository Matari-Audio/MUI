#![forbid(unsafe_code)]

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerProfile {
    pub convex: f64,
    pub concave: f64,
}
impl CornerProfile {
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
        let next = Self::new(self.convex * scale, self.concave * scale);
        next.valid().then_some(next)
    }
}
impl Default for CornerProfile {
    fn default() -> Self {
        Self::new(18.0, 14.0)
    }
}

pub use mui_layout::{Spacing, SpacingScale, SpacingToken};

/// Minimum contrast policy. All text uses the normal-text threshold; the host
/// need not classify font sizes. Stronger thresholds may reject impossible pairs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contrast {
    pub text: f64,
    pub graphics: f64,
}
impl Contrast {
    pub const AA: Self = Self {
        text: 4.5,
        graphics: 3.,
    };
    pub const AAA: Self = Self {
        text: 7.,
        graphics: 3.,
    };
    pub fn valid(self) -> bool {
        self.text.is_finite()
            && (4.5..=21.).contains(&self.text)
            && self.graphics.is_finite()
            && (3.0..=21.).contains(&self.graphics)
    }
}
impl Default for Contrast {
    fn default() -> Self {
        Self::AA
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub palette: crate::Palette,
    pub mode: crate::Mode,
    pub corners: CornerProfile,
    pub spacing: SpacingScale,
    pub stroke_width: f64,
    pub contrast: Contrast,
    pub hover_shift: f64,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            palette: crate::Palette::default(),
            mode: crate::Mode::default(),
            corners: CornerProfile::default(),
            spacing: SpacingScale::default(),
            stroke_width: 1.5,
            contrast: Contrast::AA,
            hover_shift: 0.06,
        }
    }
}
impl Theme {
    pub fn colors(self) -> Result<crate::Colors, crate::ColorError> {
        self.palette.resolve(self.mode)
    }
    pub fn valid(self) -> bool {
        self.contrast.valid()
            && self.hover_shift.is_finite()
            && self.hover_shift > 0.
            && self.hover_shift <= 0.5
            && self.corners.valid()
            && self.spacing.valid()
            && self.stroke_width.is_finite()
            && self.stroke_width >= 0.0
    }
}
