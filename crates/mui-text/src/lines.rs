use harfrust::Direction;
use unicode_bidi::BidiInfo;

use crate::error::checked_finite;
use crate::shape::{TextCluster, clusters, open, shape};
use crate::{Axis, Error, Font};

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

pub(crate) fn visual_segments(text: &str) -> Vec<(std::ops::Range<usize>, Direction)> {
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

/// One shaped advance per `char` of `text`, and whether a line may start at
/// it. Glyph clusters keep ligatures and combining marks together: the
/// cluster's advance goes to its first character and the rest get none.
pub(crate) fn advances_of(text: &str, clusters: &[TextCluster]) -> Result<Vec<(f64, bool)>, Error> {
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
    Ok(break_lines_from_advances(
        text,
        &char_advances(fonts, text, size_px, axes)?,
        max_width,
    ))
}

/// One shaped advance per `char` of `text`, and whether a line may start at
/// it: what [`break_lines_from_advances`] breaks. Shape once, keep this, and a
/// paragraph breaks at every new width without shaping again.
pub fn char_advances(
    fonts: &[Font],
    text: &str,
    size_px: f64,
    axes: &[Axis<'_>],
) -> Result<Vec<(f64, bool)>, Error> {
    let faces = open(fonts, size_px, axes)?;
    advances_of(text, &clusters(text, &shape(&faces, text, size_px)))
}

/// [`break_lines`] on advances [`char_advances`] already shaped for `text`.
pub fn break_lines_from_advances(
    text: &str,
    advances: &[(f64, bool)],
    max_width: f64,
) -> Vec<Line> {
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
            if let Some((bi, advance)) = brk.filter(|&(bi, _)| bi > start) {
                lines.push(Line {
                    text_range: start..bi,
                    advance,
                });
                start = bi;
                x = word;
            } else {
                // The word itself does not fit: break at the overflowing
                // cluster.
                lines.push(Line {
                    text_range: start..i,
                    advance: x,
                });
                start = i;
                (x, word) = (0., 0.);
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

/// The narrowest `text` breaks to without splitting a word: its widest run
/// between two line-break opportunities, trailing spaces left out. What CSS
/// calls the min-content width.
pub fn min_content_width(text: &str, advances: &[(f64, bool)]) -> f64 {
    let mut opps = unicode_linebreak::linebreaks(text)
        .map(|(i, _)| i)
        .peekable();
    let (mut widest, mut word, mut trim) = (0f64, 0., 0.);
    for ((i, ch), &(a, at_cluster_start)) in text.char_indices().zip(advances) {
        while opps.peek().is_some_and(|&bi| bi < i) {
            opps.next();
        }
        if at_cluster_start && opps.peek() == Some(&i) {
            widest = widest.max(word - trim);
            (word, trim) = (0., 0.);
        }
        word += a;
        trim = if ch.is_ascii_whitespace() {
            trim + a
        } else {
            0.
        };
    }
    widest.max(word - trim)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fonts::{emoji, hack, inter};
    use crate::{caret_x, shape_run, text_run};

    const SIZE: f64 = 16.;

    fn lines(text: &str, width: f64) -> Vec<&str> {
        break_lines(&[hack()], text, SIZE, &[], width)
            .unwrap()
            .into_iter()
            .map(|l| &text[l.text_range])
            .collect()
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
    fn min_content_is_the_widest_word() {
        let w = |t: &str| {
            let a = char_advances(&[hack()], t, SIZE, &[]).unwrap();
            min_content_width(t, &a)
        };
        // Hack is monospaced: the longest word sets it, spaces do not count.
        assert_eq!(w("ab abcd abc  "), w("abcd"));
        assert_eq!(w("ab\nabcd"), w("abcd"));
        assert_eq!(
            shape_run(&[hack()], "ab abcd", SIZE, &[])
                .unwrap()
                .min_content,
            w("abcd")
        );
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
}
