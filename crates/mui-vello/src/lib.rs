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
use mui_geometry::Error;
use mui_scene::{Fit, GradientKind, Layer, Paint, Painted, ResolvedScene, ShadowKind};
use std::sync::{Arc, Weak};
#[cfg(feature = "gpu-effects")]
pub use vello;
#[cfg(feature = "cpu")]
use vello_common::filter_effects::{EdgeMode, Filter, FilterPrimitive};
/// The brush type [`Canvas::set_paint`] takes, so the trait can be
/// implemented outside this crate.
pub use vello_common::paint::PaintType;
use vello_common::peniko::color::PremulRgba8;
use vello_common::peniko::color::{AlphaColor, DynamicColor, Srgb};
#[cfg(feature = "cpu")]
use vello_common::peniko::ImageSampler;
use vello_common::peniko::{Blob, ColorStop, ColorStops, FontData, Gradient};
use vello_common::pixmap::Pixmap;
pub use vello_common::{kurbo, peniko};
#[cfg(feature = "cpu")]
pub use vello_cpu;
#[cfg(feature = "gpu-effects")]
mod classic;
#[cfg(feature = "gpu-effects")]
pub use classic::Classic;
#[cfg(feature = "gpu-effects")]
pub mod effects;

/// The path conversion painting uses, the same one input hit-tests with.
pub use mui_geometry::{bez_path, bez_path_into, ARC_TOLERANCE};

#[cfg(test)]
mod tests {
    use super::*;
    use kurbo::{ParamCurve as _, PathEl};
    use mui_geometry::{Path, PathCommand};

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
    /// Called once at the start of every paint walk, before anything draws:
    /// a canvas with caches ages them here.
    fn begin_frame(&mut self) {}
    fn set_transform(&mut self, t: Affine);
    fn set_paint(&mut self, p: PaintType);
    /// Where the current paint's own space lands, composed after the scene
    /// transform. Image space is pixels; gradients and solids ignore it, so
    /// a canvas that never paints an image may leave both of these alone.
    fn set_paint_transform(&mut self, _t: Affine) {}
    fn reset_paint_transform(&mut self) {}
    /// This image as a brush, or `None` for [`mui_scene::Paint::solid`]'s
    /// stand-in. `vello_cpu` takes the pixmap itself, [`Classic`] the
    /// buffer as Vello's image. A canvas with no image support keeps this
    /// default and paints the stand-in.
    fn image(&mut self, _img: &mui_scene::Image) -> Option<PaintType> {
        None
    }
    fn set_stroke(&mut self, s: Stroke);
    fn fill_path(&mut self, p: &BezPath);
    fn stroke_path(&mut self, p: &BezPath);
    /// A Gaussian-blurred rounded rectangle, analytically. With `invert`
    /// the coverage is flipped -- opaque outside the rectangle, fading to
    /// nothing inside it -- which, clipped to a shape, is an inset shadow.
    fn fill_blurred_rounded_rect(&mut self, r: &Rect, radius: f32, std_dev: f32, invert: bool);
    /// Everything drawn until the matching [`Canvas::pop_clip`] is clipped to
    /// `p`. This is Vello's clip *stack*, not a compositing layer: no
    /// intermediate texture, and an unpopped clip is not a panic.
    fn push_clip(&mut self, p: &BezPath);
    fn pop_clip(&mut self);
    /// Everything drawn until the matching [`Canvas::pop_layer`] composites
    /// as one layer, through `blend` at `opacity`. Unlike a clip this costs
    /// an intermediate target on the GPU, so it is worth one per subtree
    /// that asks for it, not one per node.
    fn push_layer(&mut self, blend: peniko::BlendMode, opacity: f32);
    fn pop_layer(&mut self);
    /// Two layers, closed by two [`Canvas::pop_layer`]s: one clipped to
    /// `clip`, and inside it one that composites through a Gaussian blur of
    /// `std_dev`, in the space of the current transform. `false`, having pushed
    /// nothing, on a canvas without filter layers: a [`Layer::Backdrop`]
    /// then leaves the backdrop sharp.
    fn push_blur(&mut self, _clip: &BezPath, _std_dev: f32) -> bool {
        false
    }
    /// Draw a hinted glyph run in the current paint, each glyph's `x`
    /// measured from the run's origin along the baseline.
    fn glyphs(&mut self, text: &mui_scene::Text);
}

#[cfg(feature = "cpu")]
fn run(
    origin: mui_geometry::Point,
    glyphs: &[mui_scene::TextGlyph],
) -> impl Iterator<Item = glifo::Glyph> + Clone + '_ {
    let (ox, oy) = (origin.x as f32, origin.y as f32);
    glyphs.iter().map(move |glyph| glifo::Glyph {
        id: glyph.id,
        x: ox + glyph.x,
        y: oy + glyph.y,
    })
}

/// What one renderer remembers between frames on MUI's side: a [`FontData`]
/// per font and an entry per image buffer. Keep it next to the renderer it
/// serves and drop it with
/// that renderer.
#[derive(Default)]
pub struct Cache {
    /// Vello's hinted-glyph caches key on the blob id, and `Blob::new` mints a
    /// fresh one per call, so building the font per run would throw those
    /// caches away every frame.
    /// With the frames each was first and last drawn in: `Font` exposes no
    /// handle a `Weak` could watch, so a font goes once it sat unused for
    /// [`FONT_FRAMES`] frames -- a host that makes a fresh `Font` per frame
    /// no longer grows this without bound.
    fonts: Vec<(u64, FontData, Frames)>,
    /// Paint walks so far; see [`Canvas::begin_frame`].
    frame: u64,
    /// Keyed by a `Weak`, so no entry keeps the app's buffer alive. The one
    /// eviction rule: an entry goes on the first image lookup after its
    /// buffer's last `Arc` dropped. Until then the dead `Weak` still pins the
    /// allocation, which is what keeps the address a sound key.
    // ponytail: linear scan, bounded by the live image count.
    images: Vec<(Weak<[u8]>, Stored)>,
}

