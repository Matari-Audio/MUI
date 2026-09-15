//! The tree becomes paint.
//!
//! One pre-order walk over the styled tree, in step with the frames the
//! solver produced in the same order. Every node gets an outline; every layer
//! of its [`Style`](crate::Style) becomes one [`Painted`] entry. Children
//! paint after their parent, so a list index is a z-order.
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use mui_geometry::{
    fillet, inset_path, union, Bounds, CornerStyle, GeometryOptions, OffsetOptions, Path,
    PlacedShape, Point, Polygon, RoundedRect,
};
use mui_layout::{resolve_with, Frame, Layout, Limits, Size};
use mui_text::TextRun;

use crate::{Color, Content, Cursor, El, Fill, Paint, Radius, Theme};

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
    /// A canvas's `k`th draw.
    Draw(usize),
    /// Everything up to the matching `Unclip` is clipped to `path`; the
    /// paint is meaningless.
    Clip,
    Unclip,
}

/// A text layer's glyphs, for a renderer that hints and caches its own.
#[derive(Clone, Debug)]
pub struct Text {
    pub font: Arc<Vec<u8>>,
    pub size: f32,
    /// Baseline origin.
    pub origin: Point,
    /// Glyph id and pen x from the origin.
    pub glyphs: Arc<[(u32, f32)]>,
}
impl PartialEq for Text {
    fn eq(&self, o: &Self) -> bool {
        Arc::ptr_eq(&self.font, &o.font)
            && self.size == o.size
            && self.origin == o.origin
            && self.glyphs == o.glyphs
    }
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
    /// Present on `Layer::Text`: the same ink as `path`, as glyphs.
    pub text: Option<Text>,
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
    pub cursor: Option<Cursor>,
    pub tip: Option<String>,
    pub focusable: bool,
    /// The nearest clipping ancestor's frame, for hit-testing.
    /// ponytail: a rect, not the ancestor's rounded path.
    pub clip: Option<Bounds>,
    /// A scroll node's children extent inside its padding, unscrolled;
    /// the frame size otherwise.
    pub content: Size,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedScene {
    pub layout: Layout,
    pub paint: Vec<Painted>,
    /// Every surface key in tree order, which is also z-order.
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

/// Text runs keyed by (text, size bits): shaped once, reused across frames
/// while the font stays the same. Own one in your runtime and pass it to
/// [`resolve_scene_with`].
#[derive(Debug, Default)]
pub struct TextCache {
    font: usize,
    runs: HashMap<(String, u64), TextRun>,
}
impl TextCache {
    pub fn len(&self) -> usize {
        self.runs.len()
    }
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }
}

struct Runs<'a> {
    font: Option<&'a [u8]>,
    tolerance: f64,
    cache: &'a mut HashMap<(String, u64), TextRun>,
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

/// A float, painted after the whole tree so it sits on top and escapes
/// every clip.
#[derive(Clone)]
struct Deferred<'a> {
    at: usize,
    node: &'a El,
    path: String,
    under: Color,
    cursor: Option<Cursor>,
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
    deferred: Vec<Deferred<'a>>,
}
impl<'a> Walk<'a> {
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
            text: None,
        });
        self.paint.last_mut()
    }

    fn node<'n: 'a>(
        &mut self,
        n: &'n El,
        path: &str,
        under: Color,
        cursor: Option<Cursor>,
        clip: Option<Bounds>,
    ) -> Result<(), SceneError> {
        let frame = self.frames[self.i];
        let at = self.i;
        self.i += 1;
        let key = n.key().map_or_else(|| path.to_owned(), str::to_owned);
        let th = self.spec.theme;
        let e = n.payload();
        let s = &e.style;
        let cursor = s.cursor.or(cursor);
        if frame.size.width <= 0.0 || frame.size.height <= 0.0 {
            // A flex share that collapsed to nothing: invisible, and so are
            // its children.
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
            let d = d.resolve(th.spacing);
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

        match &e.content {
            Content::Text(t) => {
                let size = e.text_size.unwrap_or(th.text);
                // Text's own fill is its ink, not a box behind it.
                let ink = if s.fill.is_none() {
                    Fill::Role(crate::Role::Ink)
                } else {
                    s.fill.clone()
                };
                self.paint
                    .retain(|p| !(p.key == key && p.layer == Layer::Fill));
                bg = under;
                if let Some(run) = self.runs.run(t, size)? {
                    // Baseline placed so ascent+descent sits centred in the frame.
                    let dy =
                        frame.y + (frame.size.height - run.ascent - run.descent) / 2.0 + run.ascent;
                    let origin = Point::new(frame.x, dy);
                    let glyphs = run.path.rigid_transform(origin, 0.0)?;
                    let text = self.spec.font.clone().map(|font| Text {
                        font,
                        size: size as f32,
                        origin,
                        glyphs: run.glyphs.iter().map(|&(g, x)| (g, x as f32)).collect(),
                    });
                    if let Some(p) = self.push(Layer::Text, glyphs, None, &ink, under) {
                        p.text = text;
                    }
                }
            }
            Content::Canvas(c) => {
                let origin = Point::new(frame.x, frame.y);
                for (k, d) in (c.0)(frame.size).into_iter().enumerate() {
                    let moved = d.path.rigid_transform(origin, 0.0)?;
                    if let Some(p) = self.push(Layer::Draw(k), moved, None, &d.fill, bg) {
                        p.width = d.width;
                    }
                }
            }
            Content::None => {}
        }

        let scrolled = n.scroll_offset();
        let mut content = frame.size;
        if n.is_scroll() {
            // The children's extent, read back from their frames.
            let pad = n.padding(th.spacing);
            let sub = &self.frames[at + 1..at + count(n)];
            let (mut right, mut bottom) = (frame.x, frame.y);
            for f in sub {
                right = right.max(f.right());
                bottom = bottom.max(f.bottom());
            }
            content = Size::new(
                (right - frame.x + scrolled[0] + pad.right - pad.left).max(0.0),
                (bottom - frame.y + scrolled[1] + pad.bottom - pad.top).max(0.0),
            );
        }
        self.keys.push(key.clone());
        self.surfaces.insert(
            key.clone(),
            ResolvedSurface {
                key: key.clone(),
                frame,
                bounds: Bounds::from_points(outline.flatten(0.5, 100_000)?.concat()),
                path: outline.clone(),
                rect,
                topology_changed: changed,
                cursor,
                tip: e.tip.clone(),
                focusable: e.focusable,
                clip,
                content,
            },
        );
        let inner = if n.is_clip() {
            let b = bounds(frame);
            let b = clip.map_or(b, |c| {
                Bounds::new(
                    b.min.x.max(c.min.x),
                    b.min.y.max(c.min.y),
                    b.max.x.min(c.max.x),
                    b.max.y.min(c.max.y),
                )
            });
            self.push(
                Layer::Clip,
                outline,
                rect,
                &Fill::Color(Color::oklcha(0.0, 0.0, 0.0, 0.0)),
                bg,
            );
            Some(b)
        } else {
            clip
        };
        for (j, c) in n.children().iter().enumerate() {
            let path = format!("{path}/{j}");
            if c.is_float() {
                self.deferred.push(Deferred {
                    at: self.i,
                    node: c,
                    path,
                    under: bg,
                    cursor,
                });
                self.i += count(c);
            } else {
                self.node(c, &path, bg, cursor, inner)?;
            }
        }
        if n.is_clip() {
            self.key = key;
            let clear = Fill::Color(Color::oklcha(0.0, 0.0, 0.0, 0.0));
            self.push(Layer::Unclip, Path::default(), None, &clear, bg);
        }
        Ok(())
    }
}

