//! A node's outline: its own rounded frame, a weld of its children, or
//! either with carved children taken out -- and the cache for the costly ones.
use rustc_hash::FxHashMap as HashMap;
use std::sync::Arc;

use mui_geometry::{
    boolean, fillet, union, BooleanOp, Bounds, CornerStyle, Fillet, Path, Point, Polygon,
    RoundedRect, Topology,
};
use mui_layout::Frame;

use super::{bounds, polygons, SceneError, Walk};
use crate::{Carve, El, Radius};

/// A node's resolved outline, and what else its shape knows. Its geometry
/// is local: `offset` places it, so an outline that only moved is the same
/// `Arc`, and same-sized rects share one.
#[derive(Clone, Debug)]
pub(super) struct Contour {
    /// Shared by every layer, clip and surface drawn along it.
    pub(super) path: Arc<Path>,
    /// Exact rounded rectangle when the outline is one (not welded).
    pub(super) rect: Option<RoundedRect>,
    /// A shell collapsed or a merge changed ring counts.
    pub(super) changed: bool,
    /// Rounded rects that stand in for an outline with no `rect` when it
    /// casts a shadow: one per welded child, or a squircle's own frame.
    pub(super) shadow_rects: Vec<RoundedRect>,
    /// The path's bounds when already known; see [`Contour::bounds`].
    pub(super) known_bounds: Option<Bounds>,
    /// Where all of the above stands in the scene.
    pub(super) offset: Point,
}
impl Contour {
    /// A plain path in scene space: no analytic form, nothing changed.
    pub(super) fn path(path: impl Into<Arc<Path>>) -> Self {
        Self {
            path: path.into(),
            rect: None,
            changed: false,
            shadow_rects: Vec::new(),
            known_bounds: None,
            offset: Point::ZERO,
        }
    }

    /// The outline's local bounds: the rect's, the cached ones, or flattened.
    pub(super) fn bounds(&self) -> Result<Option<Bounds>, SceneError> {
        if let Some(r) = self.rect {
            return Ok(Some(r.bounds()));
        }
        if self.known_bounds.is_some() {
            return Ok(self.known_bounds);
        }
        Ok(Bounds::from_points(
            self.path.flatten(0.5, 100_000)?.concat(),
        ))
    }

    /// The path in scene space, for the geometry that mixes it with frames.
    pub(super) fn world(&self) -> Arc<Path> {
        if self.offset == Point::ZERO {
            return self.path.clone();
        }
        let mut p = (*self.path).clone();
        p.translate(self.offset);
        Arc::new(p)
    }

    pub(super) fn world_rect(&self) -> Option<RoundedRect> {
        self.rect.map(|r| r.translated(self.offset))
    }

    /// The same outline, local to `origin` instead.
    fn placed_at(mut self, origin: Point) -> Self {
        let d = self.offset - origin;
        if d != Point::ZERO {
            Arc::make_mut(&mut self.path).translate(d);
            self.rect = self.rect.map(|r| r.translated(d));
            for r in &mut self.shadow_rects {
                *r = r.translated(d);
            }
            self.known_bounds = self.known_bounds.map(|b| b.translated(d));
        }
        self.offset = origin;
        self
    }
}

/// Weld and carve outlines, the walk's most expensive geometry, keyed by
/// every input word that shaped them and compared in full: a hash match
/// alone is never trusted. Frames enter the key relative to the node's own
/// origin, and the contour is kept local to it, so a weld that moves or
/// scrolls is a hit, the same `Arc`s. Entries no resolve used are swept at
/// its end.
#[derive(Debug, Default)]
pub(super) struct OutlineCache {
    pub(super) entries: HashMap<Vec<u64>, Entry>,
    /// A key buffer, handed back after a hit so a warm frame builds keys
    /// without allocating.
    pub(super) scratch: Vec<u64>,
    pub(super) generation: u64,
    pub(super) hits: u64,
    pub(super) misses: u64,
    /// Each canvas's last draw list and its paths: a
    /// [`canvas_cached`](crate::canvas_cached) list is painted from the same
    /// paths every frame, wherever it moves. Holding the list keeps its
    /// address from being reused.
    pub(super) canvases: HashMap<Arc<str>, PlacedDraws>,
    /// Every local rounded-rect path, by the rect's words and corner style:
    /// one `Arc` for every node, and every frame, of that shape.
    pub(super) rects: HashMap<[u64; 6], (Arc<Path>, u64)>,
    /// The band each node's stroke paints along its outline.
    pub(super) bands: HashMap<Arc<str>, Band>,
}