#[derive(Clone, Copy)]
struct Frames {
    born: u64,
    used: u64,
}

/// How long an unused font keeps its [`FontData`], and with it Vello's
/// hinted outlines: glifo's own glyph atlas ages entries out after as many.
const FONT_FRAMES: u64 = 64;

enum Stored {
    /// Only the CPU canvas stores pixmaps.
    #[cfg_attr(not(feature = "cpu"), allow(dead_code))]
    Pixmap(Arc<Pixmap>),
    /// Only the GPU canvas stores these.
    #[cfg(feature = "gpu-effects")]
    Image(vello::peniko::ImageData),
}

impl Cache {
    /// Start a frame: fonts unused for [`FONT_FRAMES`] frames go.
    fn tick(&mut self) {
        self.frame += 1;
        let now = self.frame;
        self.fonts.retain(|(_, _, f)| now - f.used <= FONT_FRAMES);
    }

    /// The font's `FontData`, and whether it was already drawn in an earlier
    /// frame: a glyph atlas only pays for itself from the second frame on,
    /// and a one-shot render (a snapshot, a thumbnail) never gets there.
    fn font(&mut self, font: &mui_scene::Font) -> (FontData, bool) {
        // A `Font` id is never reused, so an entry cannot answer for another font.
        let key = font.id();
        let now = self.frame;
        if let Some((_, f, frames)) = self.fonts.iter_mut().find(|(k, ..)| *k == key) {
            frames.used = now;
            return (f.clone(), frames.born < now);
        }
        let f = FontData::new(Blob::new(Arc::new(font.clone())), 0);
        self.fonts.push((
            key,
            f.clone(),
            Frames {
                born: now,
                used: now,
            },
        ));
        (f, false)
    }

    fn find(&self, rgba: &Arc<[u8]>) -> Option<&Stored> {
        self.images
            .iter()
            .find(|(k, _)| std::ptr::addr_eq(k.as_ptr(), Arc::as_ptr(rgba)))
            .map(|(_, s)| s)
    }

    fn remember(&mut self, rgba: &Arc<[u8]>, stored: Stored) {
        self.images.push((Arc::downgrade(rgba), stored));
    }

    /// Drop every entry whose buffer the app has let go of.
    fn sweep(&mut self) {
        self.images.retain(|(k, _)| k.strong_count() > 0);
    }
}

#[cfg(feature = "cpu")]
macro_rules! wrapper {
    ($inner:ident, $atlas:expr) => {
        fn begin_frame(&mut self) {
            self.cache.tick();
        }
        fn set_transform(&mut self, t: Affine) {
            self.$inner.set_transform(t)
        }
        fn set_paint(&mut self, p: PaintType) {
            self.$inner.set_paint(p)
        }
        fn set_paint_transform(&mut self, t: Affine) {
            self.$inner.set_paint_transform(t)
        }
        fn reset_paint_transform(&mut self) {
            self.$inner.reset_paint_transform()
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
        fn fill_blurred_rounded_rect(&mut self, r: &Rect, radius: f32, std_dev: f32, invert: bool) {
            self.$inner
                .fill_blurred_rounded_rect(r, radius, std_dev, invert)
        }
        fn push_clip(&mut self, p: &BezPath) {
            self.$inner.push_clip_path(p)
        }
        fn pop_clip(&mut self) {
            self.$inner.pop_clip_path()
        }
        fn push_layer(&mut self, blend: peniko::BlendMode, opacity: f32) {
            self.$inner
                .push_layer(None, Some(blend), Some(opacity), None, None)
        }
        fn pop_layer(&mut self) {
            self.$inner.pop_layer()
        }
        fn glyphs(&mut self, text: &mui_scene::Text) {
            let mut start = 0;
            while start < text.glyphs.len() {
                let font_index = text.glyphs[start].font;
                let end = text.glyphs[start + 1..]
                    .iter()
                    .position(|glyph| glyph.font != font_index)
                    .map_or(text.glyphs.len(), |offset| start + 1 + offset);
                // A hand-built run can name a face it does not carry.
                let Some(face) = text.fonts.get(font_index) else {
                    start = end;
                    continue;
                };
                let (font, warm) = self.cache.font(face);
                let coords = text.font_coords.get(font_index).map_or(&[][..], |c| &c[..]);
                self.$inner
                    .glyph_run(self.resources, &font)
                    .font_size(text.size)
                    // Off for the frame after an axis moved: see Text::hint.
                    .hint(text.hint)
                    // The run was measured at this instance; drawing the default
                    // one under its advances is how a bold readout goes ragged.
                    .normalized_coords(coords)
                    // Rasterise each glyph once and reuse the bitmap: strip
                    // generation per glyph was most of a frame's encode.
                    .atlas_cache($atlas && warm)
                    .fill_glyphs(run(text.origin, &text.glyphs[start..end]));
                start = end;
            }
        }
    };
}

#[cfg(feature = "cpu")]
impl Canvas for Cpu<'_> {
    fn image(&mut self, img: &mui_scene::Image) -> Option<PaintType> {
        self.cache.sweep();
        let p = match self.cache.find(&img.rgba) {
            Some(Stored::Pixmap(p)) => p.clone(),
            _ => {
                let p = Arc::new(premultiply(img)?);
                self.cache.remember(&img.rgba, Stored::Pixmap(p.clone()));
                p
            }
        };
        Some(
            vello_common::paint::Image {
                image: vello_common::paint::ImageSource::Pixmap(p),
                sampler: ImageSampler::default(),
            }
            .into(),
        )
    }
    fn push_blur(&mut self, clip: &BezPath, std_dev: f32) -> bool {
        // `vello_cpu` panics on a filter layer in a multi-threaded context.
        let one_thread = self.ctx.render_settings().num_threads == 0;
        if one_thread {
            self.ctx.push_clip_layer(clip);
            self.ctx.push_filter_layer(blur(std_dev));
        }
        one_thread
    }
    // `vello_cpu` keeps its glyph atlas apart from images: nothing to mirror.
    wrapper!(ctx, true);
}