pub fn resolve_scene(spec: &SceneSpec) -> Result<ResolvedScene, SceneError> {
    resolve_scene_with(spec, &mut TextCache::default())
}

/// [`resolve_scene`] with text shaped once per (string, size) across calls.
pub fn resolve_scene_with(
    spec: &SceneSpec,
    text: &mut TextCache,
) -> Result<ResolvedScene, SceneError> {
    if !spec.theme.valid() {
        return Err(SceneError::InvalidTheme);
    }
    let font_id = spec.font.as_ref().map_or(0, |f| Arc::as_ptr(f) as usize);
    if text.font != font_id {
        text.runs.clear();
        text.font = font_id;
    }
    let mut runs = Runs {
        font: spec.font.as_deref().map(Vec::as_slice),
        tolerance: spec.tolerance,
        cache: &mut text.runs,
    };
    let th = spec.theme;
    let layout = resolve_with(
        &spec.root,
        spec.offered,
        spec.limits,
        th.spacing,
        |e| match &e.content {
            Content::Text(t) => runs.measure(t, e.text_size.unwrap_or(th.text)),
            Content::None | Content::Canvas(_) => Size::ZERO,
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
        deferred: Vec::new(),
    };
    w.node(&spec.root, "", th.palette.background(), None, None)?;
    // Floats paint last, in the order they were met; a float inside a float
    // lands on the end of the same queue.
    let mut k = 0;
    while k < w.deferred.len() {
        let Deferred {
            at,
            node,
            path,
            under,
            cursor,
        } = w.deferred[k].clone();
        w.i = at;
        w.node(node, &path, under, cursor, None)?;
        k += 1;
    }
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
    use crate::prelude::*;
    use crate::{CornerProfile, Spacing};

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

#[cfg(test)]
mod feature_tests {
    use super::*;
    use crate::prelude::*;

    #[test]
    fn clip_floats_canvas_cursor_and_content() {
        let list = column([leaf(50., 30.).id("a"), leaf(50., 30.), leaf(50., 30.)])
            .gap(10.)
            .scroll()
            .scrolled(0., 25.)
            .size(60., 60.)
            .cursor(Cursor::Hand)
            .id("list");
        let tip = leaf(10., 10.).fill(Primary).float().id("tip");
        let draw = canvas(|s| {
            vec![Draw::stroke(
                Path::default().move_to(Point::new(0., s.height)).cubic_to(
                    Point::new(s.width / 2., 0.),
                    Point::new(s.width / 2., 0.),
                    Point::new(s.width, s.height),
                ),
                Primary,
                2.,
            )]
        })
        .size(40., 40.)
        .id("curve");
        let root = column([list, tip, draw]).id("root");
        let s = resolve_scene(&SceneSpec::new(root)).unwrap();
        let layers: Vec<_> = s.paint.iter().map(|p| (p.key.as_str(), p.layer)).collect();
        let at = |k: &str, l: Layer| layers.iter().position(|x| *x == (k, l)).unwrap();
        assert!(at("list", Layer::Clip) < at("list", Layer::Unclip));
        assert_eq!(*layers.last().unwrap(), ("tip", Layer::Fill), "{layers:?}");
        assert!(s
            .paint
            .iter()
            .any(|p| p.key == "curve" && p.layer == Layer::Draw(0) && p.width == 2.));
        let list = s.surface("list").unwrap();
        assert_eq!(
            list.content,
            Size::new(55., 110.),
            "rows centred at x=5, three rows and two gaps tall"
        );
        assert_eq!(list.clip, None);
        assert_eq!(s.surface("a").unwrap().clip, Some(list.bounds.unwrap()));
        assert_eq!(s.surface("a").unwrap().cursor, Some(Cursor::Hand));
        assert_eq!(s.layout.frame("a").unwrap().y, -25.);
        assert_eq!(s.keys.last().map(String::as_str), Some("tip"));
    }

    #[test]
    fn text_cache_survives_frames_and_carries_glyphs() {
        let mut cache = TextCache::default();
        let mut sp = SceneSpec::new(row([text("hi").id("t")]));
        sp.font = Some(Arc::new(epaint_default_fonts::HACK_REGULAR.to_vec()));
        let s = resolve_scene_with(&sp, &mut cache).unwrap();
        assert_eq!(cache.len(), 1);
        let t = s
            .paint
            .iter()
            .find(|p| p.layer == Layer::Text)
            .unwrap()
            .text
            .as_ref()
            .unwrap();
        assert_eq!(t.glyphs.len(), 2);
        assert!(t.glyphs[1].1 > 0.);
        resolve_scene_with(&sp, &mut cache).unwrap();
        assert_eq!(cache.len(), 1);
    }
}
