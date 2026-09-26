use crate::Error;
pub use kurbo::{Affine, Point, Rect, Vec2};

/// The box around `points`, or `None` for none. kurbo's `Rect::from_points`
/// takes two corners; this takes a ring.
pub fn bounds(points: impl IntoIterator<Item = Point>) -> Option<Rect> {
    let mut it = points.into_iter();
    let first = it.next()?;
    Some(it.fold(Rect::from_points(first, first), |r, p| r.union_pt(p)))
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
    point_segment_distance2(p, a, b).sqrt()
}
/// Squared: the comparisons in the O(V²) passes skip the root.
pub(crate) fn point_segment_distance2(p: Point, a: Point, b: Point) -> f64 {
    let v = b - a;
    let n = v.dot(v);
    let t = if n == 0. {
        0.
    } else {
        ((p - a).dot(v) / n).clamp(0., 1.)
    };
    let d = p - (a + v * t);
    d.dot(d)
}

/// Remove duplicate/collinear vertices without deleting an entire short edge twice.
pub(crate) fn clean_ring(input: &[Point], epsilon: f64) -> Result<Vec<Point>, Error> {
    if input.iter().any(|p| !p.is_finite()) {
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
    // A vertex goes when it sits on its predecessor or on the chord to its
    // successor (without doubling back). One stack pass settles the interior;
    // the seam loop then settles the wrap-around, where only the two ends
    // gain new neighbours.
    let removable = |a: Point, b: Point, c: Point| {
        b.distance(a) <= epsilon
            || (point_segment_distance(b, a, c) <= epsilon && (b - a).dot(c - b) >= 0.)
    };
    let mut out: Vec<Point> = Vec::with_capacity(p.len());
    for v in p {
        while out.len() >= 2 && removable(out[out.len() - 2], out[out.len() - 1], v) {
            out.pop();
        }
        out.push(v);
    }
    let mut start = 0;
    loop {
        if out.len() - start < 3 {
            return Err(Error::DegenerateRing);
        }
        let last = out.len() - 1;
        if removable(out[last - 1], out[last], out[start]) {
            out.pop();
        } else if removable(out[last], out[start], out[start + 1]) {
            start += 1;
        } else {
            break;
        }
    }
    out.drain(..start);
    let p = out;
    if signed_area(&p).abs() <= epsilon * epsilon {
        return Err(Error::DegenerateRing);
    }
    Ok(p)
}

/// Validation for caller-supplied polygon rings. The Boolean backend handles
/// intersections BETWEEN valid shapes, not malformed leaf rings.
///
/// Sweep and prune: edges sorted by their left end, and only pairs whose
/// eps-grown boxes overlap get the exact test. A ring's edges mostly touch
/// only their neighbours, so this is O(V log V) plus the near pairs.
pub(crate) fn validate_simple(p: &[Point], eps: f64) -> Result<(), Error> {
    let n = p.len();
    let eps2 = eps * eps;
    let mut boxes: Vec<(f64, f64, f64, f64, usize)> = (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            (a.x.min(b.x), a.x.max(b.x), a.y.min(b.y), a.y.max(b.y), i)
        })
        .collect();
    boxes.sort_unstable_by(|l, r| l.0.total_cmp(&r.0));
    for i in 0..n {
        let a = p[i];
        let b = p[(i + 1) % n];
        let c = p[(i + 2) % n];
        if (b - a).cross(c - b).abs() <= eps * (b - a).length().max(1.0) && (b - a).dot(c - b) < 0.0
        {
            return Err(Error::SelfIntersection);
        }
    }
    for (k, &(_, x1, y0, y1, i)) in boxes.iter().enumerate() {
        let (a, b) = (p[i], p[(i + 1) % n]);
        for &(ox0, _, oy0, oy1, j) in &boxes[k + 1..] {
            if ox0 > x1 + eps {
                break;
            }
            if oy0 > y1 + eps || y0 > oy1 + eps || j == (i + 1) % n || i == (j + 1) % n {
                continue;
            }
            let (c, d) = (p[j], p[(j + 1) % n]);
            if point_segment_distance2(a, c, d) <= eps2
                || point_segment_distance2(b, c, d) <= eps2
                || point_segment_distance2(c, a, b) <= eps2
                || point_segment_distance2(d, a, b) <= eps2
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
