//! The paint list as quad instances, on the CPU. One walk serves
//! [`classify`](super::classify) and [`QuadRenderer`](super::QuadRenderer):
//! whatever the walk accepts is drawn exactly as [`crate::paint`] draws it on
//! the `Cpu` canvas, and whatever it cannot is reported, never approximated.

use super::Reason;
use super::path::{Fail, Store};
use crate::kurbo::{Affine, BezPath, Rect};
use mui_scene::{Fit, GradientKind, Layer, Paint, Painted, ShadowKind};

/// Gradient stops one instance carries. A longer ramp is
/// [`Reason::GradientStops`], not truncated.
pub const MAX_STOPS: usize = 4;

/// One instance: fourteen `vec4<f32>` slots, the vertex attributes of
/// `quad.wgsl` in order.
///
/// | slot | contents |
/// |------|----------|
/// | 0 | local-to-device affine `a b c d` |
/// | 1 | affine `e f`, unused, unused |
/// | 2 | the rect (`x0 y0 x1 y1`, local); a blur's inflated box |
/// | 3 | code, gradient table scale, radius, stroke width; glyph: code, scale, atlas `x y`; path: radius is the record |
/// | 4-7 | premultiplied colour stops; blur: colour, params, params, rect; path blur: colour, std dev; texture: opacity, used `w h`, straight alpha |
/// | 8 | stop offsets |
/// | 9 | gradient geometry; image: placement `tx ty sx sy`; screen: device origin, pixels a texel |
/// | 10 | device clip rect |
/// | 11-12 | rounded clip: device-to-local affine `a b c d`, `e f radius on` |
/// | 13 | rounded clip rect |
/// | 14 | path clip: fill record, translation `e f`, on |
///
/// The code is `page * 64 + shape * 8 + paint`. A screen paint samples a
/// texture laid over device pixels (a group layer, a blurred backdrop); its
/// table scale is the layer's blend mode. A path's rect is its local
/// bounding box (a blur's inflated by its reach) and its record (see
/// [`super::path`]) holds the outline. A glyph's rect (slot 2) is
/// in device pixels, a whole-pixel box over its atlas cell; its affine still
/// maps the entry's local space, where a gradient lives.
pub type Instance = [[f32; 4]; SLOTS];
pub const SLOTS: usize = 15;

const SHAPE_FILL: f32 = 0.;
const SHAPE_STROKE: f32 = 1.;
const SHAPE_BLUR: f32 = 2.;
const SHAPE_BLUR_INVERT: f32 = 3.;
pub const SHAPE_GLYPH: f32 = 4.;
const SHAPE_PATH: f32 = 5.;
const SHAPE_PATH_STROKE: f32 = 6.;
const SHAPE_PATH_BLUR: f32 = 7.;
pub const PAINT_IMAGE: f32 = 4.;
pub const PAINT_SCREEN: f32 = 5.;

/// The instance code of `shape` painted with `paint`.
pub fn code(shape: f32, paint: f32) -> f32 {
    shape * 8. + paint
}

/// No box: what [`union`] starts from, and what overlaps nothing.
pub const EMPTY: [f32; 4] = [f32::INFINITY, f32::INFINITY, -f32::INFINITY, -f32::INFINITY];

