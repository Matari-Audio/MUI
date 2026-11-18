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
    /// Of those, the ones a retained subtree revalidated without comparing.
    pub replayed_nodes: usize,
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
    /// least recently used first. A new revision is a new stamp, so these
    /// never go stale; they only stop being asked for.
    measured: Vec<Variant>,
}
/// One measurement of a stamp and the `prepare` pass that last asked for it.
#[derive(Clone, Debug, PartialEq)]
struct Variant {
    input: MeasureInput,
    frozen: Arc<Frozen>,
    used: u64,
}
/// One `prepare` walk's state. The buffers live in the cache between frames,
/// so a warm walk allocates nothing.
struct Scan<'k, K, R> {
    limits: Limits,
    key: &'k mut K,
    /// A node's retained-subtree token: `(token, reusable)`. See
    /// [`resolve_cached_retained`].
    retained: &'k mut R,
    /// Every visited node's `(address, revision)`, in completion order, so
    /// a retained subtree's run can be recorded and replayed.
    trail: Vec<(u64, u64)>,
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
/// A retained subtree's run, whether it reads the container, and the pass
/// that last used it.
type Replay = (Vec<(u64, u64)>, bool, u64);
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
    stats: LayoutStats,
    /// None in ordinary operation: no trace history or export buffers exist.
    trace: Option<Box<crate::diagnostics::Recorder>>,
    /// Each retained subtree's stamps, `(address, revision)` in completion
    /// order, whether it reads the container, and the pass that last used it.
    replays: FxHashMap<u64, Replay>,
    trail: Vec<(u64, u64)>,
    /// A replay's claimed stamps and the pass each had, to undo a failed one.
    claimed: Vec<(u64, u64)>,
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
    // solve. Retain that working set; what the last two solves did not ask
    // for is dropped on the next miss. Leaves keep their four entries.
    const BRANCH_VARIANTS: usize = 64;
    /// Validates the tree and returns its measurement and placement revisions.
    #[cfg(test)]
    fn prepare<P>(
        &mut self,
        n: &Node<P>,
        limits: Limits,
        scale: SpacingScale,
        key: &mut impl FnMut(&P, &mut Vec<u8>),
    ) -> Result<(u64, u64), Error> {
        self.prepare_retained(n, limits, scale, key, &mut |_| None)
    }
    fn prepare_retained<P>(
        &mut self,
        n: &Node<P>,
        limits: Limits,
        scale: SpacingScale,
        key: &mut impl FnMut(&P, &mut Vec<u8>),
        retained: &mut impl FnMut(&P) -> Option<(u64, bool)>,
    ) -> Result<(u64, u64), Error> {
        // A context change invalidates every metric. Limits cannot be bypassed.
        if self.context != Some((limits, scale)) {
            let mut trace = self.trace.take();
            if let Some(trace) = trace.as_mut() {
                trace.context_changed();
            }
            *self = Self {
                context: Some((limits, scale)),
                trace,
                ..Self::default()
            };
        }
        if let Some(trace) = self.trace.as_mut() {
            trace.frame();
        }
        self.pointers.clear();
        self.pinned = false;
        self.stats = LayoutStats::default();
        self.pass = self.pass.wrapping_add(1);
        self.placement_scratch.clear();
        let mut scan = Scan {
            limits,
            key,
            retained,
            trail: std::mem::take(&mut self.trail),
            pass: self.pass,
            count: 0,
            revisions: std::mem::take(&mut self.revisions),
            payload: std::mem::take(&mut self.payload),
            placement: std::mem::take(&mut self.placement_scratch),
        };
        let walked = self.scan(n, 0, 0, &mut scan);
        (self.revisions, self.payload, self.placement_scratch) =
            (scan.revisions, scan.payload, scan.placement);
        self.trail = scan.trail;
        self.trail.clear();
        let pass = self.pass;
        self.replays.retain(|_, r| r.2 == pass);
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
    fn scan<P, K: FnMut(&P, &mut Vec<u8>), R: FnMut(&P) -> Option<(u64, bool)>>(
        &mut self,
        n: &Node<P>,
        path: u64,
        depth: usize,
        s: &mut Scan<'_, K, R>,
    ) -> Result<(u64, bool), Error> {
        let token = (s.retained)(&n.payload);
        if let Some((token, true)) = token
            && let Some(done) = self.replay(n, token, depth, s)?
        {
            return Ok(done);
        }
        let from = s.trail.len();
        let done = self.scan_node(n, path, depth, s)?;
        if let Some((token, _)) = token {
            let run = s.trail[from..].to_vec();
            self.replays.insert(token, (run, done.1, s.pass));
        }
        Ok(done)
    }
    /// Revalidate a retained subtree from the run its last scan recorded,
    /// without projecting or comparing a node: the caller vouched it is
    /// the very subtree scanned then. Only the bookkeeping a scan leaves
    /// behind is redone. `None` (with nothing changed) when a stamp it
    /// names was replaced or already claimed this pass: scan it instead.
    fn replay<P, K, R: FnMut(&P) -> Option<(u64, bool)>>(
        &mut self,
        n: &Node<P>,
        token: u64,
        depth: usize,
        s: &mut Scan<'_, K, R>,
    ) -> Result<Option<(u64, bool)>, Error> {
        let Some((run, container, _)) = self.replays.get_mut(&token) else {
            return Ok(None);
        };
        let run = std::mem::take(run);
        let container = *container;
        let (count, placed, trailed, pinned, replayed) = (
            s.count,
            s.placement.len(),
            s.trail.len(),
            self.pinned,
            self.stats.replayed_nodes,
        );
        let mut claimed = std::mem::take(&mut self.claimed);
        claimed.clear();
        let mut at = 0;
        let ok = match self.replay_node(n, depth, &run, &mut at, &mut claimed, s) {
            Ok(ok) => ok,
            Err(e) => {
                self.claimed = claimed;
                return Err(e);
            }
        };
        if ok && at == run.len() {
            self.claimed = claimed;
            let root = run[run.len() - 1].1;
            self.replays.insert(token, (run, container, s.pass));
            return Ok(Some((root, container)));
        }
        for &(address, seen) in &claimed {
            if let Some(stamp) = self.stamps.get_mut(&address) {
                stamp.seen = seen;
            }
        }
        s.count = count;
        self.stats.replayed_nodes = replayed;
        s.placement.truncate(placed);
        s.trail.truncate(trailed);
        self.pinned = pinned;
        self.replays.remove(&token);
        self.claimed = claimed;
        Ok(None)
    }
    fn replay_node<P, K, R: FnMut(&P) -> Option<(u64, bool)>>(
        &mut self,
        n: &Node<P>,
        depth: usize,
        run: &[(u64, u64)],
        at: &mut usize,
        claimed: &mut Vec<(u64, u64)>,
        s: &mut Scan<'_, K, R>,
    ) -> Result<bool, Error> {
        if depth > s.limits.depth || s.count >= s.limits.nodes {
            return Err(Error::BudgetExceeded);
        }
        s.count += 1;
        if !n.scrolled.iter().all(|v| v.is_finite()) {
            return Err(Error::InvalidValue);
        }
        s.placement.push(n.scrolled.map(f64::to_bits));
        self.pinned |= n.rare().pin.is_some();
        // A retained subtree nested in this one keeps its own record alive.
        if let Some((token, _)) = (s.retained)(&n.payload)
            && let Some(nested) = self.replays.get_mut(&token)
        {
            nested.2 = s.pass;
        }
        self.stats.replayed_nodes += 1;
        for c in n.children() {
            if !self.replay_node(c, depth + 1, run, at, claimed, s)? {
                return Ok(false);
            }
        }
        let Some(&(address, revision)) = run.get(*at) else {
            return Ok(false);
        };
        *at += 1;
        let Some(stamp) = self.stamps.get_mut(&address) else {
            return Ok(false);
        };
        if stamp.seen == s.pass || stamp.revision != revision || stamp.shape.id != n.id {
            return Ok(false);
        }
        claimed.push((address, stamp.seen));
        stamp.seen = s.pass;
        s.trail.push((address, revision));
        self.pointers.insert(n as *const Node<P> as usize, address);
        Ok(true)
    }
    fn scan_node<P, K: FnMut(&P, &mut Vec<u8>), R: FnMut(&P) -> Option<(u64, bool)>>(
        &mut self,
        n: &Node<P>,
        path: u64,
        depth: usize,
        s: &mut Scan<'_, K, R>,
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
        s.trail.push((address, revision));
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
        place,
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
        place: *place,
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
    key: impl FnMut(&P, &mut Vec<u8>),
    measurer: impl FnMut(&P, Option<f64>) -> M,
) -> Result<Layout, Error> {
    resolve_cached_retained(root, offered, limits, scale, cache, key, |_| None, measurer)
}

/// [`resolve_cached_with`] for a tree with retained subtrees. `retained`
/// gives a subtree root's token, and whether it is *reusable*: the exact
/// subtree, untouched, that the last call scanned under that token. A
/// reusable subtree is revalidated from what that scan recorded instead of
/// node by node; one whose stamps were since replaced is scanned as usual.
#[expect(
    clippy::too_many_arguments,
    reason = "resolve_cached_with plus the token"
)]
pub fn resolve_cached_retained<P, M: Into<Intrinsic>>(
    root: &Node<P>,
    offered: Option<Size>,
    limits: Limits,
    scale: SpacingScale,
    cache: &mut LayoutCache,
    mut key: impl FnMut(&P, &mut Vec<u8>),
    mut retained: impl FnMut(&P) -> Option<(u64, bool)>,
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
    // A resize misses across frames, but not within its own solve: nested
    // flex re-measures ask for the same subtree at the same offer many times
    // over, so even a new size goes through the cache.
    let offered_bits = offered.map(|s| [s.width.to_bits(), s.height.to_bits()]);
    let (revision, placement) =
        cache.prepare_retained(root, limits, scale, &mut key, &mut retained)?;
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
    let now = cache.pass;
    let snapshot = cache.stamp(node).and_then(|s| {
        let at = s.measured.iter().position(|v| v.input == key)?;
        // Refreshing a hit keeps the list least recently used first, so a
        // frequently asked constraint is never evicted for a colder one.
        s.measured[at].used = now;
        s.measured[at..].rotate_left(1);
        s.measured.last().map(|v| v.frozen.clone())
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
            // What neither this solve nor the last asked for is a size the
            // window has left: a dragged edge never offers it again, and
            // scanning it costs every lookup. The live set stays.
            let stale = measured
                .iter()
                .take_while(|v| v.used.wrapping_add(1) < now)
                .count()
                .max((measured.len() + 1).saturating_sub(limit));
            evicted = stale > 0;
            measured.drain(..stale);
            measured.push(Variant {
                input: key,
                frozen,
                used: now,
            });
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
mod tests;
