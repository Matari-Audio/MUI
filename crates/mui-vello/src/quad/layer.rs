//! Group layers, masks and backdrops: which instances draw into which
//! target, and in what order the targets are drawn.
//!
//! Every instance stays in the one instance buffer, in device coordinates.
//! A *run* is a contiguous range of it drawn into one *surface*: the frame
//! (surface 0), or an offscreen target. A group layer that needs a target
//! of its own gets one sized to its device box and is composited onto its
//! parent by a single instance; a backdrop is a snapshot of what its
//! surfaces drew before it, blurred, sampled by its outline's instance.
//!
//! A group layer takes a target only when it must (after Rune's isolated
//! opacity, which keeps opaque containers and leaves on the direct path):
//!
//! - its blend mode is not `Normal`, or it holds a blending child (which
//!   blends with what this layer drew, not with what is under it);
//! - it holds a mask (drawn source-atop what the layer drew);
//! - it holds a backdrop, at any depth (whose snapshot replays what the
//!   layer drew so far, without the layer's own opacity, as `replay` does);
//! - or its instances overlap, so that folding its opacity into each of
//!   them would blend the overlap twice.
//!
//! Otherwise its opacity is multiplied into its instances, which is exact
//! when no two of them cover the same pixel: the case GPUI and Rune's leaf
//! dimming take for every group, here only when it is provably the same.
//! A layer at opacity 0 draws nothing at all.

use super::Reason;
use super::build::{EMPTY, Instance, PAINT_SCREEN, SLOTS, code, device_box, fade, overlaps, union};
use std::sync::Arc;

/// Instances past which a group layer is not tested for overlap: it takes
/// a target.
// ponytail: an all-pairs test; a sweep over sorted boxes if layers with
// many disjoint children turn up.
const FOLD_MAX: usize = 32;

/// The default byte budget for offscreen targets in one frame.
pub const OFFSCREEN_BUDGET: u64 = 64 << 20;

/// What a run's instances sample through bind group 1.
#[derive(Clone, Debug)]
pub enum Bind {
    /// Nothing: the run takes any binding a neighbour needs.
    None,
    Image(mui_scene::Image),
    /// A registered host texture, by key.
    Host(u64),
    /// A weld's texture, by its allocation id.
    Weld(u64, wgpu::Texture),
    /// The target of surface `0`.
    Surface(u16),
    /// A group layer's target, and the snapshot of what it blends onto.
    Blend(u16, u16),
}

impl Bind {
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Image(a), Self::Image(b)) => Arc::ptr_eq(&a.rgba, &b.rgba),
            (Self::Host(a), Self::Host(b)) => a == b,
            (Self::Weld(a, _), Self::Weld(b, _)) => a == b,
            (Self::Surface(a), Self::Surface(b)) => a == b,
            (Self::Blend(a, x), Self::Blend(b, y)) => a == b && x == y,
            _ => false,
        }
    }

    /// Whether a run binding `self` can take an instance binding `next`.
    fn joins(&self, next: &Self) -> bool {
        matches!(self, Self::None) || matches!(next, Self::None) || self.same(next)
    }
}

/// Instances `start..` (to the next run's start) drawn into `surface`.
#[derive(Clone, Debug)]
pub struct Run {
    pub start: u32,
    pub surface: u16,
    pub bind: Bind,
    /// Drawn source-atop: a mask.
    pub atop: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// The frame, or a group layer: its own runs.
    Group,
    /// A group layer whose instances were folded into its parent's, or
    /// which covers nothing: no target.
    Folded,
    /// What a blending layer composites onto: its parent's runs before it.
    Under,
    /// What was drawn before a backdrop, blurred by `sigma` device pixels
    /// at one texel per `scale` pixels.
    Backdrop { scale: u32, sigma: f32 },
}

#[derive(Clone, Debug)]
pub struct Surface {
    pub kind: Kind,
    /// The entry that made it (its `Blend` or `Backdrop`), and the last
    /// entry inside it (its `Unblend`; the backdrop itself).
    pub entries: [usize; 2],
    /// Device `x y w h`.
    pub rect: [u32; 4],
    /// A snapshot replays these surfaces' runs that start before `upto`.
    pub sources: Vec<u16>,
    pub upto: u32,
    /// Drawn this frame, rather than kept from the last.
    pub dirty: bool,
}

