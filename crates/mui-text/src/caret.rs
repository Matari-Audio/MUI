use crate::error::checked_finite;
use crate::shape::{clusters, open, shape};
use crate::{Axis, Error, Font};

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
