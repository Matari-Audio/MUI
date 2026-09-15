//! Glyph outlines as MUI geometry.
//!
//! A glyph here is not a texture — it is a [`mui_geometry::Path`] in the same
//! coordinate space as every other surface, so it flattens, tessellates and
//! (once contour winding is classified) composes with the Boolean surface
//! system like any other shape. [`text_run`] lays a whole string out the same
//! way, as one path; there is still no atlas anywhere.
//!
//! Variable-font axes are an argument rather than a font variant: the outline
//! is re-derived at whatever axis position is asked for. Animating Material
//! Symbols from unfilled to filled is `FILL` 0 -> 1 with no atlas in the way.
#![forbid(unsafe_code)]

use mui_geometry::{Path, PathCommand, Point};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::prelude::{LocationRef, Size};
use skrifa::{FontRef, GlyphId, MetadataProvider as _};

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
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
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

fn checked_size(size_px: f64) -> Result<f32, Error> {
    if !size_px.is_finite() || size_px <= 0. || size_px > f64::from(f32::MAX) {
        return Err(Error::InvalidOptions("size_px"));
    }
    let size = size_px as f32;
    if !size.is_finite() || size == 0. {
        return Err(Error::InvalidOptions("size_px"));
    }
    Ok(size)
}

fn checked_finite(value: f64, name: &'static str) -> Result<f64, Error> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(Error::InvalidOptions(name))
    }
}

fn checked_metric(value: f32, name: &'static str) -> Result<f64, Error> {
    checked_finite(f64::from(value), name)
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
    let size = checked_size(size_px)?;
    if !tolerance.is_finite() || tolerance <= 0. {
        return Err(Error::InvalidOptions("tolerance"));
    }
    let font = FontRef::new(font).map_err(|e| Error::Font(format!("{e}")))?;
    let glyph_id = font.charmap().map(ch).ok_or(Error::MissingGlyph(ch))?;
    if font.outline_glyphs().get(glyph_id).is_none() {
        return Err(Error::NoOutline(ch));
    }

    let location = font.axes().location(axes.iter().copied());
    let mut pen = PathPen {
        commands: Vec::new(),
        cursor: Point::new(0., 0.),
        open: false,
        tolerance,
        dx: 0.,
    };
    draw_glyph(&font, glyph_id, size, &location, &mut pen)?;
    let path = pen.finish();
    path.validate(usize::MAX)?;
    Ok(path)
}

/// One string laid out as a single [`Path`], plus the numbers a caller needs to
/// put a box around it.
///
/// The metrics travel with the path because they cost one lookup at the point
/// where the face is already open, and a caller that has to reopen the font to
/// find out how tall its own label is will get it wrong once and then cache it
/// wrong forever.
///
/// Every field is in the same y-down pixel space as `path`: the baseline is
/// `y = 0`, `ascent` is a positive distance *above* it and `descent` a positive
/// distance *below*. skrifa reports descent as a negative number in a y-up
/// space; flipping it here is why this is a struct and not a tuple.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    /// Every glyph's contours, each already translated to its pen position.
    /// Fill this **non-zero** -- see [`text_run`].
    pub path: Path,
    /// Total pen advance: where the next run would start, not the ink extent.
    /// A trailing space advances and draws nothing.
    pub advance: f64,
    pub ascent: f64,
    pub descent: f64,
    /// The face's own idea of a line pitch, leading included.
    pub line_height: f64,
    /// Every glyph id with its pen x, for a renderer with its own glyph
    /// cache and hinting; `path` is the same ink as plain geometry.
    pub glyphs: Vec<(u32, f64)>,
}

