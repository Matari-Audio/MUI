//! Per-element damage policy shared by CPU and GPU retained renderers.
use crate::kurbo::{Affine, Rect};
use mui_geometry::PathCommand;
use mui_scene::{Layer, Painted, ShadowKind};
use std::collections::HashMap;

/// What changed since the frame the target holds.
#[derive(Clone, Copy)]
pub(crate) enum Change {
    None,
    /// Only `[x, y, w, h]`, in device pixels.
    Part([u32; 4]),
    Parts(Regions),
    Full,
}

/// A fixed set of disjoint tile boxes. Sparse changes must not repaint the
/// empty space between them; overflow falls back to the existing full render.
#[derive(Clone, Copy)]
pub(crate) struct Regions {
    pub(crate) rects: [[u32; 4]; 16],
    pub(crate) len: usize,
}
impl Regions {
    pub(crate) fn new() -> Self {
        Self {
            rects: [[0; 4]; 16],
            len: 0,
        }
    }
    pub(crate) fn add(&mut self, mut r: [u32; 4]) -> bool {
        if r[2] == 0 || r[3] == 0 {
            return true;
        }
        let mut i = 0;
        while i < self.len {
            let a = self.rects[i];
            if r[0] <= a[0] + a[2]
                && a[0] <= r[0] + r[2]
                && r[1] <= a[1] + a[3]
                && a[1] <= r[1] + r[3]
            {
                let x = r[0].min(a[0]);
                let y = r[1].min(a[1]);
                r = [
                    x,
                    y,
                    (r[0] + r[2]).max(a[0] + a[2]) - x,
                    (r[1] + r[3]).max(a[1] + a[3]) - y,
                ];
                self.len -= 1;
                self.rects[i] = self.rects[self.len];
                i = 0;
            } else {
                i += 1;
            }
        }
        if self.len == self.rects.len() {
            // Preserve bounded metadata without turning a local edit into a
            // global invalidation. Coalesce the affected boxes only.
            for old in &self.rects[..self.len] {
                let x = r[0].min(old[0]);
                let y = r[1].min(old[1]);
                r = [
                    x,
                    y,
                    (r[0] + r[2]).max(old[0] + old[2]) - x,
                    (r[1] + r[3]).max(old[1] + old[3]) - y,
                ];
            }
            self.len = 0;
        }
        self.rects[self.len] = r;
        self.len += 1;
        true
    }
    pub(crate) fn pixels(&self) -> u64 {
        self.rects[..self.len]
            .iter()
            .map(|r| u64::from(r[2]) * u64::from(r[3]))
            .sum()
    }
}

/// Shelf packing is bounded by 16 regions, with tile-aligned placements so
/// the rasterizer sees the same coordinates modulo whole tiles.
#[cfg(feature = "gpu-effects")]
pub(crate) struct PackedRegions {
    pub(crate) offsets: [[u32; 2]; 16],
    pub(crate) size: [u32; 2],
}
impl Regions {
    #[cfg(feature = "gpu-effects")]
    pub(crate) fn packed(&self, limit: u32) -> Option<PackedRegions> {
        let mut order = std::array::from_fn::<_, 16, _>(|i| i);
        order[..self.len].sort_unstable_by_key(|&i| std::cmp::Reverse(self.rects[i][3]));
        let widest = self.rects[..self.len]
            .iter()
            .map(|r| r[2].next_multiple_of(16))
            .max()?;
        let mut best: Option<PackedRegions> = None;
        let mut width = 0;
        for &i in &order[..self.len] {
            width += self.rects[i][2].next_multiple_of(16);
            if width < widest || width > limit {
                continue;
            }
            let mut offsets = [[0; 2]; 16];
            let (mut x, mut y, mut row) = (0, 0, 0);
            for &j in &order[..self.len] {
                let r = self.rects[j];
                let w = r[2].next_multiple_of(16);
                let h = r[3].next_multiple_of(16);
                if x + w > width {
                    y += row;
                    x = 0;
                    row = 0;
                }
                offsets[j] = [x, y];
                x += w;
                row = row.max(h);
            }
            let size = [width, y + row];
            let pixels = |s: [u32; 2]| u64::from(s[0]) * u64::from(s[1]);
            if size[1] <= limit && best.as_ref().is_none_or(|b| pixels(size) < pixels(b.size)) {
                best = Some(PackedRegions { offsets, size });
            }
        }
        best
    }
}

/// A change past this share of the target renders the whole frame: the
/// render is mostly per pixel, and the copy would come on top.
pub(crate) const PARTIAL: f64 = 0.5;

/// The empty box: what `Rect::union` leaves unchanged.
pub(crate) const NOTHING: Rect = Rect::new(
    f64::INFINITY,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::NEG_INFINITY,
);

