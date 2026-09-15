#![forbid(unsafe_code)]

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
        let next = Self::new(self.convex * scale, self.concave * scale);
        next.valid().then_some(next)
    }
}
impl Default for CornerProfile {
    fn default() -> Self {
        Self::DEFAULT
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
    /// Optional perceptual source; RGB seed palettes remain supported.
    pub source_palette: Option<crate::color::Palette>,
    pub mode: crate::Mode,
    pub corners: CornerProfile,
    pub spacing: SpacingScale,
    pub stroke_width: f64,
    pub contrast: Contrast,
    pub hover_shift: f64,
}
impl Default for Theme {
    fn default() -> Self {
        Self::DEFAULT
    }
}
impl Theme {
    pub const DEFAULT: Self = Self {
        palette: crate::Palette::DEFAULT,
        source_palette: None,
        mode: crate::Mode::Dark,
        corners: CornerProfile::DEFAULT,
        spacing: SpacingScale::DEFAULT,
        stroke_width: 1.5,
        contrast: Contrast::AA,
        hover_shift: 0.06,
    };
    /// Keep the palette's derivation rules, resolving RGB only at the host boundary.
    pub const fn from_palette(palette: crate::color::Palette) -> Self {
        Self {
            source_palette: Some(palette),
            mode: palette.mode,
            hover_shift: palette.hover as f64,
            ..Self::DEFAULT
        }
    }
    pub fn colors(self) -> Result<crate::Colors, crate::ColorError> {
        match self.source_palette {
            Some(source) => source.with_mode(self.mode).resolve_rgb(),
            None => self.palette.resolve(self.mode),
        }
    }
    pub fn valid(self) -> bool {
        self.source_palette.is_none_or(|palette| palette.valid())
            && self.contrast.valid()
            && self.hover_shift.is_finite()
            && self.hover_shift > 0.
            && self.hover_shift <= 0.5
            && self.corners.valid()
            && self.spacing.valid()
            && self.stroke_width.is_finite()
            && self.stroke_width >= 0.0
    }
}

/// A const-friendly theme declaration using perceptual pigments.
/// Convert once with [`SourceTheme::resolve`] for Item/runtime authoring.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SourceTheme {
    pub corners: CornerProfile,
    pub spacing: SpacingScale,
    pub palette: crate::color::Palette,
    pub stroke_width: f64,
}
impl SourceTheme {
    pub const DEFAULT: Self = Self {
        corners: CornerProfile::DEFAULT,
        spacing: SpacingScale::DEFAULT,
        palette: crate::color::Palette::NEUTRAL,
        stroke_width: 1.5,
    };
    pub const fn resolve(self) -> Theme {
        Theme {
            corners: self.corners,
            spacing: self.spacing,
            stroke_width: self.stroke_width,
            ..Theme::from_palette(self.palette)
        }
    }
    pub fn valid(self) -> bool {
        self.resolve().valid()
    }
}
impl Default for SourceTheme {
    fn default() -> Self {
        Self::DEFAULT
    }
}
impl From<SourceTheme> for Theme {
    fn from(source: SourceTheme) -> Self {
        source.resolve()
    }
}

#[cfg(test)]
mod source_tests {
    use super::*;
    use crate::{color, Rgb};
    #[test]
    fn source_theme_retains_derivations_and_runtime_contrast() {
        const SOURCE: SourceTheme = SourceTheme {
            palette: color::Palette {
                primary: color::Pigment::new(242., 0.131),
                ..color::Palette::NEUTRAL
            },
            ..SourceTheme::DEFAULT
        };
        for mode in [color::Mode::Dark, color::Mode::Light] {
            let mut theme = SOURCE.resolve();
            theme.mode = mode;
            assert!(theme.valid());
            let colors = theme.colors().unwrap();
            let palette = SOURCE.palette.with_mode(mode);
            assert_eq!(colors.canvas, Rgb::try_from(palette.background()).unwrap());
            assert_eq!(
                colors.primary[0].fill,
                Rgb::try_from(palette.primary()).unwrap()
            );
            for bg in [colors.canvas, colors.panel, colors.raised] {
                assert!(colors.text.contrast(bg) >= 4.5);
                assert!(colors.muted.contrast(bg) >= 4.5);
                assert!(colors.outline.contrast(bg) >= 3.);
            }
            for accent in colors.primary.into_iter().chain(colors.status) {
                assert!(accent.on_fill.contrast(accent.fill) >= 4.5);
                assert!(accent.on_soft.contrast(accent.soft) >= 4.5);
            }
        }
        let bad = Theme::from_palette(color::Palette {
            step: f32::NAN,
            ..SOURCE.palette
        });
        assert!(!bad.valid());
        assert!(bad.colors().is_err());
        assert_eq!(Theme::DEFAULT, Theme::default());
    }
}
