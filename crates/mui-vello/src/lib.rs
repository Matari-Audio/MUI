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
use mui_core::{Fit, Layer, Paint, Painted, ResolvedScene};
use mui_geometry::{Error, Path, PathCommand};
use std::sync::{Arc, Mutex};
/// The brush type [`Canvas::set_paint`] takes, so the trait can be
/// implemented outside this crate.
pub use vello_common::paint::PaintType;
use vello_common::peniko::color::PremulRgba8;
use vello_common::peniko::color::{AlphaColor, DynamicColor, Srgb};
use vello_common::peniko::{Blob, ColorStop, FontData, Gradient, ImageSampler};
use vello_common::pixmap::Pixmap;
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
    /// Where the current paint's own space lands, composed after the scene
    /// transform. Image space is pixels; gradients and solids ignore it, so
    /// a canvas that never paints an image may leave both of these alone.
    fn set_paint_transform(&mut self, _t: Affine) {}
    fn reset_paint_transform(&mut self) {}
    /// This image as a brush, or `None` for [`mui_core::Paint::solid`]'s
    /// stand-in. `vello_cpu` takes the pixmap itself; `vello_hybrid` wants
    /// an atlas id and *panics* on a pixmap, so [`Gpu`] uploads through its
    /// [`Atlas`] and says `None` without one.
    fn image(&mut self, img: &mui_core::Image) -> Option<PaintType> {
        pixmap(img).map(|p| {
            vello_common::paint::Image {
                image: vello_common::paint::ImageSource::Pixmap(p),
                sampler: ImageSampler::default(),
            }
            .into()
        })
    }
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

/// A `vello_hybrid` scene together with the resources its glyph cache lives
/// in, and optionally the [`Atlas`] that lets image fills reach the GPU.
pub struct Gpu<'a> {
    pub scene: &'a mut vello_hybrid::Scene,
    pub resources: &'a mut vello_hybrid::Resources,
    pub atlas: Option<Atlas<'a>>,
}

/// What uploading an image into `vello_hybrid`'s atlas takes: the renderer
/// that owns the texture, the device and queue to write it with, and the
/// host-owned [`ImageIds`] that remember what has been uploaded already.
pub struct Atlas<'a> {
    pub renderer: &'a mut vello_hybrid::Renderer,
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub ids: &'a mut ImageIds,
}

/// One atlas id per image buffer a renderer has seen. Keep it next to the
/// `Renderer` it belongs to: an id means nothing to any other one.
// ponytail: never evicted, like `FONTS`; an entry holds its buffer so the
// address stays a sound key. `Renderer::destroy_image` is the upgrade if a
// plugin ever streams images through.
#[derive(Default)]
pub struct ImageIds(Vec<(Arc<[u8]>, vello_common::paint::ImageId, bool)>);

macro_rules! wrapper {
    ($inner:ident) => {
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
    };
}

impl Canvas for Gpu<'_> {
    fn image(&mut self, img: &mui_core::Image) -> Option<PaintType> {
        let atlas = self.atlas.as_mut()?;
        let (id, clear) = match atlas.ids.0.iter().find(|(k, ..)| Arc::ptr_eq(k, &img.rgba)) {
            Some(&(_, id, clear)) => (id, clear),
            None => {
                // Own encoder, submitted now: the queue keeps it ahead of the
                // frame that paints with the id. An upload is once per image.
                // ponytail: an image wider than the atlas panics inside
                // `upload_image`; `pixmap` only guards the u16 ceiling.
                let p = pixmap(img)?;
                let mut enc = atlas.device.create_command_encoder(&Default::default());
                let id = atlas.renderer.upload_image(
                    self.resources,
                    atlas.device,
                    atlas.queue,
                    &mut enc,
                    &p,
                );
                atlas.queue.submit([enc.finish()]);
                atlas
                    .ids
                    .0
                    .push((img.rgba.clone(), id, p.may_have_transparency()));
                (id, p.may_have_transparency())
            }
        };
        Some(
            vello_common::paint::Image {
                image: vello_common::paint::ImageSource::OpaqueId {
                    id,
                    may_have_transparency: clear,
                },
                sampler: ImageSampler::default(),
            }
            .into(),
        )
    }
    wrapper!(scene);
}