pub fn union(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

pub fn overlaps(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// The device pixels an instance's quad can reach, a pixel generous, cut to
/// its device clip. `quad.wgsl` grows a shape's rect by one device pixel in
/// local units (here bounded above by the Frobenius norm over the
/// determinant) and half its stroke.
pub fn device_box(i: &Instance) -> [f32; 4] {
    let [a, b, c, d] = i[0];
    let [e, f, ..] = i[1];
    let shape = ((i[3][0] % 64.) / 8.).floor();
    let r = if shape == SHAPE_GLYPH {
        i[2]
    } else {
        let mut grow = (a * a + b * b + c * c + d * d).sqrt() / (a * d - b * c).abs();
        if shape == SHAPE_STROKE || shape == SHAPE_PATH_STROKE {
            grow += 0.5 * i[3][3];
        }
        let [x0, y0, x1, y1] = i[2];
        let (x0, y0, x1, y1) = (x0 - grow, y0 - grow, x1 + grow, y1 + grow);
        [(x0, y0), (x1, y0), (x0, y1), (x1, y1)]
            .map(|(x, y)| {
                let (px, py) = (a * x + c * y + e, b * x + d * y + f);
                [px, py, px, py]
            })
            .into_iter()
            .fold(EMPTY, union)
    };
    let clip = i[10];
    [
        r[0].max(clip[0]) - 1.,
        r[1].max(clip[1]) - 1.,
        r[2].min(clip[2]) + 1.,
        r[3].min(clip[3]) + 1.,
    ]
}

/// An instance that covers a `size` target with transparent: drawn without
/// blending, it clears what the scissor leaves open.
pub fn clear(size: [u32; 2]) -> Instance {
    let mut out = [[0.; 4]; SLOTS];
    out[0] = [1., 0., 0., 1.];
    out[2] = [-1., -1., size[0] as f32 + 1., size[1] as f32 + 1.];
    out[3] = [code(SHAPE_FILL, 0.), 0., 0., 0.];
    out[10] = OPEN;
    out
}

/// The largest glyph, in device pixels per em, the atlas takes. A larger run
/// is [`Reason::TextSize`].
pub const MAX_GLYPH_PX: f32 = 256.;

/// glifo's tolerance for a uniform, unskewed scale.
const NEARLY_ZERO: f64 = 1. / 4096.;

/// What one entry of the list asks of the renderer.
#[derive(Debug)]
pub enum Drawn<'p> {
    /// Nothing to draw: bookkeeping, or no area.
    Nothing,
    /// The instance in `out`.
    One,
    /// The instance in `out`, sampling this image.
    Image(&'p mui_scene::Image),
    /// The instance in `out`, sampling the host texture of this key.
    Host(u64),
    /// A glyph run: `out` is the template every glyph's instance starts from.
    Text(&'p mui_scene::Text),
    /// A group layer opens: `mix` is `mui_scene::Mix` in declaration order.
    Open { mix: u32, opacity: f32 },
    /// The innermost group layer closes.
    Close,
    /// The outline in `out`, to be painted with what the list painted
    /// before it in device `region` (`x y w h`), blurred by `sigma` device
    /// pixels at one texel per `scale` pixels.
    Backdrop {
        region: [u32; 4],
        scale: u32,
        sigma: f32,
    },
    /// A weld's slot: `out` holds its transform and clip; the weld's bounds
    /// and texture come from the scene.
    External,
}

/// No device clip at all: far past any target.
const OPEN: [f32; 4] = [-1e9, -1e9, 1e9, 1e9];

/// The clip state in force: the intersection of every axis-aligned clip as
/// one device rect, at most one other rounded or rotated rect carried
/// exactly in its own space, and at most one path. All multiply into
/// coverage, as `vello_cpu`'s clip masks do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clip {
    device: [f32; 4],
    general: Option<General>,
    /// A path's fill record, and the device translation it stands at.
    path: Option<(u32, [f32; 2])>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct General {
    /// Device to local.
    inverse: [f32; 6],
    rect: [f32; 4],
    radius: f32,
}

impl Clip {
    pub const NONE: Self = Self {
        device: OPEN,
        general: None,
        path: None,
    };

    /// A clip with no area: it hides everything inside it.
    const HIDDEN: Self = Self {
        device: [0.; 4],
        general: None,
        path: None,
    };

    /// This clip narrowed by `rect` (rounded by `radius`) under `xf`, or
    /// `None` when the result needs a second general clip.
    fn push(self, rect: Rect, radius: f64, xf: Affine) -> Option<Self> {
        let [a, b, c, d, ..] = xf.as_coeffs();
        let det = a * d - b * c;
        if det == 0. {
            return Some(Self::HIDDEN);
        }
        if radius == 0. && b == 0. && c == 0. {
            let r = xf.transform_rect_bbox(rect);
            let [x0, y0, x1, y1] = self.device;
            return Some(Self {
                device: [
                    x0.max(r.x0 as f32),
                    y0.max(r.y0 as f32),
                    x1.min(r.x1 as f32),
                    y1.min(r.y1 as f32),
                ],
                ..self
            });
        }
        if self.general.is_some() {
            return None;
        }
        let inverse = xf.inverse().as_coeffs().map(|v| v as f32);
        Some(Self {
            general: Some(General {
                inverse,
                rect: rect4(rect),
                radius: radius as f32,
            }),
            ..self
        })
    }

    /// This clip narrowed by the path of fill record `record` (`None`: a
    /// path that covers nothing) under `xf`, or `None` when the result
    /// needs a second general clip.
    fn push_path(self, record: Option<u32>, xf: Affine) -> Option<Self> {
        let [a, b, c, d, e, f] = xf.as_coeffs();
        let Some(record) = record.filter(|_| a * d - b * c != 0.) else {
            return Some(Self::HIDDEN);
        };
        if self.path.is_some() {
            return None;
        }
        Some(Self {
            path: Some((record, [e as f32, f as f32])),
            ..self
        })
    }

    fn write(self, out: &mut Instance) {
        out[10] = self.device;
        (out[11], out[12], out[13]) = match self.general {
            Some(General {
                inverse: [a, b, c, d, e, f],
                rect,
                radius,
            }) => ([a, b, c, d], [e, f, radius, 1.], rect),
            None => ([0.; 4], [0.; 4], [0.; 4]),
        };
        out[14] = match self.path {
            Some((record, [e, f])) => [record as f32, e, f, 1.],
            None => [0.; 4],
        };
    }
}

fn rect4(r: Rect) -> [f32; 4] {
    [r.x0 as f32, r.y0 as f32, r.x1 as f32, r.y1 as f32]
}

/// The walk's state across one list: the clip stack, reused frame to frame,
/// and the path records its entries draw from.
pub struct Walk {
    clips: Vec<Clip>,
    /// The largest image side a texture can take; images past it are
    /// [`Reason::ImageSize`].
    pub max_image: u32,
    pub paths: Store,
    /// The target, device pixels: where a backdrop's snapshot is cut.
    pub size: [u32; 2],
    /// The size of each registered host texture, by key. An image naming
    /// any other key paints its solid stand-in, as `one` does.
    pub hosted: rustc_hash::FxHashMap<u64, [u32; 2]>,
}

impl Default for Walk {
    fn default() -> Self {
        Self {
            clips: Vec::new(),
            max_image: u32::MAX,
            paths: Store::new(super::path::PATH_BUDGET, u32::MAX),
            size: [u32::MAX; 2],
            hosted: rustc_hash::FxHashMap::default(),
        }
    }
}

fn refuse(f: Fail) -> Reason {
    match f {
        Fail::Invalid => Reason::Path,
        Fail::Full => Reason::PathBudget,
    }
}

impl Walk {
    pub fn begin(&mut self) {
        self.clips.clear();
    }

    #[cfg(test)]
    pub fn clip_capacity(&self) -> usize {
        self.clips.capacity()
    }

    fn clip(&self) -> Clip {
        self.clips.last().copied().unwrap_or(Clip::NONE)
    }

    /// `p` under the caller's `transform`: what it draws, with `out` holding
    /// its instance (or a glyph run's template). The clip stack moves even
    /// for an unsupported clip, so the rest of the list still classifies
    /// against the right state.
    pub fn entry<'p>(
        &mut self,
        p: &'p Painted,
        transform: Affine,
        out: &mut Instance,
    ) -> Result<Drawn<'p>, Reason> {
        *out = [[0.; 4]; SLOTS];
        let xf = crate::placed(transform, p);
        if !xf.as_coeffs().iter().all(|v| v.is_finite()) {
            // Keep a pushed clip balanced with its unclip.
            if p.layer == Layer::Clip {
                self.clips.push(self.clip());
            }
            return Err(Reason::NonFinite);
        }
        match p.layer {
            Layer::Unclip => {
                self.clips.pop();
                return Ok(Drawn::Nothing);
            }
            Layer::Clip => {
                let top = self.clip();
                let next = match p.rect {
                    Some(rr) => top.push(rr.bounds(), rr.radius(), xf),
                    None => match self.paths.find(&p.path, xf, 0.) {
                        Ok(found) => top.push_path(found.record, xf),
                        Err(f) => {
                            self.clips.push(top);
                            return Err(refuse(f));
                        }
                    },
                };
                self.clips.push(next.unwrap_or(top));
                return if next.is_some() {
                    Ok(Drawn::Nothing)
                } else {
                    Err(Reason::ClipDepth)
                };
            }
            Layer::Blend { mix, opacity } => {
                return Ok(Drawn::Open {
                    mix: mix as u32,
                    opacity,
                });
            }
            Layer::Unblend => return Ok(Drawn::Close),
            Layer::Backdrop => return self.backdrop(p, transform, xf, out),
            Layer::External => {
                let [a, b, c, d, e, f] = xf.as_coeffs();
                out[0] = [a as f32, b as f32, c as f32, d as f32];
                out[1] = [e as f32, f as f32, 0., 0.];
                self.clip().write(out);
                return Ok(Drawn::External);
            }
            // `one` fills a mask with a solid or gradient brush.
            Layer::Mask if matches!(p.paint, Paint::Image { .. }) => return Err(Reason::Image),
            Layer::Mask
            | Layer::Text
            | Layer::Shadow(_)
            | Layer::Fill
            | Layer::Shell(_)
            | Layer::Stroke
            | Layer::Draw(_) => {}
        }
        if let Paint::Vector { .. } = p.paint {
            return Err(Reason::Vector);
        }
        if let Some(text) = &p.text {
            return self.text(p, text, xf, out);
        }
        let blurred = p.blur > 0.0;
        let [a, b, c, d, e, f] = xf.as_coeffs();
        if a * d - b * c == 0. {
            // Collapsed to a line: no area, nothing for `vello_cpu` to cover.
            return Ok(Drawn::Nothing);
        }
        let Some(rr) = p.rect else {
            return self.path(p, xf, out);
        };
        let bounds = rr.bounds();
        let radius = rr.radius();
        out[0] = [a as f32, b as f32, c as f32, d as f32];
        let mut clip = self.clip();
        let image = self.image(p, bounds, xf, &mut clip, out)?;
        clip.write(out);
        if blurred {
            // Both backends blur with the solid stand-in of a gradient.
            let colour = premul(crate::srgb(p.paint.solid()).components);
            let invert = matches!(p.layer, Layer::Shadow(ShadowKind::Inset));
            out[1] = [e as f32, f as f32, 0., 0.];
            blur(out, bounds, radius as f32, p.blur as f32, colour, invert);
            return Ok(Drawn::One);
        }
        let shape = if p.width > 0.0 {
            SHAPE_STROKE
        } else {
            SHAPE_FILL
        };
        out[1] = [e as f32, f as f32, 0., 0.];
        out[2] = rect4(bounds);
        let (kind, scale) = match image {
            Some(_) => (PAINT_IMAGE, 0.),
            None => paint(out, &p.paint, bounds)?,
        };
        out[3] = [code(shape, kind), scale, radius as f32, p.width as f32];
        Ok(image.unwrap_or(Drawn::One))
    }

    /// An entry whose outline is its path, drawn from the path's record.
    fn path<'p>(
        &mut self,
        p: &'p Painted,
        xf: Affine,
        out: &mut Instance,
    ) -> Result<Drawn<'p>, Reason> {
        // `one` drops a rect-less blur, except the drop shadow `paint` hands
        // to a filter layer instead.
        let shadow = crate::path_shadow(p);
        if p.blur > 0.0 && !shadow {
            return Ok(Drawn::Nothing);
        }
        let stroke = !shadow && p.width > 0.0;
        let width = if stroke { p.width } else { 0. };
        let found = self.paths.find(&p.path, xf, width).map_err(refuse)?;
        let Some(record) = found.record else {
            return Ok(Drawn::Nothing);
        };
        let bounds = found.bounds;
        let [a, b, c, d, e, f] = xf.as_coeffs();
        out[0] = [a as f32, b as f32, c as f32, d as f32];
        out[1] = [e as f32, f as f32, 0., 0.];
        let mut clip = self.clip();
        if shadow {
            // `blurred_path`: the solid stand-in, blurred as far as
            // `blur_reach`, where its clip stops it.
            clip.write(out);
            let reach = 3. * p.blur;
            out[2] = rect4(bounds.inflate(reach, reach));
            out[3] = [code(SHAPE_PATH_BLUR, 0.), 0., record as f32, 0.];
            out[4] = premul(crate::srgb(p.paint.solid()).components);
            out[5] = [p.blur as f32, 0., 0., 0.];
            return Ok(Drawn::One);
        }
        let image = self.image(p, bounds, xf, &mut clip, out)?;
        clip.write(out);
        out[2] = rect4(bounds);
        let (kind, scale) = match image {
            Some(_) => (PAINT_IMAGE, 0.),
            None => paint(out, &p.paint, bounds)?,
        };
        let shape = if stroke {
            SHAPE_PATH_STROKE
        } else {
            SHAPE_PATH
        };
        out[3] = [code(shape, kind), scale, record as f32, width as f32];
        Ok(image.unwrap_or(Drawn::One))
    }

    /// An image paint placed on `bounds` as `one` places it, its placement
    /// and texture size written to `out`; `Contain` also narrows `clip` to
    /// the rect the image occupies, so the padded edge does not smear
    /// across the letterbox. `None` for any other paint, and for an image
    /// with nothing to sample (a buffer that does not match its size, an
    /// unregistered host texture), which paints its solid stand-in.
    fn image<'p>(
        &self,
        p: &'p Painted,
        bounds: Rect,
        xf: Affine,
        clip: &mut Clip,
        out: &mut Instance,
    ) -> Result<Option<Drawn<'p>>, Reason> {
        let Paint::Image { image, fit } = &p.paint else {
            return Ok(None);
        };
        let (drawn, size) = match image.texture {
            Some(key) => match self.hosted.get(&key) {
                Some(&size) if image.width > 0 && image.height > 0 => (Drawn::Host(key), size),
                _ => return Ok(None),
            },
            None if crate::size(image).is_some() => {
                if image.width.max(image.height) > self.max_image {
                    return Err(Reason::ImageSize);
                }
                (Drawn::Image(image), [image.width, image.height])
            }
            None => return Ok(None),
        };
        let t = crate::image_transform(image, *fit, bounds);
        if *fit == Fit::Contain {
            let r = t.transform_rect_bbox(Rect::new(
                0.,
                0.,
                f64::from(image.width),
                f64::from(image.height),
            ));
            *clip = clip.push(r, 0., xf).ok_or(Reason::ClipDepth)?;
        }
        // Placed in the texture's own texels: a host texture need not be
        // the size the scene's image says.
        let [sx, _, _, sy, tx, ty] = t.as_coeffs();
        let kx = f64::from(image.width) / f64::from(size[0]);
        let ky = f64::from(image.height) / f64::from(size[1]);
        out[9] = [tx as f32, ty as f32, (sx * kx) as f32, (sy * ky) as f32];
        let straight = matches!(drawn, Drawn::Host(_));
        out[4] = [
            1.,
            size[0] as f32,
            size[1] as f32,
            f32::from(u8::from(straight)),
        ];
        Ok(Some(drawn))
    }

    /// A backdrop's outline, painted with the blurred snapshot of what was
    /// drawn before it: `lib.rs`'s `backdrop`, whose reach is three
    /// deviations past the outline's scene box, cut to the target. The
    /// snapshot is taken at one texel per `scale` device pixels, the power
    /// of two that keeps its deviation within four texels, as
    /// `GpuRenderer`'s backdrop does.
    fn backdrop<'p>(
        &mut self,
        p: &Painted,
        transform: Affine,
        xf: Affine,
        out: &mut Instance,
    ) -> Result<Drawn<'p>, Reason> {
        // NaN and negatives say nothing, as a zero does.
        if p.blur.is_nan() || p.blur <= 0.0 {
            return Ok(Drawn::Nothing);
        }
        let [a, b, c, d, e, f] = xf.as_coeffs();
        if a * d - b * c == 0. {
            return Ok(Drawn::Nothing);
        }
        out[0] = [a as f32, b as f32, c as f32, d as f32];
        out[1] = [e as f32, f as f32, 0., 0.];
        self.clip().write(out);
        let local = match p.rect {
            Some(rr) => {
                out[3] = [code(SHAPE_FILL, PAINT_SCREEN), 0., rr.radius() as f32, 0.];
                rr.bounds()
            }
            None => {
                let found = self.paths.find(&p.path, xf, 0.).map_err(refuse)?;
                let Some(record) = found.record else {
                    return Ok(Drawn::Nothing);
                };
                out[3] = [code(SHAPE_PATH, PAINT_SCREEN), 0., record as f32, 0.];
                found.bounds
            }
        };
        out[2] = rect4(local);
        let scene = p
            .transform
            .transform_rect_bbox(local + crate::kurbo::Vec2::new(p.offset.x, p.offset.y));
        let reach = transform.transform_rect_bbox(scene.inflate(3. * p.blur, 3. * p.blur));
        let [w, h] = self.size.map(f64::from);
        let x0 = reach.x0.floor().clamp(0., w);
        let y0 = reach.y0.floor().clamp(0., h);
        let x1 = reach.x1.ceil().clamp(x0, w);
        let y1 = reach.y1.ceil().clamp(y0, h);
        if x1 <= x0 || y1 <= y0 {
            return Ok(Drawn::Nothing);
        }
        let sigma = p.blur * transform.determinant().abs().sqrt();
        let scale = ((sigma / 4.).ceil().max(1.) as u32).next_power_of_two();
        let low = [x1 - x0, y1 - y0].map(|v| (v / f64::from(scale)).ceil() as f32);
        out[4] = [1., low[0], low[1], 0.];
        out[9] = [x0 as f32, y0 as f32, scale as f32, scale as f32];
        Ok(Drawn::Backdrop {
            region: [x0, y0, x1 - x0, y1 - y0].map(|v| v as u32),
            scale,
            sigma: sigma as f32,
        })
    }

    /// A glyph run's template: its paint, clip and local-to-device affine.
    /// The atlas draws glyphs under translation and uniform scale only;
    /// anything else is refused rather than drawn from a mask at the wrong
    /// size or angle.
    fn text<'p>(
        &self,
        p: &Painted,
        text: &'p mui_scene::Text,
        xf: Affine,
        out: &mut Instance,
    ) -> Result<Drawn<'p>, Reason> {
        if let Paint::Image { .. } = p.paint {
            return Err(Reason::Image);
        }
        let [a, b, c, d, e, f] = xf.as_coeffs();
        if a * d - b * c == 0. || text.glyphs.is_empty() {
            return Ok(Drawn::Nothing);
        }
        // glifo's test for a run it can take from its atlas.
        let uniform = a > 0.
            && (a - d).abs() <= NEARLY_ZERO
            && b.abs() <= NEARLY_ZERO
            && c.abs() <= NEARLY_ZERO;
        if !uniform {
            return Err(Reason::TextTransform);
        }
        if text.size * a as f32 > MAX_GLYPH_PX {
            return Err(Reason::TextSize);
        }
        out[0] = [a as f32, b as f32, c as f32, d as f32];
        out[1] = [e as f32, f as f32, 0., 0.];
        self.clip().write(out);
        // The brush box comes from the run, as `paint` fits it.
        let bounds = crate::paint_box(p, &BezPath::new());
        let (kind, scale) = paint(out, &p.paint, bounds)?;
        out[3] = [code(SHAPE_GLYPH, kind), scale, 0., 0.];
        Ok(Drawn::Text(text))
    }
}