/// A `vello_cpu` context together with the resources its glyph cache lives
/// in and the [`Cache`] of its renderer.
#[cfg(feature = "cpu")]
pub struct Cpu<'a> {
    pub ctx: &'a mut vello_cpu::RenderContext,
    pub resources: &'a mut vello_cpu::Resources,
    pub cache: &'a mut Cache,
}

#[cfg(feature = "cpu")]
/// The one filter MUI asks for. `Duplicate` edges, because the backdrop
/// stops at the window, and fading it to transparent there would darken the
/// frosted glass along every screen edge.
fn blur(std_dev: f32) -> Filter {
    Filter::from_primitive(FilterPrimitive::GaussianBlur {
        std_deviation: std_dev,
        edge_mode: EdgeMode::Duplicate,
    })
}

/// `c` as painted. A frame asks for the same few theme colours thousands of
/// times, and the Oklch conversion takes cube roots, so the last conversions
/// are kept in a small direct-mapped table.
fn srgb(c: mui_scene::Color) -> AlphaColor<Srgb> {
    use std::cell::RefCell;
    type Slot = Option<([u32; 4], AlphaColor<Srgb>)>;
    thread_local!(static SEEN: RefCell<[Slot; 64]> = const { RefCell::new([None; 64]) });
    let key = [c.lightness(), c.chroma(), c.hue(), c.alpha()].map(f32::to_bits);
    let mix = key[0] ^ key[1].rotate_left(8) ^ key[2].rotate_left(16) ^ key[3].rotate_left(24);
    let at = (mix.wrapping_mul(0x9E37_79B1) >> 26) as usize;
    SEEN.with_borrow_mut(|seen| match seen[at] {
        Some((k, v)) if k == key => v,
        _ => {
            let v = c.to_srgb();
            seen[at] = Some((key, v));
            v
        }
    })
}

/// The premultiplied [`Pixmap`] of an image. MUI hands over straight RGBA --
/// what a decoder produces -- and premultiplying a photo is far too much work
/// to redo every frame, so each renderer's [`Cache`] keeps the result.
#[cfg_attr(not(any(test, feature = "cpu")), allow(dead_code))]
fn premultiply(img: &mui_scene::Image) -> Option<Pixmap> {
    size(img).map(|(w, h)| premultiplied(img, w, h))
}

/// The image's size, or `None` when it cannot become a [`Pixmap`].
fn size(img: &mui_scene::Image) -> Option<(u16, u16)> {
    // A single image has to fit one atlas tile; u16 is the hard ceiling.
    let (w, h) = (
        u16::try_from(img.width).ok()?,
        u16::try_from(img.height).ok()?,
    );
    // `Image`'s fields are public, so the buffer need not match the size;
    // `Pixmap::from_parts_with_opacity` asserts that it does.
    (img.rgba.len() == usize::from(w) * usize::from(h) * 4).then_some((w, h))
}

/// [`premultiply`] for a size [`size`] already checked.
fn premultiplied(img: &mui_scene::Image, w: u16, h: u16) -> Pixmap {
    let mut clear = false;
    let (pixels, _) = img.rgba.as_chunks::<4>();
    let data = pixels
        .iter()
        .map(|p| {
            clear |= p[3] != 255;
            let m = |c: u8| ((u16::from(p[3]) * u16::from(c)) / 255) as u8;
            PremulRgba8 {
                r: m(p[0]),
                g: m(p[1]),
                b: m(p[2]),
                a: p[3],
            }
        })
        .collect();
    Pixmap::from_parts_with_opacity(data, w, h, clear)
}

/// Where the image's pixels land so that it fills `bounds` per `fit`.
fn image_transform(img: &mui_scene::Image, fit: Fit, bounds: Rect) -> Affine {
    let (iw, ih) = (f64::from(img.width), f64::from(img.height));
    let (sx, sy) = (bounds.width() / iw, bounds.height() / ih);
    let (sx, sy) = match fit {
        Fit::Fill => (sx, sy),
        Fit::Cover => (sx.max(sy), sx.max(sy)),
        Fit::Contain => (sx.min(sy), sx.min(sy)),
    };
    // Centred: what cover crops and what contain letterboxes is symmetric.
    Affine::translate((
        bounds.center().x - iw * sx / 2.,
        bounds.center().y - ih * sy / 2.,
    )) * Affine::scale_non_uniform(sx, sy)
}