impl Surface {
    /// Its target's texels.
    pub fn texels(&self) -> [u32; 2] {
        match self.kind {
            Kind::Backdrop { scale, .. } => [self.rect[2], self.rect[3]].map(|v| v.div_ceil(scale)),
            _ => [self.rect[2], self.rect[3]],
        }
    }
}

struct Open {
    /// Its `Blend` entry.
    entry: usize,
    /// At opacity 0, or inside such a layer: no surface.
    hidden: bool,
    surface: u16,
    first: u32,
    first_run: usize,
    mix: u32,
    opacity: f32,
    isolate: bool,
}

/// The layer structure of one frame.
pub struct Layers {
    pub runs: Vec<Run>,
    pub surfaces: Vec<Surface>,
    /// Surfaces in the order their targets are drawn: each after every
    /// surface it samples. The frame is drawn last and is not listed.
    pub passes: Vec<u16>,
    open: Vec<Open>,
    /// Layers open at opacity 0 (or inside one): nothing is drawn.
    hidden: usize,
    /// Bytes of offscreen target this frame asks for, against `budget`.
    pub bytes: u64,
    pub budget: u64,
}

impl Default for Layers {
    fn default() -> Self {
        Self {
            runs: Vec::new(),
            surfaces: Vec::new(),
            passes: Vec::new(),
            open: Vec::new(),
            hidden: 0,
            bytes: 0,
            budget: OFFSCREEN_BUDGET,
        }
    }
}

/// Bytes of an `Rgba8Unorm` target of `texels`, rounded up to the 64-pixel
/// buckets the pool allocates in.
pub fn bucket_bytes(texels: [u32; 2]) -> u64 {
    let [w, h] = texels.map(|v| u64::from(v.div_ceil(64) * 64));
    4 * w * h
}

impl Layers {
    pub fn begin(&mut self, size: [u32; 2]) {
        self.runs.clear();
        self.surfaces.clear();
        self.passes.clear();
        self.open.clear();
        self.hidden = 0;
        self.bytes = 0;
        self.surfaces.push(Surface {
            kind: Kind::Group,
            entries: [0, usize::MAX],
            rect: [0, 0, size[0], size[1]],
            sources: Vec::new(),
            upto: 0,
            dirty: true,
        });
    }

    /// Whether what is drawn now is thrown away: inside a layer at opacity 0.
    pub fn hidden(&self) -> bool {
        self.hidden > 0
    }

    /// The instance at `at` (and those after it up to the next call) is
    /// drawn into the innermost open layer, binding `bind`.
    pub fn push(&mut self, at: u32, bind: Bind, atop: bool) {
        let surface = self.open.last().map_or(0, |o| o.surface);
        match self.runs.last_mut() {
            Some(r) if r.surface == surface && r.atop == atop && r.bind.joins(&bind) => {
                if matches!(r.bind, Bind::None) {
                    r.bind = bind;
                }
            }
            _ => self.runs.push(Run {
                start: at,
                surface,
                bind,
                atop,
            }),
        }
    }

    /// A group layer opens at entry `entry`, its first instance at `first`.
    pub fn open(&mut self, entry: usize, first: u32, mix: u32, opacity: f32) {
        let hidden = self.hidden > 0 || opacity.is_nan() || opacity <= 0.;
        self.hidden += usize::from(hidden);
        let surface = if hidden {
            u16::MAX
        } else {
            self.surfaces.len() as u16
        };
        self.open.push(Open {
            entry,
            hidden,
            surface,
            first,
            first_run: self.runs.len(),
            mix,
            opacity: opacity.min(1.),
            isolate: mix != 0,
        });
        if hidden {
            return;
        }
        self.surfaces.push(Surface {
            kind: Kind::Group,
            entries: [entry, entry],
            rect: [0; 4],
            sources: Vec::new(),
            upto: first,
            dirty: true,
        });
    }

    /// A mask is drawn next: its layer needs a target. `false` outside any
    /// layer, where there is nothing to draw it atop.
    pub fn mask(&mut self) -> bool {
        self.open.last_mut().map(|o| o.isolate = true).is_some()
    }

