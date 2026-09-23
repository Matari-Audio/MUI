//! A node's outline: its own rounded frame, a weld of its children, or
//! either with carved children taken out -- and the cache for the costly ones.
use std::collections::HashMap;

use mui_geometry::{
    boolean, fillet, union, BooleanOp, CornerStyle, Fillet, Path, Point, RoundedRect, Topology,
};
use mui_layout::Frame;

use super::{bounds, polygons, SceneError, Walk};
use crate::{Carve, El, Radius};

/// A node's resolved outline, and what else its shape knows.
#[derive(Clone, Debug)]
pub(super) struct Contour {
    pub(super) path: Path,
    /// Exact rounded rectangle when the outline is one (not welded).
    pub(super) rect: Option<RoundedRect>,
    /// A shell collapsed or a merge changed ring counts.
    pub(super) changed: bool,
    /// Rounded rects that stand in for an outline with no `rect` when it
    /// casts a shadow: one per welded child, or a squircle's own frame.
    pub(super) shadow_rects: Vec<RoundedRect>,
}
impl Contour {
    /// A plain path: no analytic form, nothing changed.
    pub(super) fn path(path: Path) -> Self {
        Self {
            path,
            rect: None,
            changed: false,
            shadow_rects: Vec::new(),
        }
    }
}

/// Weld and carve outlines, the walk's most expensive geometry, keyed by
/// every input word that shaped them and compared in full: a hash match
/// alone is never trusted. Entries no resolve used are swept at its end.
#[derive(Debug, Default)]
pub(super) struct OutlineCache {
    pub(super) entries: HashMap<Vec<u64>, (Contour, u64)>,
    /// A key buffer, handed back after a hit so a warm frame builds keys
    /// without allocating.
    pub(super) scratch: Vec<u64>,
    pub(super) generation: u64,
    pub(super) hits: u64,
    pub(super) misses: u64,
}

impl OutlineCache {
    fn get(&mut self, key: &[u64]) -> Option<Contour> {
        let Some((outline, seen)) = self.entries.get_mut(key) else {
            self.misses += 1;
            return None;
        };
        *seen = self.generation;
        self.hits += 1;
        Some(outline.clone())
    }

    fn insert(&mut self, key: Vec<u64>, outline: Contour) {
        self.entries.insert(key, (outline, self.generation));
    }

    /// Drop what this resolve did not use: memory follows the live tree.
    pub(super) fn sweep(&mut self) {
        let generation = self.generation;
        self.entries.retain(|_, (_, seen)| *seen == generation);
        self.generation = generation.wrapping_add(1);
    }
}

/// `n`'s own outline inputs as key words. `false` for a custom
/// `.outline(..)`: nothing here can see what its closure returns, so an
/// outline that includes one is never cached.
///
/// ponytail: such a weld redoes its boolean every frame. Keying on the
/// closure's `Arc` identity would need the cache to hold the `Arc`, which is
/// not `Send` and would take `Send` away from `TextCache`; and a tree rebuilt
/// per frame makes a new closure per frame anyway, so it would never hit.
fn geometry_shallow(n: &El, frames: &[Frame], at: usize, key: &mut Vec<u64>) -> bool {
    if n.payload().outline.is_some() {
        return false;
    }
    let frame = frames[at];
    key.extend([frame.x, frame.y, frame.size.width, frame.size.height].map(f64::to_bits));
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
    true
}

fn geometry_node(n: &El, frames: &[Frame], sizes: &[usize], at: usize, key: &mut Vec<u64>) -> bool {
    if !geometry_shallow(n, frames, at, key) {
        return false;
    }
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
        let keyed = if complex {
            geometry_node(child, frames, sizes, child_at, key)
        } else {
            geometry_shallow(child, frames, child_at, key)
        };
        if !keyed {
            return false;
        }
        child_at += sizes[child_at];
    }
    true
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
        if n.payload().outline.is_some() && n.children().iter().any(|c| c.payload().carve.is_some())
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
            if self.geometry_key(n, first, &mut words) {
                if let Some(outline) = self.outlines.get(&words) {
                    self.outlines.scratch = words;
                    return Ok(outline);
                }
                key = Some(words);
            } else {
                self.outlines.scratch = words;
            }
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
                shapes = polygons(&base.path)?;
            }
            let rhs = polygons(&self.outline(c, f, child_first)?.path)?;
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
            if let Some(key) = key {
                self.outlines.insert(key, base.clone());
            }
            return Ok(base);
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
        if let Some(key) = key {
            self.outlines.insert(key, outline.clone());
        }
        Ok(outline)
    }

    /// Every input `n`'s outline depends on, as words compared in full.
    /// `false` when a custom outline is involved; see [`geometry_shallow`].
    fn geometry_key(&self, n: &El, first: usize, key: &mut Vec<u64>) -> bool {
        key.clear();
        key.push(first as u64);
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
        geometry_node(n, &self.frames, &self.sizes, first.saturating_sub(1), key)
    }

    fn shape(&mut self, n: &El, frame: Frame, first: usize) -> Result<Contour, SceneError> {
        let th = &self.spec.theme;
        let s = &n.payload().style;
        if let Some(shape) = &n.payload().outline {
            if !s.shadow.is_empty() {
                return Err(mui_geometry::Error::InvalidOptions(
                    "custom-path shadows require a path-filter renderer; use an outer wrapper",
                )
                .into());
            }
            let local = (shape.0)(frame.size);
            local.validate(250_000)?;
            let world = local.rigid_transform(Point::new(frame.x, frame.y), 0.0)?;
            world.validate(250_000)?;
            return Ok(Contour::path(world));
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
            let rr = RoundedRect::new(bounds(frame, self.spec.device_scale), convex)?;
            // A squircle is no longer a rounded rectangle, so it gives up the
            // analytic blur and the analytic shell inset with it; the path
            // route below draws both from the outline itself.
            if s.corners != CornerStyle::Round {
                return Ok(Contour {
                    shadow_rects: vec![rr],
                    ..Contour::path(s.corners.shape(&rr.path()))
                });
            }
            return Ok(Contour {
                rect: Some(rr),
                ..Contour::path(rr.path())
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
            let Contour {
                path,
                rect: child_rect,
                ..
            } = self.outline(c, f, child_first)?;
            let child_shapes = polygons(&path)?;
            if child_shapes.is_empty() {
                continue;
            }
            shapes.extend(child_shapes);
            // Keep the child's analytic radius for the optional shadow fast
            // path. A squircle or nested weld has no analytic rect and falls
            // back to the frame with the weld's convex radius.
            rects.push(match child_rect {
                Some(r) => r,
                None => RoundedRect::new(bounds(f, self.spec.device_scale), convex)?,
            });
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
            path: s.corners.shape(&rounded.path),
            rect: None,
            changed: merged.components() != participants,
            shadow_rects: rects,
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
