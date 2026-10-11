//! Triangles for the acceleration structures, in each instance's own space.
use mui_geometry::{Path, Point};
use mui_stage::Plane;

/// A bottom-level structure's triangles: position and normal per vertex,
/// indices, and where each of its geometries starts and how many indices
/// it has.
#[derive(Default)]
pub(crate) struct Tris {
    pub vertices: Vec<[f32; 6]>,
    pub indices: Vec<u32>,
    pub parts: Vec<(u32, u32)>,
}
impl Tris {
    fn part(&mut self, vertices: &[[f32; 6]], indices: &[u32]) {
        let base = self.vertices.len() as u32;
        self.parts
            .push((self.indices.len() as u32, indices.len() as u32));
        self.vertices.extend_from_slice(vertices);
        self.indices.extend(indices.iter().map(|i| base + i));
    }
}

/// A slab in its local space (y up, centred, front face at z = 0): the
/// front face (geometry 0), the back (1) and the walls (2), as mui-stage
/// draws it. A card (no depth) is its front face alone.
pub(crate) fn plane(p: &Plane) -> Tris {
    let [w, h] = p.size.map(|v| v * 0.5);
    let quad =
        |z: f32, n: f32| [[-w, -h], [w, -h], [w, h], [-w, h]].map(|[x, y]| [x, y, z, 0., 0., n]);
    let mut t = Tris::default();
    t.part(&quad(0., 1.), &[0, 1, 2, 0, 2, 3]);
    if p.depth > 0. {
        t.part(&quad(-p.depth, -1.), &[0, 2, 1, 0, 3, 2]);
        let walls = walls(p);
        if !walls.is_empty() {
            let idx: Vec<u32> = (0..walls.len() as u32).collect();
            t.part(&walls, &idx);
        }
    }
    t
}

/// The floor: a square `r` from the middle on each side at height `y`.
pub(crate) fn floor(y: f32, r: f32) -> Tris {
    let v = [[-r, -r], [r, -r], [r, r], [-r, r]].map(|[x, z]| [x, y, z, 0., 1., 0.]);
    let mut t = Tris::default();
    t.part(&v, &[0, 2, 1, 0, 3, 2]);
    t
}

/// A mesh as uploaded: one geometry.
pub(crate) fn mesh(vertices: &[[f32; 6]], indices: &[u32]) -> Tris {
    let mut t = Tris::default();
    t.part(vertices, indices);
    t
}

/// Wall quads from the outline, as mui-stage's `wall_mesh` builds them
/// (same flattening, same outward rule), but with normals smoothed across
/// gentle corners: glass bends light by its normal, and a rounded corner
/// cut into flat facets shows every facet.
fn walls(p: &Plane) -> Vec<[f32; 6]> {
    let [w, h] = p.size;
    let rect;
    let outline = if let Some(o) = &p.outline {
        &**o
    } else {
        rect = Path::polyline(
            [(0., 0.), (w, 0.), (w, h), (0., h)]
                .map(|(x, y)| Point::new(f64::from(x), f64::from(y))),
            true,
        );
        &rect
    };
    let Ok(contours) = outline.flatten(0.25, 1 << 14) else {
        return Vec::new();
    };
    let rings: Vec<Vec<[f32; 2]>> = contours
        .iter()
        .map(|c| {
            let mut r: Vec<[f32; 2]> = c
                .iter()
                .map(|q| [q.x as f32 - w * 0.5, h * 0.5 - q.y as f32])
                .collect();
            r.dedup_by(|a, b| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-4);
            while r.len() > 1 && {
                let (a, b) = (r[0], r[r.len() - 1]);
                (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-4
            } {
                r.pop();
            }
            r
        })
        .filter(|r| r.len() >= 3)
        .collect();
    let area = |pts: &[[f32; 2]]| -> f32 {
        let n = pts.len();
        (0..n)
            .map(|i| {
                let (a, b) = (pts[i], pts[(i + 1) % n]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum()
    };
    let out = rings
        .iter()
        .map(|r| area(r))
        .max_by(|a, b| a.abs().total_cmp(&b.abs()))
        .map_or(1., f32::signum);
    // Corners sharper than this keep their crease.
    let smooth = 40f32.to_radians().cos();
    let z = -p.depth;
    let mut v = Vec::new();
    for pts in rings {
        let n = pts.len();
        let normal = |i: usize| {
            let (a, b) = (pts[i % n], pts[(i + 1) % n]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len = dx.hypot(dy);
            [dy / len * out, -dx / len * out]
        };
        // The normal at a corner from the edge on one side, blended with
        // the other edge's when the turn is gentle.
        let at = |edge: [f32; 2], other: [f32; 2]| {
            if edge[0] * other[0] + edge[1] * other[1] < smooth {
                return edge;
            }
            let s = [edge[0] + other[0], edge[1] + other[1]];
            let l = s[0].hypot(s[1]);
            [s[0] / l, s[1] / l]
        };
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            let e = normal(i);
            let na = at(e, normal(i + n - 1));
            let nb = at(e, normal(i + 1));
            let vert = |p: [f32; 2], zz: f32, m: [f32; 2]| [p[0], p[1], zz, m[0], m[1], 0.];
            v.extend_from_slice(&[
                vert(a, 0., na),
                vert(b, 0., nb),
                vert(a, z, na),
                vert(a, z, na),
                vert(b, 0., nb),
                vert(b, z, nb),
            ]);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_slab_has_faces_and_four_flat_walls() {
        let t = plane(&Plane::new("a", 100., 50.).depth(10.));
        assert_eq!(t.parts.len(), 3);
        let walls = &t.vertices[8..];
        assert_eq!(walls.len(), 4 * 6);
        // Square corners keep their crease: every wall normal is an axis.
        for v in walls {
            assert!(
                (v[3].abs() - 1.).abs() < 1e-5 || (v[4].abs() - 1.).abs() < 1e-5,
                "{v:?}"
            );
        }
        // Outward: the right wall's normal points +x.
        assert!(walls.iter().any(|v| v[0] > 49. && v[3] > 0.99));
    }

    #[test]
    fn a_round_slab_has_smooth_wall_normals() {
        let circle = Path::polyline(
            (0..64).map(|i| {
                let a = f64::from(i) / 64. * std::f64::consts::TAU;
                Point::new(50. + 50. * a.cos(), 50. + 50. * a.sin())
            }),
            true,
        );
        let t = plane(
            &Plane::new("a", 100., 100.)
                .depth(10.)
                .outline(std::sync::Arc::new(circle)),
        );
        // On a circle the normal at each vertex is its radius.
        for v in &t.vertices[8..] {
            let r = v[0].hypot(v[1]);
            let dot = (v[0] * v[3] + v[1] * v[4]) / r;
            assert!(dot > 0.999, "{v:?}");
        }
    }
}
