use crate::math::{clean_ring, signed_area};
use crate::{Affine, Bounds, Error, Point};
use i_overlay::{
    core::{fill_rule::FillRule, overlay_rule::OverlayRule},
    float::single::SingleFloatOverlay,
};

type BackendPolygon = Vec<Vec<[f64; 2]>>;
pub(crate) type BackendMulti = Vec<BackendPolygon>;

#[derive(Debug, Clone, PartialEq)]
pub struct Polygon {
    pub exterior: Vec<Point>,
    /// Each hole is SUBTRACTED independently. Overlapping holes do not XOR.
    pub holes: Vec<Vec<Point>>,
}
impl Polygon {
    pub fn new(exterior: Vec<Point>) -> Self {
        Self {
            exterior,
            holes: Vec::new(),
        }
    }
    pub fn rectangle(x: f64, y: f64, w: f64, h: f64) -> Result<Self, Error> {
        if ![x, y, w, h].iter().all(|v| v.is_finite()) {
            return Err(Error::NonFinite);
        }
        if w <= 0. || h <= 0. {
            return Err(Error::DegenerateRing);
        }
        let x1 = x + w;
        let y1 = y + h;
        if !x1.is_finite() || !y1.is_finite() {
            return Err(Error::NonFinite);
        }
        if x1 <= x || y1 <= y {
            return Err(Error::DegenerateRing);
        }
        Ok(Self::new(vec![
            Point::new(x, y),
            Point::new(x1, y),
            Point::new(x1, y1),
            Point::new(x, y1),
        ]))
    }
    pub fn with_hole(mut self, hole: Vec<Point>) -> Self {
        self.holes.push(hole);
        self
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedShape {
    pub polygon: Polygon,
    pub transform: Affine,
}
impl From<Polygon> for PlacedShape {
    fn from(polygon: Polygon) -> Self {
        Self {
            polygon,
            transform: Affine::IDENTITY,
        }
    }
}
impl PlacedShape {
    pub fn transformed(mut self, t: Affine) -> Self {
        self.transform = self.transform.then(t);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeometryOptions {
    /// Cleanup tolerance in the SAME logical units as the input, not pixels.
    pub epsilon: f64,
    /// Guardrails for the conservative O(V²) fillet pass, not a renderer limit.
    pub max_vertices: usize,
    pub coordinate_limit: f64,
}
impl Default for GeometryOptions {
    fn default() -> Self {
        Self {
            epsilon: 1e-7,
            max_vertices: 4096,
            coordinate_limit: 1e7,
        }
    }
}
impl GeometryOptions {
    pub(crate) fn validate(self) -> Result<(), Error> {
        if !self.epsilon.is_finite() || self.epsilon <= 0. {
            return Err(Error::InvalidOptions("epsilon must be finite and positive"));
        }
        if self.max_vertices < 3 {
            return Err(Error::InvalidOptions("max_vertices must be at least three"));
        }
        if !self.coordinate_limit.is_finite() || self.coordinate_limit <= self.epsilon {
            return Err(Error::InvalidOptions("coordinate_limit"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingKind {
    Exterior,
    Hole,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    pub(crate) points: Vec<Point>,
    pub(crate) kind: RingKind,
    pub(crate) component: usize,
}
impl Ring {
    pub fn points(&self) -> &[Point] {
        &self.points
    }
    pub fn kind(&self) -> RingKind {
        self.kind
    }
    pub fn component(&self) -> usize {
        self.component
    }
    pub fn signed_area(&self) -> f64 {
        signed_area(&self.points)
    }
}
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Topology {
    pub(crate) rings: Vec<Ring>,
}
impl Topology {
    pub fn rings(&self) -> &[Ring] {
        &self.rings
    }
    pub fn components(&self) -> usize {
        self.rings
            .iter()
            .filter(|r| r.kind == RingKind::Exterior)
            .count()
    }
    pub fn area(&self) -> f64 {
        self.rings.iter().map(Ring::signed_area).sum()
    }
    pub fn bounds(&self) -> Option<Bounds> {
        Bounds::from_points(self.rings.iter().flat_map(|r| r.points.iter().copied()))
    }
    pub fn vertex_count(&self) -> usize {
        self.rings.iter().map(|r| r.points.len()).sum()
    }
    /// Convert this already-normalized topology back into polygon inputs.
    /// Useful when a higher-level compositor needs to union resolved surfaces
    /// without exposing the clipping backend representation.
    pub fn polygons(&self) -> Vec<Polygon> {
        let mut out: Vec<Polygon> = Vec::new();
        for component in 0..self.components() {
            let Some(exterior) = self
                .rings
                .iter()
                .find(|r| r.component == component && r.kind == RingKind::Exterior)
            else {
                continue;
            };
            let mut p = Polygon::new(exterior.points.clone());
            for hole in self
                .rings
                .iter()
                .filter(|r| r.component == component && r.kind == RingKind::Hole)
            {
                p.holes.push(hole.points.clone());
            }
            out.push(p);
        }
        out
    }
    pub fn placed_shapes(&self) -> Vec<PlacedShape> {
        self.polygons().into_iter().map(PlacedShape::from).collect()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    Union,
    Intersection,
    Difference,
    Xor,
}
impl BooleanOp {
    pub(crate) fn rule(self) -> OverlayRule {
        match self {
            Self::Union => OverlayRule::Union,
            Self::Intersection => OverlayRule::Intersect,
            Self::Difference => OverlayRule::Difference,
            Self::Xor => OverlayRule::Xor,
        }
    }
}
fn prepared_ring(p: &[Point], t: Affine, o: GeometryOptions) -> Result<Vec<[f64; 2]>, Error> {
    if !t.finite() {
        return Err(Error::NonFinite);
    }
    let transformed: Vec<_> = p.iter().map(|&v| t.apply(v)).collect();
    if transformed.iter().any(|p| !p.finite()) {
        return Err(Error::NonFinite);
    }
    if transformed
        .iter()
        .any(|p| p.x.abs() > o.coordinate_limit || p.y.abs() > o.coordinate_limit)
    {
        return Err(Error::CoordinateLimit);
    }
    let mut ring = clean_ring(&transformed, o.epsilon)?;
    crate::math::validate_simple(&ring, o.epsilon)?;
    if signed_area(&ring) < 0. {
        ring.reverse();
    }
    Ok(ring.into_iter().map(|p| [p.x, p.y]).collect())
}
fn prepared_shape(s: &PlacedShape, o: GeometryOptions) -> Result<BackendMulti, Error> {
    let exterior = prepared_ring(&s.polygon.exterior, s.transform, o)?;
    // Union with itself regularizes an individual input; no empty-input inference.
    // Input exteriors are expected to be simple polygon rings.
    let mut out = exterior.overlay_as::<i64>(&exterior, OverlayRule::Union, FillRule::NonZero);
    for h in &s.polygon.holes {
        let hole = prepared_ring(h, s.transform, o)?;
        out = out.overlay_as::<i64>(&hole, OverlayRule::Difference, FillRule::NonZero);
    }
    Ok(out)
}
fn prepare_all(shapes: &[PlacedShape], o: GeometryOptions) -> Result<BackendMulti, Error> {
    o.validate()?;
    let count: usize = shapes
        .iter()
        .map(|s| s.polygon.exterior.len() + s.polygon.holes.iter().map(Vec::len).sum::<usize>())
        .sum();
    if count > o.max_vertices {
        return Err(Error::TooManyVertices);
    }
    let mut out: BackendMulti = Vec::new();
    for s in shapes {
        let next = prepared_shape(s, o)?;
        if out.is_empty() {
            out = next;
        } else if !next.is_empty() {
            // True set union, not globally applying EvenOdd to overlapping inputs.
            out = out.overlay_as::<i64>(&next, OverlayRule::Union, FillRule::NonZero);
        }
        if out
            .iter()
            .flat_map(|p| p.iter())
            .map(Vec::len)
            .sum::<usize>()
            > o.max_vertices
        {
            return Err(Error::TooManyVertices);
        }
    }
    Ok(out)
}
pub(crate) fn topology(raw: BackendMulti, o: GeometryOptions) -> Result<Topology, Error> {
    let mut rings = Vec::new();
    let mut count = 0;
    'polygons: for (component, poly) in raw.into_iter().enumerate() {
        for (index, ring) in poly.into_iter().enumerate() {
            let points: Vec<_> = ring.into_iter().map(|p| Point::new(p[0], p[1])).collect();
            let mut points = match clean_ring(&points, o.epsilon) {
                Ok(p) => p,
                Err(Error::DegenerateRing) if index == 0 => continue 'polygons,
                Err(Error::DegenerateRing) => continue,
                Err(e) => return Err(e),
            };
            let kind = if index == 0 {
                RingKind::Exterior
            } else {
                RingKind::Hole
            };
            let should_be_positive = kind == RingKind::Exterior;
            if (signed_area(&points) > 0.) != should_be_positive {
                points.reverse();
            }
            count += points.len();
            if count > o.max_vertices {
                return Err(Error::TooManyVertices);
            }
            rings.push(Ring {
                points,
                kind,
                component,
            });
        }
    }
    Ok(Topology { rings })
}
/// Merge all supplied shapes. Empty input is a valid empty result.
pub fn union(shapes: &[PlacedShape], options: GeometryOptions) -> Result<Topology, Error> {
    topology(prepare_all(shapes, options)?, options)
}
/// Each side is a union of its shapes, followed by the requested set operation.
pub fn boolean(
    lhs: &[PlacedShape],
    rhs: &[PlacedShape],
    op: BooleanOp,
    options: GeometryOptions,
) -> Result<Topology, Error> {
    let a = prepare_all(lhs, options)?;
    let b = prepare_all(rhs, options)?;
    let result = if a.is_empty() {
        match op {
            BooleanOp::Union | BooleanOp::Xor => b,
            _ => Vec::new(),
        }
    } else if b.is_empty() {
        match op {
            BooleanOp::Intersection => Vec::new(),
            _ => a,
        }
    } else {
        a.overlay_as::<i64>(&b, op.rule(), FillRule::NonZero)
    };
    topology(result, options)
}
