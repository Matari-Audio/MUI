#[derive(Debug)]
pub enum Error {
    /// The bytes are not a font this build can read.
    Font(skrifa::raw::ReadError),
    /// The character has no glyph in this face. Fallback is the caller's job.
    MissingGlyph(char),
    /// The face has no scalable outline for that glyph (bitmap-only, say).
    NoOutline(char),
    Draw(skrifa::outline::DrawError),
    Geometry(mui_geometry::Error),
    InvalidOptions(&'static str),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Font(e) => write!(f, "cannot read font: {e}"),
            Self::MissingGlyph(c) => write!(f, "no glyph for {c:?} in this face"),
            Self::NoOutline(c) => write!(f, "no scalable outline for {c:?}"),
            Self::Draw(e) => write!(f, "outline draw failed: {e}"),
            Self::Geometry(e) => write!(f, "{e}"),
            Self::InvalidOptions(o) => write!(f, "invalid {o}"),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Font(e) => Some(e),
            Self::Draw(e) => Some(e),
            Self::Geometry(error) => Some(error),
            _ => None,
        }
    }
}
impl From<mui_geometry::Error> for Error {
    fn from(e: mui_geometry::Error) -> Self {
        Self::Geometry(e)
    }
}

pub(crate) fn checked_size(size_px: f64) -> Result<f32, Error> {
    if !size_px.is_finite() || size_px <= 0. || size_px > f64::from(f32::MAX) {
        return Err(Error::InvalidOptions("size_px"));
    }
    let size = size_px as f32;
    if !size.is_finite() || size == 0. {
        return Err(Error::InvalidOptions("size_px"));
    }
    Ok(size)
}

pub(crate) fn checked_finite(value: f64, name: &'static str) -> Result<f64, Error> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(Error::InvalidOptions(name))
    }
}

pub(crate) fn checked_metric(value: f32, name: &'static str) -> Result<f64, Error> {
    checked_finite(f64::from(value), name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_errors_preserve_their_source_chain() {
        let error = Error::Geometry(mui_geometry::Error::InvalidPath);
        assert!(std::error::Error::source(&error).is_some());
        assert!(std::error::Error::source(&Error::InvalidOptions("size_px")).is_none());
    }
}