    /// A backdrop's outline, `one`, is drawn next at `at`: the snapshot of
    /// `region` it samples is the frame's runs and every open layer's
    /// before it. Every open layer needs a target, so that what each drew
    /// so far is in it unfolded.
    pub fn backdrop(&mut self, entry: usize, at: u32, region: [u32; 4], kind: Kind) -> u16 {
        let mut sources = vec![0];
        for o in &mut self.open {
            // Only reached outside hidden layers, where every open one has
            // a surface.
            o.isolate = true;
            sources.push(o.surface);
        }
        let id = self.surfaces.len() as u16;
        self.surfaces.push(Surface {
            kind,
            entries: [entry, entry],
            rect: region,
            sources,
            upto: at,
            dirty: true,
        });
        self.passes.push(id);
        self.push(at, Bind::Surface(id), false);
        id
    }

    /// Charge `texels` of target to the budget.
    fn charge(&mut self, texels: [u32; 2]) -> Result<(), Reason> {
        self.bytes += bucket_bytes(texels);
        if self.bytes > self.budget {
            return Err(Reason::Offscreen);
        }
        Ok(())
    }

    /// Charge a backdrop's snapshot and its blur's three low targets.
    pub fn charge_backdrop(&mut self, id: u16) -> Result<(), Reason> {
        let s = &self.surfaces[usize::from(id)];
        let (full, low) = ([s.rect[2], s.rect[3]], s.texels());
        self.charge(full)?;
        (0..3).try_for_each(|_| self.charge(low))
    }

    /// The innermost layer closes at entry `entry`: folded into its parent,
    /// or given a target and composited by an instance pushed onto `list`.
    /// Its `Blend` entry, and its device box, which damage reads as the box
    /// of both its `Blend` and its `Unblend`.
    pub fn close(
        &mut self,
        entry: usize,
        list: &mut Vec<Instance>,
        size: [u32; 2],
    ) -> Result<(usize, [f32; 4]), Reason> {
        let o = self.open.pop().ok_or(Reason::Layer)?;
        if o.hidden {
            self.hidden -= 1;
            return Ok((o.entry, EMPTY));
        }
        let id = o.surface;
        let parent = self.open.last().map_or(0, |p| p.surface);
        self.surfaces[usize::from(id)].entries[1] = entry;
        // Its instances: those in its runs, not in its children's.
        let end = list.len() as u32;
        let mut ranges = Vec::new();
        for (k, r) in self.runs.iter().enumerate().skip(o.first_run) {
            if r.surface == id {
                let to = self.runs.get(k + 1).map_or(end, |n| n.start);
                ranges.push(r.start as usize..to as usize);
            }
        }
        let count: usize = ranges.iter().map(ExactSizeIterator::len).sum();
        let boxes: Vec<[f32; 4]> = ranges
            .iter()
            .flat_map(|r| list[r.clone()].iter().map(device_box))
            .collect();
        let bounds = boxes.iter().copied().fold(EMPTY, union);
        let fold = !o.isolate
            && count <= FOLD_MAX
            && boxes
                .iter()
                .enumerate()
                .all(|(k, a)| boxes[k + 1..].iter().all(|b| !overlaps(*a, *b)));
        if fold {
            for r in ranges {
                list[r].iter_mut().for_each(|i| fade(i, o.opacity));
            }
            for r in &mut self.runs[o.first_run..] {
                if r.surface == id {
                    r.surface = parent;
                }
            }
            self.surfaces[usize::from(id)].kind = Kind::Folded;
            return Ok((o.entry, bounds));
        }
        // Its target: its box on the frame, in whole pixels.
        let [w, h] = size.map(|v| v as f32);
        let x0 = bounds[0].max(0.).floor();
        let y0 = bounds[1].max(0.).floor();
        let x1 = bounds[2].min(w).ceil();
        let y1 = bounds[3].min(h).ceil();
        if x1 <= x0 || y1 <= y0 {
            self.surfaces[usize::from(id)].kind = Kind::Folded;
            return Ok((o.entry, EMPTY));
        }
        let rect = [x0, y0, x1 - x0, y1 - y0].map(|v| v as u32);
        self.surfaces[usize::from(id)].rect = rect;
        self.charge([rect[2], rect[3]])?;
        let bind = if o.mix == 0 {
            Bind::Surface(id)
        } else {
            // It blends with what its parent drew before it, so the parent
            // cannot be folded into what is under it.
            if let Some(p) = self.open.last_mut() {
                p.isolate = true;
            }
            let under = self.surfaces.len() as u16;
            self.surfaces.push(Surface {
                kind: Kind::Under,
                entries: [entry, entry],
                rect,
                sources: vec![parent],
                upto: o.first,
                dirty: true,
            });
            self.charge([rect[2], rect[3]])?;
            self.passes.push(under);
            Bind::Blend(id, under)
        };
        self.passes.push(id);
        let mut one: Instance = [[0.; 4]; SLOTS];
        one[0] = [1., 0., 0., 1.];
        one[2] = [x0, y0, x1, y1];
        one[3] = [code(0., PAINT_SCREEN), o.mix as f32, 0., 0.];
        one[4] = [o.opacity, x1 - x0, y1 - y0, 0.];
        one[9] = [x0, y0, 1., 1.];
        // Unclipped: its instances carry every clip around it already.
        one[10] = [-1e9, -1e9, 1e9, 1e9];
        self.push(list.len() as u32, bind, false);
        list.push(one);
        Ok((o.entry, [x0, y0, x1, y1]))
    }

