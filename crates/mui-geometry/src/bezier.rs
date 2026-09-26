//! Canonical MUI-path to cubic-Bezier conversion, shared by painting and input.
use crate::{Error, Path, PathCommand};
use kurbo::{BezPath, PathEl, Vec2};
use rustc_hash::FxHashMap as HashMap;
use std::cell::RefCell;

/// Maximum arc-to-cubic error in logical scene units.
pub const ARC_TOLERANCE: f64 = 0.01;

thread_local! {
    /// Arcs repeat -- a rounded corner is one of four quadrants of its
    /// radius -- and kurbo builds every one from sines. Its cubics are the
    /// centre plus offsets nothing else moves, so the offsets are kept and
    /// the centre added as kurbo adds it: the same bits, without the sines.
    // ponytail: cleared past 256 shapes; an LRU if a scene has more radii.
    static ARCS: RefCell<HashMap<[u64; 4], Vec<[Vec2; 3]>>> = RefCell::default();
}

/// Validate and convert without selecting a renderer or creating GPU resources.
pub fn bez_path(path: &Path, tolerance: f64) -> Result<BezPath, Error> {
    // An arc is a curve or two and a line: room for most paths at once.
    let mut out = BezPath::with_capacity(2 * path.commands.len());
    bez_path_into(path, tolerance, &mut out)?;
    Ok(out)
}

/// [`bez_path`] into a caller's buffer, which is cleared first: a paint loop
/// that keeps one converts every entry without allocating.
pub fn bez_path_into(path: &Path, tolerance: f64, out: &mut BezPath) -> Result<(), Error> {
    if !(tolerance.is_finite() && tolerance > 0.0) {
        return Err(Error::InvalidPath);
    }
    path.validate(250_000)?;
    out.truncate(0);
    for command in &path.commands {
        match *command {
            PathCommand::MoveTo(p) => out.move_to((p.x, p.y)),
            PathCommand::LineTo(p) => out.line_to((p.x, p.y)),
            PathCommand::ArcTo(arc) => {
                let c = kurbo::Point::new(arc.center.x, arc.center.y);
                let key = [arc.radius, arc.start_angle, arc.sweep, tolerance].map(f64::to_bits);
                ARCS.with_borrow_mut(|arcs| {
                    if arcs.len() > 256 {
                        arcs.clear();
                    }
                    let arm = arcs.entry(key).or_insert_with(|| {
                        let k = kurbo::Arc::new(
                            (0.0, 0.0),
                            (arc.radius, arc.radius),
                            arc.start_angle,
                            arc.sweep,
                            0.0,
                        );
                        k.append_iter(tolerance)
                            .filter_map(|el| match el {
                                PathEl::CurveTo(a, b, p) => {
                                    Some([a, b, p].map(kurbo::Point::to_vec2))
                                }
                                _ => None,
                            })
                            .collect()
                    });
                    for [a, b, p] in arm.iter() {
                        out.curve_to(c + *a, c + *b, c + *p);
                    }
                });
                // Preserve the exact recorded tangent endpoint.
                out.line_to((arc.to.x, arc.to.y));
            }
            PathCommand::CubicTo(a, b, p) => out.curve_to((a.x, a.y), (b.x, b.y), (p.x, p.y)),
            PathCommand::Close => out.close_path(),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{PathEl, Shape};

    #[test]
    fn a_remembered_arc_converts_to_kurbos_own_bits() {
        let arc = |x: f64, y: f64, r: f64, start: f64, sweep: f64| {
            let mut a = crate::path::Arc {
                center: crate::Point { x, y },
                radius: r,
                start_angle: start,
                sweep,
                to: crate::Point { x, y },
            };
            a.to = a.point_at(1.0);
            a
        };
        for (x, y) in [(3.3, 7.1), (1033.7, 0.1), (-12.25, 480.9), (3.3, 7.1)] {
            for (r, start, sweep) in [(8.0, 0.0, 1.5), (6.5, 3.1, -2.0), (120.0, 1.0, 6.2)] {
                let a = arc(x, y, r, start, sweep);
                let path = Path {
                    commands: vec![PathCommand::MoveTo(a.point_at(0.0)), PathCommand::ArcTo(a)],
                };
                let mut want = BezPath::new();
                want.move_to((a.point_at(0.0).x, a.point_at(0.0).y));
                want.extend(
                    kurbo::Arc::new((x, y), (r, r), start, sweep, 0.0).append_iter(ARC_TOLERANCE),
                );
                want.line_to((a.to.x, a.to.y));
                assert_eq!(bez_path(&path, ARC_TOLERANCE).unwrap(), want);
            }
        }
    }

    #[test]
    fn a_reused_buffer_is_refilled_not_appended_to() {
        let (a, b) = (
            Path::capsule(48.0, 120.0).unwrap(),
            Path::capsule(10.0, 30.0).unwrap(),
        );
        let mut out = bez_path(&a, ARC_TOLERANCE).unwrap();
        bez_path_into(&b, ARC_TOLERANCE, &mut out).unwrap();
        assert_eq!(out, bez_path(&b, ARC_TOLERANCE).unwrap());
    }

    #[test]
    fn preserves_curves_and_capsule_bounds() {
        let p = Path::capsule(48.0, 120.0).unwrap();
        let b = bez_path(&p, ARC_TOLERANCE).unwrap();
        assert!(
            b.elements()
                .iter()
                .any(|e| matches!(e, PathEl::CurveTo(..)))
        );
        let bounds = b.bounding_box();
        assert!((bounds.width() - 48.0).abs() < 0.05);
        assert!((bounds.height() - 120.0).abs() < 0.05);
    }

    #[test]
    fn rejects_invalid_tolerances() {
        let p = Path::capsule(48.0, 120.0).unwrap();
        for t in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(bez_path(&p, t).is_err());
        }
    }
}
