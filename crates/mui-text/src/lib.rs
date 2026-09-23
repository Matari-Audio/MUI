//! Glyph outlines as MUI geometry.
//!
//! A glyph here is not a texture — it is a [`mui_geometry::Path`] in the same
//! coordinate space as every other surface, so it flattens, tessellates and
//! (once contour winding is classified) composes with the Boolean surface
//! system like any other shape. [`text_run`] lays a whole string out the same
//! way, as one path; there is still no atlas anywhere.
//!
//! Variable-font axes are an argument rather than a font variant: the outline
//! is re-derived at whatever axis position is asked for, so Material Symbols
//! morph from unfilled to filled as `FILL` 0 -> 1. The geometry path here has
//! no cache at all; `mui-vello` rasterises the same outlines through a glyph
//! cache keyed on the normalized axis coordinates, one entry per position.
#![forbid(unsafe_code)]

use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use harfrust::{
    BufferClusterLevel, BufferFlags, Direction, ShapeOptions, ShapePlan, ShapePlanKey, ShaperData,
    ShaperInstance, UnicodeBuffer,
};
use mui_geometry::{Path, PathCommand, Point};
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

/// An owned axis setting list: what an element stores and a cache keys on.
///
/// Tags are kept sorted and values as `f32` bits, so the same settings in any
/// order compare and hash equal. Setting a tag again replaces it. A tag that
/// is not four ASCII bytes, or a value that is not finite, is ignored: no
/// font can declare the one and no outline exists at the other.
///
/// Always set the same tags every frame of an animation: an omitted axis
/// sits at its default, so `FILL` present then absent is a jump to 0.
///
/// ```
/// use mui_text::{Axes, Weight};
/// let a = Axes::new().with("wght", 700.).with("FILL", 1.);
/// let b = Axes::from(Weight::BOLD).with("FILL", 1.);
/// assert_eq!(a, b);
/// assert_eq!(a.get("FILL"), Some(1.));
/// assert_eq!(a.to_vec(), [("FILL", 1.), ("wght", 700.)]);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Axes(Vec<([u8; 4], u32)>);
impl Axes {
    pub const fn new() -> Self {
        Self(Vec::new())
    }
    pub fn with(mut self, tag: &str, value: f32) -> Self {
        self.set(tag, value);
        self
    }
    pub fn set(&mut self, tag: &str, value: f32) {
        let Ok(tag) = <[u8; 4]>::try_from(tag.as_bytes()) else {
            return;
        };
        if !tag.is_ascii() || !value.is_finite() {
            return;
        }
        match self.0.binary_search_by_key(&tag, |a| a.0) {
            Ok(i) => self.0[i].1 = value.to_bits(),
            Err(i) => self.0.insert(i, (tag, value.to_bits())),
        }
    }
    pub fn get(&self, tag: &str) -> Option<f32> {
        let tag = <[u8; 4]>::try_from(tag.as_bytes()).ok()?;
        self.0
            .binary_search_by_key(&tag, |a| a.0)
            .ok()
            .map(|i| f32::from_bits(self.0[i].1))
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = Axis<'_>> + '_ {
        // ASCII was checked on the way in, so this never fails.
        self.0
            .iter()
            .map(|(t, v)| (std::str::from_utf8(t).unwrap_or(""), f32::from_bits(*v)))
    }
    /// The borrowed form every measuring function here takes.
    pub fn to_vec(&self) -> Vec<Axis<'_>> {
        self.iter().collect()
    }
}
impl From<Weight> for Axes {
    fn from(w: Weight) -> Self {
        Self::new().with("wght", f32::from(w.value()))
    }
}

/// A font face: its bytes, an identity, and what shaping it costs to set up,
/// built on first use and shared by every clone.
///
/// Equality and hashing are by identity, never bytes: every [`Font::new`] is a
/// new face. A cache keyed on one cannot serve glyphs from a buffer that was
/// freed and reallocated at the same address -- and a `Font` built afresh
/// every frame misses every such cache, so build one per face and clone it,
/// which is an `Arc` bump.
///
/// ```
/// use mui_text::Font;
/// let hack = Font::new(epaint_default_fonts::HACK_REGULAR).unwrap();
/// assert_eq!(hack, hack.clone());
/// assert_ne!(hack, Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
/// assert!(Font::new(&b"not a font"[..]).is_err());
/// ```
#[derive(Clone)]
pub struct Font(Arc<FontData>);

struct FontData {
    id: u64,
    bytes: Arc<[u8]>,
    /// harfrust's lookup accelerators for the face: the expensive part of
    /// setting a shaper up, and the same at every size and axis position.
    shaper: OnceLock<ShaperData>,
    /// Compiled feature maps, one per (script, direction, feature-variation)
    /// the face has shaped. Reusing one took Inter caret_x from 18.3 to 15.9 us.
    // ponytail: never evicted; bounded by the scripts and FeatureVariations
    // records a face declares, not by the text shaped with it.
    plans: Mutex<Vec<Arc<ShapePlan>>>,
}

