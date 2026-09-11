use crate::math::{point_segment_distance, signed_area};
use crate::{Arc, Error, Path, PathCommand, Point, RingKind, Topology};
use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerStyle {
    pub convex_radius: f64,
    pub concave_radius: f64,
    /// Strictly below 0.5: neighboring protected disks cannot overlap.
    /// Larger radii are allowed only as far as ALL geometric clearances allow.
    pub clearance_fraction: f64,
}
impl Default for CornerStyle {
    fn default() -> Self {
        Self {
            convex_radius: 24.,
            concave_radius: 32.,
            clearance_fraction: 0.49,
        }
    }
}
impl CornerStyle {
    fn validate(self) -> Result<(), Error> {
        if ![
            self.convex_radius,
            self.concave_radius,
            self.clearance_fraction,
        ]
        .iter()
        .all(|v| v.is_finite())
        {
            return Err(Error::NonFinite);
        }
        if self.convex_radius < 0. || self.concave_radius < 0. {
            return Err(Error::InvalidOptions("radii cannot be negative"));
        }
        if self.clearance_fraction <= 0. || self.clearance_fraction >= 0.5 {
            return Err(Error::InvalidOptions(
                "clearance_fraction must be in (0, 0.5)",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct Corner {
    pub vertex: Point,
    pub start: Point,
    pub end: Point,
    pub requested_radius: f64,
    pub effective_radius: f64,
    pub trim: f64,
    pub concave: bool,
    pub limited: bool,
    pub arc: Option<Arc>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct RoundedShape {
    pub path: Path,
    /// Same ring/vertex ordering as the input Topology; useful for an inspector.
    pub corners: Vec<Vec<Corner>>,
}

/// This is a conservative, local circular-fillet solver, NOT a medial-axis
/// optimizer and NOT a distance-based smooth union.
///
/// A fillet only changes the triangle bounded by its vertex and tangent points.
/// That triangle is contained in a disk of radius `trim` around the vertex.
///
/// For each vertex we bound this disk against:
///   1. EVERY other vertex, including those on other components/holes;
///   2. EVERY nonincident edge, not just its two adjacent edges.
///
/// With fraction < 0.5 the disks are disjoint and avoid unrelated boundaries.
/// That is more conservative than only enforcing trim_i + trim_j < edge_length,
/// but also handles a narrow channel facing a nonadjacent boundary. O(V²).
///
/// Topology can still change abruptly when touching/separating inputs cause the
/// Boolean result itself to change. Exact set union does not promise goo motion.
pub fn fillet(topology: &Topology, style: CornerStyle) -> Result<RoundedShape, Error> {
    style.validate()?;
    let mut corners = Vec::with_capacity(topology.rings.len());
    for (ri, ring) in topology.rings.iter().enumerate() {
        let n = ring.points.len();
        let winding = signed_area(&ring.points).signum();
        let mut output = Vec::with_capacity(n);
        for i in 0..n {
            let p = ring.points[i];
            let prev = ring.points[(i + n - 1) % n];
            let next = ring.points[(i + 1) % n];
            let incoming = (p - prev).unit();
            let outgoing = (next - p).unit();
            let turn = incoming.cross(outgoing).atan2(incoming.dot(outgoing));
            let locally_convex = turn * winding > 0.;
            let convex = if ring.kind == RingKind::Hole {
                !locally_convex
            } else {
                locally_convex
            };
            let requested = if convex {
                style.convex_radius
            } else {
                style.concave_radius
            };
            let mut clearance = f64::INFINITY;
            for (rj, other) in topology.rings.iter().enumerate() {
                for (j, &q) in other.points.iter().enumerate() {
                    if ri != rj || i != j {
                        clearance = clearance.min(p.distance(q));
                    }
                    let k = (j + 1) % other.points.len();
                    if ri != rj || (j != i && k != i) {
                        clearance = clearance.min(point_segment_distance(p, q, other.points[k]));
                    }
                }
            }
            // Straight points and near-180-degree cusps remain sharp. Avoid an
            // ill-conditioned circle center; no fabricated zero-vector normals.
            let valid = turn.abs() > 1e-8 && turn.abs() < PI - 1e-6;
            let factor = (turn.abs() * 0.5).tan();
            let trim = if valid {
                (requested * factor).min(clearance * style.clearance_fraction)
            } else {
                0.
            };
            let radius = if valid { trim / factor } else { 0. };
            let start = p - incoming * trim;
            let end = p + outgoing * trim;
            let arc = if radius > 0. {
                let center = start + incoming.perpendicular() * (radius * turn.signum());
                Some(Arc {
                    center,
                    radius,
                    start_angle: (start.y - center.y).atan2(start.x - center.x),
                    sweep: turn,
                    to: end,
                })
            } else {
                None
            };
            output.push(Corner {
                vertex: p,
                start,
                end,
                requested_radius: requested,
                effective_radius: radius,
                trim,
                concave: !convex,
                limited: radius + 1e-8 < requested,
                arc,
            });
        }
        corners.push(output);
    }
    let mut path = Path::default();
    for ring in &corners {
        if ring.is_empty() {
            continue;
        }
        path.commands.push(PathCommand::MoveTo(ring[0].end));
        for i in 1..=ring.len() {
            let corner = &ring[i % ring.len()];
            path.commands.push(PathCommand::LineTo(corner.start));
            if let Some(arc) = corner.arc {
                path.commands.push(PathCommand::ArcTo(arc));
            } else {
                path.commands.push(PathCommand::LineTo(corner.end));
            }
        }
        path.commands.push(PathCommand::Close);
    }
    Ok(RoundedShape { path, corners })
}
