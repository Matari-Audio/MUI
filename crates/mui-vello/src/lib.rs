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
#![deny(unsafe_code)]
// The shared session's wgpu resource graph needs deeper auto-trait evaluation
// with the newer Rust trait solver.
#![recursion_limit = "256"]

use kurbo::{Affine, BezPath, Rect, Shape as _, Stroke};
use mui_geometry::Error;
use mui_scene::{Fit, GradientKind, Layer, Paint, Painted, ResolvedScene, ShadowKind};
use std::sync::{Arc, Weak};
#[cfg(feature = "cpu")]
use vello_common::filter_effects::{EdgeMode, Filter, FilterPrimitive};
/// The brush type [`Canvas::set_paint`] takes, so the trait can be
/// implemented outside this crate.
pub use vello_common::paint::PaintType;
#[cfg(feature = "cpu")]
use vello_common::peniko::ImageSampler;
use vello_common::peniko::color::{AlphaColor, DynamicColor, Srgb};
use vello_common::peniko::{Blob, ColorStop, ColorStops, FontData, Gradient};
use vello_common::pixmap::{PixelMetadata, Pixmap};
pub use vello_common::{kurbo, peniko};
#[cfg(feature = "cpu")]
mod damage;
pub mod diagnostics;
#[cfg(feature = "cpu")]
pub use vello_cpu;
#[cfg(feature = "gpu-quad")]
pub mod effects;
#[cfg(feature = "gpu-quad")]
pub mod host;
#[cfg(feature = "gpu-quad")]
pub mod quad;
#[cfg(feature = "cpu")]
pub mod software;

/// The path conversion painting uses, the same one input hit-tests with.
pub use mui_geometry::{ARC_TOLERANCE, bez_path, bez_path_into};

#[cfg(test)]
mod tests;

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
    /// stand-in. `vello_cpu` takes the pixmap itself. A canvas with no image support keeps this
    /// default and paints the stand-in.
    fn image(&mut self, _img: &mui_scene::Image) -> Option<PaintType> {
        None
    }
    fn set_stroke(&mut self, s: Stroke);
    fn fill_path(&mut self, p: &BezPath);
    /// Explicit winding for imported vector artwork.
    fn fill_path_with_rule(&mut self, p: &BezPath, rule: peniko::Fill) -> bool {
        if rule != peniko::Fill::NonZero {
            return false;
        }
        self.fill_path(p);
        true
    }
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
    /// Fallible glyph painting. Renderers with fallible glyph APIs override this;
    /// existing canvases retain their infallible contract by default.
    fn try_glyphs(&mut self, text: &mui_scene::Text) -> Result<(), Error> {
        self.glyphs(text);
        Ok(())
    }
}

#[cfg(any(feature = "cpu", feature = "gpu-quad"))]
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

#[cfg_attr(
    not(any(feature = "cpu", feature = "gpu-quad", test)),
    expect(dead_code, reason = "no canvas feature, nothing keeps fonts or images")
)]
#[derive(Clone, Copy)]
struct Frames {
    born: u64,
    used: u64,
}

/// How long an unused font keeps its [`FontData`], and with it Vello's
/// hinted outlines: glifo's own glyph atlas ages entries out after as many.
#[cfg_attr(
    not(any(feature = "cpu", feature = "gpu-quad", test)),
    expect(dead_code, reason = "no canvas feature, nothing keeps fonts or images")
)]
const FONT_FRAMES: u64 = 64;

enum Stored {
    /// Only the CPU canvas stores pixmaps.
    #[cfg_attr(
        not(feature = "cpu"),
        expect(dead_code, reason = "only the CPU canvas builds it")
    )]
    Pixmap(Arc<Pixmap>),
    /// The quad pass's image texture, bound as its own group.
    #[cfg(feature = "gpu-quad")]
    Texture(mui_hal::BindGroup),
}

