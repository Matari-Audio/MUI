//! Contour regions, variable borders and shape partitions without UI dependencies.
use crate::{
    BooleanOp, Bounds, Error, GeometryOptions, OffsetOptions, Path, PathCommand, Point,
    RoundedRect, boolean, inset_path, offset_path,
};

/// Which side of the authored outline the border occupies. Default preserves MUI.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BorderAlign {
    #[default]
    Inside,
    Center,
    Outside,
}
impl BorderAlign {
    pub fn inward(self) -> f64 {
        match self {
            Self::Inside => 1.,
            Self::Center => 0.5,
            Self::Outside => 0.,
        }
    }
}

/// Width in logical units, interpolated along world-space X and clamped at
/// the endpoints. `start` and `end` are coordinates, not percentages.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WidthProfile {
    pub from: f64,
    pub to: f64,
    pub start: f64,
    pub end: f64,
}
impl WidthProfile {
    pub fn uniform(width: f64) -> Self {
        Self::horizontal(width, width, 0., 1.)
    }
    pub fn horizontal(from: f64, to: f64, start: f64, end: f64) -> Self {
        Self {
            from,
            to,
            start,
            end,
        }
    }
    pub fn validate(self) -> Result<(), Error> {
        if ![self.from, self.to, self.start, self.end]
            .iter()
            .all(|v| v.is_finite())
        {
            return Err(Error::NonFinite);
        }
        if self.from < 0.
            || self.to < 0.
            || self.from.max(self.to) > 10_000.
            || self.start >= self.end
            || !(self.end - self.start).is_finite()
        {
            return Err(Error::InvalidOptions("border widths/interval"));
        }
        Ok(())
    }
    pub fn at(self, x: f64) -> Result<f64, Error> {
        self.validate()?;
        if !x.is_finite() {
            return Err(Error::NonFinite);
        }
        Ok(self.sample(x))
    }
    fn sample(self, x: f64) -> f64 {
        self.from
            + (self.to - self.from) * ((x - self.start) / (self.end - self.start)).clamp(0., 1.)
    }
    fn scaled(self, factor: f64) -> Self {
        Self {
            from: self.from * factor,
            to: self.to * factor,
            ..self
        }
    }
}

/// The border's painted band and the interior it leaves available for content.
#[derive(Clone, Debug, PartialEq)]
pub struct BorderGeometry {
    pub band: Path,
    pub interior: Path,
}

/// Resolve inside, centered or outside borders on any normalized filled contour.
/// Padding is a subsequent `inset_path(&geometry.interior, padding, options)`.
pub fn border_geometry(
    outline: &Path,
    width: WidthProfile,
    align: BorderAlign,
    o: OffsetOptions,
    g: GeometryOptions,
) -> Result<BorderGeometry, Error> {
    let band = border_band(outline, width, align, o, g)?;
    let interior = boolean_paths(outline, &band, BooleanOp::Difference, o, g)?;
    Ok(BorderGeometry { band, interior })
}

/// Just the painted band of [`border_geometry`], one Boolean pass cheaper:
/// a stroke never needs the interior.
pub fn border_band(
    outline: &Path,
    width: WidthProfile,
    align: BorderAlign,
    o: OffsetOptions,
    g: GeometryOptions,
) -> Result<Path, Error> {
    width.validate()?;
    // Uniform borders are the difference of parallel offsets. Sweeping disks
    // around every flattened vertex is only needed for a varying width.
    if width.from == width.to {
        let inward = width.from * align.inward();
        let outward = width.from - inward;
        // Offsets come back normalized: overlay them as they are.
        let inner = offset_path(outline, -inward, o)?.topology;
        let outer = offset_path(outline, outward, o)?.topology;
        return overlay(&outer, &inner, BooleanOp::Difference, g);
    }

    let sweep = union_contours(
        &boundary_band(
            outline,
            width.scaled(if align == BorderAlign::Center {
                0.5
            } else {
                1.
            }),
            o,
        )?,
        o,
        g,
    )?;
    Ok(match align {
        BorderAlign::Inside => boolean_paths(&sweep, outline, BooleanOp::Intersection, o, g)?,
        BorderAlign::Center => sweep,
        BorderAlign::Outside => boolean_paths(&sweep, outline, BooleanOp::Difference, o, g)?,
    })
}

/// Union every contour as a solid piece, as required for overlapping sweep meshes.
/// For shapes with holes use `boolean_paths` instead.
pub fn union_contours(path: &Path, o: OffsetOptions, g: GeometryOptions) -> Result<Path, Error> {
    use i_overlay::{core::fill_rule::FillRule, float::simplify::SimplifyShape};
    crate::offset::validate(0., o)?;
    g.validate()?;
    // A border sweep is hundreds of overlapping quads and disks with a small
    // final contour. Orient every piece alike and merge them in one nonzero
    // pass: pairwise unions re-validate the growing result, O(n^2) per piece.
    let mut rings: Vec<Vec<[f64; 2]>> = Vec::new();
    for mut ring in path.flatten(o.flatten_tolerance, o.max_points)? {
        if ring.len() < 3 {
            continue;
        }
        if ring
            .iter()
            .any(|p| p.x.abs() > g.coordinate_limit || p.y.abs() > g.coordinate_limit)
        {
            return Err(Error::CoordinateLimit);
        }
        if crate::math::signed_area(&ring) < 0. {
            ring.reverse();
        }
        rings.push(ring.into_iter().map(|p| [p.x, p.y]).collect());
    }
    let merged = rings.simplify_shape_as::<i64>(FillRule::NonZero);
    Ok(crate::boolean::topology(merged, g)?.to_path())
}

