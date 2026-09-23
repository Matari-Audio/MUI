//! The arrange pass: flex distribution, cells, overlays and pinned floats.
//!
//! Walks the measured tree handing out final rects. Growth and shrink are
//! settled here, and pinned floats are placed afterwards against the
//! anchor frames this pass just produced.

use super::*;

/// Hand every child its main-axis size. Surplus goes out by `grow`; a deficit
/// comes back by `shrink` scaled by basis, which is how flexbox weights it, and
/// never takes a child below its declared `minimum`. Either way a child that
/// can absorb no more is dropped from the pool and the remainder redistributes
/// over the rest, which is what the outer loop is for.
///
/// Only one direction runs. A row that overflows never grew.
pub(crate) fn distribute<P>(
    children: &[&Measured<'_, P>],
    gap: f64,
    vertical: bool,
    inner: Size,
) -> Vec<f64> {
    let inner_main = inner.main(vertical);
    let base: Vec<f64> = children
        .iter()
        .map(|c| c.base(vertical, Some(inner)))
        .collect();
    let mut allocated = base.clone();
    let gaps = gap * children.len().saturating_sub(1) as f64;
    let mut free = inner_main - base.iter().sum::<f64>() - gaps;
    let growing = free > 0.0;
    let room = |i: usize, allocated: &[f64]| {
        let c = children[i];
        let edge = if growing {
            c.node.maximum.map_or(f64::INFINITY, |s| s.main(vertical)) - allocated[i]
        } else {
            allocated[i] - c.floor.main(vertical)
        };
        edge.max(0.0)
    };
    let weight = |i: usize| {
        let c = children[i];
        if growing {
            c.node.grow
        } else {
            c.node.shrink * base[i]
        }
    };
    // ponytail: O(children x clamp rounds), quadratic only when children hit
    // their limits one round at a time. Water-fill in room/weight order
    // (O(n log n)) if rows of thousands of clamped children ever show up.
    for _ in 0..=children.len() {
        let active: Vec<usize> = (0..children.len())
            .filter(|i| weight(*i) > 0.0 && room(*i, &allocated) > 1e-8)
            .collect();
        let total = active.iter().map(|i| weight(*i)).sum::<f64>();
        if free.abs() < 1e-8 || total <= 0.0 {
            break;
        }
        let budget = free;
        for i in active {
            let limit = room(i, &allocated);
            let delta = (budget * weight(i) / total).clamp(-limit, limit);
            allocated[i] += delta;
            free -= delta;
        }
    }
    allocated
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
        Some((w, a)) => (w, w / a),
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
    out: &mut (BTreeMap<Id, Frame>, Vec<Frame>),
) {
    let frame = Frame {
        x: origin[0],
        y: origin[1],
        size: Size::ZERO,
    };
    out.1.push(frame);
    if let Some(id) = m.node.id.clone() {
        out.0.insert(id, frame);
    }
    for c in &m.children {
        hide(c, origin, out);
    }
}

pub(crate) use crate::incremental::arrange_cached as arrange;
pub(crate) fn arrange_uncached<P>(
    m: &Measured<'_, P>,
    ancestor: &str,
    origin: [f64; 2],
    size: Size,
    pins: &Pins<'_>,
    viewport: Option<Viewport>,
    out: &mut (BTreeMap<Id, Frame>, Vec<Frame>),
) -> Result<(), Error> {
    let n = m.node;
    // Content is squeezable -- that is the whole point of shrink -- but the
    // floor is not.
    if size.width + 1e-8 < m.floor.width || size.height + 1e-8 < m.floor.height {
        return Err(Error::InsufficientSpace {
            node: label(n, ancestor),
            needs: Size::ZERO,
        });
    }
    let here = n.id.as_deref().unwrap_or(ancestor);
    let frame = Frame {
        x: origin[0],
        y: origin[1],
        size,
    };
    out.1.push(frame);
    if let Some(id) = n.id.clone() {
        out.0.insert(id, frame);
    }
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
        Kind::Leaf | Kind::Content => {}
        Kind::Overlay(_) => {
            for c in &flow {
                let (p, s) = cell(c, inner, default);
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
        Kind::Grid { .. } => {
            let cols = m.cols;
            let grid = grid_rows(&flow, cols);
            let col_w =
                (inner.width - m.gap * cols.saturating_sub(1) as f64).max(0.0) / cols as f64;
            let heights: Vec<f64> = grid
                .iter()
                .map(|r| r.iter().map(|c| c.size.height).fold(0.0, f64::max))
                .collect();
            let surplus = (inner.height
                - heights.iter().sum::<f64>()
                - m.gap * grid.len().saturating_sub(1) as f64)
                .max(0.0)
                / grid.len().max(1) as f64;
            let mut y = 0.0;
            for (row, h) in grid.iter().zip(heights) {
                let mut col = 0;
                for c in row.iter() {
                    let span = c.node.span.clamp(1, cols.max(1));
                    let cell_size =
                        Size::new(col_w * span as f64 + m.gap * (span - 1) as f64, h + surplus);
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
                            col as f64 * (col_w + m.gap) + safe(p[0], c.node.offset[0]),
                            y + safe(p[1], c.node.offset[1]),
                        ),
                        s,
                    ));
                    col += span;
                }
                y += h + surplus + m.gap;
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
            // the cross floor is still the one it measured. Nothing clips a row,
            // so say it does not fit rather than paint over the next widget.
            if lines.len() > 1 {
                let needed = lines.iter().copied().map(line_cross).sum::<f64>()
                    + m.gap * (lines.len() - 1) as f64;
                if needed > inner.cross(v) + 1e-8 {
                    return Err(Error::InsufficientSpace {
                        node: label(n, ancestor),
                        needs: Size::axes(inner.main(v), needed, v),
                    });
                }
            }
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
                let allocated = distribute(line, m.gap, v, line_inner);
                let count = line.len() as f64;
                let residual = (inner.main(v)
                    - allocated.iter().sum::<f64>()
                    - m.gap * (count - 1.0).max(0.0))
                .max(0.0);
                let (mut cursor, extra) = match n.justify {
                    Justify::Start => (0.0, 0.0),
                    Justify::Center => (residual * 0.5, 0.0),
                    Justify::End => (residual, 0.0),
                    Justify::SpaceBetween if count > 1.0 => (0.0, residual / (count - 1.0)),
                    // CSS: `space-between` on one item is `flex-start`, so a
                    // header row does not jump when its second child is gone.
                    Justify::SpaceBetween => (0.0, 0.0),
                    Justify::SpaceAround => (residual / count * 0.5, residual / count),
                    Justify::SpaceEvenly => (residual / (count + 1.0), residual / (count + 1.0)),
                };
                for (c, main) in line.iter().zip(allocated) {
                    let align = c.node.align_self.unwrap_or(n.align);
                    let cross = match c.aspect_width(line_inner) {
                        Some((w, _)) if v => w,
                        Some((_, a)) => main / a,
                        None => c.extent(!v, line_cross, align),
                    };
                    let cross_pos = line_start + place(line_cross, cross, align);
                    let pos = if v {
                        at(cross_pos, cursor)
                    } else {
                        at(cursor, cross_pos)
                    };
                    placed[c.index] = Some((pos, Size::axes(main, cross, v)));
                    cursor += main + m.gap + extra;
                }
                line_start += line_cross + m.gap;
            }
        }
    }
    for (i, c) in m.children.iter().enumerate() {
        // A candidate that lost keeps its place in the frame list, empty, so
        // a walk of the tree still lines up with it.
        if matches!(n.kind, Kind::Fits(_)) && i != m.pick {
            hide(c, origin, out);
            continue;
        }
        let (pos, s) = if c.node.float {
            let (p, s) = cell(c, inner, default);
            match c
                .node
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
