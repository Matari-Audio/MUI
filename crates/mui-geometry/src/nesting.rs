//! Nesting is a geometric relationship, not an inherited CSS-like radius.
use crate::{Arc, Error, Path, PathCommand, Point, Rect, Vec2};
use std::f64::consts::FRAC_PI_2;

/// Validated circular-corner rectangle. Fields cannot be changed independently.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoundedRect {
    bounds: Rect,
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
    /// The same rectangle moved by `d`; a translation keeps it valid.
    pub fn translated(self, d: Vec2) -> Self {
        Self {
            bounds: self.bounds + d,
            ..self
        }
    }
    pub fn new(bounds: Rect, requested_radius: f64) -> Result<Self, Error> {
        nonnegative(requested_radius)?;
        if !bounds.is_finite() {
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
    pub fn bounds(self) -> Rect {
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
        let bounds = self.bounds.inset(-inset);
        Ok(InsetRect {
            shape: Some(Self::new(bounds, (self.radius - inset).max(0.0))?),
            corner_collapsed: collapsed,
        })
    }

    /// Analytic Euclidean dilation of a rounded rectangle. The corresponding
    /// convex arc radius grows by exactly `outset`, preserving a uniform shell.
    pub fn outset(self, outset: f64) -> Result<Self, Error> {
        nonnegative(outset)?;
        let bounds = self.bounds.inset(outset);
        Self::new(bounds, self.radius + outset)
    }

    /// All arcs are exact circles. Pixel approximation belongs to flattening.
    pub fn path(self) -> Path {
        let (commands, n) = self.commands();
        Path {
            commands: commands[..n].to_vec(),
        }
    }

    /// The rounded rectangle `path` is, when it is exactly what
    /// [`RoundedRect::path`] (or [`Path::rect`], or [`Path::circle`]) built:
    /// what lets a renderer blur it analytically instead of as a path. No
    /// allocation; any other path, even the same shape drawn another way, is
    /// `None`.
    ///
    /// ```
    /// use mui_geometry::{Path, Point, Rect, RoundedRect};
    /// let card = RoundedRect::new(Rect::new(0., 0., 80., 40.), 6.).unwrap();
    /// assert_eq!(RoundedRect::of(&card.path()), Some(card));
    /// let dot = Path::circle(Point::new(10., 10.), 4.).unwrap();
    /// assert_eq!(RoundedRect::of(&dot).map(|r| r.radius()), Some(4.));
    /// let tri = Path::polyline([(0., 0.), (9., 0.), (4., 7.)].map(|(x, y)| Point::new(x, y)), true);
    /// assert_eq!(RoundedRect::of(&tri), None);
    /// ```
    // ponytail: exact command match only; a translated copy or another
    // winding is a path blur. Match within a tolerance if one shows up.
    pub fn of(path: &Path) -> Option<Self> {
        use PathCommand::{ArcTo, Close, LineTo, MoveTo};
        let (bounds, radius) = match path.commands[..] {
            [MoveTo(a), LineTo(_), LineTo(c), LineTo(_), Close] => {
                (Rect::new(a.x, a.y, c.x, c.y), 0.)
            }
            [
                MoveTo(a),
                _,
                ArcTo(r0),
                _,
                ArcTo(r1),
                _,
                ArcTo(r2),
                _,
                ArcTo(_),
                Close,
            ] => (Rect::new(r2.to.x, a.y, r0.to.x, r1.to.y), r0.radius),
            _ => return None,
        };
        let r = Self::new(bounds, radius).ok()?;
        let (commands, n) = r.commands();
        (r.radius == radius && commands[..n] == path.commands[..]).then_some(r)
    }

    /// [`RoundedRect::path`]'s commands, without the allocation.
    fn commands(self) -> ([PathCommand; 10], usize) {
        let b = self.bounds;
        let r = self.radius;
        let mut commands = [PathCommand::Close; 10];
        if r == 0.0 {
            commands[..4].copy_from_slice(&[
                PathCommand::MoveTo(b.origin()),
                PathCommand::LineTo(Point::new(b.x1, b.y0)),
                PathCommand::LineTo(Point::new(b.x1, b.y1)),
                PathCommand::LineTo(Point::new(b.x0, b.y1)),
            ]);
            return (commands, 5);
        }
        let centers = [
            Point::new(b.x1 - r, b.y0 + r),
            Point::new(b.x1 - r, b.y1 - r),
            Point::new(b.x0 + r, b.y1 - r),
            Point::new(b.x0 + r, b.y0 + r),
        ];
        let starts = [-FRAC_PI_2, 0.0, FRAC_PI_2, 2.0 * FRAC_PI_2];
        let ends = [
            Point::new(b.x1, b.y0 + r),
            Point::new(b.x1 - r, b.y1),
            Point::new(b.x0, b.y1 - r),
            Point::new(b.x0 + r, b.y0),
        ];
        let tangent_starts = [
            Point::new(b.x1 - r, b.y0),
            Point::new(b.x1, b.y1 - r),
            Point::new(b.x0 + r, b.y1),
            Point::new(b.x0, b.y0 + r),
        ];
        // One move, four line/arc pairs and the closing command.
        commands[0] = PathCommand::MoveTo(Point::new(b.x0 + r, b.y0));
        for i in 0..4 {
            commands[1 + 2 * i] = PathCommand::LineTo(tangent_starts[i]);
            commands[2 + 2 * i] = PathCommand::ArcTo(Arc {
                center: centers[i],
                radius: r,
                start_angle: starts[i],
                sweep: FRAC_PI_2,
                to: ends[i],
            });
        }
        (commands, 10)
    }
}
