//! What a frame redraws: the damage of a list replacing the one the target
//! holds, carried through layers and backdrops.

use super::{Error, QuadRenderer, build, list::Welds};
use crate::kurbo::Affine;
use mui_scene::{Layer, Painted};

/// What the target holds after the last frame drawn on it.
pub(super) struct Retained {
    pub paint: Vec<Painted>,
    pub transform: Affine,
    pub size: [u32; 2],
    pub target: wgpu::TextureView,
}

/// A partial frame pays while its damage is at most this share of the
/// target; past it the frame redraws whole.
const PARTIAL: f32 = 0.5;

impl QuadRenderer {
    /// The device rects (`x y w h`) a frame redraws when `paint` replaces
    /// `old` on a target that holds `old`: the old and new boxes of every
    /// changed entry, gathered into at most [`RECTS`] rects, with the
    /// changed entries built to learn their new boxes. `None` when the
    /// frame must redraw whole: the list grew or shrank, a clip changed (it
    /// moves what is inside it), or the damage is past [`PARTIAL`] of the
    /// target.
    pub(super) fn damage(
        &mut self,
        old: &[Painted],
        paint: &[Painted],
        transform: Affine,
        size: [u32; 2],
        touched: &[u64],
    ) -> Result<Option<Rects>, Error> {
        if old.len() != paint.len() {
            return Ok(None);
        }
        let changed = self.changed(old, paint, touched);
        if changed.iter().any(|&i| clip(&old[i]) || clip(&paint[i])) {
            return Ok(None);
        }
        let before: Vec<_> = changed.iter().map(|&i| self.cpu.boxes[i]).collect();
        self.cpu.build(
            paint,
            transform,
            &|i, _| changed.binary_search(&i).is_ok(),
            None,
        )?;
        let boxes = before
            .into_iter()
            .zip(&changed)
            .map(|(old, &i)| build::union(old, self.cpu.boxes[i]));
        Ok(gather(boxes, size))
    }

    /// [`QuadRenderer::damage`] for a list with layers, backdrops or welds:
    /// built whole, for which targets it needs and what each holds, and
    /// the damage carried through them (see [`Layers::propagate`]).
    // ponytail: builds every entry even for a partial frame; build only
    // what the damage, the dirty layers and the dirty backdrops' snapshots
    // reach if a large layered list's CPU time matters.
    pub(super) fn layered(
        &mut self,
        paint: &[Painted],
        old: Option<&[Painted]>,
        transform: Affine,
        size: [u32; 2],
        welds: Option<Welds>,
        touched: &[u64],
    ) -> Result<Option<[u32; 4]>, Error> {
        let old = old.filter(|old| old.len() == paint.len());
        let before = old.map(|_| self.cpu.boxes.clone());
        self.cpu.build(paint, transform, &|_, _| true, welds)?;
        // Without what the target held, every target is drawn (as `begin`
        // marks them) and the frame whole.
        let (Some(old), Some(before)) = (old, before) else {
            return Ok(None);
        };
        let mut hit = self.changed(old, paint, touched);
        if hit.iter().any(|&i| clip(&old[i]) || clip(&paint[i])) {
            return Ok(None);
        }
        // A changed `Blend` changes pixels where its layer composites, at
        // its `Unblend`: not under a backdrop inside it.
        let mut closes = vec![0; paint.len()];
        let mut open = Vec::new();
        for (i, p) in paint.iter().enumerate() {
            match p.layer {
                Layer::Blend { .. } => open.push(i),
                Layer::Unblend => closes[open.pop().unwrap_or(i)] = i,
                _ => {}
            }
        }
        let boxes = &self.cpu.boxes;
        let mut damage: Vec<_> = hit
            .iter()
            .map(|&i| {
                let at = if closes[i] > i { closes[i] } else { i };
                (at, build::union(before[i], boxes[i]))
            })
            .collect();
        self.cpu
            .layers
            .propagate(&mut hit, &mut damage, boxes, &|i| before[i]);
        let all = damage
            .iter()
            .fold(build::EMPTY, |a, d| build::union(a, d.1));
        Ok(partial(all, size, PARTIAL))
    }