/// Boolean operation on filled paths. Normalization preserves holes and gives
/// empty paths their ordinary set semantics.
pub fn boolean_paths(
    a: &Path,
    b: &Path,
    op: BooleanOp,
    o: OffsetOptions,
    g: GeometryOptions,
) -> Result<Path, Error> {
    g.validate()?;
    // Normalized filled contours can touch at a point after clipping/offsets.
    // They are not caller-authored simple polygons: preserve that topology
    // instead of routing them back through leaf-polygon validation.
    let a = offset_path(a, 0., o)?.topology;
    let b = offset_path(b, 0., o)?.topology;
    overlay(&a, &b, op, g)
}

/// [`boolean_paths`] on already normalized topologies.
fn overlay(
    a: &crate::Topology,
    b: &crate::Topology,
    op: BooleanOp,
    g: GeometryOptions,
) -> Result<Path, Error> {
    use i_overlay::{core::fill_rule::FillRule, float::single::SingleFloatOverlay};
    g.validate()?;
    let contours = |topology: &crate::Topology| -> Result<Vec<Vec<[f64; 2]>>, Error> {
        if topology.vertex_count() > g.max_vertices {
            return Err(Error::TooManyVertices);
        }
        if topology
            .rings()
            .iter()
            .flat_map(|r| r.points())
            .any(|p| p.x.abs() > g.coordinate_limit || p.y.abs() > g.coordinate_limit)
        {
            return Err(Error::CoordinateLimit);
        }
        Ok(topology
            .rings()
            .iter()
            .map(|r| r.points().iter().map(|p| [p.x, p.y]).collect())
            .collect())
    };
    let result = contours(a)?.overlay_as::<i64>(&contours(b)?, op.rule(), FillRule::EvenOdd);
    Ok(crate::boolean::topology(result, g)?.to_path())
}

/// Sweep both sides of a boundary with a horizontal width profile.
/// The result is a nonzero-filled set of overlapping pieces; use
/// `union_contours` before boolean operations, not even-odd normalization.
pub fn boundary_band(
    outline: &Path,
    width: WidthProfile,
    options: OffsetOptions,
) -> Result<Path, Error> {
    width.validate()?;
    let normalized = offset_path(outline, 0., options)?.path;
    let mut path = Path::default();
    for ring in normalized.flatten(options.flatten_tolerance, options.max_points)? {
        if ring.len() < 2 {
            continue;
        }
        for (a, b) in ring
            .iter()
            .copied()
            .zip(ring.iter().copied().cycle().skip(1))
            .take(ring.len())
        {
            let d = b - a;
            let len = (d.x * d.x + d.y * d.y).sqrt();
            if len < 1e-9 {
                continue;
            }
            let normal = Point::new(-d.y / len, d.x / len);
            // Split at ramp knees, not every pixel: the width is exactly linear
            // on straight edges, regardless of the card's height or scale.
            let mut cuts = vec![0.0, 1.0];
            if d.x.abs() > 1e-9 {
                for x in [width.start, width.end] {
                    let t = (x - a.x) / d.x;
                    if t > 0.0 && t < 1.0 {
                        cuts.push(t);
                    }
                }
            }
            cuts.sort_by(f64::total_cmp);
            for ts in cuts.windows(2) {
                let p = a + d * ts[0];
                let q = a + d * ts[1];
                let wp = width.sample(p.x);
                let wq = width.sample(q.x);
                if wp.max(wq) == 0.0 {
                    continue;
                }
                let points = [
                    p - normal * wp,
                    q - normal * wq,
                    q + normal * wq,
                    p + normal * wp,
                ];
                path.commands.push(PathCommand::MoveTo(points[0]));
                path.commands
                    .extend(points[1..].iter().copied().map(PathCommand::LineTo));
                path.commands.push(PathCommand::Close);
                // Round joins, clipped on their exterior half. The shared outer
                // contour remains exact; only the inner border edge varies.
                for (p, w) in [(p, wp), (q, wq)] {
                    if w > 0.0 {
                        let disk = RoundedRect::new(
                            Bounds {
                                min: Point::new(p.x - w, p.y - w),
                                max: Point::new(p.x + w, p.y + w),
                            },
                            w,
                        )?;
                        path.commands.extend(disk.path().commands);
                    }
                }
            }
            if path.commands.len() > options.max_points {
                return Err(Error::TooManySegments);
            }
        }
    }
    Ok(path)
}

