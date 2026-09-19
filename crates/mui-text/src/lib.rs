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

use std::collections::HashMap;

use mui_geometry::{Path, PathCommand, Point};
use rustybuzz::{BufferClusterLevel, BufferFlags, Direction};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::prelude::{LocationRef, Size};
use skrifa::{FontRef, GlyphId, MetadataProvider as _};
use unicode_bidi::BidiInfo;
use unicode_segmentation::UnicodeSegmentation;

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

/// How heavy a run is drawn, as the `wght` axis position every variable font
/// names the same way: 400 regular, 700 bold.
///
/// A newtype rather than an enum because the axis is continuous -- a display
/// face that looks right at 520 should be able to say so -- and the four
/// constants cover what a UI usually asks for.
///
/// ponytail: a static face has no `wght` axis, so it draws at its one
/// weight; nothing here synthesises a bold by smearing outlines. Ship a
/// variable face, or a second blob for the bold, if the difference matters.
///
/// ```
/// use mui_text::Weight;
/// assert_eq!(Weight::BOLD.axis(), ("wght", 700.0));
/// assert_eq!(Weight::default(), Weight::REGULAR);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Weight(u16);
impl Weight {
    pub const REGULAR: Self = Self(400);
    pub const MEDIUM: Self = Self(500);
    pub const SEMIBOLD: Self = Self(600);
    pub const BOLD: Self = Self(700);

    /// Any position on the axis. The face clamps it to the range it declares.
    ///
    /// ```
    /// use mui_text::Weight;
    /// assert_eq!(Weight::new(520).value(), 520);
    /// ```
    pub const fn new(wght: u16) -> Self {
        Self(wght)
    }
    /// The number, for a caller that stores or shows it.
    pub const fn value(self) -> u16 {
        self.0
    }
    /// This weight as the axis setting [`text_run`] takes.
    pub fn axis(self) -> Axis<'static> {
        ("wght", f32::from(self.0))
    }
}
impl Default for Weight {
    fn default() -> Self {
        Self::REGULAR
    }
}

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
    let font = FontRef::new(font).map_err(Error::Font)?;
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