/// `i` drawn at `alpha` times its opacity, as a group layer that does not
/// need a target of its own draws it: every colour it can paint scaled.
pub fn fade(i: &mut Instance, alpha: f32) {
    let rest = i[3][0] % 64.;
    let (shape, paint) = ((rest / 8.).floor(), rest % 8.);
    if paint >= PAINT_IMAGE {
        // A texture's opacity.
        i[4][0] *= alpha;
        return;
    }
    let ramp = paint > 0. && shape != SHAPE_BLUR && shape != SHAPE_BLUR_INVERT;
    let stops = if ramp { 4 } else { 1 };
    for slot in &mut i[4..4 + stops] {
        *slot = slot.map(|v| v * alpha);
    }
}

/// A premultiplied colour, as `vello_cpu` premultiplies it.
fn premul([r, g, b, a]: [f32; 4]) -> [f32; 4] {
    [r * a, g * a, b * a, a]
}

/// `BlurredRoundedRectangle::encode_into` and `fill_blurred_rounded_rect`,
/// in the same f32 arithmetic: the inflated box it paints, and the encoded
/// parameters of the blur.
fn blur(out: &mut Instance, rect: Rect, radius: f32, std_dev: f32, colour: [f32; 4], invert: bool) {
    let rect = rect.abs();
    let kernel = f64::from(2.5 * std_dev);
    let width = rect.width() as f32;
    let height = rect.height() as f32;
    let radius = radius.min(0.5 * width.min(height));
    let std_dev = std_dev.max(1e-6);
    let min_edge = width.min(height);
    let rmax = 0.5 * min_edge;
    let r0 = radius.hypot(std_dev * 1.15).min(rmax);
    let r1 = radius.hypot(std_dev * 2.0).min(rmax);
    let exponent = 2.0 * r1 / r0;
    let inv = std_dev.recip();
    let delta = 1.25
        * std_dev
        * ((-(0.5 * inv * width).powi(2)).exp() - (-(0.5 * inv * height).powi(2)).exp());
    let w = width + delta.min(0.0);
    let h = height - delta.max(0.0);
    let scale = 0.5 * erf7(inv * 0.5 * (w.max(h) - 0.5 * radius));
    let shape = if invert {
        SHAPE_BLUR_INVERT
    } else {
        SHAPE_BLUR
    };
    out[2] = rect4(rect.inflate(kernel, kernel));
    out[3] = [code(shape, 0.), 0., 0., 0.];
    out[4] = colour;
    out[5] = [inv, exponent, scale, r1];
    out[6] = [w, h, 0., 0.];
    out[7] = rect4(rect);
    out[8] = [0.; 4];
    out[9] = [0.; 4];
}