#[cfg_attr(
    not(any(feature = "cpu", feature = "gpu-quad")),
    expect(dead_code, reason = "no canvas feature, nothing keeps fonts or images")
)]
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
        let f = FontData::new(Blob::new(Arc::new(font.clone())), font.index());
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
        fn fill_path_with_rule(&mut self, p: &BezPath, rule: peniko::Fill) -> bool {
            self.$inner.set_fill_rule(rule);
            self.$inner.fill_path(p);
            self.$inner.set_fill_rule(peniko::Fill::NonZero);
            true
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
            self.$inner.pop_clip()
        }
        fn push_layer(&mut self, blend: peniko::BlendMode, opacity: f32) {
            self.$inner
                .push_layer(None, Some(blend), Some(opacity), None, None)
        }
        fn pop_layer(&mut self) {
            self.$inner.pop_layer()
        }
        fn glyphs(&mut self, text: &mui_scene::Text) {
            // Keep the legacy void API; paint() uses the fallible path below.
            let _ = self.try_glyphs(text);
        }
        fn try_glyphs(&mut self, text: &mui_scene::Text) -> Result<(), Error> {
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
                    .fill_glyphs(run(text.origin, &text.glyphs[start..end]))
                    .map_err(|error| {
                        diagnostics::error("mui-vello", "glyph_render_failed", &error.to_string());
                        Error::InvalidPath
                    })?;
                start = end;
            }
            Ok(())
        }
    };
}

#[cfg(feature = "cpu")]
impl Canvas for Cpu<'_> {
    fn image(&mut self, img: &mui_scene::Image) -> Option<PaintType> {
        self.cache.sweep();
        let p = if let Some(Stored::Pixmap(p)) = self.cache.find(&img.rgba) {
            p.clone()
        } else {
            let p = Arc::new(premultiply(img)?);
            self.cache.remember(&img.rgba, Stored::Pixmap(p.clone()));
            p
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
#[cfg_attr(
    not(any(test, feature = "cpu", feature = "gpu-quad")),
    expect(
        dead_code,
        reason = "only the CPU renderer, the quad pass and tests call it"
    )
)]
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
    // `Pixmap::from_parts` asserts that it does.
    (img.rgba.len() == usize::from(w) * usize::from(h) * 4).then_some((w, h))
}

/// [`premultiply`] for a size [`size`] already checked.
fn premultiplied(img: &mui_scene::Image, w: u16, h: u16) -> Pixmap {
    let mut clear = false;
    let mut data = Vec::with_capacity(img.rgba.len());
    for p in img.rgba.as_chunks::<4>().0 {
        clear |= p[3] != 255;
        let m = |c: u8| ((u16::from(p[3]) * u16::from(c)) / 255) as u8;
        data.extend_from_slice(&[m(p[0]), m(p[1]), m(p[2]), p[3]]);
    }
    Pixmap::from_parts(
        data,
        w,
        h,
        PixelMetadata::new(peniko::ImageAlphaType::AlphaPremultiplied, clear),
    )
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
        Paint::Image { .. } | Paint::Vector { .. } => PaintType::Solid(srgb(p.solid())),
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
    paint_within(canvas, scene, transform, None)
}

/// [`paint`], skipping every leaf whose reach misses the device rect
/// `view`. Clips, masks and layers are kept so their pushes and pops pair.
pub(crate) fn paint_within(
    canvas: &mut impl Canvas,
    scene: &ResolvedScene,
    transform: Affine,
    view: Option<Rect>,
) -> Result<(), Error> {
    // A CPU/sink Canvas must not silently omit external GPU paint. Use
    // effects::GpuRenderer for scenes containing native material surfaces.
    if scene.paint.iter().any(|p| p.layer == Layer::External) {
        return Err(Error::InvalidPath);
    }
    canvas.begin_frame();
    let mut bez = BezPath::new();
    for (i, p) in scene.paint.iter().enumerate() {
        if layered(canvas, p) || view.is_some_and(|v| misses(p, transform, v)) {
            continue;
        }
        bez_path_into(&p.path, ARC_TOLERANCE, &mut bez)?;
        canvas.set_transform(placed(transform, p));
        if p.layer == Layer::Backdrop {
            backdrop(canvas, &scene.paint[..i], p, &bez, transform)?;
        } else if path_shadow(p) {
            blurred_path(canvas, p, &bez)?;
        } else {
            one(canvas, p, &bez, placed(transform, p))?;
        }
    }
    Ok(())
}