#[cfg(feature = "cpu")]
impl Canvas for Cpu<'_> {
    wrapper!(ctx);
}

/// A `vello_cpu` context together with the resources its glyph cache lives in.
#[cfg(feature = "cpu")]
pub struct Cpu<'a> {
    pub ctx: &'a mut vello_cpu::RenderContext,
    pub resources: &'a mut vello_cpu::Resources,
}

fn srgb(c: mui_core::Color) -> AlphaColor<Srgb> {
    c.to_srgb()
}

/// One premultiplied [`Pixmap`] per distinct image buffer. MUI hands over
/// straight RGBA -- what a decoder produces -- and premultiplying a photo is
/// far too much work to redo every frame.
// ponytail: same never-evicted interning as `FONTS`. `vello_cpu` paints the
// pixmap itself; `Gpu` uploads it once through its `Atlas` and keeps the id.
#[allow(clippy::type_complexity)]
static IMAGES: Mutex<Vec<(Arc<[u8]>, Arc<Pixmap>)>> = Mutex::new(Vec::new());

fn pixmap(img: &mui_core::Image) -> Option<Arc<Pixmap>> {
    // A single image has to fit one atlas tile; u16 is the hard ceiling.
    let (w, h) = (
        u16::try_from(img.width).ok()?,
        u16::try_from(img.height).ok()?,
    );
    let mut images = IMAGES.lock().unwrap_or_else(|e| e.into_inner());
    // Holding the buffer is what makes its address a sound key.
    if let Some((_, p)) = images.iter().find(|(k, _)| Arc::ptr_eq(k, &img.rgba)) {
        return Some(p.clone());
    }
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
    let p = Arc::new(Pixmap::from_parts_with_opacity(data, w, h, clear));
    images.push((img.rgba.clone(), p.clone()));
    Some(p)
}

