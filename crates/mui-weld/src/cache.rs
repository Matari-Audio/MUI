use crate::analytic::{AnalyticSource, AnalyticWeld};
use crate::{Baked, Brush, Error, Geometry, Request, bake};
use rustc_hash::{FxHashMap, FxHasher};
use std::collections::BTreeMap;
use std::hash::Hasher;
use std::sync::Arc;

/// Floats quantised to 1/1024 (so -0.0 and 0.0 agree). Only a lookup key:
/// every hit is confirmed with full equality, so a collision is just a miss.
fn quantise(h: &mut FxHasher, v: f64) {
    h.write_u64(((v * 1024.0).round() + 0.0).to_bits());
}
pub(crate) fn quantised_hash(values: impl IntoIterator<Item = f64>) -> u64 {
    let mut h = FxHasher::default();
    values.into_iter().for_each(|v| quantise(&mut h, v));
    h.finish()
}

impl Request {
    /// Hashes everything `==` compares except image pixels (compared by pointer).
    pub(crate) fn key(&self) -> u64 {
        let mut h = FxHasher::default();
        let (w, q) = (self.weld, self.quality);
        h.write_u8(w.fill as u8);
        h.write_u8(w.border as u8);
        h.write_usize(q.max_pixels);
        h.write_usize(q.max_work);
        for v in [w.reach, w.blend, w.progress, q.scale] {
            quantise(&mut h, v);
        }
        for s in &self.sources {
            quantise(&mut h, s.width);
            match &s.shape {
                Geometry::RoundedRect { bounds: b, radius } => {
                    for v in [b.x0, b.y0, b.x1, b.y1, *radius] {
                        quantise(&mut h, v);
                    }
                }
                Geometry::Contours(rings) => {
                    for r in rings {
                        h.write_usize(r.len());
                        for p in r {
                            quantise(&mut h, p.x);
                            quantise(&mut h, p.y);
                        }
                    }
                }
            }
            for b in [&s.fill, &s.border] {
                match b {
                    None => h.write_u8(0),
                    Some(Brush::Solid(c)) => c.0.iter().for_each(|&v| quantise(&mut h, v)),
                    Some(
                        Brush::Linear { stops, .. }
                        | Brush::Radial { stops, .. }
                        | Brush::Conic { stops, .. },
                    ) => {
                        for s in stops {
                            quantise(&mut h, s.at);
                            s.color.0.iter().for_each(|&v| quantise(&mut h, v));
                        }
                    }
                    Some(Brush::Image { image, .. }) => {
                        h.write_u32(image.width);
                        h.write_u32(image.height);
                    }
                }
            }
        }
        h.finish()
    }
    /// Conservative bytes a cache entry keeps alive for this key (contours,
    /// gradient stops, image buffers counted in full), excluding the bake.
    pub(crate) fn retained_bytes(&self) -> usize {
        self.sources.iter().fold(
            self.sources
                .len()
                .saturating_mul(std::mem::size_of::<crate::Source>()),
            |bytes, s| {
                let rings = match &s.shape {
                    Geometry::Contours(rings) => rings.iter().fold(
                        rings
                            .len()
                            .saturating_mul(std::mem::size_of::<Vec<crate::Point>>()),
                        |b, r| {
                            b.saturating_add(
                                r.len().saturating_mul(std::mem::size_of::<crate::Point>()),
                            )
                        },
                    ),
                    Geometry::RoundedRect { .. } => 0,
                };
                [&s.fill, &s.border]
                    .into_iter()
                    .flatten()
                    .fold(bytes.saturating_add(rings), |b, brush| {
                        b.saturating_add(brush.retained_bytes())
                    })
            },
        )
    }
}