/// Axis along which sibling shares are measured (X means left/right).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitAxis {
    X,
    Y,
}

/// A divider in a shape's bounding box. The cut is a fraction of the selected
/// axis, the bend is a signed fraction of the same extent, and gap is the total
/// normal distance between the two resulting regions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeSplit {
    pub axis: SplitAxis,
    pub at: f64,
    pub gap: f64,
    pub bend: f64,
}
impl ShapeSplit {
    pub fn new(axis: SplitAxis, at: f64) -> Self {
        Self {
            axis,
            at,
            gap: 0.,
            bend: 0.,
        }
    }
    pub fn gap(mut self, gap: f64) -> Self {
        self.gap = gap;
        self
    }
    pub fn bend(mut self, bend: f64) -> Self {
        self.bend = bend;
        self
    }
    fn validate(self) -> Result<(), Error> {
        if ![self.at, self.gap, self.bend].iter().all(|v| v.is_finite()) {
            return Err(Error::NonFinite);
        }
        if !(0. ..=1.).contains(&self.at) || self.gap < 0. || self.bend.abs() > 0.45 {
            return Err(Error::InvalidOptions("shape split fraction/gap/bend"));
        }
        Ok(())
    }
    /// Clip an arbitrary (possibly disconnected or holed) shape into two shares.
    /// Split either returned path again to nest. Empty results are valid.
    ///
    /// ```
    /// use mui_geometry::*;
    /// let path=RoundedRect::new(Bounds::new(0.,0.,200.,100.),20.).unwrap().path();
    /// let inner=inset_path(&path,2.,Default::default()).unwrap().path;
    /// let [left,right]=ShapeSplit::new(SplitAxis::X,0.5).gap(2.).bend(0.2)
    ///     .regions(&inner,Default::default(),Default::default()).unwrap();
    /// assert!(!left.commands.is_empty() && !right.commands.is_empty());
    /// ```
    pub fn regions(
        self,
        path: &Path,
        o: OffsetOptions,
        g: GeometryOptions,
    ) -> Result<[Path; 2], Error> {
        self.validate()?;
        let normalized = offset_path(path, 0., o)?.path;
        let Some(bounds) = Bounds::from_points(
            normalized
                .flatten(o.flatten_tolerance, o.max_points)?
                .concat(),
        ) else {
            // Validate both option sets even when the input is empty.
            boolean(&[], &[], BooleanOp::Union, g)?;
            return Ok([Path::default(), Path::default()]);
        };
        let first = self.mask(bounds, false, o)?;
        let second = self.mask(bounds, true, o)?;
        Ok([
            boolean_paths(&normalized, &first, BooleanOp::Intersection, o, g)?,
            boolean_paths(&normalized, &second, BooleanOp::Intersection, o, g)?,
        ])
    }
    /// One partition mask, useful when a layout engine already has child frames.
    /// Extend beyond the supplied bounds before eroding so the gap affects only
    /// the divider, never the parent's outer padding.
    pub fn mask(self, b: Bounds, second: bool, o: OffsetOptions) -> Result<Path, Error> {
        self.validate()?;
        offset_path(&Path::default(), 0., o)?;
        if ![b.min.x, b.min.y, b.max.x, b.max.y]
            .iter()
            .all(|v| v.is_finite())
        {
            return Err(Error::NonFinite);
        }
        if b.width() <= 0. || b.height() <= 0. {
            return Err(Error::InvalidOptions("split bounds"));
        }
        let (start, end, origin, span) = match self.axis {
            SplitAxis::X => (b.min.y, b.max.y, b.min.x, b.width()),
            SplitAxis::Y => (b.min.x, b.max.x, b.min.y, b.height()),
        };
        let point = |u, v| match self.axis {
            SplitAxis::X => Point::new(u, v),
            SplitAxis::Y => Point::new(v, u),
        };
        let cut = origin + span * self.at;
        let margin = b.width().max(b.height()) * 2. + self.gap;
        // Quadratic chord error is abs(bend)*span/n². Respect tolerance rather
        // than silently capping subdivision and violating the requested gap.
        let steps = ((self.bend.abs() * span / o.flatten_tolerance).sqrt().ceil() as usize).max(1);
        if steps > o.max_points.saturating_sub(6) {
            return Err(Error::TooManySegments);
        }
        let mut line = Vec::with_capacity(steps + 5);
        line.push(point(cut, start - margin));
        line.extend((0..=steps).map(|i| {
            let t = i as f64 / steps as f64;
            point(
                cut + self.bend * span * 4. * t * (1. - t),
                start + (end - start) * t,
            )
        }));
        line.push(point(cut, end + margin));
        let far = if second {
            origin + span + margin
        } else {
            origin - margin
        };
        line.push(point(far, end + margin));
        line.push(point(far, start - margin));
        Ok(inset_path(&Path::polyline(line, true), self.gap / 2., o)?.path)
    }
}
