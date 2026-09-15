//! The seam between MUI geometry and the Vello renderer.
//!
//! MUI owns *what* the shape is: intrinsic layout, boolean merging, fillets,
//! constant-thickness nesting, exact arcs. Vello owns *what it looks like*:
//! analytic antialiasing, gradients, blends, clips, filters. Those are separate
//! jobs, and this crate is the whole of the connection between them: [`paint`]
//! walks a resolved scene's paint list onto any [`Canvas`], and [`bez_path`]
//! is the one conversion underneath it.
//!
//! Arcs stay arcs until this point. MUI's tessellation path flattens them to
//! line segments; here they become cubics instead, which is what Vello wants
//! and what keeps a 24 px corner smooth when the scene is scaled up.
#![forbid(unsafe_code)]

use kurbo::{Affine, BezPath, Rect, Shape as _, Stroke};
use mui_core::{Paint, Painted, ResolvedScene};
use mui_geometry::{Error, Path, PathCommand};
use vello_common::paint::PaintType;
use vello_common::peniko::color::{AlphaColor, DynamicColor, Srgb};
use vello_common::peniko::{ColorStop, Gradient};
pub use vello_common::{kurbo, peniko};
#[cfg(feature = "cpu")]
pub use vello_cpu;
pub use vello_hybrid;

/// Curve error, in scene units, allowed when an arc becomes cubics. Vello
/// re-flattens per frame at device resolution, so this only has to be finer
/// than anything a later transform can magnify into view.
pub const ARC_TOLERANCE: f64 = 0.01;

