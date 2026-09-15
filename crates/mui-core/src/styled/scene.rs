//! The tree becomes paint.
//!
//! One pre-order walk over the styled tree, in step with the frames the
//! solver produced in the same order. Every node gets an outline; every layer
//! of its [`Style`](crate::styled::Style) becomes one [`Painted`] entry. Children
//! paint after their parent, so a list index is a z-order.
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use mui_geometry::{
    fillet, inset_path, union, Bounds, CornerStyle, GeometryOptions, OffsetOptions, Path,
    PlacedShape, Point, Polygon, RoundedRect,
};
use mui_layout::generic::resolve_with;
use mui_layout::{Frame, Layout, Limits, Size};
use mui_text::TextRun;

use super::{Color, Content, El, Fill, Paint, Radius, Theme};

#[derive(Debug, Clone)]
pub struct SceneSpec {
    pub theme: Theme,
    pub root: El,
    pub offered: Option<Size>,
    pub limits: Limits,
    pub geometry: GeometryOptions,
    pub offsets: OffsetOptions,
    /// Font bytes for `text(..)` leaves. Without one, text is boxed at an
    /// estimate and draws nothing, so a layout test needs no font file.
    pub font: Option<Arc<Vec<u8>>>,
    /// Curve tolerance for glyph outlines.
    pub tolerance: f64,
}
impl SceneSpec {
    pub fn new(root: El) -> Self {
        Self {
            theme: Theme::default(),
            root,
            offered: None,
            limits: Limits::default(),
            geometry: GeometryOptions::default(),
            offsets: OffsetOptions::default(),
            font: None,
            tolerance: 0.05,
        }
    }
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }
    pub fn offered(mut self, size: Size) -> Self {
        self.offered = Some(size);
        self
    }
    pub fn font(mut self, font: impl Into<Arc<Vec<u8>>>) -> Self {
        self.font = Some(font.into());
        self
    }
}

/// Which layer of a node's style a [`Painted`] entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Shadow,
    Fill,
    Shell(usize),
    Stroke,
    Text,
}

/// One thing to draw. `key` is the node's id, or its tree path (`/0/2`)
/// when it has none: hit-testing and state keep working without names.
#[derive(Clone, Debug, PartialEq)]
pub struct Painted {
    pub key: String,
    pub layer: Layer,
    pub path: Path,
    pub paint: Paint,
    /// Analytic form when the path is a plain rounded rectangle: a renderer
    /// with a fast path (blurred rects, say) can take it.
    pub rect: Option<RoundedRect>,
    /// Stroke width; `0` fills.
    pub width: f64,
    /// Gaussian blur radius, shadows only.
    pub blur: f64,
}

/// A node's outline, for hit-testing and for anything that derives from it.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedSurface {
    pub key: String,
    pub frame: Frame,
    pub path: Path,
    pub bounds: Option<Bounds>,
    /// Exact rounded rectangle when the outline is one (not welded).
    pub rect: Option<RoundedRect>,
    /// A shell collapsed or a merge changed ring counts.
    pub topology_changed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedScene {
    pub layout: Layout,
    pub paint: Vec<Painted>,
    /// Surface keys in tree/z-order.
    pub keys: Vec<String>,
    surfaces: BTreeMap<String, ResolvedSurface>,
}
impl ResolvedScene {
    pub fn surface(&self, key: &str) -> Option<&ResolvedSurface> {
        self.surfaces.get(key)
    }
    pub fn surfaces(&self) -> impl Iterator<Item = &ResolvedSurface> {
        self.surfaces.values()
    }
}

