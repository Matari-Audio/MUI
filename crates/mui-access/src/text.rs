//! AccessKit text runs use one direction and one visual line per node.
use super::run_id;
use accesskit::{Node, NodeId, Rect, Role, TextDirection, TextPosition, TextSelection};
use mui_scene::{ResolvedSurface, TextGeometry};
use std::ops::Range;

struct Run {
    range: Range<usize>,
    line: usize,
    rtl: bool,
    bounds: Rect,
    positions: Option<Vec<f32>>,
    widths: Option<Vec<f32>>,
}

fn id(key: &str, index: usize) -> NodeId {
    if index == 0 {
        run_id(key)
    } else {
        super::fnv(
            key.bytes()
                .chain(*b"\0run")
                .chain((index as u64).to_le_bytes()),
        )
    }
}

fn content<'a>(surface: &'a ResolvedSurface, value: &'a str) -> &'a str {
    surface.text_geometry.as_ref().map_or(value, |g| &g.text)
}

fn runs(surface: &ResolvedSurface, value: &str, carets: &[f64]) -> Vec<Run> {
    if let Some(geometry) = &surface.text_geometry {
        return geometry_runs(surface, geometry);
    }
    let bidi = unicode_bidi::BidiInfo::new(value, None);
    let mut groups: Vec<(Range<usize>, usize, bool)> = Vec::new();
    let mut line = 0;
    for (byte, ch) in value.char_indices() {
        let rtl = bidi
            .levels
            .get(byte)
            .is_some_and(unicode_bidi::Level::is_rtl);
        if let Some((range, previous_line, direction)) = groups.last_mut()
            && *previous_line == line
            && (*direction == rtl || ch == '\n')
        {
            range.end = byte + ch.len_utf8();
        } else {
            groups.push((byte..byte + ch.len_utf8(), line, rtl));
        }
        if ch == '\n' {
            line += 1;
        }
    }
    if groups.is_empty() || value.ends_with('\n') {
        groups.push((value.len()..value.len(), line, false));
    }
    let count = value.chars().count();
    let rtl = groups.len() == 1 && groups[0].2;
    // The legacy geometry describes only a single-direction visual line.
    // For unmeasured/mixed/multiline authored semantics, text and selection
    // still work; do not invent per-character rectangles from a field box.
    let measured = groups.len() == 1
        && carets.len() == count + 1
        && carets.iter().all(|x| x.is_finite())
        && carets
            .windows(2)
            .all(|w| if rtl { w[0] >= w[1] } else { w[0] <= w[1] });
    let f = surface.frame;
    groups
        .into_iter()
        .map(|(range, line, direction)| Run {
            range,
            line,
            rtl: direction,
            bounds: Rect::new(f.x, f.y, f.right(), f.bottom()),
            positions: measured.then(|| {
                carets[..count]
                    .iter()
                    .map(|&x| {
                        if rtl {
                            (f.size.width - x) as f32
                        } else {
                            x as f32
                        }
                    })
                    .collect()
            }),
            widths: measured.then(|| {
                carets
                    .windows(2)
                    .map(|w| (w[1] - w[0]).abs() as f32)
                    .collect()
            }),
        })
        .collect()
}

