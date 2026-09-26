use crate::brush::Premul;
use crate::{Brush, Channel, Color, Error, MAX_SOURCES, Weld};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub(crate) fn dot(self, b: Self) -> f64 {
        self.x * b.x + self.y * b.y
    }
    pub(crate) fn cross(self, b: Self) -> f64 {
        self.x * b.y - self.y * b.x
    }
    pub(crate) fn length(self) -> f64 {
        self.dot(self).sqrt()
    }
    pub(crate) fn validate(self) -> Result<(), Error> {
        if !self.x.is_finite() || !self.y.is_finite() || self.x.abs() > 1e7 || self.y.abs() > 1e7 {
            Err(Error::Invalid("shape coordinate"))
        } else {
            Ok(())
        }
    }
}
impl std::ops::Sub for Point {
    type Output = Self;
    fn sub(self, b: Self) -> Self {
        Self::new(self.x - b.x, self.y - b.y)
    }
}
impl std::ops::Add for Point {
    type Output = Self;
    fn add(self, b: Self) -> Self {
        Self::new(self.x + b.x, self.y + b.y)
    }
}
impl std::ops::Mul<f64> for Point {
    type Output = Self;
    fn mul(self, b: f64) -> Self {
        Self::new(self.x * b, self.y * b)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}
impl Rect {
    pub const fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self { x0, y0, x1, y1 }
    }
    pub fn width(self) -> f64 {
        self.x1 - self.x0
    }
    pub fn height(self) -> f64 {
        self.y1 - self.y0
    }
    pub(crate) fn validate(self) -> Result<(), Error> {
        Point::new(self.x0, self.y0).validate()?;
        Point::new(self.x1, self.y1).validate()?;
        if self.width() <= 0.0 || self.height() <= 0.0 {
            Err(Error::Invalid("empty shape bounds"))
        } else {
            Ok(())
        }
    }
    pub(crate) fn union(self, b: Self) -> Self {
        Self::new(
            self.x0.min(b.x0),
            self.y0.min(b.y0),
            self.x1.max(b.x1),
            self.y1.max(b.y1),
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Geometry {
    RoundedRect {
        bounds: Rect,
        radius: f64,
    },
    /// Closed contours without a repeated final vertex. Nonzero winding: holes
    /// must have the opposite orientation to their exterior, just as MUI paths do.
    Contours(Vec<Vec<Point>>),
}
impl Geometry {
    pub fn bounds(&self) -> Option<Rect> {
        match self {
            Self::RoundedRect { bounds, .. } => Some(*bounds),
            Self::Contours(rings) => {
                let mut points = rings.iter().flatten();
                let p = *points.next()?;
                Some(points.fold(Rect::new(p.x, p.y, p.x, p.y), |b, p| {
                    Rect::new(b.x0.min(p.x), b.y0.min(p.y), b.x1.max(p.x), b.y1.max(p.y))
                }))
            }
        }
    }
    pub fn distance(&self, p: Point) -> f64 {
        match self {
            Self::RoundedRect { bounds: b, radius } => {
                let (hx, hy) = (b.width() / 2.0, b.height() / 2.0);
                let r = radius.min(hx).min(hy).max(0.0);
                let q = Point::new(
                    (p.x - (b.x0 + hx)).abs() - hx + r,
                    (p.y - (b.y0 + hy)).abs() - hy + r,
                );
                Point::new(q.x.max(0.0), q.y.max(0.0)).length() + q.x.max(q.y).min(0.0) - r
            }
            Self::Contours(rings) => {
                // Branch-free over edges so it vectorises: squared distances,
                // one sqrt at the end (sqrt is monotone, so bit-identical).
                let (mut d2, mut winding) = (f64::INFINITY, 0i64);
                for ring in rings {
                    let Some(&last) = ring.last() else { continue };
                    let mut a = last;
                    for &b in ring {
                        let v = b - a;
                        let len = v.dot(v);
                        let t = if len > 0.0 {
                            ((p - a).dot(v) / len).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let e = p - (a + v * t);
                        d2 = d2.min(e.dot(e));
                        let cross = v.cross(p - a);
                        winding += i64::from((a.y <= p.y) & (b.y > p.y) & (cross > 0.0))
                            - i64::from((a.y > p.y) & (b.y <= p.y) & (cross < 0.0));
                        a = b;
                    }
                }
                let distance = d2.sqrt();
                if winding == 0 { distance } else { -distance }
            }
        }
    }
    pub(crate) fn validate(&self) -> Result<(), Error> {
        self.bounds()
            .ok_or(Error::Invalid("empty contours"))?
            .validate()?;
        match self {
            Self::RoundedRect { radius, .. }
                if !radius.is_finite() || *radius < 0.0 || *radius > 1e7 =>
            {
                Err(Error::Invalid("corner radius"))
            }
            Self::Contours(rings) => {
                for ring in rings {
                    if ring.len() < 3 {
                        return Err(Error::Invalid("contour needs at least three vertices"));
                    }
                    for p in ring {
                        p.validate()?;
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    pub(crate) fn cost(&self) -> usize {
        match self {
            Self::RoundedRect { .. } => 1,
            Self::Contours(r) => r.iter().map(Vec::len).sum(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub shape: Geometry,
    pub fill: Option<Brush>,
    pub border: Option<Brush>,
    /// Inside-border thickness in logical units, not centred-stroke width.
    pub width: f64,
}
impl Source {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        self.shape.validate()?;
        if !self.width.is_finite() || !(0.0..=1e4).contains(&self.width) {
            return Err(Error::Invalid("border width"));
        }
        if let Some(b) = &self.fill {
            b.validate()?;
        }
        if let Some(b) = &self.border {
            b.validate()?;
        }
        Ok(())
    }
}

/// Distance plus normalized material weights. This is useful to interpolate
/// additional application-owned numeric properties without inventing another
/// ownership heuristic. The raster backend currently consumes fill/border only.
#[derive(Clone, Debug)]
pub struct Sample {
    pub distance: f64,
    weights: [f64; MAX_SOURCES],
    len: usize,
}
impl Sample {
    pub fn weights(&self) -> &[f64] {
        &self.weights[..self.len]
    }
    /// Interpolate an application-owned finite scalar using the SAME ownership
    /// as the body/border. Categorical properties need an explicit policy instead.
    pub fn scalar(&self, values: &[f64]) -> Result<f64, Error> {
        if values.len() != self.len || values.iter().any(|v| !v.is_finite()) {
            return Err(Error::Invalid("scalar property count or value"));
        }
        let result: f64 = values.iter().zip(self.weights()).map(|(v, w)| v * w).sum();
        if !result.is_finite() {
            return Err(Error::Invalid("scalar interpolation overflow"));
        }
        Ok(result)
    }
}

/// An order-independent, compact smooth minimum. It minimizes
/// sum(w_i*d_i) + k/2*(sum(w_i²)-1), subject to w_i>=0 and sum(w_i)=1.
/// The active-set solution has w_i=max((lambda-d_i)/k,0). In the two-source
/// case it is the familiar quadratic smooth-min, but a left fold would be
/// order-dependent for three sources. Shifting by the minimum avoids cancellation.
pub(crate) fn smooth_min(d: &[f64], k: f64) -> (f64, [f64; MAX_SOURCES]) {
    let minimum = d.iter().copied().fold(f64::INFINITY, f64::min);
    let mut weights = [0.0; MAX_SOURCES];
    if k <= 1e-12 {
        let ties = d.iter().filter(|&&v| v == minimum).count();
        for (w, v) in weights.iter_mut().zip(d) {
            *w = if *v == minimum {
                1.0 / ties as f64
            } else {
                0.0
            };
        }
        return (minimum, weights);
    }
    let mut sorted = [0.0; MAX_SOURCES];
    for (a, v) in sorted.iter_mut().zip(d) {
        *a = *v - minimum;
    }
    sorted[..d.len()].sort_unstable_by(f64::total_cmp);
    let (mut sum, mut lambda) = (0.0, k);
    for (i, v) in sorted[..d.len()].iter().enumerate().skip(1) {
        if *v >= lambda {
            break;
        }
        sum += *v;
        lambda = (k + sum) / (i + 1) as f64;
    }
    let mut total = 0.0;
    for (w, v) in weights.iter_mut().zip(d) {
        *w = ((lambda - (*v - minimum)) / k).max(0.0);
        total += *w;
    }
    let (mut linear, mut squares) = (0.0, 0.0);
    for (w, v) in weights.iter_mut().zip(d) {
        *w /= total;
        linear += *w * (*v - minimum);
        squares += *w * *w;
    }
    (minimum + linear - 0.5 * k * (1.0 - squares), weights)
}

pub fn sample_field(sources: &[Source], point: Point, weld: Weld) -> Result<Sample, Error> {
    weld.validate()?;
    point.validate()?;
    if sources.is_empty() || sources.len() > MAX_SOURCES {
        return Err(Error::Invalid("source count"));
    }
    for s in sources {
        s.validate()?;
    }
    Ok(field(sources, point, weld))
}
pub(crate) fn distances(sources: &[Source], p: Point) -> [f64; MAX_SOURCES] {
    let mut d = [0.0; MAX_SOURCES];
    for (a, s) in d.iter_mut().zip(sources) {
        *a = s.shape.distance(p);
    }
    d
}
pub(crate) fn field(sources: &[Source], p: Point, weld: Weld) -> Sample {
    let d = distances(sources, p);
    let d = &d[..sources.len()];
    let t = weld.amount();
    let (distance, _) = smooth_min(d, 2.0 * weld.reach * t);
    let (_, weights) = smooth_min(d, weld.blend * t);
    Sample {
        distance,
        weights,
        len: sources.len(),
    }
}
fn coverage(d: f64, px: f64) -> f64 {
    (0.5 - d / px).clamp(0.0, 1.0)
}
fn colour(paint: &Option<Brush>, p: Point) -> Color {
    paint.as_ref().map_or(Color::TRANSPARENT, |b| b.sample(p))
}

/// Evaluate a pixel with a one-pixel analytic coverage ramp. This is exposed for
/// reference backends/tests; callers supplying arbitrary input must validate the
/// Request first, or use `bake`, which always performs validation.
#[cfg(test)]
pub(crate) fn pixel(sources: &[Source], p: Point, weld: Weld, px: f64) -> Color {
    pixel_prepared(sources, p, weld, px, None)
}
pub(crate) fn pixel_prepared(
    sources: &[Source],
    p: Point,
    weld: Weld,
    px: f64,
    boundary: Option<&crate::boundary::Boundary>,
) -> Color {
    let ds = distances(sources, p);
    let ds = &ds[..sources.len()];
    let t = weld.amount();
    let (mut d, _) = smooth_min(ds, 2.0 * weld.reach * t);
    let edge = boundary.map(|b| b.sample(p));
    if let Some(edge) = edge {
        d = edge.distance;
    }
    // The smooth minimum never exceeds any source distance, so outside its
    // coverage every source is outside too. Avoid sampling invisible brushes.
    if coverage(d, px) == 0.0 {
        return Color::TRANSPARENT;
    }
    let (_, weights) = smooth_min(ds, weld.blend * t);
    let (mut original, mut old_fill) = (Premul::default(), Premul::default());
    for (s, di) in sources.iter().zip(ds) {
        let a = coverage(*di, px);
        let fill = if weld.fill == Channel::Omit {
            Color::TRANSPARENT
        } else {
            colour(&s.fill, p)
        };
        let border = if weld.border == Channel::Omit {
            Color::TRANSPARENT
        } else {
            colour(&s.border, p)
        };
        let ring = (a - coverage(*di + s.width, px)).max(0.0);
        let fraction = if a > 0.0 { ring / a } else { 0.0 };
        let decorated = border.premul().scale(fraction).over(fill.premul()).scale(a);
        original = decorated.over(original);
        old_fill = fill.premul().scale(a).over(old_fill);
    }
    if t == 0.0 {
        return original.straight();
    }
    let a = coverage(d, px);
    let mut result = match weld.fill {
        Channel::Blend => Color::weighted(
            sources
                .iter()
                .enumerate()
                .map(|(i, s)| (colour(&s.fill, p), weights[i])),
        )
        .premul()
        .scale(a),
        Channel::Keep => old_fill,
        Channel::Omit => Premul::default(),
    };
    match weld.border {
        Channel::Blend => {
            let bw = if let Some(edge) = edge {
                let ds = distances(sources, edge.point);
                let ds: Vec<_> = ds[..sources.len()].iter().map(|d| d.abs()).collect();
                smooth_min(&ds, weld.blend * t).1
            } else {
                weights
            };
            let width = sources
                .iter()
                .enumerate()
                .map(|(i, s)| bw[i] * if s.border.is_some() { s.width } else { 0.0 })
                .sum::<f64>();
            let border = Color::weighted(
                sources
                    .iter()
                    .enumerate()
                    .map(|(i, s)| (colour(&s.border, p), bw[i])),
            );
            let ring = (a - coverage(d + width, px)).max(0.0);
            // Resolve coverage once. Source-over of two separately AA'd opaque
            // layers would turn 50% edge coverage into 75% and make dark fringes.
            let ratio = if a > 0.0 { ring / a } else { 0.0 };
            result = Premul(std::array::from_fn(|j| {
                result.0[j] * (1.0 - border.0[3] * ratio) + border.premul().0[j] * ring
            }));
        }
        Channel::Keep => {
            for (s, di) in sources.iter().zip(ds) {
                let c = colour(&s.border, p);
                let ring = (coverage(*di, px) - coverage(*di + s.width, px)).max(0.0);
                let ratio = if a > 0.0 { (ring / a).min(1.0) } else { 0.0 };
                result = Premul(std::array::from_fn(|j| {
                    result.0[j] * (1.0 - c.0[3] * ratio) + c.premul().0[j] * ring
                }));
            }
        }
        Channel::Omit => {}
    }
    // Only NEW geometric coverage bypasses the temporal material blend. Using
    // `1 - original_alpha` here jumps at t=0 where independent AA edges overlap:
    // two 50% edges composite to 75%, whereas their union covers 50%.
    // This ratio tends to zero with geometric growth and is one on a new bridge.
    let base = coverage(ds.iter().copied().fold(f64::INFINITY, f64::min), px);
    let added = if a > 0.0 {
        ((a - base) / a).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mix = t + (1.0 - t) * added;
    original.mix(result, mix).straight()
}