#[derive(Debug)]
pub enum SceneError {
    InvalidTheme,
    InvalidRadius,
    Layout(mui_layout::Error),
    Geometry(mui_geometry::Error),
    Text(mui_text::Error),
    RevisionExhausted,
}
impl std::fmt::Display for SceneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "scene: {self:?}")
    }
}
impl std::error::Error for SceneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Layout(e) => Some(e),
            Self::Geometry(e) => Some(e),
            Self::Text(e) => Some(e),
            _ => None,
        }
    }
}
impl From<mui_layout::Error> for SceneError {
    fn from(v: mui_layout::Error) -> Self {
        Self::Layout(v)
    }
}
impl From<mui_geometry::Error> for SceneError {
    fn from(v: mui_geometry::Error) -> Self {
        Self::Geometry(v)
    }
}
impl From<mui_text::Error> for SceneError {
    fn from(v: mui_text::Error) -> Self {
        Self::Text(v)
    }
}

fn bounds(f: Frame) -> Bounds {
    Bounds {
        min: Point::new(f.x, f.y),
        max: Point::new(f.right(), f.bottom()),
    }
}
fn rect_poly(b: Bounds) -> Result<PlacedShape, SceneError> {
    Ok(Polygon::rectangle(b.min.x, b.min.y, b.width(), b.height())?.into())
}
fn count(n: &El) -> usize {
    1 + n.children().iter().map(count).sum::<usize>()
}

/// Text runs keyed by (text, size bits): measured once for layout, reused
/// for paint.
struct Runs<'a> {
    font: Option<&'a [u8]>,
    tolerance: f64,
    cache: HashMap<(String, u64), TextRun>,
}
impl Runs<'_> {
    fn run(&mut self, text: &str, size: f64) -> Result<Option<&TextRun>, mui_text::Error> {
        let Some(font) = self.font else {
            return Ok(None);
        };
        let key = (text.to_owned(), size.to_bits());
        if !self.cache.contains_key(&key) {
            let run = mui_text::text_run(font, text, size, &[], self.tolerance)?;
            self.cache.insert(key.clone(), run);
        }
        Ok(self.cache.get(&key))
    }
    fn measure(&mut self, text: &str, size: f64) -> Size {
        match self.run(text, size) {
            // ponytail: no font → a monospace guess, so layout tests stay
            // font-free. Wrong widths are visible the moment a font is set.
            Ok(None) | Err(_) => Size::new(text.chars().count() as f64 * size * 0.6, size * 1.25),
            Ok(Some(r)) => Size::new(r.advance, r.line_height),
        }
    }
}