fn geometry_runs(surface: &ResolvedSurface, geometry: &TextGeometry) -> Vec<Run> {
    let mut runs = Vec::new();
    for (line_index, line) in geometry.lines.iter().enumerate() {
        let text = &geometry.text[line.range.clone()];
        let mut clusters: Vec<_> = line.carets.clusters.iter().collect();
        clusters.sort_by_key(|c| c.range.start);
        let cluster_at = |byte| {
            let index = clusters
                .partition_point(|c| c.range.start <= byte)
                .checked_sub(1)?;
            clusters
                .get(index)
                .copied()
                .filter(|c| c.range.contains(&byte))
        };
        // Logical order is required for selection/text traversal. Glyphs and
        // clusters may arrive in visual order, so never iterate them as text.
        let mut groups: Vec<(Range<usize>, bool)> = Vec::new();
        for (byte, ch) in text.char_indices() {
            let rtl = cluster_at(byte).is_some_and(|c| c.rtl);
            if let Some((range, direction)) = groups.last_mut()
                && *direction == rtl
            {
                range.end = byte + ch.len_utf8();
            } else {
                groups.push((byte..byte + ch.len_utf8(), rtl));
            }
        }
        if groups.is_empty() {
            groups.push((0..0, false));
        }
        let group_count = groups.len();
        for (group_index, (range, rtl)) in groups.into_iter().enumerate() {
            let mut spans = Vec::new();
            for (relative, ch) in text[range.clone()].char_indices() {
                let byte = range.start + relative;
                let end = byte + ch.len_utf8();
                let (leading, trailing) = cluster_at(byte).map_or_else(
                    || (line.carets.x(byte), line.carets.x(end)),
                    |c| {
                        let leading = if c.rtl { c.right } else { c.left };
                        let trailing = if end == c.range.end {
                            if c.rtl { c.left } else { c.right }
                        } else {
                            leading
                        };
                        (leading, trailing)
                    },
                );
                spans.push((leading, trailing));
            }
            let start = spans
                .first()
                .map_or_else(|| line.carets.x(range.start), |s| s.0);
            let left = spans
                .iter()
                .flat_map(|&(a, b)| [a, b])
                .fold(start, f64::min);
            let right = spans
                .iter()
                .flat_map(|&(a, b)| [a, b])
                .fold(start, f64::max);
            let x = surface.frame.x + line.origin.x;
            let y = surface.frame.y + line.origin.y;
            let mut absolute = line.range.start + range.start..line.range.start + range.end;
            // A hard newline belongs to its preceding visual line. Preserve it
            // in text traversal, with a zero-width position at the line end.
            if group_index + 1 == group_count {
                let next = geometry
                    .lines
                    .get(line_index + 1)
                    .map_or(geometry.text.len(), |l| l.range.start);
                if next > absolute.end {
                    let end = spans
                        .last()
                        .map_or_else(|| line.carets.x(line.range.len()), |s| s.1);
                    spans.extend(
                        geometry.text[absolute.end..next]
                            .chars()
                            .map(|_| (end, end)),
                    );
                    absolute.end = next;
                }
            }
            runs.push(Run {
                range: absolute,
                line: line_index,
                rtl,
                bounds: Rect::new(x + left, y, x + right, y + line.height),
                positions: Some(
                    spans
                        .iter()
                        .map(|&(a, _)| {
                            if rtl {
                                (right - a) as f32
                            } else {
                                (a - left) as f32
                            }
                        })
                        .collect(),
                ),
                widths: Some(spans.iter().map(|&(a, b)| (b - a).abs() as f32).collect()),
            });
        }
    }
    runs
}

