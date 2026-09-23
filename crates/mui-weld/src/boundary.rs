//! Boundary-first crisp welding. A union is built from exposed line/arc pieces,
//! not from a smooth minimum. Distance is measured to the resulting boundary.
//! Geometry is prepared only when plates move/resize; materials and morphs do not
//! rebuild it. Coincident seams are classified using an outward probe.
use crate::{Error, Point};
use std::f64::consts::{FRAC_PI_2, TAU};

pub const MAX_BOUNDARY_SEGMENTS: usize = 128;
pub const BOUNDARY_BYTES: usize = MAX_BOUNDARY_SEGMENTS * 48;
const EPS: f64 = 1e-8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plate {
    pub center: Point,
    pub half: Point,
    pub radius: f64,
    pub angle: f64,
}
impl Plate {
    pub fn validate(self) -> Result<(), Error> {
        self.center.validate()?;
        self.half.validate()?;
        if self.half.x <= 0.0
            || self.half.y <= 0.0
            || !self.radius.is_finite()
            || self.radius < 0.0
            || !self.angle.is_finite()
        {
            return Err(Error::Invalid("crisp plate"));
        }
        Ok(())
    }
    fn transform(self, p: Point) -> Point {
        let (s, c) = self.angle.sin_cos();
        self.center + Point::new(c * p.x - s * p.y, s * p.x + c * p.y)
    }
    pub fn distance(self, p: Point) -> f64 {
        let (s, c) = self.angle.sin_cos();
        let v = p - self.center;
        let r = self.radius.min(self.half.x).min(self.half.y);
        let q = Point::new(
            (c * v.x + s * v.y).abs() - self.half.x + r,
            (-s * v.x + c * v.y).abs() - self.half.y + r,
        );
        Point::new(q.x.max(0.0), q.y.max(0.0)).length() + q.x.max(q.y).min(0.0) - r
    }
    fn pieces(self, owner: usize) -> Vec<Piece> {
        let (x, y) = (self.half.x, self.half.y);
        let r = self.radius.min(x).min(y);
        let points = [
            (Point::new(-x + r, -y), Point::new(x - r, -y)),
            (Point::new(x, -y + r), Point::new(x, y - r)),
            (Point::new(x - r, y), Point::new(-x + r, y)),
            (Point::new(-x, y - r), Point::new(-x, -y + r)),
        ];
        let centers = [
            Point::new(x - r, -y + r),
            Point::new(x - r, y - r),
            Point::new(-x + r, y - r),
            Point::new(-x + r, -y + r),
        ];
        let mut result = Vec::with_capacity(8);
        for i in 0..4 {
            let (a, b) = points[i];
            if (b - a).length() > EPS {
                result.push(Piece {
                    owner,
                    edge: Edge::Line {
                        a: self.transform(a),
                        b: self.transform(b),
                    },
                });
            }
            if r > EPS {
                result.push(Piece {
                    owner,
                    edge: Edge::Arc {
                        center: self.transform(centers[i]),
                        radius: r,
                        start: (i as f64 - 1.0) * FRAC_PI_2 + self.angle,
                        sweep: FRAC_PI_2,
                    },
                });
            }
        }
        result
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edge {
    Line {
        a: Point,
        b: Point,
    },
    /// Positive sweep, at most pi/2. This bound permits an atan-free GPU nearest point.
    Arc {
        center: Point,
        radius: f64,
        start: f64,
        sweep: f64,
    },
}
impl Edge {
    pub fn point(self, t: f64) -> Point {
        match self {
            Self::Line { a, b } => a + (b - a) * t,
            Self::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let (s, c) = (start + sweep * t).sin_cos();
                center + Point::new(c, s) * radius
            }
        }
    }
    fn parameter(self, p: Point) -> Option<f64> {
        match self {
            Self::Line { a, b } => {
                let v = b - a;
                let norm = v.dot(v);
                if norm < EPS * EPS {
                    return None;
                }
                let t = (p - a).dot(v) / norm;
                if !(-EPS..=1.0 + EPS).contains(&t)
                    || (p - a).cross(v).abs() > EPS * 64.0 * v.length().max(1.0)
                {
                    return None;
                }
                Some(t.clamp(0.0, 1.0))
            }
            Self::Arc {
                center,
                radius,
                start,
                sweep,
            } => {
                let q = p - center;
                if (q.length() - radius).abs() > EPS * 64.0 * radius.max(1.0) {
                    return None;
                }
                let mut a = (q.y.atan2(q.x) - start).rem_euclid(TAU);
                if a > TAU - EPS * 64.0 {
                    a = 0.0;
                }
                (a <= sweep + EPS * 64.0).then(|| (a / sweep).clamp(0.0, 1.0))
            }
        }
    }
    pub fn closest(self, p: Point) -> Point {
        match self {
            Self::Line { a, b } => {
                let v = b - a;
                a + v * ((p - a).dot(v) / v.dot(v)).clamp(0.0, 1.0)
            }
            Self::Arc { center, radius, .. } => {
                let v = p - center;
                let n = v.length();
                if n > EPS {
                    let q = center + v * (radius / n);
                    if self.parameter(q).is_some() {
                        return q;
                    }
                }
                let (a, b) = (self.point(0.0), self.point(1.0));
                if (p - a).length() <= (p - b).length() {
                    a
                } else {
                    b
                }
            }
        }
    }
    fn slice(self, a: f64, b: f64) -> Self {
        match self {
            Self::Line { .. } => Self::Line {
                a: self.point(a),
                b: self.point(b),
            },
            Self::Arc {
                center,
                radius,
                start,
                sweep,
            } => Self::Arc {
                center,
                radius,
                start: start + sweep * a,
                sweep: sweep * (b - a),
            },
        }
    }
    fn outward(self, t: f64) -> Point {
        let tangent = match self {
            Self::Line { a, b } => b - a,
            Self::Arc { start, sweep, .. } => {
                let (s, c) = (start + sweep * t).sin_cos();
                Point::new(-s, c)
            }
        };
        Point::new(tangent.y, -tangent.x) * (1.0 / tangent.length())
    }
}
#[derive(Clone, Copy)]
struct Piece {
    owner: usize,
    edge: Edge,
}

fn intersections(a: Edge, b: Edge) -> Vec<Point> {
    let mut out = Vec::with_capacity(4);
    let mut push = |p: Point| {
        if a.parameter(p).is_some()
            && b.parameter(p).is_some()
            && !out.iter().any(|q: &Point| (p - *q).length() < EPS * 32.0)
        {
            out.push(p);
        }
    };
    match (a, b) {
        (Edge::Line { a: a0, b: a1 }, Edge::Line { a: b0, b: b1 }) => {
            let (v, w) = (a1 - a0, b1 - b0);
            let den = v.cross(w);
            if den.abs() > EPS * v.length() * w.length() {
                push(a0 + v * ((b0 - a0).cross(w) / den));
            } else {
                for p in [a0, a1, b0, b1] {
                    push(p);
                }
            }
        }
        (line @ Edge::Line { .. }, arc @ Edge::Arc { .. })
        | (arc @ Edge::Arc { .. }, line @ Edge::Line { .. }) => {
            let Edge::Line { a: l0, b: l1 } = line else {
                unreachable!()
            };
            let Edge::Arc { center, radius, .. } = arc else {
                unreachable!()
            };
            let (v, o) = (l1 - l0, l0 - center);
            let aa = v.dot(v);
            let bb = 2.0 * o.dot(v);
            let cc = o.dot(o) - radius * radius;
            let disc = bb * bb - 4.0 * aa * cc;
            if disc >= -EPS * (bb * bb).max((4.0 * aa * cc).abs()).max(1.0) {
                let root = disc.max(0.0).sqrt();
                push(l0 + v * ((-bb - root) / (2.0 * aa)));
                push(l0 + v * ((-bb + root) / (2.0 * aa)));
            }
        }
        (
            Edge::Arc {
                center: a0,
                radius: ra,
                ..
            },
            Edge::Arc {
                center: b0,
                radius: rb,
                ..
            },
        ) => {
            let v = b0 - a0;
            let d = v.length();
            if d < EPS {
                if (ra - rb).abs() < EPS * 32.0 {
                    for p in [a.point(0.0), a.point(1.0), b.point(0.0), b.point(1.0)] {
                        push(p);
                    }
                }
            } else if d <= ra + rb + EPS && d >= (ra - rb).abs() - EPS {
                let x = (ra * ra - rb * rb + d * d) / (2.0 * d);
                let h = (ra * ra - x * x).max(0.0).sqrt();
                let base = a0 + v * (x / d);
                let off = Point::new(-v.y, v.x) * (h / d);
                push(base + off);
                push(base - off);
            }
        }
    }
    out
}

/// True exposed boundary of a hard union, within floating-point tolerance.
/// No reach is applied and no connecting blob is introduced between disjoint shapes.
#[derive(Clone, Debug, PartialEq)]
pub struct Boundary {
    plates: Vec<Plate>,
    edges: Vec<Edge>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundarySample {
    pub distance: f64,
    pub point: Point,
}
impl Boundary {
    pub fn new(plates: Vec<Plate>) -> Result<Self, Error> {
        if plates.is_empty() || plates.len() > 3 {
            return Err(Error::Invalid("crisp supports one to three plates"));
        }
        for p in &plates {
            p.validate()?;
        }
        let all: Vec<_> = plates
            .iter()
            .enumerate()
            .flat_map(|(i, p)| p.pieces(i))
            .collect();
        let mut edges = Vec::new();
        for piece in &all {
            let e = piece.edge;
            let mut cuts = vec![0.0, 1.0];
            for other in all.iter().filter(|p| p.owner != piece.owner) {
                for p in intersections(e, other.edge) {
                    if let Some(t) = e.parameter(p) {
                        cuts.push(t);
                    }
                }
            }
            cuts.sort_by(f64::total_cmp);
            cuts.dedup_by(|a, b| (*a - *b).abs() <= EPS);
            for ends in cuts.windows(2) {
                let (a, b) = (ends[0], ends[1]);
                if b - a <= EPS {
                    continue;
                }
                let t = (a + b) * 0.5;
                let mid = e.point(t);
                let probe = mid + e.outward(t) * 1e-6;
                let hidden = plates.iter().enumerate().any(|(i, p)| {
                    i != piece.owner
                        && (p.distance(mid) < -EPS * 16.0
                            || (p.distance(mid).abs() <= EPS * 16.0 && p.distance(probe) < -EPS))
                });
                if !hidden {
                    edges.push(e.slice(a, b));
                    if edges.len() > MAX_BOUNDARY_SEGMENTS {
                        return Err(Error::Budget);
                    }
                }
            }
        }
        if edges.is_empty() {
            return Err(Error::Invalid("empty crisp union boundary"));
        }
        Ok(Self { plates, edges })
    }
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }
    pub fn sample(&self, p: Point) -> BoundarySample {
        let mut closest = p;
        let mut d = f64::INFINITY;
        for e in &self.edges {
            let q = e.closest(p);
            let n = (q - p).length();
            if n < d {
                d = n;
                closest = q;
            }
        }
        if self.plates.iter().any(|s| s.distance(p) <= 0.0) {
            d = -d;
        }
        BoundarySample {
            distance: d,
            point: closest,
        }
    }
    /// Uniform buffer ABI: three vec4s per piece. kind is meta.x, 0=line, 1=arc.
    /// Circle endpoints are unit vectors; GPU closest-point testing uses crosses
    /// instead of atan2. Zero-filled tail is never read past the explicit count.
    pub fn uniform_bytes(&self, origin: Point) -> [u8; BOUNDARY_BYTES] {
        let mut bytes = [0u8; BOUNDARY_BYTES];
        for (i, e) in self.edges.iter().enumerate() {
            let values = match *e {
                Edge::Line { a, b } => [
                    a.x - origin.x,
                    a.y - origin.y,
                    b.x - origin.x,
                    b.y - origin.y,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                ],
                Edge::Arc {
                    center,
                    radius,
                    start,
                    sweep,
                } => [
                    center.x - origin.x,
                    center.y - origin.y,
                    radius,
                    1.0,
                    start.cos(),
                    start.sin(),
                    (start + sweep).cos(),
                    (start + sweep).sin(),
                    1.0,
                    0.0,
                    0.0,
                    0.0,
                ],
            };
            for (j, v) in values.into_iter().enumerate() {
                bytes[i * 48 + j * 4..i * 48 + j * 4 + 4]
                    .copy_from_slice(&(v as f32).to_le_bytes());
            }
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn plate(x: f64) -> Plate {
        Plate {
            center: Point::new(x, 0.0),
            half: Point::new(20.0, 12.0),
            radius: 4.0,
            angle: 0.0,
        }
    }
    #[test]
    fn isolated_plate_distance_is_unchanged() {
        let p = plate(0.0);
        let b = Boundary::new(vec![p]).unwrap();
        for x in -30..30 {
            for y in -20..20 {
                let q = Point::new(x as f64 + 0.13, y as f64 + 0.27);
                assert!((b.sample(q).distance - p.distance(q)).abs() < 1e-6);
            }
        }
    }
    #[test]
    fn shared_edge_is_not_a_border() {
        let mut a = plate(-20.0);
        a.radius = 0.0;
        let mut b = plate(20.0);
        b.radius = 0.0;
        let u = Boundary::new(vec![a, b]).unwrap();
        assert!((u.sample(Point::new(0.0, 0.0)).distance + 12.0).abs() < 1e-6);
    }
    #[test]
    fn coincident_shapes_keep_the_exterior() {
        let p = plate(0.0);
        let b = Boundary::new(vec![p, p]).unwrap();
        assert!((b.sample(Point::new(0.0, 0.0)).distance + 12.0).abs() < 1e-6);
    }
    #[test]
    fn contained_shape_has_no_interior_edge() {
        let a = plate(0.0);
        let b = Plate {
            half: Point::new(3.0, 2.0),
            radius: 1.0,
            ..a
        };
        let u = Boundary::new(vec![a, b]).unwrap();
        assert!((u.sample(Point::new(0.0, 0.0)).distance + 12.0).abs() < 1e-6);
    }
    #[test]
    fn union_is_order_independent() {
        let a = plate(-10.0);
        let b = Plate {
            angle: std::f64::consts::PI / 3.0,
            ..plate(10.0)
        };
        let x = Boundary::new(vec![a, b]).unwrap();
        let y = Boundary::new(vec![b, a]).unwrap();
        for i in -25..25 {
            let p = Point::new(i as f64, 2.31);
            assert!((x.sample(p).distance - y.sample(p).distance).abs() < 1e-6);
        }
    }
    #[test]
    fn gpu_uniform_size_is_bounded() {
        let b = Boundary::new(vec![plate(0.0)]).unwrap();
        assert_eq!(b.uniform_bytes(Point::new(0.0, 0.0)).len(), 6144);
        assert!(b.edges().len() <= 128);
    }
}
