//! A spatial border material on a fixed outline. Geometry is independent of paint.
use crate::{Fill, Gradient, SceneError};
#[cfg(test)]
use mui_geometry::RoundedRect;
use mui_geometry::{Path, PathCommand, Point, Rect};
use mui_layout::{Frame, Id};
use rustc_hash::FxHashMap as HashMap;
use std::sync::Arc;

/// One border, interpolated horizontally across a component's frame.
/// Both width and paint use the same clamped linear ramp. This never changes
/// the authored outline or weld geometry. Shape layout reserves its inward share.
#[derive(Clone, Debug, PartialEq)]
pub struct BorderRamp {
    pub align: crate::BorderAlign,
    pub from: (Fill, f64),
    pub to: (Fill, f64),
    pub start: f64,
    pub end: f64,
    pub anchor: Option<Id>,
    /// Named vertical edge tabs that share this material, beneath their controls.
    pub tabs: Vec<Id>,
    /// Horizontal shared boundaries, positioned by named descendant frames.
    pub dividers: Vec<Id>,
}
impl BorderRamp {
    pub fn horizontal(from: (impl Into<Fill>, f64), to: (impl Into<Fill>, f64)) -> Self {
        Self {
            align: crate::BorderAlign::Inside,
            from: (from.0.into(), from.1),
            to: (to.0.into(), to.1),
            start: 0.0,
            end: 1.0,
            anchor: None,
            tabs: Vec::new(),
            dividers: Vec::new(),
        }
    }
    pub fn align(mut self, align: crate::BorderAlign) -> Self {
        self.align = align;
        self
    }
    /// Fractions of the anchor width. Outside this interval the endpoints hold.
    pub fn transition(mut self, start: f64, end: f64) -> Self {
        self.start = start;
        self.end = end;
        self
    }
    /// Resolve against a named descendant (or this node). Missing names are errors.
    pub fn over(mut self, id: impl Into<Id>) -> Self {
        self.anchor = Some(id.into());
        self
    }
    pub fn tabs(mut self, ids: impl IntoIterator<Item = impl Into<Id>>) -> Self {
        self.tabs = ids.into_iter().map(Into::into).collect();
        self
    }
    pub fn dividers(mut self, ids: impl IntoIterator<Item = impl Into<Id>>) -> Self {
        self.dividers = ids.into_iter().map(Into::into).collect();
        self
    }
    pub(crate) fn validate(&self) -> Result<(), SceneError> {
        if ![self.from.1, self.to.1, self.start, self.end]
            .iter()
            .all(|x| x.is_finite())
            || self.from.1 < 0.0
            || self.to.1 < 0.0
            || self.start < 0.0
            || self.end > 1.0
            || self.start >= self.end
        {
            return Err(mui_geometry::Error::InvalidOptions("border ramp widths/interval").into());
        }
        Ok(())
    }
    fn profile(&self, frame: Frame) -> mui_geometry::WidthProfile {
        mui_geometry::WidthProfile::horizontal(
            self.from.1,
            self.to.1,
            frame.x + frame.size.width * self.start,
            frame.x + frame.size.width * self.end,
        )
    }
    pub(crate) fn width(&self, x: f64, frame: Frame) -> Result<f64, SceneError> {
        Ok(self.profile(frame).at(x)?)
    }
    pub(crate) fn fill(&self, frame: Frame, bounds: Rect) -> Fill {
        let stop =
            |t| ((frame.x + frame.size.width * t - bounds.x0) / (bounds.x1 - bounds.x0)) as f32;
        Gradient::linear(
            90.0,
            [
                (stop(self.start), self.from.0.clone()),
                (stop(self.end), self.to.0.clone()),
            ],
        )
        .into()
    }
}