/// Hashed LRU: key -> bucket (collisions share a bucket), generation order.
#[derive(Debug)]
struct Lru<T> {
    map: FxHashMap<u64, Vec<(u64, T)>>,
    /// Generation -> key, oldest first.
    order: BTreeMap<u64, u64>,
    tick: u64,
}
impl<T> Lru<T> {
    fn new() -> Self {
        Self {
            map: FxHashMap::default(),
            order: BTreeMap::new(),
            tick: 0,
        }
    }
    fn len(&self) -> usize {
        self.order.len()
    }
    fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
    /// Find by key and full equality, and mark it most recently used.
    fn get(&mut self, key: u64, eq: impl Fn(&T) -> bool) -> Option<&mut T> {
        let (generation, value) = self.map.get_mut(&key)?.iter_mut().find(|(_, v)| eq(v))?;
        self.order.remove(generation);
        self.tick += 1;
        *generation = self.tick;
        self.order.insert(self.tick, key);
        Some(value)
    }
    fn insert(&mut self, key: u64, value: T) {
        self.tick += 1;
        self.order.insert(self.tick, key);
        self.map.entry(key).or_default().push((self.tick, value));
    }
    fn pop_oldest(&mut self) -> Option<T> {
        let (generation, key) = self.order.pop_first()?;
        let bucket = self.map.get_mut(&key)?;
        let i = bucket.iter().position(|e| e.0 == generation)?;
        let (_, value) = bucket.swap_remove(i);
        if bucket.is_empty() {
            self.map.remove(&key);
        }
        Some(value)
    }
}

#[derive(Debug)]
struct Entry {
    request: Request,
    baked: Arc<Baked>,
    bytes: usize,
}
/// Host/UI-owned byte-bounded LRU. No global mutex, pointer-only shape keys, or
/// hash-only equality. A moved group can reuse a bake when its sources are passed
/// in group-local coordinates (the scene adapter does this). Bytes are the only
/// eviction rule: a count cap below the live welds would evict and rebuild every
/// one of them every frame.
#[derive(Debug)]
pub struct WeldCache {
    analytic: Lru<Arc<AnalyticWeld>>,
    analytic_hits: u64,
    analytic_builds: u64,
    entries: Lru<Entry>,
    limit: usize,
    bytes: usize,
    hits: u64,
    misses: u64,
}
impl Default for WeldCache {
    fn default() -> Self {
        Self::with_limit(32 * 1024 * 1024)
    }
}
impl WeldCache {
    pub fn with_limit(bytes: usize) -> Self {
        Self {
            analytic: Lru::new(),
            analytic_hits: 0,
            analytic_builds: 0,
            entries: Lru::new(),
            limit: bytes,
            bytes: 0,
            hits: 0,
            misses: 0,
        }
    }
    pub fn bytes(&self) -> usize {
        self.bytes + self.analytic.len() * Self::ANALYTIC_SLOT_BYTES
    }
    pub fn stats(&self) -> (u64, u64) {
        (self.hits, self.misses)
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.analytic.clear();
        self.bytes = 0;
    }
    // Conservative fixed payload accounting: up to 128 edges, uniforms, sources,
    // and Vec/Arc headers. Allocator/driver overhead is not represented.
    const ANALYTIC_SLOT_BYTES: usize = 32 * 1024;
    pub fn analytic_stats(&self) -> (u64, u64) {
        (self.analytic_hits, self.analytic_builds)
    }
    /// Owned copy of [`Self::analytic`] for holders that store the value.
    pub fn get_analytic(
        &mut self,
        sources: &[crate::Source],
        weld: crate::Weld,
        scale: f64,
    ) -> Result<AnalyticWeld, Error> {
        self.analytic(sources, weld, scale)
            .map(Arc::unwrap_or_clone)
    }
    /// The cached surface, shared. Materials and morph are retargeted in place
    /// (copy-on-write if a caller still holds last frame's Arc).
    pub fn analytic(
        &mut self,
        sources: &[crate::Source],
        weld: crate::Weld,
        scale: f64,
    ) -> Result<Arc<AnalyticWeld>, Error> {
        weld.validate()?;
        if sources.is_empty() || sources.len() > crate::analytic::ANALYTIC_SOURCES {
            return Err(Error::Invalid("GPU source count"));
        }
        let shapes = sources
            .iter()
            .map(AnalyticSource::from_source)
            .collect::<Result<Vec<_>, _>>()?;
        let key = crate::analytic::geometry_key(&shapes, weld, scale);
        if let Some(a) = self
            .analytic
            .get(key, |a| a.same_geometry(&shapes, weld, scale))
        {
            Arc::make_mut(a).retarget(shapes, weld)?;
            self.analytic_hits += 1;
            return Ok(a.clone());
        }
        let a = Arc::new(AnalyticWeld::new(shapes, weld, scale)?);
        self.analytic_builds += 1;
        if Self::ANALYTIC_SLOT_BYTES <= self.limit {
            while self.bytes() + Self::ANALYTIC_SLOT_BYTES > self.limit {
                if self.analytic.pop_oldest().is_none() {
                    if let Some(e) = self.entries.pop_oldest() {
                        self.bytes -= e.bytes;
                    } else {
                        break;
                    }
                }
            }
            self.analytic.insert(key, a.clone());
        }
        Ok(a)
    }
    pub fn get(&mut self, request: &Request) -> Result<Arc<Baked>, Error> {
        // A cache never bypasses validation for a mutated request or quality.
        request.validate()?;
        let key = request.key();
        if let Some(e) = self.entries.get(key, |e| e.request == *request) {
            self.hits = self.hits.saturating_add(1);
            return Ok(e.baked.clone());
        }
        let result = Arc::new(bake(request)?);
        self.misses = self.misses.saturating_add(1);
        // Conservative payload budget, not allocator RSS. Shared image buffers
        // are counted in full per entry rather than undercounting live assets.
        let bytes = result
            .bytes()
            .checked_add(std::mem::size_of::<Entry>().saturating_add(request.retained_bytes()))
            .ok_or(Error::Budget)?;
        // Oversized outputs can be used for this frame without evicting the whole
        // useful cache. The caller still owns its frame's Arc after an eviction.
        let available = self
            .limit
            .saturating_sub(self.analytic.len() * Self::ANALYTIC_SLOT_BYTES);
        if bytes <= available {
            while self.bytes > available - bytes {
                let Some(e) = self.entries.pop_oldest() else {
                    break;
                };
                self.bytes -= e.bytes;
            }
            self.bytes += bytes;
            self.entries.insert(
                key,
                Entry {
                    request: request.clone(),
                    baked: result.clone(),
                    bytes,
                },
            );
        }
        Ok(result)
    }
}