/// Everything `p` can paint on, in scene units, or `None` when that is not
/// bounded short of the frame: a backdrop reads all under it, a weld may
/// animate, and an inset shadow fills its clip, which it does not carry.
/// Clip and layer bookkeeping paints nothing of its own.
pub(crate) fn reach(p: &Painted) -> Option<Rect> {
    let path = || {
        let mut r = NOTHING;
        for c in &p.path.commands {
            match *c {
                PathCommand::MoveTo(a) | PathCommand::LineTo(a) => r = r.union_pt((a.x, a.y)),
                PathCommand::CubicTo(a, b, c) => {
                    for q in [a, b, c] {
                        r = r.union_pt((q.x, q.y));
                    }
                }
                PathCommand::ArcTo(a) => {
                    let d = 2. * a.radius;
                    r = r.union(Rect::from_center_size((a.center.x, a.center.y), (d, d)));
                }
                PathCommand::Close => {}
            }
        }
        r
    };
    let r = match p.layer {
        Layer::External => return None,
        Layer::Backdrop => path().inflate(3. * p.blur, 3. * p.blur),
        Layer::Blend { .. } | Layer::Unblend | Layer::Unclip => return Some(NOTHING),
        Layer::Clip | Layer::Mask => path(),
        // Vello's blurred rect stops at 2.5 standard deviations.
        Layer::Shadow(_) => path().inflate(3. * p.blur, 3. * p.blur),
        _ => match &p.text {
            // As `replay` culls a run: its box is an estimate.
            Some(t) => {
                let pad = p.width + f64::from(t.size);
                crate::paint_box(p, &crate::kurbo::BezPath::new()).inflate(pad, pad)
            }
            None => path().inflate(p.width, p.width),
        },
    };
    // Local paths, placed; a text run's box is in scene space already.
    let r = match &p.text {
        Some(_) => r,
        None if r == NOTHING => r,
        None => r + crate::kurbo::Vec2::new(p.offset.x, p.offset.y),
    };
    let r = if r == NOTHING {
        r
    } else {
        p.transform.transform_rect_bbox(r)
    };
    (r == NOTHING || [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite())).then_some(r)
}

/// A bounded merge of paint streams. Inserted/deleted canvas draws must not
/// turn every following, unchanged sibling into damage. Exact equality is the
/// only reason to skip an entry; the small lookahead keeps this linear.
pub(crate) fn changed_paint(
    old: &[Painted],
    new: &[Painted],
    mut visit: impl FnMut(bool, usize) -> bool,
) -> bool {
    let (mut i, mut j) = (0, 0);
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            i += 1;
            j += 1;
            continue;
        }
        let same_slot = i < old.len()
            && j < new.len()
            && old[i].key == new[j].key
            && std::mem::discriminant(&old[i].layer) == std::mem::discriminant(&new[j].layer);
        if !same_slot && i < old.len() && j < new.len() {
            let a = old[i + 1..(i + 17).min(old.len())]
                .iter()
                .position(|p| p == &new[j]);
            let b = new[j + 1..(j + 17).min(new.len())]
                .iter()
                .position(|p| p == &old[i]);
            if let Some(n) = a.filter(|a| b.is_none_or(|b| *a <= b)) {
                for k in i..i + n + 1 {
                    if !visit(false, k) {
                        return false;
                    }
                }
                i += n + 1;
                continue;
            }
            if let Some(n) = b {
                for k in j..j + n + 1 {
                    if !visit(true, k) {
                        return false;
                    }
                }
                j += n + 1;
                continue;
            }
        }
        if i < old.len() {
            if !visit(false, i) {
                return false;
            }
            i += 1;
        }
        if j < new.len() {
            if !visit(true, j) {
                return false;
            }
            j += 1;
        }
    }
    true
}

