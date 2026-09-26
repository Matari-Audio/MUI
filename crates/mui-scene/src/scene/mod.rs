//! The tree becomes paint.
//!
//! One pre-order walk over the styled tree, in step with the frames the
//! solver produced in the same order. Every node gets an outline; every layer
//! of its [`Style`](crate::Style) becomes one [`Painted`] entry. Children
//! paint after their parent, so a list index is a z-order.
pub mod bar;
mod material;
mod outline;
mod paint;
mod partition;
mod resolved;
mod spec;
mod text;
mod walk;

pub use resolved::{Layer, Painted, PlacedPath, ResolvedScene, ResolvedSurface, Text, TextGlyph};
pub use spec::{SceneError, SceneSpec};
pub(crate) use text::TextState;

use rustc_hash::FxHashMap as HashMap;
use std::borrow::Cow;
use std::sync::{Arc, LazyLock};

use mui_geometry::{OffsetOptions, Path, PlacedShape, Rect};
use mui_layout::{Frame, Id};

use crate::{Color, Cursor, El, Size};
use outline::OutlineCache;
use text::{Runs, fit, layout_key};

/// The key child `c`, the `j`th of the node at `parent`, gets: its id, or
/// its tree path, the `/0/2` key the scene gives a node without an id.
fn child_key(c: &El, parent: &str, j: usize) -> Id {
    c.ident()
        .cloned()
        .unwrap_or_else(|| Id::runtime_slot(parent, j))
}

/// Append child `j`'s step to a tree path held in a `String` buffer, for a
/// walk that grows and truncates one path. By hand: `write!` is most of a
/// walk's cost. Hidden: it is the runtime's
/// shared spelling of [`Id::runtime_slot`], not an authoring API.
#[doc(hidden)]
pub fn push_index(path: &mut String, mut j: usize) {
    path.push('/');
    let at = path.len();
    loop {
        path.insert(at, char::from(b'0' + (j % 10) as u8));
        j /= 10;
        if j == 0 {
            break;
        }
    }
}

/// A length on the device grid, or untouched when the host gave no scale.
fn snap(v: f64, scale: Option<f64>) -> f64 {
    scale.map_or(v, |s| (v * s).round() / s)
}
/// The one place every outline, weld rect and clip path comes from, so
/// snapping here cannot leave paint, hits and clips disagreeing.
fn bounds(f: Frame, scale: Option<f64>) -> Rect {
    Rect::new(
        snap(f.x, scale),
        snap(f.y, scale),
        snap(f.right(), scale),
        snap(f.bottom(), scale),
    )
}
/// The path a structural or glyph entry carries: empty, and shared, so
/// closing a clip allocates nothing.
fn empty() -> Arc<Path> {
    static EMPTY: LazyLock<Arc<Path>> = LazyLock::new(|| Arc::new(Path::default()));
    EMPTY.clone()
}

/// A resolved outline back as boolean input.
fn polygons(path: &Path) -> Result<Vec<PlacedShape>, SceneError> {
    Ok(mui_geometry::offset_path(
        path,
        0.,
        OffsetOptions {
            flatten_tolerance: 0.25,
            max_points: 100_000,
            ..OffsetOptions::default()
        },
    )?
    .topology
    .placed_shapes())
}

/// Every node's subtree size, by pre-order index: the subtree at `at` ends
/// just before `at + sizes[at]`. Computed once per resolve, so skipping a
/// subtree is a lookup instead of a walk.
fn subtree_sizes(n: &El, sizes: &mut Vec<usize>) -> usize {
    let at = sizes.len();
    sizes.push(1);
    let size = 1 + n
        .children()
        .iter()
        .map(|c| subtree_sizes(c, sizes))
        .sum::<usize>();
    sizes[at] = size;
    size
}

/// Per pre-order index, whether the node's parent is
/// `segmented` (`mui_material::Material`). Read at resolve time, so a child
/// pushed after `.segmented()` is squared too.
fn squared(n: &El, parent: bool, out: &mut Vec<bool>) {
    out.push(parent);
    for c in n.children() {
        squared(c, n.payload().has(crate::Element::SEGMENTED), out);
    }
}

/// The pre-order index of the node keyed `id` in the subtree `n` rooted at `at`.
fn find(n: &El, id: &str, at: usize, sizes: &[usize]) -> Option<usize> {
    if n.key() == Some(id) {
        return Some(at);
    }
    let mut next = at + 1;
    for c in n.children() {
        if let Some(i) = find(c, id, next, sizes) {
            return Some(i);
        }
        next += sizes[next];
    }
    None
}

