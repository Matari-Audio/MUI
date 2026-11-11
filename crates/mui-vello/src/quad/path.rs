//! Arbitrary outlines as records in a data texture, read by the fragment
//! shader: no mask is rasterised anywhere.
//!
//! A path is flattened once, at a tolerance of [`FLATTEN`] device pixels,
//! into line segments, which are binned into a grid of cells over its
//! device box. Each cell lists the segments that can touch a pixel whose
//! centre lies in it. One instance then draws the whole path:
//!
//! - a fill takes the exact box-filter area of the pixel under the outline,
//!   from signed areas summed along a ray to +x, non-zero
//!   (`min(|winding|, 1)`, as `vello_cpu` resolves it). The ray needs every
//!   segment to the pixel's right, so a fill's cells are horizontal bands
//!   (Slug's bands, Lengyel 2017; Rune's `outline.wgsl` reads glyph records
//!   the same way). Its segments are stored in device orientation, the
//!   entry's linear transform applied, so the area is exact under rotation
//!   and shear too.
//! - a stroke is the union of capsules, one per segment (Rune's
//!   `KIND_SEGMENT`), taken in one fragment as the distance to the nearest
//!   segment, so a translucent stroke covers each pixel once where Rune's
//!   per-segment instances blend twice at every joint. Round joins and caps
//!   come out exact, as `kurbo`'s default stroke asks. Its segments are
//!   stored in local space and its distance measured there, the pen being a
//!   local circle; its cells are squares, since a distance only needs the
//!   segments near the pixel.
//!
//! Records are keyed by the path's `Arc` (held, so its address stays its
//! own), the linear part of the transform and the stroke width: a node that
//! moves or scrolls keeps its record, and a steady frame builds and uploads
//! nothing. A record no entry used this frame is free for reuse the moment
//! space runs short: what is on a kept target was drawn already, and every
//! instance drawn this frame was built this frame.
//!
//! Record layout, one `vec4<f32>` texel each, in a texture [`WIDTH`]
//! texels wide read by linear index:
//!
//! | texel | contents |
//! |-------|----------|
//! | 0 | grid origin `x y`, `1 / cell width`, `1 / cell height` (device, translation-free) |
//! | 1 | columns, rows, unused, unused |
//! | 2.. | per cell, row-major: first segment texel, segment count |
//! | then | segments, `x0 y0 x1 y1`, each repeated in every cell it reaches |

use crate::kurbo::{Affine, BezPath, PathEl, Point, Rect, Shape as _};
use mui_geometry::Path;
use std::collections::HashMap;
use std::sync::Arc;

/// Texels a row of the data texture.
pub const WIDTH: u32 = 256;

/// The default byte budget of the path data texture.
pub const PATH_BUDGET: usize = 4 << 20;

/// Flattening tolerance, device pixels. `vello_cpu` flattens at 0.25.
const FLATTEN: f64 = 0.1;

/// Most cells a record's grid takes.
const MAX_CELLS: f64 = 4096.;

/// Why a path has no record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fail {
    /// The geometry kernel refused the path (a malformed arc).
    Invalid,
    /// It does not fit the budget alongside this frame's other records.
    Full,
}

/// A path's record and its local bounding box; `None` record for a path
/// with nothing to cover.
#[derive(Clone, Copy, Debug)]
pub struct Found {
    pub record: Option<u32>,
    pub bounds: Rect,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    path: usize,
    linear: [u64; 4],
    width: u64,
}

struct Entry {
    /// Held so its address cannot be reused by another path.
    _path: Arc<Path>,
    at: u32,
    len: u32,
    found: Found,
    used: u64,
}

/// The CPU side of the data texture: records, free space and what changed.
pub struct Store {
    map: HashMap<Key, Entry>,
    /// Free ranges, `(start, len)` in texels, sorted and coalesced.
    free: Vec<(u32, u32)>,
    /// The texture's contents, `capacity` texels.
    pub texels: Vec<[f32; 4]>,
    /// Ranges written since the last upload, `(start, len)`.
    pub dirty: Vec<(u32, u32)>,
    /// Texels the budget allows, a whole number of rows.
    max: u32,
    frame: u64,
    /// Records built this frame: none, in a steady one.
    pub built: u64,
    /// The store was emptied this frame.
    pub flushed: bool,
    bez: BezPath,
    local: Vec<[f32; 4]>,
    pairs: Vec<(u32, u32)>,
}

impl Store {
    /// A store of at most `budget` bytes, and at most `max_rows` rows.
    pub fn new(budget: usize, max_rows: u32) -> Self {
        let rows = (budget / (16 * WIDTH as usize)).clamp(1, max_rows as usize) as u32;
        Self {
            map: HashMap::new(),
            free: Vec::new(),
            texels: Vec::new(),
            dirty: Vec::new(),
            max: rows * WIDTH,
            frame: 0,
            built: 0,
            flushed: false,
            bez: BezPath::new(),
            local: Vec::new(),
            pairs: Vec::new(),
        }
    }

