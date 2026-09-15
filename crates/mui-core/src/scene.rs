//! The tree becomes paint.
//!
//! One pre-order walk over the styled tree, in step with the frames the
//! solver produced in the same order. Every node gets an outline; every layer
//! of its [`Style`](crate::Style) becomes one [`Painted`] entry. Children
//! paint after their parent, so a list index is a z-order.
use std::borrow::Cow;
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
    pub font: Option<Arc<[u8]>>,
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
    pub fn font(mut self, font: impl Into<Arc<[u8]>>) -> Self {
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
    pub font: Arc<[u8]>,
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
    pub key: Arc<str>,
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
    /// Present on `Layer::Text` whenever [`SceneSpec::font`] is set: the
    /// layer's ink, as glyphs. `path` is then empty -- a renderer that draws
    /// glyphs never looks at it, and translating every run's outline into a
    /// fresh path is the most expensive thing the walk can do.
    pub text: Option<Text>,
}

/// A node's outline, for hit-testing and for anything that derives from it.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedSurface {
    pub key: Arc<str>,
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
    pub keys: Vec<Arc<str>>,
    surfaces: BTreeMap<Arc<str>, ResolvedSurface>,
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
        match self {
            Self::InvalidTheme => {
                f.write_str("the theme's corners, spacing or palette are unusable")
            }
            Self::InvalidRadius => f.write_str(
                "a corner radius, shell inset or stroke width is negative or not finite",
            ),
            Self::Layout(e) => write!(f, "{e}"),
            Self::Geometry(e) => write!(f, "{e}"),
            Self::Text(e) => write!(f, "{e}"),
            Self::RevisionExhausted => f.write_str("the scene revision counter overflowed"),
        }
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
    runs: HashMap<String, HashMap<u64, TextRun>>,
}
impl TextCache {
    pub fn len(&self) -> usize {
        self.runs.values().map(HashMap::len).sum()
    }
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }
}

