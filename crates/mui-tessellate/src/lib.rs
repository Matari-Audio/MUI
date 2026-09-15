//! Optional triangulation adapter, still independent of any UI/GPU framework.
#![forbid(unsafe_code)]
use lyon_tessellation::path::{math::point, Path};
use lyon_tessellation::{BuffersBuilder, FillOptions, FillTessellator, FillVertex, VertexBuffers};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct TriangleMesh {
    pub positions: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// Retained for antialiased strokes, debug display and application hit tests.
    pub contours: Vec<Vec<[f32; 2]>>,
}
#[derive(Debug)]
pub enum Error {
    Geometry(mui_geometry::Error),
    Tessellation(String),
    CoordinateOverflow,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Geometry(e) => write!(f, "{e}"),
            Self::Tessellation(e) => write!(f, "tessellation failed: {e}"),
            Self::CoordinateOverflow => f.write_str("coordinates cannot be represented as f32"),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Geometry(error) => Some(error),
            Self::Tessellation(_) | Self::CoordinateOverflow => None,
        }
    }
}
impl From<mui_geometry::Error> for Error {
    fn from(e: mui_geometry::Error) -> Self {
        Self::Geometry(e)
    }
}

/// Reuse this scratch tessellator between shapes. Tessellate only when the path
/// or display-space quality changes; color changes do not invalidate geometry.
pub struct Tessellator {
    fill: FillTessellator,
}
impl Default for Tessellator {
    fn default() -> Self {
        Self {
            fill: FillTessellator::new(),
        }
    }
}
impl Tessellator {
    pub fn tessellate(
        &mut self,
        path: &mui_geometry::Path,
        tolerance: f64,
    ) -> Result<TriangleMesh, Error> {
        let flattened = path.flatten(tolerance, 250_000)?;
        let mut contours = Vec::with_capacity(flattened.len());
        let mut builder = Path::builder();
        for ring in flattened {
            let points: Vec<[f32; 2]> =
                ring.into_iter().map(|p| [p.x as f32, p.y as f32]).collect();
            if points
                .iter()
                .any(|p| !p[0].is_finite() || !p[1].is_finite())
            {
                return Err(Error::CoordinateOverflow);
            }
            if let Some(first) = points.first() {
                builder.begin(point(first[0], first[1]));
                for p in &points[1..] {
                    builder.line_to(point(p[0], p[1]));
                }
                builder.end(true);
            }
            contours.push(points);
        }
        let path = builder.build();
        let mut buffers: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
        self.fill
            .tessellate_path(
                &path,
                // Non-zero, matching the renderer. `boolean::topology` winds
                // every exterior ring positive and every hole negative, so the
                // two rules agree on anything this crate is handed -- and
                // non-zero also survives geometry nobody normalised.
                &FillOptions::non_zero(),
                &mut BuffersBuilder::new(&mut buffers, |v: FillVertex<'_>| {
                    [v.position().x, v.position().y]
                }),
            )
            .map_err(|e| Error::Tessellation(format!("{e:?}")))?;
        Ok(TriangleMesh {
            positions: buffers.vertices,
            indices: buffers.indices,
            contours,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concave_union_tessellates() {
        use mui_geometry::*;
        let shapes = [
            Polygon::rectangle(0., 50., 200., 100.).unwrap().into(),
            Polygon::rectangle(0., 0., 60., 50.).unwrap().into(),
        ];
        let t = union(&shapes, Default::default()).unwrap();
        let r = fillet(&t, Default::default()).unwrap();
        let m = Tessellator::default().tessellate(&r.path, 0.2).unwrap();
        assert!(!m.indices.is_empty());
        assert_eq!(m.indices.len() % 3, 0);
        assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()));
    }
    #[test]
    fn hole_is_not_filled() {
        use mui_geometry::*;
        let p = Polygon::rectangle(0., 0., 100., 100.)
            .unwrap()
            .with_hole(Polygon::rectangle(25., 25., 50., 50.).unwrap().exterior);
        let t = union(&[p.into()], Default::default()).unwrap();
        let r = fillet(
            &t,
            CornerStyle {
                convex_radius: 0.,
                concave_radius: 0.,
                ..Default::default()
            },
        )
        .unwrap();
        let m = Tessellator::default().tessellate(&r.path, 0.2).unwrap();
        let area: f64 = m
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|i| {
                let a = m.positions[i[0] as usize];
                let b = m.positions[i[1] as usize];
                let c = m.positions[i[2] as usize];
                ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() as f64 * 0.5
            })
            .sum();
        assert!((area - 7500.).abs() < 0.1);
    }

    #[test]
    fn geometry_source_is_exposed() {
        let error = Error::Geometry(mui_geometry::Error::NonFinite);
        let source = std::error::Error::source(&error).expect("geometry source");
        assert!(source.downcast_ref::<mui_geometry::Error>().is_some());
        assert!(std::error::Error::source(source).is_none());
    }
}