/// `span` alone -- one leaf, or a clip with everything up to its `Unclip`
/// -- rasterised under `transform` and cropped to where it can paint inside
/// a `size` device frame: straight RGBA and its device rect `[x, y, w, h]`.
/// `None` when it reaches nothing on the frame or cannot be drawn apart from
/// the list (an inset shadow, a backdrop, a weld, bookkeeping alone).
#[cfg(all(feature = "cpu", feature = "gpu-quad"))]
pub(crate) fn raster_alone(
    span: &[Painted],
    transform: Affine,
    size: [u32; 2],
    resources: &mut vello_cpu::Resources,
    cache: &mut Cache,
) -> Result<Option<(mui_scene::Image, [u32; 4])>, Error> {
    let first = span.first().ok_or(Error::InvalidPath)?;
    let leaf = matches!(
        first.layer,
        Layer::Fill | Layer::Shell(_) | Layer::Stroke | Layer::Text | Layer::Draw(_)
    ) || path_shadow(first);
    let clip = first.layer == Layer::Clip;
    if !(leaf && span.len() == 1 || clip)
        || span
            .iter()
            .any(|p| matches!(p.layer, Layer::Backdrop | Layer::External))
    {
        return Ok(None);
    }
    // A clip bounds everything inside it.
    let Some(reach) = damage::reach(first).filter(|r| *r != damage::NOTHING) else {
        return Ok(None);
    };
    let frame = Rect::new(0., 0., f64::from(size[0]), f64::from(size[1]));
    let r = transform
        .transform_rect_bbox(reach)
        .inflate(2., 2.)
        .intersect(frame)
        .expand();
    if r.width() < 1. || r.height() < 1. {
        return Ok(None);
    }
    let (x, y, w, h) = (
        r.x0 as u32,
        r.y0 as u32,
        r.width() as u16,
        r.height() as u16,
    );
    let base = Affine::translate((-r.x0, -r.y0)) * transform;
    let mut ctx = vello_cpu::RenderContext::new(w, h);
    let mut canvas = Cpu {
        ctx: &mut ctx,
        resources,
        cache,
    };
    canvas.begin_frame();
    let mut bez = BezPath::new();
    for p in span {
        if layered(&mut canvas, p) {
            continue;
        }
        bez_path_into(&p.path, ARC_TOLERANCE, &mut bez)?;
        let at = placed(base, p);
        canvas.set_transform(at);
        if path_shadow(p) {
            blurred_path(&mut canvas, p, &bez)?;
        } else {
            one(&mut canvas, p, &bez, at)?;
        }
    }
    ctx.flush();
    let mut pixels = vello_cpu::Pixmap::new(w, h);
    ctx.render(&mut pixels, resources);
    let image = mui_scene::Image {
        width: u32::from(w),
        height: u32::from(h),
        rgba: pixels.take_rgba8(peniko::ImageAlphaType::Alpha).into(),
        texture: None,
    };
    Ok(Some((image, [x, y, u32::from(w), u32::from(h)])))
}

/// Whether leaf `p` paints nothing inside device rect `view`. Only the
/// CPU damage pass culls; without it nothing is skipped.
#[cfg(not(feature = "cpu"))]
fn misses(_: &Painted, _: Affine, _: Rect) -> bool {
    false
}

/// Whether leaf `p` paints nothing inside device rect `view`.
#[cfg(feature = "cpu")]
fn misses(p: &Painted, transform: Affine, view: Rect) -> bool {
    !matches!(p.layer, Layer::Clip | Layer::Mask)
        && damage::reach(p).is_some_and(|r| {
            r == damage::NOTHING
                || !transform
                    .transform_rect_bbox(r)
                    .inflate(2., 2.)
                    .overlaps(view)
        })
}

/// `base` moved to where `p` stands: its paths are local to
/// [`Painted::offset`].
pub(crate) fn placed(base: Affine, p: &Painted) -> Affine {
    let base = base * p.transform;
    match p.offset {
        o if o.x == 0. && o.y == 0. => base,
        o => base * Affine::translate((o.x, o.y)),
    }
}