struct Runs<'a> {
    font: Option<&'a [u8]>,
    tolerance: f64,
    cache: &'a mut HashMap<String, HashMap<u64, TextRun>>,
}
impl Runs<'_> {
    fn run(&mut self, text: &str, size: f64) -> Result<Option<&TextRun>, mui_text::Error> {
        let Some(font) = self.font else {
            return Ok(None);
        };
        // Nested so a hit borrows `text` instead of allocating a key for it:
        // `run` is called several times per line, per frame.
        let bits = size.to_bits();
        if self.cache.get(text).is_none_or(|m| !m.contains_key(&bits)) {
            let run = mui_text::text_run(font, text, size, &[], self.tolerance)?;
            // ponytail: an unbounded cache holds every string ever shown, so
            // flush the lot at a ceiling -- one cold frame. Per-entry frame
            // stamping is the upgrade if that ever shows.
            if self.cache.len() > 4096 {
                self.cache.clear();
            }
            self.cache
                .entry(text.to_owned())
                .or_default()
                .insert(bits, run);
        }
        Ok(self.cache.get(text).and_then(|m| m.get(&bits)))
    }
    /// The lines `text` breaks into at `max` width, capped at `cap` of them
    /// with an ellipsis on the last. One line when it fits, or when there is
    /// no font to break against.
    ///
    /// Every line is a slice of `text`, so the overwhelmingly common case --
    /// a label that fits -- allocates the `Vec` and nothing else. Only an
    /// ellipsised last line owns its bytes.
    fn lines<'t>(
        &mut self,
        text: &'t str,
        size: f64,
        max: f64,
        cap: Option<usize>,
    ) -> Vec<Cow<'t, str>> {
        let fits = self.measure(text, size).width <= max + 0.5;
        let Some(font) = self.font.filter(|_| !fits && max > 0.0) else {
            return vec![Cow::Borrowed(text)];
        };
        let Ok(lines) = mui_text::break_lines(font, text, size, max) else {
            return vec![Cow::Borrowed(text)];
        };
        let n = cap.unwrap_or(usize::MAX).max(1);
        let mut out: Vec<Cow<'t, str>> = lines
            .iter()
            .take(n)
            .map(|l| Cow::Borrowed(text[l.text_range.clone()].trim_end()))
            .collect();
        if lines.len() > n {
            // ponytail: the ellipsis is appended, not measured -- a capped
            // line can overhang by one glyph. Re-break the last line against
            // `max - advance('…')` if that shows.
            if let Some(last) = out.last_mut() {
                last.to_mut().push('\u{2026}');
            }
        }
        if out.is_empty() {
            out.push(Cow::Borrowed(""));
        }
        out
    }
    /// A wrapped label's box: the widest line by the stack of line heights.
    fn wrapped(&mut self, text: &str, size: f64, max: f64, cap: Option<usize>) -> Size {
        let (mut w, mut h) = (0.0f64, 0.0);
        for l in self.lines(text, size, max, cap) {
            let s = self.measure(&l, size);
            w = w.max(s.width);
            h += s.height;
        }
        Size::new(w, h)
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
    key: Arc<str>,
    paint: Vec<Painted>,
    keys: Vec<Arc<str>>,
    surfaces: BTreeMap<Arc<str>, ResolvedSurface>,
    deferred: Vec<Deferred<'a>>,
    /// The baseline a `.baseline()` parent asks its text children to sit on.
    base_y: Option<f64>,
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
        // ponytail: one `Arc<str>` per node per frame, cloned four times
        // instead of four heap copies; interning across frames is the upgrade.
        let key: Arc<str> = n.key().map_or_else(|| Arc::from(path), Arc::from);
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
                // The Fill this node pushed a few lines up, not a scan of
                // every node painted so far.
                if let Some(i) = self
                    .paint
                    .iter()
                    .rposition(|p| p.key == key && p.layer == Layer::Fill)
                {
                    self.paint.remove(i);
                }
                bg = under;
                let lines = self.runs.lines(t, size, frame.size.width, e.lines);
                let n = lines.len();
                let base = self.base_y;
                for (li, line) in lines.iter().enumerate() {
                    let Some(run) = self.runs.run(line, size)? else {
                        break;
                    };
                    // One line sits centred on ascent+descent, or on the
                    // baseline its parent chose; a stack centres the block.
                    let dy = match (base, n) {
                        (Some(b), 1) => b,
                        (_, 1) => {
                            frame.y
                                + (frame.size.height - run.ascent - run.descent) / 2.0
                                + run.ascent
                        }
                        _ => {
                            frame.y
                                + (frame.size.height - n as f64 * run.line_height) / 2.0
                                + run.ascent
                                + li as f64 * run.line_height
                        }
                    };
                    let origin = Point::new(frame.x, dy);
                    let ids: Arc<[(u32, f32)]> =
                        run.glyphs.iter().map(|&(g, x)| (g, x as f32)).collect();
                    let text = self.spec.font.clone().map(|font| Text {
                        font,
                        size: size as f32,
                        origin,
                        glyphs: ids,
                    });
                    let ink_path = if text.is_some() {
                        Path::default()
                    } else {
                        run.path.rigid_transform(origin, 0.0)?
                    };
                    if let Some(p) = self.push(Layer::Text, ink_path, None, &ink, under) {
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
                bounds: match rect {
                    // A rounded rectangle already knows its bounds; only a
                    // welded outline has to be flattened to find them.
                    Some(r) => Some(r.bounds()),
                    None => Bounds::from_points(outline.flatten(0.5, 100_000)?.concat()),
                },
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
        let outer_base = self.base_y;
        self.base_y = None;
        if e.baseline {
            let mut asc: Option<f64> = None;
            for c in n.children() {
                if let Content::Text(t) = &c.payload().content {
                    let s = c.payload().text_size.unwrap_or(th.text);
                    if let Some(r) = self.runs.run(t, s)? {
                        asc = Some(asc.unwrap_or(0.0).max(r.ascent));
                    }
                }
            }
            self.base_y = asc.map(|a| frame.y + n.padding(th.spacing).top + a);
        }
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
        self.base_y = outer_base;
        if n.is_clip() {
            self.key = key;
            let clear = Fill::Color(Color::oklcha(0.0, 0.0, 0.0, 0.0));
            self.push(Layer::Unclip, Path::default(), None, &clear, bg);
        }
        Ok(())
    }
}

/// A content leaf's size: a paragraph wrapped to its room when it needs it.
/// `want` remembers the width it settled on, keyed by the element's address.
fn fit(
    runs: &mut Runs,
    th: Theme,
    e: &crate::Element,
    room: Option<f64>,
    want: &mut HashMap<usize, f64>,
) -> Size {
    let Content::Text(t) = &e.content else {
        return Size::ZERO;
    };
    let (t, size) = (t.as_str(), e.text_size.unwrap_or(th.text));
    let s = match room {
        Some(w) if w > 0.0 && runs.measure(t, size).width > w + 0.5 => {
            runs.wrapped(t, size, w, e.lines)
        }
        _ => runs.measure(t, size),
    };
    want.insert(std::ptr::from_ref(e) as usize, s.width);
    s
}

/// Every text node a row squeezed narrower than the width it measured at.
/// This keys on the text node's *own* frame, so a squeeze that lands on an
/// ancestor is invisible here: mui-layout has to clamp a container's children
/// to its cross size for that case to show up at all.
fn wrap_hints(
    n: &El,
    frames: &[Frame],
    i: &mut usize,
    want: &HashMap<usize, f64>,
    out: &mut HashMap<usize, f64>,
) {
    let f = frames[*i];
    *i += 1;
    let k = std::ptr::from_ref(n.payload()) as usize;
    if f.size.width > 0.0 && want.get(&k).is_some_and(|w| *w > f.size.width + 0.5) {
        out.insert(k, f.size.width);
    }
    for c in n.children() {
        wrap_hints(c, frames, i, want, out);
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
    if !spec.theme.is_valid() {
        return Err(SceneError::InvalidTheme);
    }
    let font_id = spec.font.as_ref().map_or(0, |f| f.as_ptr() as usize);
    if text.font != font_id {
        text.runs.clear();
        text.font = font_id;
    }
    let mut runs = Runs {
        font: spec.font.as_deref(),
        tolerance: spec.tolerance,
        cache: &mut text.runs,
    };
    let th = spec.theme;
    // A paragraph wraps to its room in this one pass. The element address
    // `want` keys on is stable for as long as `spec` is borrowed.
    let mut want = HashMap::new();
    let layout = resolve_with(
        &spec.root,
        spec.offered,
        spec.limits,
        th.spacing,
        |e, room| fit(&mut runs, th, e, room, &mut want),
    )?;
    // A row hands its content a share, not the room, so a paragraph beside
    // another can still come out narrower than it measured. Only then is the
    // tree solved again, with that share as the width to wrap to.
    // ponytail: a second solve for side-by-side paragraphs; the flex pass
    // re-measuring its items at their final main size is the upgrade.
    let mut hints = HashMap::new();
    wrap_hints(&spec.root, layout.all(), &mut 0, &want, &mut hints);
    let layout = if hints.is_empty() {
        layout
    } else {
        resolve_with(
            &spec.root,
            spec.offered,
            spec.limits,
            th.spacing,
            |e, room| match (&e.content, hints.get(&(std::ptr::from_ref(e) as usize))) {
                (Content::Text(t), Some(&w)) => {
                    runs.wrapped(t, e.text_size.unwrap_or(th.text), w, e.lines)
                }
                _ => fit(&mut runs, th, e, room, &mut want),
            },
        )?
    };
    let mut w = Walk {
        spec,
        frames: layout.all(),
        runs,
        i: 0,
        key: Arc::from(""),
        paint: Vec::new(),
        keys: Vec::new(),
        surfaces: BTreeMap::new(),
        deferred: Vec::new(),
        base_y: None,
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
        w.base_y = None;
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
        let layers: Vec<_> = s.paint.iter().map(|p| (&*p.key, p.layer)).collect();
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
        sp.font = Some(Arc::from(epaint_default_fonts::HACK_REGULAR));
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
        let layers: Vec<_> = s.paint.iter().map(|p| (&*p.key, p.layer)).collect();
        let at = |k: &str, l: Layer| layers.iter().position(|x| *x == (k, l)).unwrap();
        assert!(at("list", Layer::Clip) < at("list", Layer::Unclip));
        assert_eq!(*layers.last().unwrap(), ("tip", Layer::Fill), "{layers:?}");
        assert!(s
            .paint
            .iter()
            .any(|p| &*p.key == "curve" && p.layer == Layer::Draw(0) && p.width == 2.));
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
        assert_eq!(s.keys.last().map(|k| &**k), Some("tip"));
    }

    fn font() -> Arc<[u8]> {
        Arc::from(epaint_default_fonts::HACK_REGULAR)
    }

    #[test]
    fn a_baseline_row_lines_two_sizes_up_on_the_letters() {
        let root = row([
            text("a").text_size(12.).id("small"),
            text("b").text_size(24.).id("big"),
        ])
        .baseline();
        let mut sp = SceneSpec::new(root);
        sp.font = Some(font());
        let s = resolve_scene(&sp).unwrap();
        let y = |k: &str| {
            s.paint
                .iter()
                .find(|p| &*p.key == k && p.layer == Layer::Text)
                .unwrap()
                .text
                .as_ref()
                .unwrap()
                .origin
                .y
        };
        assert_eq!(y("small"), y("big"), "one baseline, two sizes");
        let mut plain = sp.clone();
        plain.root = row([
            text("a").text_size(12.).id("small"),
            text("b").text_size(24.).id("big"),
        ]);
        let p = resolve_scene(&plain).unwrap();
        let py = |k: &str| {
            p.paint
                .iter()
                .find(|x| &*x.key == k && x.layer == Layer::Text)
                .unwrap()
                .text
                .as_ref()
                .unwrap()
                .origin
                .y
        };
        assert_ne!(py("small"), py("big"), "and centring alone does not");
    }

    #[test]
    fn the_same_paragraph_wraps_to_each_width_it_is_given_and_a_row_share_too() {
        let long = "wrap ".repeat(40);
        // Two copies of one string in two widths: each wraps to its own,
        // so the wider one is shorter. A third beside a sibling in a
        // definite row gets its flex share, narrower than the row.
        let root = column([
            column([text(long.clone()).id("a")]).w(120),
            column([text(long.clone()).id("b")]).w(240),
            row([
                text(long.clone()).id("c").shrink(1.0),
                text(long).id("d").shrink(1.0),
            ])
            .w(300),
        ]);
        let mut sp = SceneSpec::new(root);
        sp.font = Some(font());
        let s = resolve_scene(&sp).unwrap();
        let f = |k| s.layout.frame(k).unwrap().size;
        assert!(
            f("a").width <= 120.1 && f("b").width <= 240.1,
            "{:?} {:?}",
            f("a"),
            f("b")
        );
        assert!(
            f("a").height > f("b").height * 1.5,
            "{:?} {:?}",
            f("a"),
            f("b")
        );
        assert!(
            f("c").width <= 150.1 && f("c").height > f("b").height,
            "{:?}",
            f("c")
        );
        let lines = |k| {
            s.paint
                .iter()
                .filter(|p| &*p.key == k && p.layer == Layer::Text)
                .count()
        };
        assert!(
            lines("a") > lines("b") && lines("c") > lines("b"),
            "{} {} {}",
            lines("a"),
            lines("b"),
            lines("c")
        );
    }

    #[test]
    fn a_narrow_column_wraps_a_paragraph_and_grows_taller() {
        let long = "wrap ".repeat(40);
        let one = {
            let mut sp = SceneSpec::new(column([text(long.clone()).id("t")]));
            sp.font = Some(font());
            resolve_scene(&sp).unwrap().layout.frame("t").unwrap().size
        };
        let mut sp = SceneSpec::new(column([text(long.clone()).id("t")]).w(120));
        sp.font = Some(font());
        let s = resolve_scene(&sp).unwrap();
        let lines = s
            .paint
            .iter()
            .filter(|p| &*p.key == "t" && p.layer == Layer::Text)
            .count();
        assert!(lines > 3, "wrapped into {lines} lines");
        let f = s.layout.frame("t").unwrap().size;
        assert!(f.width <= 120.1 && f.height > one.height * 3., "{f:?}");
        let mut capped = sp.clone();
        capped.root = column([text(long).id("t").lines(2)]).w(120);
        let c = resolve_scene(&capped).unwrap();
        assert_eq!(
            c.paint
                .iter()
                .filter(|p| &*p.key == "t" && p.layer == Layer::Text)
                .count(),
            2,
            "capped at two lines"
        );
    }

    #[test]
    fn a_rect_surface_reads_its_bounds_off_the_rect() {
        let s = resolve_scene(&SceneSpec::new(column([leaf(40., 20.).id("k")]))).unwrap();
        let k = s.surface("k").unwrap();
        assert_eq!(k.bounds.unwrap(), k.rect.unwrap().bounds());
    }

    #[test]
    fn a_text_node_keeps_no_fill_layer_and_no_glyph_path() {
        let root = column([text("hi").fill(Role::Primary).id("t")]);
        let mut sp = SceneSpec::new(root);
        sp.font = Some(font());
        let s = resolve_scene(&sp).unwrap();
        assert!(
            !s.paint
                .iter()
                .any(|p| &*p.key == "t" && p.layer == Layer::Fill),
            "a label's fill is its ink, not a box: {:?}",
            s.paint.iter().map(|p| p.layer).collect::<Vec<_>>()
        );
        let ink = s.paint.iter().find(|p| p.layer == Layer::Text).unwrap();
        assert_eq!(ink.paint, Paint::Solid(Theme::default().palette.primary()));
        assert!(ink.text.is_some(), "the glyphs are the ink");
        assert!(
            ink.path.commands.is_empty(),
            "and the outline is not built twice"
        );
    }

    #[test]
    fn a_canvas_closure_may_capture_a_non_send_handle() {
        let seen = std::rc::Rc::new(std::cell::Cell::new(0));
        let c = seen.clone();
        let root = canvas(move |_| {
            c.set(c.get() + 1);
            Vec::new()
        })
        .size(10., 10.);
        resolve_scene(&SceneSpec::new(root)).unwrap();
        assert_eq!(seen.get(), 1);
    }

    #[test]
    fn text_cache_keys_on_size_as_well_as_string() {
        let mut cache = TextCache::default();
        let mut sp = SceneSpec::new(row([
            text("hi").text_size(12.).id("a"),
            text("hi").text_size(24.).id("b"),
        ]));
        sp.font = Some(font());
        resolve_scene_with(&sp, &mut cache).unwrap();
        assert_eq!(cache.len(), 2, "one string, two sizes");
    }

    #[test]
    fn errors_read_as_sentences_not_as_debug() {
        let e = resolve_scene(&SceneSpec::new(leaf(f64::NAN, 1.))).unwrap_err();
        let s = e.to_string();
        assert!(!s.contains("Layout("), "{s}");
        assert_eq!(s, mui_layout::Error::InvalidValue.to_string());
        assert!(SceneError::InvalidTheme.to_string().contains("theme"));
    }

    #[test]
    fn text_cache_survives_frames_and_carries_glyphs() {
        let mut cache = TextCache::default();
        let mut sp = SceneSpec::new(row([text("hi").id("t")]));
        sp.font = Some(Arc::from(epaint_default_fonts::HACK_REGULAR));
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