#[cfg(test)]
mod analytic_tests {
    use super::*;
    use crate::{Brush, Color, Geometry, Rect, Source, Weld};
    fn sources() -> Vec<Source> {
        vec![Source {
            shape: Geometry::RoundedRect {
                bounds: Rect::new(0., 0., 40., 25.),
                radius: 4.,
            },
            fill: Some(Brush::Solid(Color([1.0, 0.0, 0.0, 1.0]))),
            border: None,
            width: 0.,
        }]
    }
    #[test]
    fn materials_reuse_prepared_boundary() {
        let mut c = WeldCache::default();
        let src = sources();
        let a = c.get_analytic(&src, Weld::crisp(), 1.).unwrap();
        let b = c.get_analytic(&src, Weld::crisp().morph(0.5), 1.).unwrap();
        assert!(Arc::ptr_eq(a.boundary_bytes(), b.boundary_bytes()));
        assert_eq!(c.analytic_stats(), (1, 1));
    }
    #[test]
    fn many_live_welds_all_stay_cached() {
        let mut c = WeldCache::default();
        let at = |i: usize| {
            let mut s = sources();
            s[0].shape = Geometry::RoundedRect {
                bounds: Rect::new(0., 0., 40. + i as f64, 25.),
                radius: 4.,
            };
            s
        };
        for _ in 0..2 {
            for i in 0..40 {
                c.get_analytic(&at(i), Weld::crisp(), 1.).unwrap();
            }
        }
        assert_eq!(c.analytic_stats(), (40, 40), "the second frame rebuilt");
    }
    #[test]
    fn clear_releases_boundary_cache() {
        let mut c = WeldCache::default();
        c.get_analytic(&sources(), Weld::crisp(), 1.).unwrap();
        assert!(c.bytes() > 0);
        c.clear();
        assert_eq!(c.bytes(), 0);
    }
    #[test]
    fn zero_budget_never_retains_boundary() {
        let mut c = WeldCache::with_limit(0);
        c.get_analytic(&sources(), Weld::crisp(), 1.).unwrap();
        assert_eq!(c.bytes(), 0);
    }
    #[test]
    fn invalid_morph_cannot_bypass_a_hit() {
        let mut c = WeldCache::default();
        c.get_analytic(&sources(), Weld::crisp(), 1.).unwrap();
        assert!(
            c.get_analytic(&sources(), Weld::crisp().morph(f64::NAN), 1.)
                .is_err()
        );
    }
}