    /// The list ended: a layer still open is malformed, refused at its
    /// `Blend`.
    pub fn finish(&mut self) -> Result<(), (usize, Reason)> {
        if let Some(o) = self.open.first() {
            return Err((o.entry, Reason::Layer));
        }
        // Folding moved runs onto their parents: join neighbours again.
        let mut joined: Vec<Run> = Vec::with_capacity(self.runs.len());
        for r in self.runs.drain(..) {
            match joined.last_mut() {
                Some(j) if j.surface == r.surface && j.atop == r.atop && j.bind.joins(&r.bind) => {
                    if matches!(j.bind, Bind::None) {
                        j.bind = r.bind;
                    }
                }
                _ => joined.push(r),
            }
        }
        self.runs = joined;
        Ok(())
    }

    /// Whether anything but the frame itself is drawn.
    pub fn offscreen(&self) -> bool {
        !self.passes.is_empty()
    }

    /// Mark which kept targets must be drawn again, given the entries
    /// whose pixels changed (`hit`, sorted) and the device boxes they
    /// changed (`damage`, by entry). A backdrop is dirty when something
    /// drawn before it changed within its region, and then its outline's
    /// box changes too; a group layer is dirty when anything inside it
    /// changed. Returns the boxes the frame must redraw, those included.
    pub fn propagate(
        &mut self,
        hit: &mut Vec<usize>,
        damage: &mut Vec<(usize, [f32; 4])>,
        boxes: &[[f32; 4]],
        old: &dyn Fn(usize) -> [f32; 4],
    ) {
        // Each where it takes effect: a backdrop at its entry, a layer
        // where it closes, after every backdrop inside it.
        let mut order: Vec<usize> = (1..self.surfaces.len()).collect();
        order.sort_by_key(|&s| {
            let surface = &self.surfaces[s];
            match surface.kind {
                Kind::Group => surface.entries[1],
                _ => surface.entries[0],
            }
        });
        for s in order {
            let surface = &mut self.surfaces[s];
            let [first, last] = surface.entries;
            match surface.kind {
                Kind::Backdrop { .. } => {
                    let r = surface.rect.map(|v| v as f32);
                    let region = [r[0], r[1], r[0] + r[2], r[1] + r[3]];
                    let under = damage
                        .iter()
                        .any(|&(i, b)| i < first && overlaps(b, region));
                    surface.dirty = under || hit.binary_search(&first).is_ok();
                    if surface.dirty {
                        damage.push((first, boxes[first]));
                        if let Err(at) = hit.binary_search(&first) {
                            hit.insert(at, first);
                        }
                    }
                }
                Kind::Group => {
                    let at = hit.partition_point(|&i| i <= first);
                    surface.dirty = hit.get(at).is_some_and(|&i| i < last);
                    if surface.dirty {
                        // Its whole box, old and new: what was folded may
                        // now composite, and the reverse.
                        damage.push((first, union(boxes[first], old(first))));
                    }
                }
                Kind::Under => surface.dirty = true,
                Kind::Folded => {}
            }
        }
    }
}
