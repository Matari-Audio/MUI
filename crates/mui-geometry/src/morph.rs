//! One outline turned into another: a play glyph into a pause, a pill into a
//! circle, a tab into the panel it opens.
//!
//! Both paths are flattened, their contours paired (exteriors with
//! exteriors, holes with holes, largest first), each pair resampled to the
//! same number of points by arc length, the second rotated to the start
//! that sits closest to the first, and the two lerped. A contour with no
//! partner shrinks into its own centroid, so a two-bar pause can come out of
//! a one-triangle play and go back again. The ends are exact: `t <= 0` is
//! `a` and `t >= 1` is `b`, curves and all; only the frames in between are
//! polygons.
use crate::{Error, Path, Point, Vec2};

/// Chord error the in-between frames are flattened at, in logical units.
const TOLERANCE: f64 = 0.05;
/// Points per paired contour: about one per logical unit of the longer
/// perimeter, so a camera punch-in to 4x still sees ~4 px chords.
const MIN_POINTS: usize = 32;
const MAX_POINTS: usize = 512;

/// `a` at `t = 0`, `b` at `t = 1`, a polygon between. `t` outside `0..=1`
/// clamps: a spring overshooting past the target does not extrapolate the
/// shape inside out.
///
/// ```
/// use mui_geometry::{morph, Path, Point};
/// let square = Path::polyline([(0., 0.), (10., 0.), (10., 10.), (0., 10.)].map(|(x, y)| Point::new(x, y)), true);
/// let wide = Path::polyline([(0., 0.), (30., 0.), (30., 10.), (0., 10.)].map(|(x, y)| Point::new(x, y)), true);
/// assert_eq!(morph(&square, &wide, 0.).unwrap(), square);
/// assert_eq!(morph(&square, &wide, 1.).unwrap(), wide);
/// let mid = morph(&square, &wide, 0.5).unwrap().flatten(0.1, 4096).unwrap();
/// let right = mid[0].iter().map(|p| p.x).fold(f64::MIN, f64::max);
/// assert!((right - 20.).abs() < 1e-6);
/// ```
pub fn morph(a: &Path, b: &Path, t: f64) -> Result<Path, Error> {
    if !t.is_finite() {
        return Err(Error::NonFinite);
    }
    if t <= 0. {
        return Ok(a.clone());
    }
    if t >= 1. {
        return Ok(b.clone());
    }
    let (from, to) = (contours(a)?, contours(b)?);
    let mut out = Path::default();
    for exterior in [true, false] {
        let pick = |c: &[Ring]| -> Vec<Ring> {
            c.iter()
                .filter(|r| (r.area >= 0.) == exterior)
                .cloned()
                .collect()
        };
        let (f, g) = (pick(&from), pick(&to));
        for i in 0..f.len().max(g.len()) {
            // No partner: collapse into the partner-less contour's own centroid.
            let (p, q) = match (f.get(i), g.get(i)) {
                (Some(p), Some(q)) => (p.clone(), q.clone()),
                (Some(p), None) => (p.clone(), p.point()),
                (None, Some(q)) => (q.point(), q.clone()),
                (None, None) => unreachable!("i is below the longer list's length"),
            };
            let n = ((p.perimeter.max(q.perimeter)).ceil() as usize).clamp(MIN_POINTS, MAX_POINTS);
            let ps = resample(&p.points, n);
            let mut qs = resample(&q.points, n);
            if p.area * q.area < 0. {
                qs.reverse();
            }
            let shift = best_shift(&ps, &qs);
            let ring = (0..n).map(|k| ps[k] + (qs[(k + shift) % n] - ps[k]) * t);
            out.commands.extend(Path::polyline(ring, true).commands);
        }
    }
    Ok(out)
}

#[derive(Clone)]
struct Ring {
    points: Vec<Point>,
    /// Signed shoelace area: its sign is the winding, so a hole pairs with a hole.
    area: f64,
    perimeter: f64,
}
impl Ring {
    fn new(points: Vec<Point>) -> Self {
        let n = points.len();
        let (mut area, mut perimeter) = (0., 0.);
        for i in 0..n {
            let (p, q) = (points[i], points[(i + 1) % n]);
            area += p.to_vec2().cross(q.to_vec2()) * 0.5;
            perimeter += p.distance(q);
        }
        Self {
            points,
            area,
            perimeter,
        }
    }
    /// This ring shrunk to its vertex centroid, keeping its winding.
    fn point(&self) -> Self {
        let c = (self.points.iter().map(|p| p.to_vec2()).sum::<Vec2>() / self.points.len() as f64)
            .to_point();
        Self {
            points: vec![c],
            area: self.area.signum() * f64::MIN_POSITIVE,
            perimeter: 0.,
        }
    }
}