    /// Rows the texture needs.
    pub fn rows(&self) -> u32 {
        (self.texels.len() as u32).div_ceil(WIDTH)
    }

    /// Start a frame: records not used from now on become reusable.
    pub fn begin(&mut self) {
        self.frame += 1;
        self.built = 0;
        self.flushed = false;
    }

    /// Forget every record: what a frame does when its records do not fit,
    /// before it tries again with the whole budget free.
    pub fn flush(&mut self) {
        self.flushed = true;
        self.map.clear();
        self.dirty.clear();
        self.free.clear();
        if !self.texels.is_empty() {
            self.free.push((0, self.texels.len() as u32));
        }
    }

    /// The record of `path` under the linear part of `xf`, stroked `width`
    /// wide (`0` fills), built if this frame is the first to ask.
    pub fn find(&mut self, path: &Arc<Path>, xf: Affine, width: f64) -> Result<Found, Fail> {
        let [a, b, c, d, ..] = xf.as_coeffs();
        let key = Key {
            path: Arc::as_ptr(path) as usize,
            linear: [a, b, c, d].map(f64::to_bits),
            width: width.to_bits(),
        };
        if let Some(e) = self.map.get_mut(&key) {
            e.used = self.frame;
            return Ok(e.found);
        }
        let linear = Affine::new([a, b, c, d, 0., 0.]);
        crate::bez_path_into(path, crate::ARC_TOLERANCE, &mut self.bez)
            .map_err(|_| Fail::Invalid)?;
        let bounds = self.bez.bounding_box();
        let found = |record| Found { record, bounds };
        let Some(mut record) = self.record(linear, width) else {
            return Ok(found(None));
        };
        let len = record.len() as u32;
        let at = self.alloc(len).ok_or(Fail::Full)?;
        // Cell entries name absolute texels.
        let cells = (record[1][0] * record[1][1]) as usize;
        for cell in &mut record[2..2 + cells] {
            cell[0] += at as f32;
        }
        self.texels[at as usize..(at + len) as usize].copy_from_slice(&record);
        self.dirty.push((at, len));
        self.built += 1;
        let found = found(Some(at));
        self.map.insert(
            key,
            Entry {
                _path: path.clone(),
                at,
                len,
                found,
                used: self.frame,
            },
        );
        Ok(found)
    }

    /// The record of `self.bez` under `linear`, with cell entries relative
    /// to the record's start; `None` when it covers nothing.
    fn record(&mut self, linear: Affine, width: f64) -> Option<Vec<[f32; 4]>> {
        let [a, b, c, d, ..] = linear.as_coeffs();
        let e = 0.5 * (a * a + b * b + c * c + d * d);
        let smax = (e + (e * e - (a * d - b * c).powi(2)).max(0.).sqrt()).sqrt();
        let stroke = width > 0.;
        segments(&self.bez, FLATTEN / smax, !stroke, &mut self.local);
        if self.local.is_empty() {
            return None;
        }
        // Device orientation, translation-free: what the grid bins.
        let device: Vec<[f32; 4]> = self
            .local
            .iter()
            .map(|s| {
                let p = linear * Point::new(f64::from(s[0]), f64::from(s[1]));
                let q = linear * Point::new(f64::from(s[2]), f64::from(s[3]));
                [p.x as f32, p.y as f32, q.x as f32, q.y as f32]
            })
            .collect();
        let mut lo = [f32::INFINITY; 2];
        let mut hi = [f32::NEG_INFINITY; 2];
        for s in &device {
            lo = [lo[0].min(s[0]).min(s[2]), lo[1].min(s[1]).min(s[3])];
            hi = [hi[0].max(s[0]).max(s[2]), hi[1].max(s[1]).max(s[3])];
        }
        // How far from a segment a pixel centre can still be covered by
        // it: half a pixel's diagonal, plus the pen's reach.
        let pen = (0.5 * width * smax) as f32;
        let reach = if stroke { pen + 0.75 } else { 0.5 };
        // The grid covers every pixel centre that can be covered, so a
        // fragment outside it covers nothing.
        let margin = reach + 1.;
        let (x0, y0) = (lo[0] - margin, lo[1] - margin);
        let (w, h) = (hi[0] - lo[0] + 2. * margin, hi[1] - lo[1] + 2. * margin);
        let (cw, ch) = if stroke {
            let side = (f64::from(w * h) / MAX_CELLS).sqrt().max(8.) as f32;
            (side, side)
        } else {
            (w, (f64::from(h) / MAX_CELLS).max(4.) as f32)
        };
        let nx = (w / cw).ceil().max(1.) as u32;
        let ny = (h / ch).ceil().max(1.) as u32;
        let row = |y: f32| ((y - y0) / ch).floor().clamp(0., (ny - 1) as f32) as u32;
        let col = |x: f32| ((x - x0) / cw).floor().clamp(0., (nx - 1) as f32) as u32;
        self.pairs.clear();
        for (i, s) in device.iter().enumerate() {
            let (sy0, sy1) = (s[1].min(s[3]), s[1].max(s[3]));
            if !stroke && s[1] == s[3] {
                // A horizontal edge crosses no row: it adds no area.
                continue;
            }
            for r in row(sy0 - reach)..=row(sy1 + reach) {
                let (cols0, cols1) = if stroke {
                    // The segment's x extent within this row, widened.
                    let (ry0, ry1) = (y0 + r as f32 * ch - reach, y0 + (r + 1) as f32 * ch + reach);
                    let (xa, xb) = span(s, ry0, ry1);
                    (col(xa - reach), col(xb + reach))
                } else {
                    (0, 0)
                };
                for c in cols0..=cols1 {
                    self.pairs.push((r * nx + c, i as u32));
                }
            }
        }
        self.pairs.sort_unstable();
        let cells = (nx * ny) as usize;
        let mut out = Vec::with_capacity(2 + cells + self.pairs.len());
        out.push([x0, y0, 1. / cw, 1. / ch]);
        out.push([nx as f32, ny as f32, 0., 0.]);
        let first = (2 + cells) as f32;
        let mut at = 0;
        for cell in 0..cells as u32 {
            let start = at;
            while at < self.pairs.len() && self.pairs[at].0 == cell {
                at += 1;
            }
            out.push([first + start as f32, (at - start) as f32, 0., 0.]);
        }
        let stored = if stroke { &self.local } else { &device };
        out.extend(self.pairs.iter().map(|&(_, i)| stored[i as usize]));
        Some(out)
    }