/// A resolved MUI paint as a Vello brush. Gradient angles follow CSS: 180
/// runs top to bottom across `bounds`.
pub fn brush(p: &Paint, bounds: Rect) -> PaintType {
    match p {
        Paint::Solid(c) => PaintType::Solid(srgb(*c)),
        // Images go through `Canvas::image`, which knows whether its
        // renderer takes a pixmap or an atlas id; here only the stand-in.
        Paint::Image { .. } => PaintType::Solid(srgb(p.solid())),
        Paint::Gradient { kind, stops } => {
            // `ColorStops` holds four stops inline, so the common gradient
            // does not allocate; a longer one allocates once, as before.
            let stops = ColorStops(
                stops
                    .iter()
                    .map(|(t, col)| ColorStop {
                        offset: *t,
                        color: DynamicColor::from_alpha_color(srgb(*col)),
                    })
                    .collect(),
            );
            let mid = bounds.center();
            let g = match *kind {
                GradientKind::Linear { angle } => {
                    let a = angle.to_radians();
                    let (s, c) = (a.sin(), -a.cos());
                    let len = (bounds.width() * s).abs() + (bounds.height() * c).abs();
                    let half = (s * len / 2.0, c * len / 2.0);
                    Gradient::new_linear(
                        (mid.x - half.0, mid.y - half.1),
                        (mid.x + half.0, mid.y + half.1),
                    )
                }
                // Unit coordinates in, scene units out: the ramp travels with
                // the box instead of carrying pixels a layout has not solved.
                GradientKind::Radial { center, radius } => Gradient::new_radial(
                    (
                        bounds.x0 + center.0 * bounds.width(),
                        bounds.y0 + center.1 * bounds.height(),
                    ),
                    (radius * bounds.width().max(bounds.height())) as f32,
                ),
                // Radians, and clockwise from three o'clock: the quarter turn
                // puts zero at the top, where CSS `conic-gradient` starts it.
                // A whole turn, because a sweep that stopped short would
                // repeat its extend mode over the rest of the box.
                GradientKind::Conic { angle } => {
                    let from = (angle - 90.0).to_radians() as f32;
                    Gradient::new_sweep((mid.x, mid.y), from, from + std::f32::consts::TAU)
                }
            };
            PaintType::Gradient(g.with_stops(stops))
        }
    }
}

/// Draw every entry of the scene's paint list, in order, under `transform`.
///
/// Shadows take Vello's analytic blurred rectangle -- inverted, for an inset
/// one; a welded outline arrives as one such rect per welded child.
pub fn paint(
    canvas: &mut impl Canvas,
    scene: &ResolvedScene,
    transform: Affine,
) -> Result<(), Error> {
    // A CPU/sink Canvas must not silently omit external GPU paint. Use
    // effects::GpuRenderer for scenes containing native material surfaces.
    if scene.paint.iter().any(|p| p.layer == Layer::External) {
        return Err(Error::InvalidPath);
    }
    canvas.begin_frame();
    let mut bez = BezPath::new();
    for (i, p) in scene.paint.iter().enumerate() {
        if layered(canvas, p) {
            continue;
        }
        bez_path_into(&p.path, ARC_TOLERANCE, &mut bez)?;
        canvas.set_transform(placed(transform, p));
        if p.layer == Layer::Backdrop {
            backdrop(canvas, &scene.paint[..i], p, &bez, transform)?;
        } else {
            one(canvas, p, &bez)?;
        }
    }
    Ok(())
}

/// `base` moved to where `p` stands: its paths are local to
/// [`Painted::offset`].
pub(crate) fn placed(base: Affine, p: &Painted) -> Affine {
    match p.offset {
        o if o.x == 0. && o.y == 0. => base,
        o => base * Affine::translate((o.x, o.y)),
    }
}

/// `p`'s paint box where it stands in the scene.
pub(crate) fn scene_box(p: &Painted, path: &BezPath) -> Rect {
    paint_box(p, path) + kurbo::Vec2::new(p.offset.x, p.offset.y)
}

/// A [`Layer::Backdrop`]: `below` -- everything the list painted before it
/// -- painted again inside `outline`, through a blur layer.
///
/// `vello_cpu` cannot read back what it has drawn, so this is the whole
/// trick: the prefix again, convolved. The GPU renderer renders the same
/// [`replay`] into a texture of its own and blurs that instead; both pay
/// it when the list changes, not per presented frame.
// ponytail: a backdrop inside `below` is not blurred again in the replay
// (its dim still paints), so k modals cost k prefixes, not 2^k. External
// welds are skipped too: `paint` cannot draw them, and the retained renderer
// would need its texture ids here. Blur them when a weld sits under a modal.
fn backdrop(
    canvas: &mut impl Canvas,
    below: &[Painted],
    p: &Painted,
    outline: &BezPath,
    base: Affine,
) -> Result<(), Error> {
    // NaN and negatives say nothing, as a zero does.
    if p.blur.is_nan() || p.blur <= 0.0 {
        return Ok(());
    }
    // What the blur can pull in: three standard deviations past the outline.
    let reach = scene_box(p, outline).inflate(3. * p.blur, 3. * p.blur);
    if canvas.push_blur(outline, p.blur as f32) {
        replay(canvas, below, reach, base)?;
        canvas.pop_layer();
        canvas.pop_layer();
    }
    Ok(())
}