/// The outline a stroke ran along, its width and alignment, the band, and
/// the resolve that last used it.
pub(super) type Band = (Arc<Path>, f64, crate::BorderAlign, Arc<Path>, u64);
pub(super) type PlacedDraws = (Arc<[crate::Draw]>, Vec<Arc<Path>>, u64);

#[derive(Debug)]
pub(super) struct Entry {
    /// Local to the key's origin.
    contour: Contour,
    seen: u64,
}

impl OutlineCache {
    fn get(&mut self, key: &[u64], origin: Point) -> Option<Contour> {
        let Some(e) = self.entries.get_mut(key) else {
            self.misses += 1;
            return None;
        };
        e.seen = self.generation;
        self.hits += 1;
        Some(Contour {
            offset: origin,
            ..e.contour.clone()
        })
    }

    fn insert(
        &mut self,
        key: Vec<u64>,
        origin: Point,
        contour: Contour,
    ) -> Result<Contour, SceneError> {
        let mut contour = contour.placed_at(origin);
        // Flattened once here rather than by every frame that hits.
        contour.known_bounds = contour.bounds()?;
        let seen = self.generation;
        self.entries.insert(
            key,
            Entry {
                contour: contour.clone(),
                seen,
            },
        );
        Ok(contour)
    }

    /// Drop what this resolve did not use: memory follows the live tree.
    pub(super) fn sweep(&mut self, age: u64) {
        let generation = self.generation;
        let live = |seen: u64| generation.wrapping_sub(seen) <= age;
        self.entries.retain(|_, e| live(e.seen));
        self.canvases.retain(|_, c| live(c.2));
        self.rects.retain(|_, r| live(r.1));
        self.bands.retain(|_, b| live(b.4));
        self.generation = generation.wrapping_add(1);
    }
}

/// `path` as key words, every coordinate by its bits.
fn path_words(path: &Path, key: &mut Vec<u64>) {
    use mui_geometry::PathCommand as C;
    key.push(path.commands.len() as u64);
    for c in &path.commands {
        match *c {
            C::MoveTo(p) => key.extend([0, p.x.to_bits(), p.y.to_bits()]),
            C::LineTo(p) => key.extend([1, p.x.to_bits(), p.y.to_bits()]),
            C::ArcTo(a) => key.extend(
                [
                    2.,
                    a.center.x,
                    a.center.y,
                    a.radius,
                    a.start_angle,
                    a.sweep,
                    a.to.x,
                    a.to.y,
                ]
                .map(f64::to_bits),
            ),
            C::CubicTo(a, b, p) => key.extend([3., a.x, a.y, b.x, b.y, p.x, p.y].map(f64::to_bits)),
            C::Close => key.push(4),
        }
    }
}

