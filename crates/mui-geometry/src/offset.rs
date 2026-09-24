//! Parallel offsets of FINAL rendered boundaries.
use crate::boolean::{topology, BackendMulti};
use crate::{Error, GeometryOptions, Path, PathCommand, Point, Topology};
use i_overlay::{
    core::{fill_rule::FillRule, overlay_rule::OverlayRule},
    float::single::SingleFloatOverlay,
    mesh::{
        outline::offset::OutlineOffset,
        style::{LineJoin, OutlineStyle},
    },
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OffsetOptions {
    /// Chord tolerance while flattening circular input arcs.
    pub flatten_tolerance: f64,
    /// Precision grid is 1/integer_scale for the fixed-scale offset backend.
    pub integer_scale: f64,
    pub max_points: usize,
}
impl Default for OffsetOptions {
    fn default() -> Self {
        Self {
            flatten_tolerance: 0.05,
            integer_scale: 1_048_576.0,
            max_points: 16_384,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OffsetShape {
    pub topology: Topology,
    pub path: Path,
    pub source_components: usize,
    pub source_holes: usize,
    pub counts_changed: bool,
    /// Positive = outward dilation; negative = inward erosion.
    pub distance: f64,
}
pub type InsetShape = OffsetShape;

impl Topology {
    pub fn to_path(&self) -> Path {
        let mut p = Path::default();
        for r in &self.rings {
            if let Some(&first) = r.points.first() {
                p.commands.push(PathCommand::MoveTo(first));
                p.commands
                    .extend(r.points[1..].iter().copied().map(PathCommand::LineTo));
                p.commands.push(PathCommand::Close);
            }
        }
        p
    }
}

pub(crate) fn validate(distance: f64, o: OffsetOptions) -> Result<(), Error> {
    if !distance.is_finite() || !o.integer_scale.is_finite() {
        return Err(Error::NonFinite);
    }
    if distance.abs() > 10_000.0 {
        return Err(Error::InvalidOptions("offset magnitude must be <=10000"));
    }
    if !o.flatten_tolerance.is_finite() || o.flatten_tolerance <= 0.0 {
        return Err(Error::InvalidOptions("flatten_tolerance"));
    }
    if o.integer_scale < 1.0 || o.integer_scale > 1e9 || o.max_points < 3 || o.max_points > 100_000
    {
        return Err(Error::InvalidOptions("offset scale or point budget"));
    }
    if distance != 0.0 && distance.abs() * (1.0 - (0.032_f64 / 2.0).cos()) > o.flatten_tolerance {
        return Err(Error::InvalidOptions(
            "offset tolerance is below backend join resolution",
        ));
    }
    Ok(())
}

/// Signed parallel offset of the filled set. Positive expands outward; negative
/// erodes inward. Topological changes (split/disappear/hole changes) are valid results.
pub fn offset_path(path: &Path, distance: f64, o: OffsetOptions) -> Result<OffsetShape, Error> {
    validate(distance, o)?;
    let rings = path.flatten(o.flatten_tolerance, o.max_points)?;
    let input: Vec<Vec<[f64; 2]>> = rings
        .iter()
        .map(|r| r.iter().map(|p| [p.x, p.y]).collect())
        .collect();
    if input
        .iter()
        .flatten()
        .any(|p| p[0].abs() > 1e6 || p[1].abs() > 1e6)
    {
        return Err(Error::CoordinateLimit);
    }
    // NonZero, like the renderer, hit testing and text: overlapping subpaths
    // are one painted region, not a hole.
    let normalized: BackendMulti =
        input.overlay_as::<i64>(&input, OverlayRule::Union, FillRule::NonZero);
    let source_components = normalized.len();
    let source_holes = normalized
        .iter()
        .map(|s| s.len().saturating_sub(1))
        .sum::<usize>();
    let raw = if distance == 0.0 || normalized.is_empty() {
        normalized
    } else {
        let d = distance.abs();
        let angle =
            (4.0 * (o.flatten_tolerance / (2.0 * d)).min(1.0).sqrt().asin()).clamp(0.032, 0.25);
        let style = OutlineStyle::new(distance).line_join(LineJoin::Round(angle));
        normalized
            .outline_fixed_scale_as::<i64>(&style, o.integer_scale)
            .map_err(|_| Error::InvalidOptions("offset fixed-scale conversion failed"))?
    };
    let g = GeometryOptions {
        max_vertices: o.max_points,
        coordinate_limit: 1e6,
        epsilon: (0.25 / o.integer_scale).min(1e-7),
    };
    let t = topology(raw, g)?;
    let holes = t.rings().len().saturating_sub(t.components());
    let counts_changed = t.components() != source_components || holes != source_holes;
    let p = t.to_path();
    Ok(OffsetShape {
        topology: t,
        path: p,
        source_components,
        source_holes,
        counts_changed,
        distance,
    })
}

pub fn inset_path(path: &Path, distance: f64, o: OffsetOptions) -> Result<InsetShape, Error> {
    if !distance.is_finite() {
        return Err(Error::NonFinite);
    }
    if distance < 0.0 {
        return Err(Error::InvalidOptions("inset must be nonnegative"));
    }
    offset_path(path, -distance, o)
}

pub fn outset_path(path: &Path, distance: f64, o: OffsetOptions) -> Result<OffsetShape, Error> {
    if !distance.is_finite() {
        return Err(Error::NonFinite);
    }
    if distance < 0.0 {
        return Err(Error::InvalidOptions("outset must be nonnegative"));
    }
    offset_path(path, distance, o)
}

pub fn boundary_distance(p: Point, contours: &[Vec<Point>]) -> f64 {
    contours
        .iter()
        .flat_map(|r| {
            (0..r.len())
                .map(move |i| crate::math::point_segment_distance(p, r[i], r[(i + 1) % r.len()]))
        })
        .fold(f64::INFINITY, f64::min)
}