/// `below` painted again under `base` as far as it can reach `reach`, bar
/// what an open ancestor already applies.
pub(crate) fn replay(
    canvas: &mut impl Canvas,
    below: &[Painted],
    reach: Rect,
    base: Affine,
) -> Result<(), Error> {
    // A clip or layer the prefix opens and never closes is an ancestor's:
    // its clip already bounds this node and its layer composites it, so
    // replaying it would apply it twice -- an ancestor's opacity squared.
    let mut open = Vec::new();
    for (i, q) in below.iter().enumerate() {
        match q.layer {
            Layer::Clip | Layer::Blend { .. } => open.push(i),
            Layer::Unclip | Layer::Unblend => {
                open.pop();
            }
            _ => {}
        }
    }
    let mut ancestors = open.into_iter().peekable();
    let mut bez = BezPath::new();
    for (i, q) in below.iter().enumerate() {
        if ancestors.next_if_eq(&i).is_some()
            || matches!(q.layer, Layer::Backdrop | Layer::External)
            || layered(canvas, q)
        {
            continue;
        }
        bez_path_into(&q.path, ARC_TOLERANCE, &mut bez)?;
        // Only plain paint is culled: clips, masks and shadows reach
        // past their path's box, and are few.
        let plain = matches!(
            q.layer,
            Layer::Fill | Layer::Shell(_) | Layer::Stroke | Layer::Text | Layer::Draw(_)
        );
        let em = q.text.as_ref().map_or(0., |t| f64::from(t.size));
        let reaches = || {
            scene_box(q, &bez)
                .inflate(q.width + em, q.width + em)
                .overlaps(reach)
        };
        if !plain || reaches() {
            canvas.set_transform(placed(base, q));
            one(canvas, q, &bez)?;
        }
    }
    Ok(())
}

/// The box a gradient or image paint is fitted to. A text layer carries its
/// ink as glyphs and leaves `path` empty, so its box comes from the run.
// ponytail: the last glyph's own advance is estimated at the em size.
fn paint_box(p: &Painted, path: &BezPath) -> Rect {
    let Some(t) = &p.text else {
        return path.bounding_box();
    };
    let w = t
        .glyphs
        .iter()
        .map(|glyph| f64::from(glyph.x))
        .fold(0.0, f64::max)
        + f64::from(t.size);
    Rect::new(
        t.origin.x,
        t.origin.y - f64::from(t.size),
        t.origin.x + w,
        t.origin.y,
    )
}

/// The entries that carry no geometry: clip and layer bookkeeping. Handled
/// before any path conversion.
fn layered(canvas: &mut impl Canvas, p: &Painted) -> bool {
    match p.layer {
        Layer::Unclip => canvas.pop_clip(),
        Layer::Blend { mix: m, opacity } => canvas.push_layer(
            peniko::BlendMode::new(mix(m), peniko::Compose::SrcOver),
            opacity,
        ),
        Layer::Unblend => canvas.pop_layer(),
        _ => return false,
    }
    true
}

/// `mui-scene` mirrors `peniko::Mix` rather than depend on it; this match is
/// exhaustive, so a rename on either side fails the build.
fn mix(m: mui_scene::Mix) -> peniko::Mix {
    use mui_scene::Mix as M;
    match m {
        M::Normal => peniko::Mix::Normal,
        M::Multiply => peniko::Mix::Multiply,
        M::Screen => peniko::Mix::Screen,
        M::Overlay => peniko::Mix::Overlay,
        M::Darken => peniko::Mix::Darken,
        M::Lighten => peniko::Mix::Lighten,
        M::ColorDodge => peniko::Mix::ColorDodge,
        M::ColorBurn => peniko::Mix::ColorBurn,
        M::HardLight => peniko::Mix::HardLight,
        M::SoftLight => peniko::Mix::SoftLight,
        M::Difference => peniko::Mix::Difference,
        M::Exclusion => peniko::Mix::Exclusion,
        M::Hue => peniko::Mix::Hue,
        M::Saturation => peniko::Mix::Saturation,
        M::Color => peniko::Mix::Color,
        M::Luminosity => peniko::Mix::Luminosity,
    }
}