/// `vello_common::math::compute_erf7`.
fn erf7(x: f32) -> f32 {
    let x = x.clamp(-10., 10.) * std::f32::consts::FRAC_2_SQRT_PI;
    let xx = x * x;
    let x = x + (0.24295 + (0.03395 + 0.0104 * xx) * xx) * (x * xx);
    x / (1.0 + x * x).sqrt()
}

/// The paint of a sharp entry: slots 4-9, and its (paint kind, table scale).
/// Mirrors [`crate::brush`] and how `vello_common` encodes the result:
/// stops interpolate premultiplied in sRGB, extend pads, and the ramp is
/// sampled from a lookup table of the size `vello_common` picks.
fn paint(out: &mut Instance, paint: &Paint, bounds: Rect) -> Result<(f32, f32), Reason> {
    let solid = |out: &mut Instance, c: mui_scene::Color| {
        out[4] = premul(crate::srgb(c).components);
        Ok((0., 0.))
    };
    let (kind, stops) = match paint {
        Paint::Solid(c) => return solid(out, *c),
        Paint::Gradient { kind, stops } => (kind, stops),
        // An image this pass cannot place paints its stand-in, as `one` does.
        Paint::Image { .. } => return solid(out, paint.solid()),
        Paint::Vector { .. } => return Err(Reason::Vector),
    };
    // `vello_common::encode::validate`: what it refuses paints solid.
    let Some(&(_, first)) = stops.first() else {
        out[4] = [0., 0., 0., 1.];
        return Ok((0., 0.));
    };
    let ordered = stops.windows(2).all(|w| w[0].0 <= w[1].0);
    let in_range = stops.iter().all(|s| (0.0..=1.0).contains(&s.0));
    if stops.len() == 1 || !ordered || !in_range {
        return solid(out, first);
    }
    if stops.len() > MAX_STOPS {
        return Err(Reason::GradientStops);
    }
    let mid = bounds.center();
    let (code, geom) = match *kind {
        GradientKind::Linear { angle } => {
            let a = angle.to_radians();
            let (s, c) = (a.sin(), -a.cos());
            let len = (bounds.width() * s).abs() + (bounds.height() * c).abs();
            let half = (s * len / 2.0, c * len / 2.0);
            let (p0, p1) = (
                (mid.x - half.0, mid.y - half.1),
                (mid.x + half.0, mid.y + half.1),
            );
            if degenerate(p0.0, p1.0) && degenerate(p0.1, p1.1) {
                return solid(out, first);
            }
            (1., [p0.0 as f32, p0.1 as f32, p1.0 as f32, p1.1 as f32])
        }
        GradientKind::Radial { center, radius } => {
            let r = (radius * bounds.width().max(bounds.height())) as f32;
            // A zero radius, or a negative one, `validate` refuses.
            if r < 0. || f64::from(r).abs() <= DEGENERATE {
                return solid(out, first);
            }
            let cx = bounds.x0 + center.0 * bounds.width();
            let cy = bounds.y0 + center.1 * bounds.height();
            (2., [cx as f32, cy as f32, r, 0.])
        }
        GradientKind::Conic { angle } => {
            let from = (angle - 90.0).to_radians() as f32;
            let to = from + std::f32::consts::TAU;
            (3., [mid.x as f32, mid.y as f32, from, 1.0 / (to - from)])
        }
    };
    out[9] = geom;
    // Pad unused stops with the last one at its own offset: an empty span
    // the shader never selects.
    let last = stops.len() - 1;
    let mut offsets = [0.; 4];
    for (i, offset) in offsets.iter_mut().enumerate() {
        let (t, c) = stops[i.min(last)];
        *offset = t;
        out[4 + i] = premul(crate::srgb(c).components).map(|v| v.clamp(0., 1.));
    }
    out[8] = offsets;
    Ok((code, lut_size(stops) as f32 - 1.))
}

/// `vello_common`'s degenerate-geometry threshold.
const DEGENERATE: f64 = 1.0e-6;

fn degenerate(a: f64, b: f64) -> bool {
    (a - b).abs() as f32 <= DEGENERATE as f32
}

/// `vello_common::encode::determine_lut_size`, over the ranges its encoder
/// makes: the stops, padded out to 0 and 1.
pub(super) fn lut_size(stops: &[(f32, mui_scene::Color)]) -> usize {
    let mut padded = [0.0_f32; MAX_STOPS + 2];
    let mut n = 0;
    let mut add = |t: f32| {
        padded[n] = t;
        n += 1;
    };
    if stops[0].0 != 0. {
        add(0.);
    }
    stops.iter().for_each(|s| add(s.0));
    if stops[stops.len() - 1].0 != 1. {
        add(1.);
    }
    let mut size = match n - 1 {
        1 => 256,
        2 => 512,
        _ => 1024,
    };
    let mut prev = 0.0_f32;
    for &x1 in &padded[1..n] {
        // A zero-width range divides to infinity, which saturates to 4096.
        let res = ((1.0 / (x1 - prev)).ceil() as usize)
            .min(4096)
            .next_power_of_two();
        size = size.max(res);
        prev = x1;
    }
    size
}
