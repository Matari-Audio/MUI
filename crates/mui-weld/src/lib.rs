//! Material welding: one union field, spatial material ownership, and an explicit
//! temporal morph. This is a bounded CPU reference backend, not a GPU speed claim.
//!
//! Omission means the default: merge fills and borders. `Weld::shape()` keeps
//! original borders; `Weld::borders()` keeps original fills. Keeping and omitting
//! a channel are deliberately different operations.
#![forbid(unsafe_code)]

pub mod analytic;
pub mod boundary;
mod brush;
mod cache;
mod field;
mod raster;

pub use brush::{Brush, Color, Image, ImageFit, Stop};
pub use cache::WeldCache;
pub use field::{sample_field, Geometry, Point, Rect, Sample, Source};
pub use raster::{bake, Baked};

/// Material policy, independently selected for the body and its inside border.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Channel {
    /// Share source properties using the union's spatial ownership field.
    #[default]
    Blend,
    /// Keep each source's own material and coverage (including internal seams).
    Keep,
    /// Intentionally paint nothing for this channel. This is NOT `Keep`.
    Omit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weld {
    pub fill: Channel,
    pub border: Channel,
    /// Maximum gap bridged by two facing surfaces at full progress, in logical
    /// units. Multiple surfaces can interact; this is not a global distance cap.
    pub reach: f64,
    /// Compact-support bandwidth for source-material ownership, in logical units.
    pub blend: f64,
    /// 0 = original components; 1 = the complete weld. No implicit clock.
    pub progress: f64,
}
impl Default for Weld {
    fn default() -> Self {
        Self::all()
    }
}
impl Weld {
    /// Crisp contact welding: preserve source contours; no pre-contact blob.
    /// In the analytic GPU backend this uses exposed line/arc boundaries and
    /// Euclidean border distances. Material morphing stays independent.
    pub fn crisp() -> Self {
        Self::all().reach(0.0)
    }
    /// Opt an existing channel policy into crisp contact welding.
    pub fn sharp(self) -> Self {
        self.reach(0.0)
    }

    pub const fn all() -> Self {
        Self {
            fill: Channel::Blend,
            border: Channel::Blend,
            reach: 16.0,
            blend: 32.0,
            progress: 1.0,
        }
    }
    pub const fn shape() -> Self {
        Self {
            border: Channel::Keep,
            ..Self::all()
        }
    }
    pub const fn borders() -> Self {
        Self {
            fill: Channel::Keep,
            ..Self::all()
        }
    }
    pub const fn fill(mut self, policy: Channel) -> Self {
        self.fill = policy;
        self
    }
    pub const fn border(mut self, policy: Channel) -> Self {
        self.border = policy;
        self
    }
    pub const fn reach(mut self, units: f64) -> Self {
        self.reach = units;
        self
    }
    pub const fn blend(mut self, units: f64) -> Self {
        self.blend = units;
        self
    }
    pub const fn morph(mut self, progress: f64) -> Self {
        self.progress = progress;
        self
    }
    pub(crate) fn validate(self) -> Result<(), Error> {
        if !self.reach.is_finite()
            || !(0.0..=1e4).contains(&self.reach)
            || !self.blend.is_finite()
            || !(0.0..=1e4).contains(&self.blend)
            || !self.progress.is_finite()
            || !(0.0..=1.0).contains(&self.progress)
        {
            return Err(Error::Invalid("weld reach, blend, or progress"));
        }
        Ok(())
    }
    pub(crate) fn amount(self) -> f64 {
        self.progress * self.progress * (3.0 - 2.0 * self.progress)
    }
}

/// Resolution is supplied by the host, not hidden in geometry or widget lengths.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quality {
    pub scale: f64,
    pub max_pixels: usize,
    pub max_work: usize,
}
impl Default for Quality {
    fn default() -> Self {
        Self {
            scale: 1.0,
            max_pixels: 1_048_576,
            max_work: 100_000_000,
        }
    }
}
impl Quality {
    pub fn at_scale(scale: f64) -> Self {
        Self {
            scale,
            ..Self::default()
        }
    }
    pub(crate) fn validate(self) -> Result<(), Error> {
        if !self.scale.is_finite()
            || !(0.125..=8.0).contains(&self.scale)
            || self.max_pixels == 0
            || self.max_work == 0
        {
            return Err(Error::Invalid("weld raster quality"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// Order is painter order only for `Keep` and the unmerged morph endpoint.
    /// Fully blended geometry/materials do not depend on input ordering.
    pub sources: Vec<Source>,
    pub weld: Weld,
    pub quality: Quality,
}
impl Request {
    pub fn validate(&self) -> Result<(), Error> {
        self.weld.validate()?;
        self.quality.validate()?;
        if self.sources.is_empty() || self.sources.len() > MAX_SOURCES {
            return Err(Error::Invalid("weld requires 1..=64 sources"));
        }
        let mut segments = 0usize;
        for source in &self.sources {
            source.validate()?;
            segments = segments
                .checked_add(source.shape.cost())
                .ok_or(Error::Budget)?;
        }
        if segments > 250_000 {
            return Err(Error::Budget);
        }
        Ok(())
    }
}

pub const MAX_SOURCES: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid(&'static str),
    Budget,
    OpenContour,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(s) => write!(f, "invalid material weld: {s}"),
            Self::Budget => f.write_str("material weld exceeds its pixel or work budget"),
            Self::OpenContour => f.write_str("material weld contour did not close"),
        }
    }
}
impl std::error::Error for Error {}

#[cfg(test)]
mod tests;