/// Convert a resolved MUI path into a Bézier path Vello can fill or stroke.
///
/// The path is validated first: a malformed arc here would silently render as
/// a wrong shape rather than fail, and geometry bugs are much cheaper to find
/// at the seam than in a screenshot.
pub fn bez_path(path: &Path, tolerance: f64) -> Result<BezPath, Error> {
    if !(tolerance.is_finite() && tolerance > 0.) {
        return Err(Error::InvalidPath);
    }
    path.validate(250_000)?;

    let mut out = BezPath::new();
    for command in &path.commands {
        match *command {
            PathCommand::MoveTo(p) => out.move_to((p.x, p.y)),
            PathCommand::LineTo(p) => out.line_to((p.x, p.y)),
            PathCommand::ArcTo(arc) => {
                // `append_iter` emits curves only, continuing from the current
                // point -- exactly the shape of an `ArcTo`.
                let k = kurbo::Arc::new(
                    (arc.center.x, arc.center.y),
                    (arc.radius, arc.radius),
                    arc.start_angle,
                    arc.sweep,
                    0.,
                );
                out.extend(k.append_iter(tolerance));
                // Land on the tangent point MUI recorded rather than on the
                // one trig reconstructed, so consecutive arcs cannot drift.
                out.line_to((arc.to.x, arc.to.y));
            }
            PathCommand::Close => out.close_path(),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{ParamCurve as _, PathEl};

    fn capsule() -> Path {
        Path::capsule(48., 120.).unwrap()
    }

    #[test]
    fn a_capsule_keeps_its_extent_through_the_conversion() {
        let path = capsule();
        let flat = path.flatten(0.01, 250_000).unwrap();
        let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
        for p in flat.iter().flatten() {
            lo = (lo.0.min(p.x), lo.1.min(p.y));
            hi = (hi.0.max(p.x), hi.1.max(p.y));
        }
        let b = bez_path(&path, ARC_TOLERANCE).unwrap().bounding_box();
        for (a, b) in [(lo.0, b.x0), (lo.1, b.y0), (hi.0, b.x1), (hi.1, b.y1)] {
            assert!((a - b).abs() < 0.05, "extent drifted: {a} vs {b}");
        }
    }

    #[test]
    fn an_arc_lands_on_the_tangent_point_mui_recorded() {
        let path = capsule();
        let bez = bez_path(&path, ARC_TOLERANCE).unwrap();
        for command in &path.commands {
            let PathCommand::ArcTo(arc) = *command else {
                continue;
            };
            let hit = bez.segments().any(|s| {
                let e = s.eval(1.);
                (e.x - arc.to.x).hypot(e.y - arc.to.y) < 1e-9
            });
            assert!(hit, "no segment ends at {:?}", arc.to);
        }
    }

    #[test]
    fn arcs_become_curves_not_a_polyline() {
        let bez = bez_path(&capsule(), ARC_TOLERANCE).unwrap();
        assert!(
            bez.elements()
                .iter()
                .any(|e| matches!(e, PathEl::CurveTo(..))),
            "a rounded shape flattened to line segments"
        );
    }

    #[test]
    fn a_nonsense_tolerance_is_refused_rather_than_hung_on() {
        for t in [0., -1., f64::NAN, f64::INFINITY] {
            assert!(bez_path(&capsule(), t).is_err(), "accepted tolerance {t}");
        }
    }

    #[test]
    fn a_malformed_arc_is_caught_at_the_seam() {
        let mut path = capsule();
        let PathCommand::ArcTo(arc) = &mut path.commands[1] else {
            panic!("expected the capsule's first arc at index 1");
        };
        arc.radius *= 2.;
        assert!(bez_path(&path, ARC_TOLERANCE).is_err());
    }
}

/// The handful of calls painting needs, so one walk serves the GPU scene and
/// the CPU context alike.
pub trait Canvas {
    fn set_transform(&mut self, t: Affine);
    fn set_paint(&mut self, p: PaintType);
    fn set_stroke(&mut self, s: Stroke);
    fn fill_path(&mut self, p: &BezPath);
    fn stroke_path(&mut self, p: &BezPath);
    fn fill_blurred_rounded_rect(&mut self, r: &Rect, radius: f32, std_dev: f32);
}
macro_rules! canvas {
    ($t:ty) => {
        impl Canvas for $t {
            fn set_transform(&mut self, t: Affine) {
                <$t>::set_transform(self, t)
            }
            fn set_paint(&mut self, p: PaintType) {
                <$t>::set_paint(self, p)
            }
            fn set_stroke(&mut self, s: Stroke) {
                <$t>::set_stroke(self, s)
            }
            fn fill_path(&mut self, p: &BezPath) {
                <$t>::fill_path(self, p)
            }
            fn stroke_path(&mut self, p: &BezPath) {
                <$t>::stroke_path(self, p)
            }
            fn fill_blurred_rounded_rect(&mut self, r: &Rect, radius: f32, std_dev: f32) {
                <$t>::fill_blurred_rounded_rect(self, r, radius, std_dev, false)
            }
        }
    };
}
canvas!(vello_hybrid::Scene);
#[cfg(feature = "cpu")]
canvas!(vello_cpu::RenderContext);

fn srgb(c: mui_core::Color) -> AlphaColor<Srgb> {
    c.to_srgb()
}

/// A resolved MUI paint as a Vello brush. Gradient angles follow CSS: 180
/// runs top to bottom across `bounds`.
pub fn brush(p: &Paint, bounds: Rect) -> PaintType {
    match p {
        Paint::Solid(c) => PaintType::Solid(srgb(*c)),
        Paint::Linear { angle, stops } => {
            let a = angle.to_radians();
            let (s, c) = (a.sin(), -a.cos());
            let len = (bounds.width() * s).abs() + (bounds.height() * c).abs();
            let mid = bounds.center();
            let half = (s * len / 2.0, c * len / 2.0);
            let stops: Vec<ColorStop> = stops
                .iter()
                .map(|(t, col)| ColorStop {
                    offset: *t,
                    color: DynamicColor::from_alpha_color(srgb(*col)),
                })
                .collect();
            PaintType::Gradient(
                Gradient::new_linear(
                    (mid.x - half.0, mid.y - half.1),
                    (mid.x + half.0, mid.y + half.1),
                )
                .with_stops(&stops[..]),
            )
        }
    }
}

/// Draw every entry of the scene's paint list, in order, under `transform`.
///
/// Shadows take Vello's analytic blurred rectangle when the outline is one;
/// a blurred *welded* outline has no fast path and draws unblurred.
pub fn paint(
    canvas: &mut impl Canvas,
    scene: &ResolvedScene,
    transform: Affine,
) -> Result<(), Error> {
    canvas.set_transform(transform);
    for p in &scene.paint {
        one(canvas, p)?;
    }
    Ok(())
}

fn one(canvas: &mut impl Canvas, p: &Painted) -> Result<(), Error> {
    let path = bez_path(&p.path, ARC_TOLERANCE)?;
    canvas.set_paint(brush(&p.paint, path.bounding_box()));
    match (p.blur > 0.0, p.rect, p.width > 0.0) {
        (true, Some(rr), _) => {
            let b = rr.bounds();
            canvas.fill_blurred_rounded_rect(
                &Rect::new(b.min.x, b.min.y, b.max.x, b.max.y),
                rr.radius() as f32,
                p.blur as f32,
            );
        }
        // ponytail: blur on a welded outline is drawn sharp; a blur filter
        // layer is the upgrade if a merged shadow ever needs it.
        (_, _, false) => canvas.fill_path(&path),
        (_, _, true) => {
            canvas.set_stroke(Stroke::new(p.width));
            canvas.stroke_path(&path);
        }
    }
    Ok(())
}

#[cfg(all(test, feature = "cpu"))]
mod snapshot {
    use super::*;
    use mui_core::prelude::*;
    use vello_common::pixmap::Pixmap;

    /// The whole stack on the CPU: a filled card reaches the pixels, its ink
    /// reads against it, and the rounded corner stays clear.
    #[test]
    fn a_card_lands_on_the_pixmap() {
        let root = column([text("hi").id("t")])
            .pad(20.)
            .fill(Role::Primary)
            .stroke(Role::Ink)
            .id("card");
        let mut spec = SceneSpec::new(root).offered(Size::new(120., 60.));
        spec.font = Some(std::sync::Arc::new(
            epaint_default_fonts::HACK_REGULAR.to_vec(),
        ));
        let scene = resolve_scene(&spec).unwrap();
        assert!(scene.paint.iter().any(|p| p.layer == mui_core::Layer::Text));

        let mut ctx = vello_cpu::RenderContext::new(120, 60);
        paint(&mut ctx, &scene, Affine::IDENTITY).unwrap();
        let mut pix = Pixmap::new(120, 60);
        ctx.render(&mut pix, &mut vello_cpu::Resources::default());
        let at = |x: usize, y: usize| pix.data()[y * 120 + x];
        let want = Theme::default().palette.primary().to_srgb().to_rgba8();
        let got = at(60, 8);
        assert!(
            (got.r as i32 - want.r as i32).abs() <= 1,
            "{got:?} vs {want:?}"
        );
        assert_eq!(at(0, 0).a, 0, "corner is rounded away");
    }
}
