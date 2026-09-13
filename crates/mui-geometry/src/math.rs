use crate::Error;
use std::ops::{Add, Div, Mul, Neg, Sub};

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
impl Point {
    pub const ZERO: Self = Self::new(0.0, 0.0);
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
    pub fn dot(self, b: Self) -> f64 {
        self.x * b.x + self.y * b.y
    }
    pub fn cross(self, b: Self) -> f64 {
        self.x * b.y - self.y * b.x
    }
    pub fn length(self) -> f64 {
        self.x.hypot(self.y)
    }
    pub fn distance(self, b: Self) -> f64 {
        (self - b).length()
    }
    pub fn perpendicular(self) -> Self {
        Self::new(-self.y, self.x)
    }
    pub fn rotated(self, radians: f64) -> Self {
        let (s, c) = radians.sin_cos();
        Self::new(c * self.x - s * self.y, s * self.x + c * self.y)
    }
    pub(crate) fn unit(self) -> Self {
        self / self.length()
    }
}
impl Add for Point {
    type Output = Self;
    fn add(self, b: Self) -> Self {
        Self::new(self.x + b.x, self.y + b.y)
    }
}
impl Sub for Point {
    type Output = Self;
    fn sub(self, b: Self) -> Self {
        Self::new(self.x - b.x, self.y - b.y)
    }
}
impl Mul<f64> for Point {
    type Output = Self;
    fn mul(self, s: f64) -> Self {
        Self::new(self.x * s, self.y * s)
    }
}
impl Div<f64> for Point {
    type Output = Self;
    fn div(self, s: f64) -> Self {
        Self::new(self.x / s, self.y / s)
    }
}
impl Neg for Point {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

/// x' = xx*x + xy*y + tx; y' = yx*x + yy*y + ty.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub xx: f64,
    pub xy: f64,
    pub yx: f64,
    pub yy: f64,
    pub tx: f64,
    pub ty: f64,
}
impl Default for Affine {
    fn default() -> Self {
        Self::IDENTITY
    }
}
impl Affine {
    pub const IDENTITY: Self = Self {
        xx: 1.,
        xy: 0.,
        yx: 0.,
        yy: 1.,
        tx: 0.,
        ty: 0.,
    };
    pub fn translation(x: f64, y: f64) -> Self {
        Self {
            tx: x,
            ty: y,
            ..Self::IDENTITY
        }
    }
    pub fn scale(x: f64, y: f64) -> Self {
        Self {
            xx: x,
            yy: y,
            ..Self::IDENTITY
        }
    }
    pub fn rotation(a: f64) -> Self {
        let (s, c) = a.sin_cos();
        Self {
            xx: c,
            xy: -s,
            yx: s,
            yy: c,
            tx: 0.,
            ty: 0.,
        }
    }
    pub fn rotation_about(a: f64, p: Point) -> Self {
        Self::translation(-p.x, -p.y)
            .then(Self::rotation(a))
            .then(Self::translation(p.x, p.y))
    }
    /// Apply self FIRST, then next. Explicit order avoids matrix-convention guesses.
    pub fn then(self, n: Self) -> Self {
        Self {
            xx: n.xx * self.xx + n.xy * self.yx,
            xy: n.xx * self.xy + n.xy * self.yy,
            yx: n.yx * self.xx + n.yy * self.yx,
            yy: n.yx * self.xy + n.yy * self.yy,
            tx: n.xx * self.tx + n.xy * self.ty + n.tx,
            ty: n.yx * self.tx + n.yy * self.ty + n.ty,
        }
    }
    pub fn apply(self, p: Point) -> Point {
        Point::new(
            self.xx * p.x + self.xy * p.y + self.tx,
            self.yx * p.x + self.yy * p.y + self.ty,
        )
    }
    /// Invert a transform; fail if its determinant is zero/nonfinite or its inverse is nonfinite.
    pub fn inverse(self) -> Option<Self> {
        if !self.finite() {
            return None;
        }
        let d = self.xx * self.yy - self.xy * self.yx;
        if !d.is_finite() || d == 0. {
            return None;
        }
        let mut inverse = Self {
            xx: self.yy / d,
            xy: -self.xy / d,
            yx: -self.yx / d,
            yy: self.xx / d,
            tx: 0.,
            ty: 0.,
        };
        inverse.tx = -(inverse.xx * self.tx + inverse.xy * self.ty);
        inverse.ty = -(inverse.yx * self.tx + inverse.yy * self.ty);
        inverse.finite().then_some(inverse)
    }
    pub fn finite(self) -> bool {
        [self.xx, self.xy, self.yx, self.yy, self.tx, self.ty]
            .iter()
            .all(|x| x.is_finite())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: Point,
    pub max: Point,
}
impl Bounds {
    pub fn from_points(points: impl IntoIterator<Item = Point>) -> Option<Self> {
        let mut it = points.into_iter();
        let first = it.next()?;
        let mut out = Self {
            min: first,
            max: first,
        };
        for p in it {
            out.min.x = out.min.x.min(p.x);
            out.min.y = out.min.y.min(p.y);
            out.max.x = out.max.x.max(p.x);
            out.max.y = out.max.y.max(p.y);
        }
        Some(out)
    }
    pub fn width(self) -> f64 {
        self.max.x - self.min.x
    }
    pub fn height(self) -> f64 {
        self.max.y - self.min.y
    }
}

pub(crate) fn signed_area(points: &[Point]) -> f64 {
    // Translating first avoids subtracting very large products at distant origins.
    if points.len() < 3 {
        return 0.;
    }
    let o = points[0];
    (1..points.len() - 1)
        .map(|i| (points[i] - o).cross(points[i + 1] - o))
        .sum::<f64>()
        * 0.5
}
pub(crate) fn point_segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let v = b - a;
    let n = v.dot(v);
    if n == 0. {
        return p.distance(a);
    }
    p.distance(a + v * ((p - a).dot(v) / n).clamp(0., 1.))
}

