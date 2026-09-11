//! Glyph outlines as MUI geometry.
//!
//! A glyph here is not a texture and not a text run — it is a
//! [`mui_geometry::Path`] in the same coordinate space as every other surface,
//! so it flattens, tessellates and (once contour winding is classified)
//! composes with the Boolean surface system like any other shape.
//!
//! Variable-font axes are an argument rather than a font variant: the outline
//! is re-derived at whatever axis position is asked for. Animating Material
//! Symbols from unfilled to filled is `FILL` 0 -> 1 with no atlas in the way.
#![forbid(unsafe_code)]

use mui_geometry::{Path, PathCommand, Point};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::prelude::{LocationRef, Size};
use skrifa::{FontRef, MetadataProvider as _};

#[derive(Debug)]
pub enum Error {
    /// The bytes are not a font this build can read.
    Font(String),
    /// The character has no glyph in this face. Fallback is the caller's job.
    MissingGlyph(char),
    /// The face has no scalable outline for that glyph (bitmap-only, say).
    NoOutline(char),
    Draw(String),
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
impl std::error::Error for Error {}
impl From<mui_geometry::Error> for Error {
    fn from(e: mui_geometry::Error) -> Self {
        Self::Geometry(e)
    }
}

/// One variable-font axis position, e.g. `("FILL", 1.0)` or `("wght", 500.0)`.
/// Unknown tags are ignored by the face; out-of-range values are clamped to the
/// axis bounds, so a caller cannot produce an outline the font does not define.
pub type Axis<'a> = (&'a str, f32);

/// One variation axis the face actually declares, with the range it accepts.
/// A UI needs this to offer a slider that cannot leave the design space.
#[derive(Debug, Clone, PartialEq)]
pub struct AxisInfo {
    pub tag: String,
    pub min: f32,
    pub default: f32,
    pub max: f32,
}

/// The variation axes of a face, in the font's own order. Empty for a static
/// font — which is the honest answer, not an error.
pub fn axes(font: &[u8]) -> Result<Vec<AxisInfo>, Error> {
    let font = FontRef::new(font).map_err(|e| Error::Font(format!("{e}")))?;
    Ok(font
        .axes()
        .iter()
        .map(|a| AxisInfo {
            tag: a.tag().to_string(),
            min: a.min_value(),
            default: a.default_value(),
            max: a.max_value(),
        })
        .collect())
}

