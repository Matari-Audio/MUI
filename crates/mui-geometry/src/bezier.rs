//! Canonical MUI-path to cubic-Bezier conversion, shared by painting and input.
use crate::{Error, Path, PathCommand};
use kurbo::BezPath;

/// Maximum arc-to-cubic error in logical scene units.
pub const ARC_TOLERANCE: f64 = 0.01;

/// Validate and convert without selecting a renderer or creating GPU resources.
pub fn bez_path(path: &Path, tolerance: f64) -> Result<BezPath, Error> {
    if !(tolerance.is_finite() && tolerance > 0.0) {
        return Err(Error::InvalidPath);
    }
    path.validate(250_000)?;
    let mut out = BezPath::new();
    for command in &path.commands {
        match *command {
            PathCommand::MoveTo(p) => out.move_to((p.x, p.y)),
            PathCommand::LineTo(p) => out.line_to((p.x, p.y)),
            PathCommand::ArcTo(arc) => {
                let k = kurbo::Arc::new(
                    (arc.center.x, arc.center.y),
                    (arc.radius, arc.radius),
                    arc.start_angle,
                    arc.sweep,
                    0.0,
                );
                out.extend(k.append_iter(tolerance));
                // Preserve the exact recorded tangent endpoint.
                out.line_to((arc.to.x, arc.to.y));
            }
            PathCommand::CubicTo(a, b, p) => out.curve_to((a.x, a.y), (b.x, b.y), (p.x, p.y)),
            PathCommand::Close => out.close_path(),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{PathEl, Shape};

    #[test]
    fn preserves_curves_and_capsule_bounds() {
        let p = Path::capsule(48.0, 120.0).unwrap();
        let b = bez_path(&p, ARC_TOLERANCE).unwrap();
        assert!(b.elements().iter().any(|e| matches!(e, PathEl::CurveTo(..))));
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