/// The face's normalized coordinates for an axis setting, one per axis it
/// declares, in the font's own order.
///
/// A renderer that draws cached glyph outlines instead of the path
/// [`text_run`] hands back needs these, or it paints the default instance
/// while layout measured the varied one. The numbers are F2Dot14 bits --
/// what every glyph cache keys its variations on.
///
/// ```
/// # let font = epaint_default_fonts::HACK_REGULAR;
/// // A static face declares no axes, so there is nothing to vary.
/// assert!(mui_text::normalized_coords(font, &[mui_text::Weight::BOLD.axis()])
///     .unwrap()
///     .is_empty());
/// ```
pub fn normalized_coords(font: &[u8], axes: &[Axis<'_>]) -> Result<Vec<i16>, Error> {
    let font = FontRef::new(font).map_err(Error::Font)?;
    Ok(font
        .axes()
        .location(axes.iter().copied())
        .coords()
        .iter()
        .map(|c| c.to_bits())
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
    let font = FontRef::new(font).map_err(Error::Font)?;
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
        dy: 0.,
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
    /// The shaped x/y offsets for the same glyphs. The y component is
    /// non-zero for marks and other GPOS placements; a glyph cache renderer
    /// must use it or its output will disagree with [`Self::path`].
    pub glyph_offsets: Vec<(f64, f64)>,
}

/// A glyph in a [`FallbackTextRun`]. The font index is part of the glyph
/// identity: glyph id `42` in a fallback face is not glyph id `42` in the
/// primary face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FallbackGlyph {
    pub font: usize,
    pub glyph: u32,
    pub x: f64,
    pub y: f64,
    pub advance: f64,
    pub cluster: usize,
}

/// A shaped run built from a primary face followed by fallback faces.
///
/// [`TextRun`] remains the cheap single-face API used by the current scene
/// renderer. This type carries the face for every glyph so a renderer can
/// safely draw fallback glyphs; treating its `glyph` values as if they all
/// belonged to the first face is incorrect.
#[derive(Debug, Clone, PartialEq)]
pub struct FallbackTextRun {
    pub path: Path,
    pub glyphs: Vec<FallbackGlyph>,
    pub advance: f64,
    pub ascent: f64,
    pub descent: f64,
    pub line_height: f64,
}

#[derive(Debug, Clone, Copy)]
struct ShapedGlyph {
    glyph_id: GlyphId,
    x: f64,
    y: f64,
    advance: f64,
    cluster: usize,
    rtl: bool,
}

#[derive(Debug, Default)]
struct ShapedText {
    glyphs: Vec<ShapedGlyph>,
    advance: f64,
}

/// Lay `text` out as one path, using OpenType shaping and the Unicode bidi
/// algorithm to determine glyph order and pen positions.
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
    font_bytes: &[u8],
    text: &str,
    size_px: f64,
    axes: &[Axis<'_>],
    tolerance: f64,
) -> Result<TextRun, Error> {
    let size = checked_size(size_px)?;
    if !tolerance.is_finite() || tolerance <= 0. {
        return Err(Error::InvalidOptions("tolerance"));
    }
    let font = FontRef::new(font_bytes).map_err(Error::Font)?;
    let font_size = Size::new(size);
    let location = font.axes().location(axes.iter().copied());
    let outlines = font.outline_glyphs();
    let shaped = shape_text(font_bytes, text, size_px, axes)?;

    let mut pen = PathPen {
        commands: Vec::new(),
        cursor: Point::new(0., 0.),
        open: false,
        tolerance,
        dx: 0.,
        dy: 0.,
    };
    let mut glyphs = Vec::with_capacity(shaped.glyphs.len());
    let mut glyph_offsets = Vec::with_capacity(shaped.glyphs.len());
    for glyph in &shaped.glyphs {
        glyphs.push((glyph.glyph_id.to_u32(), glyph.x));
        glyph_offsets.push((glyph.x, glyph.y));
        if outlines.get(glyph.glyph_id).is_some() {
            pen.dx = glyph.x;
            pen.dy = glyph.y;
            draw_glyph(&font, glyph.glyph_id, size, &location, &mut pen)?;
        }
        pen.close_open_contour();
    }
    let advance = checked_finite(shaped.advance, "font metrics")?;

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
        glyph_offsets,
    })
}