/// Lay `text` out as one path, glyphs appended at successive pen positions.
///
/// Advance-only positioning: skrifa exposes no shaper, so pairs like `AV` sit
/// at their nominal advances and nothing reorders or substitutes. For the UI
/// labels this exists to draw that is invisible; for display type it is not,
/// and the fix is a real shaper rather than a correction here.
///
/// **Fill the result non-zero.** Two glyphs whose ink overlaps -- an italic
/// `f`, a negative sidebearing -- would XOR each other's overlap away under
/// even-odd, while counters stay empty under both rules because a typeface
/// reverses them. Non-zero is also what [`mui_input::Hit`] tests with, so what
/// you can click is what you can see.
///
/// A character the face has no glyph for draws `.notdef` rather than failing:
/// one missing codepoint must not blank a whole label. A character with no
/// outline -- a space -- contributes its advance and no contours.
///
/// [`mui_input::Hit`]: https://docs.rs/mui-input
pub fn text_run(
    font: &[u8],
    text: &str,
    size_px: f64,
    axes: &[Axis<'_>],
    tolerance: f64,
) -> Result<TextRun, Error> {
    let size = checked_size(size_px)?;
    if !tolerance.is_finite() || tolerance <= 0. {
        return Err(Error::InvalidOptions("tolerance"));
    }
    let font = FontRef::new(font).map_err(|e| Error::Font(format!("{e}")))?;
    let font_size = Size::new(size);
    let location = font.axes().location(axes.iter().copied());
    let charmap = font.charmap();
    let outlines = font.outline_glyphs();
    let glyph_metrics = font.glyph_metrics(font_size, LocationRef::from(&location));

    let mut pen = PathPen {
        commands: Vec::new(),
        cursor: Point::new(0., 0.),
        open: false,
        tolerance,
        dx: 0.,
    };
    let mut glyphs = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let glyph_id = charmap.map(ch).unwrap_or(GlyphId::NOTDEF);
        glyphs.push((glyph_id.to_u32(), pen.dx));
        if outlines.get(glyph_id).is_some() {
            draw_glyph(&font, glyph_id, size, &location, &mut pen)?;
        }
        pen.close_open_contour();
        pen.dx += glyph_metrics.advance_width(glyph_id).unwrap_or(0.) as f64;
    }
    let advance = checked_finite(pen.dx, "font metrics")?;

    let metrics = font.metrics(font_size, LocationRef::from(&location));
    let ascent = checked_metric(metrics.ascent, "font metrics")?;
    let descent = checked_metric(-metrics.descent, "font metrics")?;
    let line_height = checked_metric(
        metrics.ascent - metrics.descent + metrics.leading,
        "font metrics",
    )?;
    let path = pen.finish();
    path.validate(usize::MAX)?;
    Ok(TextRun {
        path,
        advance,
        ascent,
        // Negative in font space, positive below the baseline here.
        descent,
        line_height,
        glyphs,
    })
}

fn draw_glyph(
    font: &FontRef<'_>,
    glyph_id: GlyphId,
    size: f32,
    location: &skrifa::instance::Location,
    pen: &mut PathPen,
) -> Result<(), Error> {
    let Some(outline) = font.outline_glyphs().get(glyph_id) else {
        return Ok(());
    };
    outline
        .draw(
            DrawSettings::unhinted(Size::new(size), LocationRef::from(location)),
            pen,
        )
        .map_err(|e| Error::Draw(format!("{e}")))?;
    Ok(())
}

/// Collects `skrifa` pen calls into MUI path commands, flattening as it goes.
struct PathPen {
    commands: Vec<PathCommand>,
    cursor: Point,
    open: bool,
    tolerance: f64,
    /// Where this glyph's origin sits. Carried on the pen rather than applied
    /// afterwards so a run never pays for a second pass over its own points.
    dx: f64,
}