struct Walk<'a> {
    spec: &'a SceneSpec,
    frames: &'a [Frame],
    runs: Runs<'a>,
    i: usize,
    key: String,
    paint: Vec<Painted>,
    keys: Vec<String>,
    surfaces: BTreeMap<String, ResolvedSurface>,
}
impl Walk<'_> {
    fn outline(
        &self,
        n: &El,
        frame: Frame,
    ) -> Result<(Path, Option<RoundedRect>, bool), SceneError> {
        let th = &self.spec.theme;
        let s = &n.payload().style;
        let (convex, concave) = match s.radius {
            Radius::Theme => (th.corners.convex, th.corners.concave),
            Radius::Px(r) => (r, th.corners.concave),
            Radius::Scale(k) => {
                let p = th.corners.scaled(k).ok_or(SceneError::InvalidRadius)?;
                (p.convex, p.concave)
            }
            Radius::Pill => (
                frame.size.width.min(frame.size.height) / 2.0,
                th.corners.concave,
            ),
        };
        if !(convex.is_finite() && convex >= 0.0) {
            return Err(SceneError::InvalidRadius);
        }
        if !s.weld || n.children().is_empty() {
            let rr = RoundedRect::new(bounds(frame), convex)?;
            return Ok((rr.path(), Some(rr), false));
        }
        // Children's frames sit right after this node in pre-order, each
        // subtree `count` long.
        let (mut at, mut shapes) = (self.i, Vec::new());
        for c in n.children() {
            shapes.push(rect_poly(bounds(self.frames[at]))?);
            at += count(c);
        }
        let merged = union(&shapes, self.spec.geometry)?;
        let rounded = fillet(
            &merged,
            CornerStyle {
                convex_radius: convex,
                concave_radius: concave,
                ..CornerStyle::default()
            },
        )?;
        Ok((
            rounded.path,
            None,
            merged.components() != n.children().len(),
        ))
    }

    fn push(
        &mut self,
        layer: Layer,
        path: Path,
        rect: Option<RoundedRect>,
        fill: &Fill,
        under: Color,
    ) -> Option<&mut Painted> {
        let paint = fill.paint(&self.spec.theme.palette, under)?;
        self.paint.push(Painted {
            key: self.key.clone(),
            layer,
            path,
            paint,
            rect,
            width: 0.0,
            blur: 0.0,
        });
        self.paint.last_mut()
    }

    fn node(&mut self, n: &El, path: &str, under: Color) -> Result<(), SceneError> {
        let frame = self.frames[self.i];
        self.i += 1;
        let key = n.key().map_or_else(|| path.to_owned(), str::to_owned);
        let th = self.spec.theme;
        let e = n.payload();
        let s = &e.style;
        if frame.size.width <= 0.0 || frame.size.height <= 0.0 {
            // Collapsed flex shares and their descendants are invisible.
            self.i += n.children().iter().map(count).sum::<usize>();
            return Ok(());
        }
        let (outline, rect, mut changed) = self.outline(n, frame)?;

        self.key = key.clone();
        if let Some(sh) = &s.shadow {
            let d = Point::new(sh.dx, sh.dy);
            let moved = outline.rigid_transform(d, 0.0)?;
            let moved_rect = rect
                .map(|r| {
                    let b = r.bounds();
                    RoundedRect::new(
                        Bounds::new(b.min.x + d.x, b.min.y + d.y, b.max.x + d.x, b.max.y + d.y),
                        r.radius(),
                    )
                })
                .transpose()?;
            if let Some(p) = self.push(Layer::Shadow, moved, moved_rect, &sh.fill, under) {
                p.blur = sh.blur;
            }
        }
        let solid = |p: Option<&mut Painted>, or: Color| p.map_or(or, |p| p.paint.solid());
        let mut bg = solid(
            self.push(Layer::Fill, outline.clone(), rect, &s.fill, under),
            under,
        );

        let (mut cur, mut cur_rect) = (outline.clone(), rect);
        for (i, (d, f)) in s.shells.iter().enumerate() {
            let d = d.resolve(&th.spacing).ok_or(SceneError::InvalidRadius)?;
            if !(d.is_finite() && d >= 0.0) {
                return Err(SceneError::InvalidRadius);
            }
            match cur_rect {
                Some(rr) => {
                    let i2 = rr.inset(d)?;
                    changed |= i2.corner_collapsed;
                    let Some(child) = i2.shape else { break };
                    cur = child.path();
                    cur_rect = Some(child);
                }
                None => {
                    let i2 = inset_path(&cur, d, self.spec.offsets)?;
                    changed |= i2.counts_changed;
                    cur = i2.path;
                }
            }
            bg = solid(self.push(Layer::Shell(i), cur.clone(), cur_rect, f, bg), bg);
        }

        if let Some(st) = &s.stroke {
            let w = st.width.unwrap_or(th.stroke_width);
            if !(w.is_finite() && w >= 0.0) {
                return Err(SceneError::InvalidRadius);
            }
            if let Some(p) = self.push(Layer::Stroke, outline.clone(), rect, &st.fill, bg) {
                p.width = w;
            }
        }

        if let Content::Text(t) = &e.content {
            let size = e.text_size.unwrap_or(th.text);
            if let Some(run) = self.runs.run(t, size)? {
                // Baseline placed so ascent+descent sits centred in the frame.
                let dy =
                    frame.y + (frame.size.height - run.ascent - run.descent) / 2.0 + run.ascent;
                let glyphs = run.path.rigid_transform(Point::new(frame.x, dy), 0.0)?;
                let ink = if s.fill.is_none() {
                    Fill::Role(super::Role::Ink)
                } else {
                    s.fill.clone()
                };
                // Text's own fill is its ink, not a box behind it.
                self.paint
                    .retain(|p| !(p.key == key && p.layer == Layer::Fill));
                bg = under;
                self.push(Layer::Text, glyphs, None, &ink, under);
            }
        }

        self.keys.push(key.clone());
        self.surfaces.insert(
            key.clone(),
            ResolvedSurface {
                key: key.clone(),
                frame,
                bounds: Bounds::from_points(outline.flatten(0.5, 100_000)?.concat()),
                path: outline,
                rect,
                topology_changed: changed,
            },
        );
        for (j, c) in n.children().iter().enumerate() {
            self.node(c, &format!("{path}/{j}"), bg)?;
        }
        Ok(())
    }
}

