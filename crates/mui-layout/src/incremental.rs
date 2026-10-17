//! Dependency-aware measurement cache for the existing solver.
//!
//! This is not a second layout algorithm. Cache misses run the original solver.
//! The prepass validates the complete declaration and compares exact shallow
//! layout values plus child revisions; decorative payload changes are excluded
//! by the caller's measurement key. An unchanged tree at an unchanged size
//! returns the last layout as it was; anything else re-arranges from the
//! measurements, reusing every subtree whose revision and constraints held.
use super::*;
use rustc_hash::{FxHashMap, FxHasher};
use std::{
    collections::hash_map::Entry,
    hash::{Hash, Hasher},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutStats {
    pub validated_nodes: usize,
    pub measured_nodes: usize,
    pub measure_hits: usize,
    /// Constraint variants discarded by the bounded measurement cache this
    /// frame. A high count identifies reuse lost to constraint churn.
    pub measure_variant_evictions: usize,
    /// Nodes given frames this frame: zero when the whole layout was reused.
    pub arranged_nodes: usize,
    pub rebound_nodes: usize,
}
#[derive(Clone, Debug, PartialEq)]
struct Stamp {
    shape: Node<Vec<u8>>,
    /// Kept beside `shape` so a warm frame compares it by reference instead
    /// of cloning its box -- and a pin's strings -- into a projection.
    rare: Option<Box<Rare>>,
    children: Vec<u64>,
    revision: u64,
    /// Conservatively true when any length in this subtree uses the inherited
    /// container. Computed with the existing validation walk, never in measure.
    container_dependent: bool,
    /// The `prepare` pass that last visited this address.
    seen: u64,
    /// This revision's measurements, one per constraint it was offered,
    /// Least recently used first. A new revision is a new stamp, so these
    /// never go stale. Hits rotate their existing entry to the most recent end.
    measured: Vec<(MeasureInput, Arc<Frozen>)>,
}
/// One `prepare` walk's state. The buffers live in the cache between frames,
/// so a warm walk allocates nothing.
struct Scan<'k, K> {
    limits: Limits,
    key: &'k mut K,
    pass: u64,
    count: usize,
    /// Child revisions of every node on the current path, stacked.
    revisions: Vec<u64>,
    payload: Vec<u8>,
    /// Exact scroll coordinates in declaration order. Measurement never reads
    /// them, but a changed coordinate still requires fresh arranged frames.
    placement: Vec<[u64; 2]>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MeasureInput {
    /// A flex re-measure at the final share resolves declared widths
    /// differently from the first pass, so the two never share a snapshot.
    redo: bool,
    definite: [Option<u64>; 2],
    room: Option<u64>,
    container: [Option<u64>; 2],
}
impl MeasureInput {
    fn for_node<P>(
        node: &Node<P>,
        mut definite: [Option<f64>; 2],
        mut room: Option<f64>,
        mut container: [Option<f64>; 2],
        redo: bool,
        boxed: bool,
        scale: SpacingScale,
    ) -> Self {
        // Aspect derives another axis and boxed roots consume their raw box.
        // Keep those inputs exact rather than duplicating the aspect solver.
        if !boxed && node.rare().aspect.is_none() {
            definite = measure::definite_offer(node, definite, redo, false);
            for axis in 0..2 {
                // A definite inner axis shadows the inherited container for
                // every descendant. This node still reads it for CQ units;
                // all other lengths use its own size or its parent's share.
                if definite[axis].is_some() && !matches!(node.len(axis == 1), Len::Container(_)) {
                    container[axis] = None;
                }
            }
            if definite[0].is_some() && !matches!(node.width, Len::Container(_)) {
                // Wider parent offers wrap identically at this node's own
                // definite inner width. Use the solver's exact subtraction,
                // clamp and cap, retaining unresolved/CQ widths verbatim.
                room = measure::wrapping_room(room, definite[0], node.padding(scale).horizontal());
            }
        }
        Self {
            redo,
            definite: definite.map(|v| v.map(f64::to_bits)),
            room: room.map(f64::to_bits),
            container: container.map(|v| v.map(f64::to_bits)),
        }
    }
}
/// A measured subtree with its node references dropped. Children are shared,
/// so storing a parent costs one record, not its whole subtree again.
#[derive(Debug, PartialEq)]
pub(crate) struct Frozen {
    cost: usize,
    gap: f64,
    line_gap: f64,
    padding: Insets,
    size: Size,
    extent_limit: f64,
    floor: Size,
    content: Size,
    fluid: [bool; 2],
    definite_height: bool,
    cols: usize,
    columns: measure::Tracks,
    rows: measure::Tracks,
    pick: usize,
    container: [Option<f64>; 2],
    children: Vec<Arc<Frozen>>,
}
/// This is local arrange scratch, not a recursively inline field of Measured.
/// Keep bound scratch off the recursive arrange stack.
pub(crate) enum BoundChildren<'m, 'a, P> {
    Borrowed(&'m [Measured<'a, P>]),
    Bound(Box<smallvec::SmallVec<[Measured<'a, P>; 4]>>),
}
impl<'a, P> std::ops::Deref for BoundChildren<'_, 'a, P> {
    type Target = [Measured<'a, P>];
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(children) => children,
            Self::Bound(children) => children,
        }
    }
}

