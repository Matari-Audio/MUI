//! What a resolve is asked for, and every way it can refuse.
use mui_geometry::{GeometryOptions, OffsetOptions};
use mui_layout::{Limits, Size};
use mui_text::Font;

use crate::{El, Theme};

#[derive(Debug, Clone)]
pub struct SceneSpec {
    pub theme: Theme,
    pub root: El,
    pub offered: Option<Size>,
    pub limits: Limits,
    pub geometry: GeometryOptions,
    pub offsets: OffsetOptions,
    /// The face for `text(..)` leaves. Without one, text is boxed at an
    /// estimate and draws nothing, so a layout test needs no font file.
    pub font: Option<Font>,
    /// Additional faces tried per grapheme when the primary face has no
    /// glyph. They are carried into the resolved text layer so both CPU and
    /// GPU renderers draw the selected face.
    pub fallback_fonts: Vec<Font>,
    /// The host's device pixels per layout unit. Set it and every edge the
    /// walk derives -- outlines, welds, clips, baselines -- lands on a device
    /// pixel, so abutting fills composite opaque and hinted glyphs keep an
    /// even leading. `None` leaves layout's raw f64 alone.
    pub device_scale: Option<f64>,
    /// Selected by the host. CPU reference remains available for snapshots.
    pub weld_backend: crate::WeldBackend,
}
impl SceneSpec {
    pub fn new(root: El) -> Self {
        Self {
            theme: Theme::default(),
            root,
            offered: None,
            limits: Limits::default(),
            geometry: GeometryOptions::default(),
            offsets: OffsetOptions::default(),
            font: None,
            fallback_fonts: Vec::new(),
            device_scale: None,
            weld_backend: crate::WeldBackend::Reference,
        }
    }
    /// Where material welds run: the CPU reference (default) or the GPU.
    pub fn weld_backend(mut self, backend: crate::WeldBackend) -> Self {
        self.weld_backend = backend;
        self
    }
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }
    pub fn offered(mut self, size: Size) -> Self {
        self.offered = Some(size);
        self
    }
    pub fn font(mut self, font: Font) -> Self {
        self.font = Some(font);
        self
    }
    /// Add a fallback face after the primary [`Self::font`].
    pub fn fallback_font(mut self, font: Font) -> Self {
        self.fallback_fonts.push(font);
        self
    }
    /// Snap every painted edge to the device grid. Three equal shares of 41
    /// px land on thirds, and two of the three seams between them composite
    /// translucent; at scale 1 they land on whole pixels instead:
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let row = row![block(0., 20.).grow(1.).id("a"), block(0., 20.).grow(1.)];
    /// let spec = SceneSpec::new(row).offered(Size::new(41., 20.)).scale(1.);
    /// let a = resolve(&spec).unwrap();
    /// let edge = a.surface("a").unwrap().rect.unwrap().bounds().x1;
    /// assert_eq!(edge, edge.round());
    /// ```
    pub fn scale(mut self, device_scale: f64) -> Self {
        self.device_scale = Some(device_scale);
        self
    }
    /// Refuse what no resolve could honour, before anything is cached.
    pub(super) fn validate(&self) -> Result<(), SceneError> {
        if !self.theme.is_valid() {
            return Err(SceneError::InvalidTheme);
        }
        if self
            .device_scale
            .is_some_and(|s| !(s.is_finite() && s > 0.0))
        {
            return Err(SceneError::InvalidScale);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum SceneError {
    InvalidTheme,
    InvalidRadius,
    MaterialWeld(mui_weld::Error),
    UnsupportedWeld(&'static str),
    /// Frame deltas must be finite and non-negative before any runtime state changes.
    InvalidFrameDelta,
    Layout(mui_layout::Error),
    Geometry(mui_geometry::Error),
    Text(mui_text::Error),
    /// [`ResolvedScene::set_text`](crate::ResolvedScene::set_text) was asked for a key that resolved no text
    /// layer: no such node, not a text node, or no font was set.
    NoTextLayer,
    /// [`SceneSpec::device_scale`] is not a finite, positive number.
    InvalidScale,
    /// A cross-reference (a surface member, a border ramp's tab or anchor)
    /// names an id no node in the subtree has.
    MissingId {
        what: &'static str,
        id: mui_layout::Id,
    },
}
impl std::fmt::Display for SceneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTheme => {
                f.write_str("the theme's corners, spacing or palette are unusable")
            }
            Self::InvalidFrameDelta => f.write_str("frame delta must be finite and non-negative"),
            Self::MaterialWeld(e) => write!(f, "{e}"),
            Self::UnsupportedWeld(feature) => write!(f, "unsupported material weld: {feature}"),
            Self::InvalidRadius => f.write_str(
                "a corner radius, shell inset or stroke width is negative or not finite",
            ),
            Self::Layout(e) => write!(f, "{e}"),
            Self::Geometry(e) => write!(f, "{e}"),
            Self::Text(e) => write!(f, "{e}"),
            Self::NoTextLayer => f.write_str("that key resolved no text layer"),
            Self::InvalidScale => f.write_str("the device scale must be finite and positive"),
            Self::MissingId { what, id } => {
                write!(f, "{what} `{}` is not in the subtree", id.as_str())
            }
        }
    }
}
impl std::error::Error for SceneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Layout(e) => Some(e),
            Self::Geometry(e) => Some(e),
            Self::Text(e) => Some(e),
            Self::MaterialWeld(e) => Some(e),
            _ => None,
        }
    }
}
impl From<mui_weld::Error> for SceneError {
    fn from(error: mui_weld::Error) -> Self {
        Self::MaterialWeld(error)
    }
}
impl From<mui_layout::Error> for SceneError {
    fn from(v: mui_layout::Error) -> Self {
        Self::Layout(v)
    }
}
impl From<mui_geometry::Error> for SceneError {
    fn from(v: mui_geometry::Error) -> Self {
        Self::Geometry(v)
    }
}
impl From<mui_text::Error> for SceneError {
    fn from(v: mui_text::Error) -> Self {
        Self::Text(v)
    }
}
#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::prelude::*;

    #[test]
    fn a_device_scale_that_is_not_finite_and_positive_is_refused() {
        for scale in [0., -1., f64::NAN, f64::INFINITY] {
            let spec = SceneSpec::new(block(10., 10.)).scale(scale);
            assert!(
                matches!(resolve(&spec), Err(SceneError::InvalidScale)),
                "{scale}"
            );
        }
    }

    #[test]
    fn errors_expose_their_source() {
        let e = resolve(&SceneSpec::new(block(f64::NAN, 1.))).unwrap_err();
        assert!(
            std::error::Error::source(&e)
                .unwrap()
                .is::<mui_layout::Error>()
        );
        let mut bad = welded_tab();
        bad.geometry.epsilon = f64::NAN;
        let e = resolve(&bad).unwrap_err();
        assert!(
            std::error::Error::source(&e)
                .unwrap()
                .is::<mui_geometry::Error>()
        );
        let _ = Spacing::px(1.);
    }

    #[test]
    fn errors_read_as_sentences_not_as_debug() {
        let e = resolve(&SceneSpec::new(block(f64::NAN, 1.))).unwrap_err();
        let s = e.to_string();
        assert!(!s.contains("Layout("), "{s}");
        assert_eq!(s, mui_layout::Error::InvalidValue.to_string());
        assert!(SceneError::InvalidTheme.to_string().contains("theme"));
    }
}