    /// `len` free texels: first fit, then after freeing every record this
    /// frame has not used, then in a texture grown up to the budget.
    fn alloc(&mut self, len: u32) -> Option<u32> {
        if let Some(at) = self.fit(len) {
            return Some(at);
        }
        let frame = self.frame;
        let stale: Vec<(u32, u32)> = self
            .map
            .values()
            .filter(|e| e.used != frame)
            .map(|e| (e.at, e.len))
            .collect();
        if !stale.is_empty() {
            self.map.retain(|_, e| e.used == frame);
            for (at, len) in stale {
                self.release(at, len);
            }
            if let Some(at) = self.fit(len) {
                return Some(at);
            }
        }
        let old = self.texels.len() as u32;
        let tail = self
            .free
            .last()
            .filter(|f| f.0 + f.1 == old)
            .map_or(0, |f| f.1);
        let want = (old - tail + len)
            .max(2 * old)
            .min(self.max)
            .next_multiple_of(WIDTH);
        if want > old && want - old + tail >= len {
            self.texels.resize(want as usize, [0.; 4]);
            self.release(old, want - old);
            return self.fit(len);
        }
        None
    }

    fn fit(&mut self, len: u32) -> Option<u32> {
        let i = self.free.iter().position(|f| f.1 >= len)?;
        let (at, have) = self.free[i];
        if have == len {
            self.free.remove(i);
        } else {
            self.free[i] = (at + len, have - len);
        }
        Some(at)
    }

    fn release(&mut self, at: u32, len: u32) {
        let i = self.free.partition_point(|f| f.0 < at);
        self.free.insert(i, (at, len));
        // Coalesce with the next range, then the previous one.
        if i + 1 < self.free.len() && at + len == self.free[i + 1].0 {
            self.free[i].1 += self.free[i + 1].1;
            self.free.remove(i + 1);
        }
        if i > 0 && self.free[i - 1].0 + self.free[i - 1].1 == at {
            self.free[i - 1].1 += self.free[i].1;
            self.free.remove(i);
        }
    }
}

/// The x extent of segment `s` between rows `y0` and `y1`.
fn span(s: &[f32; 4], y0: f32, y1: f32) -> (f32, f32) {
    let (dx, dy) = (s[2] - s[0], s[3] - s[1]);
    if dy == 0. {
        return (s[0].min(s[2]), s[0].max(s[2]));
    }
    let t = |y: f32| ((y - s[1]) / dy).clamp(0., 1.);
    let (xa, xb) = (s[0] + dx * t(y0), s[0] + dx * t(y1));
    (xa.min(xb), xa.max(xb))
}

/// `bez` flattened within `tolerance` into `out` as line segments. With
/// `close`, every subpath is closed, as a fill closes it.
fn segments(bez: &BezPath, tolerance: f64, close: bool, out: &mut Vec<[f32; 4]>) {
    out.clear();
    let mut start = Point::ZERO;
    let mut last = Point::ZERO;
    let push = |a: Point, b: Point, out: &mut Vec<[f32; 4]>| {
        if a != b {
            out.push([a.x as f32, a.y as f32, b.x as f32, b.y as f32]);
        }
    };
    crate::kurbo::flatten(bez, tolerance, |el| match el {
        PathEl::MoveTo(p) => {
            if close {
                push(last, start, out);
            }
            (start, last) = (p, p);
        }
        PathEl::LineTo(p) => {
            push(last, p, out);
            last = p;
        }
        PathEl::ClosePath => {
            push(last, start, out);
            last = start;
        }
        // `flatten` emits lines only.
        _ => {}
    });
    if close {
        push(last, start, out);
    }
}