/// `n`'s own outline inputs as key words: its snapped bounds relative to
/// `origin`, which sits on the device grid, and its size. A custom
/// `.outline(..)` is keyed by the local path it draws and where that lands,
/// so a tree rebuilt every frame with a fresh closure still hits.
fn geometry_shallow(
    n: &El,
    frames: &[Frame],
    at: usize,
    g: (Point, Option<f64>),
    key: &mut Vec<u64>,
) {
    let (origin, scale) = g;
    let frame = frames[at];
    if let Some(outline) = &n.payload().extras().outline {
        key.extend([
            6,
            (frame.x - origin.x).to_bits(),
            (frame.y - origin.y).to_bits(),
        ]);
        path_words(&(outline.0)(frame.size), key);
    }
    let b = bounds(frame, scale);
    key.extend(
        [
            b.min.x - origin.x,
            b.min.y - origin.y,
            b.max.x - origin.x,
            b.max.y - origin.y,
            frame.size.width,
            frame.size.height,
        ]
        .map(f64::to_bits),
    );
    let style = &n.payload().style;
    match style.radius {
        Radius::Theme => key.push(0),
        Radius::Px(value) => key.extend([1, value.to_bits()]),
        Radius::Token(corner) => key.extend([2, corner as u64]),
        Radius::Scale(value) => key.extend([3, value.to_bits()]),
        Radius::Pill => key.push(4),
        Radius::Pair(convex, concave) => key.extend([5, convex.to_bits(), concave.to_bits()]),
    }
    key.push(match style.corners {
        CornerStyle::Round => 0,
        CornerStyle::Squircle => 1,
    });
    key.push(u64::from(style.union));
    key.push(match n.payload().carve {
        None => 0,
        Some(Carve::Cut) => 1,
        Some(Carve::Keep) => 2,
    });
    key.push(n.children().len() as u64);
}

fn geometry_node(
    n: &El,
    frames: &[Frame],
    sizes: &[usize],
    at: usize,
    g: (Point, Option<f64>),
    key: &mut Vec<u64>,
) {
    geometry_shallow(n, frames, at, g, key);
    let mut child_at = at + 1;
    for child in n.children() {
        // A plain child contributes only its own rounded frame to a weld.
        // Descendants matter when this child welds them or carves one out;
        // skipping unrelated descendants keeps the cache key cheaper than
        // the boolean work it avoids.
        let complex = child.payload().style.union
            || child
                .children()
                .iter()
                .any(|grandchild| grandchild.payload().carve.is_some());
        if complex {
            geometry_node(child, frames, sizes, child_at, g, key);
        } else {
            geometry_shallow(child, frames, child_at, g, key);
        }
        child_at += sizes[child_at];
    }
}

