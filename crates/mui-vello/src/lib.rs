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
use mui_core::{Layer, Paint, Painted, ResolvedScene};
use mui_geometry::{Error, Path, PathCommand};
use std::sync::{Arc, Mutex};
/// The brush type [`Canvas::set_paint`] takes, so the trait can be
/// implemented outside this crate.
pub use vello_common::paint::PaintType;
use vello_common::peniko::color::{AlphaColor, DynamicColor, Srgb};
use vello_common::peniko::{Blob, ColorStop, FontData, Gradient};
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
            PathCommand::CubicTo(a, b, p) => out.curve_to((a.x, a.y), (b.x, b.y), (p.x, p.y)),
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
    /// Everything drawn until the matching [`Canvas::pop_clip`] is clipped to `p`.
    fn push_clip(&mut self, p: &BezPath);
    fn pop_clip(&mut self);
    /// Draw a hinted glyph run in the current paint, `x` measured from
    /// `origin` along the baseline.
    fn glyphs(&mut self, font: &Arc<Vec<u8>>, size: f32, origin: (f64, f64), glyphs: &[(u32, f32)]);
}

/// One [`FontData`] per distinct font. Vello's hinted-glyph and atlas caches
/// key on the blob id, and `Blob::new` mints a fresh one per call, so building
/// the font per run would throw those caches away every frame.
// ponytail: global and never evicted -- an entry is one `Arc` clone and a font
// outlives the process anyway; a host-owned cache is the upgrade if a plugin
// ever unloads one.
static FONTS: Mutex<Vec<(usize, FontData)>> = Mutex::new(Vec::new());

fn font_data(font: &Arc<Vec<u8>>) -> FontData {
    // Holding the `Arc` is what makes the pointer a sound key: the allocation
    // cannot be freed and its address reused under a stale entry.
    let key = Arc::as_ptr(font) as usize;
    let mut fonts = FONTS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, f)) = fonts.iter().find(|(k, _)| *k == key) {
        return f.clone();
    }
    let f = FontData::new(Blob::new(font.clone()), 0);
    fonts.push((key, f.clone()));
    f
}

fn run(
    origin: (f64, f64),
    glyphs: &[(u32, f32)],
) -> impl Iterator<Item = glifo::Glyph> + Clone + '_ {
    let (ox, oy) = (origin.0 as f32, origin.1 as f32);
    glyphs.iter().map(move |&(id, x)| glifo::Glyph {
        id,
        x: ox + x,
        y: oy,
    })
}

/// A `vello_hybrid` scene together with the resources its glyph cache lives in.
pub struct Gpu<'a> {
    pub scene: &'a mut vello_hybrid::Scene,
    pub resources: &'a mut vello_hybrid::Resources,
}

/// A `vello_cpu` context together with the resources its glyph cache lives in.
#[cfg(feature = "cpu")]
pub struct Cpu<'a> {
    pub ctx: &'a mut vello_cpu::RenderContext,
    pub resources: &'a mut vello_cpu::Resources,
}