impl PathPen {
    /// Font space is y-up, the scene is y-down.
    fn point(&self, x: f32, y: f32) -> Point {
        Point::new(self.dx + x as f64, -(y as f64))
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
        let p = self.point(x, y);
        self.commands.push(PathCommand::MoveTo(p));
        self.cursor = p;
        self.open = true;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.line(p);
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let (p0, c, p1) = (self.cursor, self.point(cx, cy), self.point(x, y));
        let n = self.steps((p0 - c * 2. + p1).length(), 0.125);
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1. - t;
            self.line(p0 * (u * u) + c * (2. * u * t) + p1 * (t * t));
        }
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let p0 = self.cursor;
        let c0 = self.point(cx0, cy0);
        let c1 = self.point(cx1, cy1);
        let p1 = self.point(x, y);
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

/// One advance per `char` of `text`, at the default variation position. The
/// only allocation the measuring functions make.
fn advances(font: &[u8], text: &str, size_px: f64) -> Result<Vec<f64>, Error> {
    let size = checked_size(size_px)?;
    let font = FontRef::new(font).map_err(|e| Error::Font(format!("{e}")))?;
    let location = font.axes().location(std::iter::empty::<Axis<'_>>());
    let charmap = font.charmap();
    let glyph_metrics = font.glyph_metrics(Size::new(size), LocationRef::from(&location));
    text.chars()
        .map(|ch| {
            let glyph_id = charmap.map(ch).unwrap_or(GlyphId::NOTDEF);
            checked_finite(
                f64::from(glyph_metrics.advance_width(glyph_id).unwrap_or(0.)),
                "font metrics",
            )
        })
        .collect()
}

/// One laid-out line: the slice of the source it covers and how wide that is.
///
/// `text_range` keeps any whitespace the break consumed -- it is a slice of the
/// original string, not a trimmed copy -- while `advance` does not count
/// trailing spaces, so a right-aligned line does not hang.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text_range: std::ops::Range<usize>,
    pub advance: f64,
}

/// Greedy line breaking on advances alone.
///
/// Break opportunities are ASCII whitespace and just after a `'-'`; `'\n'`
/// forces a break; a word wider than `max_width` breaks at the glyph that
/// overflows rather than hanging off the edge.
///
/// ponytail: no UAX#14 -- no CJK, no Thai; add `unicode-linebreak` if that matters.
pub fn break_lines(
    font: &[u8],
    text: &str,
    size_px: f64,
    max_width: f64,
) -> Result<Vec<Line>, Error> {
    if !(max_width.is_finite() && max_width > 0.) {
        return Err(Error::InvalidOptions("max_width"));
    }
    let advances = advances(font, text, size_px)?;
    let mut lines = Vec::new();
    let mut start = 0;
    // Advance since `start`, trailing-whitespace part of it, and the width of
    // the word since the last break opportunity.
    let (mut x, mut trim, mut word) = (0., 0., 0.);
    let mut brk: Option<(usize, f64)> = None;

    for ((i, ch), &a) in text.char_indices().zip(&advances) {
        if ch == '\n' {
            lines.push(Line {
                text_range: start..i,
                advance: x - trim,
            });
            start = i + 1;
            (x, trim, word, brk) = (0., 0., 0., None);
            continue;
        }
        if ch.is_ascii_whitespace() {
            // Trailing space always fits: it costs nothing at the line end.
            x += a;
            trim += a;
            continue;
        }
        if trim > 0. {
            // Ink after spaces: the word starts here, and so may a line.
            brk = Some((i, x - trim));
            trim = 0.;
            word = 0.;
        }
        if x + a > max_width && i > start {
            match brk.filter(|&(bi, _)| bi > start) {
                Some((bi, advance)) => {
                    lines.push(Line {
                        text_range: start..bi,
                        advance,
                    });
                    start = bi;
                    x = word;
                }
                // The word itself does not fit: break at the overflowing glyph.
                None => {
                    lines.push(Line {
                        text_range: start..i,
                        advance: x,
                    });
                    start = i;
                    (x, word) = (0., 0.);
                }
            }
            brk = None;
        }
        x += a;
        word += a;
        if ch == '-' {
            brk = Some((i + 1, x));
            word = 0.;
        }
    }
    lines.push(Line {
        text_range: start..text.len(),
        advance: x - trim,
    });
    Ok(lines)
}

/// Pen x of the caret sitting *before* the char at `byte_index`, which must be
/// a char boundary. `text.len()` is the caret at the end.
pub fn caret_x(font: &[u8], text: &str, size_px: f64, byte_index: usize) -> Result<f64, Error> {
    if byte_index > text.len() || !text.is_char_boundary(byte_index) {
        return Err(Error::InvalidOptions("byte_index"));
    }
    Ok(advances(font, text, size_px)?
        .iter()
        .zip(text.char_indices())
        .take_while(|(_, (i, _))| *i < byte_index)
        .map(|(a, _)| a)
        .sum())
}

/// The char boundary whose caret is nearest `x`. The inverse of [`caret_x`],
/// which is what a click in a text field needs.
pub fn hit_index(font: &[u8], text: &str, size_px: f64, x: f64) -> Result<usize, Error> {
    let advances = advances(font, text, size_px)?;
    let (mut pen, mut best, mut best_d) = (0., 0, x.abs());
    for ((i, ch), a) in text.char_indices().zip(&advances) {
        pen += a;
        let d = (pen - x).abs();
        if d < best_d {
            (best, best_d) = (i + ch.len_utf8(), d);
        }
    }
    Ok(best)
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
        // The counter comes out empty because a typeface reverses it, not
        // because of the fill rule: the filled area is the ring, not the disc.
        // Compare against the outer contour's own area.
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

    /// Crossing number at `p`, cast along +x. The whole point of the non-zero
    /// switch is that this is not the same as its parity.
    fn winding(path: &Path, p: Point) -> i32 {
        let mut w = 0;
        for ring in path.flatten(0.05, 250_000).unwrap() {
            for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
                // Half-open in y so a vertex on the ray is counted once.
                if (a.y <= p.y) == (b.y <= p.y) {
                    continue;
                }
                let t = (p.y - a.y) / (b.y - a.y);
                if a.x + t * (b.x - a.x) > p.x {
                    w += if b.y > a.y { 1 } else { -1 };
                }
            }
        }
        w
    }

