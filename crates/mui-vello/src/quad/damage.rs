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
    /// The device rect (`x y w h`) a frame redraws when `paint` replaces
    /// `old` on a target that holds `old`: the old and new boxes of every
    /// changed entry, with the changed entries built to learn their new
    /// boxes. `None` when the frame must redraw whole: the list grew or
    /// shrank, a clip changed (it moves what is inside it), or the damage
    /// is past [`PARTIAL`] of the target.
    pub(super) fn damage(
        &mut self,
        old: &[Painted],
        paint: &[Painted],
        transform: Affine,
        size: [u32; 2],
        touched: &[u64],
    ) -> Result<Option<[u32; 4]>, Error> {
        if old.len() != paint.len() {
            return Ok(None);
        }
        let changed = self.changed(old, paint, touched);
        if changed.iter().any(|&i| clip(&old[i]) || clip(&paint[i])) {
            return Ok(None);
        }
        let mut damage = changed
            .iter()
            .map(|&i| self.cpu.boxes[i])
            .fold(build::EMPTY, build::union);
        self.cpu.build(
            paint,
            transform,
            &|i, _| changed.binary_search(&i).is_ok(),
            None,
        )?;
        damage = changed
            .iter()
            .map(|&i| self.cpu.boxes[i])
            .fold(damage, build::union);
        Ok(partial(damage, size))
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
        Ok(partial(all, size))
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

/// `damage` (a device box) in whole pixels on a `size` target, or `None`
/// past [`PARTIAL`] of it.
fn partial(damage: [f32; 4], size: [u32; 2]) -> Option<[u32; 4]> {
    let [w, h] = size.map(|v| v as f32);
    let x0 = damage[0].floor().clamp(0., w);
    let y0 = damage[1].floor().clamp(0., h);
    let x1 = damage[2].ceil().clamp(x0, w);
    let y1 = damage[3].ceil().clamp(y0, h);
    if (x1 - x0) * (y1 - y0) > PARTIAL * w * h {
        return None;
    }
    Some([x0, y0, x1 - x0, y1 - y0].map(|v| v as u32))
}
