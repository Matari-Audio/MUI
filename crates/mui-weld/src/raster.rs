use crate::field::{distances, pixel_prepared, smooth_min};
use crate::{Error, Point, Rect, Request};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Baked {
    pub bounds: Rect,
    pub width: u32,
    pub height: u32,
    /// Straight sRGB RGBA, compatible with MUI's `Image::rgba`.
    pub rgba: Arc<[u8]>,
    /// Closed, oriented contours of the SAME field, in logical coordinates.
    /// Marching-triangle approximation at `1 / quality.scale` logical units.
    /// This is not exact Bézier Boolean geometry or infinite-resolution output.
    pub contours: Vec<Vec<Point>>,
}
impl Baked {
    pub fn bytes(&self) -> usize {
        self.contours.iter().fold(
            self.rgba
                .len()
                .saturating_add(std::mem::size_of::<Self>())
                .saturating_add(
                    self.contours
                        .capacity()
                        .saturating_mul(std::mem::size_of::<Vec<Point>>()),
                ),
            |bytes, c| {
                bytes.saturating_add(c.capacity().saturating_mul(std::mem::size_of::<Point>()))
            },
        )
    }
}

pub fn bake(request: &Request) -> Result<Baked, Error> {
    request.validate()?;
    let q = request.quality;
    // The CPU reference shares crisp border geometry for its analytic subset.
    // Arbitrary contours keep the existing general reference path.
    let boundary = if request.weld.reach == 0.0 && request.sources.len() <= 3 {
        let plates = request
            .sources
            .iter()
            .map(|s| match &s.shape {
                crate::Geometry::RoundedRect { bounds, radius } => Some(crate::boundary::Plate {
                    center: Point::new(
                        (bounds.x0 + bounds.x1) * 0.5,
                        (bounds.y0 + bounds.y1) * 0.5,
                    ),
                    half: crate::Vec2::new(bounds.width() * 0.5, bounds.height() * 0.5),
                    radius: *radius,
                    angle: 0.0,
                }),
                _ => None,
            })
            .collect::<Option<Vec<_>>>();
        plates.map(crate::boundary::Boundary::new).transpose()?
    } else {
        None
    };
    let mut ext = request.sources[0]
        .shape
        .bounds()
        .ok_or(Error::Invalid("source bounds"))?;
    for s in request.sources.iter().skip(1) {
        ext = ext.union(s.shape.bounds().ok_or(Error::Invalid("source bounds"))?);
    }
    let px = 1.0 / q.scale;
    // Smooth-min expansion is bounded above by k/2 = reach. Keeping this extent
    // constant through a morph also prevents texture-origin jitter at every tick.
    let pad = request.weld.reach + 2.0 * px;
    let x0 = ((ext.x0 - pad) * q.scale).floor() / q.scale;
    let y0 = ((ext.y0 - pad) * q.scale).floor() / q.scale;
    let wf = ((ext.x1 + pad - x0) * q.scale).ceil();
    let hf = ((ext.y1 + pad - y0) * q.scale).ceil();
    if !(1.0..=4096.0).contains(&wf) || !(1.0..=4096.0).contains(&hf) {
        return Err(Error::Budget);
    }
    let (w, h) = (wf as usize, hf as usize);
    let pixels = w.checked_mul(h).ok_or(Error::Budget)?;
    let vertices = (w + 1).checked_mul(h + 1).ok_or(Error::Budget)?;
    let cost = request
        .sources
        .iter()
        .map(|s| s.shape.cost() + 32)
        .sum::<usize>()
        + boundary.as_ref().map_or(0, |b| b.edges().len() * 4);
    let work = pixels
        .checked_add(vertices)
        .and_then(|n| n.checked_mul(cost))
        .ok_or(Error::Budget)?;
    if pixels > q.max_pixels || work > q.max_work {
        return Err(Error::Budget);
    }
    let bounds = Rect::new(x0, y0, x0 + w as f64 * px, y0 + h as f64 * px);
    let n = request.sources.len();
    let k = 2.0 * request.weld.reach * request.weld.amount();
    let mut values = vec![0.0; vertices];
    rows(&mut values, w + 1, 1, |y, row| {
        let py = y0 + y as f64 * px;
        for (x, v) in row.iter_mut().enumerate() {
            let p = Point::new(x0 + x as f64 * px, py);
            // Distance only: the lattice never reads the material weights.
            let d = boundary.as_ref().map_or_else(
                || smooth_min(&distances(&request.sources, p)[..n], k).0,
                |b| b.sample(p).distance,
            );
            if !d.is_finite() {
                return Err(Error::Invalid("non-finite field"));
            }
            // Symbolic outside perturbation at exact zeros gives each lattice edge
            // a distinct crossing key. No equality-dependent saddle cracks.
            *v = if d == 0.0 { px * 1e-10 } else { d };
        }
        Ok(())
    })?;
    let contours = contours(&values, w, h, bounds)?;
    let mut rgba = vec![0u8; pixels.checked_mul(4).ok_or(Error::Budget)?];
    rows(&mut rgba, 4 * w, 4, |y, row| {
        let py = y0 + (y as f64 + 0.5) * px;
        for (x, out) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let p = Point::new(x0 + (x as f64 + 0.5) * px, py);
            *out = pixel_prepared(&request.sources, p, request.weld, px, boundary.as_ref()).rgba8();
        }
        Ok(())
    })?;
    Ok(Baked {
        bounds,
        width: w as u32,
        height: h as u32,
        rgba: Arc::from(rgba),
        contours,
    })
}

