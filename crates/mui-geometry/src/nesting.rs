//! Nesting is a geometric relationship, not an inherited CSS-like radius.
use crate::{Arc, Bounds, Error, Path, PathCommand, Point};
use std::f64::consts::FRAC_PI_2;

/// These policies are deliberately different. Only `Concentric` also derives
/// the child's bounds. A radius number alone cannot guarantee a uniform band.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NestedRadius {
    /// Parallel inset of the parent's boundary in the SAME coordinate space.
    Concentric { inset: f64 },
    /// Same dimensionless r / short_side. This is a style, NOT a parallel inset.
    Proportional { scale: f64 },
    /// Deliberate independent styling. This gives no thickness guarantee.
    Independent { radius: f64 },
}

/// Validated circular-corner rectangle. Fields cannot be changed independently.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoundedRect {
    bounds: Bounds,
    radius: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InsetRect {
    pub shape: Option<RoundedRect>,
    /// At inset >= radius, convex arc correspondence collapses to a vertex.
    pub corner_collapsed: bool,
}

fn nonnegative(x: f64) -> Result<(), Error> {
    if !x.is_finite() {
        return Err(Error::NonFinite);
    }
    if x < 0.0 {
        return Err(Error::InvalidOptions("expected a nonnegative length"));
    }
    Ok(())
}

impl RoundedRect {
    pub fn new(bounds: Bounds, requested_radius: f64) -> Result<Self, Error> {
        nonnegative(requested_radius)?;
        if !bounds.min.finite() || !bounds.max.finite() {
            return Err(Error::NonFinite);
        }
        let w = bounds.width();
        let h = bounds.height();
        if !w.is_finite() || !h.is_finite() {
            return Err(Error::CoordinateLimit);
        }
        if w <= 0.0 || h <= 0.0 {
            return Err(Error::DegenerateRing);
        }
        Ok(Self {
            bounds,
            radius: requested_radius.min(0.5 * w.min(h)),
        })
    }
    pub fn bounds(self) -> Bounds {
        self.bounds
    }
    pub fn radius(self) -> f64 {
        self.radius
    }

    /// Analytic Euclidean erosion of a rounded rectangle. It is permitted to
    /// disappear. We do not silently shrink `inset` just to preserve the child.
    pub fn inset(self, inset: f64) -> Result<InsetRect, Error> {
        nonnegative(inset)?;
        let collapsed = inset >= self.radius && inset > 0.0;
        if inset >= 0.5 * self.bounds.width().min(self.bounds.height()) {
            return Ok(InsetRect {
                shape: None,
                corner_collapsed: collapsed,
            });
        }
        let d = Point::new(inset, inset);
        let bounds = Bounds {
            min: self.bounds.min + d,
            max: self.bounds.max - d,
        };
        Ok(InsetRect {
            shape: Some(Self::new(bounds, (self.radius - inset).max(0.0))?),
            corner_collapsed: collapsed,
        })
    }

    /// Analytic Euclidean dilation of a rounded rectangle. The corresponding
    /// convex arc radius grows by exactly `outset`, preserving a uniform shell.
    pub fn outset(self, outset: f64) -> Result<Self, Error> {
        nonnegative(outset)?;
        let d = Point::new(outset, outset);
        let bounds = Bounds {
            min: self.bounds.min - d,
            max: self.bounds.max + d,
        };
        Self::new(bounds, self.radius + outset)
    }

    /// All arcs are exact circles. Pixel approximation belongs to flattening.
    pub fn path(self) -> Path {
        let b = self.bounds;
        let r = self.radius;
        if r == 0.0 {
            return Path {
                commands: vec![
                    PathCommand::MoveTo(b.min),
                    PathCommand::LineTo(Point::new(b.max.x, b.min.y)),
                    PathCommand::LineTo(b.max),
                    PathCommand::LineTo(Point::new(b.min.x, b.max.y)),
                    PathCommand::Close,
                ],
            };
        }
        let centers = [
            Point::new(b.max.x - r, b.min.y + r),
            Point::new(b.max.x - r, b.max.y - r),
            Point::new(b.min.x + r, b.max.y - r),
            Point::new(b.min.x + r, b.min.y + r),
        ];
        let starts = [-FRAC_PI_2, 0.0, FRAC_PI_2, 2.0 * FRAC_PI_2];
        let ends = [
            Point::new(b.max.x, b.min.y + r),
            Point::new(b.max.x - r, b.max.y),
            Point::new(b.min.x, b.max.y - r),
            Point::new(b.min.x + r, b.min.y),
        ];
        let tangent_starts = [
            Point::new(b.max.x - r, b.min.y),
            Point::new(b.max.x, b.max.y - r),
            Point::new(b.min.x + r, b.max.y),
            Point::new(b.min.x, b.min.y + r),
        ];
        let mut commands = vec![PathCommand::MoveTo(Point::new(b.min.x + r, b.min.y))];
        for i in 0..4 {
            commands.push(PathCommand::LineTo(tangent_starts[i]));
            commands.push(PathCommand::ArcTo(Arc {
                center: centers[i],
                radius: r,
                start_angle: starts[i],
                sweep: FRAC_PI_2,
                to: ends[i],
            }));
        }
        commands.push(PathCommand::Close);
        Path { commands }
    }
}

impl NestedRadius {
    /// Concentric ignores `independent_bounds`: there is exactly one child
    /// boundary for a specified inset. Other modes deliberately use those bounds.
    pub fn resolve(
        self,
        parent: RoundedRect,
        independent_bounds: Bounds,
    ) -> Result<InsetRect, Error> {
        match self {
            Self::Concentric { inset } => parent.inset(inset),
            Self::Proportional { scale } => {
                nonnegative(scale)?;
                let short = parent.bounds.width().min(parent.bounds.height());
                let child = RoundedRect::new(independent_bounds, 0.0)?;
                let r =
                    parent.radius / short * child.bounds.width().min(child.bounds.height()) * scale;
                Ok(InsetRect {
                    shape: Some(RoundedRect::new(independent_bounds, r)?),
                    corner_collapsed: false,
                })
            }
            Self::Independent { radius } => Ok(InsetRect {
                shape: Some(RoundedRect::new(independent_bounds, radius)?),
                corner_collapsed: false,
            }),
        }
    }
}

/// For locally corresponding, smooth circular arcs before an offset singularity.
/// Convex and concave mean relative to filled material, not ring winding.
/// None says that a convex arc collapsed; a general offset needs topology cleanup.
pub fn inset_arc_radius(radius: f64, inset: f64, concave: bool) -> Result<Option<f64>, Error> {
    nonnegative(radius)?;
    nonnegative(inset)?;
    let next = if concave {
        radius + inset
    } else {
        radius - inset
    };
    if !next.is_finite() {
        return Err(Error::CoordinateLimit);
    }
    Ok((next > 0.0).then_some(next))
}

/// Uniform centerline inset needed to leave an exposed gap between two centered
/// strokes. `gap` alone is not the centerline-to-centerline distance.
pub fn inset_for_stroked_gap(gap: f64, outer_stroke: f64, inner_stroke: f64) -> Result<f64, Error> {
    nonnegative(gap)?;
    nonnegative(outer_stroke)?;
    nonnegative(inner_stroke)?;
    let d = gap + outer_stroke * 0.5 + inner_stroke * 0.5;
    if !d.is_finite() {
        return Err(Error::CoordinateLimit);
    }
    Ok(d)
}