/// The error for a cross-reference `find` did not resolve.
fn missing(what: &'static str, id: &mui_layout::Id) -> SceneError {
    SceneError::MissingId {
        what,
        id: id.clone(),
    }
}

/// What a node inherits from the nodes above it.
#[derive(Clone, Debug, Default)]
struct Ancestors {
    parent: Option<Id>,
    clip: Option<Rect>,
    clip_paths: Option<Arc<[PlacedPath]>>,
    cursor: Option<Cursor>,
    disabled: bool,
}

impl Ancestors {
    /// The same inheritance, comparing the clip outlines by pointer first.
    fn same(&self, o: &Self) -> bool {
        let paths = match (&self.clip_paths, &o.clip_paths) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a == b,
            (a, b) => a.is_none() && b.is_none(),
        };
        paths
            && self.parent == o.parent
            && self.clip == o.clip
            && self.cursor == o.cursor
            && self.disabled == o.disabled
    }
}

/// Where one memoised subtree landed in a resolve: its node range, paint
/// and surfaces, and everything its paint depended on from outside, so the
/// next resolve can copy the lot instead of walking a reused subtree.
#[derive(Clone, Debug)]
pub(crate) struct MemoSpan {
    id: u64,
    /// Copied from the resolve before, or walked from a reused tree.
    reused: bool,
    /// Pre-order index and node count.
    at: usize,
    size: usize,
    paint: std::ops::Range<usize>,
    surfaces: std::ops::Range<usize>,
    /// How many of the spans after this one lie inside it.
    nested: usize,
    origin: mui_geometry::Point,
    path: String,
    under: Color,
    base_y: Option<f64>,
    ancestors: Ancestors,
    /// Nothing of it painted elsewhere (a float) or came from outside (a
    /// surface owner's region); only a closed span is copied.
    closed: bool,
    /// It floated a node, whose paint and surface land outside the span.
    floats: bool,
    /// The resolve whose walk last filled its caches' entries.
    generation: u64,
}
impl PartialEq for MemoSpan {
    // Bookkeeping, not what the scene shows.
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// A copied span's cache entries are kept alive this many resolves past
/// its last walk; after that it is walked again to renew them.
pub(super) const MEMO_AGE: u64 = 60;

/// A float, painted after the whole tree so it sits on top and escapes
/// every clip.
#[derive(Clone)]
struct Deferred<'a> {
    at: usize,
    node: &'a El,
    path: String,
    under: Color,
    ancestors: Ancestors,
}

/// The resolve of one [`SceneSpec`]: a pre-order walk over the solved
/// frames that paints each node, records its surface and hands material
/// plans down to the descendants that consume them.
struct Walk<'a> {
    spec: &'a SceneSpec,
    tree: Tree<'a>,
    caches: Caches<'a>,
    runs: Runs<'a>,
    plan: Plan,
    out: Out<'a>,
    memo: Memos<'a>,
    /// The pre-order index of the next node.
    i: usize,
    /// The node being painted; its outline cache and paint are named by it.
    key: Id,
    /// The baseline a `.baseline()` parent asks its text children to sit on.
    base_y: Option<f64>,
    /// Ink and dim per ground colour's bits; see [`Walk::paint_of`].
    inks: HashMap<[u32; 4], (Color, Color)>,
}

/// The solved tree, by pre-order index.
struct Tree<'a> {
    frames: Cow<'a, [Frame]>,
    /// Subtree size; see [`subtree_sizes`].
    sizes: Vec<usize>,
    /// The parent is segmented, so the corners are square.
    squared: Vec<bool>,
}

/// The [`Resolver`]'s caches the walk reads and refills.
struct Caches<'a> {
    outlines: &'a mut OutlineCache,
    borders: &'a mut crate::border_ramp::BorderCache,
    regions: &'a mut crate::regions::RegionCache,
    surfaces: &'a mut crate::surfaces::Cache,
    welds: &'a mut crate::WeldCache,
}

