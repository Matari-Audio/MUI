//! The paint list as quad instances, on the CPU. One walk serves
//! [`classify`](super::classify) and [`QuadRenderer`](super::QuadRenderer):
//! whatever the walk accepts is drawn exactly as [`crate::paint`] draws it on
//! the `Cpu` canvas, and whatever it cannot is reported, never approximated.

use super::Reason;
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
/// | 3 | code, gradient table scale, radius, stroke width; glyph: code, scale, atlas `x y` |
/// | 4-7 | premultiplied colour stops; blur: colour, params, params, rect |
/// | 8 | stop offsets |
/// | 9 | gradient geometry; image: placement `tx ty sx sy` |
/// | 10 | device clip rect |
/// | 11-12 | rounded clip: device-to-local affine `a b c d`, `e f radius on` |
/// | 13 | rounded clip rect |
///
/// The code is `page * 64 + shape * 8 + paint`. A glyph's rect (slot 2) is
/// in device pixels, a whole-pixel box over its atlas cell; its affine still
/// maps the entry's local space, where a gradient lives.
pub type Instance = [[f32; 4]; SLOTS];
pub const SLOTS: usize = 14;

const SHAPE_FILL: f32 = 0.;
const SHAPE_STROKE: f32 = 1.;
const SHAPE_BLUR: f32 = 2.;
const SHAPE_BLUR_INVERT: f32 = 3.;
pub const SHAPE_GLYPH: f32 = 4.;
const PAINT_IMAGE: f32 = 4.;

/// The instance code of `shape` painted with `paint`.
pub fn code(shape: f32, paint: f32) -> f32 {
    shape * 8. + paint
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
    /// A glyph run: `out` is the template every glyph's instance starts from.
    Text(&'p mui_scene::Text),
}

/// No device clip at all: far past any target.
const OPEN: [f32; 4] = [-1e9, -1e9, 1e9, 1e9];

/// The clip state in force: the intersection of every axis-aligned clip as
/// one device rect, and at most one other clip, rounded or rotated, carried
/// exactly in its own space. Both multiply into coverage, as `vello_cpu`'s
/// clip masks do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clip {
    device: [f32; 4],
    general: Option<General>,
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
    };

    /// This clip narrowed by `rect` (rounded by `radius`) under `xf`, or
    /// `None` when the result needs a second general clip.
    fn push(self, rect: Rect, radius: f64, xf: Affine) -> Option<Self> {
        let [a, b, c, d, ..] = xf.as_coeffs();
        let det = a * d - b * c;
        if det == 0. {
            // A clip with no area hides everything inside it.
            return Some(Self {
                device: [0.; 4],
                general: None,
            });
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

    fn write(self, out: &mut Instance) {
        out[10] = self.device;
        if let Some(g) = self.general {
            let [a, b, c, d, e, f] = g.inverse;
            out[11] = [a, b, c, d];
            out[12] = [e, f, g.radius, 1.];
            out[13] = g.rect;
        } else {
            out[11] = [0.; 4];
            out[12] = [0.; 4];
            out[13] = [0.; 4];
        }
    }
}

fn rect4(r: Rect) -> [f32; 4] {
    [r.x0 as f32, r.y0 as f32, r.x1 as f32, r.y1 as f32]
}

/// The walk's state across one list: the clip stack, reused frame to frame.
pub struct Walk {
    clips: Vec<Clip>,
    /// The largest image side a texture can take; images past it are
    /// [`Reason::ImageSize`].
    pub max_image: u32,
}

impl Default for Walk {
    fn default() -> Self {
        Self {
            clips: Vec::new(),
            max_image: u32::MAX,
        }
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
                let Some(rr) = p.rect else {
                    self.clips.push(top);
                    return Err(Reason::ClipPath);
                };
                let next = top.push(rr.bounds(), rr.radius(), xf);
                self.clips.push(next.unwrap_or(top));
                return if next.is_some() {
                    Ok(Drawn::Nothing)
                } else {
                    Err(Reason::ClipDepth)
                };
            }
            Layer::Blend { .. } | Layer::Unblend | Layer::Mask => return Err(Reason::Layer),
            Layer::Backdrop => return Err(Reason::Backdrop),
            Layer::External => return Err(Reason::External),
            Layer::Text
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
        let Some(rr) = p.rect else {
            // `one` drops a rect-less blur, except the drop shadow `paint`
            // hands to a filter layer instead.
            return if crate::path_shadow(p) {
                Err(Reason::PathShadow)
            } else if blurred {
                Ok(Drawn::Nothing)
            } else {
                Err(Reason::Path)
            };
        };
        let [a, b, c, d, e, f] = xf.as_coeffs();
        if a * d - b * c == 0. {
            // Collapsed to a line: no area, nothing for `vello_cpu` to cover.
            return Ok(Drawn::Nothing);
        }
        let bounds = rr.bounds();
        let radius = rr.radius();
        out[0] = [a as f32, b as f32, c as f32, d as f32];
        let mut clip = self.clip();
        // An image is placed on the box as `one` places it; `Contain` also
        // clips to the rect the image occupies, so the padded edge does not
        // smear across the letterbox.
        let image = match &p.paint {
            Paint::Image { image, .. } if image.texture.is_some() => {
                return Err(Reason::ImageTexture);
            }
            // A buffer that does not match its size paints the solid stand-in.
            Paint::Image { image, fit } if crate::size(image).is_some() => {
                if image.width.max(image.height) > self.max_image {
                    return Err(Reason::ImageSize);
                }
                let t = crate::image_transform(image, *fit, bounds);
                if *fit == Fit::Contain {
                    let r = t.transform_rect_bbox(Rect::new(
                        0.,
                        0.,
                        f64::from(image.width),
                        f64::from(image.height),
                    ));
                    clip = clip.push(r, 0., xf).ok_or(Reason::ClipDepth)?;
                }
                Some((image, t))
            }
            _ => None,
        };
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
            Some((_, t)) => {
                let [sx, _, _, sy, tx, ty] = t.as_coeffs();
                out[9] = [tx as f32, ty as f32, sx as f32, sy as f32];
                (PAINT_IMAGE, 0.)
            }
            None => paint(out, &p.paint, bounds)?,
        };
        out[3] = [code(shape, kind), scale, radius as f32, p.width as f32];
        Ok(image.map_or(Drawn::One, |(image, _)| Drawn::Image(image)))
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
        let uniform = a > 0. && (a - d).abs() <= NEARLY_ZERO && b.abs() <= NEARLY_ZERO
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