fn one(canvas: &mut impl Canvas, p: &Painted, path: &BezPath) -> Result<(), Error> {
    // Needs the list before it; see `backdrop`. A caller without one (a
    // tile, which cannot see past its edge) leaves the backdrop sharp.
    if p.layer == Layer::Backdrop {
        return Ok(());
    }
    if p.layer == Layer::Clip {
        canvas.push_clip(path);
        return Ok(());
    }
    if p.layer == Layer::Mask {
        // Source-atop inside the node's own blend layer: the fill lands
        // only where the subtree already painted.
        // ponytail: solid and gradient paints only -- an image mask would
        // need the paint-transform dance below, and nothing asks for one.
        canvas.push_layer(
            peniko::BlendMode::new(peniko::Mix::Normal, peniko::Compose::SrcAtop),
            1.0,
        );
        canvas.set_paint(brush(&p.paint, paint_box(p, path)));
        canvas.fill_path(path);
        canvas.pop_layer();
        return Ok(());
    }
    // Solid paint has no coordinate mapping; cubic extrema are wasted work.
    let bounds = if matches!(p.paint, Paint::Solid(_)) {
        Rect::ZERO
    } else {
        paint_box(p, path)
    };
    // A canvas that cannot take this image gets its solid stand-in rather
    // than a panic, and none of the paint-transform dance below.
    let (img, b) = match &p.paint {
        Paint::Image { image, fit } => match canvas.image(image) {
            Some(b) => (Some((image, *fit)), Some(b)),
            None => (None, None),
        },
        _ => (None, None),
    };
    let fill = match (&p.paint, b) {
        (_, Some(b)) => b,
        (Paint::Image { .. }, None) => PaintType::Solid(srgb(p.paint.solid())),
        _ => brush(&p.paint, bounds),
    };
    let solid = matches!(fill, PaintType::Solid(_));
    canvas.set_paint(fill);
    if let Some(t) = &p.text {
        canvas.glyphs(t);
        return Ok(());
    }
    // An image paint lives in pixel space; this is what puts it on the box.
    // `Extend::Pad` would smear the edge pixels across a letterbox, so
    // `Contain` also clips to the rectangle the image actually occupies.
    let clipped = img.and_then(|(image, fit)| {
        let t = image_transform(image, fit, bounds);
        canvas.set_paint_transform(t);
        (fit == Fit::Contain).then(|| {
            let r = t.transform_rect_bbox(Rect::new(
                0.,
                0.,
                f64::from(image.width),
                f64::from(image.height),
            ));
            canvas.push_clip(&r.to_path(0.1));
        })
    });
    match (p.blur > 0.0, p.rect, p.width > 0.0) {
        (true, Some(rr), _) => {
            // Both backends substitute opaque black for a non-solid brush
            // when they blur a rect, so a gradient shadow would paint as a
            // black blob. Its solid stand-in is what the author asked for.
            if !solid {
                canvas.set_paint(PaintType::Solid(srgb(p.paint.solid())));
            }
            let b = rr.bounds();
            canvas.fill_blurred_rounded_rect(
                &Rect::new(b.min.x, b.min.y, b.max.x, b.max.y),
                rr.radius() as f32,
                p.blur as f32,
                // The inverse coverage is the inset shadow; the walk has
                // already clipped it to the node's outline.
                matches!(p.layer, Layer::Shadow(ShadowKind::Inset)),
            );
        }
        // The walk emits a welded shadow as one blurred rect per child, so
        // this only catches a rect-less blur built by hand: dropped, because
        // the sharp fallback reads as a second, misaligned panel.
        (true, None, _) => {}
        (_, _, false) => canvas.fill_path(path),
        (_, _, true) => {
            canvas.set_stroke(Stroke::new(p.width));
            canvas.stroke_path(path);
        }
    }
    if clipped.is_some() {
        canvas.pop_clip();
    }
    if img.is_some() {
        canvas.reset_paint_transform();
    }
    Ok(())
}

#[cfg(test)]
mod seam {
    use super::*;
    use mui_geometry::Path;
    use mui_scene::{Image, Text};

    /// `Image`'s fields are public, so a caller can skip `Image::rgba` and
    /// hand over a buffer that does not match the size. Vello's `Pixmap`
    /// asserts on that; the paint falls back instead.
    #[test]
    fn an_image_whose_buffer_does_not_match_its_size_falls_back() {
        let image = Image {
            width: 4,
            height: 4,
            rgba: Arc::from(&[0u8, 0, 0, 255][..]),
            texture: None,
        };
        assert!(premultiply(&image).is_none());
    }

    /// A font unused for `FONT_FRAMES` frames lets go of its `FontData`; one
    /// drawn every frame keeps it, blob id and all.
    #[test]
    fn an_unused_font_ages_out() {
        let font = |bytes| mui_scene::Font::new(bytes).unwrap();
        let (kept, dropped) = (
            font(epaint_default_fonts::HACK_REGULAR),
            font(epaint_default_fonts::UBUNTU_LIGHT),
        );
        let mut cache = Cache::default();
        cache.tick();
        let (first, warm) = cache.font(&kept);
        assert!(!warm && !cache.font(&kept).1, "warm in its first frame");
        cache.font(&dropped);
        for _ in 0..FONT_FRAMES {
            cache.tick();
            assert!(cache.font(&kept).1, "cold after its first frame");
        }
        assert_eq!(cache.fonts.len(), 2, "aged out early");
        cache.tick();
        assert_eq!(cache.fonts.len(), 1, "the idle font is still held");
        assert_eq!(cache.font(&kept).0.data.id(), first.data.id());
    }

    /// A gradient-filled label still gets a box to fit the gradient to,
    /// although its path is empty.
    #[test]
    fn a_text_layer_takes_its_brush_box_from_the_run() {
        let mut p = Painted {
            key: "t".into(),
            layer: Layer::Text,
            path: Path::default().into(),
            paint: Paint::Solid(mui_scene::Color::oklch(0.5, 0., 0.)),
            rect: None,
            offset: Default::default(),
            width: 0.,
            blur: 0.,
            text: Some(Text {
                fonts: Arc::from([
                    mui_scene::Font::new(epaint_default_fonts::HACK_REGULAR).unwrap()
                ]),
                size: 16.,
                origin: mui_geometry::Point::new(10., 30.),
                glyphs: Arc::from(
                    &[
                        mui_scene::TextGlyph {
                            id: 1,
                            x: 0.,
                            y: 0.,
                            font: 0,
                        },
                        mui_scene::TextGlyph {
                            id: 2,
                            x: 12.,
                            y: 0.,
                            font: 0,
                        },
                    ][..],
                ),
                axes: Default::default(),
                hint: true,
                font_coords: Arc::from(&[][..]),
            }),
        };
        let b = paint_box(&p, &BezPath::new());
        assert!(b.width() > 0. && b.height() > 0., "{b:?}");
        assert_eq!((b.x0, b.y1), (10., 30.));
        p.text = None;
        assert_eq!(paint_box(&p, &BezPath::new()), Rect::ZERO);
    }
}

