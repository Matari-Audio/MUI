//! The paint of a sharp entry: colour or ramp, as `vello_common` encodes
//! it, and what a layer folded into its instances does to it.

use super::Reason;
use super::build::{Instance, MAX_STOPS, PAINT_IMAGE, SHAPE_BLUR, SHAPE_BLUR_INVERT};
use crate::kurbo::Rect;
use mui_scene::{GradientKind, Paint};

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

/// A ramp of more than [`MAX_STOPS`] stops, drawn as `list[start..]`
/// with its first four: each instance again once per span between two
/// stops, as `(a, a, b, b)` with no colour outside `[a, b)` but the pads at
/// the ends. The shader picks a span by the same comparisons in each, so
/// every pixel takes its colour from exactly one instance and the rest add
/// exact zeros.
pub fn spans(list: &mut Vec<Instance>, start: usize, paint: &Paint) {
    let Paint::Gradient { stops, .. } = paint else {
        return;
    };
    let ramp = |i: &Instance| (1. ..=3.).contains(&(i[3][0] % 8.));
    if stops.len() <= MAX_STOPS || !list[start..].iter().all(ramp) {
        return;
    }
    let colour = |c| premul(crate::srgb(c).components).map(|v| v.clamp(0., 1.));
    let last = stops.len() - 2;
    let drawn = list.split_off(start);
    for k in 0..=last {
        let ((a, ca), (b, cb)) = (stops[k], stops[k + 1]);
        if a == b && k != 0 && k != last {
            continue;
        }
        let zero = [0.; 4];
        let (ca, cb) = (colour(ca), colour(cb));
        for one in &drawn {
            let mut one = *one;
            one[4] = if k == 0 { ca } else { zero };
            one[5] = ca;
            one[6] = cb;
            one[7] = if k == last { cb } else { zero };
            one[8] = [a, a, b, b];
            list.push(one);
        }
    }
}

/// A premultiplied colour, as `vello_cpu` premultiplies it.
pub(super) fn premul([r, g, b, a]: [f32; 4]) -> [f32; 4] {
    [r * a, g * a, b * a, a]
}

/// The paint of a sharp entry: slots 4-9, and its (paint kind, table scale).
/// Mirrors [`crate::brush`] and how `vello_common` encodes the result:
/// stops interpolate premultiplied in sRGB, extend pads, and the ramp is
/// sampled from a lookup table of the size `vello_common` picks.
pub(super) fn paint(out: &mut Instance, paint: &Paint, bounds: Rect) -> Result<(f32, f32), Reason> {
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
    let (first, last) = (stops[0].0, stops[stops.len() - 1].0);
    let padded: Vec<f32> = (first != 0.)
        .then_some(0.)
        .into_iter()
        .chain(stops.iter().map(|s| s.0))
        .chain((last != 1.).then_some(1.))
        .collect();
    let n = padded.len();
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