/// Closed contours, largest first, with degenerate slivers dropped.
fn contours(p: &Path) -> Result<Vec<Ring>, Error> {
    let mut rings: Vec<Ring> = p
        .flatten(TOLERANCE, 1 << 16)?
        .into_iter()
        .map(|mut c| {
            if c.len() > 1 && c.first() == c.last() {
                c.pop();
            }
            Ring::new(c)
        })
        .filter(|r| r.points.len() >= 3 && r.area.abs() > 1e-9)
        .collect();
    rings.sort_by(|a, b| b.area.abs().total_cmp(&a.area.abs()));
    // Exterior is whatever way the largest contour winds, so two paths
    // authored in opposite directions still pair outline with outline.
    if rings.first().is_some_and(|r| r.area < 0.) {
        for r in &mut rings {
            r.area = -r.area;
            r.points.reverse();
        }
    }
    Ok(rings)
}

/// `n` points evenly spaced by arc length around the closed ring `c`.
fn resample(c: &[Point], n: usize) -> Vec<Point> {
    if c.len() == 1 {
        return vec![c[0]; n];
    }
    let m = c.len();
    let total: f64 = (0..m).map(|i| c[i].distance(c[(i + 1) % m])).sum();
    let step = total / n as f64;
    let mut out = Vec::with_capacity(n);
    let (mut seg, mut walked) = (0, 0.);
    for k in 0..n {
        let want = k as f64 * step;
        loop {
            let len = c[seg].distance(c[(seg + 1) % m]);
            if walked + len >= want || seg + 1 == m {
                let f = if len > 0. {
                    ((want - walked) / len).clamp(0., 1.)
                } else {
                    0.
                };
                out.push(c[seg] + (c[(seg + 1) % m] - c[seg]) * f);
                break;
            }
            walked += len;
            seg += 1;
        }
    }
    out
}

/// The rotation of `q` whose points sit closest to `p`'s, by summed squared
/// distance, so the in-between frames do not twist.
///
/// ponytail: O(n^2) per call and recomputed every animating frame; at 512
/// points that is 262k mults for the few nodes morphing at once. Cache the
/// shift per (from, to) pair if a morph-heavy scene ever shows it.
fn best_shift(p: &[Point], q: &[Point]) -> usize {
    let n = p.len();
    let cost = |s: usize| -> f64 {
        (0..n)
            .map(|k| {
                let d = p[k] - q[(k + s) % n];
                d.dot(d)
            })
            .sum()
    };
    (0..n)
        .map(|s| (cost(s), s))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map_or(0, |(_, s)| s)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn poly(p: &[(f64, f64)]) -> Path {
        Path::polyline(p.iter().map(|&(x, y)| Point::new(x, y)), true)
    }

    #[test]
    fn one_triangle_becomes_two_bars_and_back() {
        let play = poly(&[(0., 0.), (10., 5.), (0., 10.)]);
        let pause = Path {
            commands: [
                poly(&[(0., 0.), (3., 0.), (3., 10.), (0., 10.)]).commands,
                poly(&[(7., 0.), (10., 0.), (10., 10.), (7., 10.)]).commands,
            ]
            .concat(),
        };
        for t in [0.25, 0.5, 0.75] {
            let m = morph(&play, &pause, t)
                .unwrap()
                .flatten(0.1, 1 << 16)
                .unwrap();
            assert_eq!(m.len(), 2, "the second bar grows out of a point at t={t}");
            assert!(m.iter().flatten().all(|p| p.is_finite()));
        }
        // Near the start the extra bar is almost a point.
        let early = morph(&play, &pause, 0.01)
            .unwrap()
            .flatten(0.1, 1 << 16)
            .unwrap();
        let span = |c: &Vec<Point>| {
            let xs = c.iter().map(|p| p.x);
            xs.clone().fold(f64::MIN, f64::max) - xs.fold(f64::MAX, f64::min)
        };
        assert!(span(&early[1]) < 0.2, "{}", span(&early[1]));
    }

    #[test]
    fn a_reversed_winding_does_not_turn_the_shape_inside_out() {
        let cw = poly(&[(0., 0.), (0., 10.), (10., 10.), (10., 0.)]);
        let ccw = poly(&[(0., 0.), (10., 0.), (10., 10.), (0., 10.)]);
        let m = morph(&ccw, &cw, 0.5)
            .unwrap()
            .flatten(0.1, 1 << 16)
            .unwrap();
        let area = Ring::new(m[0].clone()).area.abs();
        assert!(
            (area - 100.).abs() < 1.,
            "same square either way round: {area}"
        );
    }

    #[test]
    fn the_same_inputs_give_the_same_frame() {
        let a = poly(&[(0., 0.), (10., 0.), (5., 8.)]);
        let b = poly(&[(0., 0.), (20., 0.), (20., 20.), (0., 20.)]);
        assert_eq!(morph(&a, &b, 0.3).unwrap(), morph(&a, &b, 0.3).unwrap());
        assert!(morph(&a, &b, f64::NAN).is_err());
    }
}
