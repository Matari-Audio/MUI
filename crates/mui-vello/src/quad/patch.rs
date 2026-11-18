//! Per-entry CPU fallback. An entry the pass refuses for what it paints (a
//! vector, a colour glyph run, an image the device cannot hold) is
//! rasterised alone by `vello_cpu`, at device resolution and cropped to its
//! reach, and drawn as an image in its place; a clip nested past what an
//! instance carries is rasterised with everything inside it. The rest of
//! the frame stays on the GPU. Only what the pass cannot draw around (a
//! malformed layer, an exhausted budget, a backdrop inside a deep clip)
//! still sends the frame to the CPU.

use super::Reason;
use crate::kurbo::{Affine, Point, Rect};
use mui_geometry::{Path, RoundedRect};
use mui_scene::{Fit, Layer, Paint, Painted};
use std::borrow::Cow;
use std::ops::Range;
use std::sync::Arc;

/// Most spans one frame patches before giving it to the CPU whole: past
/// this, one CPU frame is cheaper than many small ones.
const MOST: usize = 64;

/// A refused span of the list and the entries drawn in its place: the
/// image, then one empty entry for each other entry, so the list keeps its
/// length and its damage stays per entry.
struct Patch {
    span: Vec<Painted>,
    transform: Affine,
    size: [u32; 2],
    drawn: Vec<Painted>,
    used: bool,
}

#[derive(Default)]
pub(super) struct Patches {
    kept: Vec<Patch>,
    /// The spans patched last frame: tried first, so a steady frame walks
    /// its list once.
    last: Vec<Range<usize>>,
    resources: vello_cpu::Resources,
    cache: crate::Cache,
}

impl Patches {
    /// `paint` with last frame's patches applied where their spans are
    /// unchanged.
    pub(super) fn begin<'a>(
        &mut self,
        paint: &'a [Painted],
        transform: Affine,
        size: [u32; 2],
    ) -> Cow<'a, [Painted]> {
        for patch in &mut self.kept {
            patch.used = false;
        }
        let mut list = Cow::Borrowed(paint);
        for span in std::mem::take(&mut self.last) {
            let Some(entries) = paint.get(span.clone()) else {
                continue;
            };
            if let Some(patch) = self.find(entries, transform, size) {
                patch.used = true;
                list.to_mut()[span.clone()].clone_from_slice(&patch.drawn);
                self.last.push(span);
            }
        }
        list
    }

    /// Patch what entry `index` of `list`, refused for `reason`, stands
    /// for; whether it could be.
    pub(super) fn patch(
        &mut self,
        list: &mut Cow<'_, [Painted]>,
        index: usize,
        reason: Reason,
        transform: Affine,
        size: [u32; 2],
    ) -> bool {
        if !matches!(
            reason,
            Reason::Text | Reason::Image | Reason::ImageSize | Reason::Vector | Reason::ClipDepth
        ) || self.last.len() >= MOST
            || self.last.iter().any(|s| s.contains(&index))
        {
            return false;
        }
        let Some(span) = span(list, index) else {
            return false;
        };
        let entries = &list[span.clone()];
        let cached = self.find(entries, transform, size).map(|patch| {
            patch.used = true;
            patch.drawn.clone()
        });
        let Some(drawn) = cached.or_else(|| self.raster(entries, transform, size)) else {
            return false;
        };
        list.to_mut()[span.clone()].clone_from_slice(&drawn);
        self.last.push(span);
        true
    }

    /// Forget the rasters this frame did not use.
    pub(super) fn end(&mut self) {
        self.kept.retain(|patch| patch.used);
    }

    /// How many spans the last frame drew from a raster.
    pub(super) fn count(&self) -> usize {
        self.last.len()
    }

    /// `span` rasterised and kept; what is drawn in its place.
    fn raster(
        &mut self,
        span: &[Painted],
        transform: Affine,
        size: [u32; 2],
    ) -> Option<Vec<Painted>> {
        let (pixels, rect) =
            crate::raster_alone(span, transform, size, &mut self.resources, &mut self.cache)
                .ok()??;
        let image = image_entry(&span[0], pixels, transform, rect)?;
        let mut drawn = Vec::with_capacity(span.len());
        drawn.push(image);
        drawn.extend(span[1..].iter().map(empty));
        self.kept.push(Patch {
            span: span.to_vec(),
            transform,
            size,
            drawn: drawn.clone(),
            used: true,
        });
        Some(drawn)
    }

    fn find(&mut self, span: &[Painted], transform: Affine, size: [u32; 2]) -> Option<&mut Patch> {
        self.kept
            .iter_mut()
            .find(|p| p.transform == transform && p.size == size && p.span == span)
    }
}

/// What a refusal at `index` stands for: a clip with everything up to its
/// `Unclip`, or the entry alone.
fn span(list: &[Painted], index: usize) -> Option<Range<usize>> {
    if list[index].layer != Layer::Clip {
        return Some(index..index + 1);
    }
    let mut depth = 0_usize;
    for (i, p) in list.iter().enumerate().skip(index) {
        match p.layer {
            Layer::Clip => depth += 1,
            Layer::Unclip => {
                depth -= 1;
                if depth == 0 {
                    return Some(index..i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// An entry that draws nothing, for `p`'s place in a patched span.
fn empty(p: &Painted) -> Painted {
    Painted {
        key: p.key.clone(),
        layer: Layer::Fill,
        path: Arc::default(),
        paint: Paint::Solid(mui_scene::Color::oklcha(0., 0., 0., 0.)),
        rect: None,
        offset: Point::ZERO,
        transform: Affine::IDENTITY,
        width: 0.,
        blur: 0.,
        text: None,
    }
}

/// An entry painting `pixels` over device rect `[x, y, w, h]`, pixel for
/// pixel, in `entry`'s place.
fn image_entry(
    entry: &Painted,
    pixels: mui_scene::Image,
    transform: Affine,
    [x, y, w, h]: [u32; 4],
) -> Option<Painted> {
    let device = Rect::new(
        f64::from(x),
        f64::from(y),
        f64::from(x + w),
        f64::from(y + h),
    );
    let r = transform.inverse().transform_rect_bbox(device);
    if ![r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite()) {
        return None;
    }
    Some(Painted {
        key: entry.key.clone(),
        layer: Layer::Fill,
        path: Arc::new(Path::polyline(
            [(r.x0, r.y0), (r.x1, r.y0), (r.x1, r.y1), (r.x0, r.y1)].map(Point::from),
            true,
        )),
        paint: Paint::Image {
            image: Arc::new(pixels),
            fit: Fit::Fill,
        },
        rect: Some(RoundedRect::new(r, 0.).ok()?),
        offset: Point::ZERO,
        transform: Affine::IDENTITY,
        width: 0.,
        blur: 0.,
        text: None,
    })
}