#[cfg(all(test, feature = "cpu"))]
mod snapshot {
    use super::*;
    use mui_scene::{prelude::*, ResolvedScene, TextGlyph};
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
        spec.font = Some(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
        let scene = resolve_scene(&spec).unwrap();
        assert!(scene
            .paint
            .iter()
            .any(|p| p.layer == mui_scene::Layer::Text));

        let mut ctx = vello_cpu::RenderContext::new(120, 60);
        let mut res = vello_cpu::Resources::default();
        paint(
            &mut Cpu {
                ctx: &mut ctx,
                resources: &mut res,
                cache: &mut Cache::default(),
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
        pixels_scene(&scene, w, h)
    }

    fn pixels_scene(scene: &ResolvedScene, w: u16, h: u16) -> Pixmap {
        let mut ctx = vello_cpu::RenderContext::new(w, h);
        let mut res = vello_cpu::Resources::default();
        paint(
            &mut Cpu {
                ctx: &mut ctx,
                resources: &mut res,
                cache: &mut Cache::default(),
            },
            scene,
            Affine::IDENTITY,
        )
        .unwrap();
        ctx.flush();
        let mut pix = Pixmap::new(w, h);
        ctx.render(&mut pix, &mut res);
        pix
    }

    /// Text reaches the pixels as a glyph run: Vello hints and caches the
    /// outlines per font blob, so ink on the pixmap means the run drew.
    #[test]
    fn a_glyph_run_lands_pixels() {
        let mut spec =
            SceneSpec::new(text("HI").fill(Role::Ink).id("t")).offered(Size::new(80., 40.));
        spec.font = Some(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
        let scene = resolve_scene(&spec).unwrap();
        assert!(
            scene.paint.iter().any(|p| p.text.is_some()),
            "no glyphs to draw"
        );
        let pix = pixels(&spec, 80, 40);
        assert!(pix.data().iter().any(|p| p.a > 0), "the run drew nothing");
    }

    #[test]
    fn a_gpos_mark_is_rendered_at_its_shaped_y_offset() {
        let mut spec =
            SceneSpec::new(text("ש\u{05b8}").fill(Role::Ink).id("t")).offered(Size::new(80., 40.));
        spec.font = Some(Font::new(ttf_inter::REGULAR).unwrap());
        let scene = resolve_scene(&spec).unwrap();
        let text = scene
            .paint
            .iter()
            .find_map(|p| p.text.as_ref())
            .expect("text layer");
        assert!(
            text.glyphs.iter().any(|glyph| glyph.y.abs() > 0.01),
            "scene dropped GPOS y offsets: {:?}",
            text.glyphs
        );
        let mut without_offsets = scene.clone();
        for painted in &mut without_offsets.paint {
            let Some(text) = painted.text.as_mut() else {
                continue;
            };
            text.glyphs = text
                .glyphs
                .iter()
                .map(|glyph| TextGlyph { y: 0., ..*glyph })
                .collect();
        }
        let positioned = pixels_scene(&scene, 80, 40);
        let flattened = pixels_scene(&without_offsets, 80, 40);
        assert_ne!(
            positioned.data(),
            flattened.data(),
            "mark offset had no raster effect"
        );
    }

    #[test]
    fn a_missing_primary_glyph_uses_the_selected_fallback_font() {
        let root = text("A😀").fill(Role::Ink).id("t");
        let mut with_fallback = SceneSpec::new(root.clone()).offered(Size::new(100., 40.));
        with_fallback.font = Some(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
        with_fallback
            .fallback_fonts
            .push(Font::new(epaint_default_fonts::NOTO_EMOJI_REGULAR).unwrap());
        let fallback_scene = resolve_scene(&with_fallback).unwrap();
        let glyphs = fallback_scene
            .paint
            .iter()
            .find_map(|p| p.text.as_ref())
            .expect("text layer")
            .glyphs
            .clone();
        assert!(
            glyphs.iter().any(|glyph| glyph.font == 1),
            "no fallback glyph was selected"
        );

        let mut primary_only = with_fallback.clone();
        primary_only.fallback_fonts.clear();
        let primary_scene = resolve_scene(&primary_only).unwrap();
        assert_ne!(
            pixels_scene(&fallback_scene, 100, 40).data(),
            pixels_scene(&primary_scene, 100, 40).data(),
            "fallback output equals the primary .notdef output"
        );
    }

    /// A gradient shadow keeps its alpha. Both backends paint opaque black
    /// for a non-solid brush on a blurred rect, so the brush is collapsed to
    /// its solid stand-in first.
    #[test]
    fn a_gradient_shadow_does_not_paint_black() {
        let faint = mui_scene::Color::oklcha(0.0, 0.0, 0.0, 0.1);
        let root = leaf(20., 20.)
            .fill(Role::Primary)
            .shadow(Shadow {
                dy: 8.,
                fill: mui_scene::Gradient::vertical(faint, faint.with_alpha(0.0)).into(),
                ..Shadow::soft(4.)
            })
            .id("card");
        let pix = pixels(&SceneSpec::new(root).offered(Size::new(20., 20.)), 40, 40);
        let below = pix.data()[30 * 40 + 10];
        assert!(below.a < 40, "the shadow went opaque black: {below:?}");
    }

    /// A welded outline's shadow is the union of its children's blurs: soft
    /// under the weld, falling off with distance, and nowhere near the
    /// outline's own alpha -- which is what a sharp copy would have given.
    #[test]
    fn a_welded_shadow_blurs() {
        let root = row([leaf(20., 20.).id("a"), leaf(20., 40.).id("b")])
            .union(Role::Surface)
            .shadow(Shadow::soft(12.))
            .id("weld");
        let spec = SceneSpec::new(root).offered(Size::new(40., 40.));
        let pix = pixels(&spec, 60, 80);
        let a = |y: usize| pix.data()[y * 60 + 10].a;
        assert!(a(29) > 0, "the outline itself is gone");
        let (near, far) = (a(34), a(46));
        assert!(near > 0, "the welded shadow is still dropped");
        assert!(far < near, "it does not fall off: {near} then {far}");
        assert!(near < a(29) / 2, "that is a sharp copy, not a blur: {near}");
    }

    /// A multiply layer darkens what is under it. The child is the same grey
    /// as the ground, so painting it straight would leave the pixel alone:
    /// only the blend can make it darker.
    #[test]
    fn a_multiply_layer_darkens_what_is_under_it() {
        let grey = Color::oklcha(0.7, 0., 0., 1.);
        let root = overlay([
            leaf(40., 40.).fill(grey).radius(0.).id("ground"),
            leaf(20., 20.)
                .fill(grey)
                .radius(0.)
                .blend(Mix::Multiply)
                .id("dim"),
        ])
        .id("root");
        let pix = pixels(&SceneSpec::new(root).offered(Size::new(40., 40.)), 40, 40);
        let at = |x: usize, y: usize| pix.data()[y * 40 + x];
        let (ground, inside) = (at(2, 2), at(20, 20));
        assert!(ground.r > 100, "no ground to darken: {ground:?}");
        assert!(
            inside.r < ground.r - 20,
            "the same grey did not multiply: {inside:?} vs {ground:?}"
        );
        assert_eq!(at(38, 38), ground, "the layer leaked outside its node");
    }

    /// A 2x2 image stretched over a leaf: each source pixel owns a quadrant,
    /// and the leaf's rounded corner still cuts the image away.
    #[test]
    fn an_image_fill_lands_the_right_pixel_in_each_quadrant() {
        #[rustfmt::skip]
        let px: Vec<u8> = vec![
            255, 0, 0, 255,  0, 255, 0, 255,
            0, 0, 255, 255,  255, 255, 255, 255,
        ];
        let img = std::sync::Arc::new(mui_scene::Image::rgba(2, 2, px).unwrap());
        let root = leaf(20., 20.)
            .fill(Fill::Image(img.clone(), Fit::Fill))
            .radius(6.)
            .id("img");
        let pix = pixels(&SceneSpec::new(root).offered(Size::new(20., 20.)), 20, 20);
        let at = |x: usize, y: usize| pix.data()[y * 20 + x];
        // Nearest neighbour is not promised, so sample well inside a quadrant.
        for ((x, y), want) in [
            ((4, 4), [255, 0, 0]),
            ((15, 4), [0, 255, 0]),
            ((4, 15), [0, 0, 255]),
            ((15, 15), [255, 255, 255]),
        ] {
            let got = at(x, y);
            assert_eq!([got.r, got.g, got.b], want, "at {x},{y}: {got:?}");
        }
        assert_eq!(at(0, 0).a, 0, "the image spilled past the rounded corner");

        // Contain letterboxes rather than smearing the edge pixels: a 2x2
        // image in a 40x20 box leaves the sides clear.
        let wide = leaf(40., 20.)
            .fill(Fill::Image(img, Fit::Contain))
            .radius(0.)
            .id("wide");
        let pix = pixels(&SceneSpec::new(wide).offered(Size::new(40., 20.)), 40, 20);
        let at = |x: usize, y: usize| pix.data()[y * 40 + x];
        assert_eq!(at(1, 10).a, 0, "contain smeared into the letterbox");
        assert!(
            at(14, 5).r > 200,
            "the image itself is missing: {:?}",
            at(14, 5)
        );
    }

    /// The CPU path lets go of a buffer the app has dropped: its pixmap goes
    /// on the next image lookup, so a panel handing over a fresh frame every
    /// frame does not keep one per frame.
    #[test]
    fn the_pixmap_cache_drops_a_buffer_the_app_let_go_of() {
        let img = |v: u8| mui_scene::Image::rgba(1, 1, vec![v, v, v, 255]).unwrap();
        let (a, b) = (img(1), img(2));
        let gone = Arc::downgrade(&a.rgba);
        let mut ctx = vello_cpu::RenderContext::new(1, 1);
        let mut resources = vello_cpu::Resources::default();
        let mut cache = Cache::default();
        let mut cpu = Cpu {
            ctx: &mut ctx,
            resources: &mut resources,
            cache: &mut cache,
        };
        assert!(cpu.image(&a).is_some());
        assert!(cpu.image(&a).is_some());
        assert_eq!(cpu.cache.images.len(), 1, "a hit added an entry");
        drop(a);
        assert!(gone.upgrade().is_none(), "the cache kept the buffer alive");
        assert!(cpu.image(&b).is_some());
        assert_eq!(
            cpu.cache.images.len(),
            1,
            "the dropped buffer kept its entry"
        );
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