    #[test]
    fn overlapping_glyphs_fill_rather_than_cancel() {
        // Two `O`s a pixel apart: the outer contours overlap almost entirely.
        // Even-odd would XOR that overlap away and leave a crescent.
        let a = glyph_path(HACK_REGULAR, 'O', 96., &[], 0.05).unwrap();
        let b = a.rigid_transform(Point::new(1., 0.), 0.).unwrap();
        let both = Path {
            commands: a
                .commands
                .iter()
                .chain(b.commands.iter())
                .copied()
                .collect(),
        };
        // On the left stroke of the ring, where both glyphs have ink.
        let bounds =
            mui_geometry::Bounds::from_points(a.flatten(0.05, 250_000).unwrap().concat()).unwrap();
        let p = Point::new(bounds.min.x + 4., (bounds.min.y + bounds.max.y) / 2.);
        assert_eq!(winding(&a, p).abs(), 1, "one glyph covers the probe once");
        assert_eq!(
            winding(&both, p).abs(),
            2,
            "non-zero: covered twice, inside"
        );
        assert_eq!(winding(&both, p) % 2, 0, "even-odd would have erased it");
    }

    #[test]
    fn a_space_advances_without_ink() {
        let one = text_run(HACK_REGULAR, "a", 96., &[], 0.05).unwrap();
        let two = text_run(HACK_REGULAR, "a a", 96., &[], 0.05).unwrap();
        assert_eq!(
            two.path.flatten(0.05, 250_000).unwrap().len(),
            2 * one.path.flatten(0.05, 250_000).unwrap().len(),
            "the space contributes no contours"
        );
        // Hack is monospaced, so every advance is the same one.
        assert!((two.advance - 3. * one.advance).abs() < 1e-6, "{two:?}");
    }

    #[test]
    fn a_run_is_its_glyphs_side_by_side() {
        let run = text_run(HACK_REGULAR, "ab", 96., &[], 0.05).unwrap();
        let a = glyph_path(HACK_REGULAR, 'a', 96., &[], 0.05).unwrap();
        let b = glyph_path(HACK_REGULAR, 'b', 96., &[], 0.05).unwrap();
        let advance = text_run(HACK_REGULAR, "a", 96., &[], 0.05).unwrap().advance;
        let shifted = b.rigid_transform(Point::new(advance, 0.), 0.).unwrap();
        let expected: Vec<PathCommand> =
            a.commands.iter().copied().chain(shifted.commands).collect();
        assert_eq!(run.path.commands.len(), expected.len());
        // Not `assert_eq!` on the paths: `rigid_transform` rotates by zero,
        // which is a multiply by cos/sin and lands a few ulp away from the
        // pen's plain addition. Same geometry, different last bits.
        for (got, want) in run.path.commands.iter().zip(&expected) {
            let (g, w) = match (got, want) {
                (PathCommand::MoveTo(g), PathCommand::MoveTo(w))
                | (PathCommand::LineTo(g), PathCommand::LineTo(w)) => (*g, *w),
                (PathCommand::Close, PathCommand::Close) => continue,
                _ => panic!("command kinds diverge: {got:?} vs {want:?}"),
            };
            assert!((g - w).length() < 1e-9, "{g:?} vs {w:?}");
        }
    }

    #[test]
    fn metrics_are_y_down_and_positive_both_ways() {
        let run = text_run(HACK_REGULAR, "Hg", 96., &[], 0.05).unwrap();
        assert!(run.ascent > 0., "{run:?}");
        assert!(run.descent > 0., "descent is below the baseline: {run:?}");
        assert!(run.line_height >= run.ascent + run.descent, "{run:?}");
        let bounds =
            mui_geometry::Bounds::from_points(run.path.flatten(0.05, 250_000).unwrap().concat())
                .unwrap();
        assert!(-bounds.min.y <= run.ascent, "ink fits above the baseline");
        assert!(bounds.max.y <= run.descent, "and below it");
    }

