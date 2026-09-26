//! Container-owned materials: authored footprints, derived corners and clearance.
//! Keeps layout/controls separate from a material that can wrap around their holes.
use crate::regions::{Operation, RAMP_BAND, RegionCache, STROKE_BAND};
use crate::{El, Frame, Id, Radius, SceneError, SceneSpec};
use mui_geometry::{
    BooleanOp, CornerStyle, Fillet, GeometryOptions, OffsetOptions, Path, PlacedShape, Point,
    Polygon, boolean_paths, fillet, inset_path, union, union_contours,
};
use rustc_hash::FxHashMap as HashMap;
use std::sync::Arc;

#[derive(Clone, Debug)]
struct Inputs {
    outline: Path,
    border: Path,
    panels: Vec<(usize, Vec<Frame>)>,
    joins: Vec<(usize, Frame, Frame, f64)>,
    padding: f64,
    border_inset: f64,
    rounding: Fillet,
    corners: CornerStyle,
    offsets: OffsetOptions,
    geometry: GeometryOptions,
    device_scale: Option<f64>,
}
impl Inputs {
    /// Equal, but for the last bits a translation rounds off the paths.
    fn near(&self, o: &Self) -> bool {
        self.outline.near(&o.outline, 1e-9)
            && self.border.near(&o.border, 1e-9)
            && self.panels == o.panels
            && self.joins == o.joins
            && self.padding == o.padding
            && self.border_inset == o.border_inset
            && self.rounding == o.rounding
            && self.corners == o.corners
            && self.offsets == o.offsets
            && self.geometry == o.geometry
            && self.device_scale == o.device_scale
    }
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Geometry {
    /// Local to `origin`, and the same `Arc`s while the inputs hold.
    pub panels: Vec<(usize, Arc<Path>)>,
    pub origin: Point,
    pub joins: Path,
    pub join_nodes: Vec<usize>,
}
/// Keyed by node identity, so a tooltip or menu reshaping the tree around an
/// owner reuses its geometry. Every boolean pass is miss-only: the border an
/// owner compares against comes from the identity-keyed region cache.
#[derive(Debug, Default)]
pub(crate) struct Cache {
    entries: HashMap<crate::Id, (Inputs, Geometry)>,
    borders: crate::border_ramp::BorderCache,
}

fn count(n: &El) -> usize {
    1 + n.children().iter().map(count).sum::<usize>()
}
/// The owner's scope in pre-order; returns the subtree size so a walk is O(n).
fn collect<'a>(n: &'a El, at: usize, nodes: &mut Vec<(usize, &'a El)>) -> usize {
    nodes.push((at, n));
    let mut next = at + 1;
    for child in n.children() {
        next += if child.payload().extras().surface_padding.is_none() && !child.is_float() {
            collect(child, next, nodes)
        } else {
            count(child)
        };
    }
    next - at
}
impl Cache {
    pub fn resolve(
        &mut self,
        (root, padding): (&El, crate::Spacing),
        (key, at): (&crate::Id, usize),
        frames: &[Frame],
        outline: &Path,
        spec: &SceneSpec,
        regions: &mut RegionCache,
    ) -> Result<Geometry, SceneError> {
        let e = root.payload();
        let padding = padding.resolve(spec.theme.spacing);
        if !padding.is_finite() || padding < 0. {
            return Err(mui_geometry::Error::InvalidOptions("surface padding").into());
        }
        if e.extras().welding.is_some() {
            return Err(SceneError::UnsupportedWeld(
                "surface layout requires a vector contour",
            ));
        }
        let th = spec.theme.corners;
        let (convex, concave) = match e.style.radius.unwrap_or_default() {
            Radius::Theme => (th.box_, th.concave),
            Radius::Px(r) => (r, th.concave),
            Radius::Pair(a, b) => (a, b),
            Radius::Token(c) => (th.get(c), th.concave),
            Radius::Scale(k) => (th.box_ * k, th.concave * k),
            Radius::Pill => (
                frames[at].size.width.min(frames[at].size.height) / 2.,
                th.concave,
            ),
        };
        let mut nodes = Vec::new();
        collect(root, at, &mut nodes);
        // Reversed, so the first node with a key wins as a scan would find.
        let keyed: HashMap<&str, usize> = nodes
            .iter()
            .rev()
            .filter_map(|(i, n)| Some((n.key()?, *i)))
            .collect();
        let named = |id: &Id| -> Result<Frame, SceneError> {
            keyed
                .get(id.as_str())
                .map(|i| frames[*i])
                .ok_or_else(|| SceneError::MissingId {
                    what: "surface member",
                    id: id.clone(),
                })
        };
        let mut panels = Vec::new();
        let mut joins = Vec::new();
        for (i, node) in &nodes {
            let frame = frames[*i];
            if frame.size.width <= 0. || frame.size.height <= 0. {
                continue;
            }
            if let Some(members) = &node.payload().extras().inset_surface {
                let footprints = if members.is_empty() {
                    vec![frame]
                } else {
                    members.iter().map(&named).collect::<Result<Vec<_>, _>>()?
                };
                panels.push((*i - at, footprints));
            }
            if let Some(body) = &node.payload().extras().border_join {
                let body = named(body)?;
                let ramp =
                    e.extras()
                        .border_ramp
                        .as_ref()
                        .ok_or(mui_geometry::Error::InvalidOptions(
                            "border join requires an owning ramp",
                        ))?;
                if ramp.align != crate::BorderAlign::Inside {
                    return Err(mui_geometry::Error::InvalidOptions(
                        "border join requires an inside ramp",
                    )
                    .into());
                }
                let anchor = ramp
                    .anchor
                    .as_ref()
                    .map(&named)
                    .transpose()?
                    .unwrap_or(frames[at]);
                let left = frame.x + frame.size.width * 0.5 < body.x + body.size.width * 0.5;
                let edge = if left { body.x } else { body.right() };
                joins.push((*i - at, frame, body, ramp.width(edge, anchor)?));
            }
        }
        if panels.is_empty() && joins.is_empty() {
            return Ok(Geometry::default());
        }
        let mut border = Path::default();
        if let Some(ramp) = &e.extras().border_ramp {
            ramp.validate()?;
            let anchor = ramp
                .anchor
                .as_ref()
                .map(&named)
                .transpose()?
                .unwrap_or(frames[at]);
            let mut inward = ramp.clone();
            inward.from.1 *= ramp.align.inward();
            inward.to.1 *= ramp.align.inward();
            let mut sweep = self.borders.band(
                key,
                outline,
                &inward,
                anchor,
                spec.offsets.flatten_tolerance,
            )?;
            crate::border_ramp::decorate(&mut sweep, ramp, anchor, concave, named)?;
            border = regions.resolve(
                (key.clone(), RAMP_BAND),
                Operation::Sweep(sweep),
                OffsetOptions {
                    max_points: 100_000,
                    ..spec.offsets
                },
                spec.geometry,
            )?;
        } else if let Some(stroke) = &e.style.stroke {
            border = regions.resolve(
                (key.clone(), STROKE_BAND),
                Operation::Border(
                    outline.clone(),
                    stroke.width.unwrap_or(spec.theme.stroke_width),
                    e.border_align,
                ),
                spec.offsets,
                spec.geometry,
            )?;
        }
        // Local to the owner's frame, so a moved owner still hits.
        let origin = Point::new(frames[at].x, frames[at].y);
        let local = |f: &mut Frame| {
            f.x -= origin.x;
            f.y -= origin.y;
        };
        panels
            .iter_mut()
            .for_each(|(_, fs)| fs.iter_mut().for_each(local));
        for (_, tab, body, _) in &mut joins {
            local(tab);
            local(body);
        }
        let mut outline = outline.clone();
        outline.translate(-origin.to_vec2());
        border.translate(-origin.to_vec2());
        let input = Inputs {
            outline,
            border,
            panels,
            joins,
            padding,
            border_inset: e.extras().border_ramp.as_ref().map_or_else(
                || {
                    e.style.stroke.as_ref().map_or(0., |stroke| {
                        stroke.width.unwrap_or(spec.theme.stroke_width) * e.border_align.inward()
                    })
                },
                |ramp| ramp.from.1.max(ramp.to.1) * ramp.align.inward(),
            ),
            rounding: Fillet {
                convex_radius: convex,
                concave_radius: concave,
                ..Fillet::default()
            },
            corners: e.style.corners.unwrap_or_default(),
            offsets: spec.offsets,
            geometry: spec.geometry,
            device_scale: spec.device_scale,
        };
        // Node indices are stored relative to the owner, which a wrapper
        // around the root shifts as a whole, and paths relative to its frame.
        let placed = |mut g: Geometry| {
            for (i, _) in &mut g.panels {
                *i += at;
            }
            g.origin = origin;
            g.joins.translate(origin.to_vec2());
            g.join_nodes.iter_mut().for_each(|i| *i += at);
            g
        };
        if let Some((old, result)) = self.entries.get(key)
            && old.near(&input)
        {
            return Ok(placed(result.clone()));
        }
        let result = resolve(&input)?;
        // Bounded per-scene cache; colors do not affect geometry.
        if self.entries.len() >= 256 {
            self.entries.clear();
        }
        self.entries.insert(key.clone(), (input, result.clone()));
        Ok(placed(result))
    }
}

fn resolve(i: &Inputs) -> Result<Geometry, SceneError> {
    let combine = |a: &Path, b: &Path, op| boolean_paths(a, b, op, i.offsets, i.geometry);
    let round = |shapes: &[PlacedShape]| -> Result<Path, SceneError> {
        Ok(i.corners
            .shape(&fillet(&union(shapes, i.geometry)?, i.rounding)?.path))
    };
    let mut additions = Path::default();
    for (_, tab, body, width) in &i.joins {
        let left = tab.x + tab.size.width * 0.5 < body.x + body.size.width * 0.5;
        let edge = if left {
            body.x + width
        } else {
            body.right() - width
        };
        let reach = if left { tab.right() } else { tab.x };
        // The exterior half-plane avoids limiting roots by the thin border's
        // width. Only the final container contour clips this filleted junction.
        let margin = body.size.width.max(body.size.height)
            + i.rounding.convex_radius
            + i.rounding.concave_radius;
        let outside = if left {
            body.x - margin
        } else {
            body.right() + margin
        };
        let points = [
            (outside, body.y - margin),
            (edge, body.y - margin),
            (edge, tab.y),
            (reach, tab.y),
            (reach, tab.bottom()),
            (edge, tab.bottom()),
            (edge, body.bottom() + margin),
            (outside, body.bottom() + margin),
        ]
        .map(|(x, y)| Point::new(x, y));
        let shape = round(&[PlacedShape::from(Polygon::new(points.to_vec()))])?;
        // A shared owner may contain bodies with different side-tab widths.
        // The attachment belongs to this body, never to its siblings' spines.
        let body_path = Path::polyline(
            [
                Point::new(body.x, body.y),
                Point::new(body.right(), body.y),
                Point::new(body.right(), body.bottom()),
                Point::new(body.x, body.bottom()),
            ],
            true,
        );
        additions
            .commands
            .extend(combine(&shape, &body_path, BooleanOp::Intersection)?.commands);
    }
    let additions = union_contours(&additions, i.offsets, i.geometry)?;
    let joins = combine(&additions, &i.outline, BooleanOp::Intersection)?;
    let band = combine(&i.border, &joins, BooleanOp::Union)?;
    let interior = combine(&i.outline, &band, BooleanOp::Difference)?;
    let available = inset_path(&interior, i.padding, i.offsets)?.path;
    // A circular arc inset by `padding`: convex arcs shrink (collapsing to a
    // sharp corner), concave arcs grow.
    let arc = |radius: f64, grow: f64| -> Result<f64, SceneError> {
        if !radius.is_finite() || radius < 0. {
            return Err(SceneError::InvalidRadius);
        }
        Ok((radius + grow).max(0.))
    };
    let rounding = Fillet {
        convex_radius: arc(i.rounding.convex_radius, -i.padding)?,
        concave_radius: arc(i.rounding.concave_radius, i.padding)?,
        ..i.rounding
    };
    let mut panels = Vec::new();
    for (index, frames) in &i.panels {
        let mut shapes = frames
            .iter()
            .filter(|f| f.size.width > 0. && f.size.height > 0.)
            .map(|f| {
                Polygon::rectangle(f.x, f.y, f.size.width, f.size.height).map(PlacedShape::from)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if shapes.is_empty() {
            panels.push((*index, Arc::default()));
            continue;
        }
        // Border labels own their cutout. Reserve their footprint here so a
        // merged well follows that one boundary instead of inventing a second
        // independently rounded notch around it.
        let Some(bounds) = mui_geometry::bounds(
            frames
                .iter()
                .flat_map(|f| [Point::new(f.x, f.y), Point::new(f.right(), f.bottom())]),
        ) else {
            panels.push((*index, Arc::default()));
            continue;
        };
        for (_, tab, _, _) in &i.joins {
            if tab.x >= bounds.x0
                && tab.right() <= bounds.x1
                && tab.y >= bounds.y0
                && tab.bottom() <= bounds.y1
            {
                shapes.push(
                    Polygon::rectangle(
                        tab.x - i.padding,
                        tab.y - i.padding,
                        tab.size.width + i.padding * 2.,
                        tab.size.height + i.padding * 2.,
                    )?
                    .into(),
                );
            }
        }
        let mut footprint = union(&shapes, i.geometry)?.to_path();
        // A label identifies its content body even when the owning outline
        // also includes ports or footer tabs. Those attachments must not pull
        // an interior material beyond the body's reserved border clearance.
        if let Some((_, _, body, _)) = i.joins.iter().find(|(_, _, body, _)| {
            bounds.x0 >= body.x
                && bounds.x1 <= body.right()
                && bounds.y0 >= body.y
                && bounds.y1 <= body.bottom()
        }) {
            let body_path = Polygon::rectangle(body.x, body.y, body.size.width, body.size.height)?;
            let body_path = union(&[body_path.into()], i.geometry)?.to_path();
            let content = inset_path(&body_path, i.padding + i.border_inset, i.offsets)?.path;
            footprint = combine(&footprint, &content, BooleanOp::Intersection)?;
        }
        // Round authored partitions before applying inherited curved boundaries.
        let topology = mui_geometry::offset_path(&footprint, 0., i.offsets)?.topology;
        let rounded = fillet(&topology, rounding)?.path;
        panels.push((
            *index,
            Arc::new(combine(
                &i.corners.shape(&rounded),
                &available,
                BooleanOp::Intersection,
            )?),
        ));
    }
    Ok(Geometry {
        panels,
        origin: Point::ZERO,
        joins,
        join_nodes: i.joins.iter().map(|(index, ..)| *index).collect(),
    })
}