pub(crate) fn decorate(
    band: &mut Path,
    ramp: &BorderRamp,
    anchor: Frame,
    shoulder: f64,
    named_frame: impl Fn(&Id) -> Result<Frame, SceneError>,
) -> Result<(), SceneError> {
    for id in &ramp.tabs {
        let tab = named_frame(id)?;
        let left = tab.x - ramp.width(tab.x, anchor)?;
        let right = tab.x + tab.size.width;
        let right = right + ramp.width(right, anchor)?;
        band.commands.extend([
            PathCommand::MoveTo(Point::new(left, tab.y - shoulder)),
            PathCommand::LineTo(Point::new(right, tab.y - shoulder)),
            PathCommand::LineTo(Point::new(right, tab.y + tab.size.height + shoulder)),
            PathCommand::LineTo(Point::new(left, tab.y + tab.size.height + shoulder)),
            PathCommand::Close,
        ]);
    }
    // Shared internal boundaries use the very same paint and width ramp.
    for id in &ramp.dividers {
        let divider = named_frame(id)?;
        let left = divider.x;
        let right = divider.x + divider.size.width;
        let y = divider.y;
        let mut cuts = [left, right, 0., 0.];
        let mut count = 2;
        for t in [ramp.start, ramp.end] {
            let x = anchor.x + anchor.size.width * t;
            if x > left && x < right {
                cuts[count] = x;
                count += 1;
            }
        }
        cuts[..count].sort_by(f64::total_cmp);
        for pair in cuts[..count].windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let (wa, wb) = (ramp.width(a, anchor)? * 0.5, ramp.width(b, anchor)? * 0.5);
            band.commands.extend([
                PathCommand::MoveTo(Point::new(a, y - wa)),
                PathCommand::LineTo(Point::new(b, y - wb)),
                PathCommand::LineTo(Point::new(b, y + wb)),
                PathCommand::LineTo(Point::new(a, y + wa)),
                PathCommand::Close,
            ]);
        }
    }
    Ok(())
}

#[derive(Debug)]
struct Entry {
    outline: Path,
    frame: Frame,
    widths: [f64; 4],
    tolerance: f64,
    band: Path,
    /// The resolve that last used it.
    seen: u64,
}
/// Geometry only: theme/color changes reuse the vector band. Bounded independently
/// of card area; a tall card never allocates a full-surface raster texture.
/// A band the last resolve did not use is swept at its end.
#[derive(Debug)]
struct GeometryEntry {
    key: Vec<u64>,
    // Producers may rebuild an identical contour. Identity is a fast path;
    // the exact immutable geometry is what determines the cached band.
    source: Arc<Path>,
    path: Arc<Path>,
    bounds: Option<Rect>,
    seen: u64,
}

// Retain the base prefix of a completed band; its decorations may change
// independently. Sharing the completed path avoids a second copy on misses.
#[derive(Debug)]
struct BaseEntry {
    key: [u64; 8],
    source: Arc<Path>,
    path: Arc<Path>,
    prefix: usize,
    seen: u64,
}
const BASE_VARIANTS: usize = 2;

#[derive(Clone, Copy)]
pub(crate) struct GeometryInput<'a> {
    pub outline: &'a Arc<Path>,
    pub joins: Option<&'a Path>,
    pub origin: Point,
    pub ramp: &'a BorderRamp,
    pub anchor: Frame,
    pub shoulder: f64,
    pub tolerance: f64,
}