/// Shape a run with `fonts[0]` as the primary face and subsequent faces as
/// fallback. Fallback selection is per grapheme cluster, so a base character
/// and its combining marks stay in one face and one shaping call.
pub fn fallback_text_run(
    fonts: &[&[u8]],
    text: &str,
    size_px: f64,
    axes: &[Axis<'_>],
    tolerance: f64,
) -> Result<FallbackTextRun, Error> {
    let size = checked_size(size_px)?;
    if fonts.is_empty() {
        return Err(Error::InvalidOptions("fonts"));
    }
    if !tolerance.is_finite() || tolerance <= 0. {
        return Err(Error::InvalidOptions("tolerance"));
    }
    let faces = fonts
        .iter()
        .map(|font| shape_face(font, axes))
        .collect::<Result<Vec<_>, _>>()?;
    let font_refs = fonts
        .iter()
        .map(|font| FontRef::new(font).map_err(Error::Font))
        .collect::<Result<Vec<_>, _>>()?;
    let metrics: Vec<_> = font_refs
        .iter()
        .map(|font| {
            let size = Size::new(size);
            let location = font.axes().location(axes.iter().copied());
            font.metrics(size, LocationRef::from(&location))
        })
        .collect();
    let ascent = metrics
        .iter()
        .map(|m| checked_metric(m.ascent, "font metrics"))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .fold(0., f64::max);
    let descent = metrics
        .iter()
        .map(|m| checked_metric(-m.descent, "font metrics"))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .fold(0., f64::max);
    let line_height = metrics
        .iter()
        .map(|m| checked_metric(m.ascent - m.descent + m.leading, "font metrics"))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .fold(0., f64::max);

    let mut pen = PathPen {
        commands: Vec::new(),
        cursor: Point::new(0., 0.),
        open: false,
        tolerance,
        dx: 0.,
        dy: 0.,
    };
    let mut glyphs = Vec::new();
    let mut advance = 0.;
    for (range, direction) in visual_segments(text) {
        let mut chunks = fallback_chunks(&font_refs, text, range);
        if direction == Direction::RightToLeft {
            chunks.reverse();
        }
        for (chunk, font_index) in chunks {
            let mut shaped = shape_segment(
                &faces[font_index],
                &text[chunk.clone()],
                chunk.start,
                direction,
                size_px,
            )?;
            for glyph in &mut shaped.glyphs {
                glyph.x += advance;
                glyphs.push(FallbackGlyph {
                    font: font_index,
                    glyph: glyph.glyph_id.to_u32(),
                    x: glyph.x,
                    y: glyph.y,
                    advance: glyph.advance,
                    cluster: glyph.cluster,
                });
                if font_refs[font_index]
                    .outline_glyphs()
                    .get(glyph.glyph_id)
                    .is_some()
                {
                    let location = font_refs[font_index].axes().location(axes.iter().copied());
                    pen.dx = glyph.x;
                    pen.dy = glyph.y;
                    draw_glyph(
                        &font_refs[font_index],
                        glyph.glyph_id,
                        size,
                        &location,
                        &mut pen,
                    )?;
                }
                pen.close_open_contour();
            }
            advance += shaped.advance;
        }
    }
    let path = pen.finish();
    path.validate(usize::MAX)?;
    Ok(FallbackTextRun {
        path,
        glyphs,
        advance: checked_finite(advance, "font metrics")?,
        ascent,
        descent,
        line_height,
    })
}

fn fallback_chunks(
    fonts: &[FontRef<'_>],
    text: &str,
    range: std::ops::Range<usize>,
) -> Vec<(std::ops::Range<usize>, usize)> {
    let mut chunks = Vec::new();
    let mut current = None;
    let mut start = range.start;
    for (offset, grapheme) in text[range.clone()].grapheme_indices(true) {
        let at = range.start + offset;
        let font = fonts
            .iter()
            .position(|font| grapheme.chars().all(|ch| font.charmap().map(ch).is_some()))
            .unwrap_or(0);
        if current.is_some_and(|index| index != font) {
            chunks.push((start..at, current.unwrap()));
            start = at;
        }
        current = Some(font);
    }
    if let Some(font) = current {
        chunks.push((start..range.end, font));
    }
    chunks
}

fn variations(axes: &[Axis<'_>]) -> Vec<rustybuzz::Variation> {
    axes.iter()
        .filter_map(|&(tag, value)| {
            let tag: [u8; 4] = tag.as_bytes().try_into().ok()?;
            Some(rustybuzz::Variation {
                tag: rustybuzz::ttf_parser::Tag::from_bytes(&tag),
                value,
            })
        })
        .collect()
}

fn shape_face<'a>(font: &'a [u8], axes: &[Axis<'_>]) -> Result<rustybuzz::Face<'a>, Error> {
    let mut face = rustybuzz::Face::from_slice(font, 0).ok_or(Error::InvalidOptions("font"))?;
    face.set_variations(&variations(axes));
    Ok(face)
}

fn shape_segment(
    face: &rustybuzz::Face<'_>,
    text: &str,
    byte_offset: usize,
    direction: Direction,
    size_px: f64,
) -> Result<ShapedText, Error> {
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.set_direction(direction);
    buffer.set_cluster_level(BufferClusterLevel::MonotoneGraphemes);
    buffer.set_flags(BufferFlags::BEGINNING_OF_TEXT | BufferFlags::END_OF_TEXT);
    let shaped = rustybuzz::shape(face, &[], buffer);
    let scale = size_px / f64::from(face.units_per_em());
    let mut pen = 0.;
    let mut glyphs = Vec::with_capacity(shaped.len());
    for (info, position) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
        let x_advance = f64::from(position.x_advance) * scale;
        glyphs.push(ShapedGlyph {
            glyph_id: GlyphId::new(info.glyph_id),
            x: pen + f64::from(position.x_offset) * scale,
            // HarfBuzz uses a y-up offset; MUI paths use y-down coordinates.
            y: -f64::from(position.y_offset) * scale,
            advance: x_advance,
            cluster: byte_offset + info.cluster as usize,
            rtl: direction == Direction::RightToLeft,
        });
        pen += x_advance;
    }
    Ok(ShapedText {
        glyphs,
        advance: pen,
    })
}

fn paragraph_line_range(text: &str, start: usize, end: usize) -> std::ops::Range<usize> {
    let mut end = end;
    while end > start {
        let ch = text[..end].chars().next_back().unwrap();
        if ch == '\n' || ch == '\r' {
            end -= ch.len_utf8();
        } else {
            break;
        }
    }
    start..end
}

fn visual_segments(text: &str) -> Vec<(std::ops::Range<usize>, Direction)> {
    let bidi = BidiInfo::new(text, None);
    let mut segments = Vec::new();
    for para in &bidi.paragraphs {
        let line = paragraph_line_range(text, para.range.start, para.range.end);
        if line.is_empty() {
            continue;
        }
        let (levels, runs) = bidi.visual_runs(para, line);
        segments.extend(runs.into_iter().map(|range| {
            let direction = if levels[range.start].is_rtl() {
                Direction::RightToLeft
            } else {
                Direction::LeftToRight
            };
            (range, direction)
        }));
    }
    segments
}

fn shape_text(
    font: &[u8],
    text: &str,
    size_px: f64,
    axes: &[Axis<'_>],
) -> Result<ShapedText, Error> {
    if text.is_empty() {
        return Ok(ShapedText::default());
    }
    let face = shape_face(font, axes)?;
    let mut out = ShapedText::default();
    for (range, direction) in visual_segments(text) {
        let mut segment =
            shape_segment(&face, &text[range.clone()], range.start, direction, size_px)?;
        for glyph in &mut segment.glyphs {
            glyph.x += out.advance;
        }
        out.advance += segment.advance;
        out.glyphs.extend(segment.glyphs);
    }
    Ok(out)
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
        .map_err(Error::Draw)?;
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
    dy: f64,
}

impl PathPen {
    /// Font space is y-up, the scene is y-down.
    fn point(&self, x: f32, y: f32) -> Point {
        Point::new(self.dx + x as f64, self.dy - y as f64)
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

#[derive(Debug, Clone, Copy)]
struct TextCluster {
    start: usize,
    end: usize,
    advance: f64,
    rtl: bool,
    left: f64,
    right: f64,
}

fn shaped_clusters(font: &[u8], text: &str, size_px: f64) -> Result<Vec<TextCluster>, Error> {
    shaped_clusters_with_axes(font, text, size_px, &[])
}

fn shaped_clusters_with_axes(
    font: &[u8],
    text: &str,
    size_px: f64,
    axes: &[Axis<'_>],
) -> Result<Vec<TextCluster>, Error> {
    let shaped = shape_text(font, text, size_px, axes)?;
    let mut clusters: Vec<TextCluster> = Vec::new();
    for glyph in shaped.glyphs {
        if let Some(cluster) = clusters
            .iter_mut()
            .find(|cluster| cluster.start == glyph.cluster)
        {
            cluster.advance += glyph.advance;
            cluster.left = cluster.left.min(glyph.x);
            cluster.right = cluster.right.max(glyph.x + glyph.advance);
        } else {
            clusters.push(TextCluster {
                start: glyph.cluster,
                end: text.len(),
                advance: glyph.advance,
                rtl: glyph.rtl,
                left: glyph.x,
                right: glyph.x + glyph.advance,
            });
        }
    }
    clusters.sort_unstable_by_key(|cluster| cluster.start);
    for i in 0..clusters.len().saturating_sub(1) {
        clusters[i].end = clusters[i + 1].start;
    }
    Ok(clusters)
}

/// One shaped advance per `char` of `text`, at the default variation position.
/// Glyph clusters keep ligatures and combining marks together. The advance is
/// assigned to the cluster's first source character for wrapping; caret queries
/// use [`caret_positions`] so they never enter a ligature or combining cluster.
fn advances_with_axes(
    font: &[u8],
    text: &str,
    size_px: f64,
    axes: &[Axis<'_>],
) -> Result<Vec<f64>, Error> {
    checked_size(size_px)?;
    let clusters = shaped_clusters_with_axes(font, text, size_px, axes)?;
    let starts: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    let mut out = vec![0.; starts.len()];
    if starts.is_empty() {
        return Ok(out);
    }
    for cluster in clusters {
        let start = cluster.start;
        let first = starts
            .partition_point(|&byte| byte < start)
            .min(out.len() - 1);
        out[first] += checked_finite(cluster.advance, "font metrics")?;
    }
    Ok(out)
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
/// Break opportunities come from UAX#14 (`unicode-linebreak`), so a space and a
/// hyphen break, a no-break space and an emoji ZWJ sequence do not, and CJK
/// breaks between ideographs; `'\n'` forces a break; a word wider than
/// `max_width` breaks at the glyph that overflows rather than hanging off the
/// edge.
///
pub fn break_lines(
    font: &[u8],
    text: &str,
    size_px: f64,
    max_width: f64,
) -> Result<Vec<Line>, Error> {
    break_lines_with_axes(font, text, size_px, &[], max_width)
}

/// [`break_lines`] at a variable-font axis location. Wrapping must use the
/// same instance that shaped the rendered line; otherwise a bold run can
/// cross its box even though the default instance fit during layout.
pub fn break_lines_with_axes(
    font: &[u8],
    text: &str,
    size_px: f64,
    axes: &[Axis<'_>],
    max_width: f64,
) -> Result<Vec<Line>, Error> {
    if !(max_width.is_finite() && max_width > 0.) {
        return Err(Error::InvalidOptions("max_width"));
    }
    let advances = advances_with_axes(font, text, size_px, axes)?;
    let cluster_starts: Vec<usize> = shaped_clusters_with_axes(font, text, size_px, axes)?
        .into_iter()
        .map(|cluster| cluster.start)
        .collect();
    break_lines_from_advances(text, &advances, &cluster_starts, max_width)
}

fn break_lines_from_advances(
    text: &str,
    advances: &[f64],
    cluster_starts: &[usize],
    max_width: f64,
) -> Result<Vec<Line>, Error> {
    if advances.len() != text.chars().count() {
        return Err(Error::InvalidOptions("advances"));
    }
    // Byte offsets a line may start at, ascending, walked alongside the chars.
    // One at a skipped space (LB8 after a ZWSP) is dropped; the next ink char
    // offers it again.
    let mut opps = unicode_linebreak::linebreaks(text)
        .filter(|&(_, o)| o == unicode_linebreak::BreakOpportunity::Allowed)
        .map(|(i, _)| i)
        .peekable();
    let mut lines = Vec::new();
    let mut start = 0;
    // Advance since `start`, trailing-whitespace part of it, and the width of
    // the word since the last break opportunity.
    let (mut x, mut trim, mut word) = (0., 0., 0.);
    let mut brk: Option<(usize, f64)> = None;

    for ((i, ch), &a) in text.char_indices().zip(advances.iter()) {
        if ch == '\n' {
            lines.push(Line {
                text_range: start..i,
                advance: x - trim,
            });
            start = i + 1;
            (x, trim, word, brk) = (0., 0., 0., None);
            continue;
        }
        let at_cluster_start = cluster_starts
            .partition_point(|&start| start <= i)
            .checked_sub(1)
            .is_none_or(|index| cluster_starts[index] == i);
        if ch.is_ascii_whitespace() {
            // Trailing space always fits: it costs nothing at the line end.
            x += a;
            trim += a;
            continue;
        }
        while opps.peek().is_some_and(|&bi| bi < i) {
            opps.next();
        }
        if at_cluster_start && opps.peek() == Some(&i) {
            // A line may start here; any spaces before it stay on this one.
            brk = Some((i, x - trim));
            word = 0.;
        }
        trim = 0.;
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
    }
    lines.push(Line {
        text_range: start..text.len(),
        advance: x - trim,
    });
    Ok(lines)
}

/// Greedy UAX#14 line breaking using the same fallback faces and variation
/// axes as [`fallback_text_run`]. A grapheme cluster remains indivisible even
/// when its glyph comes from a fallback face.
pub fn fallback_break_lines(
    fonts: &[&[u8]],
    text: &str,
    size_px: f64,
    axes: &[Axis<'_>],
    max_width: f64,
) -> Result<Vec<Line>, Error> {
    if !(max_width.is_finite() && max_width > 0.) {
        return Err(Error::InvalidOptions("max_width"));
    }
    let run = fallback_text_run(fonts, text, size_px, axes, 0.05)?;
    let starts: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    let mut advances = vec![0.; starts.len()];
    let mut clusters = Vec::new();
    for glyph in &run.glyphs {
        if let Some((_, advance)) = clusters
            .iter_mut()
            .find(|(start, _)| *start == glyph.cluster)
        {
            *advance += glyph.advance;
        } else {
            clusters.push((glyph.cluster, glyph.advance));
        }
    }
    for (start, advance) in clusters {
        let Some(first) = starts.iter().position(|&byte| byte == start) else {
            continue;
        };
        advances[first] += advance;
    }
    let mut cluster_starts = run
        .glyphs
        .iter()
        .map(|glyph| glyph.cluster)
        .collect::<Vec<_>>();
    cluster_starts.sort_unstable();
    cluster_starts.dedup();
    break_lines_from_advances(text, &advances, &cluster_starts, max_width)
}

fn caret_positions(font: &[u8], text: &str, size_px: f64) -> Result<Vec<(usize, f64)>, Error> {
    caret_positions_from_clusters(text, shaped_clusters(font, text, size_px)?)
}

fn caret_positions_from_clusters(
    text: &str,
    clusters: Vec<TextCluster>,
) -> Result<Vec<(usize, f64)>, Error> {
    let boundaries: Vec<usize> = text
        .char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(text.len()))
        .collect();
    let mut positions = vec![f64::NAN; boundaries.len()];
    for cluster in clusters {
        let first = boundaries
            .partition_point(|&byte| byte < cluster.start)
            .min(boundaries.len() - 1);
        let last = boundaries
            .partition_point(|&byte| byte < cluster.end)
            .min(boundaries.len() - 1);
        let leading = if cluster.rtl {
            cluster.right
        } else {
            cluster.left
        };
        let trailing = if cluster.rtl {
            cluster.left
        } else {
            cluster.right
        };
        for position in first..=last {
            positions[position] = if boundaries[position] == cluster.end {
                trailing
            } else {
                leading
            };
        }
    }
    let mut previous = 0.;
    for position in &mut positions {
        if position.is_finite() {
            previous = *position;
        } else {
            *position = previous;
        }
    }
    Ok(boundaries.into_iter().zip(positions).collect())
}

/// Pen x of the caret sitting *before* the char at `byte_index`, which must be
/// a char boundary. `text.len()` is the caret at the end.
pub fn caret_x(font: &[u8], text: &str, size_px: f64, byte_index: usize) -> Result<f64, Error> {
    if byte_index > text.len() || !text.is_char_boundary(byte_index) {
        return Err(Error::InvalidOptions("byte_index"));
    }
    let positions = caret_positions(font, text, size_px)?;
    Ok(positions
        .into_iter()
        .find_map(|(byte, x)| (byte == byte_index).then_some(x))
        .unwrap_or(0.))
}

/// The char boundary whose caret is nearest `x`. The inverse of [`caret_x`],
/// which is what a click in a text field needs.
pub fn hit_index(font: &[u8], text: &str, size_px: f64, x: f64) -> Result<usize, Error> {
    let x = checked_finite(x, "x")?;
    let positions = caret_positions(font, text, size_px)?;
    let (best, _) =
        positions
            .into_iter()
            .fold((0, x.abs()), |(best, best_distance), (byte, position)| {
                let distance = (position - x).abs();
                if distance < best_distance {
                    (byte, distance)
                } else {
                    (best, best_distance)
                }
            });
    Ok(best)
}

fn fallback_caret_positions(
    fonts: &[&[u8]],
    text: &str,
    size_px: f64,
) -> Result<Vec<(usize, f64)>, Error> {
    let run = fallback_text_run(fonts, text, size_px, &[], 0.05)?;
    let mut rtl = HashMap::new();
    for (range, direction) in visual_segments(text) {
        for (offset, _) in text[range.clone()].char_indices() {
            rtl.insert(range.start + offset, direction == Direction::RightToLeft);
        }
    }
    let mut by_cluster: HashMap<usize, TextCluster> = HashMap::new();
    for glyph in run.glyphs {
        let entry = by_cluster.entry(glyph.cluster).or_insert(TextCluster {
            start: glyph.cluster,
            end: text.len(),
            advance: 0.,
            rtl: rtl.get(&glyph.cluster).copied().unwrap_or(false),
            left: glyph.x,
            right: glyph.x,
        });
        entry.advance += glyph.advance;
        entry.left = entry.left.min(glyph.x);
        entry.right = entry.right.max(glyph.x + glyph.advance);
    }
    let mut clusters: Vec<TextCluster> = by_cluster.into_values().collect();
    clusters.sort_unstable_by_key(|cluster| cluster.start);
    for i in 0..clusters.len().saturating_sub(1) {
        clusters[i].end = clusters[i + 1].start;
    }
    caret_positions_from_clusters(text, clusters)
}

/// Pen x of a caret in a run that can use fallback faces.
pub fn fallback_caret_x(
    fonts: &[&[u8]],
    text: &str,
    size_px: f64,
    byte_index: usize,
) -> Result<f64, Error> {
    if byte_index > text.len() || !text.is_char_boundary(byte_index) {
        return Err(Error::InvalidOptions("byte_index"));
    }
    Ok(fallback_caret_positions(fonts, text, size_px)?
        .into_iter()
        .find_map(|(byte, x)| (byte == byte_index).then_some(x))
        .unwrap_or(0.))
}

/// The char boundary whose caret is nearest `x` in a fallback run.
pub fn fallback_hit_index(
    fonts: &[&[u8]],
    text: &str,
    size_px: f64,
    x: f64,
) -> Result<usize, Error> {
    let x = checked_finite(x, "x")?;
    let (best, _) = fallback_caret_positions(fonts, text, size_px)?
        .into_iter()
        .fold((0, x.abs()), |(best, best_distance), (byte, position)| {
            let distance = (position - x).abs();
            if distance < best_distance {
                (byte, distance)
            } else {
                (best, best_distance)
            }
        });
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

    #[test]
    fn open_type_shaping_applies_kerning_and_combining_substitution() {
        let pair = text_run(ttf_inter::REGULAR, "AV", 32., &[], 0.05).unwrap();
        let separate = text_run(ttf_inter::REGULAR, "A", 32., &[], 0.05)
            .unwrap()
            .advance
            + text_run(ttf_inter::REGULAR, "V", 32., &[], 0.05)
                .unwrap()
                .advance;
        assert!(pair.advance < separate, "kerning was not applied: {pair:?}");

        let composed = text_run(ttf_inter::REGULAR, "A\u{301}", 32., &[], 0.05).unwrap();
        assert_eq!(composed.glyphs.len(), 1, "the mark was not composed");
    }

    #[test]
    fn combining_marks_keep_their_position_offsets_for_renderers() {
        // Inter keeps the Hebrew niqqud as a separate mark and positions it
        // with GPOS, which exercises the renderer's y-offset transport.
        let run = text_run(ttf_inter::REGULAR, "ש\u{05b8}", 32., &[], 0.05).unwrap();
        assert!(
            run.glyph_offsets.iter().any(|&(_, y)| y.abs() > 0.01),
            "GPOS mark placement was discarded: {:?}",
            run.glyph_offsets
        );
    }

    #[test]
    fn variable_weight_reaches_the_shaper_and_outline() {
        let regular = text_run(
            ttf_inter::REGULAR,
            "KURV",
            24.,
            &[Weight::REGULAR.axis()],
            0.05,
        )
        .unwrap();
        let bold = text_run(
            ttf_inter::REGULAR,
            "KURV",
            24.,
            &[Weight::BOLD.axis()],
            0.05,
        )
        .unwrap();
        assert_ne!(regular.path, bold.path, "the wght axis was ignored");
    }

    #[test]
    fn bidi_reorders_a_rtl_run_in_visual_order() {
        let shaped = shape_text(HACK_REGULAR, "ab \u{05D0}\u{05D1}\u{05D2} cd", 16., &[]).unwrap();
        let clusters: Vec<usize> = shaped.glyphs.iter().map(|glyph| glyph.cluster).collect();
        assert!(
            clusters.windows(3).any(|window| window == [7, 5, 3]),
            "RTL glyphs were not visually reordered: {clusters:?}"
        );
    }

    #[test]
    fn wrapping_does_not_split_a_combining_cluster() {
        let text = "A\u{301}B";
        let first = text_run(ttf_inter::REGULAR, "A\u{301}", 16., &[], 0.05)
            .unwrap()
            .advance;
        let lines = break_lines(ttf_inter::REGULAR, text, 16., first);
        assert_eq!(
            lines
                .unwrap()
                .into_iter()
                .map(|line| &text[line.text_range])
                .collect::<Vec<_>>(),
            ["A\u{301}", "B"]
        );
    }

    #[test]
    fn fallback_selects_a_face_per_grapheme_cluster() {
        let run = fallback_text_run(
            &[HACK_REGULAR, epaint_default_fonts::NOTO_EMOJI_REGULAR],
            "A😀",
            24.,
            &[],
            0.05,
        )
        .unwrap();
        assert_eq!(run.glyphs[0].font, 0);
        assert_eq!(run.glyphs.last().unwrap().font, 1);
        assert!(run.advance > 0.);
    }

    #[test]
    fn fallback_caret_round_trips_across_a_missing_glyph() {
        let fonts = [HACK_REGULAR, epaint_default_fonts::NOTO_EMOJI_REGULAR];
        let text = "A😀";
        let end = fallback_caret_x(&fonts, text, 24., text.len()).unwrap();
        assert_eq!(
            fallback_hit_index(&fonts, text, 24., end).unwrap(),
            text.len()
        );
        let emoji = text.char_indices().nth(1).unwrap().0;
        let before_emoji = fallback_caret_x(&fonts, text, 24., emoji).unwrap();
        assert!(end > before_emoji, "fallback glyph has no advance");
    }

    #[test]
    fn fallback_wrap_breaks_before_a_glyph_cluster() {
        let fonts = [HACK_REGULAR, epaint_default_fonts::NOTO_EMOJI_REGULAR];
        let text = "A😀B";
        let emoji_end = text.char_indices().nth(2).unwrap().0;
        let first_two = fallback_text_run(&fonts, &text[..emoji_end], 24., &[], 0.05)
            .unwrap()
            .advance;
        let lines = fallback_break_lines(&fonts, text, 24., &[], first_two + 0.1).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text_range, 0..emoji_end);
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
    fn uax14_says_where_a_line_may_start() {
        // Width of the first `n` bytes of the text itself, so the font's own
        // advances decide and no test hard-codes a pixel.
        let w = |t: &str, n: usize| caret_x(HACK_REGULAR, t, SIZE, n).unwrap();

        // A no-break space holds its word together; the ASCII space breaks.
        let nbsp = "a\u{00A0}b c";
        assert_eq!(lines(nbsp, w(nbsp, 4)), ["a\u{00A0}b ", "c"]);
        // A hyphen still breaks, after it.
        assert_eq!(lines("ab-cd", w("ab-cd", 3)), ["ab-", "cd"]);
        // CJK breaks between ideographs with no space in sight.
        let cjk = "\u{4E00}\u{4E8C}\u{4E09}";
        assert_eq!(lines(cjk, w(cjk, 6)), ["\u{4E00}\u{4E8C}", "\u{4E09}"]);
        // An emoji ZWJ sequence is one unit: the break lands on the space.
        let emoji = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F466}";
        let text = format!("x {emoji}");
        let width = w(&text, text.len()) - w(&text, 1);
        assert_eq!(lines(&text, width), ["x ", emoji]);
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

    #[test]
    fn a_bad_font_keeps_its_skrifa_cause() {
        let Err(e) = axes(b"not a font") else {
            panic!("bad bytes must not parse");
        };
        assert!(matches!(e, Error::Font(_)));
        assert!(std::error::Error::source(&e).is_some());
    }

    #[test]
    fn a_non_finite_click_does_not_snap_to_the_start() {
        for x in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            assert!(hit_index(HACK_REGULAR, "abc", SIZE, x).is_err(), "{x}");
        }
        assert_eq!(hit_index(HACK_REGULAR, "abc", SIZE, 1e6).unwrap(), 3);
    }
}