pub(crate) fn paint_damage(
    old: &[Painted],
    paint: &[Painted],
    old_external: &HashMap<mui_scene::Id, Rect>,
    new_external: &HashMap<mui_scene::Id, Rect>,
    dirty: Rect,
    xf: Affine,
    size: [u32; 2],
) -> Change {
    let reach = |p: &Painted, external: &HashMap<mui_scene::Id, Rect>| {
        if p.layer == Layer::External {
            external.get(&p.key).copied()
        } else {
            reach(p)
        }
    };
    if old == paint && dirty == NOTHING {
        return Change::None;
    }
    let mut regions = Regions::new();
    let tile = |r: Rect| {
        let d = xf.transform_rect_bbox(r).inflate(2., 2.);
        let [w, h] = size.map(f64::from);
        let snap = |v: f64, up: bool, max: f64| {
            let t = v / 16.;
            (if up { t.ceil() } else { t.floor() } * 16.).clamp(0., max)
        };
        let (x0, y0) = (snap(d.x0, false, w), snap(d.y0, false, h));
        let (x1, y1) = (snap(d.x1, true, w), snap(d.y1, true, h));
        [x0, y0, x1 - x0, y1 - y0].map(|v| v as u32)
    };
    if dirty != NOTHING && !regions.add(tile(dirty)) {
        return Change::Full;
    }
    if !changed_paint(old, paint, |new, i| {
        let (list, external) = if new {
            (paint, new_external)
        } else {
            (old, old_external)
        };
        let p = &list[i];
        let Some(b) = reach(p, external) else {
            return false;
        };
        if b != NOTHING && !regions.add(tile(b)) {
            return false;
        }
        // A layer's blend or opacity reaches everything inside it.
        if matches!(p.layer, Layer::Blend { .. }) {
            let mut depth = 0;
            for q in &list[i..] {
                let Some(b) = reach(q, external) else {
                    return false;
                };
                if b != NOTHING && !regions.add(tile(b)) {
                    return false;
                }
                match q.layer {
                    Layer::Blend { .. } => depth += 1,
                    Layer::Unblend => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
        true
    }) {
        return Change::Full;
    }
    // A backdrop depends on paint beneath it even when its own declaration
    // stayed equal. Expand damage through overlapping effect footprints until
    // stable; unrelated widgets never become global invalidations.
    loop {
        let before = regions.pixels();
        for p in old
            .iter()
            .chain(paint)
            .filter(|p| p.layer == Layer::Backdrop)
        {
            if let Some(bounds) = reach(p, &HashMap::new()) {
                let r = tile(bounds);
                if regions.rects[..regions.len].iter().any(|a| {
                    a[0] < r[0] + r[2]
                        && r[0] < a[0] + a[2]
                        && a[1] < r[1] + r[3]
                        && r[1] < a[1] + a[3]
                }) {
                    regions.add(r);
                }
            }
        }
        if regions.pixels() == before {
            break;
        }
    }
    match regions.len {
        0 => Change::Part([0; 4]),
        1 => Change::Part(regions.rects[0]),
        _ => Change::Parts(regions),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui_scene::prelude::*;
    fn scene(x: f64, color: Color) -> mui_scene::ResolvedScene {
        mui_scene::resolve(&SceneSpec::new(
            stack![
                block(512., 384.)
                    .radius(0.)
                    .fill(Color::srgb(0.1, 0.1, 0.1))
                    .id("background"),
                block(32., 32.)
                    .radius(0.)
                    .fill(color)
                    .id("active")
                    .at(x, 32.),
                block(32., 32.)
                    .radius(0.)
                    .fill(Color::srgb(0., 1., 0.))
                    .id("untouched")
                    .at(320., 240.),
            ]
            .size(512., 384.),
        ))
        .unwrap()
    }
    fn local(before: &[Painted], after: &[Painted]) -> Vec<[u32; 4]> {
        let external = HashMap::new();
        match paint_damage(
            before,
            after,
            &external,
            &external,
            NOTHING,
            Affine::IDENTITY,
            [512, 384],
        ) {
            Change::None => Vec::new(),
            Change::Part(r) => vec![r],
            Change::Parts(r) => r.rects[..r.len].to_vec(),
            Change::Full => panic!("a single widget must never invalidate the whole window"),
        }
    }
    #[test]
    fn one_widget_style_change_automatically_damages_only_its_aa_tiles() {
        let before = scene(32., Color::srgb(1., 0., 0.));
        let after = scene(32., Color::srgb(0., 0., 1.));
        let rects = local(&before.paint, &after.paint);
        assert!(!rects.is_empty());
        for [x, y, w, h] in rects {
            assert!(x >= 16 && y >= 16 && x + w <= 80 && y + h <= 80);
        }
        assert!(local(&after.paint, &after.paint).is_empty());
    }
    #[test]
    fn drag_or_animation_automatically_covers_old_and_new_widget_bounds() {
        let before = scene(32., Color::srgb(1., 0., 0.));
        let after = scene(44., Color::srgb(1., 0., 0.));
        let rects = local(&before.paint, &after.paint);
        assert!(!rects.is_empty());
        for [x, y, w, h] in &rects {
            assert!(*x >= 16 && *y >= 16 && x + w <= 80 && y + h <= 80);
        }
        for (x, y) in [(32, 32), (63, 63), (44, 32), (75, 63)] {
            assert!(
                rects
                    .iter()
                    .any(|r| x >= r[0] && y >= r[1] && x < r[0] + r[2] && y < r[1] + r[3])
            );
        }
    }
}