/// Where the image's pixels land so that it fills `bounds` per `fit`.
fn image_transform(img: &mui_core::Image, fit: Fit, bounds: Rect) -> Affine {
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
        // The pixmap form; a `Canvas` decides for itself in `Canvas::image`.
        // An image too big for the atlas draws nothing rather than panicking.
        Paint::Image { image, .. } => {
            pixmap(image).map_or(PaintType::Solid(AlphaColor::TRANSPARENT), |p| {
                vello_common::paint::Image {
                    image: vello_common::paint::ImageSource::Pixmap(p),
                    sampler: ImageSampler::default(),
                }
                .into()
            })
        }
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
///
/// Every path is converted afresh. [`paint_cached`] is the same walk with the
/// conversion remembered between frames.
pub fn paint(
    canvas: &mut impl Canvas,
    scene: &ResolvedScene,
    transform: Affine,
) -> Result<(), Error> {
    canvas.set_transform(transform);
    for p in &scene.paint {
        if p.layer == Layer::Unclip {
            canvas.pop_clip();
            continue;
        }
        one(canvas, p, &bez_path(&p.path, ARC_TOLERANCE)?)?;
    }
    Ok(())
}

/// One converted path, and the frame it was last wanted on.
struct Entry {
    bez: Arc<BezPath>,
    frame: u64,
}

/// Remembers [`bez_path`] between frames, so a surface that did not change
/// is not validated, re-flattened and re-allocated every time it is drawn.
///
/// A static frame is the common case in a plugin UI: one knob moves and six
/// hundred other outlines are byte-identical to the last frame. The cache
/// keys on a fingerprint of everything the conversion reads -- the node key,
/// the layer, every coordinate, the stroke width and the blur -- so a changed
/// path simply misses. Entries not wanted during a [`paint_cached`] call are
/// dropped at the end of it, which is what keeps a scrolling list bounded.
// ponytail: a 64-bit fingerprint, not a stored copy of the path -- a
// collision would draw the wrong outline. At ~1e3 live entries that is a
// 1e-13 chance; compare `Painted::path` on a hit if that is ever too much.
#[derive(Default)]
pub struct PathCache {
    entries: std::collections::HashMap<u64, Entry>,
    frame: u64,
    hits: u64,
    misses: u64,
}

impl PathCache {
    pub fn new() -> Self {
        Self::default()
    }
    /// Live entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Conversions served from the cache, and conversions actually run,
    /// since it was built.
    pub fn hits(&self) -> u64 {
        self.hits
    }
    pub fn misses(&self) -> u64 {
        self.misses
    }

    /// The Bézier form of `p`, converted only if it is new or changed.
    pub fn bez(&mut self, p: &Painted) -> Result<Arc<BezPath>, Error> {
        let (key, frame) = (fingerprint(p), self.frame);
        if let Some(e) = self.entries.get_mut(&key) {
            e.frame = frame;
            self.hits += 1;
            return Ok(e.bez.clone());
        }
        self.misses += 1;
        let bez = Arc::new(bez_path(&p.path, ARC_TOLERANCE)?);
        self.entries.insert(
            key,
            Entry {
                bez: bez.clone(),
                frame,
            },
        );
        Ok(bez)
    }
}

/// FNV-1a over everything [`PathCache`] must notice a change in.
///
/// Folded eight bytes at a time rather than one: this runs over every
/// coordinate of every path on screen, and byte-at-a-time FNV over that is
/// slower than the conversion it is trying to avoid.
fn fingerprint(p: &Painted) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    let mut eat = |w: u64| h = (h ^ w).wrapping_mul(0x100_0000_01b3);
    for c in p.key.as_bytes().chunks(8) {
        let mut w = [0u8; 8];
        w[..c.len()].copy_from_slice(c);
        eat(u64::from_ne_bytes(w));
    }
    let (tag, n) = match p.layer {
        Layer::Shadow => (0, 0),
        Layer::Fill => (1, 0),
        Layer::Shell(i) => (2, i),
        Layer::Stroke => (3, 0),
        Layer::Text => (4, 0),
        Layer::Draw(i) => (5, i),
        Layer::Clip => (6, 0),
        Layer::Unclip => (7, 0),
    };
    eat(tag);
    eat(n as u64);
    eat(p.width.to_bits());
    eat(p.blur.to_bits());
    for command in &p.path.commands {
        match *command {
            PathCommand::MoveTo(a) => {
                eat(0);
                eat(a.x.to_bits() ^ a.y.rotate());
            }
            PathCommand::LineTo(a) => {
                eat(1);
                eat(a.x.to_bits() ^ a.y.rotate());
            }
            PathCommand::ArcTo(a) => {
                eat(2);
                eat(a.center.x.to_bits() ^ a.center.y.rotate());
                eat(a.radius.to_bits() ^ a.start_angle.rotate());
                eat(a.sweep.to_bits() ^ a.to.x.rotate());
                eat(a.to.y.to_bits());
            }
            PathCommand::CubicTo(a, b, c) => {
                eat(3);
                eat(a.x.to_bits() ^ a.y.rotate());
                eat(b.x.to_bits() ^ b.y.rotate());
                eat(c.x.to_bits() ^ c.y.rotate());
            }
            PathCommand::Close => eat(4),
        }
    }
    h
}

/// Pack two coordinates into one FNV round without letting a swap of the
/// pair go unnoticed.
trait Rotate {
    fn rotate(self) -> u64;
}
impl Rotate for f64 {
    fn rotate(self) -> u64 {
        self.to_bits().rotate_left(32)
    }
}

/// [`paint`], with the path conversion remembered in `cache` between frames.
///
/// Entries untouched by this call are dropped, so the cache tracks whatever
/// is on screen rather than everything that ever was.
pub fn paint_cached(
    canvas: &mut impl Canvas,
    scene: &ResolvedScene,
    transform: Affine,
    cache: &mut PathCache,
) -> Result<(), Error> {
    canvas.set_transform(transform);
    cache.frame += 1;
    let frame = cache.frame;
    for p in &scene.paint {
        if p.layer == Layer::Unclip {
            canvas.pop_clip();
            continue;
        }
        let bez = cache.bez(p)?;
        one(canvas, p, &bez)?;
    }
    cache.entries.retain(|_, e| e.frame == frame);
    Ok(())
}