impl Frozen {
    fn bind<'a, P>(
        this: &Arc<Self>,
        n: &'a Node<P>,
        stats: &mut LayoutStats,
        deep: bool,
    ) -> Measured<'a, P> {
        stats.rebound_nodes += 1;
        Measured {
            node: n,
            frozen: Some(this.clone()),
            index: 0,
            fluid: this.fluid,
            definite_height: this.definite_height,
            gap: this.gap,
            line_gap: this.line_gap,
            padding: this.padding,
            size: this.size,
            extent_limit: this.extent_limit,
            floor: this.floor,
            content: this.content,
            cols: this.cols,
            columns: this.columns.clone(),
            rows: this.rows.clone(),
            pick: this.pick,
            container: this.container,
            children: if deep {
                this.children
                    .iter()
                    .zip(n.children())
                    .enumerate()
                    .map(|(i, (c, n))| {
                        let mut m = Self::bind(c, n, stats, true);
                        m.index = i;
                        m
                    })
                    .collect()
            } else {
                Vec::new()
            },
        }
    }
    /// Borrow eager bindings, or bind only the cached branch's immediate
    /// children. The bound scratch is boxed to keep the enum and recursive
    /// arrange frames small; short rows retain inline storage inside that box.
    /// Pin replay continues to use its eager measured descendants.
    pub(crate) fn children<'m, 'a, P>(m: &'m Measured<'a, P>) -> BoundChildren<'m, 'a, P> {
        if !m.children.is_empty() || m.node.children().is_empty() {
            return BoundChildren::Borrowed(&m.children);
        }
        let Some(frozen) = &m.frozen else {
            return BoundChildren::Borrowed(&m.children);
        };
        BoundChildren::Bound(Box::new(frozen.bindings(m.node)))
    }

    fn bindings<'a, P, C: FromIterator<Measured<'a, P>>>(&self, node: &'a Node<P>) -> C {
        let mut ignored = LayoutStats::default();
        self.children
            .iter()
            .zip(node.children())
            .enumerate()
            .map(|(index, (frozen, node))| {
                let mut child = Self::bind(frozen, node, &mut ignored, false);
                child.index = index;
                child
            })
            .collect()
    }

    /// Flex remeasurement detaches cached metrics before changing its basis.
    /// It still needs an owned, mutable child list; only arrange can borrow.
    pub(crate) fn expand<'a, P>(m: &Measured<'a, P>) -> Option<Measured<'a, P>> {
        if !m.children.is_empty() || m.node.children().is_empty() {
            return None;
        }
        let frozen = m.frozen.as_ref()?;
        let mut ignored = LayoutStats::default();
        let mut expanded = Self::bind(frozen, m.node, &mut ignored, false);
        expanded.index = m.index;
        expanded.children = frozen.bindings(m.node);
        Some(expanded)
    }

    fn freeze<P>(m: &Measured<'_, P>, cost: usize) -> Arc<Self> {
        Arc::new(Self {
            cost,
            gap: m.gap,
            line_gap: m.line_gap,
            padding: m.padding,
            size: m.size,
            extent_limit: m.extent_limit,
            floor: m.floor,
            content: m.content,
            fluid: m.fluid,
            definite_height: m.definite_height,
            cols: m.cols,
            columns: m.columns.clone(),
            rows: m.rows.clone(),
            pick: m.pick,
            container: m.container,
            // Every child came through `measure_cached`, which froze it.
            children: m
                .children
                .iter()
                .map(|c| c.frozen.clone().unwrap_or_else(|| Self::freeze(c, 0)))
                .collect(),
        })
    }
}
fn fx(v: impl Hash) -> u64 {
    let mut h = FxHasher::default();
    v.hash(&mut h);
    h.finish()
}
#[derive(Debug, Default)]
pub struct LayoutCache {
    /// Keyed by a hash of the node's id, or of its child-index path if it
    /// has none. See `scan` for why a collision cannot give a wrong answer.
    stamps: FxHashMap<u64, Stamp>,
    /// Node address to stamp address, for this pass's tree only.
    pointers: FxHashMap<usize, u64>,
    serial: u64,
    pass: u64,
    revisions: Vec<u64>,
    payload: Vec<u8>,
    placement: Vec<[u64; 2]>,
    placement_scratch: Vec<[u64; 2]>,
    placement_revision: u64,
    context: Option<(Limits, SpacingScale)>,
    pinned: bool,
    /// Measurement, placement and offered-size revisions must all agree to
    /// reuse the complete layout rather than arrange the measured tree again.
    last: Option<(u64, u64, Option<[u64; 2]>, Layout)>,
    /// The size offered last call, cached or not.
    sized: Option<Option<[u64; 2]>>,
    stats: LayoutStats,
    /// None in ordinary operation: no trace history or export buffers exist.
    trace: Option<Box<crate::diagnostics::Recorder>>,
}
impl LayoutCache {
    pub fn clear(&mut self) {
        *self = Self {
            trace: self.trace.take(),
            ..Self::default()
        };
    }
    pub fn stats(&self) -> LayoutStats {
        self.stats
    }
    /// Start a fresh bounded work trace, or discard it. Enabled tracing adds
    /// history-search overhead; its timings are not a performance baseline.
    pub fn trace_requests(&mut self, enabled: bool) {
        self.trace = enabled.then(|| Box::new(crate::diagnostics::Recorder::new()));
    }
    /// Export owned records and disable collection. Geometry caches are kept.
    pub fn take_request_trace(&mut self) -> Option<LayoutRequestTrace> {
        self.trace.take().map(|trace| trace.export())
    }
    /// Constraint variants kept per node: a first pass, a flex re-measure and
    /// a couple of sizes to go back to. Fewer trades hit rate for memory,
    /// never correctness.
    const VARIANTS: usize = 4;
    // Nested Fits/flex branches can visit dozens of exact offers in one
    // solve. Retain that bounded working set instead of evicting each pass's
    // inputs before the next pass. Leaves keep their small four-entry cache.
    const BRANCH_VARIANTS: usize = 64;
    /// Validates the tree and returns its measurement and placement revisions.
    fn prepare<P>(
        &mut self,
        n: &Node<P>,
        limits: Limits,
        scale: SpacingScale,
        key: &mut impl FnMut(&P, &mut Vec<u8>),
    ) -> Result<(u64, u64), Error> {
        // A context change invalidates every metric. Limits cannot be bypassed.
        if self.context != Some((limits, scale)) {
            let mut trace = self.trace.take();
            if let Some(trace) = trace.as_mut() {
                trace.context_changed();
            }
            *self = Self {
                context: Some((limits, scale)),
                sized: self.sized,
                trace,
                ..Self::default()
            };
        }
        if let Some(trace) = self.trace.as_mut() {
            trace.frame(false);
        }
        self.pointers.clear();
        self.pinned = false;
        self.stats = LayoutStats::default();
        self.pass = self.pass.wrapping_add(1);
        self.placement_scratch.clear();
        let mut scan = Scan {
            limits,
            key,
            pass: self.pass,
            count: 0,
            revisions: std::mem::take(&mut self.revisions),
            payload: std::mem::take(&mut self.payload),
            placement: std::mem::take(&mut self.placement_scratch),
        };
        let walked = self.scan(n, 0, 0, &mut scan);
        (self.revisions, self.payload, self.placement_scratch) =
            (scan.revisions, scan.payload, scan.placement);
        // A failed walk returns before popping its path's revisions.
        self.revisions.clear();
        let revision = walked?.0;
        if self.placement != self.placement_scratch {
            self.placement_revision = self
                .placement_revision
                .checked_add(1)
                .ok_or(Error::RevisionExhausted)?;
            std::mem::swap(&mut self.placement, &mut self.placement_scratch);
        }
        self.placement_scratch.clear();
        self.stamps.retain(|_, s| s.seen == scan.pass);
        self.stats.validated_nodes = scan.count;
        Ok((revision, self.placement_revision))
    }
    /// Validates `n`'s subtree and returns its revision: unchanged while its
    /// shallow projection and every child revision are, so equal revisions
    /// mean equal subtrees. That makes the u64 address a hint, not an
    /// identity: two nodes that collide on it probe apart, and a node that
    /// lands on a stranger's stamp just fails the comparison and misses.
    fn scan<P, K: FnMut(&P, &mut Vec<u8>)>(
        &mut self,
        n: &Node<P>,
        path: u64,
        depth: usize,
        s: &mut Scan<'_, K>,
    ) -> Result<(u64, bool), Error> {
        if depth > s.limits.depth || s.count >= s.limits.nodes {
            return Err(Error::BudgetExceeded);
        }
        s.count += 1;
        // Scroll coordinates are excluded from measurement equality, so they
        // must still be checked on every warm scan, not only changed stamps.
        if !n.scrolled.iter().all(|v| v.is_finite()) {
            return Err(Error::InvalidValue);
        }
        s.placement.push(n.scrolled.map(f64::to_bits));
        self.pinned |= n.rare().pin.is_some();
        let base = s.revisions.len();
        let mut container_dependent =
            matches!(n.width, Len::Container(_)) || matches!(n.height, Len::Container(_));
        for (i, c) in n.children().iter().enumerate() {
            let (revision, child_container) = self.scan(c, fx((path, i)), depth + 1, s)?;
            container_dependent |= child_container;
            s.revisions.push(revision);
        }
        let mut address = n.key().map_or(path, fx);
        // A slot already visited this pass belongs to another node: the same
        // id is a duplicate, anything else a collision to probe past.
        while let Some(taken) = self.stamps.get(&address).filter(|t| t.seen == s.pass) {
            if let Some(id) = n.key().filter(|_| taken.shape.id == n.id) {
                return Err(Error::DuplicateKey(id.to_owned()));
            }
            address = address.wrapping_add(1);
        }
        s.payload.clear();
        (s.key)(&n.payload, &mut s.payload);
        let shape = projection(n, std::mem::take(&mut s.payload));
        let children = &s.revisions[base..];
        let revision = match self.stamps.entry(address) {
            Entry::Occupied(mut o)
                if o.get().shape == shape
                    && o.get().rare == n.rare
                    && o.get().children == children =>
            {
                s.payload = shape.payload;
                o.get_mut().seen = s.pass;
                o.get().revision
            }
            entry => {
                // A matching stamp was validated when it was made, under the
                // same limits: only a changed node needs checking.
                measure::validate_node(n, s.limits)?;
                self.serial = self.serial.checked_add(1).ok_or(Error::RevisionExhausted)?;
                let stamp = Stamp {
                    shape,
                    rare: n.rare.clone(),
                    children: children.to_vec(),
                    revision: self.serial,
                    container_dependent,
                    seen: s.pass,
                    measured: Vec::new(),
                };
                match entry {
                    // Hand the old payload buffer back for the next node.
                    Entry::Occupied(mut o) => s.payload = o.insert(stamp).shape.payload,
                    Entry::Vacant(v) => {
                        v.insert(stamp);
                    }
                }
                self.serial
            }
        };
        s.revisions.truncate(base);
        self.pointers.insert(n as *const Node<P> as usize, address);
        Ok((revision, container_dependent))
    }
    /// The stamp `scan` gave this node. `None` for a node `scan` did not
    /// visit: it is measured uncached rather than panicking.
    fn stamp<P>(&mut self, n: &Node<P>) -> Option<&mut Stamp> {
        let address = self.pointers.get(&(n as *const Node<P> as usize))?;
        self.stamps.get_mut(address)
    }
}
/// Complete shallow measurement projection; destructuring without `..` makes
/// adding a layout field a compile-time prompt to update invalidation. Payload values
/// are exact bytes, not a hash used as a substitute for equality. The pin is
/// left out: [`Stamp`] keeps it and `scan` compares it by reference.
fn projection<P>(n: &Node<P>, payload: Vec<u8>) -> Node<Vec<u8>> {
    let Node {
        id,
        kind,
        payload: _,
        gap,
        padding,
        pad,
        minimum,
        width,
        height,
        grow,
        basis,
        shrink,
        align,
        align_self,
        justify,
        anchor,
        offset,
        rare: _,
        scroll,
        clip,
        scrolled: _,
        sticky,
        float,
        wrap,
        span,
        order,
    } = n;
    let kind = match kind {
        Kind::Leaf => Kind::Leaf,
        Kind::Content(_) => Kind::Content(Vec::new()),
        Kind::Branch { vertical, .. } => Kind::Branch {
            vertical: *vertical,
            children: Vec::new(),
        },
        Kind::Overlay(_) => Kind::Overlay(Vec::new()),
        Kind::Grid { cols, .. } => Kind::Grid {
            cols: *cols,
            children: Vec::new(),
        },
        Kind::Fits(_) => Kind::Fits(Vec::new()),
    };
    Node {
        id: id.clone(),
        kind,
        payload,
        gap: *gap,
        padding: *padding,
        pad: *pad,
        minimum: *minimum,
        width: *width,
        height: *height,
        grow: *grow,
        basis: *basis,
        shrink: *shrink,
        align: *align,
        align_self: *align_self,
        justify: *justify,
        anchor: *anchor,
        offset: *offset,
        rare: None,
        scroll: *scroll,
        clip: *clip,
        // Kept exactly in the separate placement receipt. The original
        // measure pass does not consume these arrange-only coordinates.
        scrolled: [0.; 2],
        sticky: *sticky,
        float: *float,
        wrap: *wrap,
        span: *span,
        order: *order,
    }
}
/// Exact content-measurement keys are mandatory. Every captured external font,
/// locale or measurement policy must be included or explicitly clear the cache.
/// Decorator colours should not appear in the key. `key` appends a payload's
/// key to a buffer the cache reuses, so a warm frame allocates none.
pub fn resolve_cached_with<P, M: Into<Intrinsic>>(
    root: &Node<P>,
    offered: Option<Size>,
    limits: Limits,
    scale: SpacingScale,
    cache: &mut LayoutCache,
    mut key: impl FnMut(&P, &mut Vec<u8>),
    measurer: impl FnMut(&P, Option<f64>) -> M,
) -> Result<Layout, Error> {
    if !limits.extent.is_finite()
        || limits.extent <= 0.0
        || limits.nodes == 0
        || limits.depth > 256
        || !scale.is_valid()
    {
        return Err(Error::InvalidValue);
    }
    // A size that just changed is likely to change again next frame -- a
    // window being dragged -- and a resize misses nearly every measurement,
    // so it solves bare rather than pay to validate and store what the next
    // size cannot use. The cache starts again once the size holds.
    let offered_bits = offered.map(|s| [s.width.to_bits(), s.height.to_bits()]);
    if cache.sized.replace(offered_bits) != Some(offered_bits) {
        if let Some(trace) = cache.trace.as_mut() {
            trace.frame(true);
        }
        let layout = super::resolve_impl(root, offered, limits, scale, measurer, None, None)?;
        cache.stats = LayoutStats {
            arranged_nodes: layout.all().len(),
            ..LayoutStats::default()
        };
        return Ok(layout);
    }
    let (revision, placement) = cache.prepare(root, limits, scale, &mut key)?;
    if let Some((r, p, o, layout)) = &cache.last
        && (*r, *p, *o) == (revision, placement, offered_bits)
    {
        if let Some(trace) = cache.trace.as_mut() {
            trace.whole_layout_reused();
        }
        return Ok(layout.clone());
    }
    let layout = super::resolve_impl(root, offered, limits, scale, measurer, Some(cache), None)?;
    cache.stats.arranged_nodes = layout.all().len();
    cache.last = Some((revision, placement, offered_bits, layout.clone()));
    Ok(layout)
}
/// Hook around the ORIGINAL measure implementation. Re-measure requests under
/// different consumed flex constraints receive different keys. Fixed axes
/// discard only offers replaced by the original solver's own decision.
pub(crate) fn measure_cached<'a, P>(
    node: &'a Node<P>,
    ancestor: &str,
    definite: [Option<f64>; 2],
    room: Option<f64>,
    container: [Option<f64>; 2],
    depth: usize,
    pass: &mut Pass<'a, '_, P>,
) -> Result<Measured<'a, P>, Error> {
    let Some(cache) = pass.cache.as_deref_mut() else {
        return measure::measure_uncached(node, ancestor, definite, room, container, depth, pass);
    };
    let mut key = MeasureInput::for_node(
        node,
        definite,
        room,
        container,
        pass.redo,
        pass.boxed.is_some(),
        pass.scale,
    );
    if cache
        .stamp(node)
        .is_some_and(|stamp| !stamp.container_dependent)
    {
        // Only Len::Container reads these inherited extents. A subtree without
        // any CQ length is measured and arranged identically at different
        // inherited containers: its own offers, room, padding and aspect still
        // remain exact in the key. Frozen::container is then unconsumed data.
        // A later CQ declaration changes the subtree revision before reuse.
        key.container = [None; 2];
    }
    let snapshot = cache.stamp(node).and_then(|s| {
        let at = s.measured.iter().position(|(k, _)| *k == key)?;
        let snapshot = s.measured[at].1.clone();
        // Four entries stay in their existing allocation. Refreshing a
        // hit prevents an older, frequently used constraint from being
        // evicted merely because it was inserted before a colder one.
        s.measured[at..].rotate_left(1);
        Some(snapshot)
    });
    let traced = if let Some(trace) = &mut cache.trace {
        let stamp = cache
            .pointers
            .get(&(node as *const Node<P> as usize))
            .and_then(|&address| cache.stamps.get(&address).map(|s| (address, s.revision)));
        let started = (cache.stats.measured_nodes, cache.stats.rebound_nodes);
        let ticket = trace.begin(
            stamp,
            node.id.as_ref(),
            ancestor,
            depth,
            LayoutRequestInput {
                redo: key.redo,
                definite: key.definite,
                room: key.room,
                container: key.container,
            },
            snapshot.is_some(),
        );
        Some((ticket, started))
    } else {
        None
    };
    if let Some(snapshot) = snapshot {
        if !pass.redo {
            if snapshot.cost > pass.left {
                finish_request(cache, traced, false, true);
                return Err(Error::BudgetExceeded);
            }
            pass.left -= snapshot.cost;
        }
        pass.pinned |= cache.pinned;
        cache.stats.measure_hits += 1;
        let bound = Frozen::bind(&snapshot, node, &mut cache.stats, cache.pinned);
        finish_request(cache, traced, false, false);
        return Ok(bound);
    }
    cache.stats.measured_nodes += 1;
    let before = pass.left;
    let measured =
        measure::measure_uncached(node, ancestor, definite, room, container, depth, pass);
    if let Some(cache) = pass.cache.as_deref_mut() {
        finish_request(cache, traced, true, measured.is_err());
    }
    let mut m = measured?;
    let frozen = Frozen::freeze(&m, before - pass.left);
    m.frozen = Some(frozen.clone());
    if let Some(cache) = pass.cache.as_deref_mut() {
        let mut evicted = false;
        if let Some(stamp) = cache.stamp(node) {
            let limit = if stamp.children.is_empty() {
                LayoutCache::VARIANTS
            } else {
                LayoutCache::BRANCH_VARIANTS
            };
            let measured = &mut stamp.measured;
            if measured.len() >= limit {
                measured.remove(0);
                evicted = true;
            }
            measured.push((key, frozen));
        }
        cache.stats.measure_variant_evictions += usize::from(evicted);
    }
    Ok(m)
}