/// Export current display text, including preedit, using its actual geometry.
pub(super) fn export(
    field: &mut Node,
    surface: &ResolvedSurface,
    value: &str,
    selection: (usize, usize),
    carets: &[f64],
    output: &mut Vec<(NodeId, Node)>,
) {
    let text = content(surface, value);
    let runs = runs(surface, value, carets);
    if runs.is_empty() {
        return;
    }
    field.set_value(text);
    if surface
        .text_geometry
        .as_ref()
        .is_some_and(|g| g.state.multiline)
        || runs.iter().any(|r| r.line > 0)
    {
        field.set_role(Role::MultilineTextInput);
    }
    let scalar_byte = |index: usize| {
        text.char_indices()
            .nth(index)
            .map_or(text.len(), |(byte, _)| byte)
    };
    let (anchor, focus) = if let Some(g) = &surface.text_geometry {
        // Display selection and caret are in UTF-8; anchor direction survives
        // a reversed selection and native IME preedit cursor movement.
        let anchor = if g.state.caret == g.state.selection.start {
            g.state.selection.end
        } else {
            g.state.selection.start
        };
        (anchor, g.state.caret)
    } else {
        (scalar_byte(selection.0), scalar_byte(selection.1))
    };
    let position = |byte: usize| {
        let geometry = surface.text_geometry.as_ref();
        let line = geometry.map(|g| g.row(byte));
        let mut index = runs
            .iter()
            .rposition(|r| r.range.start <= byte && line.is_none_or(|line| line == r.line))
            .unwrap_or(0);
        if let Some(g) = geometry
            && byte == g.state.caret
        {
            // At a direction boundary both neighboring runs can represent
            // this logical offset. Choose the physical caret actually drawn.
            let expected = surface.frame.x + g.caret.x;
            let x_at = |r: &Run| {
                let n = text[r.range.start..byte.clamp(r.range.start, r.range.end)]
                    .chars()
                    .count();
                let position = r
                    .positions
                    .as_ref()
                    .and_then(|p| p.get(n).copied())
                    .or_else(|| Some(*r.positions.as_ref()?.last()? + *r.widths.as_ref()?.last()?))
                    .unwrap_or(0.);
                if r.rtl {
                    r.bounds.x1 - f64::from(position)
                } else {
                    r.bounds.x0 + f64::from(position)
                }
            };
            if let Some((nearest, _)) = runs
                .iter()
                .enumerate()
                .filter(|(_, r)| {
                    r.line == g.row(byte) && r.range.start <= byte && byte <= r.range.end
                })
                .min_by(|(_, a), (_, b)| {
                    (x_at(a) - expected)
                        .abs()
                        .total_cmp(&(x_at(b) - expected).abs())
                })
            {
                index = nearest;
            }
        }
        let run = &runs[index];
        let byte = byte.clamp(run.range.start, run.range.end);
        TextPosition {
            node: id(&surface.key, index),
            character_index: text[run.range.start..byte].chars().count(),
        }
    };
    field.set_text_selection(TextSelection {
        anchor: position(anchor),
        focus: position(focus),
    });
    for (index, run) in runs.iter().enumerate() {
        let node_id = id(&surface.key, index);
        field.push_child(node_id);
        let mut node = Node::new(Role::TextRun);
        let value = &text[run.range.clone()];
        node.set_value(value);
        node.set_character_lengths(
            value
                .chars()
                .map(|ch| ch.len_utf8() as u8)
                .collect::<Vec<_>>(),
        );
        node.set_text_direction(if run.rtl {
            TextDirection::RightToLeft
        } else {
            TextDirection::LeftToRight
        });
        node.set_bounds(run.bounds);
        if surface.transform != mui_geometry::Affine::IDENTITY {
            node.set_transform(accesskit::Affine::new(surface.transform.as_coeffs()));
        }
        if let (Some(positions), Some(widths)) = (&run.positions, &run.widths) {
            node.set_character_positions(positions.clone());
            node.set_character_widths(widths.clone());
        }
        // Direction changes stay on the same visual line; hard/soft line
        // breaks have no next_on_line relation.
        if index > 0 && runs[index - 1].line == run.line {
            node.set_previous_on_line(id(&surface.key, index - 1));
        }
        if runs
            .get(index + 1)
            .is_some_and(|next| next.line == run.line)
        {
            node.set_next_on_line(id(&surface.key, index + 1));
        }
        output.push((node_id, node));
    }
}

pub(super) fn selection(
    surface: &ResolvedSurface,
    value: &str,
    carets: &[f64],
    selection: &TextSelection,
) -> Option<(usize, usize)> {
    let text = content(surface, value);
    let runs = runs(surface, value, carets);
    let position = |position: &TextPosition| {
        let run = runs
            .iter()
            .enumerate()
            .find(|&(index, _)| id(&surface.key, index) == position.node)?
            .1;
        let run_text = &text[run.range.clone()];
        if position.character_index > run_text.chars().count() {
            return None;
        }
        let byte = run.range.start
            + run_text
                .char_indices()
                .nth(position.character_index)
                .map_or(run_text.len(), |(byte, _)| byte);
        if let Some(g) = &surface.text_geometry {
            Some(g.state.value[..g.display_to_source(byte)].chars().count())
        } else {
            Some(value[..byte].chars().count())
        }
    };
    Some((position(&selection.anchor)?, position(&selection.focus)?))
}

pub(super) fn is_upstream(
    surface: &ResolvedSurface,
    value: &str,
    carets: &[f64],
    position: &TextPosition,
) -> Option<bool> {
    let text = content(surface, value);
    let runs = runs(surface, value, carets);
    let run = runs
        .iter()
        .enumerate()
        .find(|&(index, _)| id(&surface.key, index) == position.node)?
        .1;
    let run_text = &text[run.range.clone()];
    if position.character_index > run_text.chars().count() {
        return None;
    }
    let byte = run.range.start
        + run_text
            .char_indices()
            .nth(position.character_index)
            .map_or(run_text.len(), |(byte, _)| byte);
    let Some(g) = &surface.text_geometry else {
        return Some(false);
    };
    Some(
        g.lines[run.line].range.end == byte
            && g.lines
                .get(run.line + 1)
                .is_some_and(|next| next.range.start == byte),
    )
}