pub fn resolve_scene(spec: &SceneSpec) -> Result<ResolvedScene, SceneError> {
    if !spec.theme.valid() {
        return Err(SceneError::InvalidTheme);
    }
    let mut runs = Runs {
        font: spec.font.as_deref().map(Vec::as_slice),
        tolerance: spec.tolerance,
        cache: HashMap::new(),
    };
    let th = spec.theme;
    let layout = resolve_with(
        &spec.root,
        spec.offered,
        spec.limits,
        th.spacing,
        |e| match &e.content {
            Content::Text(t) => runs.measure(t, e.text_size.unwrap_or(th.text)),
            Content::None => Size::ZERO,
        },
    )?;
    let mut w = Walk {
        spec,
        frames: layout.all(),
        runs,
        i: 0,
        key: String::new(),
        paint: Vec::new(),
        keys: Vec::new(),
        surfaces: BTreeMap::new(),
    };
    w.node(&spec.root, "", th.palette.background())?;
    let (paint, keys, surfaces) = (w.paint, w.keys, w.surfaces);
    Ok(ResolvedScene {
        layout,
        paint,
        keys,
        surfaces,
    })
}

#[derive(Debug, Default)]
pub struct SceneState {
    revision: u64,
    current: Option<ResolvedScene>,
}
impl SceneState {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn current(&self) -> Option<&ResolvedScene> {
        self.current.as_ref()
    }
    pub fn commit(&mut self, spec: &SceneSpec) -> Result<(), SceneError> {
        let next = self
            .revision
            .checked_add(1)
            .ok_or(SceneError::RevisionExhausted)?;
        self.current = Some(resolve_scene(spec)?);
        self.revision = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::styled::prelude::*;
    use crate::{CornerProfile, Spacing};

    #[test]
    fn collapsed_subtrees_skip_paint_without_losing_later_frames() {
        let root = row([
            column([leaf(0., 10.).id("hidden")])
                .width(0.)
                .id("collapsed"),
            leaf(20., 10.).fill(Role::Primary).id("visible"),
        ])
        .id("root");
        let scene = resolve_scene(&SceneSpec::new(root)).unwrap();
        assert_eq!(scene.keys, ["root", "visible"]);
        assert!(scene.surface("hidden").is_none());
        assert_eq!(
            scene.surface("visible").unwrap().frame.size,
            Size::new(20., 10.)
        );
        assert_eq!(scene.paint.len(), 1);
    }

    /// The canonical case: a tab welded to its panel, with a pill shell
    /// inside the tab.
    fn spec() -> SceneSpec {
        let tab = column([leaf(28., 28.), leaf(28., 28.), leaf(28., 28.)])
            .gap(10.)
            .pad(22.)
            .min_width(92.)
            .align(Align::Center)
            .id("tab")
            .shell(12., Role::Raised);
        let root = column([tab, leaf(520., 230.).id("panel")])
            .align(Align::Start)
            .id("root")
            .weld(Role::Surface);
        SceneSpec::new(root).theme(Theme {
            corners: CornerProfile::new(28., 32.),
            ..Theme::default()
        })
    }
    #[test]
    fn shells_are_parallel_and_paint_in_z_order() {
        let s = resolve_scene(&spec()).unwrap();
        let tab = s.surface("tab").unwrap().rect.unwrap();
        let shell = s.paint.iter().find(|p| p.layer == Layer::Shell(0)).unwrap();
        let r = shell.rect.unwrap();
        assert!((tab.radius() - r.radius() - 12.).abs() < 1e-9);
        assert!((r.bounds().min.x - tab.bounds().min.x - 12.).abs() < 1e-9);
        let layers: Vec<_> = s.paint.iter().map(|p| (p.key.as_str(), p.layer)).collect();
        assert_eq!(layers, [("root", Layer::Fill), ("tab", Layer::Shell(0))]);
        assert!(
            s.surface("/0/1").is_some(),
            "unnamed nodes are keyed by path"
        );
    }
    #[test]
    fn a_shell_on_a_weld_follows_the_concave_outline() {
        let mut sp = spec();
        sp.root = sp.root.shell(6., Role::Field);
        let r = resolve_scene(&sp).unwrap();
        let outer = r.surface("root").unwrap();
        assert!(outer.rect.is_none());
        let inner = &r
            .paint
            .iter()
            .find(|p| p.layer == Layer::Shell(0))
            .unwrap()
            .path;
        let oc = outer.path.flatten(0.1, 20_000).unwrap();
        let ic = inner.flatten(0.1, 20_000).unwrap();
        let mut min = f64::INFINITY;
        for p in ic.iter().flatten().step_by(7) {
            min = min.min(mui_geometry::boundary_distance(*p, &oc));
        }
        assert!((min - 6.0).abs() < 0.35, "measured inset={min}");
    }
    #[test]
    fn roles_resolve_against_the_palette_and_ink_reads_on_its_ground() {
        let root = column([text("hi").id("t")]).fill(Role::Primary).id("card");
        let mut sp = SceneSpec::new(root);
        sp.font = Some(Arc::new(epaint_default_fonts::HACK_REGULAR.to_vec()));
        let s = resolve_scene(&sp).unwrap();
        let th = Theme::default();
        let card = &s.paint[0];
        assert_eq!(card.paint, Paint::Solid(th.palette.primary()));
        let ink = s.paint.iter().find(|p| p.layer == Layer::Text).unwrap();
        assert_eq!(ink.paint, Paint::Solid(th.palette.on(th.palette.primary())));
        assert!(s.layout.frame("t").unwrap().size.width > 10.);
    }
    #[test]
    fn tokens_pill_and_gradient() {
        let root = row([leaf(40., 20.)
            .pill()
            .fill(Gradient::vertical(Role::Raised, Role::Surface))
            .id("k")])
        .gap(M)
        .pad(S);
        let s = resolve_scene(&SceneSpec::new(root)).unwrap();
        assert_eq!(s.layout.frame("k").unwrap().x, 8.);
        assert_eq!(s.surface("k").unwrap().rect.unwrap().radius(), 10.);
        assert!(matches!(s.paint[0].paint, Paint::Linear { angle, .. } if angle == 180.));
    }
    #[test]
    fn failed_commit_is_transactional() {
        let mut state = SceneState::default();
        state.commit(&spec()).unwrap();
        let rev = state.revision();
        let bad = SceneSpec::new(leaf(f64::NAN, 1.));
        assert!(state.commit(&bad).is_err());
        assert_eq!(state.revision(), rev);
        assert!(state.current().is_some());
    }
    #[test]
    fn errors_expose_their_source() {
        let e = resolve_scene(&SceneSpec::new(leaf(f64::NAN, 1.))).unwrap_err();
        assert!(std::error::Error::source(&e)
            .unwrap()
            .is::<mui_layout::Error>());
        let mut bad = spec();
        bad.geometry.epsilon = f64::NAN;
        let e = resolve_scene(&bad).unwrap_err();
        assert!(std::error::Error::source(&e)
            .unwrap()
            .is::<mui_geometry::Error>());
        let _ = Spacing::px(1.);
    }
}
