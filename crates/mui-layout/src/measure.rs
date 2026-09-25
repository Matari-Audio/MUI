//! The measure pass: intrinsic sizes, wrapping, grid rows and `fits`.
//!
//! One walk down the tree turning a [`Node`] into a `Measured` tree of
//! sizes. It decides nothing about position -- that is `arrange` -- but it
//! does pick which `fits` candidate survives.

use super::*;

pub(crate) struct Measured<'a, P> {
    pub(crate) node: &'a Node<P>,
    /// What the layout cache holds for this subtree; `None` without one.
    pub(crate) frozen: Option<std::sync::Arc<crate::incremental::Frozen>>,
    /// Slot among the parent's children, so a reordered placement can be
    /// written back to the declaration order the frames keep.
    pub(crate) index: usize,
    pub(crate) gap: f64,
    /// `line_gap` resolved: the gap between wrapped lines and grid rows.
    pub(crate) line_gap: f64,
    pub(crate) padding: Insets,
    pub(crate) size: Size,
    /// The smallest this subtree may be squeezed to: every minimum in it,
    /// summed along the axis they sit on.
    pub(crate) floor: Size,
    /// The children's extent inside the padding, before any definite size
    /// overrides it: what a scroll node lays its children into.
    pub(crate) content: Size,
    /// This subtree's measured size can still change if its main extent does:
    /// a content leaf that wraps, a percentage-sized descendant, an
    /// aspect-ratio node, or a `min_col` grid with no width yet.
    pub(crate) fluid: bool,
    /// A grid's resolved column count, after `min_col`; 0 for anything else.
    /// Measured once so arrange cannot re-derive a different one.
    pub(crate) cols: usize,
    /// A `Fits` node's chosen candidate; 0 for anything else. Picked once, at
    /// measure, for the same reason as `cols`.
    pub(crate) pick: usize,
    /// The nearest definite ancestor extent per axis: what a
    /// [`Len::Container`] on *this* node is a share of.
    pub(crate) container: [Option<f64>; 2],
    pub(crate) children: Vec<Measured<'a, P>>,
}

/// The in-flow children -- everything but the floats -- in placement order.
/// The sort is stable, so `order` only moves what asked to be moved.
pub(crate) fn flow_of<'a, 'm, P>(children: &'m [Measured<'a, P>]) -> Vec<&'m Measured<'a, P>> {
    let mut flow: Vec<_> = children.iter().filter(|c| !c.node.float).collect();
    if !flow.is_sorted_by_key(|c| c.node.order) {
        flow.sort_by_key(|c| c.node.order);
    }
    flow
}