#[derive(Debug, Default)]
pub(crate) struct BorderCache {
    geometry: HashMap<Id, GeometryEntry>,
    bases: HashMap<Id, Vec<BaseEntry>>,
    #[cfg(test)]
    base_builds: usize,
    scratch: Vec<u64>,
    entries: HashMap<String, Entry>,
    generation: u64,
}
impl BorderCache {
    /// Retain the complete decorated band in contour-local space. Paint-only
    /// changes and scrolling reuse its path and flattened bounds without copies.
    pub(crate) fn geometry(
        &mut self,
        id: &Id,
        input: GeometryInput<'_>,
        named_frame: impl Fn(&Id) -> Result<Frame, SceneError>,
    ) -> Result<(Arc<Path>, Option<Rect>), SceneError> {
        let GeometryInput {
            outline,
            joins,
            origin,
            ramp,
            anchor,
            shoulder,
            tolerance,
        } = input;
        let local = |f: Frame| Frame {
            x: f.x - origin.x,
            y: f.y - origin.y,
            ..f
        };
        let anchor = local(anchor);
        let mut key = std::mem::take(&mut self.scratch);
        key.clear();
        key.extend(
            [
                anchor.x,
                anchor.size.width,
                shoulder,
                tolerance,
                ramp.from.1,
                ramp.to.1,
                ramp.start,
                ramp.end,
            ]
            .map(f64::to_bits),
        );
        key.push(ramp.align as u64);
        let built = (|| {
            for list in [&ramp.tabs, &ramp.dividers] {
                key.push(list.len() as u64);
                for id in list {
                    let f = local(named_frame(id)?);
                    key.extend([f.x, f.y, f.size.width, f.size.height].map(f64::to_bits));
                }
            }
            key.push(joins.map_or(0, |p| p.commands.len()) as u64);
            if let Some(joins) = joins {
                let point = |key: &mut Vec<u64>, p: Point| {
                    key.extend([(p.x - origin.x).to_bits(), (p.y - origin.y).to_bits()]);
                };
                for command in &joins.commands {
                    match *command {
                        PathCommand::MoveTo(p) => {
                            key.push(0);
                            point(&mut key, p);
                        }
                        PathCommand::LineTo(p) => {
                            key.push(1);
                            point(&mut key, p);
                        }
                        PathCommand::CubicTo(a, b, c) => {
                            key.push(2);
                            for p in [a, b, c] {
                                point(&mut key, p);
                            }
                        }
                        PathCommand::ArcTo(a) => {
                            key.push(3);
                            point(&mut key, a.center);
                            point(&mut key, a.to);
                            key.extend([a.radius, a.start_angle, a.sweep].map(f64::to_bits));
                        }
                        PathCommand::Close => key.push(4),
                    }
                }
            }
            if let Some(entry) = self.geometry.get_mut(id).filter(|e| {
                e.key == key
                    && (Arc::ptr_eq(&e.source, outline) || e.source.as_ref() == outline.as_ref())
            }) {
                entry.seen = self.generation;
                entry.source.clone_from(outline);
                return Ok((entry.path.clone(), entry.bounds));
            }
            let base_key = [
                anchor.x.to_bits(),
                anchor.size.width.to_bits(),
                tolerance.to_bits(),
                ramp.from.1.to_bits(),
                ramp.to.1.to_bits(),
                ramp.start.to_bits(),
                ramp.end.to_bits(),
                ramp.align as u64,
            ];
            let base_slot = self.bases.get(id).and_then(|versions| {
                versions.iter().position(|entry| {
                    entry.key == base_key
                        && (Arc::ptr_eq(&entry.source, outline)
                            || entry.source.as_ref() == outline.as_ref())
                })
            });
            let mut path = if let Some(slot) = base_slot {
                let entry = &self.bases[id][slot];
                let mut path = entry.path.as_ref().clone();
                path.commands.truncate(entry.prefix);
                path
            } else {
                #[cfg(test)]
                {
                    self.base_builds += 1;
                }
                let mut sweep = ramp.clone();
                if ramp.align == crate::BorderAlign::Center {
                    sweep.from.1 *= 0.5;
                    sweep.to.1 *= 0.5;
                }
                band(outline, &sweep, anchor, tolerance)?
            };
            let prefix = path.commands.len();
            decorate(&mut path, ramp, anchor, shoulder, |id| {
                named_frame(id).map(local)
            })?;
            if let Some(joins) = joins {
                let mut joins = joins.clone();
                joins.translate(-origin.to_vec2());
                path.commands.extend(joins.commands);
            }
            let bounds = mui_geometry::bounds(path.flatten(0.1, 250_000)?.concat());
            let path = Arc::new(path);
            let generation = self.generation;
            let versions = self
                .bases
                .entry(id.clone())
                .or_insert_with(|| Vec::with_capacity(BASE_VARIANTS));
            let base = BaseEntry {
                key: base_key,
                source: outline.clone(),
                path: path.clone(),
                prefix,
                seen: self.generation,
            };
            if let Some(slot) = base_slot {
                versions[slot] = base;
            } else if versions.len() < BASE_VARIANTS {
                versions.push(base);
            } else {
                let oldest = versions
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, entry)| generation.wrapping_sub(entry.seen))
                    .unwrap()
                    .0;
                versions[oldest] = base;
            }

            let old = self.geometry.insert(
                id.clone(),
                GeometryEntry {
                    key: std::mem::take(&mut key),
                    source: outline.clone(),
                    path: path.clone(),
                    bounds,
                    seen: self.generation,
                },
            );
            if let Some(old) = old {
                key = old.key;
            }
            Ok((path, bounds))
        })();
        self.scratch = key;
        built
    }

    pub(crate) fn band(
        &mut self,
        key: &str,
        outline: &Path,
        ramp: &BorderRamp,
        frame: Frame,
        tolerance: f64,
    ) -> Result<Path, SceneError> {
        let widths = [ramp.from.1, ramp.to.1, ramp.start, ramp.end];
        // Local to the anchor's origin, so a moved card still hits.
        let origin = Point::new(frame.x, frame.y);
        let mut outline = outline.clone();
        outline.translate(-origin.to_vec2());
        let frame = Frame {
            x: frame.x - origin.x,
            y: frame.y - origin.y,
            ..frame
        };
        let world = |mut p: Path| {
            p.translate(origin.to_vec2());
            p
        };
        if let Some(e) = self.entries.get_mut(key)
            && e.outline.near(&outline, 1e-9)
            && e.frame == frame
            && e.widths == widths
            && e.tolerance == tolerance
        {
            e.seen = self.generation;
            return Ok(world(e.band.clone()));
        }
        let band = band(&outline, ramp, frame, tolerance)?;
        self.entries.insert(
            key.to_owned(),
            Entry {
                outline,
                frame,
                widths,
                tolerance,
                band: band.clone(),
                seen: self.generation,
            },
        );
        Ok(world(band))
    }
    pub(crate) fn sweep(&mut self, age: u64) {
        let generation = self.generation;
        self.entries
            .retain(|_, e| generation.wrapping_sub(e.seen) <= age);
        self.geometry
            .retain(|_, e| generation.wrapping_sub(e.seen) <= age);
        self.bases.retain(|_, versions| {
            versions.retain(|entry| generation.wrapping_sub(entry.seen) <= age.max(2));
            !versions.is_empty()
        });
        self.generation = generation.wrapping_add(1);
    }
}

