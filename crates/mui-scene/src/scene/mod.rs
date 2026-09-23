//! The tree becomes paint.
//!
//! One pre-order walk over the styled tree, in step with the frames the
//! solver produced in the same order. Every node gets an outline; every layer
//! of its [`Style`](crate::Style) becomes one [`Painted`] entry. Children
//! paint after their parent, so a list index is a z-order.
mod material;
mod outline;
mod paint;
mod partition;
mod resolved;
mod spec;
mod text;
mod walk;

pub use resolved::{Layer, Painted, ResolvedScene, ResolvedSurface, Text, TextGlyph};
pub use spec::{SceneError, SceneSpec};
pub use text::TextCache;

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use mui_geometry::{Bounds, OffsetOptions, Path, PlacedShape, Point};
use mui_layout::Frame;

use crate::{Color, Cursor, El};
use outline::OutlineCache;
use text::{fit, layout_key, Runs};

/// A length on the device grid, or untouched when the host gave no scale.
fn snap(v: f64, scale: Option<f64>) -> f64 {
    scale.map_or(v, |s| (v * s).round() / s)
}
/// The one place every outline, weld rect and clip path comes from, so
/// snapping here cannot leave paint, hits and clips disagreeing.
fn bounds(f: Frame, scale: Option<f64>) -> Bounds {
    Bounds {
        min: Point::new(snap(f.x, scale), snap(f.y, scale)),
        max: Point::new(snap(f.right(), scale), snap(f.bottom(), scale)),
    }
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

/// What a node inherits from the nodes above it.
#[derive(Clone, Default)]
struct Ancestors {
    parent: Option<Arc<str>>,
    clip: Option<Bounds>,
    clip_paths: Option<Arc<[Path]>>,
    cursor: Option<Cursor>,
    disabled: bool,
}

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

struct Walk<'a> {
    spec: &'a SceneSpec,
    frames: Cow<'a, [Frame]>,
    regions: HashMap<usize, Path>,
    region_envelopes: HashMap<usize, Path>,
    runs: Runs<'a>,
    /// Subtree size per pre-order index; see [`subtree_sizes`].
    sizes: Vec<usize>,
    outlines: &'a mut OutlineCache,
    borders: &'a mut crate::border_ramp::BorderCache,
    region_cache: &'a mut crate::regions::RegionCache,
    surface_cache: &'a mut crate::surfaces::Cache,
    /// Border joins a surface owner adds to its ramp band.
    surface_joins: HashMap<usize, Path>,
    /// Nodes whose `.join_border(..)` an owner resolved.
    joined_nodes: std::collections::HashSet<usize>,
    ramp_anchors: HashMap<usize, Frame>,
    ramp_frames: HashMap<(usize, mui_layout::Id), Frame>,
    weld_cache: &'a mut crate::WeldCache,
    i: usize,
    key: Arc<str>,
    paint: Vec<Painted>,
    surfaces: Vec<ResolvedSurface>,
    at: HashMap<Arc<str>, usize>,
    pub(crate) external_welds: HashMap<Arc<str>, crate::ExternalWeld>,
    deferred: Vec<Deferred<'a>>,
    /// The baseline a `.baseline()` parent asks its text children to sit on.
    base_y: Option<f64>,
}

pub fn resolve_scene(spec: &SceneSpec) -> Result<ResolvedScene, SceneError> {
    resolve_scene_with(spec, &mut TextCache::default())
}

/// [`resolve_scene`] with text shaped once per (string, size) across calls.
/// Material welds still start cold every call; see [`resolve_scene_cached`].
pub fn resolve_scene_with(
    spec: &SceneSpec,
    text: &mut TextCache,
) -> Result<ResolvedScene, SceneError> {
    resolve_scene_cached(spec, text, &mut crate::WeldCache::default())
}

/// Resolve with persistent text and material-weld caches. A runtime keeps
/// both; [`resolve_scene`] and [`resolve_scene_with`] make a fresh
/// [`WeldCache`](crate::WeldCache) per call.
pub fn resolve_scene_cached(
    spec: &SceneSpec,
    text: &mut TextCache,
    weld_cache: &mut crate::WeldCache,
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
        |e, out| layout_key(e, th, out),
        |e, room| fit(&mut runs, th, e, room),
    )?;
    let nodes = layout.all().len();
    let mut sizes = Vec::with_capacity(nodes);
    subtree_sizes(&spec.root, &mut sizes);
    let mut w = Walk {
        spec,
        frames: Cow::Borrowed(layout.all()),
        regions: HashMap::new(),
        region_envelopes: HashMap::new(),
        runs,
        sizes,
        outlines: &mut text.outlines,
        borders: &mut text.borders,
        region_cache: &mut text.region_cache,
        surface_cache: &mut text.surface_cache,
        surface_joins: HashMap::new(),
        joined_nodes: std::collections::HashSet::new(),
        ramp_anchors: HashMap::new(),
        ramp_frames: HashMap::new(),
        weld_cache,
        i: 0,
        key: Arc::from(""),
        paint: Vec::new(),
        surfaces: Vec::with_capacity(nodes),
        at: HashMap::with_capacity(nodes),
        external_welds: HashMap::new(),
        deferred: Vec::new(),
        base_y: None,
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
    while k < w.deferred.len() {
        let Deferred {
            at,
            node,
            mut path,
            under,
            ancestors,
        } = w.deferred[k].clone();
        w.i = at;
        w.base_y = None;
        w.node(node, &mut path, under, &ancestors)?;
        k += 1;
    }
    let Walk {
        frames,
        paint,
        surfaces,
        at,
        external_welds,
        ..
    } = w;
    let layout = match frames {
        Cow::Owned(frames) => layout.reframe(&spec.root, frames)?,
        Cow::Borrowed(_) => layout,
    };
    text.sweep();
    Ok(ResolvedScene {
        layout,
        paint,
        surfaces,
        at,
        external_welds,
    })
}

#[cfg(test)]
use fixtures::{font, welded_tab};
#[cfg(test)]
mod fixtures {
    use crate::prelude::*;
    use crate::Corners;

    /// The canonical union: a tab welded to its panel, with a pill shell inside
    /// the tab.
    pub fn welded_tab() -> SceneSpec {
        let tab = column([leaf(28., 28.), leaf(28., 28.), leaf(28., 28.)])
            .gap(10.)
            .pad(22.)
            .min_width(92.)
            .align(Align::Center)
            .id("tab")
            .shell(12., Role::Raised);
        let root = column([tab, leaf(520., 230.).id("panel")])
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
