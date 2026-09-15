use crate::{Error, Point};
use std::f64::consts::{FRAC_PI_2, PI, TAU};
use std::fmt::Write;

/// An exact circular arc. Renderers may flatten it or approximate with cubics.
/// `to` preserves the original tangent point instead of accumulating trig error.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arc {
    pub center: Point,
    pub radius: f64,
    pub start_angle: f64,
    pub sweep: f64,
    pub to: Point,
}
impl Arc {
    pub fn point_at(self, t: f64) -> Point {
        let a = self.start_angle + self.sweep * t;
        self.center + Point::new(a.cos(), a.sin()) * self.radius
    }
    fn validate(self, current: Point) -> Result<(), Error> {
        if !self.center.finite()
            || !self.to.finite()
            || ![self.radius, self.start_angle, self.sweep]
                .iter()
                .all(|v| v.is_finite())
        {
            return Err(Error::NonFinite);
        }
        if self.radius <= 0. || self.sweep.abs() > TAU + 1e-9 {
            return Err(Error::InvalidPath);
        }
        let eps = 1e-7 * self.radius.max(1.);
        if current.distance(self.point_at(0.)) > eps || self.to.distance(self.point_at(1.)) > eps {
            return Err(Error::InvalidPath);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathCommand {
    MoveTo(Point),
    LineTo(Point),
    ArcTo(Arc),
    /// A cubic Bézier: two control points and the end. Custom drawing --
    /// response curves, envelopes -- needs it; nothing derived (welds,
    /// shells) ever produces one.
    CubicTo(Point, Point, Point),
    Close,
}
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Path {
    pub commands: Vec<PathCommand>,
}
impl Path {
    /// Validate command state and arc consistency without allocating polygons.
    pub fn validate(&self, max_commands: usize) -> Result<(), Error> {
        if self.commands.len() > max_commands {
            return Err(Error::TooManySegments);
        }
        let mut current = None;
        for command in &self.commands {
            match *command {
                PathCommand::MoveTo(p) => {
                    // An open contour (a stroked curve) simply ends here.
                    if !p.finite() {
                        return Err(Error::NonFinite);
                    }
                    current = Some(p);
                }
                PathCommand::LineTo(p) => {
                    if current.is_none() {
                        return Err(Error::InvalidPath);
                    }
                    if !p.finite() {
                        return Err(Error::NonFinite);
                    }
                    current = Some(p);
                }
                PathCommand::ArcTo(a) => {
                    a.validate(current.ok_or(Error::InvalidPath)?)?;
                    current = Some(a.to);
                }
                PathCommand::CubicTo(a, b, p) => {
                    if current.is_none() {
                        return Err(Error::InvalidPath);
                    }
                    if !(a.finite() && b.finite() && p.finite()) {
                        return Err(Error::NonFinite);
                    }
                    current = Some(p);
                }
                PathCommand::Close => {
                    if current.take().is_none() {
                        return Err(Error::InvalidPath);
                    }
                }
            }
        }
        Ok(())
    }

    /// Flatten at a maximum arc chord error measured in logical units. Use
    /// 0.2 / pixels_per_point for the egui adapter. Budget is for the WHOLE path.
    pub fn flatten(&self, tolerance: f64, max_points: usize) -> Result<Vec<Vec<Point>>, Error> {
        if !tolerance.is_finite() || tolerance <= 0. {
            return Err(Error::InvalidOptions("flatten tolerance"));
        }
        let mut contours = Vec::new();
        let mut active: Option<Vec<Point>> = None;
        let mut count = 0;
        for command in &self.commands {
            match *command {
                PathCommand::MoveTo(p) => {
                    if !p.finite() {
                        return Err(Error::NonFinite);
                    }
                    if let Some(ring) = active.take() {
                        contours.push(ring);
                    }
                    count += 1;
                    active = Some(vec![p]);
                }
                PathCommand::LineTo(p) => {
                    if !p.finite() {
                        return Err(Error::NonFinite);
                    }
                    let ring = active.as_mut().ok_or(Error::InvalidPath)?;
                    if ring.last() != Some(&p) {
                        ring.push(p);
                        count += 1;
                    }
                }
                PathCommand::ArcTo(arc) => {
                    let ring = active.as_mut().ok_or(Error::InvalidPath)?;
                    arc.validate(*ring.last().ok_or(Error::InvalidPath)?)?;
                    // Stable inverse sagitta formula; 1 - tolerance/r loses
                    // precision for very small tolerance/r ratios.
                    let step =
                        (4. * (tolerance / (2. * arc.radius)).min(1.).sqrt().asin()).min(FRAC_PI_2);
                    let needed = (arc.sweep.abs() / step).ceil().max(1.);
                    if !needed.is_finite() || needed > max_points.saturating_sub(count) as f64 {
                        return Err(Error::TooManySegments);
                    }
                    let n = needed as usize;
                    for i in 1..=n {
                        ring.push(if i == n {
                            arc.to
                        } else {
                            arc.point_at(i as f64 / n as f64)
                        });
                    }
                    count += n;
                }
                PathCommand::CubicTo(a, b, p) => {
                    if !(a.finite() && b.finite() && p.finite()) {
                        return Err(Error::NonFinite);
                    }
                    let ring = active.as_mut().ok_or(Error::InvalidPath)?;
                    let p0 = *ring.last().ok_or(Error::InvalidPath)?;
                    // Standard flatness bound: an n-piece polyline errs by at
                    // most the largest second difference times 3/4 over n².
                    let bow = (p0 - a * 2. + b).length().max((a - b * 2. + p).length());
                    let n = ((bow * 0.75 / tolerance).sqrt().ceil().max(1.) as usize).min(256);
                    if n > max_points.saturating_sub(count) {
                        return Err(Error::TooManySegments);
                    }
                    for i in 1..=n {
                        let t = i as f64 / n as f64;
                        let u = 1. - t;
                        ring.push(if i == n {
                            p
                        } else {
                            p0 * (u * u * u)
                                + a * (3. * u * u * t)
                                + b * (3. * u * t * t)
                                + p * (t * t * t)
                        });
                    }
                    count += n;
                }
                PathCommand::Close => {
                    let mut ring = active.take().ok_or(Error::InvalidPath)?;
                    if ring.len() > 1 && ring.first() == ring.last() {
                        ring.pop();
                    }
                    if ring.len() < 3 {
                        return Err(Error::InvalidPath);
                    }
                    contours.push(ring);
                }
            }
            if count > max_points {
                return Err(Error::TooManySegments);
            }
        }
        if let Some(ring) = active {
            // Open contour: a stroked curve. Booleans close it implicitly.
            contours.push(ring);
        }
        Ok(contours)
    }
    /// Only a rigid transform is offered here. Non-uniform scaling would turn
    /// circular arcs into ellipses; never pretend the radius is still circular.
    pub fn rigid_transform(&self, translation: Point, radians: f64) -> Result<Self, Error> {
        if !translation.finite() || !radians.is_finite() {
            return Err(Error::NonFinite);
        }
        self.validate(100_000)?;
        let map = |p: Point| p.rotated(radians) + translation;
        let commands = self
            .commands
            .iter()
            .map(|c| match *c {
                PathCommand::MoveTo(p) => PathCommand::MoveTo(map(p)),
                PathCommand::LineTo(p) => PathCommand::LineTo(map(p)),
                PathCommand::ArcTo(a) => PathCommand::ArcTo(Arc {
                    center: map(a.center),
                    start_angle: a.start_angle + radians,
                    to: map(a.to),
                    ..a
                }),
                PathCommand::CubicTo(a, b, p) => PathCommand::CubicTo(map(a), map(b), map(p)),
                PathCommand::Close => PathCommand::Close,
            })
            .collect();
        let result = Self { commands };
        result.validate(100_000)?;
        Ok(result)
    }
    /// Chainable construction for hand-drawn geometry: a response curve, a
    /// grid line. `quad_to` is stored as the exact equivalent cubic.
    pub fn move_to(mut self, p: Point) -> Self {
        self.commands.push(PathCommand::MoveTo(p));
        self
    }
    pub fn line_to(mut self, p: Point) -> Self {
        self.commands.push(PathCommand::LineTo(p));
        self
    }
    pub fn cubic_to(mut self, a: Point, b: Point, p: Point) -> Self {
        self.commands.push(PathCommand::CubicTo(a, b, p));
        self
    }
    pub fn quad_to(self, c: Point, p: Point) -> Self {
        let from = self.current().unwrap_or(c);
        self.cubic_to(from + (c - from) * (2. / 3.), p + (c - p) * (2. / 3.), p)
    }
    pub fn close(mut self) -> Self {
        self.commands.push(PathCommand::Close);
        self
    }
    /// The pen position after the last command, if any.
    pub fn current(&self) -> Option<Point> {
        self.commands.iter().rev().find_map(|c| match *c {
            PathCommand::MoveTo(p) | PathCommand::LineTo(p) | PathCommand::CubicTo(_, _, p) => {
                Some(p)
            }
            PathCommand::ArcTo(a) => Some(a.to),
            PathCommand::Close => None,
        })
    }
    /// A polyline through `points`, open (a stroke) or closed (a fill).
    pub fn polyline(points: impl IntoIterator<Item = Point>, closed: bool) -> Self {
        let mut it = points.into_iter();
        let mut p = match it.next() {
            Some(first) => Self::default().move_to(first),
            None => return Self::default(),
        };
        for q in it {
            p = p.line_to(q);
        }
        if closed {
            p.close()
        } else {
            p
        }
    }
    /// A centered, exact vertical capsule. This is a widget shape, not an
    /// automatically merged tab; its radius is exactly width/2.
    pub fn capsule(width: f64, height: f64) -> Result<Self, Error> {
        if !width.is_finite() || !height.is_finite() {
            return Err(Error::NonFinite);
        }
        if width <= 0. || height < width {
            return Err(Error::InvalidOptions("capsule height must be >= width > 0"));
        }
        let r = width * 0.5;
        let top = -height * 0.5 + r;
        let bottom = height * 0.5 - r;
        Ok(Self {
            commands: vec![
                PathCommand::MoveTo(Point::new(-r, top)),
                PathCommand::ArcTo(Arc {
                    center: Point::new(0., top),
                    radius: r,
                    start_angle: PI,
                    sweep: PI,
                    to: Point::new(r, top),
                }),
                PathCommand::LineTo(Point::new(r, bottom)),
                PathCommand::ArcTo(Arc {
                    center: Point::new(0., bottom),
                    radius: r,
                    start_angle: 0.,
                    sweep: PI,
                    to: Point::new(-r, bottom),
                }),
                PathCommand::Close,
            ],
        })
    }
    /// Standalone SVG path data; circular arcs remain exact SVG arcs.
    /// Caller wraps in <path fill-rule="evenodd" d="..."/>.
    pub fn to_svg_data(&self) -> Result<String, Error> {
        self.validate(100_000)?;
        let mut out = String::new();
        for command in &self.commands {
            match *command {
                PathCommand::MoveTo(p) => {
                    let _ = write!(out, "M {:0.9} {:0.9} ", p.x, p.y);
                }
                PathCommand::LineTo(p) => {
                    let _ = write!(out, "L {:0.9} {:0.9} ", p.x, p.y);
                }
                PathCommand::ArcTo(a) => {
                    // Splitting also supports a full 360-degree circle: SVG cannot
                    // encode one when the start/end points are identical.
                    let n = (a.sweep.abs() / PI).ceil().max(1.) as usize;
                    for i in 1..=n {
                        let end = if i == n {
                            a.to
                        } else {
                            a.point_at(i as f64 / n as f64)
                        };
                        let _ = write!(
                            out,
                            "A {:0.9} {:0.9} 0 0 {} {:0.9} {:0.9} ",
                            a.radius,
                            a.radius,
                            u8::from(a.sweep > 0.),
                            end.x,
                            end.y
                        );
                    }
                }
                PathCommand::CubicTo(a, b, p) => {
                    let _ = write!(
                        out,
                        "C {:0.9} {:0.9} {:0.9} {:0.9} {:0.9} {:0.9} ",
                        a.x, a.y, b.x, b.y, p.x, p.y
                    );
                }
                PathCommand::Close => out.push_str("Z "),
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod cubic_tests {
    use super::*;

    #[test]
    fn cubic_flattens_within_tolerance_and_lands_exactly() {
        let p = Path::default().move_to(Point::new(0., 0.)).cubic_to(
            Point::new(0., 100.),
            Point::new(100., 100.),
            Point::new(100., 0.),
        );
        let rings = p.flatten(0.05, 10_000).unwrap();
        let ring = &rings[0];
        assert_eq!(*ring.last().unwrap(), Point::new(100., 0.));
        assert!(ring.len() > 20);
        // Symmetric curve: the midpoint of the curve is (50, 75).
        let mid = ring
            .iter()
            .min_by(|a, b| (a.x - 50.).abs().partial_cmp(&(b.x - 50.).abs()).unwrap())
            .unwrap();
        assert!((mid.y - 75.).abs() < 0.5, "{mid:?}");
        let svg = p.to_svg_data().unwrap();
        assert!(svg.starts_with("M 0"), "{svg}");
        assert!(svg.contains(" C "), "{svg}");
        assert!(Path::default()
            .cubic_to(Point::new(0., 0.), Point::new(0., 0.), Point::new(0., 0.))
            .validate(10)
            .is_err());
    }
}

/// A cursor over SVG path data: numbers, flags and command letters, with
/// commas and whitespace treated alike as separators.
struct Scan<'a> {
    b: &'a [u8],
    i: usize,
}
impl Scan<'_> {
    fn skip(&mut self) {
        while matches!(self.b.get(self.i), Some(c) if c.is_ascii_whitespace() || *c == b',') {
            self.i += 1;
        }
    }
    fn done(&mut self) -> bool {
        self.skip();
        self.i >= self.b.len()
    }
    fn num(&mut self) -> Option<f64> {
        self.skip();
        let (b, start) = (self.b, self.i);
        let mut i = start;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        while matches!(b.get(i), Some(c) if c.is_ascii_digit()) {
            i += 1;
        }
        if b.get(i) == Some(&b'.') {
            i += 1;
            while matches!(b.get(i), Some(c) if c.is_ascii_digit()) {
                i += 1;
            }
        }
        if i > start && matches!(b.get(i), Some(b'e' | b'E')) {
            let mut j = i + 1;
            if matches!(b.get(j), Some(b'+' | b'-')) {
                j += 1;
            }
            if matches!(b.get(j), Some(c) if c.is_ascii_digit()) {
                while matches!(b.get(j), Some(c) if c.is_ascii_digit()) {
                    j += 1;
                }
                i = j;
            }
        }
        let v = std::str::from_utf8(&b[start..i]).ok()?.parse().ok()?;
        self.i = i;
        Some(v)
    }
    /// An arc flag is a single character, and `11` is two flags, not eleven.
    fn flag(&mut self) -> Option<bool> {
        self.skip();
        let f = match self.b.get(self.i)? {
            b'0' => false,
            b'1' => true,
            _ => return None,
        };
        self.i += 1;
        Some(f)
    }
    fn letter(&mut self) -> Option<u8> {
        self.skip();
        let c = *self.b.get(self.i)?;
        c.is_ascii_alphabetic().then(|| {
            self.i += 1;
            c
        })
    }
}

/// An SVG elliptical arc as cubics: endpoint parametrisation to centre
/// parametrisation (SVG 1.1 F.6.5), then one cubic per <=90 degrees, which
/// is under a thousandth of the radius in error.
fn arc_cubics(
    from: Point,
    (rx, ry): (f64, f64),
    phi: f64,
    (large, sweep): (bool, bool),
    to: Point,
    out: &mut Vec<PathCommand>,
) {
    let (rx, ry) = (rx.abs(), ry.abs());
    // Out-of-range radii degrade to a line, as the spec requires.
    if rx == 0. || ry == 0. || from == to {
        out.push(PathCommand::LineTo(to));
        return;
    }
    let (cp, sp) = (phi.cos(), phi.sin());
    let d = (from - to) * 0.5;
    let (x1, y1) = (cp * d.x + sp * d.y, -sp * d.x + cp * d.y);
    // Scale the radii up until they can span the chord.
    let lambda = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
    let (rx, ry) = if lambda > 1. {
        (rx * lambda.sqrt(), ry * lambda.sqrt())
    } else {
        (rx, ry)
    };
    let (rx2, ry2) = (rx * rx, ry * ry);
    let den = rx2 * y1 * y1 + ry2 * x1 * x1;
    let mut k = ((rx2 * ry2 - den) / den).max(0.).sqrt();
    if large == sweep {
        k = -k;
    }
    let (cx1, cy1) = (k * rx * y1 / ry, -k * ry * x1 / rx);
    let center = Point::new(cp * cx1 - sp * cy1, sp * cx1 + cp * cy1) + (from + to) * 0.5;
    let angle = |u: Point, v: Point| {
        let c = u.dot(v) / (u.length() * v.length());
        u.cross(v).signum() * c.clamp(-1., 1.).acos()
    };
    let u = Point::new((x1 - cx1) / rx, (y1 - cy1) / ry);
    let v = Point::new((-x1 - cx1) / rx, (-y1 - cy1) / ry);
    let theta = angle(Point::new(1., 0.), u);
    let mut sweep_angle = angle(u, v) % TAU;
    if !sweep && sweep_angle > 0. {
        sweep_angle -= TAU;
    } else if sweep && sweep_angle < 0. {
        sweep_angle += TAU;
    }
    let n = (sweep_angle.abs() / FRAC_PI_2).ceil().max(1.);
    let (n, step) = (n as usize, sweep_angle / n);
    // 4/3 tan(step/4) is the classic cubic-through-an-arc control length.
    let hand = 4. / 3. * (step / 4.).tan();
    let at = |t: f64| {
        let (c, s) = (t.cos(), t.sin());
        center + Point::new(cp * rx * c - sp * ry * s, sp * rx * c + cp * ry * s)
    };
    let tangent = |t: f64| {
        let (c, s) = (t.cos(), t.sin());
        Point::new(-cp * rx * s - sp * ry * c, -sp * rx * s + cp * ry * c)
    };
    for i in 0..n {
        let (t0, t1) = (theta + step * i as f64, theta + step * (i + 1) as f64);
        let (p0, p1) = (at(t0), at(t1));
        out.push(PathCommand::CubicTo(
            p0 + tangent(t0) * hand,
            p1 - tangent(t1) * hand,
            // End on the coordinate the data gave, not on reconstructed trig.
            if i + 1 == n { to } else { p1 },
        ));
    }
}

fn quad(from: Point, c: Point, p: Point) -> PathCommand {
    PathCommand::CubicTo(from + (c - from) * (2. / 3.), p + (c - p) * (2. / 3.), p)
}

impl Path {
    /// Parse SVG path data (the `d` attribute): every command, relative and
    /// absolute, implicit repeats, and both separators.
    ///
    /// Elliptical arcs become cubics -- MUI's own [`Arc`] is circular, and an
    /// imported icon is drawn, never inset or welded, so exactness buys
    /// nothing. Quadratics become their exact cubic equivalent, as
    /// [`Path::quad_to`] does.
    pub fn from_svg_data(data: &str) -> Result<Self, Error> {
        let mut s = Scan {
            b: data.as_bytes(),
            i: 0,
        };
        let mut out: Vec<PathCommand> = Vec::new();
        let (mut cur, mut start) = (Point::new(0., 0.), Point::new(0., 0.));
        let mut last: Option<u8> = None;
        // The control point a following S or T reflects, if the last command
        // was of the matching kind.
        let (mut cubic_ctrl, mut quad_ctrl) = (None, None);
        while !s.done() {
            let cmd = match s.letter() {
                Some(c) => {
                    last = Some(c);
                    c
                }
                // An implicit repeat: another coordinate set for the last
                // command, except a moveto, which repeats as a lineto.
                None => match last {
                    Some(b'M') => b'L',
                    Some(b'm') => b'l',
                    Some(b'Z' | b'z') | None => return Err(Error::InvalidPath),
                    Some(c) => c,
                },
            };
            if out.is_empty() && !matches!(cmd, b'M' | b'm') {
                return Err(Error::InvalidPath);
            }
            let o = if cmd.is_ascii_lowercase() {
                cur
            } else {
                Point::new(0., 0.)
            };
            macro_rules! num {
                () => {
                    s.num().ok_or(Error::InvalidPath)?
                };
            }
            macro_rules! pt {
                () => {{
                    let x = num!();
                    o + Point::new(x, num!())
                }};
            }
            let (mut next_cubic, mut next_quad) = (None, None);
            match cmd.to_ascii_uppercase() {
                b'M' => {
                    cur = pt!();
                    start = cur;
                    out.push(PathCommand::MoveTo(cur));
                }
                b'L' => {
                    cur = pt!();
                    out.push(PathCommand::LineTo(cur));
                }
                b'H' => {
                    cur = Point::new(o.x + num!(), cur.y);
                    out.push(PathCommand::LineTo(cur));
                }
                b'V' => {
                    cur = Point::new(cur.x, o.y + num!());
                    out.push(PathCommand::LineTo(cur));
                }
                b'C' | b'S' => {
                    let a = if cmd.eq_ignore_ascii_case(&b'C') {
                        pt!()
                    } else {
                        cubic_ctrl.map_or(cur, |c: Point| cur * 2. - c)
                    };
                    let b = pt!();
                    let p = pt!();
                    out.push(PathCommand::CubicTo(a, b, p));
                    (cur, next_cubic) = (p, Some(b));
                }
                b'Q' | b'T' => {
                    let c = if cmd.eq_ignore_ascii_case(&b'Q') {
                        pt!()
                    } else {
                        quad_ctrl.map_or(cur, |q: Point| cur * 2. - q)
                    };
                    let p = pt!();
                    out.push(quad(cur, c, p));
                    (cur, next_quad) = (p, Some(c));
                }
                b'A' => {
                    let radii = (num!(), num!());
                    let rotation: f64 = num!();
                    let flags = (
                        s.flag().ok_or(Error::InvalidPath)?,
                        s.flag().ok_or(Error::InvalidPath)?,
                    );
                    let p = pt!();
                    arc_cubics(cur, radii, rotation.to_radians(), flags, p, &mut out);
                    cur = p;
                }
                b'Z' => {
                    out.push(PathCommand::Close);
                    cur = start;
                }
                _ => return Err(Error::InvalidPath),
            }
            (cubic_ctrl, quad_ctrl) = (next_cubic, next_quad);
        }
        let path = Self { commands: out };
        path.validate(100_000)?;
        Ok(path)
    }
}

#[cfg(test)]
mod svg_tests {
    use super::*;

    /// Data in, the same drawing out: an arc's cubics survive a round trip
    /// through `to_svg_data` and back.
    #[test]
    fn svg_data_round_trips_including_an_arc() {
        let d = "M10,80 h40 V20 q20-20 40 0 t40 0 A30 30 0 0 1 150 130 \
                 c-10 10-30 10-40 0 s-30-10-40 0 Z";
        let a = Path::from_svg_data(d).unwrap();
        let b = Path::from_svg_data(&a.to_svg_data().unwrap()).unwrap();
        let (fa, fb) = (
            a.flatten(0.01, 100_000).unwrap(),
            b.flatten(0.01, 100_000).unwrap(),
        );
        assert_eq!(fa.len(), fb.len());
        for (x, y) in fa.iter().flatten().zip(fb.iter().flatten()) {
            assert!(x.distance(*y) < 1e-6, "{x:?} vs {y:?}");
        }
        assert!(a
            .commands
            .iter()
            .any(|c| matches!(c, PathCommand::CubicTo(..))));

        // The arc really bows: the half-circle from (130,20) to (150,130)
        // bulges past both endpoints' x.
        let bounds = crate::Bounds::from_points(fa.iter().flatten().copied()).unwrap();
        assert!(bounds.max.x > 155., "arc did not bow: {bounds:?}");
        // A relative lineto is relative, and `h40` lands at x=50.
        assert_eq!(a.commands[1], PathCommand::LineTo(Point::new(50., 80.)));
        // Implicit repeats and flag packing.
        let two = Path::from_svg_data("M0 0L1 1 2 2").unwrap();
        assert_eq!(two.commands.len(), 3);
        assert!(Path::from_svg_data("M0 0A5 5 0 11 10 0").is_ok());
        for bad in ["L0 0", "M0", "M0 0A5 5 0 5 1 10 0"] {
            assert!(Path::from_svg_data(bad).is_err(), "accepted {bad:?}");
        }
    }
}