macro_rules! wrapper {
    ($w:ty, $inner:ident) => {
        impl Canvas for $w {
            fn set_transform(&mut self, t: Affine) {
                self.$inner.set_transform(t)
            }
            fn set_paint(&mut self, p: PaintType) {
                self.$inner.set_paint(p)
            }
            fn set_stroke(&mut self, s: Stroke) {
                self.$inner.set_stroke(s)
            }
            fn fill_path(&mut self, p: &BezPath) {
                self.$inner.fill_path(p)
            }
            fn stroke_path(&mut self, p: &BezPath) {
                self.$inner.stroke_path(p)
            }
            fn fill_blurred_rounded_rect(&mut self, r: &Rect, radius: f32, std_dev: f32) {
                self.$inner
                    .fill_blurred_rounded_rect(r, radius, std_dev, false)
            }
            fn push_clip(&mut self, p: &BezPath) {
                self.$inner.push_clip_layer(p)
            }
            fn pop_clip(&mut self) {
                self.$inner.pop_layer()
            }
            fn glyphs(
                &mut self,
                font: &Arc<Vec<u8>>,
                size: f32,
                origin: (f64, f64),
                glyphs: &[(u32, f32)],
            ) {
                self.$inner
                    .glyph_run(self.resources, &font_data(font))
                    .font_size(size)
                    .hint(true)
                    .fill_glyphs(run(origin, glyphs));
            }
        }
    };
}
wrapper!(Gpu<'_>, scene);
#[cfg(feature = "cpu")]
wrapper!(Cpu<'_>, ctx);

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
    if p.layer == Layer::Unclip {
        canvas.pop_clip();
        return Ok(());
    }
    let path = bez_path(&p.path, ARC_TOLERANCE)?;
    if p.layer == Layer::Clip {
        canvas.push_clip(&path);
        return Ok(());
    }
    canvas.set_paint(brush(&p.paint, path.bounding_box()));
    if let Some(t) = &p.text {
        canvas.glyphs(&t.font, t.size, (t.origin.x, t.origin.y), &t.glyphs);
        return Ok(());
    }
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
        let mut res = vello_cpu::Resources::default();
        paint(
            &mut Cpu {
                ctx: &mut ctx,
                resources: &mut res,
            },
            &scene,
            Affine::IDENTITY,
        )
        .unwrap();
        ctx.flush();
        let mut pix = Pixmap::new(120, 60);
        ctx.render(&mut pix, &mut res);
        let at = |x: usize, y: usize| pix.data()[y * 120 + x];
        let want = Theme::default().palette.primary().to_srgb().to_rgba8();
        let got = at(60, 8);
        assert!(
            (got.r as i32 - want.r as i32).abs() <= 1,
            "{got:?} vs {want:?}"
        );
        assert_eq!(at(0, 0).a, 0, "corner is rounded away");
    }

    /// Render `spec` on the CPU and hand back the pixels.
    fn pixels(spec: &SceneSpec, w: u16, h: u16) -> Pixmap {
        let scene = resolve_scene(spec).unwrap();
        let mut ctx = vello_cpu::RenderContext::new(w, h);
        let mut res = vello_cpu::Resources::default();
        paint(
            &mut Cpu {
                ctx: &mut ctx,
                resources: &mut res,
            },
            &scene,
            Affine::IDENTITY,
        )
        .unwrap();
        ctx.flush();
        let mut pix = Pixmap::new(w, h);
        ctx.render(&mut pix, &mut res);
        pix
    }

    /// Text reaches the pixels as a glyph run: it goes through Vello's
    /// atlas, so ink on the pixmap means the run drew.
    #[test]
    fn a_glyph_run_lands_pixels() {
        let mut spec =
            SceneSpec::new(text("HI").fill(Role::Ink).id("t")).offered(Size::new(80., 40.));
        spec.font = Some(std::sync::Arc::new(
            epaint_default_fonts::HACK_REGULAR.to_vec(),
        ));
        let scene = resolve_scene(&spec).unwrap();
        assert!(
            scene.paint.iter().any(|p| p.text.is_some()),
            "no glyphs to draw"
        );
        let pix = pixels(&spec, 80, 40);
        assert!(pix.data().iter().any(|p| p.a > 0), "the run drew nothing");
    }

    /// A clip layer actually clips: the oversized child stops at its parent.
    #[test]
    fn a_clipped_child_stays_inside_its_parent() {
        let child = leaf(200., 200.).fill(Role::Ink).id("child");
        let boxed = column([child])
            .size(40., 40.)
            .clip()
            .fill(Role::Surface)
            .anchor(Align::Start, Align::Start)
            .id("box");
        let spec = SceneSpec::new(overlay([boxed])).offered(Size::new(80., 80.));
        let scene = resolve_scene(&spec).unwrap();
        let c = scene.surface("child").expect("child").frame;
        assert!(c.x < 0. && c.right() > 40., "no overflow to clip: {c:?}");
        let pix = pixels(&spec, 80, 80);
        let at = |x: usize, y: usize| pix.data()[y * 80 + x];
        assert!(at(20, 20).a > 0, "nothing drew inside the clip");
        for (x, y) in [(60, 20), (20, 60), (60, 60)] {
            assert_eq!(at(x, y).a, 0, "painted outside the clip at {x},{y}");
        }
    }
}
