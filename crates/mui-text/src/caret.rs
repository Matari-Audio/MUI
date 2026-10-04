use crate::error::checked_finite;
use crate::shape::{clusters, open, shape};
use crate::{Axis, Error, Font};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

/// Every char boundary of `text` with the pen x of a caret there, ascending
/// by byte. A caret never lands inside a ligature or combining cluster: every
/// boundary within one sits at its leading edge. One shaping for any number
/// of carets; [`caret_x`] is one lookup in this.
pub fn caret_positions(
    fonts: &[Font],
    text: &str,
    size_px: f64,
    axes: &[Axis<'_>],
) -> Result<Vec<(usize, f64)>, Error> {
    let faces = open(fonts, size_px, axes)?;
    let clusters = clusters(text, &shape(&faces, text, size_px));
    Ok(positions(text, &clusters))
}
fn positions(text: &str, clusters: &[crate::shape::TextCluster]) -> Vec<(usize, f64)> {
    // One allocation: boundaries and their x filled in place, NaN until set.
    let mut carets: Vec<(usize, f64)> = text
        .char_indices()
        .map(|(byte, _)| (byte, f64::NAN))
        .chain(std::iter::once((text.len(), f64::NAN)))
        .collect();
    let last_index = carets.len() - 1;
    for cluster in clusters {
        let first = carets
            .partition_point(|&(byte, _)| byte < cluster.start)
            .min(last_index);
        let last = carets
            .partition_point(|&(byte, _)| byte < cluster.end)
            .min(last_index);
        let (leading, trailing) = if cluster.rtl {
            (cluster.right, cluster.left)
        } else {
            (cluster.left, cluster.right)
        };
        for (byte, x) in &mut carets[first..=last] {
            *x = if *byte == cluster.end {
                trailing
            } else {
                leading
            };
        }
    }
    let mut previous = 0.;
    for (_, x) in &mut carets {
        if x.is_finite() {
            previous = *x;
        } else {
            *x = previous;
        }
    }
    carets
}

/// A shaping cluster's visual extent. Logical ranges can be non-contiguous
/// in visual order when a line contains both left-to-right and right-to-left text.
#[derive(Clone, Debug, PartialEq)]
pub struct CaretCluster {
    pub range: Range<usize>,
    pub left: f64,
    pub right: f64,
    pub rtl: bool,
}

/// Shaped positions for editing one visual line. `positions` retains scalar
/// boundaries for native/accessibility clients; pointer and arrow movement use
/// extended grapheme boundaries only.
#[derive(Clone, Debug, PartialEq)]
pub struct CaretMap {
    pub positions: Vec<(usize, f64)>,
    pub clusters: Vec<CaretCluster>,
    pub stops: Vec<(usize, f64)>,
}
impl CaretMap {
    pub fn new(fonts: &[Font], text: &str, size: f64, axes: &[Axis<'_>]) -> Result<Self, Error> {
        let faces = open(fonts, size, axes)?;
        let shaped = shape(&faces, text, size);
        let shaped_clusters = clusters(text, &shaped);
        let positions = positions(text, &shaped_clusters);
        let clusters = shaped_clusters
            .into_iter()
            .map(|c| CaretCluster {
                range: c.start..c.end,
                left: c.left,
                right: c.right,
                rtl: c.rtl,
            })
            .collect();
        Ok(Self::with_positions(text, positions, clusters))
    }
    /// The same fontless estimate used by scene measurement. Real fonts always
    /// take the shaped path above.
    pub fn fallback(text: &str, advance: f64) -> Self {
        let positions: Vec<_> = text
            .char_indices()
            .map(|(b, _)| b)
            .chain([text.len()])
            .enumerate()
            .map(|(i, b)| (b, i as f64 * advance))
            .collect();
        let clusters = positions
            .windows(2)
            .map(|w| CaretCluster {
                range: w[0].0..w[1].0,
                left: w[0].1,
                right: w[1].1,
                rtl: false,
            })
            .collect();
        Self::with_positions(text, positions, clusters)
    }
    fn with_positions(
        text: &str,
        positions: Vec<(usize, f64)>,
        clusters: Vec<CaretCluster>,
    ) -> Self {
        let mut stops: Vec<_> = text
            .grapheme_indices(true)
            .map(|(b, _)| b)
            .chain([text.len()])
            .map(|b| {
                (
                    b,
                    positions
                        .binary_search_by_key(&b, |p| p.0)
                        .map_or(0., |i| positions[i].1),
                )
            })
            .collect();
        stops.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        Self {
            positions,
            clusters,
            stops,
        }
    }
    pub fn x(&self, byte: usize) -> f64 {
        self.positions
            .binary_search_by_key(&byte, |p| p.0)
            .map_or(0., |i| self.positions[i].1)
    }
    pub fn hit(&self, x: f64) -> usize {
        self.stops
            .iter()
            .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
            .map_or(0, |p| p.0)
    }
    /// Move in visual order without entering a grapheme cluster.
    pub fn move_visual(&self, byte: usize, right: bool) -> usize {
        let i = self.stops.iter().position(|p| p.0 == byte).unwrap_or(0);
        let next = if right {
            (i + 1).min(self.stops.len().saturating_sub(1))
        } else {
            i.saturating_sub(1)
        };
        self.stops.get(next).map_or(byte, |p| p.0)
    }
    /// Separate visual bands for a logical selection. Entire shaping clusters
    /// are included, so ligature and combining glyphs keep their original shape.
    pub fn selection_spans(&self, range: Range<usize>) -> Vec<Range<f64>> {
        let mut spans: Vec<_> = self
            .clusters
            .iter()
            .filter(|c| c.range.start < range.end && c.range.end > range.start)
            .map(|c| c.left..c.right)
            .collect();
        spans.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut merged: Vec<Range<f64>> = Vec::new();
        for s in spans {
            if let Some(last) = merged.last_mut()
                && s.start <= last.end + 0.01
            {
                last.end = last.end.max(s.end);
            } else {
                merged.push(s);
            }
        }
        merged
    }
}

/// Pen x of the caret sitting *before* the char at `byte_index`, which must be
/// a char boundary. `text.len()` is the caret at the end. `fonts` and `axes`
/// must match what the run is drawn with: a variable face advances
/// differently at `wght` 700 than at 400, and a caret measured at the wrong
/// weight drifts.
///
/// Every call shapes `text` again; a caller wanting several carets in one
/// string takes [`caret_positions`] once instead.
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
    use crate::test_fonts::{emoji, hack, inter};
    use crate::{Weight, text_run};

    const SIZE: f64 = 16.;

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
    fn a_non_finite_click_does_not_snap_to_the_start() {
        for x in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            assert!(hit_index(&[hack()], "abc", SIZE, &[], x).is_err(), "{x}");
        }
        assert_eq!(hit_index(&[hack()], "abc", SIZE, &[], 1e6).unwrap(), 3);
    }
}

