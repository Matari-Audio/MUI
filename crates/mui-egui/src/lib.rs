//! Thin egui adapter; mui-geometry and mui-layout never import egui.
//! Paint into a real egui backend (including its texture/clipping support).
//! Arbitrary mesh fills require backend MSAA/coverage AA; the stroke is not a
//! substitute for fill coverage. No GPU/window/parameter ownership lives here.
#![forbid(unsafe_code)]
use egui::{Color32, Mesh, Painter, Pos2, Shape, Stroke, Vec2};
use mui_geometry::{CornerStyle, GeometryOptions, OffsetOptions, Path, PlacedShape, Topology};
use mui_tessellate::{Tessellator, TriangleMesh};

#[derive(Debug)]
pub enum Error {
    Geometry(mui_geometry::Error),
    Tessellation(mui_tessellate::Error),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Geometry(e) => write!(f, "{e}"),
            Self::Tessellation(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<mui_geometry::Error> for Error {
    fn from(e: mui_geometry::Error) -> Self {
        Self::Geometry(e)
    }
}
impl From<mui_tessellate::Error> for Error {
    fn from(e: mui_tessellate::Error) -> Self {
        Self::Tessellation(e)
    }
}

#[derive(Debug, Clone)]
pub struct PreparedSurface {
    pub outer: TriangleMesh,
    pub inner: TriangleMesh,
    pub band: TriangleMesh,
    pub inner_topology: Topology,
    pub counts_changed: bool,
}

/// Prepare all topology, fillets, offsets, and meshes before exposing any result.
/// Band uses EVEN-ODD outer minus inset, so translucent borders do not double-fill.
pub fn prepare(
    inputs: &[PlacedShape],
    corners: CornerStyle,
    inset: f64,
    quality: OffsetOptions,
) -> Result<PreparedSurface, Error> {
    let raw = mui_geometry::union(inputs, GeometryOptions::default())?;
    let rounded = mui_geometry::fillet(&raw, corners)?;
    let inner = mui_geometry::inset_path(&rounded.path, inset, quality)?;
    let band = Path {
        commands: rounded
            .path
            .commands
            .iter()
            .chain(inner.path.commands.iter())
            .copied()
            .collect(),
    };
    let mut tess = Tessellator::default();
    Ok(PreparedSurface {
        outer: tess.tessellate(&rounded.path, quality.flatten_tolerance)?,
        inner: tess.tessellate(&inner.path, quality.flatten_tolerance)?,
        band: tess.tessellate(&band, quality.flatten_tolerance)?,
        inner_topology: inner.topology,
        counts_changed: inner.counts_changed,
    })
}

/// UI-thread transactional snapshot. No mutex, unsafe global, or audio ownership.
/// Keep this per plugin instance. Commit on geometry/style changes, not per frame.
#[derive(Debug, Default)]
pub struct SurfaceState {
    current: Option<PreparedSurface>,
    revision: u64,
}
impl SurfaceState {
    pub fn current(&self) -> Option<&PreparedSurface> {
        self.current.as_ref()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn commit(
        &mut self,
        inputs: &[PlacedShape],
        corners: CornerStyle,
        inset: f64,
        quality: OffsetOptions,
    ) -> Result<(), Error> {
        let next = prepare(inputs, corners, inset, quality)?;
        let rev = self
            .revision
            .checked_add(1)
            .ok_or(mui_geometry::Error::InvalidOptions("revision exhausted"))?;
        self.current = Some(next);
        self.revision = rev;
        Ok(())
    }
}

/// Translation is presentation-only. Scale the geometry/tolerance explicitly for
/// arbitrary/nonuniform transforms rather than accidentally stretching radii.
pub fn paint(painter: &Painter, mesh: &TriangleMesh, origin: Pos2, fill: Color32, stroke: Stroke) {
    let vertices = mesh
        .positions
        .iter()
        .map(|p| egui::epaint::Vertex {
            pos: origin + Vec2::new(p[0], p[1]),
            uv: egui::epaint::WHITE_UV,
            color: fill,
        })
        .collect();
    let m = Mesh {
        vertices,
        indices: mesh.indices.clone(),
        ..Mesh::default()
    };
    if !m.indices.is_empty() {
        painter.add(Shape::mesh(m));
    }
    if stroke.width > 0.0 {
        for ring in &mesh.contours {
            painter.add(Shape::closed_line(
                ring.iter()
                    .map(|p| origin + Vec2::new(p[0], p[1]))
                    .collect(),
                stroke,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn shapes() -> [PlacedShape; 2] {
        [
            mui_geometry::Polygon::rectangle(0., 100., 300., 150.)
                .unwrap()
                .into(),
            mui_geometry::Polygon::rectangle(0., 0., 80., 110.)
                .unwrap()
                .into(),
        ]
    }
    fn area(m: &TriangleMesh) -> f64 {
        m.indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| {
                let a = m.positions[t[0] as usize];
                let b = m.positions[t[1] as usize];
                let c = m.positions[t[2] as usize];
                ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() as f64 * 0.5
            })
            .sum()
    }
    #[test]
    fn border_mesh_has_no_double_fill() {
        let p = prepare(
            &shapes(),
            CornerStyle::default(),
            6.,
            OffsetOptions::default(),
        )
        .unwrap();
        assert!((area(&p.band) + area(&p.inner) - area(&p.outer)).abs() < 0.1);
    }
    #[test]
    fn failed_surface_commit_keeps_previous() {
        let mut s = SurfaceState::default();
        s.commit(
            &shapes(),
            CornerStyle::default(),
            6.,
            OffsetOptions::default(),
        )
        .unwrap();
        let a = area(&s.current().unwrap().outer);
        assert!(s
            .commit(
                &shapes(),
                CornerStyle::default(),
                f64::NAN,
                OffsetOptions::default()
            )
            .is_err());
        assert_eq!(s.revision(), 1);
        assert_eq!(area(&s.current().unwrap().outer), a);
    }
    #[test]
    fn instances_do_not_share_state() {
        let a = SurfaceState::default();
        let mut b = SurfaceState::default();
        b.commit(
            &shapes(),
            CornerStyle::default(),
            6.,
            OffsetOptions::default(),
        )
        .unwrap();
        assert!(a.current().is_none());
    }
}

/// Small UI-thread path cache for adapters. Geometry/layout changes should
/// invalidate upstream; paint-only changes reuse the same mesh.
#[derive(Debug, Default)]
pub struct PathMeshCache {
    path: Option<Path>,
    tolerance: f64,
    mesh: Option<TriangleMesh>,
    revision: u64,
}
impl PathMeshCache {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn mesh(&self) -> Option<&TriangleMesh> {
        self.mesh.as_ref()
    }
    pub fn prepare(&mut self, path: &Path, tolerance: f64) -> Result<&TriangleMesh, Error> {
        if !tolerance.is_finite() || tolerance <= 0.0 {
            return Err(mui_geometry::Error::InvalidOptions("tessellation tolerance").into());
        }
        let same = self.path.as_ref() == Some(path)
            && self.mesh.is_some()
            && (self.tolerance - tolerance).abs() <= f64::EPSILON;
        if !same {
            let next = Tessellator::default().tessellate(path, tolerance)?;
            let rev = self
                .revision
                .checked_add(1)
                .ok_or(mui_geometry::Error::InvalidOptions("revision exhausted"))?;
            self.path = Some(path.clone());
            self.tolerance = tolerance;
            self.mesh = Some(next);
            self.revision = rev;
        }
        Ok(self.mesh.as_ref().expect("cache populated"))
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    #[test]
    fn unchanged_path_does_not_retessellate() {
        let path = mui_geometry::RoundedRect::new(
            mui_geometry::Bounds {
                min: mui_geometry::Point::ZERO,
                max: mui_geometry::Point::new(100.0, 80.0),
            },
            12.0,
        )
        .unwrap()
        .path();
        let mut c = PathMeshCache::default();
        c.prepare(&path, 0.2).unwrap();
        let r = c.revision();
        c.prepare(&path, 0.2).unwrap();
        assert_eq!(c.revision(), r);
        c.prepare(&path, 0.1).unwrap();
        assert_eq!(c.revision(), r + 1);
    }
}