pub(crate) fn band(
    outline: &Path,
    ramp: &BorderRamp,
    frame: Frame,
    tolerance: f64,
) -> Result<Path, SceneError> {
    Ok(mui_geometry::boundary_band(
        outline,
        ramp.profile(frame),
        mui_geometry::OffsetOptions {
            flatten_tolerance: tolerance,
            max_points: 100_000,
            ..Default::default()
        },
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;

    #[test]
    fn moving_tabs_reuse_the_exact_base_band_with_bounded_variants() {
        let outline = |radius| {
            Arc::new(
                RoundedRect::new(Rect::new(0., 0., 120., 40.), radius)
                    .unwrap()
                    .path(),
            )
        };
        let mut ramp = BorderRamp::horizontal((Role::Primary, 3.), (Role::Dim, 1.))
            .tabs(["tab"])
            .dividers(["divider"]);
        let mut cache = BorderCache::default();
        let id = Id::of("moving-decorations");
        let get = |cache: &mut BorderCache, outline: &Arc<Path>, ramp: &BorderRamp, y| {
            let anchor = Frame {
                x: 0.,
                y: 0.,
                size: Size::new(120., 40.),
            };
            let named = |_: &Id| {
                Ok(Frame {
                    x: 10.,
                    y,
                    size: Size::new(20., 8.),
                })
            };
            let actual = cache
                .geometry(
                    &id,
                    GeometryInput {
                        outline,
                        ramp,
                        anchor,
                        origin: Point::ZERO,
                        joins: None,
                        shoulder: 4.,
                        tolerance: 0.1,
                    },
                    named,
                )
                .unwrap();
            let mut fresh = band(outline, ramp, anchor, 0.1).unwrap();
            decorate(&mut fresh, ramp, anchor, 4., named).unwrap();
            let bounds = mui_geometry::bounds(fresh.flatten(0.1, 250_000).unwrap().concat());
            assert_eq!(*actual.0, fresh);
            assert_eq!(actual.1, bounds);
        };
        for y in [8.1, 8.2, 8.3] {
            get(&mut cache, &outline(4.), &ramp, y);
        }
        assert_eq!(
            cache.base_builds, 1,
            "tab/divider motion cannot rebuild the base"
        );
        cache.sweep(8);
        get(&mut cache, &outline(6.), &ramp, 8.4);
        assert_eq!(cache.base_builds, 2, "changed contour must rebuild");
        cache.sweep(8);
        ramp.from.1 = 5.;
        get(&mut cache, &outline(4.), &ramp, 8.5);
        assert_eq!(cache.base_builds, 3, "changed width must rebuild");
        assert_eq!(cache.bases[&id].len(), BASE_VARIANTS);
        cache.sweep(8);
        ramp.from.1 = 3.;
        get(&mut cache, &outline(4.), &ramp, 8.6);
        assert_eq!(cache.base_builds, 4, "the oldest variant was evicted");
        for _ in 0..4 {
            cache.sweep(0);
        }
        assert!(cache.bases.is_empty(), "unused base paths expire");
    }

    #[test]
    fn decorated_geometry_reuses_paths_when_scrolled_and_invalidates_real_inputs() {
        let outline = Arc::new(
            RoundedRect::new(Rect::new(0., 0., 120., 40.), 4.)
                .unwrap()
                .path(),
        );
        let ramp = BorderRamp::horizontal((crate::Role::Primary, 3.), (crate::Role::Dim, 1.));
        let mut cache = BorderCache::default();
        let id = Id::of("band");
        let mut get = |origin: Point, width| {
            cache
                .geometry(
                    &id,
                    GeometryInput {
                        outline: &outline,
                        joins: None,
                        origin,
                        ramp: &ramp,
                        anchor: Frame {
                            x: origin.x,
                            y: origin.y,
                            size: crate::Size::new(width, 40.),
                        },
                        shoulder: 4.,
                        tolerance: 0.1,
                    },
                    |_| unreachable!(),
                )
                .unwrap()
        };
        let first = get(Point::ZERO, 120.);
        let moved = get(Point::new(10., -80.), 120.);
        assert!(Arc::ptr_eq(&first.0, &moved.0));
        assert_eq!(first.1, moved.1);
        let resized = get(Point::new(10., -80.), 140.);
        assert!(!Arc::ptr_eq(&first.0, &resized.0));
        cache.sweep(0);
        cache.sweep(0);
        assert!(cache.geometry.is_empty());
    }

    #[test]
    fn rebuilt_equal_contours_reuse_bands_but_changed_geometry_does_not() {
        let ramp = BorderRamp::horizontal((crate::Role::Primary, 3.), (crate::Role::Dim, 1.));
        let mut cache = BorderCache::default();
        let id = Id::of("rebuilt-band");
        let mut get = |radius, origin: Point| {
            // An outline producer may return a fresh allocation every frame.
            let outline = Arc::new(
                RoundedRect::new(Rect::new(0., 0., 120., 40.), radius)
                    .unwrap()
                    .path(),
            );
            cache
                .geometry(
                    &id,
                    GeometryInput {
                        outline: &outline,
                        joins: None,
                        origin,
                        ramp: &ramp,
                        anchor: Frame {
                            x: origin.x,
                            y: origin.y,
                            size: crate::Size::new(120., 40.),
                        },
                        shoulder: 4.,
                        tolerance: 0.1,
                    },
                    |_| unreachable!(),
                )
                .unwrap()
        };
        let first = get(4., Point::ZERO);
        let rebuilt = get(4., Point::new(10., -80.));
        assert!(Arc::ptr_eq(&first.0, &rebuilt.0));
        assert_eq!(first.1, rebuilt.1);
        let changed = get(6., Point::new(10., -80.));
        assert!(!Arc::ptr_eq(&first.0, &changed.0));
        assert_ne!(first.0, changed.0);
    }

    #[test]
    fn internal_divider_shares_the_outer_border_material_and_width() {
        let object = col([block(200., 60.), block(200., 60.).id("lower")])
            .union(Role::Surface)
            .radius(12.)
            .border_ramp(
                BorderRamp::horizontal((Role::Primary, 4.), (Role::Dim, 1.))
                    .dividers([Id::of("lower")]),
            )
            .id("object");
        let scene = crate::resolve(&SceneSpec::new(object).offered(Size::new(200., 120.))).unwrap();
        let bands: Vec<_> = scene
            .paint
            .iter()
            .filter(|p| p.key.as_ref() == "object" && p.layer == crate::Layer::Stroke)
            .collect();
        assert_eq!(
            bands.len(),
            1,
            "divider and outline are one paint operation"
        );
        assert!(matches!(bands[0].paint, crate::Paint::Gradient { .. }));
        let points: Vec<_> = bands[0]
            .path
            .commands
            .iter()
            .filter_map(|command| match command {
                PathCommand::MoveTo(p) | PathCommand::LineTo(p) => Some(*p),
                _ => None,
            })
            .collect();
        for point in [
            Point::new(0., 58.),
            Point::new(200., 59.5),
            Point::new(200., 60.5),
            Point::new(0., 62.),
        ] {
            assert!(points.contains(&point), "missing ramp boundary {point:?}");
        }
    }

    #[test]
    fn edge_tabs_share_one_material_below_controls() {
        let tab = block(32., 36.).radius((0., 0.)).id("tab");
        let plate = row![tab, block(180., 140.).radius((0., 0.))]
            .align(Align::Center)
            .union(Fill::None)
            .radius((0., 0.));
        let object = col([plate])
            .union(Role::Surface)
            .radius((12., 10.))
            .border_ramp(
                BorderRamp::horizontal((Role::Primary, 4.), (Role::Dim, 1.)).tabs([Id::of("tab")]),
            )
            .id("object");
        let scene = crate::resolve(&SceneSpec::new(object).offered(Size::new(212., 140.))).unwrap();
        let bands: Vec<_> = scene
            .paint
            .iter()
            .filter(|p| p.key.as_ref() == "object" && p.layer == crate::Layer::Stroke)
            .collect();
        assert_eq!(bands.len(), 1);
        assert!(matches!(bands[0].paint, crate::Paint::Gradient { .. }));
        assert!(
            !scene
                .paint
                .iter()
                .any(|p| p.key.as_ref() == "tab" && p.layer == crate::Layer::Fill)
        );
        let tab = scene.surface("tab").unwrap().frame;
        let rings = bands[0].path.flatten(0.1, 250_000).unwrap();
        assert!(
            rings.iter().any(|ring| {
                let bounds = mui_geometry::bounds(ring.clone()).unwrap();
                bounds.x0 <= tab.x
                    && bounds.x1 >= tab.x + tab.size.width
                    && bounds.y0 < tab.y
                    && bounds.y1 > tab.y + tab.size.height
            }),
            "tab material must reach both concave shoulders"
        );
    }

    #[test]
    fn widths_follow_the_anchor_and_vector_band_matches_the_ramp() {
        let ramp =
            BorderRamp::horizontal((Role::Primary, 6.0), (Role::Dim, 1.0)).transition(0.35, 0.65);
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            size: Size::new(400.0, 80.0),
        };
        for (x, w) in [
            (0.0, 6.0),
            (140.0, 6.0),
            (200.0, 3.5),
            (260.0, 1.0),
            (400.0, 1.0),
        ] {
            assert!((ramp.width(x, frame).unwrap() - w).abs() < 1e-9);
        }
        let outline = RoundedRect::new(Rect::new(0.0, 0.0, 400.0, 80.0), 8.0)
            .unwrap()
            .path();
        let path = band(&outline, &ramp, frame, 0.05).unwrap();
        // Nonzero coverage: overlapping sweep pieces must not cancel at joins.
        let rings = path.flatten(0.02, 100_000).unwrap();
        let contains = |p: Point| {
            let mut winding = 0;
            for ring in &rings {
                for (a, b) in ring
                    .iter()
                    .zip(ring.iter().cycle().skip(1))
                    .take(ring.len())
                {
                    let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
                    if a.y <= p.y && b.y > p.y && cross > 0.0 {
                        winding += 1;
                    }
                    if a.y > p.y && b.y <= p.y && cross < 0.0 {
                        winding -= 1;
                    }
                }
            }
            winding != 0
        };
        for (x, w) in [(30.0, 6.0), (200.0, 3.5), (370.0, 1.0)] {
            assert!(contains(Point::new(x, w - 0.15)), "missing border at {x}");
            assert!(!contains(Point::new(x, w + 0.15)), "border too wide at {x}");
        }
        let resized = Frame {
            x: 100.0,
            size: Size::new(800.0, 80.0),
            ..frame
        };
        assert!((ramp.width(500.0, resized).unwrap() - 3.5).abs() < 1e-9);
    }

    #[test]
    fn cache_reuses_color_changes_but_invalidates_width_and_contour() {
        let frame = Frame {
            x: 0.0,
            y: 0.0,
            size: Size::new(400.0, 80.0),
        };
        let outline = RoundedRect::new(Rect::new(0.0, 0.0, 400.0, 80.0), 8.0)
            .unwrap()
            .path();
        let mut cache = BorderCache::default();
        let mut ramp = BorderRamp::horizontal((Role::Primary, 6.0), (Role::Dim, 1.0));
        let before = cache.band("card", &outline, &ramp, frame, 0.1).unwrap();
        ramp.from.0 = Fill::Role(Role::Secondary);
        assert_eq!(
            before,
            cache.band("card", &outline, &ramp, frame, 0.1).unwrap()
        );
        ramp.from.1 = 10.0;
        assert_ne!(
            before,
            cache.band("card", &outline, &ramp, frame, 0.1).unwrap()
        );
        let moved = outline
            .rigid_transform(mui_geometry::Vec2::new(10.0, 0.0), 0.0)
            .unwrap();
        assert_ne!(
            before,
            cache.band("card", &moved, &ramp, frame, 0.1).unwrap()
        );
    }
}