fn finish_request(
    cache: &mut LayoutCache,
    traced: Option<(crate::diagnostics::Ticket, (usize, usize))>,
    missed: bool,
    failed: bool,
) {
    if let Some((ticket, (measured, rebound))) = traced {
        let children = cache
            .stats
            .measured_nodes
            .saturating_sub(measured + usize::from(missed));
        let rebound = cache.stats.rebound_nodes.saturating_sub(rebound);
        if let Some(trace) = cache.trace.as_mut() {
            trace.finish(ticket, children, rebound, failed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A node the scan never stamped has no stamp: the measure runs
    /// uncached instead of panicking on a missing map entry.
    #[test]
    fn an_unscanned_node_has_no_stamp() {
        let mut cache = LayoutCache::default();
        assert!(cache.stamp(&label("a", "x")).is_none());
    }
    #[test]
    fn variant_churn_counts_evictions_without_counting_cache_hits() {
        let n = label("text", "A measured label");
        let mut cache = LayoutCache::default();
        let limits = Limits::default();
        cache
            .prepare(&n, limits, SpacingScale::DEFAULT, &mut key)
            .unwrap();
        let mut measurer = crate::measure::intrinsic(metric);
        let mut pass = Pass {
            left: limits.nodes,
            limits,
            scale: SpacingScale::DEFAULT,
            keys: rustc_hash::FxHashSet::default(),
            redo: false,
            pinned: false,
            measurer: &mut measurer,
            cache: Some(&mut cache),
            boxed: None,
        };
        for i in 0..LayoutCache::VARIANTS + 2 {
            let width = 32. + i as f64;
            let measured = measure_cached(
                &n,
                "",
                [Some(width), None],
                Some(width),
                [Some(width), None],
                0,
                &mut pass,
            )
            .unwrap();
            assert_eq!(measured.size.width, width);
        }
        assert_eq!(
            pass.cache.as_ref().unwrap().stats.measure_variant_evictions,
            2
        );
        let width = 32. + (LayoutCache::VARIANTS + 1) as f64;
        measure_cached(
            &n,
            "",
            [Some(width), None],
            Some(width),
            [Some(width), None],
            0,
            &mut pass,
        )
        .unwrap();
        let stats = pass.cache.as_ref().unwrap().stats;
        assert_eq!(stats.measure_hits, 1);
        assert_eq!(stats.measure_variant_evictions, 2);
    }

    #[test]
    fn hot_constraint_hits_survive_a_five_key_cycle_with_the_same_four_entries() {
        let n = label("text", "A measured label");
        let mut cache = LayoutCache::default();
        let limits = Limits::default();
        cache.trace_requests(true);
        cache
            .prepare(&n, limits, SpacingScale::DEFAULT, &mut key)
            .unwrap();
        let mut measurer = crate::measure::intrinsic(metric);
        let mut pass = Pass {
            left: limits.nodes,
            limits,
            scale: SpacingScale::DEFAULT,
            keys: rustc_hash::FxHashSet::default(),
            redo: false,
            pinned: false,
            measurer: &mut measurer,
            cache: Some(&mut cache),
            boxed: None,
        };
        // A stays hot while B/C/D/E cycle. The old insertion-order selector
        // does not refresh A on its hits and eventually discards it.
        let widths = [
            32., 33., 32., 34., 32., 35., 32., 36., 32., 33., 32., 34., 32., 35., 32., 36., 32.,
        ];
        let mut fifo = Vec::new();
        let mut fifo_hits = 0;
        let mut hot_snapshot = None;
        let mut capacity = None;
        for width in widths {
            if fifo.contains(&width) {
                fifo_hits += 1;
            } else {
                if fifo.len() == LayoutCache::VARIANTS {
                    fifo.remove(0);
                }
                fifo.push(width);
            }
            let measured = measure_cached(
                &n,
                "root",
                [Some(width), None],
                Some(width),
                [Some(width), None],
                0,
                &mut pass,
            )
            .unwrap();
            assert_eq!(measured.size, metric(&n.payload, Some(width)));
            let cache = pass.cache.as_deref_mut().unwrap();
            let variants = &cache.stamp(&n).unwrap().measured;
            assert!(variants.len() <= LayoutCache::VARIANTS);
            if variants.len() == LayoutCache::VARIANTS {
                let allocated = capacity.get_or_insert(variants.capacity());
                assert_eq!(
                    variants.capacity(),
                    *allocated,
                    "hit promotion never grows storage"
                );
            }
            let hot = &variants
                .iter()
                .find(|(k, _)| k.definite[0] == Some(32_f64.to_bits()))
                .unwrap()
                .1;
            if let Some(original) = &hot_snapshot {
                assert!(
                    Arc::ptr_eq(original, hot),
                    "the hot snapshot is retained, not remeasured"
                );
            } else {
                hot_snapshot = Some(hot.clone());
            }
            assert_eq!(
                variants.last().unwrap().0.definite[0],
                Some(width.to_bits())
            );
        }
        let cache = pass.cache.as_deref_mut().unwrap();
        assert_eq!(cache.stats.measure_hits, 8, "every repeated A request hits");
        assert!(cache.stats.measure_hits > fifo_hits);
        let trace = cache.take_request_trace().unwrap();
        assert_eq!(trace.hits, cache.stats.measure_hits as u64);
        assert_eq!(trace.misses, cache.stats.measured_nodes as u64);
        let revision = &trace.nodes[0].revisions[0];
        assert_eq!(revision.keys.len(), 5);
        assert_eq!(revision.key_overflow_requests, 0);
        assert_eq!(revision.keys[0].misses, 1);
        assert_eq!(revision.keys[0].hits, 8);
        assert_eq!(
            revision.miss_reuse_distances[..LayoutCache::VARIANTS]
                .iter()
                .sum::<u64>(),
            0
        );

        // A remains a lookup hit, but retained data never bypasses the host's
        // node budget. Promotion changes replacement order, not admission.
        pass.left = 0;
        assert!(matches!(
            measure_cached(
                &n,
                "root",
                [Some(32.), None],
                Some(32.),
                [Some(32.), None],
                0,
                &mut pass,
            ),
            Err(Error::BudgetExceeded)
        ));
    }

    #[test]
    fn promoted_constraint_variants_keep_exact_fresh_frames_and_warm_validation() {
        let n = scroll_fixture();
        let offers: Vec<_> = [
            300., 320., 300., 340., 300., 360., 300., 380., 300., 320., 300.,
        ]
        .into_iter()
        .map(|w| Size::new(w, 100.))
        .collect();
        offer_oracle(&n, &offers, None);
        let mut cache = LayoutCache::default();
        prime(&n, 400., &mut cache);
        let mut invalid = n.clone();
        invalid.children_mut()[0].width = Len::Px(f64::NAN);
        let actual = resolve_cached_with(
            &invalid,
            Some(Size::new(400., 100.)),
            Limits::default(),
            SpacingScale::DEFAULT,
            &mut cache,
            key,
            metric,
        );
        let fresh = resolve_with(
            &invalid,
            Some(Size::new(400., 100.)),
            Limits::default(),
            SpacingScale::DEFAULT,
            metric,
        );
        assert!(fresh.is_err());
        assert_eq!(actual, fresh);
        for limits in [
            Limits {
                nodes: 2,
                ..Limits::default()
            },
            Limits {
                depth: 1,
                ..Limits::default()
            },
        ] {
            let actual = resolve_cached_with(
                &n,
                Some(Size::new(400., 100.)),
                limits,
                SpacingScale::DEFAULT,
                &mut cache,
                key,
                metric,
            );
            let fresh = resolve_with(
                &n,
                Some(Size::new(400., 100.)),
                limits,
                SpacingScale::DEFAULT,
                metric,
            );
            assert!(fresh.is_err());
            assert_eq!(actual, fresh);
        }
    }

    #[test]
    fn request_trace_records_exact_fifo_revisits_without_changing_measurements() {
        let n = label("text", "A measured label");
        let mut cache = LayoutCache::default();
        assert!(cache.trace.is_none());
        assert!(cache.take_request_trace().is_none());
        cache.trace_requests(false);
        assert!(
            cache.trace.is_none(),
            "disabled collection owns no recorder or history"
        );
        cache.trace_requests(true);
        let limits = Limits::default();
        cache
            .prepare(&n, limits, SpacingScale::DEFAULT, &mut key)
            .unwrap();
        let mut measurer = crate::measure::intrinsic(metric);
        let mut pass = Pass {
            left: limits.nodes,
            limits,
            scale: SpacingScale::DEFAULT,
            keys: rustc_hash::FxHashSet::default(),
            redo: false,
            pinned: false,
            measurer: &mut measurer,
            cache: Some(&mut cache),
            boxed: None,
        };
        for width in [32., 33., 34., 35., 36., 37., 32., 37.] {
            let measured = measure_cached(
                &n,
                "root",
                [Some(width), None],
                Some(width),
                [Some(width), None],
                0,
                &mut pass,
            )
            .unwrap();
            assert_eq!(measured.size, metric(&n.payload, Some(width)));
        }
        let cache = pass.cache.as_deref_mut().unwrap();
        assert_eq!(cache.stats.measure_variant_evictions, 3);
        let trace = cache.take_request_trace().unwrap();
        assert_eq!((trace.requests, trace.hits, trace.misses), (8, 1, 7));
        assert_eq!(trace.depths[0].requests, trace.requests);
        assert_eq!(trace.nodes.len(), 1);
        let revision = &trace.nodes[0].revisions[0];
        assert_eq!(revision.keys.len(), 6);
        assert_eq!(revision.miss_reuse_distances[5], 1);
        assert_eq!(revision.lru_reuse_distances[1], 1);
        assert_eq!(revision.repeated_misses, 1);
        assert_eq!(revision.keys[0].input.definite[0], Some(32_f64.to_bits()));
        assert!(
            cache.trace.is_none(),
            "export stops capture and keeps the geometry cache"
        );
        assert_eq!(
            cache.stamp(&n).unwrap().measured.len(),
            LayoutCache::VARIANTS
        );
    }

    #[test]
    fn tracing_preserves_frames_stats_and_persistent_stamp_identity_across_rebuilt_trees() {
        let original = Box::new(scroll_fixture());
        let mut plain = LayoutCache::default();
        let mut traced = LayoutCache::default();
        traced.trace_requests(true);
        for _ in 0..2 {
            assert_eq!(
                cached(&original, 400., &mut traced),
                cached(&original, 400., &mut plain)
            );
            assert_eq!(traced.stats(), plain.stats());
        }
        for scroll in [3., 11., 29.] {
            let mut rebuilt = Box::new((*original).clone());
            assert!(!std::ptr::eq(&*rebuilt, &*original));
            rebuilt.children_mut()[0].scrolled = [0., scroll];
            for _ in 0..2 {
                let actual = cached(&rebuilt, 400., &mut traced);
                assert_eq!(actual, cached(&rebuilt, 400., &mut plain));
                assert_eq!(actual, oracle(&rebuilt, 400.));
                assert_eq!(traced.stats(), plain.stats());
            }
        }
        let trace = traced.take_request_trace().unwrap();
        assert!(trace.whole_layout_reuses >= 3);
        assert_eq!(trace.bare_resolves, 1);
        assert_eq!(trace.requests, trace.hits + trace.misses);
        assert_eq!(
            trace.requests,
            trace.depths.iter().map(|d| d.requests).sum::<u64>()
        );
        assert_eq!(
            trace.node_overflow_requests
                + trace.revision_overflow_requests
                + trace.key_overflow_requests,
            0
        );
        assert_eq!(trace.failed_requests, 0);
        let roots: Vec<_> = trace
            .nodes
            .iter()
            .filter(|n| n.id.as_deref() == Some("root"))
            .collect();
        assert_eq!(
            roots.len(),
            1,
            "rebuilt pointers retain one exact cache stamp slot"
        );
        assert_eq!(roots[0].revisions.len(), 1);
        assert!(roots[0].child_misses > 0);
        assert!(roots[0].rebound_nodes > 0);
        assert!(plain.trace.is_none());
        assert!(plain.take_request_trace().is_none());
    }
    #[derive(Clone, Debug, Default)]
    struct Text {
        value: String,
        colour: u32,
    }
    fn key(t: &Text, out: &mut Vec<u8>) {
        out.extend_from_slice(t.value.as_bytes());
    }
    fn metric(t: &Text, room: Option<f64>) -> Size {
        let full = t.value.len() as f64 * 8.;
        let w = room.unwrap_or(full).max(1.);
        Size::new(full.min(w), (full / w).ceil().max(1.) * 16.)
    }
    fn label(id: &str, value: &str) -> Node<Text> {
        Node::content()
            .with(Text {
                value: value.into(),
                colour: 0,
            })
            .id(id)
    }
    fn fixture() -> Node<Text> {
        Node::row([
            Node::col([label("a", "A short label"), label("b", "Another label")])
                .id("left")
                .grow(1.),
            Node::col([label("c", "Static right panel"), label("d", "Stable")])
                .id("right")
                .grow(1.),
        ])
        .id("root")
    }
    fn cached(n: &Node<Text>, w: f64, c: &mut LayoutCache) -> Layout {
        resolve_cached_with(
            n,
            Some(Size::new(w, 100.)),
            Limits::default(),
            SpacingScale::DEFAULT,
            c,
            key,
            metric,
        )
        .unwrap()
    }
    /// A new size solves bare; the cache takes over once it holds.
    fn prime(n: &Node<Text>, w: f64, c: &mut LayoutCache) -> Layout {
        let bare = cached(n, w, c);
        assert_eq!(c.stats().validated_nodes, 0);
        assert_eq!(cached(n, w, c), bare);
        bare
    }
    fn oracle(n: &Node<Text>, w: f64) -> Layout {
        resolve_with(
            n,
            Some(Size::new(w, 100.)),
            Limits::default(),
            SpacingScale::DEFAULT,
            metric,
        )
        .unwrap()
    }

    /// Exercise the measurement hook at changing offers, including offers
    /// inside one solve. The public cache deliberately solves live resizes
    /// bare, which would otherwise hide constraint-variant equivalence.
    fn offer_oracle(n: &Node<Text>, offers: &[Size], boxed: Option<Insets>) -> LayoutStats {
        let mut cache = LayoutCache::default();
        let mut totals = LayoutStats::default();
        for &size in offers {
            cache
                .prepare(n, Limits::default(), SpacingScale::DEFAULT, &mut key)
                .unwrap();
            let actual = crate::resolve_impl(
                n,
                Some(size),
                Limits::default(),
                SpacingScale::DEFAULT,
                metric,
                Some(&mut cache),
                boxed,
            );
            let fresh = crate::resolve_impl(
                n,
                Some(size),
                Limits::default(),
                SpacingScale::DEFAULT,
                metric,
                None,
                boxed,
            );
            assert_eq!(actual, fresh, "offer {size:?}, boxed {boxed:?}");
            actual.unwrap();
            totals.measured_nodes += cache.stats.measured_nodes;
            totals.measure_hits += cache.stats.measure_hits;
            totals.measure_variant_evictions += cache.stats.measure_variant_evictions;
        }
        totals
    }

    #[test]
    fn branch_working_set_survives_nested_flex_offers() {
        let n = Node::row([Node::block(24., 16.).shrink(0.).with(Text {
            value: String::new(),
            colour: 0,
        })])
        .id("branch");
        let widths: Vec<_> = (0..24).map(|i| Size::new(100. + i as f64, 80.)).collect();
        let once = offer_oracle(&n, &widths, None);
        let twice = offer_oracle(
            &n,
            &widths.iter().chain(&widths).copied().collect::<Vec<_>>(),
            None,
        );
        assert_eq!(
            twice.measured_nodes, once.measured_nodes,
            "a repeated branch offer must not rebuild the measured subtree"
        );
        assert!(twice.measure_hits >= once.measure_hits + widths.len());
        assert_eq!(
            twice.measure_variant_evictions,
            once.measure_variant_evictions
        );
    }

    #[test]
    fn replaced_fixed_offers_share_one_variant_with_exact_fresh_layouts() {
        let n = Node::stack([
            Node::col([
                label("percent", "Percent descendant")
                    .w(Len::Pct(50.))
                    .h(Len::Pct(25.)),
                label("container", "Container descendant")
                    .w(Len::Container(75.))
                    .h(Len::Container(50.)),
            ])
            .scroll()
            .scrolled(0., 7.)
            .id("flow"),
            label("pin", "Pinned").pin(Pin::to("percent").area(Area::End)),
        ])
        .size(240., 160.)
        .shrink(0.)
        .pad(4.)
        .id("root");
        let offers: Vec<_> = (0..12)
            .map(|i| Size::new(280. + i as f64 * 7., 180. + i as f64 * 3.))
            .collect();
        offer_oracle(&n, &offers, None);
        let fixed_label = label("fixed-label", "Fixed measured content")
            .size(240., 160.)
            .shrink(0.);
        let stats = offer_oracle(&fixed_label, &offers, None);
        assert_eq!(stats.measure_hits, offers.len() - 1);
        assert_eq!(stats.measured_nodes, 1, "only the first offer is measured");
        assert_eq!(stats.measure_variant_evictions, 0);
    }

    #[test]
    fn effective_offer_keys_preserve_redo_room_aspect_box_and_container_axes() {
        let fixed = label("fixed", "Fixed").size(120., 80.).shrink(0.);
        let input = |n: &Node<Text>, h, room, cq, redo, boxed| {
            MeasureInput::for_node(
                n,
                [Some(200.), Some(h)],
                Some(room),
                [Some(200.), Some(cq)],
                redo,
                boxed,
                SpacingScale::DEFAULT,
            )
        };
        let fixed_key = input(&fixed, 100., 200., 300., false, false);
        assert_eq!(fixed_key, input(&fixed, 500., 200., 600., false, false));
        assert_ne!(fixed_key, input(&fixed, 100., 100., 300., false, false));
        assert_ne!(fixed_key, input(&fixed, 100., 200., 300., true, false));
        for flexible in [fixed.clone().grow(1.), fixed.clone().shrink(1.)] {
            assert_eq!(
                input(&flexible, 100., 200., 300., false, false),
                input(&flexible, 500., 200., 600., false, false)
            );
            assert_ne!(
                input(&flexible, 100., 200., 300., true, false),
                input(&flexible, 500., 200., 600., true, false)
            );
        }
        let cq = fixed.clone().h(Len::Container(50.));
        assert_ne!(
            input(&cq, 100., 200., 300., false, false),
            input(&cq, 100., 200., 600., false, false)
        );
        for n in [fixed.clone().aspect(2.), fixed.clone().aspect_fit(2.)] {
            assert_ne!(
                input(&n, 100., 200., 300., false, false),
                input(&n, 100., 200., 600., false, false)
            );
            assert_ne!(
                input(&n, 100., 200., 300., false, false),
                input(&n, 500., 200., 300., false, false)
            );
        }
        assert_ne!(
            input(&fixed, 100., 200., 300., false, true),
            input(&fixed, 500., 200., 600., false, true)
        );
        let auto = fixed.h(Len::Auto);
        assert_ne!(
            MeasureInput::for_node(
                &auto,
                [Some(200.), None],
                None,
                [None, Some(300.)],
                false,
                false,
                SpacingScale::DEFAULT
            ),
            MeasureInput::for_node(
                &auto,
                [Some(200.), None],
                None,
                [None, Some(600.)],
                false,
                false,
                SpacingScale::DEFAULT
            )
        );
    }

    #[test]
    fn unused_inherited_containers_reuse_natural_controls_with_fresh_frames() {
        let natural = Node::col([
            Node::row([
                label("wrapped", "A deliberately long musical parameter caption")
                    .w(Len::Pct(60.))
                    .min_w(18.),
                label("aspect", "Aspect").w(48.).aspect(2.).shrink(0.),
            ])
            .wrap()
            .id("rail"),
            label("bounded", "Minimum and maximum")
                .min_size(Size::new(20., 18.))
                .max_size(Size::new(160., 90.)),
        ])
        .w(180.)
        .shrink(0.)
        .id("natural");
        let mut cache = LayoutCache::default();
        for height in [320., 240., 160., 80., 20., 4., 160., 320.] {
            let root = Node::stack([
                Node::col([natural.clone()])
                    .size(320., height)
                    .shrink(0.)
                    .scroll()
                    .scrolled(0., height * 0.1)
                    .id("shutter"),
                label("pin", "Pinned").pin(Pin::to("bounded").area(Area::End)),
            ])
            .id("root");
            assert_eq!(cached(&root, 400., &mut cache), oracle(&root, 400.));
            assert_eq!(cached(&root, 400., &mut cache), oracle(&root, 400.));
            let stamp = cache.stamp(&root.children()[0].children()[0]).unwrap();
            assert!(
                stamp
                    .measured
                    .iter()
                    .all(|(input, _)| input.container == [None; 2]),
                "neither the initial pass nor the flex remeasure consumes the inherited shutter height"
            );
        }
    }

    #[test]
    fn introducing_container_units_rebuilds_inherited_extent_dependencies() {
        let mut cache = LayoutCache::default();
        for use_container in [false, true, false] {
            for height in [320., 240., 160., 80., 20., 160., 320.] {
                let length = if use_container {
                    Len::Container(50.)
                } else {
                    Len::Px(24.)
                };
                let root = Node::col([Node::col([Node::block(32., length).id("control")])
                    .w(180.)
                    .h(Len::Pct(100.))
                    .shrink(0.)
                    .id("natural")])
                .size(320., height)
                .shrink(0.)
                .scroll()
                .id("root");
                let actual = cached(&root, 400., &mut cache);
                assert_eq!(actual, oracle(&root, 400.));
                assert_eq!(
                    actual.frame("control").unwrap().size.height,
                    if use_container { height * 0.5 } else { 24. },
                    "the consumed CQ extent must change sound-independent geometry"
                );
                assert_eq!(cached(&root, 400., &mut cache), oracle(&root, 400.));
                if !use_container {
                    assert!(
                        cache
                            .stamp(&root.children()[0])
                            .unwrap()
                            .measured
                            .iter()
                            .all(|(input, _)| input.container == [None; 2])
                    );
                }
            }
        }
    }

    #[test]
    fn fixed_natural_subtree_matches_fresh_inside_a_changing_scroll_shutter() {
        let natural = Node::col([
            label("relative", "A relative child of the natural content")
                .w(Len::Container(80.))
                .h(Len::Pct(50.)),
            label("tail", "Stable tail").h(32.).shrink(0.),
        ])
        .size(240., 160.)
        .shrink(0.)
        .id("natural");
        let mut cache = LayoutCache::default();
        for height in [100., 80., 60., 40., 20., 4., 40., 80., 100.] {
            let n = Node::stack([
                Node::col([natural.clone()])
                    .size(240., height)
                    .shrink(0.)
                    .scroll()
                    .scrolled(0., height * 0.1)
                    .id("shutter"),
                label("pin", "Pinned").pin(Pin::to("relative").area(Area::End)),
            ])
            .id("root");
            assert_eq!(cached(&n, 300., &mut cache), oracle(&n, 300.));
            assert_eq!(cached(&n, 300., &mut cache), oracle(&n, 300.));
            let variants = cache
                .stamp(&n.children()[0].children()[0])
                .unwrap()
                .measured
                .len();
            assert_eq!(variants, 1, "the shutter does not resize natural content");
        }
    }

    #[test]
    fn consumed_room_keeps_narrow_unresolved_cq_aspect_and_boxed_offers_distinct() {
        let fixed = label("fixed", "Fixed").size(120., 80.).shrink(0.);
        let input = |n: &Node<Text>, width, room, boxed, scale| {
            MeasureInput::for_node(
                n,
                [width, Some(100.)],
                room,
                [Some(300.), Some(200.)],
                false,
                boxed,
                scale,
            )
        };
        for xs in [2., 5., 11.] {
            let scale = SpacingScale {
                xs,
                ..SpacingScale::DEFAULT
            };
            let padded = fixed.clone().pad(SpacingToken::Xs);
            let capped = input(&padded, Some(200.), Some(120.), false, scale);
            assert_eq!(capped.room, Some((120. - 2. * xs).to_bits()));
            for room in [None, Some(200.), Some(500.)] {
                assert_eq!(capped, input(&padded, Some(200.), room, false, scale));
            }
            assert_ne!(capped, input(&padded, Some(200.), Some(60.), false, scale));
            for n in [
                padded.clone().aspect(2.),
                padded.clone().aspect_fit(2.),
                padded.clone().w(Len::Container(40.)),
            ] {
                assert_ne!(
                    input(&n, Some(200.), Some(200.), false, scale),
                    input(&n, Some(200.), Some(500.), false, scale)
                );
            }
            let unresolved = padded.clone().w(Len::Auto);
            assert_ne!(
                input(&unresolved, None, Some(200.), false, scale),
                input(&unresolved, None, Some(500.), false, scale)
            );
            assert_ne!(
                input(&padded, Some(200.), Some(200.), true, scale),
                input(&padded, Some(200.), Some(500.), true, scale)
            );
        }
    }

    #[test]
    fn capped_parent_rooms_reuse_scaled_wrapping_subtrees_with_exact_fresh_frames() {
        for xs in [2., 5., 11.] {
            let scale = SpacingScale {
                xs,
                ..SpacingScale::DEFAULT
            };
            for scrolling in [false, true] {
                let mut panel = Node::col([
                    Node::row([
                        label("first", "Percent wrapping child").w(Len::Pct(55.)),
                        label("second", "Clamped wrapping child").w(Len::Clamp {
                            min: 50.,
                            pct: 45.,
                            max: 100.,
                        }),
                    ])
                    .wrap()
                    .id("wrap"),
                    label("cq", "Container child").w(Len::Container(80.)).h(24.),
                ])
                .w(180.)
                .min_w(120.)
                .max_size(Size::new(220., 200.))
                .shrink(0.)
                .pad(SpacingToken::Xs)
                .id("panel");
                if scrolling {
                    panel = panel.h(110.).scroll().scrolled(0., 13.);
                }
                let mut cache = LayoutCache::default();
                for width in [240., 260., 280., 300., 320., 340., 360., 240.] {
                    let n =
                        Node::col([Node::col([panel.clone()]).w(width).shrink(0.).id("parent")])
                            .id("root");
                    let fresh = resolve_with(
                        &n,
                        Some(Size::new(600., 400.)),
                        Limits::default(),
                        scale,
                        metric,
                    )
                    .unwrap();
                    for _ in 0..2 {
                        assert_eq!(
                            resolve_cached_with(
                                &n,
                                Some(Size::new(600., 400.)),
                                Limits::default(),
                                scale,
                                &mut cache,
                                key,
                                metric,
                            )
                            .unwrap(),
                            fresh
                        );
                    }
                    assert_eq!(
                        cache
                            .stamp(&n.children()[0].children()[0])
                            .unwrap()
                            .measured
                            .len(),
                        1,
                        "wider parent rooms do not create another panel variant"
                    );
                }
                let invalid = panel.pad(f64::NAN);
                let offered = Some(Size::new(600., 400.));
                let cached = resolve_cached_with(
                    &invalid,
                    offered,
                    Limits::default(),
                    scale,
                    &mut cache,
                    key,
                    metric,
                );
                let fresh = resolve_with(&invalid, offered, Limits::default(), scale, metric);
                assert!(fresh.is_err());
                assert_eq!(cached, fresh, "room reuse cannot bypass padding validation");
            }
        }
    }

    #[test]
    fn normalized_flex_minmax_aspect_and_relative_lengths_match_fresh_layouts() {
        let offers = [
            Size::new(300., 160.),
            Size::new(180., 95.),
            Size::new(350., 220.),
            Size::new(220., 110.),
            Size::new(300., 160.),
        ];
        for (grow, shrink) in [(0., 0.), (1., 0.), (0., 1.), (1., 1.)] {
            for len in [
                Len::Px(120.),
                Len::Pct(60.),
                Len::Container(70.),
                Len::Clamp {
                    min: 80.,
                    pct: 55.,
                    max: 180.,
                },
            ] {
                for aspect in [None, Some(false), Some(true)] {
                    let mut panel = Node::col([
                        label("text", "Wrapping a long label inside the final flex share"),
                        label("relative", "Relative")
                            .w(Len::Container(80.))
                            .h(Len::Pct(40.)),
                    ])
                    .size(len, 90.)
                    .min_size(Size::new(75., 50.))
                    .max_size(Size::new(190., 120.))
                    .grow(grow)
                    .shrink(shrink)
                    .pad(3.)
                    .id("panel");
                    panel = match aspect {
                        Some(false) => panel.h(Len::Auto).aspect(2.),
                        Some(true) => panel.aspect_fit(2.),
                        None => panel,
                    };
                    let n = Node::row([
                        panel,
                        label("sibling", "Sibling takes the remaining share").grow(1.),
                    ])
                    .id("root");
                    offer_oracle(&n, &offers, None);
                }
            }
        }
        let n = Node::col([label("boxed-child", "Boxed child")])
            .size(100., 50.)
            .min_h(30.)
            .max_size(Size::new(200., 130.));
        offer_oracle(&n, &offers, Some(Insets::all(5.)));
    }

    #[test]
    fn normalized_warm_measurements_keep_invalid_offers_and_limits_rejected() {
        let n = Node::col([label("child", "Child")])
            .size(120., 80.)
            .shrink(0.)
            .id("root");
        let mut cache = LayoutCache::default();
        prime(&n, 200., &mut cache);
        for limits in [
            Limits {
                nodes: 1,
                ..Limits::default()
            },
            Limits {
                depth: 0,
                ..Limits::default()
            },
            Limits {
                extent: 70.,
                ..Limits::default()
            },
        ] {
            let actual = resolve_cached_with(
                &n,
                Some(Size::new(200., 100.)),
                limits,
                SpacingScale::DEFAULT,
                &mut cache,
                key,
                metric,
            );
            let fresh = resolve_with(
                &n,
                Some(Size::new(200., 100.)),
                limits,
                SpacingScale::DEFAULT,
                metric,
            );
            assert!(fresh.is_err());
            assert_eq!(actual, fresh);
            assert_eq!(cached(&n, 200., &mut cache), oracle(&n, 200.));
            assert_eq!(cached(&n, 200., &mut cache), oracle(&n, 200.));
        }
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.] {
            let offered = Some(Size::new(200., invalid));
            let actual = resolve_cached_with(
                &n,
                offered,
                Limits::default(),
                SpacingScale::DEFAULT,
                &mut cache,
                key,
                metric,
            );
            let fresh = resolve_with(
                &n,
                offered,
                Limits::default(),
                SpacingScale::DEFAULT,
                metric,
            );
            assert_eq!(actual, fresh);
            assert!(fresh.is_err());
            assert_eq!(cached(&n, 200., &mut cache), oracle(&n, 200.));
            assert_eq!(cached(&n, 200., &mut cache), oracle(&n, 200.));
        }
    }

    fn scroll_fixture() -> Node<Text> {
        let nested = Node::col([
            label("nested-first", "Nested first")
                .size(240., 48.)
                .shrink(0.),
            label("nested-last", "Nested last")
                .size(240., 48.)
                .shrink(0.),
        ])
        .size(240., 64.)
        .scroll()
        .id("nested");
        let flow = Node::col([
            label("anchor", "Scrolling anchor")
                .size(240., 36.)
                .shrink(0.),
            nested.shrink(0.),
            label("tail", "Scrolling tail").size(240., 96.).shrink(0.),
        ])
        .shrink(0.)
        .id("flow");
        Node::stack([
            Node::col([
                label("header", "Sticky header")
                    .size(240., 24.)
                    .shrink(0.)
                    .sticky(),
                flow,
            ])
            .size(260., 100.)
            .scroll()
            .id("scroll"),
            label("tip", "Pinned").pin(Pin::to("anchor").area(Area::End)),
        ])
        .pad(SpacingToken::Xs)
        .id("root")
    }

    #[test]
    fn cached_scroll_binds_the_requested_root_without_rebuilding_descendant_measurements() {
        let mut n =
            Node::col((0..128).map(|i| label(&format!("row/{i}"), "Stable row").shrink(0.)))
                .size(240., 100.)
                .scroll()
                .id("scroll");
        let mut cache = LayoutCache::default();
        prime(&n, 400., &mut cache);
        for offset in [1., 17., 64., 120.] {
            n.scrolled = [0., offset];
            assert_eq!(cached(&n, 400., &mut cache), oracle(&n, 400.));
            assert_eq!(cache.stats().measured_nodes, 0);
            assert_eq!(
                cache.stats().rebound_nodes,
                1,
                "a root cache hit must not allocate and rebind the entire measured descendant tree"
            );
        }
    }

    #[test]
    fn scroll_placement_reuses_measurements_and_rearranges_nested_sticky_pins() {
        let mut n = scroll_fixture();
        let mut c = LayoutCache::default();
        let before = prime(&n, 400., &mut c);
        for step in 1..=64 {
            n.children_mut()[0].scrolled = [step as f64 * 0.125, step as f64 * 0.75];
            n.children_mut()[0].children_mut()[1].children_mut()[1].scrolled =
                [0., step as f64 * 0.25];
            let result = cached(&n, 400., &mut c);
            assert_eq!(result, oracle(&n, 400.));
            assert_ne!(result.frame("anchor"), before.frame("anchor"));
            let stats = c.stats();
            assert_eq!(
                stats.measured_nodes, 0,
                "scroll does not change any intrinsic input"
            );
            assert_eq!(
                stats.measure_hits, 1,
                "the complete root measurement is shared"
            );
            assert_eq!(stats.rebound_nodes, result.all().len());
            assert_eq!(
                stats.arranged_nodes,
                result.all().len(),
                "scroll must update real frames"
            );
            assert_eq!(cached(&n, 400., &mut c), result);
            assert_eq!(
                c.stats().arranged_nodes,
                0,
                "unchanged placement reuses the whole layout"
            );
        }
    }

    #[test]
    fn scroll_placement_preserves_reorder_offset_and_pin_invalidation() {
        let mut n = scroll_fixture();
        let mut c = LayoutCache::default();
        prime(&n, 400., &mut c);
        n.children_mut()[0].scrolled = [2., 17.];
        n.children_mut()[0].children_mut()[1]
            .children_mut()
            .swap(0, 2);
        assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
        assert!(
            c.stats().measured_nodes > 0,
            "a reordered intrinsic declaration is still invalidated"
        );
        n.children_mut()[0].offset = [9.5, -2.];
        assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
        assert!(
            c.stats().measured_nodes > 0,
            "OFFSET remains in the measurement projection"
        );
        n.children_mut()[1] =
            label("tip", "Pinned").pin(Pin::to("nested-last").area(Area::Bottom).match_width());
        assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
        n.children_mut()[0].scrolled = [3., 29.];
        assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
        assert_eq!(
            c.stats().measured_nodes,
            0,
            "a fresh pin uses freshly arranged scroll anchors"
        );

        // Anonymous ancestor addresses and named child identities both take
        // part in the intrinsic revision; placement is not keyed by hash.
        n.children_mut()[0].children_mut()[1].id = None;
        assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
        n.children_mut()[0].children_mut()[1]
            .children_mut()
            .swap(0, 2);
        n.children_mut()[0].scrolled = [0., 11.];
        assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
    }

    #[test]
    fn scroll_placement_validates_warm_coordinates_and_budget_context() {
        let mut n = scroll_fixture();
        let mut c = LayoutCache::default();
        prime(&n, 400., &mut c);
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            n.children_mut()[0].scrolled = [0., invalid];
            assert!(matches!(
                resolve_cached_with(
                    &n,
                    Some(Size::new(400., 100.)),
                    Limits::default(),
                    SpacingScale::DEFAULT,
                    &mut c,
                    key,
                    metric
                ),
                Err(Error::InvalidValue)
            ));
            n.children_mut()[0].scrolled = [0., 13.];
            assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
        }
        for limits in [
            Limits {
                nodes: 2,
                ..Limits::default()
            },
            Limits {
                depth: 1,
                ..Limits::default()
            },
            Limits {
                extent: 250.,
                ..Limits::default()
            },
        ] {
            assert!(
                resolve_cached_with(
                    &n,
                    Some(Size::new(400., 100.)),
                    limits,
                    SpacingScale::DEFAULT,
                    &mut c,
                    key,
                    metric
                )
                .is_err(),
                "placement reuse never bypasses nodes, depth or extent limits"
            );
            assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
            assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
        }
        n.children_mut()[0].scrolled = [0., 19.];
        assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
        assert_eq!(c.stats().measured_nodes, 0);
    }

    #[test]
    fn scroll_placement_survives_resizing_and_spacing_context_changes() {
        let mut n = scroll_fixture();
        let mut c = LayoutCache::default();
        let wider_spacing = SpacingScale {
            xs: 9.,
            ..SpacingScale::DEFAULT
        };
        for width in [400., 300., 600., 400.] {
            n.children_mut()[0].scrolled = [1., width * 0.04];
            for scale in [SpacingScale::DEFAULT, wider_spacing, SpacingScale::DEFAULT] {
                let fresh = resolve_with(
                    &n,
                    Some(Size::new(width, 100.)),
                    Limits::default(),
                    scale,
                    metric,
                )
                .unwrap();
                for _ in 0..2 {
                    assert_eq!(
                        resolve_cached_with(
                            &n,
                            Some(Size::new(width, 100.)),
                            Limits::default(),
                            scale,
                            &mut c,
                            key,
                            metric
                        )
                        .unwrap(),
                        fresh
                    );
                }
                n.children_mut()[0].scrolled[1] += 0.375;
                assert_eq!(
                    resolve_cached_with(
                        &n,
                        Some(Size::new(width, 100.)),
                        Limits::default(),
                        scale,
                        &mut c,
                        key,
                        metric
                    )
                    .unwrap(),
                    resolve_with(
                        &n,
                        Some(Size::new(width, 100.)),
                        Limits::default(),
                        scale,
                        metric
                    )
                    .unwrap()
                );
                assert_eq!(c.stats().measured_nodes, 0);
            }
        }
    }

    #[test]
    fn a_changed_word_moves_the_cached_floor() {
        // Eight per char, as `metric`, and the longest word is the floor:
        // "ab" cannot shrink, so "a" takes the whole squeeze down to its word.
        let words = |t: &Text, room: Option<f64>| Intrinsic {
            size: metric(t, room),
            min_width: t.value.split(' ').map(str::len).max().unwrap_or(0) as f64 * 8.,
        };
        let solve = |n: &Node<Text>, c: &mut LayoutCache| {
            resolve_cached_with(
                n,
                Some(Size::new(48., 100.)),
                Limits::default(),
                SpacingScale::DEFAULT,
                c,
                key,
                words,
            )
            .unwrap()
        };
        let mut n = Node::row([label("a", "ab abcd"), label("b", "ab")]).id("root");
        let mut c = LayoutCache::default();
        solve(&n, &mut c);
        let l = solve(&n, &mut c);
        assert_eq!(l.frame("a").unwrap().size.width, 32.);
        n.children_mut()[0].payload_mut().value = "ab abcdef".into();
        let l = solve(&n, &mut c);
        assert!(c.stats().measured_nodes > 0);
        assert_eq!(l.frame("a").unwrap().size.width, 48.);
        assert_eq!(l.min_size().width, 64.);
    }
    #[test]
    fn warm_tree_skips_measure_and_arrange() {
        let n = fixture();
        let mut c = LayoutCache::default();
        prime(&n, 400., &mut c);
        let l = cached(&n, 400., &mut c);
        assert_eq!(l, oracle(&n, 400.));
        let s = c.stats();
        assert_eq!(s.measured_nodes, 0);
        assert_eq!(s.arranged_nodes, 0);
        assert_eq!(s.validated_nodes, 7);
    }
    #[test]
    fn decorative_payload_does_not_invalidate_layout() {
        let mut n = fixture();
        let mut c = LayoutCache::default();
        let before = prime(&n, 400., &mut c);
        n.children_mut()[0].children_mut()[0].payload_mut().colour = 42;
        assert_eq!(cached(&n, 400., &mut c), before);
        assert_eq!(c.stats().measured_nodes, 0);
    }
    #[test]
    fn changing_one_label_reuses_other_metrics() {
        let mut n = fixture();
        let mut c = LayoutCache::default();
        prime(&n, 400., &mut c);
        n.children_mut()[0].children_mut()[0].payload_mut().value = "Changed".into();
        assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
        assert!(c.stats().measure_hits > 0);
    }
    #[test]
    fn resize_and_wrap_match_uncached() {
        let n = fixture();
        let mut c = LayoutCache::default();
        for w in [400., 120., 640., 121., 400.] {
            assert_eq!(prime(&n, w, &mut c), oracle(&n, w));
        }
    }
    #[test]
    fn removed_node_does_not_leak_into_layout() {
        let mut n = fixture();
        let mut c = LayoutCache::default();
        prime(&n, 400., &mut c);
        n.children_mut()[0].children_mut()[0] = label("new", "Replacement");
        let l = cached(&n, 400., &mut c);
        assert!(l.frame("a").is_none());
        assert_eq!(l, oracle(&n, 400.));
    }
    #[test]
    fn duplicate_ids_are_validated_on_a_warm_cache() {
        let mut n = fixture();
        let mut c = LayoutCache::default();
        prime(&n, 400., &mut c);
        n.children_mut()[1].children_mut()[0] = label("a", "Duplicate");
        assert!(matches!(
            resolve_cached_with(
                &n,
                Some(Size::new(400., 100.)),
                Limits::default(),
                SpacingScale::DEFAULT,
                &mut c,
                key,
                metric
            ),
            Err(Error::DuplicateKey(_))
        ));
    }
    #[test]
    fn reordered_layout_matches_uncached() {
        let mut n = fixture();
        let mut c = LayoutCache::default();
        prime(&n, 400., &mut c);
        n.children_mut().swap(0, 1);
        assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
    }
    #[test]
    fn pin_dependencies_never_use_stale_frames() {
        let mut n = Node::stack([
            label("anchor", "Hello"),
            label("tip", "Tip").pin(Pin::to("anchor")),
        ]);
        let mut c = LayoutCache::default();
        prime(&n, 400., &mut c);
        n.children_mut()[0] = label("anchor", "Longer anchor").offset(80., 0.);
        assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
    }
    /// Stamps are keyed by hashed paths now, and the pin lives outside the
    /// projection: unkeyed moves, pin edits and an id reused down its own
    /// subtree must all still be seen on a warm cache.
    #[test]
    fn hashed_addresses_see_moves_pins_and_nested_duplicates() {
        let mut n = Node::stack([
            Node::row([label("x", "One"), Node::block(30., 10.)]),
            Node::row([Node::block(50., 10.), label("y", "Two")]),
            label("tip", "Tip").pin(Pin::to("x")),
        ]);
        let mut c = LayoutCache::default();
        assert_eq!(prime(&n, 400., &mut c), oracle(&n, 400.));
        n.children_mut().swap(0, 1);
        assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
        n.children_mut()[2] = label("tip", "Tip").pin(Pin::to("y").area(Area::End));
        assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
        n.children_mut()[0].children_mut()[0] = label("y", "Dup");
        let n = n.id("y");
        assert!(matches!(
            resolve_cached_with(
                &n,
                Some(Size::new(400., 100.)),
                Limits::default(),
                SpacingScale::DEFAULT,
                &mut c,
                key,
                metric
            ),
            Err(Error::DuplicateKey(id)) if id == "y"
        ));
    }
    #[test]
    fn budget_changes_invalidate_cached_work() {
        let n = fixture();
        let mut c = LayoutCache::default();
        prime(&n, 400., &mut c);
        assert!(matches!(
            resolve_cached_with(
                &n,
                Some(Size::new(400., 100.)),
                Limits {
                    nodes: 2,
                    ..Limits::default()
                },
                SpacingScale::DEFAULT,
                &mut c,
                key,
                metric
            ),
            Err(Error::BudgetExceeded)
        ));
    }
    #[test]
    fn changing_measure_environment_requires_clear() {
        let n = fixture();
        let mut c = LayoutCache::default();
        prime(&n, 400., &mut c);
        c.clear();
        prime(&n, 400., &mut c);
        assert!(c.stats().measured_nodes > 0);
    }
}
