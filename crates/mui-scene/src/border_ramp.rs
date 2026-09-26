//! A spatial border material on a fixed outline. Geometry is independent of paint.
use crate::{Fill, Gradient, SceneError};
use mui_geometry::{Bounds, Path, Point};
#[cfg(test)]
use mui_geometry::{PathCommand, RoundedRect};
use mui_layout::{Frame, Id};
use rustc_hash::FxHashMap as HashMap;

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
    pub fn tabs(mut self, ids: impl IntoIterator<Item = Id>) -> Self {
        self.tabs = ids.into_iter().collect();
        self
    }
    pub fn dividers(mut self, ids: impl IntoIterator<Item = Id>) -> Self {
        self.dividers = ids.into_iter().collect();
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
    pub(crate) fn fill(&self, frame: Frame, bounds: Bounds) -> Fill {
        let stop = |t| {
            ((frame.x + frame.size.width * t - bounds.min.x) / (bounds.max.x - bounds.min.x)) as f32
        };
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
        band.commands.extend(
            Path::polyline(
                [
                    Point::new(left, tab.y - shoulder),
                    Point::new(right, tab.y - shoulder),
                    Point::new(right, tab.y + tab.size.height + shoulder),
                    Point::new(left, tab.y + tab.size.height + shoulder),
                ],
                true,
            )
            .commands,
        );
    }
    // Shared internal boundaries use the very same paint and width ramp.
    for id in &ramp.dividers {
        let divider = named_frame(id)?;
        let left = divider.x;
        let right = divider.x + divider.size.width;
        let y = divider.y;
        let mut cuts = vec![left, right];
        for t in [ramp.start, ramp.end] {
            let x = anchor.x + anchor.size.width * t;
            if x > left && x < right {
                cuts.push(x);
            }
        }
        cuts.sort_by(f64::total_cmp);
        for pair in cuts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let (wa, wb) = (ramp.width(a, anchor)? * 0.5, ramp.width(b, anchor)? * 0.5);
            band.commands.extend(
                Path::polyline(
                    [
                        Point::new(a, y - wa),
                        Point::new(b, y - wb),
                        Point::new(b, y + wb),
                        Point::new(a, y + wa),
                    ],
                    true,
                )
                .commands,
            );
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
#[derive(Debug, Default)]
pub(crate) struct BorderCache {
    entries: HashMap<String, Entry>,
    generation: u64,
}
impl BorderCache {
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
        outline.translate(-origin);
        let frame = Frame {
            x: frame.x - origin.x,
            y: frame.y - origin.y,
            ..frame
        };
        let world = |mut p: Path| {
            p.translate(origin);
            p
        };
        if let Some(e) = self.entries.get_mut(key) {
            if e.outline.near(&outline, 1e-9)
                && e.frame == frame
                && e.widths == widths
                && e.tolerance == tolerance
            {
                e.seen = self.generation;
                return Ok(world(e.band.clone()));
            }
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
    fn internal_divider_shares_the_outer_border_material_and_width() {
        let object = column([leaf(200., 60.), leaf(200., 60.).id("lower")])
            .union(Surface)
            .radius(12.)
            .border_ramp(
                BorderRamp::horizontal((Primary, 4.), (Dim, 1.)).dividers([Id::of("lower")]),
            )
            .id("object");
        let scene =
            crate::resolve_scene(&SceneSpec::new(object).offered(Size::new(200., 120.))).unwrap();
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
        let tab = leaf(32., 36.).radius((0., 0.)).id("tab");
        let plate = row![tab, leaf(180., 140.).radius((0., 0.))]
            .align(Align::Center)
            .union(Fill::None)
            .radius((0., 0.));
        let object = column([plate])
            .union(Surface)
            .radius((12., 10.))
            .border_ramp(BorderRamp::horizontal((Primary, 4.), (Dim, 1.)).tabs([Id::of("tab")]))
            .id("object");
        let scene =
            crate::resolve_scene(&SceneSpec::new(object).offered(Size::new(212., 140.))).unwrap();
        let bands: Vec<_> = scene
            .paint
            .iter()
            .filter(|p| p.key.as_ref() == "object" && p.layer == crate::Layer::Stroke)
            .collect();
        assert_eq!(bands.len(), 1);
        assert!(matches!(bands[0].paint, crate::Paint::Gradient { .. }));
        assert!(!scene
            .paint
            .iter()
            .any(|p| p.key.as_ref() == "tab" && p.layer == crate::Layer::Fill));
        let tab = scene.surface("tab").unwrap().frame;
        let rings = bands[0].path.flatten(0.1, 250_000).unwrap();
        assert!(
            rings.iter().any(|ring| {
                let bounds = Bounds::from_points(ring.clone()).unwrap();
                bounds.min.x <= tab.x
                    && bounds.max.x >= tab.x + tab.size.width
                    && bounds.min.y < tab.y
                    && bounds.max.y > tab.y + tab.size.height
            }),
            "tab material must reach both concave shoulders"
        );
    }

    #[test]
    fn widths_follow_the_anchor_and_vector_band_matches_the_ramp() {
        let ramp = BorderRamp::horizontal((Primary, 6.0), (Dim, 1.0)).transition(0.35, 0.65);
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
        let outline = RoundedRect::new(Bounds::new(0.0, 0.0, 400.0, 80.0), 8.0)
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
        let outline = RoundedRect::new(Bounds::new(0.0, 0.0, 400.0, 80.0), 8.0)
            .unwrap()
            .path();
        let mut cache = BorderCache::default();
        let mut ramp = BorderRamp::horizontal((Primary, 6.0), (Dim, 1.0));
        let before = cache.band("card", &outline, &ramp, frame, 0.1).unwrap();
        ramp.from.0 = Fill::Role(Secondary);
        assert_eq!(
            before,
            cache.band("card", &outline, &ramp, frame, 0.1).unwrap()
        );
        ramp.from.1 = 10.0;
        assert_ne!(
            before,
            cache.band("card", &outline, &ramp, frame, 0.1).unwrap()
        );
        let moved = outline.rigid_transform(Point::new(10.0, 0.0), 0.0).unwrap();
        assert_ne!(
            before,
            cache.band("card", &moved, &ramp, frame, 0.1).unwrap()
        );
    }
}
