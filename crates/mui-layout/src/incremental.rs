//! Dependency-aware measurement and arrangement caches for the existing solver.
//!
//! This is not a second layout algorithm. Cache misses run the original solver.
//! The prepass validates the complete declaration and compares exact shallow
//! layout values plus child revisions; decorative payload changes are excluded
//! by the caller's measurement key. Current trees are still visited and the flat
//! output is still copied. Pins conservatively disable arrangement reuse.
use super::*;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutStats {
    pub validated_nodes: usize,
    pub measured_nodes: usize,
    pub measure_hits: usize,
    pub arranged_nodes: usize,
    pub arrange_hits: usize,
    pub rebound_nodes: usize,
    pub copied_frames: usize,
    pub pinned_arrange_fallback: bool,
}
#[derive(Clone, Debug, PartialEq)]
struct Stamp {
    shape: Node<Vec<u8>>,
    children: Vec<u64>,
    revision: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct MeasureInput {
    revision: u64,
    /// A flex re-measure at the final share resolves declared widths
    /// differently from the first pass, so the two never share a snapshot.
    redo: bool,
    definite: [Option<u64>; 2],
    room: Option<u64>,
    container: [Option<u64>; 2],
}
#[derive(Clone, Debug)]
struct Frozen {
    ticket: u64,
    cost: usize,
    gap: f64,
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
    fn bind<'a, P>(&self, n: &'a Node<P>, stats: &mut LayoutStats) -> Measured<'a, P> {
        stats.rebound_nodes += 1;
        Measured {
            node: n,
            index: 0,
            memo_id: self.ticket,
            fluid: self.fluid,
            gap: self.gap,
            padding: self.padding,
            size: self.size,
            floor: self.floor,
            content: self.content,
            cols: self.cols,
            pick: self.pick,
            container: self.container,
            children: self
                .children
                .iter()
                .zip(n.children())
                .enumerate()
                .map(|(i, (c, n))| {
                    let mut m = c.bind(n, stats);
                    m.index = i;
                    m
                })
                .collect(),
        }
    }
    fn freeze<P>(m: &Measured<'_, P>, cost: usize) -> Arc<Self> {
        Arc::new(Self {
            ticket: m.memo_id,
            cost,
            gap: m.gap,
            padding: m.padding,
            size: m.size,
            floor: m.floor,
            content: m.content,
            fluid: m.fluid,
            cols: m.cols,
            pick: m.pick,
            container: m.container,
            children: m.children.iter().map(|c| Self::freeze(c, 0)).collect(),
        })
    }
    fn count(&self) -> usize {
        1 + self.children.iter().map(|c| c.count()).sum::<usize>()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ArrangeInput {
    ticket: u64,
    origin: [u64; 2],
    size: [u64; 2],
    viewport: Option<(bool, u64)>,
}
#[derive(Clone, Debug)]
struct Arrangement {
    frames: Vec<Frame>,
    named: Vec<(Id, Frame)>,
}
#[derive(Debug, Default)]
pub(crate) struct ArrangementCache {
    entries: HashMap<ArrangeInput, Arc<Arrangement>>,
    order: VecDeque<ArrangeInput>,
    frames: usize,
    pub(crate) stats: LayoutStats,
}
#[derive(Debug, Default)]
pub struct LayoutCache {
    stamps: HashMap<String, Stamp>,
    pointers: HashMap<usize, u64>,
    entries: HashMap<MeasureInput, Arc<Frozen>>,
    order: VecDeque<MeasureInput>,
    snapshot_nodes: usize,
    serial: u64,
    ticket: u64,
    context: Option<String>,
    pinned: bool,
    pub(crate) arrangement: RefCell<ArrangementCache>,
}
impl LayoutCache {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn stats(&self) -> LayoutStats {
        self.arrangement.borrow().stats
    }
    /// Bounds both variant count and retained subtree records. A small value
    /// trades hit rate for memory, never correctness.
    const MAX_ENTRIES: usize = 2048;
    const MAX_RECORDS: usize = 32768;
    fn prepare<P>(
        &mut self,
        n: &Node<P>,
        limits: Limits,
        scale: SpacingScale,
        key: &mut impl FnMut(&P) -> Vec<u8>,
    ) -> Result<(), Error> {
        // Context changes invalidate metrics AND arrangement. Cache tickets may
        // be reused only after both stores are gone. Limits cannot be bypassed.
        let context = format!("{limits:?}/{scale:?}");
        if self.context.as_ref() != Some(&context) {
            self.clear();
            self.context = Some(context);
        }
        self.pointers.clear();
        self.pinned = false;
        self.arrangement.borrow_mut().stats = LayoutStats::default();
        let mut names = HashSet::new();
        let mut live = HashSet::new();
        let mut count = 0;
        self.scan(n, "", 0, limits, key, &mut names, &mut live, &mut count)?;
        self.stamps.retain(|k, _| live.contains(k));
        let mut a = self.arrangement.borrow_mut();
        a.stats.validated_nodes = count;
        a.stats.pinned_arrange_fallback = self.pinned;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    fn scan<P>(
        &mut self,
        n: &Node<P>,
        path: &str,
        depth: usize,
        limits: Limits,
        key: &mut impl FnMut(&P) -> Vec<u8>,
        names: &mut HashSet<String>,
        live: &mut HashSet<String>,
        count: &mut usize,
    ) -> Result<u64, Error> {
        if depth > limits.depth || *count >= limits.nodes {
            return Err(Error::BudgetExceeded);
        }
        *count += 1;
        measure::validate_node(n, limits)?;
        if !n.scrolled.iter().all(|v| v.is_finite()) {
            return Err(Error::InvalidValue);
        }
        if let Some(id) = n.key() {
            if !names.insert(id.to_owned()) {
                return Err(Error::DuplicateKey(id.to_owned()));
            }
        }
        self.pinned |= n.pin.is_some();
        let address = n
            .key()
            .map_or_else(|| format!("path:{path}"), |id| format!("id:{id}"));
        let child_revisions = n
            .children()
            .iter()
            .enumerate()
            .map(|(i, c)| {
                self.scan(
                    c,
                    &format!("{path}/{i}"),
                    depth + 1,
                    limits,
                    key,
                    names,
                    live,
                    count,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let shape = projection(n, key(&n.payload));
        let unchanged = self
            .stamps
            .get(&address)
            .is_some_and(|s| s.shape == shape && s.children == child_revisions);
        let revision = if unchanged {
            self.stamps[&address].revision
        } else {
            self.serial = self.serial.checked_add(1).ok_or(Error::RevisionExhausted)?;
            self.stamps.insert(
                address.clone(),
                Stamp {
                    shape,
                    children: child_revisions,
                    revision: self.serial,
                },
            );
            self.serial
        };
        live.insert(address);
        self.pointers.insert(n as *const Node<P> as usize, revision);
        Ok(revision)
    }
    fn input<P>(
        &self,
        n: &Node<P>,
        redo: bool,
        definite: [Option<f64>; 2],
        room: Option<f64>,
        container: [Option<f64>; 2],
    ) -> MeasureInput {
        MeasureInput {
            revision: self.pointers[&(n as *const Node<P> as usize)],
            redo,
            definite: definite.map(|v| v.map(f64::to_bits)),
            room: room.map(f64::to_bits),
            container: container.map(|v| v.map(f64::to_bits)),
        }
    }
    fn get(&mut self, key: MeasureInput) -> Option<Arc<Frozen>> {
        self.entries.get(&key).cloned()
    }
    fn store<P>(&mut self, key: MeasureInput, m: &Measured<'_, P>, cost: usize) {
        let snapshot = Frozen::freeze(m, cost);
        let n = snapshot.count();
        if n > Self::MAX_RECORDS {
            return;
        }
        while self.entries.len() >= Self::MAX_ENTRIES || self.snapshot_nodes + n > Self::MAX_RECORDS
        {
            if let Some(k) = self.order.pop_front() {
                if let Some(v) = self.entries.remove(&k) {
                    self.snapshot_nodes -= v.count();
                }
            } else {
                break;
            }
        }
        self.snapshot_nodes += n;
        self.entries.insert(key, snapshot);
        self.order.push_back(key);
    }
}
/// Complete shallow layout projection; destructuring without `..` makes adding
/// a layout field a compile-time prompt to update invalidation. Payload values
/// are exact bytes, not a hash used as a substitute for equality.
fn projection<P>(n: &Node<P>, payload: Vec<u8>) -> Node<Vec<u8>> {
    let Node {
        id,
        kind,
        payload: _,
        gap,
        padding,
        pad,
        minimum,
        maximum,
        width,
        height,
        aspect,
        grow,
        basis,
        shrink,
        align,
        align_self,
        justify,
        anchor,
        offset,
        pin,
        scroll,
        clip,
        scrolled,
        sticky,
        float,
        wrap,
        span,
        min_col,
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
        maximum: *maximum,
        width: *width,
        height: *height,
        aspect: *aspect,
        grow: *grow,
        basis: *basis,
        shrink: *shrink,
        align: *align,
        align_self: *align_self,
        justify: *justify,
        anchor: *anchor,
        offset: *offset,
        pin: pin.clone(),
        scroll: *scroll,
        clip: *clip,
        scrolled: *scrolled,
        sticky: *sticky,
        float: *float,
        wrap: *wrap,
        span: *span,
        min_col: *min_col,
        order: *order,
    }
}
/// Exact content-measurement keys are mandatory. Every captured external font,
/// locale or measurement policy must be included or explicitly clear the cache.
/// Decorator colours should not appear in the key. The existing DSL is unchanged.
#[allow(clippy::too_many_arguments)]
pub fn resolve_cached_with<P>(
    root: &Node<P>,
    offered: Option<Size>,
    limits: Limits,
    scale: SpacingScale,
    cache: &mut LayoutCache,
    mut key: impl FnMut(&P) -> Vec<u8>,
    measurer: impl FnMut(&P, Option<f64>) -> Size,
) -> Result<Layout, Error> {
    if !limits.extent.is_finite()
        || limits.extent <= 0.0
        || limits.nodes == 0
        || limits.depth > 256
        || !scale.is_valid()
    {
        return Err(Error::InvalidValue);
    }
    cache.prepare(root, limits, scale, &mut key)?;
    super::resolve_impl(root, offered, limits, scale, measurer, Some(cache))
}
/// Hook around the ORIGINAL measure implementation. Re-measure requests under
/// different flex constraints receive different keys, including room/container.
#[allow(clippy::too_many_arguments)]
pub(crate) fn measure_cached<'a, P>(
    node: &'a Node<P>,
    ancestor: &str,
    definite: [Option<f64>; 2],
    room: Option<f64>,
    container: [Option<f64>; 2],
    depth: usize,
    pass: &mut Pass<'a, '_, P>,
) -> Result<Measured<'a, P>, Error> {
    let key = pass
        .cache
        .as_ref()
        .map(|c| c.input(node, pass.redo, definite, room, container));
    if let (Some(cache), Some(key)) = (pass.cache.as_deref_mut(), key) {
        if let Some(snapshot) = cache.get(key) {
            if !pass.redo {
                if snapshot.cost > pass.left {
                    return Err(Error::BudgetExceeded);
                }
                pass.left -= snapshot.cost;
            }
            pass.pinned |= cache.pinned;
            let mut a = cache.arrangement.borrow_mut();
            a.stats.measure_hits += 1;
            return Ok(snapshot.bind(node, &mut a.stats));
        }
        cache.arrangement.borrow_mut().stats.measured_nodes += 1;
    }
    let before = pass.left;
    let mut m = measure::measure_uncached(node, ancestor, definite, room, container, depth, pass)?;
    if let (Some(cache), Some(key)) = (pass.cache.as_deref_mut(), key) {
        cache.ticket = cache
            .ticket
            .checked_add(1)
            .ok_or(Error::RevisionExhausted)?;
        m.memo_id = cache.ticket;
        cache.store(key, &m, before - pass.left);
    }
    Ok(m)
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn arrange_cached<P>(
    m: &Measured<'_, P>,
    ancestor: &str,
    origin: [f64; 2],
    size: Size,
    pins: &Pins<'_>,
    viewport: Option<Viewport>,
    out: &mut (BTreeMap<Id, Frame>, Vec<Frame>),
) -> Result<(), Error> {
    let key = ArrangeInput {
        ticket: m.memo_id,
        origin: origin.map(f64::to_bits),
        size: [size.width.to_bits(), size.height.to_bits()],
        viewport: viewport.map(|v| (v.vertical, v.edge.to_bits())),
    };
    if let Some(cache) = pins.memo {
        let mut c = cache.borrow_mut();
        if let Some(value) = c.entries.get(&key).cloned() {
            c.stats.arrange_hits += 1;
            c.stats.copied_frames += value.frames.len();
            out.1.extend_from_slice(&value.frames);
            out.0.extend(value.named.iter().cloned());
            return Ok(());
        }
        c.stats.arranged_nodes += 1;
    }
    let start = out.1.len();
    arrange::arrange_uncached(m, ancestor, origin, size, pins, viewport, out)?;
    if let Some(cache) = pins.memo {
        let frames = out.1[start..].to_vec();
        let mut named = Vec::new();
        fn keys<P>(m: &Measured<'_, P>, out: &BTreeMap<Id, Frame>, names: &mut Vec<(Id, Frame)>) {
            if let Some(id) = &m.node.id {
                if let Some(frame) = out.get(id) {
                    names.push((id.clone(), *frame));
                }
            }
            for c in &m.children {
                keys(c, out, names);
            }
        }
        keys(m, &out.0, &mut named);
        let n = frames.len();
        if n <= LayoutCache::MAX_RECORDS {
            let mut c = cache.borrow_mut();
            while c.entries.len() >= LayoutCache::MAX_ENTRIES
                || c.frames + n > LayoutCache::MAX_RECORDS
            {
                if let Some(k) = c.order.pop_front() {
                    if let Some(v) = c.entries.remove(&k) {
                        c.frames -= v.frames.len();
                    }
                } else {
                    break;
                }
            }
            c.frames += n;
            c.entries
                .insert(key, Arc::new(Arrangement { frames, named }));
            c.order.push_back(key);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Clone, Debug, Default)]
    struct Text {
        value: String,
        colour: u32,
    }
    fn key(t: &Text) -> Vec<u8> {
        t.value.as_bytes().to_vec()
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
    fn warm_tree_skips_measure_and_arrange() {
        let n = fixture();
        let mut c = LayoutCache::default();
        cached(&n, 400., &mut c);
        let l = cached(&n, 400., &mut c);
        assert_eq!(l, oracle(&n, 400.));
        let s = c.stats();
        assert_eq!(s.measured_nodes, 0);
        assert_eq!(s.arranged_nodes, 0);
        assert!(s.measure_hits > 0 && s.arrange_hits > 0);
    }
    #[test]
    fn decorative_payload_does_not_invalidate_layout() {
        let mut n = fixture();
        let mut c = LayoutCache::default();
        let before = cached(&n, 400., &mut c);
        n.children_mut()[0].children_mut()[0].payload_mut().colour = 42;
        assert_eq!(cached(&n, 400., &mut c), before);
        assert_eq!(c.stats().measured_nodes, 0);
    }
    #[test]
    fn changing_one_label_reuses_other_metrics() {
        let mut n = fixture();
        let mut c = LayoutCache::default();
        cached(&n, 400., &mut c);
        n.children_mut()[0].children_mut()[0].payload_mut().value = "Changed".into();
        assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
        assert!(c.stats().measure_hits > 0);
    }
    #[test]
    fn resize_and_wrap_match_uncached() {
        let n = fixture();
        let mut c = LayoutCache::default();
        for w in [400., 120., 640., 121., 400.] {
            assert_eq!(cached(&n, w, &mut c), oracle(&n, w));
        }
    }
    #[test]
    fn removed_node_does_not_leak_into_layout() {
        let mut n = fixture();
        let mut c = LayoutCache::default();
        cached(&n, 400., &mut c);
        n.children_mut()[0].children_mut()[0] = label("new", "Replacement");
        let l = cached(&n, 400., &mut c);
        assert!(l.frame("a").is_none());
        assert_eq!(l, oracle(&n, 400.));
    }
    #[test]
    fn duplicate_ids_are_validated_on_a_warm_cache() {
        let mut n = fixture();
        let mut c = LayoutCache::default();
        cached(&n, 400., &mut c);
        n.children_mut()[1].children_mut()[0] = label("a", "Duplicate");
        assert!(matches!(
            resolve_cached_with(
                &n,
                None,
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
        cached(&n, 400., &mut c);
        n.children_mut().swap(0, 1);
        assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
    }
    #[test]
    fn pin_dependencies_never_use_stale_arrangements() {
        let mut n = Node::overlay([
            label("anchor", "Hello"),
            label("tip", "Tip").pin(Pin::to("anchor")),
        ]);
        let mut c = LayoutCache::default();
        cached(&n, 400., &mut c);
        n.children_mut()[0] = label("anchor", "Longer anchor").offset(80., 0.);
        assert_eq!(cached(&n, 400., &mut c), oracle(&n, 400.));
        assert!(c.stats().pinned_arrange_fallback);
        assert_eq!(c.stats().arrange_hits, 0);
    }
    #[test]
    fn budget_changes_invalidate_cached_work() {
        let n = fixture();
        let mut c = LayoutCache::default();
        cached(&n, 400., &mut c);
        assert!(matches!(
            resolve_cached_with(
                &n,
                None,
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
        cached(&n, 400., &mut c);
        c.clear();
        cached(&n, 400., &mut c);
        assert!(c.stats().measured_nodes > 0);
    }
}