/// What a material owner (a region split, a surface layout, a ramp) decided
/// for nodes below it, by their pre-order index, for them to pick up when the
/// walk reaches them.
#[derive(Default)]
struct Plan {
    /// Each region child's outline, placed.
    regions: ByNode<PlacedPath>,
    region_envelopes: ByNode<Arc<Path>>,
    /// Border joins a surface owner adds to its ramp band.
    surface_joins: ByNode<Path>,
    /// Nodes whose `.join_border(..)` an owner resolved.
    joined_nodes: ByNode<()>,
    ramp_anchors: ByNode<Frame>,
    ramp_frames: HashMap<(usize, Id), Frame>,
}
impl Plan {
    /// Whether anything in `range` takes a region, an envelope, a join or a
    /// ramp anchor from the walk around it.
    fn feeds(&self, range: std::ops::Range<usize>) -> bool {
        self.regions.any_in(range.clone())
            || self.region_envelopes.any_in(range.clone())
            || self.joined_nodes.any_in(range.clone())
            || self.ramp_anchors.any_in(range.clone())
            || self.surface_joins.any_in(range)
    }
}

/// A value for some nodes, by pre-order index. Holds nothing until the
/// first insert, then one slot per node, so a scene without materials pays
/// nothing and one with them looks each node up by index.
struct ByNode<T>(Vec<Option<T>>);
impl<T> Default for ByNode<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}
impl<T> ByNode<T> {
    fn get(&self, i: usize) -> Option<&T> {
        self.0.get(i)?.as_ref()
    }
    fn contains(&self, i: usize) -> bool {
        self.get(i).is_some()
    }
    fn slot(&mut self, i: usize) -> &mut Option<T> {
        if i >= self.0.len() {
            self.0.resize_with(i + 1, || None);
        }
        &mut self.0[i]
    }
    fn insert(&mut self, i: usize, v: T) {
        *self.slot(i) = Some(v);
    }
    fn get_or_insert(&mut self, i: usize, v: T) -> &mut T {
        self.slot(i).get_or_insert(v)
    }
    fn remove(&mut self, i: usize) -> Option<T> {
        self.0.get_mut(i)?.take()
    }
    fn any_in(&self, range: std::ops::Range<usize>) -> bool {
        let end = range.end.min(self.0.len());
        self.0
            .get(range.start.min(end)..end)
            .is_some_and(|s| s.iter().any(Option::is_some))
    }
}

/// The scene the walk is building.
struct Out<'a> {
    paint: Vec<Painted>,
    surfaces: Vec<ResolvedSurface>,
    at: HashMap<Id, usize>,
    external_welds: HashMap<Id, crate::ExternalWeld>,
    /// Floats, painted after the tree in the order they were met.
    deferred: Vec<Deferred<'a>>,
}

/// Memoised subtrees: the spans this walk records and the scene it copies
/// reused ones from.
struct Memos<'a> {
    /// The resolve before, whose memo spans a reused subtree copies.
    prev: Option<&'a ResolvedScene>,
    spans: Vec<MemoSpan>,
    /// How many resolves back the oldest copied span was walked.
    age: u64,
}

/// Walk the tree in layout order handing each animating node's frame to
/// `glide`. `anchor` is the nearest animating ancestor's solved and shown
/// origin, `[tx, ty, sx, sy]`; everything under it is offset by the
/// difference. Keys match the runtime's: the id, or the `/0/2` tree path.
fn glide_frames(
    n: &El,
    at: &mut usize,
    path: &mut String,
    anchor: [f64; 4],
    frames: &mut Cow<'_, [Frame]>,
    glide: &mut dyn FnMut(&Id, &crate::Element, Frame) -> Frame,
) {
    let i = *at;
    *at += 1;
    let Some(&target) = frames.get(i) else {
        return;
    };
    let [tx, ty, sx, sy] = anchor;
    let mut inner = anchor;
    if n.payload().extras().layout_transition.is_some() {
        let rel = Frame {
            x: target.x - tx,
            y: target.y - ty,
            ..target
        };
        let anonymous;
        let key = if let Some(id) = n.ident() {
            id
        } else {
            anonymous = Id::runtime(path);
            &anonymous
        };
        let got = glide(key, n.payload(), rel);
        let ok = [got.x, got.y, got.size.width, got.size.height]
            .iter()
            .all(|v| v.is_finite());
        let shown = if ok {
            Frame {
                x: sx + got.x,
                y: sy + got.y,
                size: Size::new(got.size.width.max(0.), got.size.height.max(0.)),
            }
        } else {
            target
        };
        if shown != target {
            frames.to_mut()[i] = shown;
        }
        inner = [target.x, target.y, shown.x, shown.y];
    } else if (sx, sy) != (tx, ty) {
        let f = &mut frames.to_mut()[i];
        f.x += sx - tx;
        f.y += sy - ty;
    }
    let mark = path.len();
    for (j, c) in n.children().iter().enumerate() {
        push_index(path, j);
        glide_frames(c, at, path, inner, frames, glide);
        path.truncate(mark);
    }
}