/// Remove duplicate/collinear vertices without deleting an entire short edge twice.
pub(crate) fn clean_ring(input: &[Point], epsilon: f64) -> Result<Vec<Point>, Error> {
    if input.iter().any(|p| !p.finite()) {
        return Err(Error::NonFinite);
    }
    let mut p: Vec<Point> = Vec::with_capacity(input.len());
    for &v in input {
        if p.last().is_none_or(|&last| v.distance(last) > epsilon) {
            p.push(v);
        }
    }
    if p.len() > 1 && p[0].distance(*p.last().unwrap()) <= epsilon {
        p.pop();
    }
    loop {
        if p.len() < 3 {
            return Err(Error::DegenerateRing);
        }
        let n = p.len();
        let remove = (0..n).find(|&i| {
            let a = p[(i + n - 1) % n];
            let b = p[i];
            let c = p[(i + 1) % n];
            b.distance(a) <= epsilon
                || (point_segment_distance(b, a, c) <= epsilon && (b - a).dot(c - b) >= 0.)
        });
        if let Some(i) = remove {
            p.remove(i);
        } else {
            break;
        }
    }
    if signed_area(&p).abs() <= epsilon * epsilon {
        return Err(Error::DegenerateRing);
    }
    Ok(p)
}

/// Bounded O(V²) validation for caller-supplied polygon rings. The Boolean
/// backend handles intersections BETWEEN valid shapes, not malformed leaf rings.
pub(crate) fn validate_simple(p: &[Point], eps: f64) -> Result<(), Error> {
    let n = p.len();
    for i in 0..n {
        let a = p[i];
        let b = p[(i + 1) % n];
        let c = p[(i + 2) % n];
        if (b - a).cross(c - b).abs() <= eps * (b - a).length().max(1.0) && (b - a).dot(c - b) < 0.0
        {
            return Err(Error::SelfIntersection);
        }
        for j in i + 1..n {
            if j == (i + 1) % n || (j + 1) % n == i {
                continue;
            }
            let c = p[j];
            let d = p[(j + 1) % n];
            if point_segment_distance(a, c, d) <= eps
                || point_segment_distance(b, c, d) <= eps
                || point_segment_distance(c, a, b) <= eps
                || point_segment_distance(d, a, b) <= eps
            {
                return Err(Error::SelfIntersection);
            }
            let u = (b - a).cross(c - a);
            let v = (b - a).cross(d - a);
            let w = (d - c).cross(a - c);
            let z = (d - c).cross(b - c);
            if u.signum() != v.signum() && w.signum() != z.signum() {
                return Err(Error::SelfIntersection);
            }
        }
    }
    Ok(())
}