/// `p`'s paint box where it stands in the scene.
pub(crate) fn scene_box(p: &Painted, path: &BezPath) -> Rect {
    p.transform
        .transform_rect_bbox(paint_box(p, path) + kurbo::Vec2::new(p.offset.x, p.offset.y))
}

/// A drop shadow with no rounded rect to blur analytically: a custom
/// outline's, which the walk hands over as the path itself, or a canvas's
/// `Draw::shadow` of any other path.
pub(crate) fn path_shadow(p: &Painted) -> bool {
    matches!(p.layer, Layer::Shadow(ShadowKind::Drop) | Layer::Draw(_))
        && p.rect.is_none()
        && p.blur > 0.0
}

/// How far past its path a blur of `p` reaches: three standard deviations,
/// where Vello's own blurred rect stops too.
pub(crate) fn blur_reach(p: &Painted, path: &BezPath) -> Rect {
    use kurbo::Shape;
    path.bounding_box().inflate(3. * p.blur, 3. * p.blur)
}

/// A [`path_shadow`]: the path filled sharp inside a blur layer. On a canvas
/// without filter layers this returns an explicit unsupported-options error.
fn blurred_path(canvas: &mut impl Canvas, p: &Painted, path: &BezPath) -> Result<(), Error> {
    if canvas.push_blur(&blur_reach(p, path).to_path(0.1), p.blur as f32) {
        canvas.set_paint(PaintType::Solid(srgb(p.paint.solid())));
        canvas.fill_path(path);
        canvas.pop_layer();
        canvas.pop_layer();
        Ok(())
    } else {
        Err(Error::InvalidOptions(
            "canvas does not support blurred path shadows",
        ))
    }
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
        ) && q.blur == 0.0;
        let em = q.text.as_ref().map_or(0., |t| f64::from(t.size));
        // Twice the width: a miter join reaches four half widths out.
        let reaches = || {
            scene_box(q, &bez)
                .inflate(2. * q.width + em, 2. * q.width + em)
                .overlaps(reach)
        };
        if !plain || reaches() {
            canvas.set_transform(placed(base, q));
            if path_shadow(q) {
                blurred_path(canvas, q, &bez)?;
            } else {
                one(canvas, q, &bez, placed(base, q))?;
            }
        }
    }
    Ok(())
}