impl Walk<'_> {
    /// The node's own shape, with every [`Carve`] child taken out of it (or
    /// intersected with it). A carved outline is a path like a welded one:
    /// no analytic rect, so shells, strokes and clips all follow the result.
    ///
    /// `first` is the pre-order index of the node's first child. Weld and
    /// carve both inspect descendants while the walk is still at the parent;
    /// the explicit index gives them the child's own radius, corner style
    /// and nested topology instead of a sharp frame rectangle.
    pub(super) fn outline(
        &mut self,
        n: &El,
        frame: Frame,
        first: usize,
    ) -> Result<Contour, SceneError> {
        if let Some(path) = self.regions.get(&first.saturating_sub(1)) {
            return Ok(Contour::path(path.clone()));
        }
        if n.payload().extras().outline.is_some()
            && n.children().iter().any(|c| c.payload().carve.is_some())
        {
            return Err(mui_geometry::Error::InvalidOptions(
                "cut/keep on a custom outline requires contour normalization; provide the finished outline instead",
            )
            .into());
        }
        let cacheable = n.payload().style.union
            || n.children()
                .iter()
                .any(|child| child.payload().carve.is_some());
        let mut key = None;
        if cacheable {
            let mut words = std::mem::take(&mut self.outlines.scratch);
            let origin = self.geometry_key(n, first, &mut words);
            if let Some(outline) = self.outlines.get(&words, origin) {
                self.outlines.scratch = words;
                return Ok(outline);
            }
            key = Some((words, origin));
        }
        let base = self.shape(n, frame, first)?;
        let (mut at, mut topo): (usize, Option<Topology>) = (first, None);
        let mut shapes = Vec::new();
        for c in n.children() {
            let (f, carve) = (self.frames[at], c.payload().carve);
            let child_first = at + 1;
            at += self.sizes[at];
            let Some(carve) = carve.filter(|_| f.size.width > 0.0 && f.size.height > 0.0) else {
                continue;
            };
            if topo.is_none() {
                shapes = polygons(&base.world())?;
            }
            let rhs = polygons(&self.outline(c, f, child_first)?.world())?;
            if rhs.is_empty() {
                continue;
            }
            let op = match carve {
                Carve::Cut => BooleanOp::Difference,
                Carve::Keep => BooleanOp::Intersection,
            };
            let t = boolean(&shapes, &rhs, op, self.spec.geometry)?;
            shapes = t.placed_shapes();
            topo = Some(t);
        }
        let Some(topo) = topo else {
            return match key {
                Some((key, origin)) => self.outlines.insert(key, origin, base),
                None => Ok(base),
            };
        };
        // Radius 0: the shapes going in already carry their own rounding,
        // and a second fillet would eat the corners the carve just made.
        let rounded = fillet(
            &topo,
            Fillet {
                convex_radius: 0.,
                concave_radius: 0.,
                ..Fillet::default()
            },
        )?;
        let outline = Contour {
            changed: true,
            ..Contour::path(n.payload().style.corners.shape(&rounded.path))
        };
        match key {
            Some((key, origin)) => self.outlines.insert(key, origin, outline),
            None => Ok(outline),
        }
    }

    /// Every input `n`'s outline depends on, as words compared in full,
    /// and the origin its frames are relative to: the node's own, floored to
    /// the device grid when there is one so snapping moves with it.
    fn geometry_key(&mut self, n: &El, first: usize, key: &mut Vec<u64>) -> Point {
        use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
        key.clear();
        // Identity is the walk's key (the id, or the tree path) and the
        // node's own, not its pre-order index: a tooltip or menu wrapping
        // the root shifts every index but no key or geometry.
        key.push(BuildHasherDefault::<DefaultHasher>::default().hash_one((&*self.key, n.key())));
        for value in [
            self.spec.theme.corners.selector,
            self.spec.theme.corners.field,
            self.spec.theme.corners.box_,
            self.spec.theme.corners.concave,
            self.spec.geometry.epsilon,
            self.spec.geometry.coordinate_limit,
        ] {
            key.push(value.to_bits());
        }
        key.push(self.spec.geometry.max_vertices as u64);
        match self.spec.device_scale {
            None => key.push(0),
            Some(scale) => key.extend([1, scale.to_bits()]),
        }
        let at = first.saturating_sub(1);
        let f = self.frames[at];
        let scale = self.spec.device_scale;
        let origin = match scale {
            None => Point::new(f.x, f.y),
            Some(s) => Point::new((f.x * s).floor() / s, (f.y * s).floor() / s),
        };
        geometry_node(n, &self.frames, &self.sizes, at, (origin, scale), key);
        origin
    }

    /// `rr`'s path in `corners`, the one `Arc` every node of that shape
    /// shares, this frame and the next.
    pub(super) fn rect_path(&mut self, rr: RoundedRect, corners: CornerStyle) -> Arc<Path> {
        let b = rr.bounds();
        let key = [
            b.min.x.to_bits(),
            b.min.y.to_bits(),
            b.max.x.to_bits(),
            b.max.y.to_bits(),
            rr.radius().to_bits(),
            corners as u64,
        ];
        let generation = self.outlines.generation;
        let (p, seen) = self.outlines.rects.entry(key).or_insert_with(|| {
            let p = match corners {
                CornerStyle::Round => rr.path(),
                _ => corners.shape(&rr.path()),
            };
            (Arc::new(p), generation)
        });
        *seen = generation;
        p.clone()
    }

    fn shape(&mut self, n: &El, frame: Frame, first: usize) -> Result<Contour, SceneError> {
        let th = &self.spec.theme;
        let s = &n.payload().style;
        if let Some(shape) = &n.payload().extras().outline {
            if !s.shadow.is_empty() {
                return Err(mui_geometry::Error::InvalidOptions(
                    "custom-path shadows require a path-filter renderer; use an outer wrapper",
                )
                .into());
            }
            let path = (shape.0)(frame.size);
            path.validate(250_000)?;
            return Ok(Contour {
                offset: Point::new(frame.x, frame.y),
                ..Contour::path(path)
            });
        }
        let (convex, concave) = match s.radius {
            Radius::Theme => (th.corners.box_, th.corners.concave),
            // A pixel radius names the outer (convex) corner. The inner
            // (concave) corner remains the theme contract; a pair such as
            // `(20., 14.)` is two radii, never an elliptical radius.
            Radius::Px(r) => (r, th.corners.concave),
            Radius::Pair(convex, concave) => (convex, concave),
            Radius::Token(c) => (th.corners.get(c), th.corners.concave),
            Radius::Scale(k) => {
                let p = th.corners.scaled(k).ok_or(SceneError::InvalidRadius)?;
                (p.box_, p.concave)
            }
            Radius::Pill => (
                frame.size.width.min(frame.size.height) / 2.0,
                th.corners.concave,
            ),
        };
        if !(convex.is_finite() && convex >= 0.0 && concave.is_finite() && concave >= 0.0) {
            return Err(SceneError::InvalidRadius);
        }
        if !s.union || n.children().is_empty() {
            // Local to its snapped corner, so a move keeps the path.
            let b = bounds(frame, self.spec.device_scale);
            let size = Bounds::new(0., 0., b.max.x - b.min.x, b.max.y - b.min.y);
            let rr = RoundedRect::new(size, convex)?;
            let path = self.rect_path(rr, s.corners);
            let offset = b.min;
            // A squircle is no longer a rounded rectangle, so it gives up the
            // analytic blur and the analytic shell inset with it; the path
            // route below draws both from the outline itself.
            if s.corners != CornerStyle::Round {
                return Ok(Contour {
                    shadow_rects: vec![rr],
                    offset,
                    ..Contour::path(path)
                });
            }
            return Ok(Contour {
                rect: Some(rr),
                offset,
                ..Contour::path(path)
            });
        }
        // Children's outlines sit right after this node in pre-order, each
        // subtree `count` long. Carved children shape this node separately;
        // zero-area children have no paint or geometry and must not turn a
        // valid weld into a DegenerateRing error.
        let (mut at, mut shapes, mut rects) = (first, Vec::new(), Vec::new());
        let mut participants = 0;
        for c in n.children() {
            let child_at = at;
            let child_first = child_at + 1;
            let f = self.frames[child_at];
            at += self.sizes[at];
            if c.payload().carve.is_some() || f.size.width <= 0.0 || f.size.height <= 0.0 {
                continue;
            }
            // The weld is made in scene space, and the cache localises it.
            let child = self.outline(c, f, child_first)?;
            let child_rect = child.world_rect();
            let child_rects = child
                .shadow_rects
                .iter()
                .map(|r| r.translated(child.offset));
            // A rounded rect is one simple ring already: no normalizing pass.
            let child_shapes = match child_rect {
                Some(r) => {
                    let ring = r.path().flatten(0.25, 100_000)?.swap_remove(0);
                    vec![Polygon::new(ring).into()]
                }
                None => polygons(&child.world())?,
            };
            if child_shapes.is_empty() {
                continue;
            }
            shapes.extend(child_shapes);
            // Keep the child's analytic radius for the optional shadow fast
            // path. A nested weld lends its own participants' rects (its
            // frame can be far larger than its outline); a squircle has no
            // analytic rect and falls back to the frame with the weld's
            // convex radius.
            match child_rect {
                Some(r) => rects.push(r),
                None if c.payload().style.union && !child.shadow_rects.is_empty() => {
                    rects.extend(child_rects);
                }
                None => rects.push(RoundedRect::new(bounds(f, self.spec.device_scale), convex)?),
            }
            participants += 1;
        }
        if shapes.is_empty() {
            return Ok(Contour {
                shadow_rects: rects,
                ..Contour::path(Path::default())
            });
        }
        let merged = union(&shapes, self.spec.geometry)?;
        let rounded = fillet(
            &merged,
            Fillet {
                convex_radius: convex,
                concave_radius: concave,
                ..Fillet::default()
            },
        )?;
        Ok(Contour {
            changed: merged.components() != participants,
            shadow_rects: rects,
            ..Contour::path(s.corners.shape(&rounded.path))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::prelude::*;
    use crate::{Corners, Paint};

    /// A hole is a hole: the carved outline loses the child's area, the
    /// child never paints, and `keep` is the same machinery inverted.
    #[test]
    fn a_cut_child_leaves_a_hole_and_paints_nothing() {
        // Signed, so a hole subtracts: the rings come back wound apart.
        let area = |el: El| {
            let s = resolve_scene(&SceneSpec::new(stack![el.id("card")])).unwrap();
            let rings = s
                .surface("card")
                .unwrap()
                .path
                .flatten(0.1, 100_000)
                .unwrap();
            let signed: f64 = rings
                .iter()
                .map(|r| {
                    r.iter()
                        .zip(r.iter().cycle().skip(1))
                        .map(|(a, b)| a.x * b.y - b.x * a.y)
                        .sum::<f64>()
                        / 2.0
                })
                .sum();
            (signed.abs(), s.paint.len())
        };
        let square = |w: f64, h: f64| leaf(w, h).radius(Radius::Px(0.)).center();
        let plain = stack![]
            .square(100.)
            .radius(Radius::Px(0.))
            .fill(Role::Primary);
        let (whole, layers) = area(plain.clone());
        let (holed, carved) = area(plain.clone().cut(square(50., 50.)));
        let (kept, _) = area(plain.keep(square(50., 50.)));
        assert!((whole - 10_000.).abs() < 1.0, "{whole}");
        assert!((holed - 7_500.).abs() < 1.0, "{holed}");
        assert!((kept - 2_500.).abs() < 1.0, "{kept}");
        // The carve child added no paint of its own.
        assert_eq!(layers, carved);
    }

    #[test]
    fn welded_children_keep_their_own_outlines() {
        // With a square parent radius, the only way for the first contour to
        // miss the origin is for the child's rounded outline to participate in
        // the weld. The old frame-only union produced a sharp (0, 0) corner.
        let root = row([leaf(20., 20.).radius(8.)])
            .radius(0.)
            .union(Role::Surface)
            .id("weld");
        let s = resolve_scene(&SceneSpec::new(root).offered(Size::new(20., 20.))).unwrap();
        let points = s
            .surface("weld")
            .unwrap()
            .path
            .flatten(0.1, 20_000)
            .unwrap()
            .concat();
        assert!(
            !points.iter().any(|p| p.x.abs() < 1e-8 && p.y.abs() < 1e-8),
            "weld regressed to the child's sharp frame: {points:?}"
        );
    }

    #[test]
    fn weld_cache_reuses_only_matching_geometry_inputs() {
        let base = SceneSpec::new(
            row([leaf(20., 20.).radius(6.), leaf(18., 24.).radius(8.)])
                .radius(0.)
                .union(Role::Surface),
        );
        let mut text = TextCache::default();
        resolve_scene_with(&base, &mut text).unwrap();
        let first_misses = text.outlines.misses;
        assert!(first_misses > 0, "the welded outline was not cached");

        resolve_scene_with(&base, &mut text).unwrap();
        assert_eq!(text.outlines.misses, first_misses);
        assert!(text.outlines.hits > 0, "the unchanged weld was not reused");

        let mut changed = base.clone();
        changed.root = changed.root.radius(3.);
        let misses = text.outlines.misses;
        resolve_scene_with(&changed, &mut text).unwrap();
        assert!(
            text.outlines.misses > misses,
            "a style change reused stale geometry"
        );

        changed.theme.corners.box_ += 1.;
        let misses = text.outlines.misses;
        resolve_scene_with(&changed, &mut text).unwrap();
        assert!(
            text.outlines.misses > misses,
            "a theme change reused stale geometry"
        );

        changed.device_scale = Some(2.);
        let misses = text.outlines.misses;
        resolve_scene_with(&changed, &mut text).unwrap();
        assert!(
            text.outlines.misses > misses,
            "a scale change reused stale geometry"
        );
    }

    /// A cached weld is only reused when every input matches, so the
    /// cached outline always equals a fresh resolve of the same spec.
    fn same_as_fresh(spec: &SceneSpec, text: &mut TextCache) {
        let cached = resolve_scene_with(spec, text).unwrap();
        let fresh = resolve_scene(spec).unwrap();
        assert_eq!(
            cached.surface("weld").unwrap().path,
            fresh.surface("weld").unwrap().path,
            "a stale welded outline was reused"
        );
    }

    #[test]
    fn a_weld_reshapes_when_a_childs_custom_outline_changes() {
        let spec = |w: f64| {
            let child = leaf(40., 20.).outline(move |s| {
                let p = [(0., 0.), (s.width * w, 0.), (0., s.height)];
                Path::polyline(p.map(|(x, y)| Point::new(x, y)), true)
            });
            SceneSpec::new(row([child, leaf(20., 20.)]).union(Role::Surface).id("weld"))
                .offered(Size::new(60., 20.))
        };
        let mut text = TextCache::default();
        resolve_scene_with(&spec(1.), &mut text).unwrap();
        // Same frames, same radii: only the closure differs.
        same_as_fresh(&spec(0.5), &mut text);
    }

    /// A weld with a custom outline in it is cached by the path the
    /// closure draws: the same drawing hits, even from a rebuilt closure,
    /// and a different one reshapes.
    #[test]
    fn a_custom_outline_weld_is_cached_while_its_drawing_stays() {
        fn send<T: Send>() {}
        send::<TextCache>();
        let triangle = |w: f64| {
            move |s: Size| {
                let p = [(0., 0.), (s.width * w, 0.), (0., s.height)];
                Path::polyline(p.map(|(x, y)| Point::new(x, y)), true)
            }
        };
        let spec = |child: El| {
            SceneSpec::new(row([child, leaf(20., 20.)]).union(Role::Surface).id("weld"))
                .offered(Size::new(60., 20.))
        };
        let kept = spec(leaf(40., 20.).outline(triangle(1.)));
        let mut text = TextCache::default();
        resolve_scene_with(&kept, &mut text).unwrap();
        let misses = text.outlines.misses;
        resolve_scene_with(&kept, &mut text).unwrap();
        let rebuilt = spec(leaf(40., 20.).outline(triangle(1.)));
        resolve_scene_with(&rebuilt, &mut text).unwrap();
        assert_eq!(
            text.outlines.misses, misses,
            "an unchanged drawing reshaped the weld"
        );
        let swapped = spec(leaf(40., 20.).outline(triangle(0.5)));
        same_as_fresh(&swapped, &mut text);
        assert!(text.outlines.misses > misses, "a new drawing hit the cache");
    }

    #[test]
    fn a_token_radius_and_a_pill_never_share_a_cached_weld() {
        // The old 64-bit key hashed `Token(Box)` and `Pill` to the same word.
        let spec = |r: Radius| {
            SceneSpec::new(
                row([leaf(40., 20.), leaf(20., 20.)])
                    .radius(r)
                    .union(Role::Surface)
                    .id("weld"),
            )
            .offered(Size::new(60., 20.))
            .theme(Theme {
                corners: Corners {
                    box_: 2.,
                    ..Corners::DEFAULT
                },
                ..Theme::default()
            })
        };
        let mut text = TextCache::default();
        resolve_scene_with(&spec(Radius::Token(Corner::Box)), &mut text).unwrap();
        same_as_fresh(&spec(Radius::Pill), &mut text);
    }

    /// Geometry is cached in local space: a steady frame runs no Boolean
    /// pass, and neither does one that slides every panel over, and the
    /// moved outline is the fresh one moved.
    #[test]
    fn a_steady_or_moved_frame_runs_no_boolean_pass() {
        let spec = |shift: f64| {
            let tab = column([leaf(20., 20.).pill()])
                .pad(8.)
                .shell(4., Role::Raised);
            let body = row([leaf(40., 24.).stroke(Role::Dim), leaf(40., 24.)])
                .inside(4.)
                .stroke(Role::Dim)
                .cut(leaf(8., 8.).center());
            let weld = column([tab, body])
                .align(Align::Start)
                .union(Role::Surface)
                .stroke(Role::Dim)
                .shell(3., Role::Raised)
                .id("weld");
            SceneSpec::new(column([weld]).pad(Spacing::Px(8. + shift)))
                .offered(Size::new(400. + 2. * shift, 300. + 2. * shift))
        };
        let mut text = TextCache::default();
        resolve_scene_with(&spec(0.), &mut text).unwrap();
        for shift in [0., 13., 13.25, 0.5] {
            let before = mui_geometry::boolean_passes();
            let cached = resolve_scene_with(&spec(shift), &mut text).unwrap();
            assert_eq!(mui_geometry::boolean_passes(), before, "shift {shift}");
            let fresh = resolve_scene(&spec(shift)).unwrap();
            let flat = |s: &ResolvedScene| s.surface("weld").unwrap().path.flatten(0.1, 20_000);
            for (a, b) in flat(&cached)
                .unwrap()
                .concat()
                .iter()
                .zip(flat(&fresh).unwrap().concat())
            {
                assert!(a.distance(b) < 1e-6, "shift {shift}: {a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn weld_ignores_zero_area_children() {
        let root = row([leaf(0., 20.), leaf(20., 20.)])
            .union(Role::Surface)
            .id("weld");
        let s = resolve_scene(&SceneSpec::new(root).offered(Size::new(20., 20.))).unwrap();
        assert!(!s.surface("weld").unwrap().path.commands.is_empty());
    }

    #[test]
    fn tokens_pill_and_gradient() {
        let root = row([leaf(40., 20.)
            .pill()
            .fill(Gradient::vertical(Role::Raised, Role::Surface))
            .id("k")])
        .gap(M)
        .pad(S);
        let s = resolve_scene(&SceneSpec::new(root)).unwrap();
        assert_eq!(s.layout.frame("k").unwrap().x, 8.);
        assert_eq!(s.surface("k").unwrap().rect.unwrap().radius(), 10.);
        assert!(matches!(
            s.paint[0].paint,
            Paint::Gradient { kind: crate::GradientKind::Linear { angle }, .. } if angle == 180.
        ));
    }

    #[test]
    /// A square port tab next to a square body welds into one contour; a
    /// rounded tab is a pill that only kisses the body and the union splits.
    #[allow(clippy::float_cmp)]
    fn a_square_tab_welds_into_one_contour_with_its_body() {
        let tab = column([leaf(24., 24.)])
            .w(36.)
            .h(36.)
            .pad(6.)
            .gap(0.)
            .shrink(0.)
            .fill(Role::Surface)
            .radius(0.)
            .align(Align::End);
        let body = leaf(400., 200.)
            .grow(1.)
            .shrink(1.)
            .min_width(0.)
            .radius(0.)
            .id("body");
        let root = row([tab, body])
            .align(Align::Start)
            .gap(Spacing::Px(0.0))
            .w(pct(100.))
            .union(Role::Surface)
            .stroke(Role::Dim)
            .stroke_width(1.5)
            .radius(20.)
            .id("weld");
        let s = resolve_scene(&SceneSpec::new(root).offered(Size::new(500., 200.))).unwrap();
        let pts = s
            .surface("weld")
            .unwrap()
            .path
            .flatten(0.1, 20_000)
            .unwrap()
            .concat();
        let minx = pts.iter().map(|p| p.x).fold(f64::MAX, f64::min);
        // a point on the seam x=36 at y=100: inside the union?
        let contours = s
            .surface("weld")
            .unwrap()
            .path
            .flatten(0.1, 20_000)
            .unwrap();
        assert!(minx < 1.0, "tab dropped from the weld: minx {minx}");
        assert_eq!(contours.len(), 1);
    }
}