#[cfg(test)]
mod editing_tests {
    use super::*;
    use crate::test_fonts::inter;
    #[test]
    fn grapheme_stops_and_mixed_direction_selection_use_shaped_clusters() {
        let text = "Ae\u{301} אב cd";
        let map = CaretMap::new(&[inter()], text, 16., &[]).unwrap();
        assert!(
            !map.stops.iter().any(|(b, _)| *b == 2),
            "no stop inside combining grapheme"
        );
        assert!(map.clusters.iter().any(|c| c.rtl));
        let hebrew = text.find('א').unwrap();
        let spans = map.selection_spans(hebrew..hebrew + 'א'.len_utf8());
        assert_eq!(spans.len(), 1);
        let cluster = map
            .clusters
            .iter()
            .find(|c| c.range.start == hebrew)
            .unwrap();
        assert_eq!(spans[0], cluster.left..cluster.right);
        for &(b, x) in &map.stops {
            let hit = map.hit(x);
            assert!(text.is_char_boundary(hit));
            assert!(map.stops.iter().any(|p| p.0 == hit));
            if b == hebrew {
                assert!(map.move_visual(b, true) != b);
            }
        }
        let rtl = CaretMap::new(&[inter()], "אבג", 16., &[]).unwrap();
        assert!(
            rtl.move_visual(0, false) > 0,
            "left moves forward logically in RTL"
        );
    }
}