/// The box a gradient or image paint is fitted to. A text layer carries its
/// ink as glyphs and leaves `path` empty, so its box comes from the run.
// ponytail: the last glyph's own advance is estimated at the em size.
pub(crate) fn paint_box(p: &Painted, path: &BezPath) -> Rect {
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

/// Replay retained vector paths under the element and device transforms.
fn vector_paint(
    canvas: &mut impl Canvas,
    vector: &mui_scene::Vector,
    fit: Fit,
    outline: &BezPath,
    base: Affine,
) -> Result<(), Error> {
    use mui_scene::VectorCommand;
    let bounds = outline.bounding_box();
    let sx = bounds.width() / vector.width;
    let sy = bounds.height() / vector.height;
    let (sx, sy) = match fit {
        Fit::Fill => (sx, sy),
        Fit::Contain => {
            let s = sx.min(sy);
            (s, s)
        }
        Fit::Cover => {
            let s = sx.max(sy);
            (s, s)
        }
    };
    let at =
        base * Affine::translate((
            bounds.x0 + (bounds.width() - vector.width * sx) * 0.5,
            bounds.y0 + (bounds.height() - vector.height * sy) * 0.5,
        )) * Affine::scale_non_uniform(sx, sy);
    if let Some(source) = vector.raster_source() {
        // Request physical pixels, including node fit, scene transforms and DPI.
        // Rotation changes sampling axes, not the source's authored geometry.
        let [a, b, c, d, _, _] = at.as_coeffs();
        let w = (vector.width * a.hypot(b)).ceil();
        let h = (vector.height * c.hypot(d)).ceil();
        if ![w, h]
            .into_iter()
            .all(|n| n.is_finite() && n > 0. && n <= u32::MAX as f64)
        {
            return Err(Error::InvalidPath);
        }
        let Some(image) = source
            .prepared_image(w as u32, h as u32)
            .map_err(|_| Error::InvalidPath)?
        else {
            return Ok(());
        };
        let paint = canvas.image(&image).ok_or(Error::InvalidPath)?;
        canvas.set_transform(base);
        canvas.push_clip(outline);
        canvas.set_transform(at);
        canvas.set_paint(paint);
        canvas.set_paint_transform(Affine::scale_non_uniform(
            vector.width / image.width as f64,
            vector.height / image.height as f64,
        ));
        canvas.fill_path(&Rect::new(0., 0., vector.width, vector.height).to_path(0.01));
        canvas.reset_paint_transform();
        canvas.pop_clip();
        canvas.set_transform(base);
        return Ok(());
    }
    canvas.set_transform(base);
    canvas.push_clip(outline);
    let mut layers = 0;
    let result = (|| {
        for command in vector.commands() {
            match command {
                VectorCommand::Fill {
                    path,
                    transform,
                    brush,
                    brush_transform,
                    rule,
                } => {
                    canvas.set_transform(at * *transform);
                    canvas.set_paint(vector_brush(brush)?);
                    canvas.set_paint_transform(*brush_transform);
                    if !canvas.fill_path_with_rule(path, *rule) {
                        return Err(Error::InvalidPath);
                    }
                    canvas.reset_paint_transform();
                }
                VectorCommand::Stroke {
                    path,
                    transform,
                    brush,
                    brush_transform,
                    stroke,
                } => {
                    canvas.set_transform(at * *transform);
                    canvas.set_paint(vector_brush(brush)?);
                    canvas.set_paint_transform(*brush_transform);
                    canvas.set_stroke(stroke.clone());
                    canvas.stroke_path(path);
                    canvas.reset_paint_transform();
                }
                VectorCommand::PushLayer {
                    path,
                    transform,
                    blend,
                    alpha,
                } => {
                    canvas.set_transform(at * *transform);
                    canvas.push_clip(path);
                    canvas.push_layer(*blend, *alpha);
                    layers += 1;
                }
                VectorCommand::PopLayer => {
                    canvas.pop_layer();
                    canvas.pop_clip();
                    layers -= 1;
                }
            }
        }
        Ok(())
    })();
    for _ in 0..layers {
        canvas.pop_layer();
        canvas.pop_clip();
    }
    canvas.reset_paint_transform();
    canvas.pop_clip();
    canvas.set_transform(base);
    result
}
fn vector_brush(brush: &peniko::Brush) -> Result<PaintType, Error> {
    match brush {
        peniko::Brush::Solid(c) => Ok(PaintType::Solid(*c)),
        peniko::Brush::Gradient(g) => Ok(PaintType::Gradient(g.clone())),
        peniko::Brush::Image(_) => Err(Error::InvalidPath),
    }
}

fn one(
    canvas: &mut impl Canvas,
    p: &Painted,
    path: &BezPath,
    transform: Affine,
) -> Result<(), Error> {
    if p.layer == Layer::AlphaMask {
        // Composite the complete mask, including vector command overlap and
        // transparent regions, against the isolated subtree in one operation.
        canvas.push_layer(
            peniko::BlendMode::new(peniko::Mix::Normal, peniko::Compose::DestIn),
            1.0,
        );
        let result = paint_one(canvas, p, path, transform);
        canvas.pop_layer();
        return result;
    }
    paint_one(canvas, p, path, transform)
}

fn paint_one(
    canvas: &mut impl Canvas,
    p: &Painted,
    path: &BezPath,
    transform: Affine,
) -> Result<(), Error> {
    if let Paint::Vector { vector, fit, .. } = &p.paint {
        return vector_paint(canvas, vector, *fit, path, transform);
    }
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
        // Source-atop tints the subtree inside its isolated layer.
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
        canvas.try_glyphs(t)?;
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
                &b,
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
            canvas.set_stroke(Stroke::new(p.width).with_caps(p.cap).with_join(p.join));
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
mod seam;

#[cfg(all(test, feature = "cpu"))]
mod snapshot;

// Reuse the adapter/pixel contracts in targeted --lib validation too.
#[cfg(all(test, feature = "gpu-quad"))]
extern crate self as mui_vello;
#[cfg(all(test, feature = "gpu-quad"))]
#[path = "../tests/effects_smoke.rs"]
mod retained_gpu_contract;