/// The smaller of two optional extents; `None` is unbounded.
fn narrower(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Greedy line breaking: ranges into `flow`, each as long as it fits `avail`.
/// A child wider than the line gets a line of its own.
pub(crate) fn wrap_lines<P>(
    flow: &[&Measured<'_, P>],
    gap: f64,
    vertical: bool,
    avail: f64,
) -> Vec<(usize, usize)> {
    let mut lines = Vec::new();
    let (mut start, mut used) = (0, 0.0);
    for (i, c) in flow.iter().enumerate() {
        let w = c.base(vertical, None);
        if i > start && used + gap + w > avail + 1e-9 {
            lines.push((start, i));
            (start, used) = (i, w);
        } else {
            used += if i > start { gap + w } else { w };
        }
    }
    lines.push((start, flow.len()));
    lines
}

impl<'a, P> Measured<'a, P> {
    pub(crate) fn flow(&self) -> Vec<&Measured<'a, P>> {
        flow_of(&self.children)
    }
    /// Where this child starts before growth or shrink: a percentage of the
    /// parent, a height derived from its aspect, its declared `basis`, or what
    /// it measured. `inner` is `None` while the parent is still hugging.
    pub(crate) fn base(&self, vertical: bool, inner: Option<Size>) -> f64 {
        let fallback = || self.node.basis.unwrap_or(self.size.main(vertical));
        let Some(inner) = inner else {
            return fallback();
        };
        match (self.node.len(vertical), self.aspect_width(inner)) {
            (l @ (Len::Pct(_) | Len::Clamp { .. } | Len::Container(_)), _) => l
                .fixed(inner.main(vertical), self.container[vertical as usize])
                .unwrap_or_else(fallback),
            (_, Some((w, a))) if vertical => w / a,
            _ => fallback(),
        }
    }
    /// Aspect is width-first, like CSS: when the height is `Auto`, the width
    /// is whatever the node would have -- fixed, a share, or the parent's inner
    /// width -- and the height follows.
    pub(crate) fn aspect_width(&self, inner: Size) -> Option<(f64, f64)> {
        let n = self.node;
        let a = n.aspect.filter(|_| matches!(n.height, Len::Auto))?;
        let cap = |v: f64| n.maximum.map_or(v, |m| v.min(m.width));
        Some((
            cap(n
                .width
                .fixed(inner.width, self.container[0])
                .unwrap_or(inner.width)),
            a,
        ))
    }
    /// Size on one axis inside `avail`, given how it is aligned there.
    pub(crate) fn extent(&self, vertical: bool, avail: f64, align: Align) -> f64 {
        let n = self.node;
        // A share of the parent is still never less than this subtree's floor:
        // `height: 50%` on a padded node is a squeeze, not an error.
        let cap = |v: f64| {
            n.maximum
                .map_or(v, |m| v.min(m.cross(!vertical)))
                .max(self.floor.main(vertical))
        };
        match n
            .len(vertical)
            .fixed(avail, self.container[vertical as usize])
        {
            Some(v) => cap(v),
            None if align == Align::Stretch && n.is_container() => cap(avail),
            // Content never keeps a cross extent wider than the room it was
            // given: it would be centred half outside its own parent.
            None => cap(self.size.main(vertical).min(avail)),
        }
    }
}

/// In a cell, `align` governs both axes until `justify` is actually set.
pub(crate) fn cell_default<P>(n: &Node<P>) -> (Align, Align) {
    let y = if n.justify == Justify::Start {
        n.align
    } else {
        n.justify.as_align()
    };
    (n.align, y)
}

pub(crate) fn place(avail: f64, extent: f64, align: Align) -> f64 {
    match align {
        Align::Start => 0.0,
        Align::End => avail - extent,
        _ => (avail - extent) * 0.5,
    }
}

pub(crate) fn validate_node<P>(node: &Node<P>, l: Limits) -> Result<(), Error> {
    let finite = |v: f64| v.is_finite() && (0.0..=l.extent).contains(&v);
    if node.id.as_deref().is_some_and(str::is_empty)
        || !node.minimum.valid(l.extent)
        || node.maximum.is_some_and(|s| !s.valid(l.extent))
        || !node.padding.valid(l.extent)
        || ![node.grow, node.shrink].iter().all(|v| finite(*v))
        || node.basis.is_some_and(|b| !finite(b))
        || !node.width.valid(l.extent)
        || !node.height.valid(l.extent)
        || node.aspect.is_some_and(|a| !(a.is_finite() && a > 0.0))
        || !node
            .offset
            .iter()
            .all(|v| v.is_finite() && v.abs() <= l.extent)
        || matches!(node.kind, Kind::Grid { cols: 0, .. })
        || node.min_col.is_some_and(|v| !finite(v))
    {
        return Err(Error::InvalidValue);
    }
    if let Some(max) = node.maximum {
        if max.width + 1e-9 < node.minimum.width || max.height + 1e-9 < node.minimum.height {
            return Err(Error::InvalidValue);
        }
    }
    Ok(())
}

/// What to call a node in an error. Unnamed nodes are structural, so the
/// nearest named ancestor is the useful thing to point at.
pub(crate) fn label<P>(node: &Node<P>, ancestor: &str) -> String {
    node.id
        .as_ref()
        .map_or_else(|| format!("{ancestor} > unnamed"), Id::to_string)
}

/// Rows of a grid, splitting where the spans fill the column count. A cell
/// wider than the grid takes a row alone.
pub(crate) fn grid_rows<'a, 'm, P>(
    children: &'m [&'m Measured<'a, P>],
    cols: usize,
) -> Vec<&'m [&'m Measured<'a, P>]> {
    let mut rows = Vec::new();
    let (mut start, mut used) = (0, 0);
    for (i, c) in children.iter().enumerate() {
        let span = c.node.span.clamp(1, cols.max(1));
        if i > start && used + span > cols.max(1) {
            rows.push(&children[start..i]);
            (start, used) = (i, span);
        } else {
            used += span;
        }
    }
    if start < children.len() {
        rows.push(&children[start..]);
    }
    rows
}

