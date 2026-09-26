//! A bounded GPU-ready specialization, not a second material model.
//!
//! Geometry is cached in local coordinates. Whole-group placement is deliberately
//! absent from the uniform block. The domain includes the maximum join reach, so
//! changing morph progress cannot resize a texture. Unsupported brushes/contours
//! return an error; this path never silently invokes the CPU rasterizer.
use crate::boundary::{BOUNDARY_BYTES, Boundary, Plate};
use crate::{Brush, Channel, Color, Error, Geometry, Point, Rect, Source, Weld};
use std::sync::Arc;

pub const PARAM_BYTES: usize = 336;
pub const ANALYTIC_SOURCES: usize = 3;

/// One shader source. Colour conversion is performed while constructing or
/// changing this value, not while marshalling every animation frame.
#[derive(Clone, Debug, PartialEq)]
pub struct AnalyticSource {
    center: [f64; 2],
    half: [f64; 2],
    radius: f64,
    width: f64,
    // ponytail: always the identity -- nothing in the tree rotates a source.
    // The shader and `Boundary` still take the angle, so a rotating adapter
    // only has to set it here.
    rotation: [f64; 2],
    fill0: [f32; 4],
    fill1: [f32; 4],
    border: [f32; 4],
    gradient: [f64; 4],
}
fn lab(c: Color) -> Result<[f32; 4], Error> {
    c.validate()?;
    let [r, g, b, a] = c.0;
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    Ok([
        (0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s) as f32,
        (1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s) as f32,
        (0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s) as f32,
        a as f32,
    ])
}
impl AnalyticSource {
    pub fn from_source(source: &Source) -> Result<Self, Error> {
        source.validate()?;
        let Geometry::RoundedRect { bounds, radius } = &source.shape else {
            return Err(Error::Invalid(
                "GPU welding requires analytic rounded rectangles",
            ));
        };
        let center = [(bounds.x0 + bounds.x1) * 0.5, (bounds.y0 + bounds.y1) * 0.5];
        let (fill0, fill1, gradient) = match &source.fill {
            None => ([0.; 4], [0.; 4], [0.; 4]),
            Some(Brush::Solid(c)) => (lab(*c)?, lab(*c)?, [0.; 4]),
            Some(Brush::Linear { from, to, stops })
                if stops.len() == 2 && stops[0].at == 0. && stops[1].at == 1. =>
            {
                let (dx, dy) = (to.x - from.x, to.y - from.y);
                let norm = dx * dx + dy * dy;
                (
                    lab(stops[0].color)?,
                    lab(stops[1].color)?,
                    [from.x - center[0], from.y - center[1], dx / norm, dy / norm],
                )
            }
            _ => {
                return Err(Error::Invalid(
                    "GPU fill supports solid or two-stop 0..1 linear gradient",
                ));
            }
        };
        let border = match &source.border {
            None => [0.; 4],
            Some(Brush::Solid(c)) => lab(*c)?,
            _ => {
                return Err(Error::Invalid(
                    "GPU border currently requires a solid paint",
                ));
            }
        };
        let value = Self {
            center,
            half: [bounds.width() * 0.5, bounds.height() * 0.5],
            radius: *radius,
            width: if source.border.is_some() {
                source.width
            } else {
                0.0
            },
            rotation: [1., 0.],
            fill0,
            fill1,
            border,
            gradient,
        };
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> Result<(), Error> {
        let mut scalars = self
            .center
            .iter()
            .chain(self.half.iter())
            .chain(self.rotation.iter())
            .chain(self.gradient.iter())
            .copied()
            .chain([self.radius, self.width]);
        if scalars.clone().any(|v| !v.is_finite())
            || self.center.iter().any(|v| v.abs() > 100_000.)
            || self.half.iter().any(|v| *v <= 0. || *v > 10_000.)
            || !(0.0..=10_000.).contains(&self.radius)
            || !(0.0..=10_000.).contains(&self.width)
            || scalars.any(|v| !(v as f32).is_finite())
        {
            return Err(Error::Invalid("GPU source extent or value"));
        }
        Ok(())
    }
    fn bounds(&self) -> Rect {
        let [c, s] = self.rotation;
        let ex = c.abs() * self.half[0] + s.abs() * self.half[1];
        let ey = s.abs() * self.half[0] + c.abs() * self.half[1];
        Rect::new(
            self.center[0] - ex,
            self.center[1] - ey,
            self.center[0] + ex,
            self.center[1] + ey,
        )
    }
    pub fn distance(&self, p: Point) -> f64 {
        let [c, s] = self.rotation;
        let (x, y) = (p.x - self.center[0], p.y - self.center[1]);
        let r = self.radius.min(self.half[0]).min(self.half[1]);
        let qx = (c * x + s * y).abs() - self.half[0] + r;
        let qy = (-s * x + c * y).abs() - self.half[1] + r;
        let (ox, oy) = (qx.max(0.), qy.max(0.));
        (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.) - r
    }
}

/// Cache key over exactly what `AnalyticWeld::same_geometry` compares.
pub(crate) fn geometry_key(sources: &[AnalyticSource], weld: Weld, scale: f64) -> u64 {
    crate::cache::quantised_hash(
        [scale, weld.reach]
            .into_iter()
            .chain(sources.iter().flat_map(|s| {
                s.center
                    .into_iter()
                    .chain(s.half)
                    .chain(s.rotation)
                    .chain([s.radius])
            })),
    )
}

/// A validated local-space material surface. Fields are private so an update
/// cannot inject NaN, increase reach beyond the cached domain, or overrun the ABI.
#[derive(Clone, Debug, PartialEq)]
pub struct AnalyticWeld {
    sources: Vec<AnalyticSource>,
    weld: Weld,
    domain: Rect,
    pixels: [u32; 2],
    scale: f64,
    boundary: Option<Arc<Boundary>>,
    boundary_bytes: Arc<[u8; BOUNDARY_BYTES]>,
}
impl AnalyticWeld {
    pub fn new(sources: Vec<AnalyticSource>, weld: Weld, scale: f64) -> Result<Self, Error> {
        weld.validate()?;
        if sources.is_empty() || sources.len() > ANALYTIC_SOURCES {
            return Err(Error::Invalid("GPU welding supports one to three sources"));
        }
        if !scale.is_finite() || !(0.125..=8.).contains(&scale) {
            return Err(Error::Invalid("GPU scale must be finite and in 0.125..=8"));
        }
        for s in &sources {
            s.validate()?;
        }
        let bounds = sources
            .iter()
            .skip(1)
            .fold(sources[0].bounds(), |b, s| b.union(s.bounds()));
        // The symmetric smooth minimum is at most `reach` below the nearest
        // source for k=2*reach. Include AA support at BOTH ends, at full morph.
        let pad = weld.reach + 2. / scale;
        let domain = Rect::new(
            ((bounds.x0 - pad) * scale).floor() / scale,
            ((bounds.y0 - pad) * scale).floor() / scale,
            ((bounds.x1 + pad) * scale).ceil() / scale,
            ((bounds.y1 + pad) * scale).ceil() / scale,
        );
        let size = [
            (domain.width() * scale).round(),
            (domain.height() * scale).round(),
        ];
        if size
            .iter()
            .any(|v| !v.is_finite() || *v < 1. || *v > 65535.)
        {
            return Err(Error::Budget);
        }
        let pixels = size.map(|v| v as u32);
        let boundary = if weld.reach == 0.0 {
            Some(Arc::new(Boundary::new(
                sources
                    .iter()
                    .map(|s| Plate {
                        center: Point::new(s.center[0], s.center[1]),
                        half: Point::new(s.half[0], s.half[1]),
                        radius: s.radius,
                        angle: s.rotation[1].atan2(s.rotation[0]),
                    })
                    .collect(),
            )?))
        } else {
            None
        };
        let boundary_bytes = Arc::new(boundary.as_ref().map_or([0; BOUNDARY_BYTES], |b| {
            b.uniform_bytes(Point::new(domain.x0, domain.y0))
        }));
        Ok(Self {
            sources,
            weld,
            domain,
            pixels,
            scale,
            boundary,
            boundary_bytes,
        })
    }
    pub fn from_sources(sources: &[Source], weld: Weld, scale: f64) -> Result<Self, Error> {
        // Check count before allocating/converting source material data.
        if sources.is_empty() || sources.len() > ANALYTIC_SOURCES {
            return Err(Error::Invalid("GPU source count"));
        }
        Self::new(
            sources
                .iter()
                .map(AnalyticSource::from_source)
                .collect::<Result<_, _>>()?,
            weld,
            scale,
        )
    }
    pub fn boundary(&self) -> Option<&Boundary> {
        self.boundary.as_deref()
    }
    pub fn boundary_bytes(&self) -> &Arc<[u8; BOUNDARY_BYTES]> {
        &self.boundary_bytes
    }
    pub fn boundary_segments(&self) -> usize {
        self.boundary.as_ref().map_or(0, |b| b.edges().len())
    }
    /// Geometry-only equality allows materials to reuse prepared boundary data.
    pub fn same_geometry(&self, sources: &[AnalyticSource], weld: Weld, scale: f64) -> bool {
        self.scale == scale
            && self.weld.reach == weld.reach
            && self.sources.len() == sources.len()
            && self.sources.iter().zip(sources).all(|(a, b)| {
                a.center == b.center
                    && a.half == b.half
                    && a.radius == b.radius
                    && a.rotation == b.rotation
            })
    }
    pub fn retarget(&mut self, sources: Vec<AnalyticSource>, weld: Weld) -> Result<(), Error> {
        weld.validate()?;
        if !self.same_geometry(&sources, weld, self.scale) {
            return Err(Error::Invalid("retarget changed geometry"));
        }
        for s in &sources {
            s.validate()?;
        }
        self.sources = sources;
        self.weld = weld;
        Ok(())
    }
    pub fn domain(&self) -> Rect {
        self.domain
    }
    pub fn pixels(&self) -> [u32; 2] {
        self.pixels
    }
    pub fn scale(&self) -> f64 {
        self.scale
    }
    pub fn options(&self) -> Weld {
        self.weld
    }
    /// Changes only parameters; the domain and texture dimensions stay fixed.
    /// Raising reach or changing source geometry instead requires `new`.
    pub fn set_morph(&mut self, progress: f64) -> Result<bool, Error> {
        if !progress.is_finite() || !(0.0..=1.).contains(&progress) {
            return Err(Error::Invalid("GPU morph"));
        }
        let changed = self.weld.progress != progress;
        self.weld.progress = progress;
        Ok(changed)
    }
    pub fn set_material_blend(&mut self, blend: f64) -> Result<bool, Error> {
        if !blend.is_finite() || !(0.0..=1e4).contains(&blend) {
            return Err(Error::Invalid("GPU material blend"));
        }
        let changed = self.weld.blend != blend;
        self.weld.blend = blend;
        Ok(changed)
    }
    /// Change solid paints/width without changing source geometry or allocation.
    pub fn set_solid_material(
        &mut self,
        index: usize,
        fill: Option<Color>,
        border: Option<Color>,
        width: f64,
    ) -> Result<bool, Error> {
        let mut replacement = self
            .sources
            .get(index)
            .cloned()
            .ok_or(Error::Invalid("GPU source index"))?;
        replacement.fill0 = fill.map(lab).transpose()?.unwrap_or([0.; 4]);
        replacement.fill1 = replacement.fill0;
        replacement.border = border.map(lab).transpose()?.unwrap_or([0.; 4]);
        replacement.gradient = [0.; 4];
        replacement.width = if border.is_some() { width } else { 0.0 };
        self.set_source_material(index, replacement)
    }
    /// Replace paint/border parameters while proving geometry stayed unchanged.
    /// Construct the replacement from the same source bounds and rotation. A
    /// geometry change requires a fresh request with a newly checked domain.
    pub fn set_source_material(
        &mut self,
        index: usize,
        replacement: AnalyticSource,
    ) -> Result<bool, Error> {
        replacement.validate()?;
        let old = self
            .sources
            .get(index)
            .ok_or(Error::Invalid("GPU source index"))?;
        if old.center != replacement.center
            || old.half != replacement.half
            || old.radius != replacement.radius
            || old.rotation != replacement.rotation
        {
            return Err(Error::Invalid("material-only update changed geometry"));
        }
        let changed = *old != replacement;
        self.sources[index] = replacement;
        Ok(changed)
    }
    /// Geometric containment, NOT a sampled-alpha ownership rule. Explicit hit
    /// policy remains separate from translucent appearance and child semantics.
    pub fn contains(&self, p: Point) -> bool {
        if !p.x.is_finite() || !p.y.is_finite() {
            return false;
        }
        let mut distances = [0.; ANALYTIC_SOURCES];
        for (d, s) in distances.iter_mut().zip(&self.sources) {
            *d = s.distance(p);
        }
        crate::field::smooth_min(
            &distances[..self.sources.len()],
            2. * self.weld.reach * self.weld.amount(),
        )
        .0 <= 0.
    }
    pub fn uniform_bytes(&self) -> [u8; PARAM_BYTES] {
        let mut out = [0u8; PARAM_BYTES];
        let mut set = |offset: usize, v: [f32; 4]| {
            for (i, x) in v.into_iter().enumerate() {
                out[offset + i * 4..offset + i * 4 + 4].copy_from_slice(&x.to_le_bytes());
            }
        };
        let (w, h) = (self.domain.width() as f32, self.domain.height() as f32);
        set(0, [self.pixels[0] as f32, self.pixels[1] as f32, w, h]);
        set(
            16,
            [
                self.weld.reach as f32,
                self.weld.blend as f32,
                self.weld.progress as f32,
                self.sources.len() as f32,
            ],
        );
        let channel = |c| match c {
            Channel::Blend => 0.,
            Channel::Keep => 1.,
            Channel::Omit => 2.,
        };
        set(
            32,
            [
                channel(self.weld.fill),
                channel(self.weld.border),
                self.boundary_segments() as f32,
                0.,
            ],
        );
        for (i, s) in self.sources.iter().enumerate() {
            let b = 48 + 96 * i;
            set(
                b,
                [
                    (s.center[0] - self.domain.x0) as f32,
                    (s.center[1] - self.domain.y0) as f32,
                    s.half[0] as f32,
                    s.half[1] as f32,
                ],
            );
            set(
                b + 16,
                [
                    s.radius as f32,
                    s.width as f32,
                    s.rotation[0] as f32,
                    s.rotation[1] as f32,
                ],
            );
            set(b + 32, s.fill0);
            set(b + 48, s.fill1);
            set(b + 64, s.border);
            set(b + 80, s.gradient.map(|v| v as f32));
        }
        out
    }
}

/// Changed 16-byte lanes, coalesced without allocations. Offsets satisfy wgpu's
/// write-buffer alignment. Compare against UPLOADED bytes, while separately
/// tracking whether the texture was actually rendered/submitted from them.
pub fn dirty_ranges<'a>(
    old: Option<&'a [u8; PARAM_BYTES]>,
    new: &'a [u8; PARAM_BYTES],
) -> impl Iterator<Item = std::ops::Range<usize>> + 'a {
    let mut lane = 0;
    std::iter::from_fn(move || {
        while lane < PARAM_BYTES / 16
            && old.is_some_and(|o| o[lane * 16..lane * 16 + 16] == new[lane * 16..lane * 16 + 16])
        {
            lane += 1;
        }
        if lane == PARAM_BYTES / 16 {
            return None;
        }
        let start = lane * 16;
        lane += 1;
        while lane < PARAM_BYTES / 16
            && old.is_none_or(|o| o[lane * 16..lane * 16 + 16] != new[lane * 16..lane * 16 + 16])
        {
            lane += 1;
        }
        Some(start..lane * 16)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(x: f64) -> Source {
        Source {
            shape: Geometry::RoundedRect {
                bounds: Rect::new(x, 0., x + 80., 60.),
                radius: 12.,
            },
            fill: Some(Brush::Solid(Color::srgb(0.8, 0.3, 0.2, 0.6))),
            border: Some(Brush::Solid(Color::srgb(0.2, 0.7, 0.9, 0.8))),
            width: 3.,
        }
    }
    fn request() -> AnalyticWeld {
        AnalyticWeld::from_sources(&[source(0.), source(90.)], Weld::all().reach(30.), 1.5).unwrap()
    }
    #[test]
    fn abi_is_336_bytes_and_first_upload_is_one_range() {
        let a = request().uniform_bytes();
        assert_eq!(a.len(), 336);
        assert_eq!(dirty_ranges(None, &a).collect::<Vec<_>>(), vec![0..336]);
    }
    #[test]
    fn morph_touches_one_16_byte_lane_and_no_geometry() {
        let mut r = request();
        let old = r.uniform_bytes();
        let size = r.pixels();
        let domain = r.domain();
        r.set_morph(0.25).unwrap();
        let next = r.uniform_bytes();
        assert_eq!(
            dirty_ranges(Some(&old), &next).collect::<Vec<_>>(),
            vec![16..32]
        );
        assert_eq!(r.pixels(), size);
        assert_eq!(r.domain(), domain);
    }
    #[test]
    fn unchanged_upload_is_empty() {
        let a = request().uniform_bytes();
        assert_eq!(dirty_ranges(Some(&a), &a).count(), 0);
    }
    #[test]
    fn invalid_update_leaves_previous_state() {
        let mut a = request();
        let old = a.clone();
        assert!(a.set_morph(f64::NAN).is_err());
        assert!(a.set_material_blend(-1.).is_err());
        assert_eq!(a, old);
    }
    #[test]
    fn material_update_cannot_move_geometry() {
        let mut a = request();
        let before = a.clone();
        assert!(
            a.set_source_material(0, AnalyticSource::from_source(&source(5.)).unwrap())
                .is_err()
        );
        assert_eq!(a, before);
    }
    #[test]
    fn border_update_preserves_domain() {
        let mut a = request();
        let mut s = source(0.);
        s.width = 9.;
        let old = a.uniform_bytes();
        let domain = a.domain();
        a.set_source_material(0, AnalyticSource::from_source(&s).unwrap())
            .unwrap();
        let next = a.uniform_bytes();
        assert_eq!(
            dirty_ranges(Some(&old), &next).collect::<Vec<_>>(),
            vec![64..80]
        );
        assert_eq!(domain, a.domain());
    }
    #[test]
    fn source_count_is_explicitly_bounded() {
        assert!(AnalyticWeld::from_sources(&[], Weld::all(), 1.).is_err());
        assert!(
            AnalyticWeld::from_sources(
                &[source(0.), source(30.), source(60.), source(90.)],
                Weld::all(),
                1.
            )
            .is_err()
        );
    }
    #[test]
    fn unsupported_gradient_is_not_silently_baked() {
        let mut s = source(0.);
        s.fill = Some(Brush::Radial {
            center: Point::new(20., 20.),
            radius: 10.,
            stops: vec![crate::Stop {
                at: 0.,
                color: Color::srgb(1., 0., 0., 1.),
            }],
        });
        assert!(AnalyticSource::from_source(&s).is_err());
    }
    #[test]
    fn invalid_coordinates_never_hit() {
        assert!(!request().contains(Point::new(f64::NAN, 0.)));
    }
    #[test]
    fn maximum_reach_domain_covers_the_neck() {
        let mut a = request();
        for i in 0..101 {
            a.set_morph(f64::from(i) / 100.).unwrap();
            let b = a.domain();
            for j in 0..100 {
                let y = b.y0 + f64::from(j) / 99. * b.height();
                assert!(!a.contains(Point::new(b.x0, y)));
                assert!(!a.contains(Point::new(b.x1, y)));
            }
        }
    }
}
