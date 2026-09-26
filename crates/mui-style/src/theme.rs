#![forbid(unsafe_code)]

use crate::color::Palette;
use mui_layout::SpacingScale;

/// The theme's radii, named by what kind of thing they round -- daisyUI's
/// `--radius-selector` / `--radius-field` / `--radius-box` -- plus the one
/// CSS has no word for: the concave radius a weld's junction turns through.
///
/// ```
/// use mui_style::{Corner, Corners, Theme};
/// let square = Theme { corners: Corners { field: 0.0, ..Corners::DEFAULT }, ..Theme::DEFAULT };
/// assert_eq!(square.corners.get(Corner::Field), 0.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corners {
    /// A toggle, a checkbox, a badge: small things that read as pills.
    pub selector: f64,
    /// A button, an input, a tab: the things a hand aims at.
    pub field: f64,
    /// A card, a panel, a dialog -- and what `Radius::Theme` resolves to.
    pub box_: f64,
    /// The inside of a weld's junction, where two frames meet.
    pub concave: f64,
}
impl Corners {
    pub const DEFAULT: Self = Self {
        selector: 12.0,
        field: 8.0,
        box_: 18.0,
        concave: 14.0,
    };

    /// The radius this kind of thing rounds by.
    ///
    /// ```
    /// use mui_style::{Corner, Corners};
    /// assert_eq!(Corners::DEFAULT.get(Corner::Box), Corners::DEFAULT.box_);
    /// ```
    pub fn get(self, c: Corner) -> f64 {
        match c {
            Corner::Selector => self.selector,
            Corner::Field => self.field,
            Corner::Box => self.box_,
        }
    }
    pub fn is_valid(self) -> bool {
        [self.selector, self.field, self.box_, self.concave]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.0)
    }
    /// Every radius multiplied, for [`Radius::Scale`](crate::Radius::Scale).
    ///
    /// ```
    /// use mui_style::Corners;
    /// assert_eq!(Corners::DEFAULT.scaled(0.).map(|c| c.box_), Some(0.));
    /// ```
    pub fn scaled(self, scale: f64) -> Option<Self> {
        if !scale.is_finite() || scale < 0.0 {
            return None;
        }
        Some(Self {
            selector: self.selector * scale,
            field: self.field * scale,
            box_: self.box_ * scale,
            concave: self.concave * scale,
        })
    }
}
impl Default for Corners {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Which of the theme's radii a node rounds by: `.radius(Corner::Field)`.
///
/// ```
/// use mui_style::{Corner, Radius};
/// assert_eq!(Radius::from(Corner::Field), Radius::Token(Corner::Field));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    Selector,
    Field,
    Box,
}

/// The sizes the text roles resolve to, in pixels: `title(..)`, `body(..)`
/// and `caption(..)` in `mui-scene` read these at resolve time, so a theme
/// that re-tunes them re-tunes every heading.
///
/// ```
/// use mui_style::{Theme, TypeScale};
/// let big = Theme { type_scale: TypeScale { title: 24.0, ..TypeScale::DEFAULT }, ..Theme::DEFAULT };
/// assert_eq!(big.type_scale.body, 13.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypeScale {
    pub title: f64,
    pub body: f64,
    pub caption: f64,
}
impl TypeScale {
    pub const DEFAULT: Self = Self {
        title: 18.0,
        body: 13.0,
        caption: 11.0,
    };
    fn is_valid(self) -> bool {
        [self.title, self.body, self.caption]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0)
    }
}
impl Default for TypeScale {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub corners: Corners,
    pub spacing: SpacingScale,
    pub palette: Palette,
    pub stroke_width: f64,
    /// Default text size in pixels.
    pub text: f64,
    /// The text roles' sizes.
    pub type_scale: TypeScale,
    /// The unit every control size multiplies: daisyUI's `--size-field`. One
    /// number rescales every button, knob, toggle and slider in the tree.
    pub control: f64,
    /// The face [`icon`](../mui_scene/fn.icon.html) draws its symbol in,
    /// unless the icon names its own with `.font(f)`.
    pub icon_font: Option<mui_text::Font>,
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
    /// # use mui_style::{Palette, Pigment, Theme};
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
        corners: Corners::DEFAULT,
        spacing: SpacingScale::DEFAULT,
        palette: Palette::NEUTRAL,
        stroke_width: 1.5,
        text: 14.0,
        type_scale: TypeScale::DEFAULT,
        control: 4.0,
        icon_font: None,
    };

    pub fn is_valid(&self) -> bool {
        self.corners.is_valid()
            && self.spacing.is_valid()
            && self.palette.is_valid()
            && self.stroke_width.is_finite()
            && self.stroke_width >= 0.0
            && self.text.is_finite()
            && self.text > 0.0
            && self.type_scale.is_valid()
            && self.control.is_finite()
            && self.control > 0.0
    }
}