/// The outline of one glyph, baseline at `y = 0` and growing upward as
/// negative `y` — the screen convention, already flipped out of font space.
///
/// `size_px` is the em size. `tolerance` is the maximum chord deviation when
/// the quadratic and cubic segments are flattened, in the same units.
///
/// Curves are flattened here rather than carried, because `PathCommand` models
/// only lines and exact circular arcs. That is the right vocabulary for the
/// corner system, and the wrong one for a glyph: no bezier in a typeface is a
/// circular arc. Flattening at load makes the tolerance a property of the call
/// instead of the paint, so re-derive the path if the display scale changes.
pub fn glyph_path(
    font: &[u8],
    ch: char,
    size_px: f64,
    axes: &[Axis<'_>],
    tolerance: f64,
) -> Result<Path, Error> {
    if !size_px.is_finite() || size_px <= 0. {
        return Err(Error::InvalidOptions("size_px"));
    }
    if !tolerance.is_finite() || tolerance <= 0. {
        return Err(Error::InvalidOptions("tolerance"));
    }
    let font = FontRef::new(font).map_err(|e| Error::Font(format!("{e}")))?;
    let glyph_id = font.charmap().map(ch).ok_or(Error::MissingGlyph(ch))?;
    let outline = font
        .outline_glyphs()
        .get(glyph_id)
        .ok_or(Error::NoOutline(ch))?;

    let location = font.axes().location(axes.iter().copied());
    let mut pen = PathPen {
        commands: Vec::new(),
        cursor: Point::new(0., 0.),
        open: false,
        tolerance,
    };
    outline
        .draw(
            DrawSettings::unhinted(Size::new(size_px as f32), LocationRef::from(&location)),
            &mut pen,
        )
        .map_err(|e| Error::Draw(format!("{e}")))?;
    Ok(pen.finish())
}

/// Collects `skrifa` pen calls into MUI path commands, flattening as it goes.
struct PathPen {
    commands: Vec<PathCommand>,
    cursor: Point,
    open: bool,
    tolerance: f64,
}

impl PathPen {
    /// Font space is y-up, the scene is y-down.
    fn point(x: f32, y: f32) -> Point {
        Point::new(x as f64, -(y as f64))
    }
    fn line(&mut self, p: Point) {
        self.commands.push(PathCommand::LineTo(p));
        self.cursor = p;
    }
    fn close_open_contour(&mut self) {
        if self.open {
            self.commands.push(PathCommand::Close);
            self.open = false;
        }
    }
    /// Uniform subdivision counts from the standard flatness bounds: the error
    /// of an n-segment polyline is bounded by the second difference of the
    /// control points over n squared, so invert that for n.
    fn steps(&self, second_difference: f64, numerator: f64) -> usize {
        let n = (second_difference * numerator / self.tolerance)
            .sqrt()
            .ceil();
        (n.max(1.) as usize).min(64)
    }
    fn finish(mut self) -> Path {
        self.close_open_contour();
        Path {
            commands: self.commands,
        }
    }
}

impl OutlinePen for PathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        // Some faces run contours together without an explicit close.
        self.close_open_contour();
        let p = Self::point(x, y);
        self.commands.push(PathCommand::MoveTo(p));
        self.cursor = p;
        self.open = true;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = Self::point(x, y);
        self.line(p);
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let (p0, c, p1) = (self.cursor, Self::point(cx, cy), Self::point(x, y));
        let n = self.steps((p0 - c * 2. + p1).length(), 0.125);
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1. - t;
            self.line(p0 * (u * u) + c * (2. * u * t) + p1 * (t * t));
        }
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let p0 = self.cursor;
        let c0 = Self::point(cx0, cy0);
        let c1 = Self::point(cx1, cy1);
        let p1 = Self::point(x, y);
        let bow = (p0 - c0 * 2. + c1)
            .length()
            .max((c0 - c1 * 2. + p1).length());
        let n = self.steps(bow, 0.75);
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1. - t;
            self.line(
                p0 * (u * u * u) + c0 * (3. * u * u * t) + c1 * (3. * u * t * t) + p1 * (t * t * t),
            );
        }
    }

    fn close(&mut self) {
        self.close_open_contour();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use epaint_default_fonts::HACK_REGULAR;

    fn area(path: &Path) -> f64 {
        path.flatten(0.05, 250_000)
            .unwrap()
            .iter()
            .map(|ring| {
                ring.iter()
                    .zip(ring.iter().cycle().skip(1))
                    .map(|(a, b)| a.cross(*b))
                    .sum::<f64>()
                    * 0.5
            })
            .sum()
    }

    #[test]
    fn a_counter_is_its_own_contour() {
        // "O" is the cheapest proof that contours survive: an outer ring and a
        // counter, wound against each other so the signed areas cancel down.
        let path = glyph_path(HACK_REGULAR, 'O', 64., &[], 0.05).unwrap();
        let rings = path.flatten(0.05, 250_000).unwrap();
        assert_eq!(rings.len(), 2, "outer contour plus counter");
        assert!(rings.iter().all(|r| r.len() > 8), "curves were flattened");
    }

    #[test]
    fn outline_scales_with_em_size() {
        let small = area(&glyph_path(HACK_REGULAR, 'H', 32., &[], 0.05).unwrap()).abs();
        let large = area(&glyph_path(HACK_REGULAR, 'H', 64., &[], 0.05).unwrap()).abs();
        assert!(small > 0.);
        assert!(
            (large / small - 4.).abs() < 0.05,
            "doubling the em size quadruples the area, got {large} / {small}"
        );
    }

    #[test]
    fn baseline_sits_at_zero_and_the_glyph_grows_upward() {
        let path = glyph_path(HACK_REGULAR, 'H', 64., &[], 0.05).unwrap();
        let rings = path.flatten(0.05, 250_000).unwrap();
        let ys: Vec<f64> = rings.iter().flatten().map(|p| p.y).collect();
        let top = ys.iter().cloned().fold(f64::INFINITY, f64::min);
        let bottom = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        assert!(top < 0., "cap height is above the baseline");
        assert!(bottom.abs() < 1., "'H' rests on the baseline, got {bottom}");
    }

    #[test]
    fn tolerance_buys_vertices() {
        let coarse = glyph_path(HACK_REGULAR, 'O', 64., &[], 0.5).unwrap();
        let fine = glyph_path(HACK_REGULAR, 'O', 64., &[], 0.01).unwrap();
        assert!(fine.commands.len() > coarse.commands.len() * 2);
        // Both describe the same glyph, so the area must not drift with it.
        let (fine, coarse) = (area(&fine).abs(), area(&coarse).abs());
        assert!((fine - coarse).abs() / fine < 0.05, "{fine} vs {coarse}");
    }

    #[test]
    fn an_axis_a_static_font_lacks_is_ignored_not_an_error() {
        let plain = glyph_path(HACK_REGULAR, 'H', 64., &[], 0.05).unwrap();
        let asked = glyph_path(HACK_REGULAR, 'H', 64., &[("FILL", 1.0)], 0.05).unwrap();
        assert_eq!(plain, asked, "Hack has no FILL axis, so nothing moves");
    }

    #[test]
    fn a_glyph_tessellates_like_any_other_surface() {
        let path = glyph_path(HACK_REGULAR, 'O', 96., &[], 0.05).unwrap();
        let mesh = mui_tessellate::Tessellator::default()
            .tessellate(&path, 0.05)
            .unwrap();
        assert_eq!(mesh.indices.len() % 3, 0);
        assert!(!mesh.indices.is_empty());
        // Even-odd leaves the counter empty: the filled area is the ring, not
        // the disc. Compare against the outer contour's own area.
        let rings = path.flatten(0.05, 250_000).unwrap();
        let outer = rings
            .iter()
            .map(|r| {
                (r.iter()
                    .zip(r.iter().cycle().skip(1))
                    .map(|(a, b)| a.cross(*b))
                    .sum::<f64>()
                    * 0.5)
                    .abs()
            })
            .fold(0., f64::max);
        let filled: f64 = mesh
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|i| {
                let (a, b, c) = (
                    mesh.positions[i[0] as usize],
                    mesh.positions[i[1] as usize],
                    mesh.positions[i[2] as usize],
                );
                ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() as f64 * 0.5
            })
            .sum();
        assert!(filled < outer * 0.9, "the counter is a hole, not fill");
        assert!(filled > outer * 0.3, "but the ring itself is filled");
    }

    #[test]
    fn a_missing_glyph_is_reported_not_drawn_blank() {
        assert!(matches!(
            glyph_path(HACK_REGULAR, '\u{10FFFD}', 64., &[], 0.05),
            Err(Error::MissingGlyph(_))
        ));
    }
}

#[cfg(test)]
mod axis_tests {
    use super::*;

    #[test]
    fn a_static_font_declares_no_axes() {
        assert!(axes(epaint_default_fonts::HACK_REGULAR).unwrap().is_empty());
    }
}