/// Fill `out` one row of `len` elements at a time, rows spread over every
/// core. `per` elements make one sample (4 for rgba), which is what the
/// threading threshold counts.
/// Workers pull a few rows at a time so the busy middle of a weld balances.
// ponytail: spawns threads per bake; a persistent pool if many tiny bakes show
// up in a profile (small ones stay serial).
fn rows<T: Send>(
    out: &mut [T],
    len: usize,
    per: usize,
    f: impl Fn(usize, &mut [T]) -> Result<(), Error> + Sync,
) -> Result<(), Error> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::sync::OnceLock;
        use std::sync::atomic::{AtomicBool, Ordering};
        const BAND: usize = 4;
        static THREADS: OnceLock<usize> = OnceLock::new();
        let threads =
            *THREADS.get_or_init(|| std::thread::available_parallelism().map_or(1, usize::from));
        if threads > 1 && out.len() / per >= PARALLEL_MIN {
            let out_len = out.len();
            let bands = std::sync::Mutex::new(out.chunks_mut(len * BAND).enumerate());
            // The first error stops every worker at its next band.
            let failed = AtomicBool::new(false);
            let work = || {
                let r = (|| loop {
                    if failed.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    let next = bands
                        .lock()
                        .map_err(|_| Error::Invalid("bake worker"))?
                        .next();
                    let Some((band, chunk)) = next else {
                        return Ok(());
                    };
                    for (i, row) in chunk.chunks_mut(len).enumerate() {
                        f(band * BAND + i, row)?;
                    }
                })();
                if r.is_err() {
                    failed.store(true, Ordering::Relaxed);
                }
                r
            };
            return std::thread::scope(|s| {
                // No more workers than bands: the rest would only spawn.
                let n = threads.min(out_len.div_ceil(len * BAND));
                let workers: Vec<_> = (1..n).map(|_| s.spawn(work)).collect();
                let mine = work();
                workers.into_iter().fold(mine, |r, h| {
                    let theirs = h.join().unwrap_or_else(|e| std::panic::resume_unwind(e));
                    r.and(theirs)
                })
            });
        }
    }
    out.chunks_mut(len)
        .enumerate()
        .try_for_each(|(y, row)| f(y, row))
}
/// Samples below which a pass stays on one thread. Measured on 16 cores
/// (one bordered rounded rect, whole bake): serial wins below ~3600 px
/// (270 us vs 520 us at 44x44), threads win from ~4600 px (0.63 ms vs
/// 0.94 ms at 68x68).
#[cfg(not(target_arch = "wasm32"))]
const PARALLEL_MIN: usize = 4096;

type Edge = (usize, usize);
fn edge(a: usize, b: usize) -> Edge {
    if a < b { (a, b) } else { (b, a) }
}

/// Split every cell along the same diagonal. Unlike a naive marching-squares
/// lookup this has no ambiguous four-edge saddle; the shared diagonal is a real
/// lattice edge and its crossings join just like horizontal/vertical crossings.
fn contours(values: &[f64], w: usize, h: usize, b: Rect) -> Result<Vec<Vec<Point>>, Error> {
    let sx = b.width() / w as f64;
    let sy = b.height() / h as f64;
    let position = |i: usize| {
        Point::new(
            b.x0 + (i % (w + 1)) as f64 * sx,
            b.y0 + (i / (w + 1)) as f64 * sy,
        )
    };
    let mut next: BTreeMap<Edge, (Edge, Point)> = BTreeMap::new();
    let mut incoming = BTreeSet::new();
    for y in 0..h {
        for x in 0..w {
            let a = y * (w + 1) + x;
            let d = a + w + 1;
            for ids in [[a, a + 1, d + 1], [a, d + 1, d]] {
                let mut cuts = [((0usize, 0usize), Point::default()); 2];
                let mut cut_count = 0;
                for j in 0..3 {
                    let (u, v) = (ids[j], ids[(j + 1) % 3]);
                    if (values[u] < 0.0) == (values[v] < 0.0) {
                        continue;
                    }
                    let t = values[u] / (values[u] - values[v]);
                    if cut_count == cuts.len() {
                        return Err(Error::OpenContour);
                    }
                    cuts[cut_count] = (edge(u, v), position(u) + (position(v) - position(u)) * t);
                    cut_count += 1;
                }
                if cut_count == 0 {
                    continue;
                }
                if cut_count != 2 {
                    return Err(Error::OpenContour);
                }
                let inside = *ids
                    .iter()
                    .find(|&&i| values[i] < 0.0)
                    .ok_or(Error::OpenContour)?;
                let (mut a, mut z) = (cuts[0], cuts[1]);
                // Keep the negative side on the left. Hole loops automatically wind
                // oppositely to exteriors, preserving nonzero-fill semantics.
                if (z.1 - a.1).cross(position(inside) - a.1) < 0.0 {
                    std::mem::swap(&mut a, &mut z);
                }
                if next.insert(a.0, (z.0, a.1)).is_some() || !incoming.insert(z.0) {
                    return Err(Error::OpenContour);
                }
            }
        }
    }
    let mut out = Vec::new();
    while let Some((&start, _)) = next.first_key_value() {
        let (mut at, mut ring) = (start, Vec::new());
        loop {
            let (to, p) = next.remove(&at).ok_or(Error::OpenContour)?;
            ring.push(p);
            at = to;
            if at == start {
                break;
            }
        }
        if ring.len() >= 3 {
            out.push(ring);
        }
    }
    Ok(out)
}
