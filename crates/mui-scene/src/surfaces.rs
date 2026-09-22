//! Container-owned materials: authored footprints, derived corners and clearance.
//! Keeps layout/controls separate from a material that can wrap around their holes.
use crate::{El, Frame, Id, Radius, SceneError, SceneSpec};
use mui_geometry::{
    boolean_paths, fillet, inset_path, union, union_contours, BooleanOp, CornerStyle, Fillet,
    GeometryOptions, OffsetOptions, Path, PlacedShape, Point, Polygon,
};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
struct Inputs {
    outline: Path,
    border: Path,
    panels: Vec<(usize, Vec<Frame>)>,
    joins: Vec<(usize, Frame, Frame, f64)>,
    padding: f64,
    rounding: Fillet,
    corners: CornerStyle,
    offsets: OffsetOptions,
    geometry: GeometryOptions,
    device_scale: Option<f64>,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Geometry {
    pub panels: Vec<(usize, Path)>,
    pub joins: Path,
    pub join_nodes: Vec<usize>,
}
#[derive(Debug, Default)]
pub(crate) struct Cache {
    entries: HashMap<usize, (Inputs, Geometry)>,
    borders: crate::border_ramp::BorderCache,
}

fn count(n: &El) -> usize {
    1 + n.children().iter().map(count).sum::<usize>()
}
fn collect<'a>(n: &'a El, at: usize, nodes: &mut Vec<(usize, &'a El)>) {
    nodes.push((at, n));
    let mut next = at + 1;
    for child in n.children() {
        if child.payload().surface_padding.is_none() && !child.is_float() {
            collect(child, next, nodes);
        }
        next += count(child);
    }
}
impl Cache {
    pub fn resolve(
        &mut self,
        root: &El,
        at: usize,
        frames: &[Frame],
        outline: &Path,
        spec: &SceneSpec,
    ) -> Result<Geometry, SceneError> {
        let e = root.payload();
        let padding = e.surface_padding.unwrap().resolve(spec.theme.spacing);
        if !padding.is_finite() || padding < 0. {
            return Err(mui_geometry::Error::InvalidOptions("surface padding").into());
        }
        if e.welding.is_some() {
            return Err(SceneError::UnsupportedWeld(
                "surface layout requires a vector contour",
            ));
        }
        let th = spec.theme.corners;
        let (convex, concave) = match e.style.radius {
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
        let named = |id: &Id| -> Result<Frame, SceneError> {
            nodes
                .iter()
                .find(|(_, n)| n.key() == Some(id.as_str()))
                .map(|(i, _)| frames[*i])
                .ok_or_else(|| {
                    mui_geometry::Error::InvalidOptions("surface footprint missing").into()
                })
        };
        let mut panels = Vec::new();
        let mut joins = Vec::new();
        for (i, node) in &nodes {
            let frame = frames[*i];
            if frame.size.width <= 0. || frame.size.height <= 0. {
                continue;
            }
            if let Some(members) = &node.payload().inset_surface {
                let footprints = if members.is_empty() {
                    vec![frame]
                } else {
                    members.iter().map(&named).collect::<Result<Vec<_>, _>>()?
                };
                panels.push((*i, footprints));
            }
            if let Some(body) = &node.payload().border_join {
                let body = named(body)?;
                let ramp = e
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
                joins.push((*i, frame, body, ramp.width(edge, anchor)?));
            }
        }
        if panels.is_empty() && joins.is_empty() {
            return Ok(Geometry::default());
        }
        let mut border = Path::default();
        if let Some(ramp) = &e.border_ramp {
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
            border = self.borders.band(
                &at.to_string(),
                outline,
                &inward,
                anchor,
                spec.offsets.flatten_tolerance,
            )?;
            crate::border_ramp::decorate(&mut border, ramp, anchor, concave, named)?;
            border = union_contours(
                &border,
                OffsetOptions {
                    max_points: 100_000,
                    ..spec.offsets
                },
                spec.geometry,
            )?;
        } else if let Some(stroke) = &e.style.stroke {
            border = mui_geometry::border_geometry(
                outline,
                mui_geometry::WidthProfile::uniform(
                    stroke.width.unwrap_or(spec.theme.stroke_width),
                ),
                e.border_align,
                spec.offsets,
                spec.geometry,
            )?
            .band;
        }
        let input = Inputs {
            outline: outline.clone(),
            border,
            panels,
            joins,
            padding,
            rounding: Fillet {
                convex_radius: convex,
                concave_radius: concave,
                ..Fillet::default()
            },
            corners: e.style.corners,
            offsets: spec.offsets,
            geometry: spec.geometry,
            device_scale: spec.device_scale,
        };
        if let Some((old, result)) = self.entries.get(&at) {
            if *old == input {
                return Ok(result.clone());
            }
        }
        let result = resolve(&input)?;
        // Bounded per-scene cache; colors do not affect geometry.
        if self.entries.len() >= 256 {
            self.entries.clear();
        }
        self.entries.insert(at, (input, result.clone()));
        Ok(result)
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
    let rounding = Fillet {
        convex_radius: mui_geometry::inset_arc_radius(i.rounding.convex_radius, i.padding, false)?
            .unwrap_or(0.),
        concave_radius: mui_geometry::inset_arc_radius(i.rounding.concave_radius, i.padding, true)?
            .unwrap_or(0.),
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
            panels.push((*index, Path::default()));
            continue;
        }
        // Border labels own their cutout. Reserve their footprint here so a
        // merged well follows that one boundary instead of inventing a second
        // independently rounded notch around it.
        let bounds = mui_geometry::Bounds::from_points(
            frames
                .iter()
                .flat_map(|f| [Point::new(f.x, f.y), Point::new(f.right(), f.bottom())]),
        )
        .unwrap();
        for (_, tab, _, _) in &i.joins {
            if tab.x >= bounds.min.x
                && tab.right() <= bounds.max.x
                && tab.y >= bounds.min.y
                && tab.bottom() <= bounds.max.y
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
        // Round only authored partitions; clipping supplies all inherited
        // border and title curves without re-filleting flattened arcs.
        let rounded = fillet(&union(&shapes, i.geometry)?, rounding)?.path;
        panels.push((
            *index,
            combine(
                &i.corners.shape(&rounded),
                &available,
                BooleanOp::Intersection,
            )?,
        ));
    }
    Ok(Geometry {
        panels,
        joins,
        join_nodes: i.joins.iter().map(|(index, ..)| *index).collect(),
    })
}
