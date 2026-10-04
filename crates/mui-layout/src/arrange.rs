//! The arrange pass: flex distribution, cells, overlays and pinned floats.
//!
//! Walks the measured tree handing out final rects. Growth and shrink are
//! settled here, and pinned floats are placed afterwards against the
//! anchor frames this pass just produced.

use super::*;

/// Resolve flexible lengths from unclamped bases and constrained hypothetical
/// sizes. Frozen items keep their min/max bound; remaining items redistribute
/// free space using grow factors or basis-scaled shrink factors.
pub(crate) fn distribute<P>(
    children: &[&Measured<'_, P>],
    gap: f64,
    vertical: bool,
    inner: Size,
) -> Vec<f64> {
    let available = inner.main(vertical) - gap * children.len().saturating_sub(1) as f64;
    let bases: Vec<_> = children
        .iter()
        .map(|c| c.flex_basis(vertical, Some(inner)))
        .collect();
    let hypothetical: Vec<_> = children
        .iter()
        .map(|c| c.base(vertical, Some(inner)))
        .collect();
    let growing = hypothetical.iter().sum::<f64>() < available;
    let factor = |i: usize| {
        if growing {
            children[i].node.grow
        } else {
            children[i].node.shrink
        }
    };
    let weight = |i: usize| factor(i) * if growing { 1.0 } else { bases[i] };
    let mut sizes = hypothetical.clone();
    let mut frozen: Vec<_> = (0..children.len())
        .map(|i| {
            factor(i) == 0.0
                || if growing {
                    bases[i] > hypothetical[i]
                } else {
                    bases[i] < hypothetical[i]
                }
        })
        .collect();
    let remaining = |sizes: &[f64], frozen: &[bool]| {
        available
            - (0..children.len())
                .map(|i| if frozen[i] { sizes[i] } else { bases[i] })
                .sum::<f64>()
    };
    let initial_free = remaining(&sizes, &frozen);
    // ponytail: CSS's freeze loop is O(n²) in the worst case (one bound per
    // round). Keep this direct algorithm until large constrained rows warrant
    // a sorted solver that also handles mixed min/max violations.
    let mut violations = vec![0.0; children.len()];
    while frozen.iter().any(|f| !f) {
        let mut free = remaining(&sizes, &frozen);
        let factors: f64 = (0..children.len())
            .filter(|&i| !frozen[i])
            .map(factor)
            .sum();
        if factors < 1.0 && (initial_free * factors).abs() < free.abs() {
            free = initial_free * factors;
        }
        let total: f64 = (0..children.len())
            .filter(|&i| !frozen[i])
            .map(weight)
            .sum();
        violations.fill(0.0);
        for i in 0..children.len() {
            if frozen[i] {
                continue;
            }
            let target = bases[i]
                + if total > 0.0 {
                    free * weight(i) / total
                } else {
                    0.0
                };
            let c = children[i];
            sizes[i] = c
                .node
                .rare()
                .maximum
                .map_or(target, |m| target.min(m.main(vertical)))
                .max(c.floor.main(vertical));
            violations[i] = sizes[i] - target;
        }
        let violation: f64 = violations.iter().sum();
        for i in 0..children.len() {
            if violation.abs() < 1e-9
                || (violation > 0.0 && violations[i] > 0.0)
                || (violation < 0.0 && violations[i] < 0.0)
            {
                frozen[i] = true;
            }
        }
    }
    sizes
}

/// A child in a rect of its own: an overlay layer or a grid cell. Returns the
/// child's origin within the cell and its size.
pub(crate) fn cell<P>(
    c: &Measured<'_, P>,
    cell: Size,
    default: (Align, Align),
) -> ([f64; 2], Size) {
    let n = c.node;
    let (ax, ay) = n.anchor.unwrap_or(default);
    let (w, h) = match c.aspect_width(cell) {
        Some((w, a)) => (
            w,
            n.rare()
                .maximum
                .map_or(w / a, |m| (w / a).min(m.height))
                .max(c.floor.height),
        ),
        None => (
            c.extent(false, cell.width, ax),
            c.extent(true, cell.height, ay),
        ),
    };
    (
        [
            place(cell.width, w, ax) + n.offset[0],
            place(cell.height, h, ay) + n.offset[1],
        ],
        Size::new(w, h),
    )
}

/// An empty frame for a subtree that is not laid out -- a `fits` candidate
/// that lost. At the parent's own origin, so a walk that checks children
/// against their parent's box still passes, and at zero size, which is how a
/// paint walk knows to skip it.
pub(crate) fn hide<P>(
    m: &Measured<'_, P>,
    origin: [f64; 2],
    out: &mut (Vec<(Id, u32)>, Vec<Frame>),
) {
    let frame = Frame {
        x: origin[0],
        y: origin[1],
        size: Size::ZERO,
    };
    if let Some(id) = m.node.id.clone() {
        out.0.push((id, out.1.len() as u32));
    }
    out.1.push(frame);
    for c in &m.children {
        hide(c, origin, out);
    }
}

pub(crate) fn arrange<P>(
    m: &Measured<'_, P>,
    ancestor: &str,
    origin: [f64; 2],
    size: Size,
    pins: &Pins<'_>,
    viewport: Option<Viewport>,
    out: &mut (Vec<(Id, u32)>, Vec<Frame>),
) -> Result<(), Error> {
    let n = m.node;
    // A box handed less than its floor is not refused: `distribute` and
    // `extent` keep every child at its own floor, so the content overflows.
    let here = n.id.as_deref().unwrap_or(ancestor);
    let frame = Frame {
        x: origin[0],
        y: origin[1],
        size,
    };
    if let Some(id) = n.id.clone() {
        out.0.push((id, out.1.len() as u32));
    }
    out.1.push(frame);
    let inner = Size::new(
        (size.width - m.padding.horizontal()).max(0.0),
        (size.height - m.padding.vertical()).max(0.0),
    );
    let at = |x: f64, y: f64| {
        [
            origin[0] + m.padding.left + x - n.scrolled[0],
            origin[1] + m.padding.top + y - n.scrolled[1],
        ]
    };
    let default = cell_default(n);
    let flow = m.flow();
    // The box a sticky child travels in -- this node's content box, which for
    // a scroll node is the whole extent its children were laid into.
    let mut section = inner;
    // Everything under a scroll node sticks to that node's leading edge.
    let viewport = match n.vertical().filter(|_| n.scroll) {
        Some(v) => Some(Viewport {
            vertical: v,
            edge: origin[v as usize] + if v { m.padding.top } else { m.padding.left },
        }),
        None => viewport,
    };
    // Where each in-flow child goes, then every child in declaration order
    // so frames stay in tree order; a float sits in the padding box like an
    // overlay child, unscrolled.
    let mut placed: Vec<Option<([f64; 2], Size)>> = vec![None; m.children.len()];
    match &n.kind {
        Kind::Leaf => {}
        Kind::Overlay(_) | Kind::Content(_) => {
            // A scrolling stack lays its children into their own extent,
            // like a scrolling column does on its main axis; placed in the
            // viewport instead they were squeezed to it, and a viewport-sized
            // child is nothing the wheel can slide.
            if n.scroll {
                section = Size::new(
                    inner.width.max(m.content.width),
                    inner.height.max(m.content.height),
                );
            }
            for c in &flow {
                let (p, s) = cell(c, section, default);
                placed[c.index] = Some((at(p[0], p[1]), s));
            }
        }
        Kind::Fits(_) => {
            for (i, c) in m.children.iter().enumerate() {
                if i == m.pick && !c.node.float {
                    let (p, s) = cell(c, inner, default);
                    placed[i] = Some((at(p[0], p[1]), s));
                } else if !c.node.float {
                    placed[i] = Some(([origin[0], origin[1]], Size::ZERO));
                }
            }
        }
        Kind::Grid { .. } if !m.columns.is_empty() => {
            for p in crate::grid::placements(&flow, m.cols, usize::MAX)? {
                let child = &m.children[p.index];
                let width = m.columns[p.column..p.column + p.column_span]
                    .iter()
                    .sum::<f64>()
                    + m.gap * (p.column_span - 1) as f64;
                let height = m.rows[p.row..p.row + p.row_span].iter().sum::<f64>()
                    + m.line_gap * (p.row_span - 1) as f64;
                let (offset, mut size) = cell(child, Size::new(width, height), default);
                size = Size::new(size.width.min(width), size.height.min(height));
                let x = m.columns[..p.column].iter().sum::<f64>() + m.gap * p.column as f64;
                let y = m.rows[..p.row].iter().sum::<f64>() + m.line_gap * p.row as f64;
                placed[p.index] = Some((
                    at(
                        x + (offset[0] - child.node.offset[0]).max(0.0) + child.node.offset[0],
                        y + (offset[1] - child.node.offset[1]).max(0.0) + child.node.offset[1],
                    ),
                    size,
                ));
            }
        }
        Kind::Grid { .. } => {
            let cols = m.cols;
            let grid = grid_rows(&flow, cols);
            let col_w =
                (inner.width - m.gap * cols.saturating_sub(1) as f64).max(0.0) / cols as f64;
            let columns = if m.columns.is_empty() {
                vec![col_w; cols]
            } else {
                m.columns.clone()
            };
            let heights: Vec<f64> = grid
                .iter()
                .map(|r| r.iter().map(|c| c.size.height).fold(0.0, f64::max))
                .collect();
            let surplus = (inner.height
                - heights.iter().sum::<f64>()
                - m.line_gap * grid.len().saturating_sub(1) as f64)
                .max(0.0)
                / grid.len().max(1) as f64;
            let mut y = 0.0;
            for (row, h) in grid.iter().zip(heights) {
                let mut col = 0;
                for c in *row {
                    let span = c.node.span.clamp(1, cols.max(1));
                    let cell_size = Size::new(
                        columns[col..col + span].iter().sum::<f64>() + m.gap * (span - 1) as f64,
                        h + surplus,
                    );
                    let (p, mut s) = cell(c, cell_size, default);
                    // A cell never outgrows its track: a fixed size larger than
                    // the column would otherwise paint straight through the
                    // neighbour, since nothing here shrinks it. The clamp stays
                    // in the grid, not in `cell`, so a deliberately oversized
                    // overlay or float keeps its size.
                    s = Size::new(s.width.min(cell_size.width), s.height.min(cell_size.height));
                    // Placement was computed from the unclamped extent, so a
                    // centred cell starts left of its column; pull it back,
                    // like CSS safe alignment.
                    let safe = |v: f64, o: f64| (v - o).max(0.0) + o;
                    placed[c.index] = Some((
                        at(
                            columns[..col].iter().sum::<f64>()
                                + col as f64 * m.gap
                                + safe(p[0], c.node.offset[0]),
                            y + safe(p[1], c.node.offset[1]),
                        ),
                        s,
                    ));
                    col += span;
                }
                y += h + surplus + m.line_gap;
            }
        }
        Kind::Branch { vertical, .. } => {
            let v = *vertical;
            // A scroll node lays its children into their own extent when
            // that is larger than the frame: nothing shrinks, it overflows.
            if n.scroll {
                section = Size::axes(inner.main(v).max(m.content.main(v)), inner.cross(v), v);
            }
            let inner = section;
            let lines = if n.wrap {
                wrap_lines(&flow, m.gap, v, inner.main(v))
            } else {
                vec![(0, flow.len())]
            };
            let line_cross = |(a, b): (usize, usize)| {
                flow[a..b]
                    .iter()
                    .map(|c| c.size.cross(v))
                    .fold(0.0, f64::max)
            };
            // Measure broke the lines against the width it was offered; a flex
            // ancestor that squeezed the row since then can force one more, and
            // the lines then overflow the cross axis it measured.
            // One line keeps the whole cross axis, so an unwrapped row is
            // exactly what it was; several share it by their own heights.
            let single = lines.len() == 1;
            let mut line_start = 0.0;
            for (a, b) in lines {
                let line = &flow[a..b];
                let line_cross = if single {
                    inner.cross(v)
                } else {
                    line_cross((a, b))
                };
                let line_inner = Size::axes(inner.main(v), line_cross, v);
                let mut allocated = distribute(line, m.gap, v, line_inner);
                for (main, child) in allocated.iter_mut().zip(line) {
                    if let Some(fitted) = child
                        .node
                        .rare()
                        .fitted_aspect(Size::axes(*main, line_cross, v))
                    {
                        *main = fitted.main(v);
                    }
                }
                let count = line.len() as f64;
                let residual = (inner.main(v)
                    - allocated.iter().sum::<f64>()
                    - m.gap * (count - 1.0).max(0.0))
                .max(0.0);
                let (mut cursor, extra) = match n.justify {
                    Justify::SpaceBetween if count > 1.0 => (0.0, residual / (count - 1.0)),
                    // CSS: `space-between` on one item is `flex-start`, so a
                    // header row does not jump when its second child is gone.
                    Justify::Start | Justify::SpaceBetween => (0.0, 0.0),
                    Justify::Center => (residual * 0.5, 0.0),
                    Justify::End => (residual, 0.0),
                    Justify::SpaceAround => (residual / count * 0.5, residual / count),
                    Justify::SpaceEvenly => (residual / (count + 1.0), residual / (count + 1.0)),
                };
                for (c, main) in line.iter().zip(allocated) {
                    let align = c.node.align_self.unwrap_or(n.align);
                    let aspect_room = if c.node.rare().aspect_fit {
                        Size::axes(main, line_cross, v)
                    } else {
                        line_inner
                    };
                    let cross = match c.aspect_width(aspect_room) {
                        Some((w, _)) if v => w,
                        Some((_, a)) => main / a,
                        None => c.extent(!v, line_cross, align),
                    };
                    let cross = c
                        .node
                        .rare()
                        .maximum
                        .map_or(cross, |m| cross.min(m.cross(v)))
                        .max(c.floor.cross(v));
                    let cross_pos = line_start + place(line_cross, cross, align);
                    let pos = if v {
                        at(cross_pos, cursor)
                    } else {
                        at(cursor, cross_pos)
                    };
                    placed[c.index] = Some((pos, Size::axes(main, cross, v)));
                    cursor += main + m.gap + extra;
                }
                line_start += line_cross + m.line_gap;
            }
        }
    }
    for (i, c) in m.children.iter().enumerate() {
        // A candidate that lost keeps its place in the frame list, empty, so
        // a walk of the tree still lines up with it.
        if matches!(n.kind, Kind::Fits(_)) && i != m.pick && !c.node.float {
            hide(c, origin, out);
            continue;
        }
        let (pos, s) = if c.node.float {
            let (p, s) = cell(c, inner, default);
            match c
                .node
                .rare()
                .pin
                .as_ref()
                .and_then(|pin| Some((pin, *pins.anchors.get(pin.anchor.as_str())?)))
            {
                // A pin is absolute: the anchor may be anywhere in the tree,
                // so the parent's padding box has nothing to say about it.
                Some((pin, anchor)) => {
                    let s = pin.sized(anchor, s);
                    (pin.place(anchor, s, pins.root, pins.scale), s)
                }
                // A decoration belongs to this local box and its clip; do
                // not pull authored offsets back in like a window popup.
                None if c.node.is_underlay() || c.node.is_overlay() => (
                    [
                        origin[0] + m.padding.left + p[0],
                        origin[1] + m.padding.top + p[1],
                    ],
                    s,
                ),
                // A tooltip or menu offset past the edge is pulled back inside
                // the box it floats in -- floats are painted after the root and
                // clipped by nothing, so off the box is off the window.
                None => (
                    [
                        origin[0] + m.padding.left + inside(p[0], s.width, inner.width),
                        origin[1] + m.padding.top + inside(p[1], s.height, inner.height),
                    ],
                    s,
                ),
            }
        } else {
            let (pos, s) = placed[i].take().ok_or(Error::BudgetExceeded)?;
            match viewport.filter(|_| c.node.sticky) {
                Some(vp) => {
                    let end = at(0.0, 0.0)[vp.vertical as usize] + section.main(vp.vertical);
                    (vp.stick(pos, s, end), s)
                }
                None => (pos, s),
            }
        };
        arrange(c, here, pos, s, pins, viewport, out)?;
    }
    Ok(())
}