/// Resolve `spec` once, with cold caches. Anything that resolves every
/// frame keeps a [`Resolver`] instead.
pub fn resolve(spec: &SceneSpec) -> Result<ResolvedScene, SceneError> {
    let mut r = Resolver::default();
    r.resolve(spec)?;
    Ok(r.prev.take().expect("resolve keeps its scene"))
}

/// The caches a resolve reuses across calls: shaped text, layout, outlines,
/// material welds and the last scene, which reused [`Memo`](crate::Memo)
/// subtrees are copied from. Keep one per window.
#[derive(Default)]
pub struct Resolver {
    pub(crate) text: TextState,
    pub welds: crate::WeldCache,
    prev: Option<ResolvedScene>,
}
impl Resolver {
    pub fn new() -> Self {
        Self::default()
    }
    /// Hand a scene you are done with back, so the next resolve fills its
    /// buffers instead of growing new ones. Only for scenes from
    /// [`Resolver::resolve_after`]: [`Resolver::resolve`] recycles its own.
    pub fn recycle(&mut self, scene: ResolvedScene) {
        self.text.recycle(scene);
    }
    pub fn layout_stats(&self) -> mui_layout::LayoutStats {
        self.text.layout_stats()
    }
    /// Shaped text runs held: one per (string, size, face, axes) variant.
    pub fn text_runs(&self) -> usize {
        self.text.len()
    }
    /// Solve, shape and paint `spec`. The scene is kept until the next
    /// call, which copies every reused [`Memo`](crate::Memo) subtree out of
    /// it instead of walking it again; clone it to keep it longer.
    pub fn resolve(&mut self, spec: &SceneSpec) -> Result<&ResolvedScene, SceneError> {
        let prev = self.prev.take();
        let (text, welds) = (&mut self.text, &mut self.welds);
        let scene = match resolve_with(spec, text, welds, &mut |_, _, f| f, prev.as_ref()) {
            Ok(scene) => scene,
            Err(e) => {
                self.prev = prev;
                return Err(e);
            }
        };
        if let Some(old) = prev {
            self.text.recycle(old);
        }
        Ok(self.prev.insert(scene))
    }
    /// The frame after `prev`, for a runtime that keeps its scenes itself:
    /// [`Resolver::resolve`] with every
    /// [`animate_layout`](crate::Styled::animate_layout) node's frame handed
    /// to `glide` between the solve and the walk: `glide(key, element,
    /// target)` returns the frame to paint, clip and hit it at. `target` is
    /// relative to the nearest animating ancestor's *solved* origin
    /// (absolute for the outermost), and so is the answer, so a nested glide
    /// is never chased twice. A node that does not animate moves with its
    /// nearest animating ancestor. The runtime's springs live in `glide`;
    /// this only places them.
    ///
    /// Every reused [`Memo`](crate::Memo) subtree is painted by copying its
    /// paint and surfaces out of `prev`, the scene the previous call
    /// returned, when nothing it depended on from outside moved --
    /// translated when only its origin did. The copy keeps every `Arc`, so a
    /// renderer comparing by pointer sees it unchanged, and it keeps the
    /// caches' entries the subtree used alive. This does not touch the scene
    /// [`Resolver::resolve`] keeps.
    pub fn resolve_after(
        &mut self,
        spec: &SceneSpec,
        glide: &mut dyn FnMut(&Id, &crate::Element, Frame) -> Frame,
        prev: Option<&ResolvedScene>,
    ) -> Result<ResolvedScene, SceneError> {
        let (text, weld_cache) = (&mut self.text, &mut self.welds);
        resolve_with(spec, text, weld_cache, glide, prev)
    }
}

// The internal tests read the text caches' insides, so they resolve
// against a bare `TextState` with a cold weld cache.
#[cfg(test)]
impl TextState {
    pub(crate) fn resolve(&mut self, spec: &SceneSpec) -> Result<ResolvedScene, SceneError> {
        resolve_with(
            spec,
            self,
            &mut crate::WeldCache::default(),
            &mut |_, _, f| f,
            None,
        )
    }
}