    /// The entries of `paint` that differ from `old`, or sample a host
    /// texture set since, or a weld redrawn this frame.
    fn changed(&self, old: &[Painted], paint: &[Painted], touched: &[u64]) -> Vec<usize> {
        let samples = |p: &Painted| match &p.paint {
            mui_scene::Paint::Image { image, .. } => {
                image.texture.is_some_and(|k| touched.contains(&k))
            }
            _ => p.layer == Layer::External && self.redrawn.iter().any(|k| **k == *p.key.as_ref()),
        };
        (0..paint.len())
            .filter(|&i| old[i] != paint[i] || samples(&paint[i]))
            .collect()
    }
}

fn clip(p: &Painted) -> bool {
    matches!(p.layer, Layer::Clip | Layer::Unclip)
}

/// Most rects a partial frame redraws: past this, nearby changes share one.
pub const RECTS: usize = 4;

/// The device rects (`x y w h`) a partial frame redrew, at most [`RECTS`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rects {
    at: [[u32; 4]; RECTS],
    len: usize,
}

impl Rects {
    pub(super) fn one(rect: [u32; 4]) -> Self {
        let mut at = [[0; 4]; RECTS];
        at[0] = rect;
        Self { at, len: 1 }
    }
    pub fn as_slice(&self) -> &[[u32; 4]] {
        &self.at[..self.len]
    }
    /// The box around them all.
    pub fn bounds(&self) -> [u32; 4] {
        let [x0, y0, x1, y1] = self.as_slice().iter().fold(
            [u32::MAX, u32::MAX, 0, 0],
            |[x0, y0, x1, y1], &[x, y, w, h]| [x0.min(x), y0.min(y), x1.max(x + w), y1.max(y + h)],
        );
        if x0 > x1 {
            [0; 4]
        } else {
            [x0, y0, x1 - x0, y1 - y0]
        }
    }
}

/// Device boxes gathered into at most [`RECTS`] whole-pixel rects on a
/// `size` target: a box joins a rect it overlaps, and past [`RECTS`] the two
/// whose union adds least area merge. `None` past [`PARTIAL`] of the target.
fn gather(boxes: impl IntoIterator<Item = [f32; 4]>, size: [u32; 2]) -> Option<Rects> {
    let area = |b: [f32; 4]| (b[2] - b[0]).max(0.) * (b[3] - b[1]).max(0.);
    let mut rects: Vec<[f32; 4]> = Vec::with_capacity(RECTS + 1);
    let add = |b: [f32; 4], rects: &mut Vec<[f32; 4]>| {
        if let Some(r) = rects.iter_mut().find(|r| build::overlaps(**r, b)) {
            *r = build::union(*r, b);
        } else {
            rects.push(b);
        }
    };
    for b in boxes {
        if b[2] <= b[0] || b[3] <= b[1] {
            continue;
        }
        add(b, &mut rects);
        while rects.len() > RECTS {
            let mut best = (0, 1, f32::INFINITY);
            for i in 0..rects.len() {
                for j in i + 1..rects.len() {
                    let grown =
                        area(build::union(rects[i], rects[j])) - area(rects[i]) - area(rects[j]);
                    if grown < best.2 {
                        best = (i, j, grown);
                    }
                }
            }
            let merged = build::union(rects[best.0], rects.swap_remove(best.1));
            rects.swap_remove(best.0);
            add(merged, &mut rects);
        }
    }
    let mut out = Rects::default();
    let mut total = 0.;
    for r in rects {
        let [x, y, w, h] = partial(r, size, 1.)?;
        if w > 0 && h > 0 {
            total += (w * h) as f32;
            out.at[out.len] = [x, y, w, h];
            out.len += 1;
        }
    }
    let [w, h] = size.map(|v| v as f32);
    (total <= PARTIAL * w * h).then_some(out)
}

/// `damage` (a device box) in whole pixels on a `size` target, or `None`
/// past `share` of it.
fn partial(damage: [f32; 4], size: [u32; 2], share: f32) -> Option<[u32; 4]> {
    let [w, h] = size.map(|v| v as f32);
    let x0 = damage[0].floor().clamp(0., w);
    let y0 = damage[1].floor().clamp(0., h);
    let x1 = damage[2].ceil().clamp(x0, w);
    let y1 = damage[3].ceil().clamp(y0, h);
    if (x1 - x0) * (y1 - y0) > share * w * h {
        return None;
    }
    Some([x0, y0, x1 - x0, y1 - y0].map(|v| v as u32))
}