    #[test]
    fn size_conversion_rejects_underflow_overflow_and_invalid_derived_values() {
        let ordinary = text_run(HACK_REGULAR, "A", 96., &[], 0.05).unwrap();
        assert!(ordinary.advance.is_finite());
        assert!(ordinary.ascent.is_finite());
        assert!(ordinary.descent.is_finite());
        assert!(ordinary.line_height.is_finite());
        assert!(glyph_path(HACK_REGULAR, 'A', 96., &[], 0.05).is_ok());

        let max_glyph = glyph_path(HACK_REGULAR, 'A', f64::from(f32::MAX), &[], 0.05)
            .expect("f32::MAX is representable and must keep finite path commands");
        assert!(max_glyph.validate(usize::MAX).is_ok());
        assert!(
            text_run(HACK_REGULAR, "A", f64::from(f32::MAX), &[], 0.05).is_err(),
            "text_run must reject non-finite metrics derived at f32::MAX"
        );

        for result in [
            glyph_path(HACK_REGULAR, 'A', f64::MAX, &[], 0.05).map(|_| ()),
            text_run(HACK_REGULAR, "A", f64::MAX, &[], 0.05).map(|_| ()),
        ] {
            assert!(result.is_err(), "f64::MAX must not cross the f32 boundary");
        }

        let smallest_positive = f64::from_bits(1);
        assert!(glyph_path(HACK_REGULAR, 'A', smallest_positive, &[], 0.05).is_err());
        assert!(text_run(HACK_REGULAR, "A", smallest_positive, &[], 0.05).is_err());
    }

    #[test]
    fn geometry_errors_preserve_their_source_chain() {
        let error = Error::Geometry(mui_geometry::Error::InvalidPath);
        assert!(std::error::Error::source(&error).is_some());
        assert!(std::error::Error::source(&Error::InvalidOptions("size_px")).is_none());
    }

    #[test]
    fn a_glyph_the_face_lacks_does_not_blank_the_label() {
        // `glyph_path` reports it; a run must not, or one stray codepoint
        // silently erases a whole line of UI text.
        assert!(glyph_path(HACK_REGULAR, '\u{10FFFF}', 96., &[], 0.05).is_err());
        let run = text_run(HACK_REGULAR, "a\u{10FFFF}a", 96., &[], 0.05).unwrap();
        assert!(!run.path.commands.is_empty());
        assert!(run.advance > 0.);
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

#[cfg(test)]
mod measure_tests {
    use super::*;
    use epaint_default_fonts::HACK_REGULAR;

    const SIZE: f64 = 16.;

    fn lines(text: &str, width: f64) -> Vec<&str> {
        break_lines(HACK_REGULAR, text, SIZE, width)
            .unwrap()
            .into_iter()
            .map(|l| &text[l.text_range])
            .collect()
    }

    #[test]
    fn a_line_breaks_at_the_last_space_that_fits() {
        let text = "hello world foo";
        let width = caret_x(HACK_REGULAR, text, SIZE, 11).unwrap();
        assert_eq!(lines(text, width), ["hello world ", "foo"]);
        let first = &break_lines(HACK_REGULAR, text, SIZE, width).unwrap()[0];
        assert!(
            (first.advance - width).abs() < 1e-9,
            "the trailing space does not count: {first:?}"
        );
    }

    #[test]
    fn a_word_wider_than_the_line_breaks_mid_word() {
        let word = "x".repeat(40);
        let out = lines(&word, caret_x(HACK_REGULAR, &word, SIZE, 10).unwrap());
        assert_eq!(out.len(), 4, "{out:?}");
        assert!(out.iter().all(|l| l.len() == 10), "{out:?}");
    }

    #[test]
    fn a_newline_breaks_whatever_fits() {
        assert_eq!(lines("a\nb", 1e6), ["a", "b"]);
    }

    #[test]
    fn a_caret_round_trips_through_its_x() {
        let text = "the quick brown fox";
        for (i, _) in text
            .char_indices()
            .chain(std::iter::once((text.len(), ' ')))
        {
            let x = caret_x(HACK_REGULAR, text, SIZE, i).unwrap();
            assert_eq!(hit_index(HACK_REGULAR, text, SIZE, x).unwrap(), i, "at {i}");
        }
    }
}