impl Font {
    /// Parse `bytes` as one font face. Bytes no parser accepts are an error
    /// here, once, rather than on every run shaped with them.
    pub fn new(bytes: impl Into<Arc<[u8]>>) -> Result<Self, Error> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let bytes = bytes.into();
        FontRef::new(&bytes).map_err(Error::Font)?;
        Ok(Self(Arc::new(FontData {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            bytes,
            shaper: OnceLock::new(),
            plans: Mutex::new(Vec::new()),
        })))
    }
    /// Unique to one [`Font::new`] for the life of the process and shared by
    /// its clones: the key for any cache of what this face draws.
    pub fn id(&self) -> u64 {
        self.0.id
    }
    fn font_ref(&self) -> Result<FontRef<'_>, Error> {
        // Validated in `new`; the table directory is all this re-reads.
        FontRef::new(&self.0.bytes).map_err(Error::Font)
    }
}
impl AsRef<[u8]> for Font {
    fn as_ref(&self) -> &[u8] {
        &self.0.bytes
    }
}
impl PartialEq for Font {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}
impl Eq for Font {}
impl Hash for Font {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.id.hash(state);
    }
}
impl std::fmt::Debug for Font {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Font")
            .field("id", &self.0.id)
            .field("len", &self.0.bytes.len())
            .finish()
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
    /// fvar `HIDDEN_AXIS`: the font asks UIs not to expose this axis directly
    /// (an `opsz` it sets itself, an internal `XTRA`). Still settable.
    pub hidden: bool,
}

/// The variation axes of a face, in the font's own order. Empty for a static
/// font -- which is the honest answer, not an error.
pub fn axes(font: &Font) -> Vec<AxisInfo> {
    let Ok(font) = font.font_ref() else {
        return Vec::new();
    };
    font.axes()
        .iter()
        .map(|a| AxisInfo {
            tag: a.tag().to_string(),
            min: a.min_value(),
            default: a.default_value(),
            max: a.max_value(),
            hidden: a.is_hidden(),
        })
        .collect()
}

/// The face's normalized coordinates for an axis setting, one per axis it
/// declares, in the font's own order.
///
/// A renderer that draws cached glyph outlines instead of the path
/// [`text_run`] hands back needs these, or it paints the default instance
/// while layout measured the varied one. The numbers are F2Dot14 bits --
/// what every glyph cache keys its variations on. `size_px` matters only to
/// a face with an `opsz` axis, which follows the em size unless set.
///
/// ```
/// # let font = mui_text::Font::new(epaint_default_fonts::HACK_REGULAR).unwrap();
/// // A static face declares no axes, so there is nothing to vary.
/// assert!(mui_text::normalized_coords(&font, 16., &[mui_text::Weight::BOLD.axis()])
///     .unwrap()
///     .is_empty());
/// ```
pub fn normalized_coords(font: &Font, size_px: f64, axes: &[Axis<'_>]) -> Result<Vec<i16>, Error> {
    let font = font.font_ref()?;
    Ok(location(&font, size_px, axes)
        .coords()
        .iter()
        .map(|c| c.to_bits())
        .collect())
}

/// The one place an axis setting becomes a position in the design space, so
/// outlines, metrics, shaping and a renderer's cache key all agree.
///
/// `opsz` follows the em size unless the caller sets it -- CSS
/// `font-optical-sizing: auto`, in CSS-pixel semantics; the face clamps it
/// to the range it declares, so nothing is asked for that does not exist.
///
/// Coordinates are then rounded to 1/128 of a half-axis. A spring never
/// lands on the same float twice, so without this every frame of a `FILL`
/// tween is a new glyph-cache instance; with it a hover in and out reuses
/// at most 128 per axis, and the extremes and the default round to
/// themselves. Nobody can see 1/128 of an axis.
fn location(font: &FontRef<'_>, size_px: f64, axes: &[Axis<'_>]) -> skrifa::instance::Location {
    let all = font.axes();
    let auto_opsz =
        all.get_by_tag(skrifa::Tag::new(b"opsz")).is_some() && !axes.iter().any(|a| a.0 == "opsz");
    let mut location = if auto_opsz {
        all.location(axes.iter().copied().chain([("opsz", size_px as f32)]))
    } else {
        all.location(axes.iter().copied())
    };
    for c in location.coords_mut() {
        *c = skrifa::instance::NormalizedCoord::from_bits(
            ((i32::from(c.to_bits()) + 64).div_euclid(128) * 128) as i16,
        );
    }
    location
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
    font: &Font,
    ch: char,
    size_px: f64,
    axes: &[Axis<'_>],
    tolerance: f64,
) -> Result<Path, Error> {
    let size = checked_size(size_px)?;
    let mut pen = PathPen::new(tolerance)?;
    let font = font.font_ref()?;
    let glyph_id = font.charmap().map(ch).ok_or(Error::MissingGlyph(ch))?;
    let outlines = font.outline_glyphs();
    let outline = outlines.get(glyph_id).ok_or(Error::NoOutline(ch))?;
    let location = location(&font, size_px, axes);
    outline
        .draw(
            DrawSettings::unhinted(Size::new(size), LocationRef::from(&location)),
            &mut pen,
        )
        .map_err(Error::Draw)?;
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
/// space; flipping it here is why this is a struct and not a tuple. With
/// fallback faces each metric is the largest any face reports.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    /// Every glyph's contours, each already translated to its pen position.
    /// Fill this **non-zero** -- see [`text_run`].
    pub path: Path,
    /// Every glyph with its face and pen position, for a renderer with its
    /// own glyph cache and hinting; `path` is the same ink as plain geometry.
    pub glyphs: Vec<Glyph>,
    /// Total pen advance: where the next run would start, not the ink extent.
    /// A trailing space advances and draws nothing.
    pub advance: f64,
    pub ascent: f64,
    pub descent: f64,
    /// The face's own idea of a line pitch, leading included.
    pub line_height: f64,
}

/// One glyph of a [`TextRun`]. `font` indexes the faces the run was shaped
/// with, and is part of the glyph's identity: glyph id `42` in a fallback face
/// is not glyph id `42` in the primary face.
///
/// `x` and `y` are the pen position plus the shaped offset. `y` is non-zero
/// for marks and other GPOS placements; a glyph-cache renderer must use it or
/// its output will disagree with [`TextRun::path`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph {
    pub font: usize,
    pub id: u32,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy)]
