use mui_geometry::{Path, PathCommand, Point};
use skrifa::MetadataProvider as _;
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::prelude::{LocationRef, Size};

use crate::error::checked_size;
use crate::font::location;
use crate::{Axis, Error, Font};

/// The outline of one glyph, baseline at `y = 0` and growing upward as
/// negative `y` — the screen convention, already flipped out of font space.
///
/// `size_px` is the em size. `tolerance` is the maximum chord deviation when
/// the quadratic and cubic segments are flattened, in the same units.
///
/// Curves are flattened here rather than carried, because `PathCommand` models
/// only lines and exact circular arcs. That is the right vocabulary for the
/// corner system, and the wrong one for a glyph: no bezier in a typeface is a
/// circular arc. Flattening at load makes the tolerance a property of the call
/// instead of the paint, so re-derive the path if the display scale changes.
pub fn glyph_path(
    font: &Font,
    ch: char,
    size_px: f64,
    axes: &[Axis<'_>],
    tolerance: f64,
) -> Result<Path, Error> {
    let size = checked_size(size_px)?;
    let mut pen = PathPen::new(tolerance)?;
    let font = font.font_ref()?;
    let glyph_id = font.charmap().map(ch).ok_or(Error::MissingGlyph(ch))?;
    let outlines = font.outline_glyphs();
    let outline = outlines.get(glyph_id).ok_or(Error::NoOutline(ch))?;
    let location = location(&font, size_px, axes);
    outline
        .draw(
            DrawSettings::unhinted(Size::new(size), LocationRef::from(&location)),
            &mut pen,
        )
        .map_err(Error::Draw)?;
    let path = pen.finish();
    path.validate(usize::MAX)?;
    Ok(path)
}

/// Collects `skrifa` pen calls into MUI path commands, flattening as it goes.
pub(crate) struct PathPen {
    commands: Vec<PathCommand>,
    cursor: Point,
    open: bool,
    tolerance: f64,
    /// Where this glyph's origin sits. Carried on the pen rather than applied
    /// afterwards so a run never pays for a second pass over its own points.
    pub(crate) dx: f64,
    pub(crate) dy: f64,
}

impl PathPen {
    pub(crate) fn new(tolerance: f64) -> Result<Self, Error> {
        if !tolerance.is_finite() || tolerance <= 0. {
            return Err(Error::InvalidOptions("tolerance"));
        }
        Ok(Self {
            commands: Vec::new(),
            cursor: Point::new(0., 0.),
            open: false,
            tolerance,
            dx: 0.,
            dy: 0.,
        })
    }
    /// Font space is y-up, the scene is y-down.
    fn point(&self, x: f32, y: f32) -> Point {
        Point::new(self.dx + x as f64, self.dy - y as f64)
    }
    fn line(&mut self, p: Point) {
        self.commands.push(PathCommand::LineTo(p));
        self.cursor = p;
    }
    pub(crate) fn close_open_contour(&mut self) {
        if self.open {
            self.commands.push(PathCommand::Close);
            self.open = false;
        }
    }
    /// Uniform subdivision counts from the standard flatness bounds: the error
    /// of an n-segment polyline is bounded by the second difference of the
    /// control points over n squared, so invert that for n.
    fn steps(&self, second_difference: f64, numerator: f64) -> usize {
        let n = (second_difference * numerator / self.tolerance)
            .sqrt()
            .ceil();
        (n.max(1.) as usize).min(64)
    }
    pub(crate) fn finish(mut self) -> Path {
        self.close_open_contour();
        Path {
            commands: self.commands,
        }
    }
}

impl OutlinePen for PathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        // Some faces run contours together without an explicit close.
        self.close_open_contour();
        let p = self.point(x, y);
        self.commands.push(PathCommand::MoveTo(p));
        self.cursor = p;
        self.open = true;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.line(p);
    }

    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let (p0, c, p1) = (self.cursor, self.point(cx, cy), self.point(x, y));
        let n = self.steps((p0 - c * 2. + p1).length(), 0.125);
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1. - t;
            self.line(p0 * (u * u) + c * (2. * u * t) + p1 * (t * t));
        }
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let p0 = self.cursor;
        let c0 = self.point(cx0, cy0);
        let c1 = self.point(cx1, cy1);
        let p1 = self.point(x, y);
        let bow = (p0 - c0 * 2. + c1)
            .length()
            .max((c0 - c1 * 2. + p1).length());
        let n = self.steps(bow, 0.75);
        for i in 1..=n {
            let t = i as f64 / n as f64;
            let u = 1. - t;
            self.line(
                p0 * (u * u * u) + c0 * (3. * u * u * t) + c1 * (3. * u * t * t) + p1 * (t * t * t),
            );
        }
    }

    fn close(&mut self) {
        self.close_open_contour();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fonts::{hack, symbols};

    fn area(path: &Path) -> f64 {
        path.flatten(0.05, 250_000)
            .unwrap()
            .iter()
            .map(|ring| {
                ring.iter()
                    .zip(ring.iter().cycle().skip(1))
                    .map(|(a, b)| a.cross(*b))
                    .sum::<f64>()
                    * 0.5
            })
            .sum()
    }

    #[test]
    fn a_counter_is_its_own_contour() {
        // "O" is the cheapest proof that contours survive: an outer ring and a
        // counter, wound against each other so the signed areas cancel down.
        let path = glyph_path(&hack(), 'O', 64., &[], 0.05).unwrap();
        let rings = path.flatten(0.05, 250_000).unwrap();
        assert_eq!(rings.len(), 2, "outer contour plus counter");
        assert!(rings.iter().all(|r| r.len() > 8), "curves were flattened");
    }

    #[test]
    fn outline_scales_with_em_size() {
        let small = area(&glyph_path(&hack(), 'H', 32., &[], 0.05).unwrap()).abs();
        let large = area(&glyph_path(&hack(), 'H', 64., &[], 0.05).unwrap()).abs();
        assert!(small > 0.);
        assert!(
            (large / small - 4.).abs() < 0.05,
            "doubling the em size quadruples the area, got {large} / {small}"
        );
    }

    #[test]
    fn baseline_sits_at_zero_and_the_glyph_grows_upward() {
        let path = glyph_path(&hack(), 'H', 64., &[], 0.05).unwrap();
        let rings = path.flatten(0.05, 250_000).unwrap();
        let ys: Vec<f64> = rings.iter().flatten().map(|p| p.y).collect();
        let top = ys.iter().copied().fold(f64::INFINITY, f64::min);
        let bottom = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        assert!(top < 0., "cap height is above the baseline");
        assert!(bottom.abs() < 1., "'H' rests on the baseline, got {bottom}");
    }

    #[test]
    fn tolerance_buys_vertices() {
        let coarse = glyph_path(&hack(), 'O', 64., &[], 0.5).unwrap();
        let fine = glyph_path(&hack(), 'O', 64., &[], 0.01).unwrap();
        assert!(fine.commands.len() > coarse.commands.len() * 2);
        // Both describe the same glyph, so the area must not drift with it.
        let (fine, coarse) = (area(&fine).abs(), area(&coarse).abs());
        assert!((fine - coarse).abs() / fine < 0.05, "{fine} vs {coarse}");
    }

    #[test]
    fn an_axis_a_static_font_lacks_is_ignored_not_an_error() {
        let plain = glyph_path(&hack(), 'H', 64., &[], 0.05).unwrap();
        let asked = glyph_path(&hack(), 'H', 64., &[("FILL", 1.0)], 0.05).unwrap();
        assert_eq!(plain, asked, "Hack has no FILL axis, so nothing moves");
    }

    #[test]
    fn a_collapsed_fill_contour_is_dropped_not_an_error() {
        // At FILL=0 the fill ring of every Material Symbol collapses to a
        // point. That is a legal empty contour, not an invalid path.
        let hollow = glyph_path(&symbols(), '\u{E87D}', 24., &[("FILL", 0.)], 0.05).unwrap();
        let solid = glyph_path(&symbols(), '\u{E87D}', 24., &[("FILL", 1.)], 0.05).unwrap();
        let hollow_area = area(&hollow).abs();
        let solid_area = area(&solid).abs();
        assert!(hollow_area > 0.);
        assert!(
            solid_area > hollow_area * 1.5,
            "filled heart covers more: {solid_area} vs {hollow_area}"
        );
        let mid = glyph_path(&symbols(), '\u{E87D}', 24., &[("FILL", 0.5)], 0.05).unwrap();
        let mid_area = area(&mid).abs();
        assert!(
            mid_area > hollow_area && mid_area < solid_area,
            "morph is monotone: {mid_area}"
        );
    }

    #[test]
    fn a_glyph_counter_winds_as_a_hole() {
        let path = glyph_path(&hack(), 'O', 96., &[], 0.05).unwrap();
        // The counter comes out empty because a typeface reverses it, not
        // because of the fill rule: the signed area is the ring, not the disc.
        // Compare against the outer contour's own area.
        let rings = path.flatten(0.05, 250_000).unwrap();
        let outer = rings
            .iter()
            .map(|r| {
                (r.iter()
                    .zip(r.iter().cycle().skip(1))
                    .map(|(a, b)| a.cross(*b))
                    .sum::<f64>()
                    * 0.5)
                    .abs()
            })
            .fold(0., f64::max);
        let filled = area(&path).abs();
        assert!(filled < outer * 0.9, "the counter is a hole, not fill");
        assert!(filled > outer * 0.3, "but the ring itself is filled");
    }

    /// Crossing number at `p`, cast along +x. The whole point of the non-zero
    /// switch is that this is not the same as its parity.
    fn winding(path: &Path, p: Point) -> i32 {
        let mut w = 0;
        for ring in path.flatten(0.05, 250_000).unwrap() {
            for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
                // Half-open in y so a vertex on the ray is counted once.
                if (a.y <= p.y) == (b.y <= p.y) {
                    continue;
                }
                let t = (p.y - a.y) / (b.y - a.y);
                if a.x + t * (b.x - a.x) > p.x {
                    w += if b.y > a.y { 1 } else { -1 };
                }
            }
        }
        w
    }

    #[test]
    fn overlapping_glyphs_fill_rather_than_cancel() {
        // Two `O`s a pixel apart: the outer contours overlap almost entirely.
        // Even-odd would XOR that overlap away and leave a crescent.
        let a = glyph_path(&hack(), 'O', 96., &[], 0.05).unwrap();
        let b = a.rigid_transform(Point::new(1., 0.), 0.).unwrap();
        let both = Path {
            commands: a
                .commands
                .iter()
                .chain(b.commands.iter())
                .copied()
                .collect(),
        };
        // On the left stroke of the ring, where both glyphs have ink.
        let bounds =
            mui_geometry::Bounds::from_points(a.flatten(0.05, 250_000).unwrap().concat()).unwrap();
        let p = Point::new(bounds.min.x + 4., (bounds.min.y + bounds.max.y) / 2.);
        assert_eq!(winding(&a, p).abs(), 1, "one glyph covers the probe once");
        assert_eq!(
            winding(&both, p).abs(),
            2,
            "non-zero: covered twice, inside"
        );
        assert_eq!(winding(&both, p) % 2, 0, "even-odd would have erased it");
    }

    #[test]
    fn a_missing_glyph_is_reported_not_drawn_blank() {
        assert!(matches!(
            glyph_path(&hack(), '\u{10FFFD}', 64., &[], 0.05),
            Err(Error::MissingGlyph(_))
        ));
    }
}
