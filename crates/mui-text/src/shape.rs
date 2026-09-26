use std::sync::{Arc, Mutex};

use harfrust::{
    BufferClusterLevel, BufferFlags, Direction, ShapeOptions, ShapePlan, ShapePlanKey, ShaperData,
    ShaperInstance, UnicodeBuffer,
};
use mui_geometry::Path;
use skrifa::outline::DrawSettings;
use skrifa::prelude::{LocationRef, Size};
use skrifa::{FontRef, GlyphId, MetadataProvider as _};
use unicode_segmentation::UnicodeSegmentation;

use crate::error::{checked_finite, checked_metric, checked_size};
use crate::font::location;
use crate::lines::{advances_of, visual_segments};
use crate::outline::PathPen;
use crate::{Axis, Error, Font, min_content_width};

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
    /// Its widest unbreakable piece: see [`min_content_width`].
    pub min_content: f64,
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
pub(crate) struct ShapedGlyph {
    font: usize,
    glyph_id: GlyphId,
    x: f64,
    y: f64,
    advance: f64,
    pub(crate) cluster: usize,
    rtl: bool,
}

#[derive(Debug, Default)]
pub(crate) struct ShapedText {
    pub(crate) glyphs: Vec<ShapedGlyph>,
    advance: f64,
}

/// A face opened for one call: parsed, placed in its design space at the
/// call's size and axes, and ready to shape. Omitted axes sit at their
/// defaults, so the same tag set always means the same instance.
pub(crate) struct Face<'a> {
    font: FontRef<'a>,
    data: &'a ShaperData,
    plans: &'a Mutex<Vec<Arc<ShapePlan>>>,
    location: skrifa::instance::Location,
    instance: ShaperInstance,
}

/// Open `fonts` -- the primary face, then its fallbacks -- at one size and
/// axis setting. A single face is simply a fallback chain of one.
pub(crate) fn open<'a>(
    fonts: &'a [Font],
    size_px: f64,
    axes: &[Axis<'_>],
) -> Result<Vec<Face<'a>>, Error> {
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
    let mut run = shaped_run(&faces, text, size_px)?;
    let size = Size::new(checked_size(size_px)?);
    let outlines: Vec<_> = faces.iter().map(|f| f.font.outline_glyphs()).collect();
    for g in &run.glyphs {
        if let Some(outline) = outlines[g.font].get(GlyphId::new(g.id)) {
            pen.dx = g.x;
            pen.dy = g.y;
            let location = LocationRef::from(&faces[g.font].location);
            outline
                .draw(DrawSettings::unhinted(size, location), &mut pen)
                .map_err(Error::Draw)?;
        }
        pen.close_open_contour();
    }
    run.path = pen.finish();
    run.path.validate(usize::MAX)?;
    Ok(run)
}

/// [`text_run`] without the outlines: the same glyphs, advance and metrics,
/// and an empty `path`. For a renderer that draws `glyphs` from its own
/// cache, and for measuring, which never needs the ink.
pub fn shape_run(
    fonts: &[Font],
    text: &str,
    size_px: f64,
    axes: &[Axis<'_>],
) -> Result<TextRun, Error> {
    shaped_run(&open(fonts, size_px, axes)?, text, size_px)
}

fn shaped_run(faces: &[Face<'_>], text: &str, size_px: f64) -> Result<TextRun, Error> {
    let size = Size::new(checked_size(size_px)?);
    let (mut ascent, mut descent, mut line_height) = (0f64, 0f64, 0f64);
    for face in faces {
        let m = face.font.metrics(size, LocationRef::from(&face.location));
        ascent = ascent.max(checked_metric(m.ascent, "font metrics")?);
        // Negative in font space, positive below the baseline here.
        descent = descent.max(checked_metric(-m.descent, "font metrics")?);
        line_height = line_height.max(checked_metric(
            m.ascent - m.descent + m.leading,
            "font metrics",
        )?);
    }
    let shaped = shape(faces, text, size_px);
    let min_content = min_content_width(text, &advances_of(text, &clusters(text, &shaped))?);
    Ok(TextRun {
        path: Path::default(),
        glyphs: shaped
            .glyphs
            .iter()
            .map(|g| Glyph {
                font: g.font,
                id: g.glyph_id.to_u32(),
                x: g.x,
                y: g.y,
            })
            .collect(),
        advance: checked_finite(shaped.advance, "font metrics")?,
        ascent,
        descent,
        line_height,
        min_content,
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
pub(crate) fn shape(faces: &[Face<'_>], text: &str, size_px: f64) -> ShapedText {
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
    let mut plans = face
        .plans
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(plan) = plans.iter().find(|plan| key.matches(plan)) {
        return plan.clone();
    }
    let plan = Arc::new(ShapePlan::new(shaper, direction, script, None, &[]));
    plans.push(plan.clone());
    plan
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct TextCluster {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) advance: f64,
    pub(crate) rtl: bool,
    pub(crate) left: f64,
    pub(crate) right: f64,
}

/// The shaped glyphs grouped by source cluster, in logical order. Glyphs of
/// one cluster share its `start`, so sorting brings them together and one
/// pass merges them.
pub(crate) fn clusters(text: &str, shaped: &ShapedText) -> Vec<TextCluster> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fonts::{emoji, hack, inter, symbols};
    use crate::{Weight, glyph_path};
    use mui_geometry::{PathCommand, Point};

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
    fn shape_run_is_text_run_without_the_ink() {
        let fonts = [hack(), inter()];
        let drawn = text_run(&fonts, "Hg \u{e9}", 24., &[], 0.05).unwrap();
        let shaped = shape_run(&fonts, "Hg \u{e9}", 24., &[]).unwrap();
        assert!(!drawn.path.commands.is_empty());
        assert_eq!(
            shaped,
            TextRun {
                path: Path::default(),
                ..drawn
            }
        );
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
    fn a_glyph_the_face_lacks_does_not_blank_the_label() {
        // `glyph_path` reports it; a run must not, or one stray codepoint
        // silently erases a whole line of UI text.
        assert!(glyph_path(&hack(), '\u{10FFFF}', 96., &[], 0.05).is_err());
        let run = text_run(&[hack()], "a\u{10FFFF}a", 96., &[], 0.05).unwrap();
        assert!(!run.path.commands.is_empty());
        assert!(run.advance > 0.);
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
    fn fallback_selects_a_face_per_grapheme_cluster() {
        let run = text_run(&[hack(), emoji()], "A😀", 24., &[], 0.05).unwrap();
        assert_eq!(run.glyphs[0].font, 0);
        assert_eq!(run.glyphs.last().unwrap().font, 1);
        assert!(run.advance > 0.);
    }
}