struct ShapedGlyph {
    font: usize,
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

/// A face opened for one call: parsed, placed in its design space at the
/// call's size and axes, and ready to shape. Omitted axes sit at their
/// defaults, so the same tag set always means the same instance.
struct Face<'a> {
    font: FontRef<'a>,
    data: &'a ShaperData,
    plans: &'a Mutex<Vec<Arc<ShapePlan>>>,
    location: skrifa::instance::Location,
    instance: ShaperInstance,
}

/// Open `fonts` -- the primary face, then its fallbacks -- at one size and
/// axis setting. A single face is simply a fallback chain of one.
fn open<'a>(fonts: &'a [Font], size_px: f64, axes: &[Axis<'_>]) -> Result<Vec<Face<'a>>, Error> {
    checked_size(size_px)?;
    if fonts.is_empty() {
        return Err(Error::InvalidOptions("fonts"));
    }
    fonts
        .iter()
        .map(|f| {
            let font = f.font_ref()?;
            let data = f.0.shaper.get_or_init(|| ShaperData::new(&font));
            let location = location(&font, size_px, axes);
            // The same normalized coordinates the outlines are drawn at, so
            // the shaper's advances and FeatureVariations never disagree
            // with the ink.
            let instance = ShaperInstance::from_coords(&font, location.coords().iter().copied());
            Ok(Face {
                font,
                data,
                plans: &f.0.plans,
                location,
                instance,
            })
        })
        .collect()
}

/// Lay `text` out as one path, using OpenType shaping and the Unicode bidi
/// algorithm to determine glyph order and pen positions.
///
/// `fonts[0]` is the primary face and the rest are fallback, chosen per
/// grapheme cluster so a base character and its combining marks stay in one
/// face and one shaping call.
///
/// **Fill the result non-zero.** Two glyphs whose ink overlaps -- an italic
/// `f`, a negative sidebearing -- would XOR each other's overlap away under
/// even-odd, while counters stay empty under both rules because a typeface
/// reverses them. Non-zero is also what [`mui_input::Hit`] tests with, so what
/// you can click is what you can see.
///
/// A character no face has a glyph for draws the primary face's `.notdef`
/// rather than failing: one missing codepoint must not blank a whole label. A
/// character with no outline -- a space -- contributes its advance and no
/// contours.
///
/// [`mui_input::Hit`]: https://docs.rs/mui-input
pub fn text_run(
    fonts: &[Font],
    text: &str,
    size_px: f64,
    axes: &[Axis<'_>],
    tolerance: f64,
) -> Result<TextRun, Error> {
    let mut pen = PathPen::new(tolerance)?;
    let faces = open(fonts, size_px, axes)?;
    let size = Size::new(checked_size(size_px)?);
    let (mut ascent, mut descent, mut line_height) = (0f64, 0f64, 0f64);
    for face in &faces {
        let m = face.font.metrics(size, LocationRef::from(&face.location));
        ascent = ascent.max(checked_metric(m.ascent, "font metrics")?);
        // Negative in font space, positive below the baseline here.
        descent = descent.max(checked_metric(-m.descent, "font metrics")?);
        line_height = line_height.max(checked_metric(
            m.ascent - m.descent + m.leading,
            "font metrics",
        )?);
    }
    let shaped = shape(&faces, text, size_px);
    let outlines: Vec<_> = faces.iter().map(|f| f.font.outline_glyphs()).collect();
    let mut glyphs = Vec::with_capacity(shaped.glyphs.len());
    for g in &shaped.glyphs {
        glyphs.push(Glyph {
            font: g.font,
            id: g.glyph_id.to_u32(),
            x: g.x,
            y: g.y,
        });
        if let Some(outline) = outlines[g.font].get(g.glyph_id) {
            pen.dx = g.x;
            pen.dy = g.y;
            let location = LocationRef::from(&faces[g.font].location);
            outline
                .draw(DrawSettings::unhinted(size, location), &mut pen)
                .map_err(Error::Draw)?;
        }
        pen.close_open_contour();
    }
    let path = pen.finish();
    path.validate(usize::MAX)?;
    Ok(TextRun {
        path,
        glyphs,
        advance: checked_finite(shaped.advance, "font metrics")?,
        ascent,
        descent,
        line_height,
    })
}

/// Split one visual segment into runs of the first face that covers each
/// grapheme whole, appended to `chunks`. A grapheme no face covers stays with
/// the primary face and draws its `.notdef`.
fn fallback_chunks(
    charmaps: &[skrifa::charmap::Charmap<'_>],
    text: &str,
    range: std::ops::Range<usize>,
    chunks: &mut Vec<(std::ops::Range<usize>, usize)>,
) {
    let mut current = None;
    let mut start = range.start;
    for (offset, grapheme) in text[range.clone()].grapheme_indices(true) {
        let at = range.start + offset;
        let font = charmaps
            .iter()
            .position(|charmap| grapheme.chars().all(|ch| charmap.map(ch).is_some()))
            .unwrap_or(0);
        if let Some(previous) = current.filter(|&index| index != font) {
            chunks.push((start..at, previous));
            start = at;
        }
        current = Some(font);
    }
    if let Some(font) = current {
        chunks.push((start..range.end, font));
    }
}

/// Shape `text` in visual order: bidi segments, then per-grapheme fallback
/// chunks within each. Every glyph's x is its pen position from the run
/// start; outlines are never touched, so measuring costs only the shaping.
fn shape(faces: &[Face<'_>], text: &str, size_px: f64) -> ShapedText {
    let shapers: Vec<_> = faces
        .iter()
        .map(|f| f.data.shaper(&f.font).instance(Some(&f.instance)).build())
        .collect();
    // One face needs no coverage test: it is the fallback for everything.
    let charmaps: Vec<_> = if faces.len() > 1 {
        faces.iter().map(|f| f.font.charmap()).collect()
    } else {
        Vec::new()
    };
    let mut out = ShapedText::default();
    let mut chunks = Vec::new();
    let mut buffer = UnicodeBuffer::new();
    for (range, direction) in visual_segments(text) {
        if charmaps.is_empty() {
            chunks.push((range, 0));
        } else {
            fallback_chunks(&charmaps, text, range, &mut chunks);
        }
        let rtl = direction == Direction::RightToLeft;
        if rtl {
            chunks.reverse();
        }
        for (chunk, font) in chunks.drain(..) {
            buffer.push_str(&text[chunk.clone()]);
            buffer.set_direction(direction);
            buffer.set_cluster_level(BufferClusterLevel::MonotoneGraphemes);
            buffer.set_flags(BufferFlags::BEGINNING_OF_TEXT | BufferFlags::END_OF_TEXT);
            // Unlike rustybuzz, harfrust does not infer the script; without
            // it the shaper picks the default shaper and skips mark
            // positioning.
            buffer.guess_segment_properties();
            let shaper = &shapers[font];
            let plan = plan(&faces[font], shaper, &buffer);
            // Default feature set: HarfBuzz turns on rlig/rclt/calt/liga,
            // which is what FeatureVariations-driven swaps (Material Symbols
            // FILL) hang off.
            let shaped = shaper.shape(buffer, ShapeOptions::new().plan(Some(&plan)));
            let scale = size_px / f64::from(shaper.units_per_em());
            for (info, position) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
                let advance = f64::from(position.x_advance) * scale;
                out.glyphs.push(ShapedGlyph {
                    font,
                    glyph_id: GlyphId::new(info.glyph_id),
                    x: out.advance + f64::from(position.x_offset) * scale,
                    // HarfBuzz uses a y-up offset; MUI paths use y-down.
                    y: -f64::from(position.y_offset) * scale,
                    advance,
                    cluster: chunk.start + info.cluster as usize,
                    rtl,
                });
                out.advance += advance;
            }
            buffer = shaped.clear();
        }
    }
    out
}

/// The face's compiled plan for `buffer`'s script and direction at this
/// instance, compiled on first use: exactly what `shape` would build per call.
fn plan(face: &Face<'_>, shaper: &harfrust::Shaper<'_>, buffer: &UnicodeBuffer) -> Arc<ShapePlan> {
    // An unset script reads back as Unknown; the per-call plan sees `None`.
    let script = Some(buffer.script()).filter(|&s| s != harfrust::script::UNKNOWN);
    let direction = buffer.direction();
    let key = ShapePlanKey::new(script, direction).instance(Some(&face.instance));
    let mut plans = face.plans.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(plan) = plans.iter().find(|plan| key.matches(plan)) {
        return plan.clone();
    }
    let plan = Arc::new(ShapePlan::new(shaper, direction, script, None, &[]));
    plans.push(plan.clone());
    plan
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
    fn new(tolerance: f64) -> Result<Self, Error> {
        if !tolerance.is_finite() || tolerance <= 0. {
            return Err(Error::InvalidOptions("tolerance"));
        }
        Ok(Self {
            commands: Vec::new(),
            cursor: Point::new(0., 0.),
            open: false,
            tolerance,
            dx: 0.,
            dy: 0.,
        })
    }
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

/// The shaped glyphs grouped by source cluster, in logical order. Glyphs of
/// one cluster share its `start`, so sorting brings them together and one
/// pass merges them.
fn clusters(text: &str, shaped: &ShapedText) -> Vec<TextCluster> {
    let mut clusters: Vec<TextCluster> = shaped
        .glyphs
        .iter()
        .map(|g| TextCluster {
            start: g.cluster,
            end: text.len(),
            advance: g.advance,
            rtl: g.rtl,
            left: g.x,
            right: g.x + g.advance,
        })
        .collect();
    clusters.sort_by_key(|cluster| cluster.start);
    clusters.dedup_by(|next, kept| {
        let same = next.start == kept.start;
        if same {
            kept.advance += next.advance;
            kept.left = kept.left.min(next.left);
            kept.right = kept.right.max(next.right);
        }
        same
    });
    for i in 1..clusters.len() {
        clusters[i - 1].end = clusters[i].start;
    }
    clusters
}

/// One shaped advance per `char` of `text`, and whether a line may start at
/// it. Glyph clusters keep ligatures and combining marks together: the
/// cluster's advance goes to its first character and the rest get none.
fn char_advances(text: &str, clusters: &[TextCluster]) -> Result<Vec<(f64, bool)>, Error> {
    let mut clusters = clusters.iter().peekable();
    let mut seen = false;
    text.char_indices()
        .map(|(i, _)| {
            let (mut advance, mut starts) = (0., !seen);
            while let Some(cluster) = clusters.next_if(|c| c.start <= i) {
                advance += checked_finite(cluster.advance, "font metrics")?;
                starts = true;
                seen = true;
            }
            Ok((advance, starts))
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

/// Greedy line breaking on shaped advances, with the faces and axes the run
/// is drawn at: a bold run measured at the default instance would cross its
/// box.
///
/// Break opportunities come from UAX#14 (`unicode-linebreak`), so a space and a
/// hyphen break, a no-break space and an emoji ZWJ sequence do not, and CJK
/// breaks between ideographs; `'\n'` forces a break; a word wider than
/// `max_width` breaks at the glyph cluster that overflows rather than hanging
/// off the edge. A grapheme cluster stays whole even when it alone is wider
/// than the line.
pub fn break_lines(
    fonts: &[Font],
    text: &str,
    size_px: f64,
    axes: &[Axis<'_>],
    max_width: f64,
) -> Result<Vec<Line>, Error> {
    if !(max_width.is_finite() && max_width > 0.) {
        return Err(Error::InvalidOptions("max_width"));
    }
    let faces = open(fonts, size_px, axes)?;
    let clusters = clusters(text, &shape(&faces, text, size_px));
    Ok(break_lines_from_advances(
        text,
        &char_advances(text, &clusters)?,
        max_width,
    ))
}

fn break_lines_from_advances(text: &str, advances: &[(f64, bool)], max_width: f64) -> Vec<Line> {
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

    for ((i, ch), &(a, at_cluster_start)) in text.char_indices().zip(advances) {
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
        while opps.peek().is_some_and(|&bi| bi < i) {
            opps.next();
        }
        if at_cluster_start && opps.peek() == Some(&i) {
            // A line may start here; any spaces before it stay on this one.
            brk = Some((i, x - trim));
            word = 0.;
        }
        trim = 0.;
        // Only a cluster start can overflow: a mark inside a cluster wider
        // than the line rides with its base instead of starting a line.
        if at_cluster_start && x + a > max_width && i > start {
            match brk.filter(|&(bi, _)| bi > start) {
                Some((bi, advance)) => {
                    lines.push(Line {
                        text_range: start..bi,
                        advance,
                    });
                    start = bi;
                    x = word;
                }
                // The word itself does not fit: break at the overflowing
                // cluster.
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
    lines
}

/// Every char boundary of `text` with the pen x of a caret there, ascending
/// by byte. A caret never lands inside a ligature or combining cluster: every
/// boundary within one sits at its leading edge.
fn caret_positions(
    fonts: &[Font],
    text: &str,
    size_px: f64,
    axes: &[Axis<'_>],
) -> Result<Vec<(usize, f64)>, Error> {
    let faces = open(fonts, size_px, axes)?;
    let clusters = clusters(text, &shape(&faces, text, size_px));
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
        let (leading, trailing) = if cluster.rtl {
            (cluster.right, cluster.left)
        } else {
            (cluster.left, cluster.right)
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
/// a char boundary. `text.len()` is the caret at the end. `fonts` and `axes`
/// must match what the run is drawn with: a variable face advances
/// differently at `wght` 700 than at 400, and a caret measured at the wrong
/// weight drifts.
///
/// ponytail: every call shapes `text` again -- the face's parse and plans are
/// cached, the glyphs are not. A field asking for four carets a frame shapes
/// four times; return the whole boundary table once if that ever shows.
pub fn caret_x(
    fonts: &[Font],
    text: &str,
    size_px: f64,
    axes: &[Axis<'_>],
    byte_index: usize,
) -> Result<f64, Error> {
    if byte_index > text.len() || !text.is_char_boundary(byte_index) {
        return Err(Error::InvalidOptions("byte_index"));
    }
    let positions = caret_positions(fonts, text, size_px, axes)?;
    Ok(positions
        .binary_search_by_key(&byte_index, |&(byte, _)| byte)
        .map_or(0., |i| positions[i].1))
}

/// The char boundary whose caret is nearest `x`. The inverse of [`caret_x`],
/// which is what a click in a text field needs.
pub fn hit_index(
    fonts: &[Font],
    text: &str,
    size_px: f64,
    axes: &[Axis<'_>],
    x: f64,
) -> Result<usize, Error> {
    let x = checked_finite(x, "x")?;
    // Seeded at infinity, not at byte 0's distance: byte 0 sits at the right
    // edge of a right-to-left run, not at x = 0.
    let (best, _) = caret_positions(fonts, text, size_px, axes)?
        .into_iter()
        .fold(
            (0, f64::INFINITY),
            |(best, best_distance), (byte, position)| {
                let distance = (position - x).abs();
                if distance < best_distance {
                    (byte, distance)
                } else {
                    (best, best_distance)
                }
            },
        );
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn hack() -> Font {
        Font::new(epaint_default_fonts::HACK_REGULAR).unwrap()
    }
    fn inter() -> Font {
        Font::new(ttf_inter::REGULAR).unwrap()
    }
    fn emoji() -> Font {
        Font::new(epaint_default_fonts::NOTO_EMOJI_REGULAR).unwrap()
    }
    fn symbols() -> Font {
        Font::new(MATERIAL_SYMBOLS).unwrap()
    }

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
        let path = glyph_path(&hack(), 'O', 64., &[], 0.05).unwrap();
        let rings = path.flatten(0.05, 250_000).unwrap();
        assert_eq!(rings.len(), 2, "outer contour plus counter");
        assert!(rings.iter().all(|r| r.len() > 8), "curves were flattened");
    }

    #[test]
    fn outline_scales_with_em_size() {
        let small = area(&glyph_path(&hack(), 'H', 32., &[], 0.05).unwrap()).abs();
        let large = area(&glyph_path(&hack(), 'H', 64., &[], 0.05).unwrap()).abs();
        assert!(small > 0.);
        assert!(
            (large / small - 4.).abs() < 0.05,
            "doubling the em size quadruples the area, got {large} / {small}"
        );
    }

    #[test]
    fn baseline_sits_at_zero_and_the_glyph_grows_upward() {
        let path = glyph_path(&hack(), 'H', 64., &[], 0.05).unwrap();
        let rings = path.flatten(0.05, 250_000).unwrap();
        let ys: Vec<f64> = rings.iter().flatten().map(|p| p.y).collect();
        let top = ys.iter().cloned().fold(f64::INFINITY, f64::min);
        let bottom = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        assert!(top < 0., "cap height is above the baseline");
        assert!(bottom.abs() < 1., "'H' rests on the baseline, got {bottom}");
    }

    #[test]
    fn tolerance_buys_vertices() {
        let coarse = glyph_path(&hack(), 'O', 64., &[], 0.5).unwrap();
        let fine = glyph_path(&hack(), 'O', 64., &[], 0.01).unwrap();
        assert!(fine.commands.len() > coarse.commands.len() * 2);
        // Both describe the same glyph, so the area must not drift with it.
        let (fine, coarse) = (area(&fine).abs(), area(&coarse).abs());
        assert!((fine - coarse).abs() / fine < 0.05, "{fine} vs {coarse}");
    }

    #[test]
    fn an_axis_a_static_font_lacks_is_ignored_not_an_error() {
        let plain = glyph_path(&hack(), 'H', 64., &[], 0.05).unwrap();
        let asked = glyph_path(&hack(), 'H', 64., &[("FILL", 1.0)], 0.05).unwrap();
        assert_eq!(plain, asked, "Hack has no FILL axis, so nothing moves");
    }

    /// Three Material Symbols glyphs (home, favorite, settings), fvar/avar/
    /// gvar/HVAR and GSUB FeatureVariations kept. Regenerate with
    /// `pyftsubset MaterialSymbolsOutlined[FILL,GRAD,opsz,wght].ttf
    /// --unicodes=U+E88A,U+E87D,U+E8B8 --layout-features='*' --no-hinting`.
    pub(crate) const MATERIAL_SYMBOLS: &[u8] =
        include_bytes!("../fonts/MaterialSymbolsOutlined-subset.ttf");

    #[test]
    fn a_collapsed_fill_contour_is_dropped_not_an_error() {
        // At FILL=0 the fill ring of every Material Symbol collapses to a
        // point. That is a legal empty contour, not an invalid path.
        let hollow = glyph_path(&symbols(), '\u{E87D}', 24., &[("FILL", 0.)], 0.05).unwrap();
        let solid = glyph_path(&symbols(), '\u{E87D}', 24., &[("FILL", 1.)], 0.05).unwrap();
        let hollow_area = area(&hollow).abs();
        let solid_area = area(&solid).abs();
        assert!(hollow_area > 0.);
        assert!(
            solid_area > hollow_area * 1.5,
            "filled heart covers more: {solid_area} vs {hollow_area}"
        );
        let mid = glyph_path(&symbols(), '\u{E87D}', 24., &[("FILL", 0.5)], 0.05).unwrap();
        let mid_area = area(&mid).abs();
        assert!(
            mid_area > hollow_area && mid_area < solid_area,
            "morph is monotone: {mid_area}"
        );
    }

    #[test]
    fn the_shaper_applies_feature_variations_at_the_fill_extreme() {
        // Material Symbols swap in a dedicated filled glyph via GSUB
        // FeatureVariations once FILL >= 0.99; the shaper must honour it, and
        // the swap must not move the advance (icons are fixed-width).
        let hollow = text_run(&[symbols()], "\u{E88A}", 24., &[("FILL", 0.)], 0.05).unwrap();
        let solid = text_run(&[symbols()], "\u{E88A}", 24., &[("FILL", 1.)], 0.05).unwrap();
        assert_ne!(
            hollow.glyphs[0].id, solid.glyphs[0].id,
            "filled glyph substituted"
        );
        assert_eq!(hollow.advance, solid.advance);
        assert_eq!(hollow.advance, 24., "Material Symbols advance one em");
    }

    #[test]
    fn caret_follows_the_weight_it_is_drawn_at() {
        let text = "mmmm";
        let regular = caret_x(&[inter()], text, 24., &[Weight::REGULAR.axis()], 4).unwrap();
        let bold = caret_x(&[inter()], text, 24., &[Weight::BOLD.axis()], 4).unwrap();
        assert!(bold > regular, "bold is wider: {bold} vs {regular}");
        let run = text_run(&[inter()], text, 24., &[Weight::BOLD.axis()], 0.05).unwrap();
        assert!((run.advance - bold).abs() < 1e-9, "caret and run agree");
        assert_eq!(
            hit_index(&[inter()], text, 24., &[Weight::BOLD.axis()], bold).unwrap(),
            4
        );
    }

    #[test]
    fn opsz_follows_the_size_unless_set_and_coords_are_quantised() {
        let at24 = normalized_coords(&symbols(), 24., &[]).unwrap();
        let at48 = normalized_coords(&symbols(), 48., &[]).unwrap();
        let at480 = normalized_coords(&symbols(), 480., &[]).unwrap();
        // Axis order is FILL, GRAD, opsz, wght; opsz default is 24, max 48.
        assert_eq!(at24[2], 0);
        assert_eq!(at48[2], 16384);
        assert_eq!(at480, at48, "clamped to the font's range, not reset");
        let pinned = normalized_coords(&symbols(), 480., &[("opsz", 24.)]).unwrap();
        assert_eq!(pinned[2], 0, "an explicit opsz wins");
        let a = normalized_coords(&symbols(), 24., &[("FILL", 0.5001)]).unwrap();
        let b = normalized_coords(&symbols(), 24., &[("FILL", 0.5019)]).unwrap();
        assert_eq!(a, b, "a spring's neighbouring floats share a cache key");
        assert_eq!(a[0] % 128, 0);
        assert_eq!(
            normalized_coords(&symbols(), 24., &[("FILL", 1.)]).unwrap()[0],
            16384
        );
        assert_eq!(
            normalized_coords(&symbols(), 24., &[("wght", 100.)]).unwrap()[3],
            -16384
        );
    }

    #[test]
    fn a_glyph_counter_winds_as_a_hole() {
        let path = glyph_path(&hack(), 'O', 96., &[], 0.05).unwrap();
        // The counter comes out empty because a typeface reverses it, not
        // because of the fill rule: the signed area is the ring, not the disc.
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
        let filled = area(&path).abs();
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
        let a = glyph_path(&hack(), 'O', 96., &[], 0.05).unwrap();
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
        let one = text_run(&[hack()], "a", 96., &[], 0.05).unwrap();
        let two = text_run(&[hack()], "a a", 96., &[], 0.05).unwrap();
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
        let run = text_run(&[hack()], "ab", 96., &[], 0.05).unwrap();
        let a = glyph_path(&hack(), 'a', 96., &[], 0.05).unwrap();
        let b = glyph_path(&hack(), 'b', 96., &[], 0.05).unwrap();
        let advance = text_run(&[hack()], "a", 96., &[], 0.05).unwrap().advance;
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
        let run = text_run(&[hack()], "Hg", 96., &[], 0.05).unwrap();
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
        let ordinary = text_run(&[hack()], "A", 96., &[], 0.05).unwrap();
        assert!(ordinary.advance.is_finite());
        assert!(ordinary.ascent.is_finite());
        assert!(ordinary.descent.is_finite());
        assert!(ordinary.line_height.is_finite());
        assert!(glyph_path(&hack(), 'A', 96., &[], 0.05).is_ok());

        let max_glyph = glyph_path(&hack(), 'A', f64::from(f32::MAX), &[], 0.05)
            .expect("f32::MAX is representable and must keep finite path commands");
        assert!(max_glyph.validate(usize::MAX).is_ok());
        assert!(
            text_run(&[hack()], "A", f64::from(f32::MAX), &[], 0.05).is_err(),
            "text_run must reject non-finite metrics derived at f32::MAX"
        );

        for result in [
            glyph_path(&hack(), 'A', f64::MAX, &[], 0.05).map(|_| ()),
            text_run(&[hack()], "A", f64::MAX, &[], 0.05).map(|_| ()),
        ] {
            assert!(result.is_err(), "f64::MAX must not cross the f32 boundary");
        }

        let smallest_positive = f64::from_bits(1);
        assert!(glyph_path(&hack(), 'A', smallest_positive, &[], 0.05).is_err());
        assert!(text_run(&[hack()], "A", smallest_positive, &[], 0.05).is_err());
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
        assert!(glyph_path(&hack(), '\u{10FFFF}', 96., &[], 0.05).is_err());
        let run = text_run(&[hack()], "a\u{10FFFF}a", 96., &[], 0.05).unwrap();
        assert!(!run.path.commands.is_empty());
        assert!(run.advance > 0.);
    }

    #[test]
    fn a_missing_glyph_is_reported_not_drawn_blank() {
        assert!(matches!(
            glyph_path(&hack(), '\u{10FFFD}', 64., &[], 0.05),
            Err(Error::MissingGlyph(_))
        ));
    }

    #[test]
    fn open_type_shaping_applies_kerning_and_combining_substitution() {
        let pair = text_run(&[inter()], "AV", 32., &[], 0.05).unwrap();
        let separate = text_run(&[inter()], "A", 32., &[], 0.05).unwrap().advance
            + text_run(&[inter()], "V", 32., &[], 0.05).unwrap().advance;
        assert!(pair.advance < separate, "kerning was not applied: {pair:?}");

        let composed = text_run(&[inter()], "A\u{301}", 32., &[], 0.05).unwrap();
        assert_eq!(composed.glyphs.len(), 1, "the mark was not composed");
    }

    #[test]
    fn combining_marks_keep_their_position_offsets_for_renderers() {
        // Inter keeps the Hebrew niqqud as a separate mark and positions it
        // with GPOS, which exercises the renderer's y-offset transport.
        let run = text_run(&[inter()], "ש\u{05b8}", 32., &[], 0.05).unwrap();
        assert!(
            run.glyphs.iter().any(|g| g.y.abs() > 0.01),
            "GPOS mark placement was discarded: {:?}",
            run.glyphs
        );
    }

    #[test]
    fn variable_weight_reaches_the_shaper_and_outline() {
        let regular = text_run(&[inter()], "KURV", 24., &[Weight::REGULAR.axis()], 0.05).unwrap();
        let bold = text_run(&[inter()], "KURV", 24., &[Weight::BOLD.axis()], 0.05).unwrap();
        assert_ne!(regular.path, bold.path, "the wght axis was ignored");
    }

    #[test]
    fn bidi_reorders_a_rtl_run_in_visual_order() {
        let fonts = [hack()];
        let shaped = shape(
            &open(&fonts, 16., &[]).unwrap(),
            "ab \u{05D0}\u{05D1}\u{05D2} cd",
            16.,
        );
        let clusters: Vec<usize> = shaped.glyphs.iter().map(|glyph| glyph.cluster).collect();
        assert!(
            clusters.windows(3).any(|window| window == [7, 5, 3]),
            "RTL glyphs were not visually reordered: {clusters:?}"
        );
    }

    #[test]
    fn wrapping_does_not_split_a_combining_cluster() {
        let text = "A\u{301}B";
        let first = text_run(&[inter()], "A\u{301}", 16., &[], 0.05)
            .unwrap()
            .advance;
        let lines = break_lines(&[inter()], text, 16., &[], first);
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
    fn a_cluster_wider_than_the_line_is_not_split() {
        // No break opportunity and no room even for the first cluster: the
        // overflow break must still wait for the next cluster start.
        let text = "A\u{301}B";
        let first = text_run(&[inter()], "A\u{301}", 16., &[], 0.05)
            .unwrap()
            .advance;
        let lines = break_lines(&[inter()], text, 16., &[], first / 2.).unwrap();
        assert_eq!(
            lines
                .into_iter()
                .map(|line| &text[line.text_range])
                .collect::<Vec<_>>(),
            ["A\u{301}", "B"]
        );
    }

    #[test]
    fn a_click_left_of_a_rtl_run_lands_at_its_end() {
        // Byte 0 of a right-to-left run is its right edge; x = 0 is its end.
        let text = "\u{05D0}\u{05D1}\u{05D2}";
        assert_eq!(caret_x(&[inter()], text, 16., &[], text.len()).unwrap(), 0.);
        assert_eq!(
            hit_index(&[inter()], text, 16., &[], -5.).unwrap(),
            text.len()
        );
    }

    #[test]
    fn fallback_selects_a_face_per_grapheme_cluster() {
        let run = text_run(&[hack(), emoji()], "A😀", 24., &[], 0.05).unwrap();
        assert_eq!(run.glyphs[0].font, 0);
        assert_eq!(run.glyphs.last().unwrap().font, 1);
        assert!(run.advance > 0.);
    }

    #[test]
    fn fallback_caret_round_trips_across_a_missing_glyph() {
        let fonts = [hack(), emoji()];
        let text = "A😀";
        let end = caret_x(&fonts, text, 24., &[], text.len()).unwrap();
        assert_eq!(hit_index(&fonts, text, 24., &[], end).unwrap(), text.len());
        let emoji = text.char_indices().nth(1).unwrap().0;
        let before_emoji = caret_x(&fonts, text, 24., &[], emoji).unwrap();
        assert!(end > before_emoji, "fallback glyph has no advance");
    }

    #[test]
    fn fallback_wrap_breaks_before_a_glyph_cluster() {
        let fonts = [hack(), emoji()];
        let text = "A😀B";
        let emoji_end = text.char_indices().nth(2).unwrap().0;
        let first_two = text_run(&fonts, &text[..emoji_end], 24., &[], 0.05)
            .unwrap()
            .advance;
        let lines = break_lines(&fonts, text, 24., &[], first_two + 0.1).unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text_range, 0..emoji_end);
    }
}

#[cfg(test)]
mod axis_tests {
    use super::*;

    #[test]
    fn a_static_font_declares_no_axes() {
        assert!(axes(&super::tests::hack()).is_empty());
    }
}

#[cfg(test)]
mod measure_tests {
    use super::tests::hack;
    use super::*;

    const SIZE: f64 = 16.;

    fn lines(text: &str, width: f64) -> Vec<&str> {
        break_lines(&[hack()], text, SIZE, &[], width)
            .unwrap()
            .into_iter()
            .map(|l| &text[l.text_range])
            .collect()
    }

    #[test]
    fn a_line_breaks_at_the_last_space_that_fits() {
        let text = "hello world foo";
        let width = caret_x(&[hack()], text, SIZE, &[], 11).unwrap();
        assert_eq!(lines(text, width), ["hello world ", "foo"]);
        let first = &break_lines(&[hack()], text, SIZE, &[], width).unwrap()[0];
        assert!(
            (first.advance - width).abs() < 1e-9,
            "the trailing space does not count: {first:?}"
        );
    }

    #[test]
    fn a_word_wider_than_the_line_breaks_mid_word() {
        let word = "x".repeat(40);
        let out = lines(&word, caret_x(&[hack()], &word, SIZE, &[], 10).unwrap());
        assert_eq!(out.len(), 4, "{out:?}");
        assert!(out.iter().all(|l| l.len() == 10), "{out:?}");
    }

    #[test]
    fn uax14_says_where_a_line_may_start() {
        // Width of the first `n` bytes of the text itself, so the font's own
        // advances decide and no test hard-codes a pixel.
        let w = |t: &str, n: usize| caret_x(&[hack()], t, SIZE, &[], n).unwrap();

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
            let x = caret_x(&[hack()], text, SIZE, &[], i).unwrap();
            assert_eq!(
                hit_index(&[hack()], text, SIZE, &[], x).unwrap(),
                i,
                "at {i}"
            );
        }
    }

    #[test]
    fn a_bad_font_keeps_its_skrifa_cause() {
        let Err(e) = Font::new(&b"not a font"[..]) else {
            panic!("bad bytes must not parse");
        };
        assert!(matches!(e, Error::Font(_)));
        assert!(std::error::Error::source(&e).is_some());
    }

    #[test]
    fn a_non_finite_click_does_not_snap_to_the_start() {
        for x in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            assert!(hit_index(&[hack()], "abc", SIZE, &[], x).is_err(), "{x}");
        }
        assert_eq!(hit_index(&[hack()], "abc", SIZE, &[], 1e6).unwrap(), 3);
    }
}