fn resolve_with(
    spec: &SceneSpec,
    text: &mut TextState,
    weld_cache: &mut crate::WeldCache,
    glide: &mut dyn FnMut(&Id, &crate::Element, Frame) -> Frame,
    prev: Option<&ResolvedScene>,
) -> Result<ResolvedScene, SceneError> {
    spec.validate()?;
    text.retain_for(spec);
    let mut runs = Runs {
        fonts: spec
            .font
            .iter()
            .chain(&spec.fallback_fonts)
            .cloned()
            .collect(),
        own_fonts: None,
        scale: spec.device_scale,
        generation: text.generation,
        cache: &mut text.runs,
        breaks: &mut text.breaks,
        coords: &mut text.coords,
        last_coords: &mut text.last_coords,
    };
    let th = spec.theme;
    // Every paragraph wraps in this one pass: mui-layout hands a flex item's
    // final main size back to the measurer, so there is no share left to learn
    // afterwards.
    let layout = mui_layout::resolve_cached_with(
        &spec.root,
        spec.offered,
        spec.limits,
        th.spacing,
        &mut text.layout,
        |e, out| layout_key(e, th, spec.device_scale, out),
        |e, room| fit(&mut runs, th, e, room),
    )?;
    let nodes = layout.all().len();
    let mut sizes = Vec::with_capacity(nodes);
    subtree_sizes(&spec.root, &mut sizes);
    let mut square = Vec::with_capacity(nodes);
    squared(&spec.root, false, &mut square);
    let mut frames = Cow::Borrowed(layout.all());
    glide_frames(
        &spec.root,
        &mut 0,
        &mut String::new(),
        [0.; 4],
        &mut frames,
        glide,
    );
    let (paint, mut surfaces, mut at) = std::mem::take(&mut text.spare);
    surfaces.reserve(nodes);
    at.reserve(nodes);
    let mut w = Walk {
        spec,
        tree: Tree {
            frames,
            sizes,
            squared: square,
        },
        caches: Caches {
            outlines: &mut text.outlines,
            borders: &mut text.borders,
            regions: &mut text.region_cache,
            surfaces: &mut text.surface_cache,
            welds: weld_cache,
        },
        runs,
        plan: Plan::default(),
        out: Out {
            paint,
            surfaces,
            at,
            external_welds: HashMap::default(),
            deferred: Vec::new(),
        },
        memo: Memos {
            prev,
            spans: Vec::new(),
            age: 0,
        },
        i: 0,
        key: Id::runtime(""),
        base_y: None,
        inks: HashMap::default(),
    };
    w.node(
        &spec.root,
        &mut String::new(),
        th.palette.background(),
        &Ancestors::default(),
    )?;
    // Floats paint last, in the order they were met; a float inside a float
    // lands on the end of the same queue.
    let mut k = 0;
    while k < w.out.deferred.len() {
        let Deferred {
            at,
            node,
            mut path,
            under,
            ancestors,
        } = w.out.deferred[k].clone();
        w.i = at;
        w.base_y = None;
        w.node(node, &mut path, under, &ancestors)?;
        k += 1;
    }
    let Walk {
        tree: Tree { frames, .. },
        out:
            Out {
                paint,
                surfaces,
                at,
                external_welds,
                ..
            },
        memo: Memos {
            spans: memos, age, ..
        },
        ..
    } = w;
    let layout = match frames {
        Cow::Owned(frames) => layout.reframe(&spec.root, frames)?,
        Cow::Borrowed(_) => layout,
    };
    text.sweep(age);
    Ok(ResolvedScene {
        layout,
        paint,
        surfaces,
        at,
        external_welds,
        memos,
    })
}

#[cfg(test)]
use fixtures::{font, welded_tab};
#[cfg(test)]
mod fixtures {
    use crate::Corners;
    use crate::prelude::*;

    /// The canonical union: a tab welded to its panel, with a pill shell inside
    /// the tab.
    pub fn welded_tab() -> SceneSpec {
        let tab = col([block(28., 28.), block(28., 28.), block(28., 28.)])
            .gap(10.)
            .pad(22.)
            .min_width(92.)
            .align(Align::Center)
            .id("tab")
            .shell(12., Role::Raised);
        let root = col([tab, block(520., 230.).id("panel")])
            .align(Align::Start)
            .id("root")
            .union(Role::Surface);
        SceneSpec::new(root).theme(Theme {
            corners: Corners {
                box_: 28.,
                concave: 32.,
                ..Corners::DEFAULT
            },
            ..Theme::default()
        })
    }

    pub fn font() -> Font {
        Font::new(epaint_default_fonts::HACK_REGULAR).unwrap()
    }
}
