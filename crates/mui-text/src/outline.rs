use mui_geometry::kurbo::{self, PathEl};
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

/// Collects `skrifa` pen calls into MUI path commands, flattening curves
/// adaptively (kurbo) as it goes: `PathCommand` has no bezier to carry them.
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
    /// Flattens one curve from the cursor with kurbo's adaptive subdivision.
    ///
    /// `bow` is the largest second difference of the control points and `k`
    /// the flatness-bound constant for the degree: `bow * k / n^2` bounds the
    /// error of n uniform segments. ponytail: tolerance is loosened so no
    /// segment needs more than 64 lines, which only bites for glyphs
    /// thousands of pixels tall; raise the cap if those must stay exact.
    fn flatten(&mut self, seg: PathEl, bow: f64, k: f64) {
        let tolerance = self.tolerance.max(bow * k / (64. * 64.));
        kurbo::flatten([PathEl::MoveTo(self.cursor), seg], tolerance, |el| {
            if let PathEl::LineTo(p) = el {
                self.line(p);
            }
        });
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
        let (c, p1) = (self.point(cx, cy), self.point(x, y));
        let bow = (self.cursor.to_vec2() - c.to_vec2() * 2. + p1.to_vec2()).length();
        self.flatten(PathEl::QuadTo(c, p1), bow, 0.125);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (c0, c1, p1) = (self.point(cx0, cy0), self.point(cx1, cy1), self.point(x, y));
        let [p0, v0, v1, v2] = [self.cursor, c0, c1, p1].map(Point::to_vec2);
        let bow = (p0 - v0 * 2. + v1)
            .length()
            .max((v0 - v1 * 2. + v2).length());
        self.flatten(PathEl::CurveTo(c0, c1, p1), bow, 0.75);
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
                    .map(|(a, b)| a.to_vec2().cross(b.to_vec2()))
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
    fn a_large_glyph_stays_within_tolerance() {
        use mui_geometry::kurbo::{Line, ParamCurveNearest as _};
        // At 1000 px a fixed segment count would sag visibly; adaptive
        // flattening keeps every chord midpoint within tolerance of the curve,
        // here stood in for by a far finer flattening of the same glyph.
        let tolerance = 0.25;
        let coarse = glyph_path(&hack(), 'O', 1000., &[], tolerance).unwrap();
        let fine = glyph_path(&hack(), 'O', 1000., &[], 0.01).unwrap();
        let fine: Vec<Vec<Point>> = fine.flatten(0.01, 250_000).unwrap();
        let distance = |p: Point| {
            fine.iter()
                .flat_map(|r| r.iter().zip(r.iter().cycle().skip(1)))
                .map(|(&a, &b)| Line::new(a, b).nearest(p, 1e-9).distance_sq.sqrt())
                .fold(f64::INFINITY, f64::min)
        };
        for ring in coarse.flatten(tolerance, 250_000).unwrap() {
            assert!(ring.len() > 16, "curves were flattened");
            for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
                let d = distance(a.midpoint(*b));
                assert!(d <= tolerance + 0.05, "chord sags {d} px");
            }
        }
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
                    .map(|(a, b)| a.to_vec2().cross(b.to_vec2()))
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
        let b = a
            .rigid_transform(mui_geometry::Vec2::new(1., 0.), 0.)
            .unwrap();
        let both = Path {
            commands: a
                .commands
                .iter()
                .chain(b.commands.iter())
                .copied()
                .collect(),
        };
        // On the left stroke of the ring, where both glyphs have ink.
        let bounds = mui_geometry::bounds(a.flatten(0.05, 250_000).unwrap().concat()).unwrap();
        let p = Point::new(bounds.x0 + 4., (bounds.y0 + bounds.y1) / 2.);
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
