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
    /// The `prepare` pass that last visited this address.
    seen: u64,
    /// This revision's measurements, one per constraint it was offered,
    /// oldest first. A new revision is a new stamp, so these never go stale.
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
/// A measured subtree with its node references dropped. Children are shared,
/// so storing a parent costs one record, not its whole subtree again.
#[derive(Debug, PartialEq)]
pub(crate) struct Frozen {
    cost: usize,
    gap: f64,
    line_gap: f64,
    padding: Insets,
    size: Size,
    floor: Size,
    content: Size,
    fluid: bool,
    cols: usize,
    pick: usize,
    container: [Option<f64>; 2],
    children: Vec<Arc<Frozen>>,
}
impl Frozen {
    fn bind<'a, P>(this: &Arc<Self>, n: &'a Node<P>, stats: &mut LayoutStats) -> Measured<'a, P> {
        stats.rebound_nodes += 1;
        Measured {
            node: n,
            frozen: Some(this.clone()),
            index: 0,
            fluid: this.fluid,
            gap: this.gap,
            line_gap: this.line_gap,
            padding: this.padding,
            size: this.size,
            floor: this.floor,
            content: this.content,
            cols: this.cols,
            pick: this.pick,
            container: this.container,
            children: this
                .children
                .iter()
                .zip(n.children())
                .enumerate()
                .map(|(i, (c, n))| {
                    let mut m = Self::bind(c, n, stats);
                    m.index = i;
                    m
                })
                .collect(),
        }
    }
    fn freeze<P>(m: &Measured<'_, P>, cost: usize) -> Arc<Self> {
        Arc::new(Self {
            cost,
            gap: m.gap,
            line_gap: m.line_gap,
            padding: m.padding,
            size: m.size,
            floor: m.floor,
            content: m.content,
            fluid: m.fluid,
            cols: m.cols,
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
    context: Option<(Limits, SpacingScale)>,
    pinned: bool,
    /// The root's revision and offered size, and what they resolved to.
    last: Option<(u64, Option<[u64; 2]>, Layout)>,
    /// The size offered last call, cached or not.
    sized: Option<Option<[u64; 2]>>,
    stats: LayoutStats,
}
impl LayoutCache {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn stats(&self) -> LayoutStats {
        self.stats
    }
    /// Constraint variants kept per node: a first pass, a flex re-measure and
    /// a couple of sizes to go back to. Fewer trades hit rate for memory,
    /// never correctness.
    const VARIANTS: usize = 4;
    /// Validates the tree and returns the root's revision.
    fn prepare<P>(
        &mut self,
        n: &Node<P>,
        limits: Limits,
        scale: SpacingScale,
        key: &mut impl FnMut(&P, &mut Vec<u8>),
    ) -> Result<u64, Error> {
        // A context change invalidates every metric. Limits cannot be bypassed.
        if self.context != Some((limits, scale)) {
            *self = Self {
                context: Some((limits, scale)),
                sized: self.sized,
                ..Self::default()
            };
        }
        self.pointers.clear();
        self.pinned = false;
        self.stats = LayoutStats::default();
        self.pass = self.pass.wrapping_add(1);
        let mut scan = Scan {
            limits,
            key,
            pass: self.pass,
            count: 0,
            revisions: std::mem::take(&mut self.revisions),
            payload: std::mem::take(&mut self.payload),
        };
        let walked = self.scan(n, 0, 0, &mut scan);
        (self.revisions, self.payload) = (scan.revisions, scan.payload);
        // A failed walk returns before popping its path's revisions.
        self.revisions.clear();
        let revision = walked?;
        self.stamps.retain(|_, s| s.seen == scan.pass);
        self.stats.validated_nodes = scan.count;
        Ok(revision)
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
    ) -> Result<u64, Error> {
        if depth > s.limits.depth || s.count >= s.limits.nodes {
            return Err(Error::BudgetExceeded);
        }
        s.count += 1;
        self.pinned |= n.rare().pin.is_some();
        let base = s.revisions.len();
        for (i, c) in n.children().iter().enumerate() {
            let revision = self.scan(c, fx((path, i)), depth + 1, s)?;
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
                if !n.scrolled.iter().all(|v| v.is_finite()) {
                    return Err(Error::InvalidValue);
                }
                self.serial = self.serial.checked_add(1).ok_or(Error::RevisionExhausted)?;
                let stamp = Stamp {
                    shape,
                    rare: n.rare.clone(),
                    children: children.to_vec(),
                    revision: self.serial,
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
        Ok(revision)
    }
    /// The stamp `scan` gave this node.
    fn stamp<P>(&mut self, n: &Node<P>) -> &mut Stamp {
        let address = self.pointers[&(n as *const Node<P> as usize)];
        self.stamps
            .get_mut(&address)
            .expect("scan stamped every node")
    }
}
/// Complete shallow layout projection; destructuring without `..` makes adding
/// a layout field a compile-time prompt to update invalidation. Payload values
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
        scrolled,
        sticky,
        float,
        wrap,
        span,
        order,
    } = n;
    let kind = match kind {
        Kind::Leaf => Kind::Leaf,
        Kind::Content => Kind::Content,
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
        scrolled: *scrolled,
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
        let layout = super::resolve_impl(root, offered, limits, scale, measurer, None, None)?;
        cache.stats = LayoutStats {
            arranged_nodes: layout.all().len(),
            ..LayoutStats::default()
        };
        return Ok(layout);
    }
    let revision = cache.prepare(root, limits, scale, &mut key)?;
    if let Some((r, o, layout)) = &cache.last
        && (*r, *o) == (revision, offered_bits)
    {
        return Ok(layout.clone());
    }
    let layout = super::resolve_impl(root, offered, limits, scale, measurer, Some(cache), None)?;
    cache.stats.arranged_nodes = layout.all().len();
    cache.last = Some((revision, offered_bits, layout.clone()));
    Ok(layout)
}
/// Hook around the ORIGINAL measure implementation. Re-measure requests under
/// different flex constraints receive different keys, including room/container.
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
    let key = MeasureInput {
        redo: pass.redo,
        definite: definite.map(|v| v.map(f64::to_bits)),
        room: room.map(f64::to_bits),
        container: container.map(|v| v.map(f64::to_bits)),
    };
    let hit = cache.stamp(node).measured.iter().find(|(k, _)| *k == key);
    if let Some(snapshot) = hit.map(|(_, f)| f.clone()) {
        if !pass.redo {
            if snapshot.cost > pass.left {
                return Err(Error::BudgetExceeded);
            }
            pass.left -= snapshot.cost;
        }
        pass.pinned |= cache.pinned;
        cache.stats.measure_hits += 1;
        return Ok(Frozen::bind(&snapshot, node, &mut cache.stats));
    }
    cache.stats.measured_nodes += 1;
    let before = pass.left;
    let mut m = measure::measure_uncached(node, ancestor, definite, room, container, depth, pass)?;
    let frozen = Frozen::freeze(&m, before - pass.left);
    m.frozen = Some(frozen.clone());
    let cache = pass.cache.as_deref_mut().expect("checked above");
    let measured = &mut cache.stamp(node).measured;
    if measured.len() >= LayoutCache::VARIANTS {
        measured.remove(0);
    }
    measured.push((key, frozen));
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
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
            Node::column([label("a", "A short label"), label("b", "Another label")])
                .id("left")
                .grow(1.),
            Node::column([label("c", "Static right panel"), label("d", "Stable")])
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
        let mut n = Node::overlay([
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
        let mut n = Node::overlay([
            Node::row([label("x", "One"), Node::leaf(30., 10.)]),
            Node::row([Node::leaf(50., 10.), label("y", "Two")]),
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