/// What a parent can promise a child about its outer size before layout runs:
/// a fixed length, a share of a definite inner extent, or -- for a container
/// (or an aspect-ratio node) that will be stretched -- that extent itself.
/// `None` is "measure yourself".
pub(crate) fn offer<P>(
    c: &Node<P>,
    vertical: bool,
    inner: Option<f64>,
    container: Option<f64>,
    stretch: bool,
) -> Option<f64> {
    match c.len(vertical) {
        Len::Px(v) => Some(v),
        Len::Auto => inner.filter(|_| stretch && (c.is_container() || c.aspect.is_some())),
        // A container share needs no definite parent: that is the point of it.
        Len::Container(p) => container.map(|cq| cq * p / 100.0),
        l => inner.and_then(|i| l.fixed(i, container)),
    }
}

/// One measure pass: the budget, the id set and the content measurer.
pub(crate) struct Pass<'a, 'f, P> {
    pub(crate) left: usize,
    pub(crate) limits: Limits,
    pub(crate) scale: SpacingScale,
    pub(crate) keys: rustc_hash::FxHashSet<&'a str>,
    /// Inside a re-measure: ids are already checked and the subtree is being
    /// measured a second time at its final main size.
    pub(crate) redo: bool,
    /// Any node carries a [`Pin`], so arrange runs a second pass with the
    /// anchor frames the first one found.
    pub(crate) pinned: bool,
    pub(crate) measurer: &'f mut dyn FnMut(&P, Option<f64>) -> Size,
    pub(crate) cache: Option<&'f mut LayoutCache>,
    /// The root's padding when its box is given rather than declared; see
    /// [`resolve_boxed_with`]. Taken by the first node measured, the root.
    pub(crate) boxed: Option<Insets>,
}