fn one(canvas: &mut impl Canvas, p: &Painted, path: &BezPath) -> Result<(), Error> {
    if p.layer == Layer::Clip {
        canvas.push_clip(path);
        return Ok(());
    }
    let bounds = path.bounding_box();
    // A canvas that cannot take this image gets its solid stand-in rather
    // than a panic, and none of the paint-transform dance below.
    let (img, b) = match &p.paint {
        Paint::Image { image, fit } => match canvas.image(image) {
            Some(b) => (Some((image, *fit)), Some(b)),
            None => (None, None),
        },
        _ => (None, None),
    };
    canvas.set_paint(match (&p.paint, b) {
        (_, Some(b)) => b,
        (Paint::Image { .. }, None) => PaintType::Solid(srgb(p.paint.solid())),
        _ => brush(&p.paint, bounds),
    });
    if let Some(t) = &p.text {
        canvas.glyphs(&t.font, t.size, (t.origin.x, t.origin.y), &t.glyphs);
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
            let b = rr.bounds();
            canvas.fill_blurred_rounded_rect(
                &Rect::new(b.min.x, b.min.y, b.max.x, b.max.y),
                rr.radius() as f32,
                p.blur as f32,
            );
        }
        // ponytail: blur on a welded outline is drawn sharp; a blur filter
        // layer is the upgrade if a merged shadow ever needs it.
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

    /// A 2x2 image stretched over a leaf: each source pixel owns a quadrant,
    /// and the leaf's rounded corner still cuts the image away.
    #[test]
    fn an_image_fill_lands_the_right_pixel_in_each_quadrant() {
        #[rustfmt::skip]
        let px: Vec<u8> = vec![
            255, 0, 0, 255,  0, 255, 0, 255,
            0, 0, 255, 255,  255, 255, 255, 255,
        ];
        let img = std::sync::Arc::new(mui_core::Image::rgba(2, 2, px).unwrap());
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

    /// Second frame, same tree: every conversion is a hit and hands back the
    /// very same `BezPath`. A surface that goes away takes its entry with it.
    #[test]
    fn a_static_frame_reuses_its_paths_and_a_gone_one_is_dropped() {
        let tree = |n: usize| {
            let kids: Vec<El> = (0..n)
                .map(|i| {
                    leaf(20., 20.)
                        .fill(Role::Primary)
                        .radius(6.)
                        .id(format!("l{i}"))
                })
                .collect();
            SceneSpec::new(column(kids).pad(4.)).offered(Size::new(60., 200.))
        };
        let scene = resolve_scene(&tree(3)).unwrap();
        let mut cache = PathCache::new();
        let mut ctx = vello_cpu::RenderContext::new(60, 200);
        let mut res = vello_cpu::Resources::default();
        let mut draw = |scene: &ResolvedScene, cache: &mut PathCache| {
            ctx.reset();
            paint_cached(
                &mut Cpu {
                    ctx: &mut ctx,
                    resources: &mut res,
                },
                scene,
                Affine::IDENTITY,
                cache,
            )
            .unwrap();
        };

        draw(&scene, &mut cache);
        let n = cache.misses();
        assert!(n >= 3, "only {n} paths for three leaves");
        let first = cache.bez(&scene.paint[0]).unwrap();

        let hits = cache.hits();
        draw(&scene, &mut cache);
        assert_eq!(
            cache.misses(),
            n,
            "a byte-identical frame reconverted a path"
        );
        assert_eq!(
            cache.hits(),
            hits + n,
            "the second frame came entirely from the cache"
        );
        assert!(Arc::ptr_eq(&first, &cache.bez(&scene.paint[0]).unwrap()));

        let full = cache.len();
        draw(&resolve_scene(&tree(2)).unwrap(), &mut cache);
        assert!(cache.len() < full, "the gone leaf kept its entry: {full}");
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