/// `room` is the narrowest definite inner width above this node (a grid
/// column counts; a scroll node stops it), handed to content so a paragraph
/// can wrap in the first and only pass. It is the room, not the flex share:
/// a row still squeezes its content after measuring.
pub(crate) use crate::incremental::measure_cached as measure;
pub(crate) fn measure_uncached<'a, P>(
    node: &'a Node<P>,
    ancestor: &str,
    definite: [Option<f64>; 2],
    room: Option<f64>,
    container: [Option<f64>; 2],
    depth: usize,
    pass: &mut Pass<'a, '_, P>,
) -> Result<Measured<'a, P>, Error> {
    let l = pass.limits;
    if depth > l.depth || (!pass.redo && pass.left == 0) {
        return Err(Error::BudgetExceeded);
    }
    pass.pinned |= node.pin.is_some();
    let boxed = pass.boxed.take();
    // The node budget counts nodes: a re-measure at the final share visits a
    // subtree again but adds nothing to the tree.
    if !pass.redo {
        pass.left -= 1;
    }
    // The cache's prepass has already validated every node.
    if pass.cache.is_none() {
        validate_node(node, l)?;
    }
    let padding = boxed.unwrap_or_else(|| node.padding(pass.scale));
    let gap = node.gap.resolve(pass.scale);
    let line_gap = node.line_gap.map_or(gap, |g| g.resolve(pass.scale));
    if !([gap, line_gap]
        .iter()
        .all(|g| g.is_finite() && (0.0..=l.extent).contains(g))
        && padding.valid(l.extent))
    {
        return Err(Error::InvalidValue);
    }
    if let Some(id) = node
        .id
        .as_deref()
        .filter(|_| !pass.redo && pass.cache.is_none())
    {
        if !pass.keys.insert(id) {
            return Err(Error::DuplicateKey(id.to_string()));
        }
    }
    let here = node.id.as_deref().unwrap_or(ancestor);
    // Aspect is width-first, like CSS: a definite width settles the height,
    // and only a definite height with no width settles the width.
    // A flex item may use an explicit width as its intrinsic basis while its
    // parent is still measuring the row. Once that item is re-measured at the
    // share the parent actually assigned, the share is the authoritative width
    // for descendants whose layout depends on it. Keep the declared width for
    // the first pass and for nodes that opted out of both growth and shrink;
    // those are genuinely fixed even when a parent has spare room.
    let flex_width = node
        .width
        .px()
        .filter(|_| !(pass.redo && (node.grow > 0.0 || node.shrink > 0.0)));
    // A given box is the offered size on both axes, whatever the node says.
    let mut definite = match boxed {
        Some(_) => definite,
        None => [flex_width.or(definite[0]), node.height.px().or(definite[1])],
    };
    if let Some(a) = node.aspect.filter(|_| boxed.is_none()) {
        match (definite, node.height, node.width) {
            ([Some(w), _], Len::Auto, _) => definite[1] = Some(w / a),
            ([None, Some(h)], _, Len::Auto) => definite[0] = Some(h * a),
            _ => {}
        }
    }
    let inner = [
        definite[0].map(|w| (w - padding.horizontal()).max(0.0)),
        definite[1].map(|h| (h - padding.vertical()).max(0.0)),
    ];
    let room = narrower(room.map(|r| (r - padding.horizontal()).max(0.0)), inner[0]);
    // A grid's column count is settled once, before anything is offered a
    // column's worth of room: `min_col` makes the declared count a ceiling and
    // drops columns until each one clears it. Everything downstream reads
    // `Measured::cols`.
    // ponytail: a hugging grid, with no offered width at all, keeps its
    // declared count. Put a knob bank where its width is definite (a flex
    // share now counts: the flex pass re-measures its items) until a hugging
    // grid learns a width to drop columns against.
    let cols = match node.kind {
        Kind::Grid { cols, .. } => match (node.min_col, inner[0]) {
            (Some(min), Some(w)) if min > 0.0 => {
                (((w + gap) / (min + gap)).floor() as usize).clamp(1, cols)
            }
            _ => cols,
        },
        _ => 0,
    };
    // A box with a definite inner extent is the container everything under it
    // takes a `Len::Container` share of, until a nearer one says otherwise.
    // ponytail: a grid column is room, not a container; make it one if a
    // `cq()` inside a cell ever needs the cell rather than the grid.
    let sub = [inner[0].or(container[0]), inner[1].or(container[1])];
    let mut children = Vec::with_capacity(node.children().len());
    for (index, c) in node.children().iter().enumerate() {
        let align = c.align_self.unwrap_or(node.align);
        let mut child_room = room.filter(|_| !node.scroll);
        let promise = match &node.kind {
            // Candidates are measured at what they would like to be: a
            // stretched one would fit every time and pick itself.
            Kind::Fits(_) if !c.float => [
                offer(c, false, inner[0], sub[0], false),
                offer(c, true, inner[1], sub[1], false),
            ],
            _ if c.float => {
                let (ax, ay) = c.anchor.unwrap_or(cell_default(node));
                [
                    offer(c, false, inner[0], sub[0], ax == Align::Stretch),
                    offer(c, true, inner[1], sub[1], ay == Align::Stretch),
                ]
            }
            Kind::Branch { vertical, .. } => {
                let v = *vertical;
                let cross = offer(
                    c,
                    !v,
                    inner[!v as usize],
                    sub[!v as usize],
                    align == Align::Stretch,
                );
                let main = offer(c, v, inner[v as usize], sub[v as usize], false);
                if v {
                    [cross, main]
                } else {
                    [main, cross]
                }
            }
            Kind::Overlay(_) => {
                let (ax, ay) = c.anchor.unwrap_or(cell_default(node));
                [
                    offer(c, false, inner[0], sub[0], ax == Align::Stretch),
                    offer(c, true, inner[1], sub[1], ay == Align::Stretch),
                ]
            }
            Kind::Grid { .. } => {
                let (ax, _) = c.anchor.unwrap_or(cell_default(node));
                let span = c.span.clamp(1, cols) as f64;
                let col = inner[0].map(|w| {
                    ((w - gap * (cols - 1) as f64).max(0.0) / cols as f64) * span
                        + gap * (span - 1.0)
                });
                child_room = narrower(child_room, col);
                [offer(c, false, col, sub[0], ax == Align::Stretch), None]
            }
            _ => [None; 2],
        };
        let mut m = measure(c, here, promise, child_room, sub, depth + 1, pass)?;
        m.index = index;
        children.push(m);
    }
    // Largest first, so the first candidate that clears both offered axes is
    // the richest one that fits. An axis with no offer never rejects. A float
    // is no candidate: it floats over whichever one wins.
    let pick = match &node.kind {
        Kind::Fits(_) => {
            let fits = |m: &Measured<'_, P>| {
                inner[0].is_none_or(|w| m.size.width <= w + 1e-8)
                    && inner[1].is_none_or(|h| m.size.height <= h + 1e-8)
            };
            let mut candidates = children.iter().enumerate().filter(|(_, c)| !c.node.float);
            let last = candidates.clone().last().map_or(0, |(i, _)| i);
            candidates.find(|(_, c)| fits(c)).map_or(last, |(i, _)| i)
        }
        _ => 0,
    };
    // A flex item only learns its final main size once the row's surplus (or
    // deficit) is dealt, so anything whose measured cross depends on its main
    // -- a paragraph, a `min_col` grid -- is measured again at the share it
    // actually got. Doing it here, inside the one measure pass, is what makes
    // the row's own cross size right; a second solve outside cannot. A
    // wrapping row deals lines, not shares: squeezing its items onto one
    // line would break words instead of wrapping them.
    if let (
        Kind::Branch {
            vertical: false, ..
        },
        Some(avail),
        false,
    ) = (&node.kind, inner[0], node.wrap)
    {
        let shares: Vec<(usize, f64)> = {
            let flow = flow_of(&children);
            if flow.iter().any(|c| c.fluid) {
                // A scrolling row deals what arrange will lay it into: its
                // content where that overflows, so nothing is squeezed to
                // the viewport and wrapped a letter a line.
                let avail = if node.scroll {
                    let bases = flow.iter().map(|c| c.base(false, None)).sum::<f64>();
                    avail.max(bases + gap * flow.len().saturating_sub(1) as f64)
                } else {
                    avail
                };
                let inner = Size::new(avail, inner[1].unwrap_or(0.0));
                let main = distribute(&flow, gap, false, inner);
                flow.iter().map(|c| c.index).zip(main).collect()
            } else {
                Vec::new()
            }
        };
        for (index, main) in shares {
            let c = &node.children()[index];
            let main = c.maximum.map_or(main, |m| main.min(m.width));
            if !children[index].fluid || (children[index].size.width - main).abs() <= 0.5 {
                continue;
            }
            let align = c.align_self.unwrap_or(node.align);
            let cross = offer(c, true, inner[1], sub[1], align == Align::Stretch);
            let was = std::mem::replace(&mut pass.redo, true);
            let m = measure(
                c,
                here,
                [Some(main), cross],
                Some(main),
                sub,
                depth + 1,
                pass,
            );
            pass.redo = was;
            children[index] = m?;
            children[index].index = index;
        }
    }
    let flow = flow_of(&children);
    let max_of =
        |g: &dyn Fn(&Measured<'_, P>) -> f64| flow.iter().map(|c| g(c)).fold(0.0, f64::max);
    let (content, sunk) = match &node.kind {
        Kind::Leaf => (Size::ZERO, Size::ZERO),
        Kind::Content => {
            let s = (pass.measurer)(&node.payload, room);
            if !s.valid(l.extent) {
                return Err(Error::InvalidValue);
            }
            (s, Size::ZERO)
        }
        Kind::Branch { vertical: v, .. } => {
            let v = *v;
            let gaps = flow.len().saturating_sub(1) as f64 * gap;
            // Intrinsic main is not the sum of the children: a child with a
            // `basis` contributes that instead, and then the row has to be
            // wide enough that its *share* of the surplus still clears its
            // content. This is the flex fraction. Without it a hugging row
            // collapses to its non-flexible children and squashes the rest.
            let base = flow.iter().map(|c| c.base(v, None)).sum::<f64>();
            let total_grow = flow.iter().map(|c| c.node.grow).sum::<f64>();
            let surplus = flow
                .iter()
                .filter(|c| c.node.grow > 0.0)
                .map(|c| (c.size.main(v) - c.base(v, None)) * total_grow / c.node.grow)
                .fold(0.0, f64::max);
            let floor_main = flow.iter().map(|c| c.floor.main(v)).sum::<f64>() + gaps;
            // A wrapping row measured under an offered main axis is as tall as
            // its lines. With nothing offered there is nothing to break
            // against, so it stays one line.
            // ponytail: the cross floor stays the single-line one, so a squeeze
            // past the measured width overflows instead of erroring.
            if let Some(avail) = node.wrap.then(|| inner[v as usize]).flatten() {
                let lines = wrap_lines(&flow, gap, v, avail);
                let cross = lines
                    .iter()
                    .map(|(a, b)| {
                        flow[*a..*b]
                            .iter()
                            .map(|c| c.size.cross(v))
                            .fold(0.0, f64::max)
                    })
                    .sum::<f64>()
                    + line_gap * lines.len().saturating_sub(1) as f64;
                let sunk_main = if node.scroll {
                    0.0
                } else {
                    max_of(&|c| c.floor.main(v))
                };
                (
                    Size::axes(avail, cross, v),
                    Size::axes(sunk_main, max_of(&|c| c.floor.cross(v)), v),
                )
            } else {
                (
                    Size::axes(base + surplus + gaps, max_of(&|c| c.size.cross(v)), v),
                    // A scroll node can always be squeezed on its main axis.
                    Size::axes(
                        if node.scroll { 0.0 } else { floor_main },
                        max_of(&|c| c.floor.cross(v)),
                        v,
                    ),
                )
            }
        }
        Kind::Overlay(_) => (
            Size::new(max_of(&|c| c.size.width), max_of(&|c| c.size.height)),
            Size::new(max_of(&|c| c.floor.width), max_of(&|c| c.floor.height)),
        ),
        // The chosen candidate is the whole content; the rest were measured
        // and dropped, and cost nothing but their measure.
        Kind::Fits(_) => children
            .get(pick)
            .map_or((Size::ZERO, Size::ZERO), |c| (c.size, c.floor)),
        Kind::Grid { .. } => {
            let rows = grid_rows(&flow, cols);
            let gaps = |n: f64| (n - 1.0).max(0.0) * gap;
            // With no width offered there is nothing to drop columns against,
            // so the declared count stands -- but `min_col` is still a column
            // minimum, and a hugging parent (a modal, a popover) must widen to
            // it instead of squeezing its cells below it. It widens the hug
            // only: the floor stays the cells' own, so a flex ancestor can
            // still squeeze the grid and have it drop columns.
            let col_min = node.min_col.filter(|_| inner[0].is_none()).unwrap_or(0.0);
            let hug = |g: fn(&Measured<'_, P>) -> Size, col_min: f64| {
                // A spanning cell pays for its span, so its share of one
                // column is what sets the column width.
                let widest = flow
                    .iter()
                    .map(|c| g(c).width / c.node.span.clamp(1, cols) as f64)
                    .fold(col_min, f64::max);
                let tall: f64 = rows
                    .iter()
                    .map(|r| r.iter().map(|c| g(c).height).fold(0.0, f64::max))
                    .sum();
                Size::new(
                    widest * cols as f64 + gaps(cols as f64),
                    tall + (rows.len() as f64 - 1.0).max(0.0) * line_gap,
                )
            };
            (hug(|c| c.size, col_min), hug(|c| c.floor, 0.0))
        }
    };
    let pad = |s: Size| {
        Size::new(
            (s.width + padding.horizontal()).max(node.minimum.width),
            (s.height + padding.vertical()).max(node.minimum.height),
        )
    };
    let hug = pad(content);
    let size = Size::new(
        definite[0].unwrap_or(hug.width),
        definite[1].unwrap_or(hug.height),
    );
    let floor = match node.vertical() {
        Some(v) if node.scroll => Size::axes(
            if v {
                padding.vertical()
            } else {
                padding.horizontal()
            }
            .max(node.minimum.main(v)),
            pad(sunk).cross(v),
            v,
        ),
        // A scrolling stack has no main axis, so it scrolls on both and
        // neither floor holds. Its content floor would otherwise pin it open
        // and squeeze its siblings instead: `stack![body].scroll()` beside a
        // header shrank the header and never overflowed at all.
        None if node.scroll && matches!(node.kind, Kind::Overlay(_)) => Size::new(
            padding.horizontal().max(node.minimum.width),
            padding.vertical().max(node.minimum.height),
        ),
        _ => pad(sunk),
    };
    // A size the node declares itself is also its floor: `.size(10., 10.).pad(6.)`
    // is a 10x10 box with no room inside, not a layout error.
    let cap = |f: f64, l: Len, given: Option<f64>| {
        let own = if boxed.is_some() { given } else { l.px() };
        own.map_or(f, |v| f.min(v))
    };
    let floor = Size::new(
        cap(floor.width, node.width, definite[0]),
        cap(floor.height, node.height, definite[1]),
    );
    if !size.valid(l.extent) {
        return Err(Error::BudgetExceeded);
    }
    if let Some(max) = node.maximum {
        if size.width > max.width + 1e-9 || size.height > max.height + 1e-9 {
            return Err(Error::InsufficientSpace {
                node: label(node, ancestor),
                needs: size,
            });
        }
    }
    // A squeezed flex row re-measures its fluid items, which is the one
    // chance a `fits` inside one gets to pick against its real share.
    // Percentage-like widths and aspect ratios resolve from a flex ancestor's
    // final share. Carry that local dependency through the measured child bit;
    // the existing child propagation handles overlays and scroll wrappers
    // without rescanning their descendants or pulling floats into the chain.
    let width_fluid = matches!(
        node.width,
        Len::Pct(_) | Len::Clamp { .. } | Len::Container(_)
    ) || node.aspect.is_some();
    let wrap_fluid = matches!(
        node.kind,
        Kind::Branch {
            vertical: false,
            ..
        }
    ) && node.wrap;
    let fluid = matches!(node.kind, Kind::Content | Kind::Fits(_))
        || (node.min_col.is_some() && inner[0].is_none() && matches!(node.kind, Kind::Grid { .. }))
        || children.iter().any(|c| c.fluid && !c.node.float)
        || width_fluid
        || wrap_fluid;
    Ok(Measured {
        node,
        frozen: None,
        index: 0,
        fluid,
        gap,
        line_gap,
        padding,
        size,
        floor,
        content,
        cols,
        pick,
        container,
        children,
    })
}
