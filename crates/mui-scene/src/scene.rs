//! The tree becomes paint.
//!
//! One pre-order walk over the styled tree, in step with the frames the
//! solver produced in the same order. Every node gets an outline; every layer
//! of its [`Style`](crate::Style) becomes one [`Painted`] entry. Children
//! paint after their parent, so a list index is a z-order.
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use mui_geometry::{
    boolean, fillet, inset_path, union, BooleanOp, Bounds, CornerStyle, Fillet, GeometryOptions,
    OffsetOptions, Path, PlacedShape, Point, RoundedRect, Topology,
};
use mui_layout::{Frame, Layout, Limits, Size};
use mui_text::{Axes, Font, TextRun};

use crate::regions::Operation;
use crate::{
    Carve, Color, Content, Cursor, El, Fill, Mix, Paint, Radius, Semantics, Shadow, ShadowKind,
    Theme,
};

#[derive(Debug, Clone)]
pub struct SceneSpec {
    pub theme: Theme,
    pub root: El,
    pub offered: Option<Size>,
    pub limits: Limits,
    pub geometry: GeometryOptions,
    pub offsets: OffsetOptions,
    /// The face for `text(..)` leaves. Without one, text is boxed at an
    /// estimate and draws nothing, so a layout test needs no font file.
    pub font: Option<Font>,
    /// Additional faces tried per grapheme when the primary face has no
    /// glyph. They are carried into the resolved text layer so both CPU and
    /// GPU renderers draw the selected face.
    pub fallback_fonts: Vec<Font>,
    /// Curve tolerance for glyph outlines.
    pub tolerance: f64,
    /// The host's device pixels per layout unit. Set it and every edge the
    /// walk derives -- outlines, welds, clips, baselines -- lands on a device
    /// pixel, so abutting fills composite opaque and hinted glyphs keep an
    /// even leading. `None` leaves layout's raw f64 alone.
    pub device_scale: Option<f64>,
    /// Selected by the host. CPU reference remains available for snapshots.
    pub weld_backend: crate::WeldBackend,
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
            fallback_fonts: Vec::new(),
            tolerance: 0.05,
            device_scale: None,
            weld_backend: crate::WeldBackend::Reference,
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
    pub fn font(mut self, font: Font) -> Self {
        self.font = Some(font);
        self
    }
    /// Add a fallback face after the primary [`Self::font`].
    pub fn fallback_font(mut self, font: Font) -> Self {
        self.fallback_fonts.push(font);
        self
    }
    /// Snap every painted edge to the device grid. Three equal shares of 41
    /// px land on thirds, and two of the three seams between them composite
    /// translucent; at scale 1 they land on whole pixels instead:
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// let row = row![leaf(0., 20.).grow(1.).id("a"), leaf(0., 20.).grow(1.)];
    /// let spec = SceneSpec::new(row).offered(Size::new(41., 20.)).scale(1.);
    /// let a = resolve_scene(&spec).unwrap();
    /// let edge = a.surface("a").unwrap().rect.unwrap().bounds().max.x;
    /// assert_eq!(edge, edge.round());
    /// ```
    pub fn scale(mut self, device_scale: f64) -> Self {
        self.device_scale = Some(device_scale);
        self
    }
}

/// Which layer of a node's style a [`Painted`] entry is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layer {
    /// One shadow of the node's list; an inset one paints clipped to the
    /// node's own outline.
    Shadow(ShadowKind),
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
    /// Everything up to the matching `Unblend` composites as one layer; the
    /// path and paint are meaningless. Wraps the node's own `Clip`, so a
    /// blended subtree's clip is inside its layer -- but a float declared in
    /// that subtree paints after the root, hence outside it.
    Blend {
        mix: Mix,
        opacity: f32,
    },
    /// Painted source-atop the node's own blend layer, in `path`: it lands
    /// only where the node and its children already painted. Always inside
    /// a `Blend`/`Unblend` pair. See [`Paints::mask`](crate::Paints::mask).
    Mask,
    Unblend,
    /// External GPU material, sampled in paint order through the effect renderer.
    External,
}

/// One shaped glyph in a text layer.
///
/// `x` and `y` are offsets from the run baseline origin in scene pixels. The
/// y offset matters for combining marks and OpenType GPOS; carrying it here
/// keeps the glyph cache renderer in agreement with the outline path. `font`
/// indexes [`Text::fonts`], so a fallback glyph can never accidentally be
/// looked up in the primary face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextGlyph {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub font: usize,
}

/// A text layer's glyphs, for a renderer that hints and caches its own.
#[derive(Clone, Debug)]
pub struct Text {
    /// The primary face, retained for compatibility with callers that only
    /// need one face. It is also `fonts[0]` whenever `fonts` is non-empty.
    pub font: Font,
    /// Primary face followed by any fallback faces used by this run.
    pub fonts: Arc<[Font]>,
    pub size: f32,
    /// Baseline origin.
    pub origin: Point,
    /// Shaped glyph id, x/y offset and face index from the origin.
    pub glyphs: Arc<[TextGlyph]>,
    /// The axis settings the run was shaped at; `coords` is the same thing
    /// in the form a glyph cache wants, and [`ResolvedScene::set_text`]
    /// re-shapes from this one.
    pub axes: Axes,
    /// The face's normalized axis coordinates this run was measured at, from
    /// [`mui_text::normalized_coords`]. Empty for a static face. A renderer
    /// with its own glyph cache has to pass these on, or it paints the
    /// default instance under a bold run's advances.
    pub coords: Arc<[i16]>,
    /// Per-face normalized coordinates. `coords` remains the primary face's
    /// value for callers that only know about one font.
    pub font_coords: Arc<[Arc<[i16]>]>,
    /// Whether a renderer should hint this run. Off for the frame after its
    /// axes moved: a glyph mid-morph gains nothing from stem snapping and
    /// would cost a fresh hinting instance per frame.
    pub hint: bool,
}
impl PartialEq for Text {
    fn eq(&self, o: &Self) -> bool {
        self.font == o.font
            && self.fonts == o.fonts
            && self.size == o.size
            && self.origin == o.origin
            && self.glyphs == o.glyphs
            && self.axes == o.axes
            && self.coords == o.coords
            && self.font_coords == o.font_coords
            && self.hint == o.hint
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
    /// Switched off by itself or by an ancestor: not a hit target, not a Tab
    /// stop, and reported disabled to a screen reader. See
    /// [`Styled::disabled`](crate::Styled::disabled).
    pub disabled: bool,
    /// The role and name this surface reports to a screen reader.
    pub semantics: Option<Semantics>,
    /// The name came from this node's text because no explicit `.label(..)`
    /// was supplied. Live text swaps update this name; an explicit label does
    /// not move with the paint.
    semantic_label_implicit: bool,
    /// Current authored text, used as the accessible name unless explicitly
    /// overridden by semantics. Live readout updates change this too.
    pub text_value: Option<String>,
    /// The nearest clipping ancestor's frame, for hit-testing.
    ///
    /// This is kept as a rectangle for compatibility with the input adapter.
    /// [`Self::clip_path`] carries the same ancestor's actual outline for
    /// adapters that need corner-accurate filtering.
    pub clip: Option<Bounds>,
    /// The clipping ancestors' outlines, cached during scene resolution from
    /// outermost to innermost. This is the path counterpart to [`Self::clip`];
    /// it avoids making every pointer query tessellate a rounded or welded
    /// clip and preserves every nested clip boundary.
    pub clip_path: Option<Arc<[Path]>>,
    /// Nearest explicitly named ancestor in the authored tree, not a containing
    /// rectangle. A floating node keeps this parent even when it escapes clipping.
    pub parent: Option<Arc<str>>,
    /// A scroll node's children extent inside its padding, unscrolled;
    /// the frame size otherwise.
    pub content: Size,
    /// The tagged shapes a `canvas` drew, in scene space. Non-empty means
    /// *these* are the surface's hit geometry, not its outline: the pointer
    /// outside all of them is outside the node. See [`Draw::tag`].
    pub hits: Vec<(Arc<str>, Path)>,
}
impl ResolvedSurface {
    /// Borrow the cached clip outlines without exposing their shared
    /// allocation. Paths are ordered outermost to innermost.
    pub fn clip_paths(&self) -> Option<&[Path]> {
        self.clip_path.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedScene {
    pub layout: Layout,
    pub paint: Vec<Painted>,
    /// Every surface in paint order, which is also z-order.
    surfaces: Vec<ResolvedSurface>,
    at: HashMap<Arc<str>, usize>,
    pub(crate) external_welds: HashMap<Arc<str>, crate::ExternalWeld>,
    /// What the scene was shaped with, so a live readout can re-shape one
    /// run without the spec that produced it. See [`Self::set_text`].
    font: Option<Font>,
    fallback_fonts: Vec<Font>,
    tolerance: f64,
}
impl ResolvedScene {
    /// Swap what one text node says, keeping every frame this scene already
    /// solved: only that node's glyph run is shaped again.
    ///
    /// This is the 60 Hz readout -- a modulated value, a meter, a clock --
    /// where re-resolving the tree to move six digits is the whole frame
    /// budget. Pair it with [`Styled::reserve`](crate::Styled::reserve): the
    /// box was measured for the widest string the node can show, so the
    /// shorter ones sit inside it and nothing reflows.
    ///
    /// ponytail: one line, painted from the old run's origin. A string wider
    /// than the frame overhangs instead of wrapping. The accessible text is
    /// updated, but an explicitly authored accessibility label is preserved.
    /// The next resolve paints whatever the tree says, so update its value too.
    ///
    /// ```
    /// use mui_scene::prelude::*;
    /// # use mui_scene::{Layer, SceneSpec};
    /// let root = row![text("0.0 dB").reserve("-88.8 dB").id("gain")];
    /// let spec = SceneSpec::new(root).font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    /// let mut scene = resolve_scene(&spec).unwrap();
    /// let before = scene.surface("gain").unwrap().frame;
    /// scene.set_text("gain", "-12.4 dB").unwrap();
    /// assert_eq!(scene.surface("gain").unwrap().frame, before, "the frame is kept");
    /// let run = scene.paint.iter().find(|p| p.layer == Layer::Text).unwrap();
    /// assert_eq!(run.text.as_ref().unwrap().glyphs.len(), "-12.4 dB".len());
    /// ```
    pub fn set_text(&mut self, key: &str, s: &str) -> Result<(), SceneError> {
        let mut at = self
            .paint
            .iter()
            .enumerate()
            .filter(|(_, p)| &*p.key == key && p.layer == Layer::Text)
            .map(|(i, _)| i);
        let first = at.next().ok_or(SceneError::NoTextLayer)?;
        // A wrapped label's later lines have no string to re-break against,
        // so the swap collapses it to the one run it now says.
        let rest: Vec<usize> = at.collect();
        let old = self.paint[first]
            .text
            .as_ref()
            .ok_or(SceneError::NoTextLayer)?;
        let (size, origin, axes, coords, font_coords, font, all_fonts, hint) = (
            old.size,
            old.origin,
            old.axes.clone(),
            old.coords.clone(),
            old.font_coords.clone(),
            old.font.clone(),
            old.fonts.clone(),
            old.hint,
        );
        // The faces the run was resolved with, per-node font included.
        let fonts = if all_fonts.is_empty() {
            std::slice::from_ref(&font)
        } else {
            &all_fonts[..]
        };
        let run = CachedRun::from_run(mui_text::text_run(
            fonts,
            s,
            f64::from(size),
            &axes.to_vec(),
            self.tolerance,
        )?);
        self.paint[first].text = Some(Text {
            font,
            fonts: all_fonts,
            size,
            origin,
            glyphs: run.glyphs,
            axes,
            coords,
            font_coords,
            hint,
        });
        for i in rest.into_iter().rev() {
            self.paint.remove(i);
        }
        if let Some(&i) = self.at.get(key) {
            self.surfaces[i].text_value = Some(s.to_owned());
        }
        for surface in &mut self.surfaces {
            if &*surface.key == key && surface.semantic_label_implicit {
                if let Some(semantics) = surface.semantics.as_mut() {
                    semantics.label = Some(s.to_owned());
                }
            }
        }
        Ok(())
    }
    pub fn surface(&self, key: &str) -> Option<&ResolvedSurface> {
        self.at.get(key).map(|&i| &self.surfaces[i])
    }
    /// Every surface in paint order, which is also z-order. A key is
    /// `ResolvedSurface::key`, so nothing has to look one up to walk them.
    pub fn surfaces(&self) -> impl DoubleEndedIterator<Item = &ResolvedSurface> {
        self.surfaces.iter()
    }
}

#[derive(Debug)]
pub enum SceneError {
    InvalidTheme,
    InvalidRadius,
    MaterialWeld(mui_weld::Error),
    UnsupportedWeld(&'static str),
    /// Frame deltas must be finite and non-negative before any runtime state changes.
    InvalidFrameDelta,
    Layout(mui_layout::Error),
    Geometry(mui_geometry::Error),
    Text(mui_text::Error),
    /// [`ResolvedScene::set_text`] was asked for a key that resolved no text
    /// layer: no such node, not a text node, or no font was set.
    NoTextLayer,
    /// [`SceneSpec::device_scale`] is not a finite, positive number.
    InvalidScale,
}
impl std::fmt::Display for SceneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTheme => {
                f.write_str("the theme's corners, spacing or palette are unusable")
            }
            Self::InvalidFrameDelta => f.write_str("frame delta must be finite and non-negative"),
            Self::MaterialWeld(e) => write!(f, "{e}"),
            Self::UnsupportedWeld(feature) => write!(f, "unsupported material weld: {feature}"),
            Self::InvalidRadius => f.write_str(
                "a corner radius, shell inset or stroke width is negative or not finite",
            ),
            Self::Layout(e) => write!(f, "{e}"),
            Self::Geometry(e) => write!(f, "{e}"),
            Self::Text(e) => write!(f, "{e}"),
            Self::NoTextLayer => f.write_str("that key resolved no text layer"),
            Self::InvalidScale => f.write_str("the device scale must be finite and positive"),
        }
    }
}
impl std::error::Error for SceneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Layout(e) => Some(e),
            Self::Geometry(e) => Some(e),
            Self::Text(e) => Some(e),
            Self::MaterialWeld(e) => Some(e),
            _ => None,
        }
    }
}
impl From<mui_weld::Error> for SceneError {
    fn from(error: mui_weld::Error) -> Self {
        Self::MaterialWeld(error)
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

/// A length on the device grid, or untouched when the host gave no scale.
fn snap(v: f64, scale: Option<f64>) -> f64 {
    scale.map_or(v, |s| (v * s).round() / s)
}
/// The one place every outline, weld rect and clip path comes from, so
/// snapping here cannot leave paint, hits and clips disagreeing.
fn bounds(f: Frame, scale: Option<f64>) -> Bounds {
    Bounds {
        min: Point::new(snap(f.x, scale), snap(f.y, scale)),
        max: Point::new(snap(f.right(), scale), snap(f.bottom(), scale)),
    }
}
/// A resolved outline back as boolean input.
fn polygons(path: &Path) -> Result<Vec<PlacedShape>, SceneError> {
    Ok(mui_geometry::offset_path(
        path,
        0.,
        OffsetOptions {
            flatten_tolerance: 0.25,
            max_points: 100_000,
            ..OffsetOptions::default()
        },
    )?
    .topology
    .placed_shapes())
}

/// Every node's subtree size, by pre-order index: the subtree at `at` ends
/// just before `at + sizes[at]`. Computed once per resolve, so skipping a
/// subtree is a lookup instead of a walk.
fn subtree_sizes(n: &El, sizes: &mut Vec<usize>) -> usize {
    let at = sizes.len();
    sizes.push(1);
    let size = 1 + n
        .children()
        .iter()
        .map(|c| subtree_sizes(c, sizes))
        .sum::<usize>();
    sizes[at] = size;
    size
}

/// The pre-order index of the node keyed `id` in the subtree `n` rooted at `at`.
fn find(n: &El, id: &str, at: usize, sizes: &[usize]) -> Option<usize> {
    if n.key() == Some(id) {
        return Some(at);
    }
    let mut next = at + 1;
    for c in n.children() {
        if let Some(i) = find(c, id, next, sizes) {
            return Some(i);
        }
        next += sizes[next];
    }
    None
}

type OutlineResult = (Path, Option<RoundedRect>, bool, Vec<RoundedRect>);

/// Weld and carve outlines, the walk's most expensive geometry, keyed by
/// every input word that shaped them and compared in full: a hash match
/// alone is never trusted. Entries no resolve used are swept at its end.
#[derive(Debug, Default)]
struct OutlineCache {
    entries: HashMap<Vec<u64>, (OutlineResult, u64)>,
    /// A key buffer, handed back after a hit so a warm frame builds keys
    /// without allocating.
    scratch: Vec<u64>,
    generation: u64,
    hits: u64,
    misses: u64,
}

impl OutlineCache {
    fn get(&mut self, key: &[u64]) -> Option<OutlineResult> {
        let Some((outline, seen)) = self.entries.get_mut(key) else {
            self.misses += 1;
            return None;
        };
        *seen = self.generation;
        self.hits += 1;
        Some(outline.clone())
    }

    fn insert(&mut self, key: Vec<u64>, outline: OutlineResult) {
        self.entries.insert(key, (outline, self.generation));
    }

    /// Drop what this resolve did not use: memory follows the live tree.
    fn sweep(&mut self) {
        let generation = self.generation;
        self.entries.retain(|_, (_, seen)| *seen == generation);
        self.generation = generation.wrapping_add(1);
    }
}

/// `n`'s own outline inputs as key words. `false` for a custom
/// `.outline(..)`: nothing here can see what its closure returns, so an
/// outline that includes one is never cached.
///
/// ponytail: such a weld redoes its boolean every frame. Keying on the
/// closure's `Arc` identity would need the cache to hold the `Arc`, which is
/// not `Send` and would take `Send` away from `TextCache`; and a tree rebuilt
/// per frame makes a new closure per frame anyway, so it would never hit.
fn geometry_shallow(n: &El, frames: &[Frame], at: usize, key: &mut Vec<u64>) -> bool {
    if n.payload().outline.is_some() {
        return false;
    }
    let frame = frames[at];
    key.extend([frame.x, frame.y, frame.size.width, frame.size.height].map(f64::to_bits));
    let style = &n.payload().style;
    match style.radius {
        Radius::Theme => key.push(0),
        Radius::Px(value) => key.extend([1, value.to_bits()]),
        Radius::Token(corner) => key.extend([2, corner as u64]),
        Radius::Scale(value) => key.extend([3, value.to_bits()]),
        Radius::Pill => key.push(4),
        Radius::Pair(convex, concave) => key.extend([5, convex.to_bits(), concave.to_bits()]),
    }
    key.push(match style.corners {
        CornerStyle::Round => 0,
        CornerStyle::Squircle => 1,
    });
    key.push(u64::from(style.weld));
    key.push(match n.payload().carve {
        None => 0,
        Some(Carve::Cut) => 1,
        Some(Carve::Keep) => 2,
    });
    key.push(n.children().len() as u64);
    true
}

fn geometry_node(n: &El, frames: &[Frame], sizes: &[usize], at: usize, key: &mut Vec<u64>) -> bool {
    if !geometry_shallow(n, frames, at, key) {
        return false;
    }
    let mut child_at = at + 1;
    for child in n.children() {
        // A plain child contributes only its own rounded frame to a weld.
        // Descendants matter when this child welds them or carves one out;
        // skipping unrelated descendants keeps the cache key cheaper than
        // the boolean work it avoids.
        let complex = child.payload().style.weld
            || child
                .children()
                .iter()
                .any(|grandchild| grandchild.payload().carve.is_some());
        let keyed = if complex {
            geometry_node(child, frames, sizes, child_at, key)
        } else {
            geometry_shallow(child, frames, child_at, key)
        };
        if !keyed {
            return false;
        }
        child_at += sizes[child_at];
    }
    true
}

/// One shaped string, kept in [`TextCache`] while the font stays the same.
#[derive(Debug, Clone)]
struct CachedRun {
    path: Path,
    advance: f64,
    ascent: f64,
    descent: f64,
    line_height: f64,
    glyphs: Arc<[TextGlyph]>,
}

impl CachedRun {
    fn from_run(run: TextRun) -> Self {
        Self {
            path: run.path,
            advance: run.advance,
            ascent: run.ascent,
            descent: run.descent,
            line_height: run.line_height,
            glyphs: run
                .glyphs
                .into_iter()
                .map(|g| TextGlyph {
                    id: g.id,
                    x: g.x as f32,
                    y: g.y as f32,
                    font: g.font,
                })
                .collect(),
        }
    }
}

/// Per-face normalized coordinates, primary face first. What a run is keyed
/// on: two axis settings that round to the same coordinates share a run.
type Coords = Arc<[Arc<[i16]>]>;
/// (size bits, primary face id, coordinates).
type RunKey = (u64, u64, Coords);
/// A run's key plus the width it broke at and its line cap.
type BreakKey = (RunKey, u64, Option<usize>);
/// A paragraph's line byte ranges, and whether the cap cut lines off.
type Breaks = (Vec<std::ops::Range<usize>>, bool);
/// Per string, per variant: the value and the resolve that last used it.
type PerText<K, V> = HashMap<String, HashMap<K, (V, u64)>>;
/// Normalized coordinates per axes, then per (size bits, primary face id).
type CoordsCache = HashMap<Axes, HashMap<(u64, Option<u64>), (Coords, u64)>>;

/// Everything shaped, broken and bent across frames. Own one in your runtime
/// and pass it to [`resolve_scene_with`]. Nothing is flushed wholesale: an
/// entry the last resolve did not use is dropped at its end, so memory tracks
/// the live tree and a steady frame reshapes nothing.
#[derive(Debug, Default)]
pub struct TextCache {
    layout: mui_layout::LayoutCache,
    /// The ids of the spec's faces the cache was filled with.
    fonts: Vec<u64>,
    tolerance_bits: u64,
    generation: u64,
    runs: PerText<RunKey, CachedRun>,
    /// Wrapped paragraphs' line breaks, keyed like `runs`.
    breaks: PerText<BreakKey, Breaks>,
    /// Axis settings already normalized, so a frame does not re-derive the
    /// same coordinates per measure call. Keyed by axes first so a lookup
    /// borrows them.
    coords: CoordsCache,
    /// The coordinates each text key drew with last frame, for `Text::hint`.
    last_coords: HashMap<Arc<str>, (Coords, u64)>,
    outlines: OutlineCache,
    borders: crate::border_ramp::BorderCache,
    region_cache: crate::regions::RegionCache,
}
impl TextCache {
    pub fn layout_stats(&self) -> mui_layout::LayoutStats {
        self.layout.stats()
    }
    /// Shaped runs held: one per (string, size, face, axes) variant.
    pub fn len(&self) -> usize {
        self.runs.values().map(HashMap::len).sum()
    }
    pub fn is_empty(&self) -> bool {
        // An inner map is never left empty, so no strings means no runs.
        self.runs.is_empty()
    }
    /// End of a resolve: keep exactly what it used.
    fn sweep(&mut self) {
        fn keep<K, V>(m: &mut HashMap<K, (V, u64)>, g: u64) -> bool {
            m.retain(|_, (_, seen)| *seen == g);
            !m.is_empty()
        }
        let g = self.generation;
        self.runs.retain(|_, m| keep(m, g));
        self.breaks.retain(|_, m| keep(m, g));
        self.coords.retain(|_, m| keep(m, g));
        keep(&mut self.last_coords, g);
        self.generation = g.wrapping_add(1);
        self.outlines.sweep();
        self.borders.sweep();
        self.region_cache.sweep();
    }
}

/// What one text node is measured and shaped with.
#[derive(Clone, Copy)]
struct Face<'a> {
    size: f64,
    axes: &'a Axes,
    /// The node's own face, if it set one; tried before the scene's fonts.
    font: Option<&'a Font>,
}
impl<'a> Face<'a> {
    fn of(e: &'a crate::Element, th: Theme) -> Self {
        Self {
            size: e.text_size.unwrap_or(th.text),
            axes: &e.axes,
            font: e.font.as_ref(),
        }
    }
}

struct Runs<'a> {
    /// The scene's faces, shared into every `Text` that has no face of its own.
    fonts: Arc<[Font]>,
    /// The last node face's chain, so a column of icons builds it once.
    own_fonts: Option<(u64, Arc<[Font]>)>,
    tolerance: f64,
    generation: u64,
    cache: &'a mut PerText<RunKey, CachedRun>,
    breaks: &'a mut PerText<BreakKey, Breaks>,
    coords: &'a mut CoordsCache,
    last_coords: &'a mut HashMap<Arc<str>, (Coords, u64)>,
}
impl<'a> Runs<'a> {
    /// The faces a node shapes with: its own first, then the scene's.
    fn fonts_for(&mut self, face: Face<'_>) -> Arc<[Font]> {
        let Some(own) = face.font else {
            return self.fonts.clone();
        };
        if let Some((id, fonts)) = &self.own_fonts {
            if *id == own.id() {
                return fonts.clone();
            }
        }
        let fonts: Arc<[Font]> = std::iter::once(own).chain(&*self.fonts).cloned().collect();
        self.own_fonts = Some((own.id(), fonts.clone()));
        fonts
    }
    /// Per-face coordinates for `face`, memoised per (size, font, axes).
    fn coords(&mut self, fonts: &[Font], face: Face<'_>) -> Coords {
        let key = (face.size.to_bits(), fonts.first().map(Font::id));
        let generation = self.generation;
        if let Some((c, seen)) = self.coords.get_mut(face.axes).and_then(|m| m.get_mut(&key)) {
            *seen = generation;
            return c.clone();
        }
        let settings = face.axes.to_vec();
        let coords: Coords = fonts
            .iter()
            .map(|f| {
                mui_text::normalized_coords(f, face.size, &settings)
                    .map_or_else(|_| Arc::from(&[][..]), Arc::from)
            })
            .collect::<Vec<_>>()
            .into();
        self.coords
            .entry(face.axes.clone())
            .or_default()
            .insert(key, (coords.clone(), generation));
        coords
    }
    /// Whether `key` drew at these coordinates last frame too. False only
    /// for the frame after an axis moved, which is what turns hinting off
    /// mid-tween; text seen for the first time counts as settled.
    fn settled(&mut self, key: &Arc<str>, coords: &Coords) -> bool {
        match self
            .last_coords
            .insert(key.clone(), (coords.clone(), self.generation))
        {
            Some((last, _)) => last == *coords,
            None => true,
        }
    }
    fn run(&mut self, text: &str, face: Face<'_>) -> Result<Option<&CachedRun>, mui_text::Error> {
        let fonts = self.fonts_for(face);
        let Some(font) = fonts.first() else {
            return Ok(None);
        };
        let coords = self.coords(&fonts, face);
        // Nested so a hit borrows `text` instead of allocating a key for it:
        // `run` is called several times per line, per frame.
        let key: RunKey = (face.size.to_bits(), font.id(), coords);
        let generation = self.generation;
        if self.cache.get(text).is_none_or(|m| !m.contains_key(&key)) {
            let settings = face.axes.to_vec();
            let run = CachedRun::from_run(mui_text::text_run(
                &fonts,
                text,
                face.size,
                &settings,
                self.tolerance,
            )?);
            self.cache
                .entry(text.to_owned())
                .or_default()
                .insert(key.clone(), (run, generation));
        }
        Ok(self
            .cache
            .get_mut(text)
            .and_then(|m| m.get_mut(&key))
            .map(|(run, seen)| {
                *seen = generation;
                &*run
            }))
    }
    /// The lines `text` breaks into at `max` width, capped at `cap` of them
    /// with an ellipsis on the last. One line when it fits, or when there is
    /// no font to break against.
    ///
    /// Every line is a slice of `text`, so the overwhelmingly common case --
    /// a label that fits -- allocates the `Vec` and nothing else. Only an
    /// ellipsised last line owns its bytes. The breaks are cached like runs:
    /// a steady paragraph is broken once, not once per frame.
    fn lines<'t>(
        &mut self,
        text: &'t str,
        face: Face<'_>,
        max: f64,
        cap: Option<usize>,
    ) -> Vec<Cow<'t, str>> {
        let one = || vec![Cow::Borrowed(text)];
        if max <= 0.0 || self.measure(text, face).width <= max + 0.5 {
            return one();
        }
        let fonts = self.fonts_for(face);
        let Some(font) = fonts.first() else {
            return one();
        };
        let key: BreakKey = (
            (face.size.to_bits(), font.id(), self.coords(&fonts, face)),
            max.to_bits(),
            cap,
        );
        let generation = self.generation;
        if self.breaks.get(text).is_none_or(|m| !m.contains_key(&key)) {
            let settings = face.axes.to_vec();
            let Ok(lines) = mui_text::break_lines(&fonts, text, face.size, &settings, max) else {
                return one();
            };
            let n = cap.unwrap_or(usize::MAX).max(1);
            let ranges = lines.iter().take(n).map(|l| l.text_range.clone()).collect();
            self.breaks
                .entry(text.to_owned())
                .or_default()
                .insert(key.clone(), ((ranges, lines.len() > n), generation));
        }
        let Some(((ranges, cut), seen)) = self.breaks.get_mut(text).and_then(|m| m.get_mut(&key))
        else {
            return one();
        };
        *seen = generation;
        let mut out: Vec<Cow<'t, str>> = ranges
            .iter()
            .map(|r| Cow::Borrowed(text[r.clone()].trim_end()))
            .collect();
        if *cut {
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
    fn wrapped(&mut self, text: &str, face: Face<'_>, max: f64, cap: Option<usize>) -> Size {
        let (mut w, mut h) = (0.0f64, 0.0);
        for l in self.lines(text, face, max, cap) {
            let s = self.measure(&l, face);
            w = w.max(s.width);
            h += s.height;
        }
        Size::new(w, h)
    }
    fn measure(&mut self, text: &str, face: Face<'_>) -> Size {
        let size = face.size;
        match self.run(text, face) {
            // ponytail: no font → a monospace guess, so layout tests stay
            // font-free. Wrong widths are visible the moment a font is set.
            Ok(None) | Err(_) => Size::new(text.chars().count() as f64 * size * 0.6, size * 1.25),
            Ok(Some(r)) => Size::new(r.advance, r.line_height),
        }
    }
}

/// A float, painted after the whole tree so it sits on top and escapes
/// every clip.
#[derive(Clone, Default)]
struct Ancestors {
    parent: Option<Arc<str>>,
    clip: Option<Bounds>,
    clip_paths: Option<Arc<[Path]>>,
}

#[derive(Clone)]
struct Deferred<'a> {
    at: usize,
    node: &'a El,
    path: String,
    under: Color,
    cursor: Option<Cursor>,
    disabled: bool,
    parent: Option<Arc<str>>,
}

struct Walk<'a> {
    spec: &'a SceneSpec,
    frames: Cow<'a, [Frame]>,
    regions: HashMap<usize, Path>,
    region_envelopes: HashMap<usize, Path>,
    runs: Runs<'a>,
    /// Subtree size per pre-order index; see [`subtree_sizes`].
    sizes: Vec<usize>,
    outlines: &'a mut OutlineCache,
    borders: &'a mut crate::border_ramp::BorderCache,
    region_cache: &'a mut crate::regions::RegionCache,
    ramp_anchors: HashMap<usize, Frame>,
    ramp_frames: HashMap<(usize, mui_layout::Id), Frame>,
    weld_cache: &'a mut crate::WeldCache,
    i: usize,
    key: Arc<str>,
    paint: Vec<Painted>,
    surfaces: Vec<ResolvedSurface>,
    at: HashMap<Arc<str>, usize>,
    pub(crate) external_welds: HashMap<Arc<str>, crate::ExternalWeld>,
    deferred: Vec<Deferred<'a>>,
    /// The baseline a `.baseline()` parent asks its text children to sit on.
    base_y: Option<f64>,
}
impl<'a> Walk<'a> {
    fn partition(
        &mut self,
        n: &El,
        outline: &Path,
        frame: Frame,
        at: usize,
    ) -> Result<(), SceneError> {
        let e = n.payload();
        let Some(padding) = e.inside else {
            if e.bend != 0. {
                return Err(mui_geometry::Error::InvalidOptions("bend requires inside").into());
            }
            return Ok(());
        };
        let padding = padding.resolve(self.spec.theme.spacing);
        if !padding.is_finite() || padding < 0. || !e.bend.is_finite() || e.bend.abs() > 0.45 {
            return Err(mui_geometry::Error::InvalidOptions("shape padding/bend").into());
        }
        if e.welding.is_some() {
            return Err(SceneError::UnsupportedWeld(
                "inside requires vector welding",
            ));
        }
        let mut interior = outline.clone();
        if let Some(ramp) = &e.border_ramp {
            ramp.validate()?;
            let anchor = match &ramp.anchor {
                None => frame,
                Some(id) => {
                    self.frames[find(n, id.as_str(), at, &self.sizes).ok_or(
                        mui_geometry::Error::InvalidOptions("border ramp descendant missing"),
                    )?]
                }
            };
            if anchor.size.width <= 0. {
                return Err(
                    mui_geometry::Error::InvalidOptions("border ramp anchor has no width").into(),
                );
            }
            let anchor = *self.ramp_anchors.entry(at).or_insert(anchor);
            for id in ramp.tabs.iter().chain(&ramp.dividers) {
                let index = find(n, id.as_str(), at, &self.sizes).ok_or(
                    mui_geometry::Error::InvalidOptions("border ramp descendant missing"),
                )?;
                self.ramp_frames
                    .insert((at, id.clone()), self.frames[index]);
            }
            let decoration = ramp.clone();
            let mut ramp = ramp.clone();
            ramp.from.1 *= ramp.align.inward();
            ramp.to.1 *= ramp.align.inward();
            if ramp.from.1.max(ramp.to.1) > 0. {
                let mut band = self.borders.band(
                    &format!("inside/{at}"),
                    outline,
                    &ramp,
                    anchor,
                    0.1 / self.spec.device_scale.unwrap_or(1.),
                )?;
                let shoulder = match e.style.radius {
                    Radius::Pair(_, r) => r,
                    Radius::Scale(k) => self.spec.theme.corners.concave * k,
                    _ => self.spec.theme.corners.concave,
                };
                crate::border_ramp::decorate(&mut band, &decoration, anchor, shoulder, |id| {
                    Ok(self.ramp_frames[&(at, id.clone())])
                })?;
                let band = self.region_cache.resolve(
                    (at, 0),
                    Operation::Sweep(band),
                    self.spec.offsets,
                    self.spec.geometry,
                )?;
                interior = self.region_cache.resolve(
                    (at, 1),
                    Operation::Combine(interior, band, BooleanOp::Difference),
                    self.spec.offsets,
                    self.spec.geometry,
                )?;
            }
        } else if let Some(stroke) = &e.style.stroke {
            let width = stroke.width.unwrap_or(self.spec.theme.stroke_width);
            if !width.is_finite() || width < 0. {
                return Err(SceneError::InvalidRadius);
            }
            interior = self.region_cache.resolve(
                (at, 2),
                Operation::Inset(interior, width * e.border_align.inward()),
                self.spec.offsets,
                self.spec.geometry,
            )?;
        }
        interior = self.region_cache.resolve(
            (at, 3),
            Operation::Inset(interior, padding),
            self.spec.offsets,
            self.spec.geometry,
        )?;
        let Some(b) = Bounds::from_points(
            interior
                .flatten(
                    self.spec.offsets.flatten_tolerance,
                    self.spec.offsets.max_points,
                )?
                .concat(),
        ) else {
            let end = at + self.sizes[at];
            for f in &mut self.frames.to_mut()[at + 1..end] {
                f.size = Size::ZERO;
            }
            return Ok(());
        };
        let size = Size::new(b.max.x - b.min.x, b.max.y - b.min.y);
        let root = n.clone().size(size.width, size.height).pad(0.);
        let th = self.spec.theme;
        let layout = mui_layout::resolve_with(
            &root,
            Some(size),
            self.spec.limits,
            th.spacing,
            |e, room| fit(&mut self.runs, th, e, room),
        )?;
        let end = at + self.sizes[at];
        for (dest, f) in self.frames.to_mut()[at + 1..end]
            .iter_mut()
            .zip(&layout.all()[1..])
        {
            *dest = Frame {
                x: f.x + b.min.x,
                y: f.y + b.min.y,
                size: f.size,
            };
        }
        let mut next = at + 1;
        let children: Vec<_> = n
            .children()
            .iter()
            .filter_map(|c| {
                let i = next;
                next += self.sizes[i];
                (!c.is_float() && c.payload().carve.is_none()).then_some((i, c))
            })
            .collect();
        let mut masks: Vec<Path> = children
            .iter()
            .map(|(i, _)| {
                if self.frames[*i].size.width <= 0. || self.frames[*i].size.height <= 0. {
                    Ok(Path::default())
                } else {
                    RoundedRect::new(bounds(self.frames[*i], None), 0.).map(|r| r.path())
                }
            })
            .collect::<Result<_, _>>()?;
        if e.bend != 0. {
            if children.len() != 2 {
                return Err(
                    mui_geometry::Error::InvalidOptions("bend requires two siblings").into(),
                );
            }
            let a = self.frames[children[0].0];
            let z = self.frames[children[1].0];
            let horizontal = z.x >= a.right() - 1e-6;
            if !horizontal && z.y < a.bottom() - 1e-6 {
                return Err(
                    mui_geometry::Error::InvalidOptions("bend requires a row or column").into(),
                );
            }
            let (axis, cut, gap) = if horizontal {
                (
                    mui_geometry::SplitAxis::X,
                    ((a.right() + z.x) * 0.5 - b.min.x) / size.width,
                    z.x - a.right(),
                )
            } else {
                (
                    mui_geometry::SplitAxis::Y,
                    ((a.bottom() + z.y) * 0.5 - b.min.y) / size.height,
                    z.y - a.bottom(),
                )
            };
            let split = mui_geometry::ShapeSplit::new(axis, cut)
                .gap(gap.max(0.))
                .bend(e.bend);
            for (side, mask) in masks.iter_mut().enumerate() {
                *mask = self.region_cache.resolve(
                    (at, 4 + side as u8),
                    Operation::SplitMask(b, split, side == 1),
                    self.spec.offsets,
                    self.spec.geometry,
                )?;
            }
        }

        for ((i, child), mask) in children.into_iter().zip(masks) {
            let path = self.region_cache.resolve(
                (i, 6),
                Operation::Combine(interior.clone(), mask, BooleanOp::Intersection),
                self.spec.offsets,
                self.spec.geometry,
            )?;
            // A child's outward border belongs inside its allocation too. Reserve
            // it before fitting the child, and retain the allocation as a paint cap.
            self.region_envelopes.insert(i, path.clone());
            let mut path = path;
            if let Some(ramp) = &child.payload().border_ramp {
                ramp.validate()?;
                let outward = 1. - ramp.align.inward();
                if outward > 0. && ramp.from.1.max(ramp.to.1) > 0. {
                    if ramp
                        .anchor
                        .as_ref()
                        .is_some_and(|id| child.key() != Some(id.as_str()))
                    {
                        return Err(mui_geometry::Error::InvalidOptions(
                            "outward region border anchor must be the child",
                        )
                        .into());
                    }
                    let anchor = self.frames[i];
                    self.ramp_anchors.insert(i, anchor);
                    let mut sweep = ramp.clone();
                    sweep.from.1 *= outward;
                    sweep.to.1 *= outward;
                    let band = self.borders.band(
                        &format!("outside/{i}"),
                        &path,
                        &sweep,
                        anchor,
                        0.1 / self.spec.device_scale.unwrap_or(1.),
                    )?;
                    let band = self.region_cache.resolve(
                        (i, 8),
                        Operation::Sweep(band),
                        self.spec.offsets,
                        self.spec.geometry,
                    )?;
                    path = self.region_cache.resolve(
                        (i, 9),
                        Operation::Combine(path, band, BooleanOp::Difference),
                        self.spec.offsets,
                        self.spec.geometry,
                    )?;
                }
            } else if let Some(stroke) = &child.payload().style.stroke {
                let width = stroke.width.unwrap_or(self.spec.theme.stroke_width);
                if !width.is_finite() || width < 0. {
                    return Err(SceneError::InvalidRadius);
                }
                let outward = width * (1. - child.payload().border_align.inward());
                if outward > 0. {
                    path = self.region_cache.resolve(
                        (i, 9),
                        Operation::Inset(path, outward),
                        self.spec.offsets,
                        self.spec.geometry,
                    )?;
                }
            }
            if let Some(b) = Bounds::from_points(
                path.flatten(
                    self.spec.offsets.flatten_tolerance,
                    self.spec.offsets.max_points,
                )?
                .concat(),
            ) {
                let size = Size::new(b.max.x - b.min.x, b.max.y - b.min.y);
                let root = child.clone().size(size.width, size.height);
                let layout = mui_layout::resolve_with(
                    &root,
                    Some(size),
                    self.spec.limits,
                    th.spacing,
                    |e, room| fit(&mut self.runs, th, e, room),
                )?;
                let end = i + self.sizes[i];
                for (dest, f) in self.frames.to_mut()[i..end].iter_mut().zip(layout.all()) {
                    *dest = Frame {
                        x: f.x + b.min.x,
                        y: f.y + b.min.y,
                        size: f.size,
                    };
                }
            } else {
                let end = i + self.sizes[i];
                for f in &mut self.frames.to_mut()[i..end] {
                    f.size = Size::ZERO;
                }
            }
            self.regions.insert(i, path);
        }
        Ok(())
    }

    /// The node's own shape, with every [`Carve`] child taken out of it (or
    /// intersected with it). A carved outline is a path like a welded one:
    /// no analytic rect, so shells, strokes and clips all follow the result.
    ///
    /// `first` is the pre-order index of the node's first child. Weld and
    /// carve both inspect descendants while the walk is still at the parent;
    /// the explicit index gives them the child's own radius, corner style
    /// and nested topology instead of a sharp frame rectangle.
    fn outline(&mut self, n: &El, frame: Frame, first: usize) -> Result<OutlineResult, SceneError> {
        if let Some(path) = self.regions.get(&first.saturating_sub(1)) {
            return Ok((path.clone(), None, false, Vec::new()));
        }
        if n.payload().outline.is_some() && n.children().iter().any(|c| c.payload().carve.is_some())
        {
            return Err(mui_geometry::Error::InvalidOptions(
                "cut/keep on a custom outline requires contour normalization; provide the finished outline instead",
            )
            .into());
        }
        let cacheable = n.payload().style.weld
            || n.children()
                .iter()
                .any(|child| child.payload().carve.is_some());
        let mut key = None;
        if cacheable {
            let mut words = std::mem::take(&mut self.outlines.scratch);
            if self.geometry_key(n, first, &mut words) {
                if let Some(outline) = self.outlines.get(&words) {
                    self.outlines.scratch = words;
                    return Ok(outline);
                }
                key = Some(words);
            } else {
                self.outlines.scratch = words;
            }
        }
        let base = self.shape(n, frame, first)?;
        let (mut at, mut topo): (usize, Option<Topology>) = (first, None);
        let mut shapes = Vec::new();
        for c in n.children() {
            let (f, carve) = (self.frames[at], c.payload().carve);
            let child_first = at + 1;
            at += self.sizes[at];
            let Some(carve) = carve.filter(|_| f.size.width > 0.0 && f.size.height > 0.0) else {
                continue;
            };
            if topo.is_none() {
                shapes = polygons(&base.0)?;
            }
            let rhs = polygons(&self.outline(c, f, child_first)?.0)?;
            if rhs.is_empty() {
                continue;
            }
            let op = match carve {
                Carve::Cut => BooleanOp::Difference,
                Carve::Keep => BooleanOp::Intersection,
            };
            let t = boolean(&shapes, &rhs, op, self.spec.geometry)?;
            shapes = t.placed_shapes();
            topo = Some(t);
        }
        let Some(topo) = topo else {
            if let Some(key) = key {
                self.outlines.insert(key, base.clone());
            }
            return Ok(base);
        };
        // Radius 0: the shapes going in already carry their own rounding,
        // and a second fillet would eat the corners the carve just made.
        let rounded = fillet(
            &topo,
            Fillet {
                convex_radius: 0.,
                concave_radius: 0.,
                ..Fillet::default()
            },
        )?;
        let outline = (
            n.payload().style.corners.shape(&rounded.path),
            None,
            true,
            Vec::new(),
        );
        if let Some(key) = key {
            self.outlines.insert(key, outline.clone());
        }
        Ok(outline)
    }

    /// Every input `n`'s outline depends on, as words compared in full.
    /// `false` when a custom outline is involved; see [`geometry_shallow`].
    fn geometry_key(&self, n: &El, first: usize, key: &mut Vec<u64>) -> bool {
        key.clear();
        key.push(first as u64);
        for value in [
            self.spec.theme.corners.selector,
            self.spec.theme.corners.field,
            self.spec.theme.corners.box_,
            self.spec.theme.corners.concave,
            self.spec.geometry.epsilon,
            self.spec.geometry.coordinate_limit,
        ] {
            key.push(value.to_bits());
        }
        key.push(self.spec.geometry.max_vertices as u64);
        match self.spec.device_scale {
            None => key.push(0),
            Some(scale) => key.extend([1, scale.to_bits()]),
        }
        geometry_node(n, &self.frames, &self.sizes, first.saturating_sub(1), key)
    }

    fn shape(&mut self, n: &El, frame: Frame, first: usize) -> Result<OutlineResult, SceneError> {
        let th = &self.spec.theme;
        let s = &n.payload().style;
        if let Some(shape) = &n.payload().outline {
            if !s.shadow.is_empty() {
                return Err(mui_geometry::Error::InvalidOptions(
                    "custom-path shadows require a path-filter renderer; use an outer wrapper",
                )
                .into());
            }
            let local = (shape.0)(frame.size);
            local.validate(250_000)?;
            let world = local.rigid_transform(Point::new(frame.x, frame.y), 0.0)?;
            world.validate(250_000)?;
            return Ok((world, None, false, Vec::new()));
        }
        let (convex, concave) = match s.radius {
            Radius::Theme => (th.corners.box_, th.corners.concave),
            // A pixel radius names the outer (convex) corner. The inner
            // (concave) corner remains the theme contract; a pair such as
            // `(20., 14.)` is two radii, never an elliptical radius.
            Radius::Px(r) => (r, th.corners.concave),
            Radius::Pair(convex, concave) => (convex, concave),
            Radius::Token(c) => (th.corners.get(c), th.corners.concave),
            Radius::Scale(k) => {
                let p = th.corners.scaled(k).ok_or(SceneError::InvalidRadius)?;
                (p.box_, p.concave)
            }
            Radius::Pill => (
                frame.size.width.min(frame.size.height) / 2.0,
                th.corners.concave,
            ),
        };
        if !(convex.is_finite() && convex >= 0.0 && concave.is_finite() && concave >= 0.0) {
            return Err(SceneError::InvalidRadius);
        }
        if !s.weld || n.children().is_empty() {
            let rr = RoundedRect::new(bounds(frame, self.spec.device_scale), convex)?;
            // A squircle is no longer a rounded rectangle, so it gives up the
            // analytic blur and the analytic shell inset with it; the path
            // route below draws both from the outline itself.
            if s.corners != CornerStyle::Round {
                return Ok((s.corners.shape(&rr.path()), None, false, vec![rr]));
            }
            return Ok((rr.path(), Some(rr), false, Vec::new()));
        }
        // Children's outlines sit right after this node in pre-order, each
        // subtree `count` long. Carved children shape this node separately;
        // zero-area children have no paint or geometry and must not turn a
        // valid weld into a DegenerateRing error.
        let (mut at, mut shapes, mut rects) = (first, Vec::new(), Vec::new());
        let mut participants = 0;
        for c in n.children() {
            let child_at = at;
            let child_first = child_at + 1;
            let f = self.frames[child_at];
            at += self.sizes[at];
            if c.payload().carve.is_some() || f.size.width <= 0.0 || f.size.height <= 0.0 {
                continue;
            }
            let (path, child_rect, ..) = self.outline(c, f, child_first)?;
            let child_shapes = polygons(&path)?;
            if child_shapes.is_empty() {
                continue;
            }
            shapes.extend(child_shapes);
            // Keep the child's analytic radius for the optional shadow fast
            // path. A squircle or nested weld has no analytic rect and falls
            // back to the frame with the weld's convex radius.
            rects.push(match child_rect {
                Some(r) => r,
                None => RoundedRect::new(bounds(f, self.spec.device_scale), convex)?,
            });
            participants += 1;
        }
        if shapes.is_empty() {
            return Ok((Path::default(), None, false, rects));
        }
        let merged = union(&shapes, self.spec.geometry)?;
        let rounded = fillet(
            &merged,
            Fillet {
                convex_radius: convex,
                concave_radius: concave,
                ..Fillet::default()
            },
        )?;
        Ok((
            s.corners.shape(&rounded.path),
            None,
            merged.components() != participants,
            rects,
        ))
    }

    /// Bake the plates in group-local coordinates, before clips and the parent
    /// hit surface are emitted. Descendant content and identities are untouched.
    fn material_weld(
        &mut self,
        n: &El,
        frame: Frame,
        path: &str,
        under: Color,
    ) -> Result<Option<crate::material_weld::MaterialWeld>, SceneError> {
        let Some(weld) = n.payload().welding else {
            return Ok(None);
        };
        let gpu = n.payload().weld_backend.unwrap_or(self.spec.weld_backend)
            == crate::WeldBackend::AnalyticGpu;
        if gpu && n.is_clip() {
            return Err(SceneError::UnsupportedWeld(
                "GPU weld cannot itself clip children to its changing union; put a normal clipping viewport above it",
            ));
        }
        crate::material_weld::check_plate(n, false)?;
        if n.payload().outline.is_some() || n.payload().style.weld {
            return Err(SceneError::UnsupportedWeld(
                "custom/legacy-weld outline on the group; put it on a source child",
            ));
        }
        let mut quality = n.payload().weld_quality.unwrap_or_default();
        if let Some(scale) = self.spec.device_scale {
            quality.scale = scale;
        }
        if !quality.scale.is_finite() || !(0.125..=8.0).contains(&quality.scale) {
            return Err(mui_weld::Error::Invalid("weld raster scale").into());
        }
        // A snapped local origin keeps the baked image on the device grid even
        // when layout has a fractional origin. Layout itself remains untouched.
        let origin = Point::new(
            snap(frame.x, Some(quality.scale)),
            snap(frame.y, Some(quality.scale)),
        );
        let th = self.spec.theme;
        let parent = &n.payload().style;
        let mut sources = Vec::new();
        let mut members = std::collections::HashSet::new();
        let mut at = self.i;
        for (j, c) in n.children().iter().enumerate() {
            let first = at;
            let f = self.frames[first];
            at += self.sizes[first];
            let e = c.payload();
            if c.is_float()
                || c.is_sticky()
                || e.carve.is_some()
                || e.weld_excluded
                || matches!(&e.content, Content::Text(_))
                || f.size.width <= 0.0
                || f.size.height <= 0.0
            {
                continue;
            }
            // Empty spacers affect layout, never material. A group fill can
            // explicitly give otherwise unpainted child outlines material.
            if parent.fill.is_none()
                && parent.stroke.is_none()
                && e.style.fill.is_none()
                && e.style.stroke.is_none()
            {
                continue;
            }
            crate::material_weld::check_plate(c, true)?;
            if gpu
                && (sources.len() >= mui_weld::analytic::ANALYTIC_SOURCES
                    || e.outline.is_some()
                    || e.style.weld
                    || e.style.corners != CornerStyle::Round
                    || c.children()
                        .iter()
                        .any(|child| child.payload().carve.is_some()))
            {
                return Err(SceneError::UnsupportedWeld(
                    "analytic GPU weld requires at most three ordinary rounded-rectangle plates; custom/carved/nested outlines need an explicit reference backend",
                ));
            }
            let (outline, rect, _, _) = self.outline(c, f, first + 1)?;
            if gpu && rect.is_none() {
                return Err(SceneError::UnsupportedWeld(
                    "GPU participant is not an analytic rounded rectangle",
                ));
            }
            let fill = if parent.fill.is_none() {
                &e.style.fill
            } else {
                &parent.fill
            };
            let fill_paint = fill.paint(&th.palette, under);
            let ground = fill_paint.as_ref().map_or(under, Paint::solid);
            let stroke = parent.stroke.as_ref().or(e.style.stroke.as_ref());
            let border = stroke.and_then(|s| s.fill.paint(&th.palette, ground));
            let width = stroke.map_or(0.0, |s| s.width.unwrap_or(th.stroke_width));
            sources.push(crate::material_weld::source(
                crate::material_weld::Plate {
                    outline,
                    rect,
                    frame: f,
                    fill: fill_paint,
                    border,
                    width,
                },
                origin,
                0.25 / quality.scale,
            )?);
            members.insert(
                c.key()
                    .map_or_else(|| Arc::from(format!("{path}/{j}")), Arc::from),
            );
        }
        if sources.is_empty() {
            return Err(SceneError::UnsupportedWeld(
                "no painted, non-excluded plate children",
            ));
        }
        if gpu {
            return Ok(Some(crate::external::finish_gpu(
                &sources,
                weld,
                quality,
                origin,
                members,
                self.weld_cache,
            )?));
        }
        let request = mui_weld::Request {
            sources,
            weld,
            quality,
        };
        let baked = self.weld_cache.get(&request)?;
        Ok(Some(crate::material_weld::finish(&baked, origin, members)?))
    }

    /// One shadow of a node, offset and spread off the node's own outline.
    ///
    /// A rounded rect blurs analytically, so that is what the entry carries
    /// whenever the outline is one. A welded outline is not, and becomes one
    /// blurred rect per welded child instead.
    fn shadow(
        &mut self,
        sh: &Shadow,
        outline: &Path,
        rect: Option<RoundedRect>,
        welds: &[RoundedRect],
        under: Color,
    ) -> Result<(), SceneError> {
        let d = Point::new(sh.dx, sh.dy);
        // CSS spread: the drop grows, the inset shrinks, and the radius
        // follows so the corner keeps its shape.
        let grow = match sh.kind {
            ShadowKind::Drop => sh.spread,
            ShadowKind::Inset => -sh.spread,
        };
        let moved = |r: RoundedRect| {
            let b = r.bounds();
            RoundedRect::new(
                Bounds::new(
                    b.min.x + d.x - grow,
                    b.min.y + d.y - grow,
                    b.max.x + d.x + grow,
                    b.max.y + d.y + grow,
                ),
                (r.radius() + grow).max(0.0),
            )
        };
        // ponytail: a welded shadow is the union of the children's blurs,
        // not the blur of the union -- each child rect keeps the convex
        // radius, so the seams are rounded where the welded outline is
        // straight or concave, and overlapping children over-composite
        // there. A blur filter layer is the upgrade.
        let rects: Vec<RoundedRect> = match rect {
            Some(r) => vec![r],
            None if !welds.is_empty() => welds.to_vec(),
            // ponytail: no analytic rect and no welds -- the shape travels
            // as a path, and the spread with it is dropped.
            None => {
                if let Some(p) = self.push(
                    Layer::Shadow(sh.kind),
                    outline.rigid_transform(d, 0.0)?,
                    None,
                    &sh.fill,
                    under,
                ) {
                    p.blur = sh.blur;
                }
                return Ok(());
            }
        };
        for r in rects {
            let r = moved(r)?;
            if let Some(p) = self.push(Layer::Shadow(sh.kind), r.path(), Some(r), &sh.fill, under) {
                p.blur = sh.blur;
            }
        }
        Ok(())
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
        path: &mut String,
        under: Color,
        cursor: Option<Cursor>,
        ancestors: &Ancestors,
        disabled: bool,
    ) -> Result<(), SceneError> {
        let clip = ancestors.clip;
        let frame = self.frames[self.i];
        let at = self.i;
        self.i += 1;
        // ponytail: one `Arc<str>` per node per frame, cloned four times
        // instead of four heap copies; interning across frames is the upgrade.
        let key: Arc<str> = n.key().map_or_else(|| Arc::from(path.as_str()), Arc::from);
        let th = self.spec.theme;
        let e = n.payload();
        let s = &e.style;
        let cursor = s.cursor.or(cursor);
        // A switched-off card switches off what it contains: nothing inside
        // it may be reached while its own frame cannot be.
        let disabled = disabled || e.disabled;
        if frame.size.width <= 0.0 || frame.size.height <= 0.0 {
            // A flex share that collapsed to nothing: invisible, and so are
            // its children.
            self.i = at + self.sizes[at];
            return Ok(());
        }
        let start = self.paint.len();
        let material = self.material_weld(n, frame, path, under)?;
        let (outline, rect, mut changed, welds) = match &material {
            Some(m) => (m.outline.clone(), None, true, Vec::new()),
            None => self.outline(n, frame, self.i)?,
        };

        self.partition(n, &outline, frame, at)?;
        self.key = key.clone();
        let clear = Fill::Color(Color::oklcha(0.0, 0.0, 0.0, 0.0));
        // Each envelope belongs to exactly one node, visited once.
        let enveloped = self.region_envelopes.remove(&at).map(|p| {
            self.push(Layer::Clip, p, None, &clear, under);
        });
        // A mask composites against what the subtree drew, so the subtree
        // needs a layer of its own even when nothing asked to blend.
        let masked = !s.mask.is_none();
        let blended = match s.layer {
            Some((mix, opacity)) if !(mix == Mix::Normal && opacity == 1.0) => Some((mix, opacity)),
            _ if masked => Some((Mix::Normal, 1.0)),
            _ => None,
        };
        if let Some((mix, opacity)) = blended {
            self.push(
                Layer::Blend { mix, opacity },
                Path::default(),
                None,
                &clear,
                under,
            );
        }
        for sh in s.shadow.iter().filter(|sh| sh.kind == ShadowKind::Drop) {
            self.shadow(sh, &outline, rect, &welds, under)?;
        }
        let solid = |p: Option<&mut Painted>, or: Color| p.map_or(or, |p| p.paint.solid());
        let mut bg = match &material {
            Some(m) if m.external.is_some() => {
                let external = m.external.as_ref().expect("matched Some").clone();
                self.external_welds.insert(self.key.clone(), external);
                self.paint.push(Painted {
                    key: self.key.clone(),
                    layer: Layer::External,
                    path: m.image_rect.path(),
                    paint: Paint::Solid(under),
                    rect: Some(m.image_rect),
                    width: 0.0,
                    blur: 0.0,
                    text: None,
                });
                under
            }
            Some(m) => solid(
                self.push(
                    Layer::Fill,
                    m.image_rect.path(),
                    Some(m.image_rect),
                    &m.image_fill,
                    under,
                ),
                under,
            ),
            // Text's own fill is its ink, not a box behind it: never pushed,
            // though its colour still grounds the shells as before.
            None if matches!(e.content, Content::Text(_)) => s
                .fill
                .paint(&th.palette, under)
                .map_or(under, |p| p.solid()),
            // ponytail: `Painted` owns its path, so a filled node copies its
            // outline once. `Arc<Path>` there is the upgrade -- an API break
            // for every renderer.
            None => solid(
                self.push(Layer::Fill, outline.clone(), rect, &s.fill, under),
                under,
            ),
        };

        let border_background = self.paint.len();
        // The shell before this one: an analytic rect insets as one, and
        // only a path shell needs the previous path kept.
        let (mut cur, mut cur_rect) = (None::<Path>, rect);
        for (i, (d, f)) in s.shells.iter().enumerate() {
            let d = d.resolve(th.spacing);
            if !(d.is_finite() && d >= 0.0) {
                return Err(SceneError::InvalidRadius);
            }
            let shell = match cur_rect {
                Some(rr) => {
                    let i2 = rr.inset(d)?;
                    changed |= i2.corner_collapsed;
                    let Some(child) = i2.shape else { break };
                    cur_rect = Some(child);
                    child.path()
                }
                None => {
                    let i2 = inset_path(cur.as_ref().unwrap_or(&outline), d, self.spec.offsets)?;
                    changed |= i2.counts_changed;
                    cur = Some(i2.path.clone());
                    i2.path
                }
            };
            bg = solid(self.push(Layer::Shell(i), shell, cur_rect, f, bg), bg);
        }

        if s.shadow.iter().any(|sh| sh.kind == ShadowKind::Inset) {
            // Inside the shape, over everything it has painted so far: the
            // inverse blur is opaque *outside* its rectangle, so the outline
            // is what keeps it in the box.
            self.push(Layer::Clip, outline.clone(), rect, &clear, bg);
            for sh in s.shadow.iter().filter(|sh| sh.kind == ShadowKind::Inset) {
                self.shadow(sh, &outline, rect, &welds, bg)?;
            }
            self.push(Layer::Unclip, Path::default(), None, &clear, bg);
        }

        // A welded parent is one continuous outline, but its children paint
        // after the parent. Keep the stroke until the subtree is complete so
        // a child fill cannot erase the shared outer border. Ordinary nodes
        // retain the historical ordering (stroke before their content).
        let mut deferred_stroke: Option<(Path, Option<RoundedRect>, Fill, f64)> = None;
        if let Some(st) = s
            .stroke
            .as_ref()
            .filter(|_| material.is_none() && e.border_ramp.is_none())
        {
            let w = st.width.unwrap_or(th.stroke_width);
            if !(w.is_finite() && w >= 0.0) {
                return Err(SceneError::InvalidRadius);
            }
            if let (crate::BorderAlign::Inside, Some(rr)) = (e.border_align, rect) {
                let stroked = rr.inset(w / 2.)?.shape;
                if let Some(rr) = stroked {
                    if s.weld {
                        deferred_stroke = Some((rr.path(), Some(rr), st.fill.clone(), w));
                    } else if let Some(p) =
                        self.push(Layer::Stroke, rr.path(), Some(rr), &st.fill, bg)
                    {
                        p.width = w;
                    }
                } else {
                    deferred_stroke = Some((outline.clone(), None, st.fill.clone(), 0.));
                }
            } else {
                let band = mui_geometry::border_geometry(
                    &outline,
                    mui_geometry::WidthProfile::uniform(w),
                    e.border_align,
                    self.spec.offsets,
                    self.spec.geometry,
                )?
                .band;
                deferred_stroke = Some((band, None, st.fill.clone(), 0.));
            }
        }

        // A canvas's tagged draws, collected as the surface's hit shapes.
        let mut hits = Vec::new();
        let shaped_content = self.regions.contains_key(&at) && !matches!(e.content, Content::None);
        if shaped_content {
            self.push(Layer::Clip, outline.clone(), rect, &clear, bg);
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
                bg = under;
                let face = Face::of(e, th);
                let lines = self.runs.lines(t, face, frame.size.width, e.lines);
                let fonts = self.runs.fonts_for(face);
                let font_coords = self.runs.coords(&fonts, face);
                let coords = font_coords
                    .first()
                    .cloned()
                    .unwrap_or_else(|| Arc::from(&[][..]));
                let hint = self.runs.settled(&key, &font_coords);
                let n = lines.len();
                let base = self.base_y;
                for (li, line) in lines.iter().enumerate() {
                    let Some(run) = self.runs.run(line, face)? else {
                        break;
                    };
                    // One line sits centred on ascent+descent, or on the
                    // baseline its parent chose; a stack centres the block.
                    let dy = match (base, n) {
                        (Some(b), _) => {
                            b + li as f64 * snap(run.line_height, self.spec.device_scale)
                        }
                        (_, 1) => {
                            frame.y
                                + (frame.size.height - run.ascent - run.descent) / 2.0
                                + run.ascent
                        }
                        _ => {
                            // A snapped line height, so the stack of
                            // baselines is even once the renderer hints each
                            // one to a whole device pixel.
                            let lh = snap(run.line_height, self.spec.device_scale);
                            frame.y
                                + (frame.size.height - n as f64 * lh) / 2.0
                                + run.ascent
                                + li as f64 * lh
                        }
                    };
                    // glifo hints by rounding the device-space baseline per
                    // glyph, so an unsnapped stack of fractional line heights
                    // rounds to uneven leading. Snap the line, not the glyph.
                    let origin = Point::new(
                        snap(frame.x, self.spec.device_scale),
                        snap(dy, self.spec.device_scale),
                    );
                    let text = fonts.first().cloned().map(|font| Text {
                        font,
                        fonts: fonts.clone(),
                        size: size as f32,
                        origin,
                        glyphs: run.glyphs.clone(),
                        axes: e.axes.clone(),
                        coords: coords.clone(),
                        font_coords: font_coords.clone(),
                        hint,
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
                for (k, d) in (c.0)(frame.size).iter().enumerate() {
                    let moved = d.path.rigid_transform(origin, 0.0)?;
                    if let Some(tag) = &d.tag {
                        hits.push((Arc::clone(tag), moved.clone()));
                    }
                    if let Some(p) = self.push(Layer::Draw(k), moved, None, &d.fill, bg) {
                        p.width = d.width;
                    }
                }
            }
            Content::None => {}
        }

        if shaped_content {
            self.push(Layer::Unclip, Path::default(), None, &clear, bg);
        }
        let scrolled = n.scroll_offset();
        let mut content = frame.size;
        if n.is_scroll() {
            // Only in-flow child boxes belong to this viewport. A nested
            // viewport owns its own overflow; floats do not enlarge the flow.
            let pad = n.padding(th.spacing);
            let (mut right, mut bottom) = (frame.x - scrolled[0], frame.y - scrolled[1]);
            let mut child_at = at + 1;
            for child in n.children() {
                if !child.is_float() {
                    let f = self.frames[child_at];
                    right = right.max(f.right());
                    bottom = bottom.max(f.bottom());
                }
                child_at += self.sizes[child_at];
            }
            content = Size::new(
                (right - frame.x + scrolled[0] + pad.right - pad.left).max(0.0),
                (bottom - frame.y + scrolled[1] + pad.bottom - pad.top).max(0.0),
            );
        }
        let surface = self.surfaces.len();
        self.at.insert(key.clone(), surface);
        let (semantics, semantic_label_implicit) = match (&e.semantics, &e.content) {
            (Some(semantics), Content::Text(text)) if semantics.label.is_none() => {
                let mut semantics = semantics.clone();
                semantics.label = Some(text.clone());
                (Some(semantics), true)
            }
            (semantics, _) => (semantics.clone(), false),
        };
        self.surfaces.push(ResolvedSurface {
            key: key.clone(),
            frame,
            bounds: match rect {
                // A rounded rectangle already knows its bounds; only a
                // welded outline has to be flattened to find them.
                Some(r) => Some(r.bounds()),
                None => Bounds::from_points(outline.flatten(0.5, 100_000)?.concat()),
            },
            // Filled in at the end of this node, from the outline itself.
            path: Path::default(),
            rect,
            topology_changed: changed,
            cursor,
            tip: e.tip.clone(),
            focusable: e.focusable,
            disabled,
            semantics,
            semantic_label_implicit,
            text_value: match &e.content {
                Content::Text(t) => Some(t.clone()),
                _ => None,
            },
            clip,
            clip_path: ancestors.clip_paths.clone(),
            parent: ancestors.parent.clone(),
            content,
            hits,
        });
        let mask_path = if masked {
            outline.clone()
        } else {
            Path::default()
        };
        let mut inner = ancestors.clone();
        if n.key()
            .is_some_and(|k| !k.is_empty() && !k.starts_with('/'))
        {
            inner.parent = Some(key.clone());
        }
        // A weld is one contour, so its children paint inside it: a square
        // tab's own fill stops at the filleted corner instead of poking past
        // the shared outline.
        let clips = n.is_clip() || s.weld || e.inside.is_some() || self.regions.contains_key(&at);
        if clips {
            let b = match &material {
                Some(m) => m.image_rect.bounds(),
                None => Bounds::from_points(outline.flatten(0.5, 100_000)?.concat())
                    .unwrap_or_else(|| bounds(frame, self.spec.device_scale)),
            };
            let b = clip.map_or(b, |c| {
                Bounds::new(
                    b.min.x.max(c.min.x),
                    b.min.y.max(c.min.y),
                    b.max.x.min(c.max.x),
                    b.max.y.min(c.max.y),
                )
            });
            self.push(Layer::Clip, outline.clone(), rect, &clear, bg);
            inner.clip = Some(b);
            // Keep every exact outline in one shared allocation for all
            // descendants. `clip` remains the rectangular fast path used by
            // existing input adapters; rounded or welded corners can now be
            // tested without tessellating during each pointer query.
            // ponytail: a nested clip copies its clipping ancestors' paths,
            // O(clip depth) per clipping node; `Arc<[Arc<Path>]>` is the
            // upgrade, and it changes mui-input's `&[Path]` clip API too.
            let mut paths = inner
                .clip_paths
                .as_deref()
                .map_or_else(Vec::new, |paths| paths.to_vec());
            paths.push(outline.clone());
            inner.clip_paths = Some(Arc::from(paths.into_boxed_slice()));
        }
        let outer_base = self.base_y;
        self.base_y = None;
        // Each direct text child's own centred baseline, then every child
        // takes the lowest of the ones it shares a line with: a row taller
        // than its text keeps its labels inside their frames, and a wrapping
        // row gets one baseline per line instead of one per box.
        // ponytail: O(n^2) over direct children, which is a handful.
        let mut bases: Vec<Option<(f64, Frame)>> = Vec::new();
        if e.baseline {
            let mut at2 = at + 1;
            for c in n.children() {
                let f = self.frames[at2];
                at2 += self.sizes[at2];
                let own = match &c.payload().content {
                    Content::Text(t) => {
                        let face = Face::of(c.payload(), th);
                        let lines = self.runs.lines(t, face, f.size.width, c.payload().lines);
                        self.runs.run(&lines[0], face)?.map(|r| {
                            let height = if lines.len() == 1 {
                                r.ascent + r.descent
                            } else {
                                lines.len() as f64 * snap(r.line_height, self.spec.device_scale)
                            };
                            f.y + (f.size.height - height) / 2.0 + r.ascent
                        })
                    }
                    _ => None,
                };
                bases.push(own.map(|b| (b, f)));
            }
            let lines = bases.clone();
            for (b, f) in bases.iter_mut().flatten() {
                *b = lines
                    .iter()
                    .flatten()
                    .filter(|(_, g)| g.y < f.bottom() && f.y < g.bottom())
                    .fold(*b, |m, (o, _)| m.max(*o));
            }
        }
        // One scratch string for the whole walk: a path is O(depth) bytes and
        // formatting a fresh one per node was the walk's largest single cost.
        let mark = path.len();
        let mut sticky = Vec::new();
        for (j, c) in n.children().iter().enumerate() {
            self.base_y = bases.get(j).and_then(|b| b.map(|(y, _)| y));
            path.truncate(mark);
            let _ = write!(path, "/{j}");
            if c.payload().carve.is_some() {
                // Already spent: it shaped the outline instead of painting.
                self.i += self.sizes[self.i];
                continue;
            }
            if c.is_sticky() && !c.is_float() {
                // Pinned over the siblings that scroll under it, so it paints
                // after them -- but inside this node's clip, unlike a float.
                sticky.push((self.i, c, path.clone(), self.base_y));
                self.i += self.sizes[self.i];
                continue;
            }
            if c.is_float() {
                self.deferred.push(Deferred {
                    at: self.i,
                    node: c,
                    path: path.clone(),
                    under: bg,
                    cursor,
                    disabled,
                    parent: inner.parent.clone(),
                });
                self.i += self.sizes[self.i];
            } else {
                self.node(c, path, bg, cursor, &inner, disabled)?;
            }
        }
        let end = self.i;
        for (at2, c, mut p, base) in sticky {
            self.i = at2;
            self.base_y = base;
            self.node(c, &mut p, bg, cursor, &inner, disabled)?;
        }
        self.i = end;
        path.truncate(mark);
        self.base_y = outer_base;
        if clips {
            self.key = key.clone();
            self.push(Layer::Unclip, Path::default(), None, &clear, bg);
        }
        if let Some((stroke_path, stroke_rect, fill, width)) = deferred_stroke {
            self.key = key.clone();
            if let Some(p) = self.push(Layer::Stroke, stroke_path, stroke_rect, &fill, bg) {
                p.width = width;
            }
        }
        if let Some(ramp) = &e.border_ramp {
            ramp.validate()?;
            if material.is_some() {
                return Err(SceneError::UnsupportedWeld(
                    "border ramp requires a fixed outline, not material welding",
                ));
            }
            let named_frame = |id: &mui_layout::Id| -> Result<_, SceneError> {
                if let Some(frame) = self.ramp_frames.get(&(at, id.clone())) {
                    return Ok(*frame);
                }
                let index = find(n, id.as_str(), at, &self.sizes).ok_or(
                    mui_geometry::Error::InvalidOptions("border ramp descendant missing"),
                )?;
                Ok(self.frames[index])
            };
            let anchor = match self.ramp_anchors.get(&at) {
                Some(frame) => *frame,
                None => match &ramp.anchor {
                    None => frame,
                    Some(id) => named_frame(id)?,
                },
            };
            if anchor.size.width <= 0.0 {
                return Err(
                    mui_geometry::Error::InvalidOptions("border ramp anchor has no width").into(),
                );
            }
            let mut sweep = ramp.clone();
            if ramp.align == crate::BorderAlign::Center {
                sweep.from.1 *= 0.5;
                sweep.to.1 *= 0.5;
            }
            let mut band = self.borders.band(
                &key,
                &outline,
                &sweep,
                anchor,
                0.1 / self.spec.device_scale.unwrap_or(1.0),
            )?;
            // Extend the same material into the tabs and their concave shoulders.
            // The final welded outline supplies all corners through the clip.
            let shoulder = match s.radius {
                Radius::Pair(_, concave) => concave,
                Radius::Scale(k) => th.corners.concave * k,
                _ => th.corners.concave,
            };
            crate::border_ramp::decorate(&mut band, ramp, anchor, shoulder, named_frame)?;
            if ramp.align == crate::BorderAlign::Outside {
                let merged = self.region_cache.resolve(
                    (at, 7),
                    Operation::Sweep(band),
                    self.spec.offsets,
                    self.spec.geometry,
                )?;
                band = mui_geometry::boolean_paths(
                    &merged,
                    &outline,
                    BooleanOp::Difference,
                    self.spec.offsets,
                    self.spec.geometry,
                )?;
            }
            if let Some(bounds) = Bounds::from_points(band.flatten(0.1, 250_000)?.concat()) {
                self.key = key.clone();
                let start = self.paint.len();
                if ramp.align == crate::BorderAlign::Inside {
                    self.push(Layer::Clip, outline.clone(), rect, &clear, bg);
                }
                self.push(Layer::Stroke, band, None, &ramp.fill(anchor, bounds), bg);
                if ramp.align == crate::BorderAlign::Inside {
                    self.push(Layer::Unclip, Path::default(), None, &clear, bg);
                }
                if !ramp.tabs.is_empty() {
                    let paint: Vec<_> = self.paint.drain(start..).collect();
                    self.paint
                        .splice(border_background..border_background, paint);
                }
            }
        }
        if let Some(m) = &material {
            // Consume only immediate source plates. Text, canvases, children,
            // clips and semantics keep their own authoring and painter order.
            // The plates painted inside this node, so only its entries are
            // scanned, not everything painted before it.
            let own = self.paint.split_off(start);
            self.paint
                .extend(own.into_iter().filter(|p| !m.consumes(&p.key, p.layer)));
        }
        if blended.is_some() {
            self.key = key.clone();
            if masked {
                self.push(Layer::Mask, mask_path, rect, &s.mask, bg);
            }
            self.key = key;
            self.push(Layer::Unblend, Path::default(), None, &clear, bg);
        }
        if enveloped.is_some() {
            self.push(Layer::Unclip, Path::default(), None, &clear, bg);
        }
        // The outline's last use, so the surface takes it instead of a copy.
        self.surfaces[surface].path = outline;
        Ok(())
    }
}

/// A content leaf's size: a paragraph wrapped to its room when it needs it.
fn fit(runs: &mut Runs, th: Theme, e: &crate::Element, room: Option<f64>) -> Size {
    let Content::Text(t) = &e.content else {
        return Size::ZERO;
    };
    let (t, face) = (t.as_str(), Face::of(e, th));
    let mut fit = match room {
        // The room it wrapped into, not its longest line: a paragraph that
        // reported the ragged width would then be centred inside its own
        // column, aligned with nothing above it.
        Some(room) if room > 0.0 && runs.measure(t, face).width > room + 0.5 => {
            Size::new(room, runs.wrapped(t, face, room, e.lines).height)
        }
        _ => runs.measure(t, face),
    };
    // The reserved string widens the box and nothing else: its own height is
    // the same line at the same size, and a longer value still measures long.
    if let Some(r) = &e.reserve {
        fit.width = fit.width.max(runs.measure(r, face).width);
    }
    fit
}

pub fn resolve_scene(spec: &SceneSpec) -> Result<ResolvedScene, SceneError> {
    resolve_scene_with(spec, &mut TextCache::default())
}

/// [`resolve_scene`] with text shaped once per (string, size) across calls.
pub fn resolve_scene_with(
    spec: &SceneSpec,
    text: &mut TextCache,
) -> Result<ResolvedScene, SceneError> {
    resolve_scene_cached(spec, text, &mut crate::WeldCache::default())
}

/// Resolve with persistent text and material-weld caches. Runtime owners should
/// keep both; the compatibility functions use a temporary welding cache.
pub fn resolve_scene_cached(
    spec: &SceneSpec,
    text: &mut TextCache,
    weld_cache: &mut crate::WeldCache,
) -> Result<ResolvedScene, SceneError> {
    if !spec.theme.is_valid() {
        return Err(SceneError::InvalidTheme);
    }
    if spec
        .device_scale
        .is_some_and(|s| !(s.is_finite() && s > 0.0))
    {
        return Err(SceneError::InvalidScale);
    }
    if !(spec.tolerance.is_finite() && spec.tolerance > 0.0) {
        return Err(SceneError::Text(mui_text::Error::InvalidOptions(
            "tolerance",
        )));
    }
    let font_ids = spec.font.iter().chain(&spec.fallback_fonts).map(Font::id);
    if !text.fonts.iter().copied().eq(font_ids.clone())
        || text.tolerance_bits != spec.tolerance.to_bits()
    {
        text.runs.clear();
        text.breaks.clear();
        text.coords.clear();
        text.last_coords.clear();
        text.fonts = font_ids.collect();
        text.tolerance_bits = spec.tolerance.to_bits();
        text.layout.clear();
    }
    let mut runs = Runs {
        fonts: spec
            .font
            .iter()
            .chain(&spec.fallback_fonts)
            .cloned()
            .collect(),
        own_fonts: None,
        tolerance: spec.tolerance,
        generation: text.generation,
        cache: &mut text.runs,
        breaks: &mut text.breaks,
        coords: &mut text.coords,
        last_coords: &mut text.last_coords,
    };
    let th = spec.theme;
    // Every paragraph wraps in this one pass: mui-layout hands a flex item's
    // final main size back to the measurer, so there is no share left to learn
    // afterwards.
    let layout = mui_layout::resolve_cached_with(
        &spec.root,
        spec.offered,
        spec.limits,
        th.spacing,
        &mut text.layout,
        |e, out| {
            // Exactly the fields `fit` consumes, as bytes the layout cache
            // compares in full. Strings carry their length, so no two
            // payloads share an encoding, and nothing is formatted.
            if let Content::Text(t) = &e.content {
                let mut bytes = |b: &[u8]| {
                    out.extend_from_slice(&b.len().to_le_bytes());
                    out.extend_from_slice(b);
                };
                bytes(t.as_bytes());
                match &e.reserve {
                    Some(r) => bytes(r.as_bytes()),
                    None => out.extend_from_slice(&u64::MAX.to_le_bytes()),
                }
                // Four-byte tags, so the count is all the framing they need.
                out.extend_from_slice(&e.axes.iter().count().to_le_bytes());
                for (tag, value) in e.axes.iter() {
                    out.extend_from_slice(tag.as_bytes());
                    out.extend_from_slice(&value.to_bits().to_le_bytes());
                }
                out.extend_from_slice(&e.text_size.unwrap_or(th.text).to_bits().to_le_bytes());
                // 0 is "no face of its own"; ids shift up one past it.
                out.extend_from_slice(&e.font.as_ref().map_or(0, |f| f.id() + 1).to_le_bytes());
                // usize::MAX is "no cap".
                out.extend_from_slice(&e.lines.unwrap_or(usize::MAX).to_le_bytes());
            }
        },
        |e, room| fit(&mut runs, th, e, room),
    )?;
    let nodes = layout.all().len();
    let mut sizes = Vec::with_capacity(nodes);
    subtree_sizes(&spec.root, &mut sizes);
    let mut w = Walk {
        spec,
        frames: Cow::Borrowed(layout.all()),
        regions: HashMap::new(),
        region_envelopes: HashMap::new(),
        runs,
        sizes,
        outlines: &mut text.outlines,
        borders: &mut text.borders,
        region_cache: &mut text.region_cache,
        ramp_anchors: HashMap::new(),
        ramp_frames: HashMap::new(),
        weld_cache,
        i: 0,
        key: Arc::from(""),
        paint: Vec::new(),
        surfaces: Vec::with_capacity(nodes),
        at: HashMap::with_capacity(nodes),
        external_welds: HashMap::new(),
        deferred: Vec::new(),
        base_y: None,
    };
    w.node(
        &spec.root,
        &mut String::new(),
        th.palette.background(),
        None,
        &Ancestors::default(),
        false,
    )?;
    // Floats paint last, in the order they were met; a float inside a float
    // lands on the end of the same queue.
    let mut k = 0;
    while k < w.deferred.len() {
        let Deferred {
            at,
            node,
            mut path,
            under,
            cursor,
            disabled,
            parent,
        } = w.deferred[k].clone();
        w.i = at;
        w.base_y = None;
        w.node(
            node,
            &mut path,
            under,
            cursor,
            &Ancestors {
                parent,
                ..Ancestors::default()
            },
            disabled,
        )?;
        k += 1;
    }
    let Walk {
        frames,
        paint,
        surfaces,
        at,
        external_welds,
        ..
    } = w;
    let layout = match frames {
        Cow::Owned(frames) => layout.reframe(&spec.root, frames)?,
        Cow::Borrowed(_) => layout,
    };
    text.sweep();
    Ok(ResolvedScene {
        layout,
        paint,
        surfaces,
        at,
        external_welds,
        font: spec.font.clone(),
        fallback_fonts: spec.fallback_fonts.clone(),
        tolerance: spec.tolerance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use crate::{Corners, Spacing};

    /// A hole is a hole: the carved outline loses the child's area, the
    /// child never paints, and `keep` is the same machinery inverted.
    #[test]
    fn a_cut_child_leaves_a_hole_and_paints_nothing() {
        // Signed, so a hole subtracts: the rings come back wound apart.
        let area = |el: El| {
            let s = resolve_scene(&SceneSpec::new(stack![el.id("card")])).unwrap();
            let rings = s
                .surface("card")
                .unwrap()
                .path
                .flatten(0.1, 100_000)
                .unwrap();
            let signed: f64 = rings
                .iter()
                .map(|r| {
                    r.iter()
                        .zip(r.iter().cycle().skip(1))
                        .map(|(a, b)| a.x * b.y - b.x * a.y)
                        .sum::<f64>()
                        / 2.0
                })
                .sum();
            (signed.abs(), s.paint.len())
        };
        let square = |w: f64, h: f64| leaf(w, h).radius(Radius::Px(0.)).center();
        let plain = stack![]
            .square(100.)
            .radius(Radius::Px(0.))
            .fill(Role::Primary);
        let (whole, layers) = area(plain.clone());
        let (holed, carved) = area(plain.clone().cut(square(50., 50.)));
        let (kept, _) = area(plain.keep(square(50., 50.)));
        assert!((whole - 10_000.).abs() < 1.0, "{whole}");
        assert!((holed - 7_500.).abs() < 1.0, "{holed}");
        assert!((kept - 2_500.).abs() < 1.0, "{kept}");
        // The carve child added no paint of its own.
        assert_eq!(layers, carved);
    }

    /// The fade paints last, source-atop, and only inside the layer the node
    /// opened for it.
    #[test]
    fn a_mask_paints_inside_the_nodes_own_blend_layer() {
        let fade = Gradient::linear(
            180.,
            [(0.7, Role::Surface.alpha(0.)), (1., Role::Surface.into())],
        );
        let row = col![leaf(40., 20.).fill(Role::Primary)]
            .pad(8.)
            .mask(fade)
            .id("list");
        let s = resolve_scene(&SceneSpec::new(row)).unwrap();
        let at = |l: Layer| s.paint.iter().position(|p| p.layer == l).unwrap();
        let blend = at(Layer::Blend {
            mix: Mix::Normal,
            opacity: 1.0,
        });
        assert!(
            blend < at(Layer::Fill),
            "the subtree paints inside the layer"
        );
        assert!(at(Layer::Fill) < at(Layer::Mask));
        assert!(at(Layer::Mask) < at(Layer::Unblend));
        assert!(!s.paint[at(Layer::Mask)].path.commands.is_empty());
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
            corners: Corners {
                box_: 28.,
                concave: 32.,
                ..Corners::DEFAULT
            },
            ..Theme::default()
        })
    }
    /// An inset shadow paints over the fill and inside the outline, which
    /// is the whole difference from a drop shadow: same call, opposite side.
    #[test]
    fn an_inset_shadow_paints_over_the_fill_and_clipped_to_the_outline() {
        let root = leaf(40., 40.)
            .radius(8.)
            .fill(Role::Surface)
            .shadow(Shadow::soft(6.))
            .shadow(Shadow::inset(4.))
            .id("box");
        let s = resolve_scene(&SceneSpec::new(root).offered(Size::new(40., 40.))).unwrap();
        let at = |l: Layer| s.paint.iter().position(|p| p.layer == l).expect("layer");
        let (drop, fill) = (at(Layer::Shadow(ShadowKind::Drop)), at(Layer::Fill));
        let inset = at(Layer::Shadow(ShadowKind::Inset));
        assert!(drop < fill, "the drop shadow is over the fill");
        assert!(fill < at(Layer::Clip) && at(Layer::Clip) < inset);
        assert!(
            inset < at(Layer::Unclip),
            "the inset shadow escapes the box"
        );
        assert_eq!(s.paint[inset].blur, 4.);
    }

    /// A welded outline has no analytic rounded rect, so its shadow is one
    /// blurred rect per welded child instead of a single dropped entry.
    #[test]
    fn a_welded_shadow_is_one_blurred_rect_per_child() {
        let root = row([leaf(20., 20.).id("a"), leaf(20., 40.).id("b")])
            .weld(Role::Surface)
            .shadow(Shadow::soft(12.))
            .id("weld");
        let s = resolve_scene(&SceneSpec::new(root).offered(Size::new(40., 40.))).unwrap();
        let sh: Vec<_> = s
            .paint
            .iter()
            .filter(|p| matches!(p.layer, Layer::Shadow(_)))
            .collect();
        assert_eq!(sh.len(), 2, "one blurred rect per welded child");
        for (p, k) in sh.iter().zip(["a", "b"]) {
            assert_eq!(p.blur, 12.);
            let r = p.rect.expect("a rect the renderer can blur").bounds();
            let child = s.surface(k).unwrap().rect.unwrap().bounds();
            assert!((r.min.x - child.min.x).abs() < 1e-9);
            assert!((r.min.y - child.min.y - Shadow::soft(12.).dy).abs() < 1e-9);
        }
    }

    /// A blended node's whole subtree, clip included, sits between the pair.
    #[test]
    fn a_blended_node_is_wrapped_in_a_layer_pair() {
        let dim = column([leaf(10., 10.).fill(Role::Ink).id("kid")])
            .fill(Role::Surface)
            .blend(Mix::Multiply)
            .opacity(0.5)
            .id("dim");
        let root = row([dim, leaf(10., 10.).fill(Role::Surface).id("plain")]).id("root");
        let s = resolve_scene(&SceneSpec::new(root).offered(Size::new(60., 20.))).unwrap();
        let layers: Vec<_> = s.paint.iter().map(|p| (&*p.key, p.layer)).collect();
        let at = |k, l| layers.iter().position(|x| *x == (k, l)).unwrap();
        let open = at(
            "dim",
            Layer::Blend {
                mix: Mix::Multiply,
                opacity: 0.5,
            },
        );
        assert!(open < at("dim", Layer::Fill));
        assert!(at("kid", Layer::Fill) < at("dim", Layer::Unblend));
        assert!(
            !layers
                .iter()
                .any(|(k, l)| *k == "plain" && matches!(l, Layer::Blend { .. } | Layer::Unblend)),
            "{layers:?}"
        );
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
        assert_eq!(
            layers,
            [
                ("root", Layer::Fill),
                ("root", Layer::Clip),
                ("tab", Layer::Shell(0)),
                ("root", Layer::Unclip)
            ]
        );
        assert!(
            s.surface("/0/1").is_some(),
            "unnamed nodes are keyed by path"
        );
    }

    #[test]
    fn welded_children_keep_their_own_outlines() {
        // With a square parent radius, the only way for the first contour to
        // miss the origin is for the child's rounded outline to participate in
        // the weld. The old frame-only union produced a sharp (0, 0) corner.
        let root = row([leaf(20., 20.).radius(8.)])
            .radius(0.)
            .weld(Role::Surface)
            .id("weld");
        let s = resolve_scene(&SceneSpec::new(root).offered(Size::new(20., 20.))).unwrap();
        let points = s
            .surface("weld")
            .unwrap()
            .path
            .flatten(0.1, 20_000)
            .unwrap()
            .concat();
        assert!(
            !points.iter().any(|p| p.x.abs() < 1e-8 && p.y.abs() < 1e-8),
            "weld regressed to the child's sharp frame: {points:?}"
        );
    }

    #[test]
    fn weld_cache_reuses_only_matching_geometry_inputs() {
        let base = SceneSpec::new(
            row([leaf(20., 20.).radius(6.), leaf(18., 24.).radius(8.)])
                .radius(0.)
                .weld(Role::Surface),
        );
        let mut text = TextCache::default();
        resolve_scene_with(&base, &mut text).unwrap();
        let first_misses = text.outlines.misses;
        assert!(first_misses > 0, "the welded outline was not cached");

        resolve_scene_with(&base, &mut text).unwrap();
        assert_eq!(text.outlines.misses, first_misses);
        assert!(text.outlines.hits > 0, "the unchanged weld was not reused");

        let mut changed = base.clone();
        changed.root = changed.root.radius(3.);
        let misses = text.outlines.misses;
        resolve_scene_with(&changed, &mut text).unwrap();
        assert!(
            text.outlines.misses > misses,
            "a style change reused stale geometry"
        );

        changed.theme.corners.box_ += 1.;
        let misses = text.outlines.misses;
        resolve_scene_with(&changed, &mut text).unwrap();
        assert!(
            text.outlines.misses > misses,
            "a theme change reused stale geometry"
        );

        changed.device_scale = Some(2.);
        let misses = text.outlines.misses;
        resolve_scene_with(&changed, &mut text).unwrap();
        assert!(
            text.outlines.misses > misses,
            "a scale change reused stale geometry"
        );
    }

    /// A cached weld is only reused when every input matches, so the
    /// cached outline always equals a fresh resolve of the same spec.
    fn same_as_fresh(spec: &SceneSpec, text: &mut TextCache) {
        let cached = resolve_scene_with(spec, text).unwrap();
        let fresh = resolve_scene(spec).unwrap();
        assert_eq!(
            cached.surface("weld").unwrap().path,
            fresh.surface("weld").unwrap().path,
            "a stale welded outline was reused"
        );
    }

    #[test]
    fn a_weld_reshapes_when_a_childs_custom_outline_changes() {
        let spec = |w: f64| {
            let child = leaf(40., 20.).outline(move |s| {
                let p = [(0., 0.), (s.width * w, 0.), (0., s.height)];
                Path::polyline(p.map(|(x, y)| Point::new(x, y)), true)
            });
            SceneSpec::new(row([child, leaf(20., 20.)]).weld(Role::Surface).id("weld"))
                .offered(Size::new(60., 20.))
        };
        let mut text = TextCache::default();
        resolve_scene_with(&spec(1.), &mut text).unwrap();
        // Same frames, same radii: only the closure differs.
        same_as_fresh(&spec(0.5), &mut text);
    }

    #[test]
    fn a_token_radius_and_a_pill_never_share_a_cached_weld() {
        // The old 64-bit key hashed `Token(Box)` and `Pill` to the same word.
        let spec = |r: Radius| {
            SceneSpec::new(
                row([leaf(40., 20.), leaf(20., 20.)])
                    .radius(r)
                    .weld(Role::Surface)
                    .id("weld"),
            )
            .offered(Size::new(60., 20.))
            .theme(Theme {
                corners: Corners {
                    box_: 2.,
                    ..Corners::DEFAULT
                },
                ..Theme::default()
            })
        };
        let mut text = TextCache::default();
        resolve_scene_with(&spec(Radius::Token(Corner::Box)), &mut text).unwrap();
        same_as_fresh(&spec(Radius::Pill), &mut text);
    }

    #[test]
    fn caches_keep_only_what_the_last_resolve_used() {
        let tree = |n: usize, tag: &str| {
            column((0..n).map(|i| {
                row([
                    leaf(12., 12.),
                    text(format!("{tag}{i}")).id(format!("{tag}{i}")),
                ])
                .weld(Role::Surface)
            }))
        };
        let spec = |n, tag| {
            SceneSpec::new(tree(n, tag))
                .offered(Size::new(200., 8192.))
                .font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap())
        };
        let mut text = TextCache::default();
        resolve_scene_with(&spec(300, "a"), &mut text).unwrap();
        assert_eq!(
            text.outlines.entries.len(),
            300,
            "no cap flushes a big tree"
        );
        resolve_scene_with(&spec(1, "b"), &mut text).unwrap();
        assert_eq!(text.outlines.entries.len(), 1);
        assert_eq!(text.len(), 1, "only b0's run is left");
        assert_eq!(text.last_coords.len(), 1, "only b0's hinting state is left");
    }

    #[test]
    fn inside_regions_past_the_old_flush_limit_stay_cached() {
        // Three region entries per row: past the 256 the cache used to
        // clear at, which refilled and flushed it every frame.
        let tree = column((0..100).map(|_| {
            row([leaf(10., 10.).grow(1.), leaf(10., 10.).grow(1.)])
                .inside(2.)
                .w(40.)
                .h(20.)
        }));
        let spec = SceneSpec::new(tree).offered(Size::new(40., 2000.));
        let mut text = TextCache::default();
        resolve_scene_with(&spec, &mut text).unwrap();
        let held = text.region_cache.len();
        assert!(held > 256, "{held}");
        resolve_scene_with(&spec, &mut text).unwrap();
        assert_eq!(text.region_cache.len(), held);
    }

    #[test]
    fn a_steady_paragraph_reuses_its_line_breaks() {
        let para = "one two three four five six seven eight nine ten";
        let spec = SceneSpec::new(col![text(para).id("p")].w(80.))
            .font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
        let mut text = TextCache::default();
        let lines = |s: &ResolvedScene| s.paint.iter().filter(|p| p.layer == Layer::Text).count();
        assert!(lines(&resolve_scene_with(&spec, &mut text).unwrap()) > 1);
        // Doctor the cached breaks: a resolve that re-broke would not see it.
        for ((ranges, _), _) in text.breaks.values_mut().flat_map(|m| m.values_mut()) {
            *ranges = std::iter::once(0..para.len()).collect();
        }
        assert_eq!(lines(&resolve_scene_with(&spec, &mut text).unwrap()), 1);
    }

    #[test]
    fn a_device_scale_that_is_not_finite_and_positive_is_refused() {
        for scale in [0., -1., f64::NAN, f64::INFINITY] {
            let spec = SceneSpec::new(leaf(10., 10.)).scale(scale);
            assert!(
                matches!(resolve_scene(&spec), Err(SceneError::InvalidScale)),
                "{scale}"
            );
        }
    }

    #[test]
    fn weld_ignores_zero_area_children() {
        let root = row([leaf(0., 20.), leaf(20., 20.)])
            .weld(Role::Surface)
            .id("weld");
        let s = resolve_scene(&SceneSpec::new(root).offered(Size::new(20., 20.))).unwrap();
        assert!(!s.surface("weld").unwrap().path.commands.is_empty());
    }

    #[test]
    fn welded_stroke_paints_after_child_fills() {
        let root = row([leaf(20., 20.).fill(Role::Primary).id("child")])
            .weld(Role::Surface)
            .stroke(Role::Ink)
            .stroke_width(2.)
            .id("weld");
        let s = resolve_scene(&SceneSpec::new(root).offered(Size::new(20., 20.))).unwrap();
        let order: Vec<_> = s.paint.iter().map(|p| (&*p.key, p.layer)).collect();
        let at = |key: &str, layer: Layer| order.iter().position(|x| *x == (key, layer)).unwrap();
        assert!(
            at("weld", Layer::Fill) < at("child", Layer::Fill)
                && at("child", Layer::Fill) < at("weld", Layer::Stroke),
            "welded border was painted under its child: {order:?}"
        );
    }

    #[test]
    fn a_weld_clips_its_children_to_the_shared_contour() {
        // A square tab welded to a rounded body: the tab's own fill must not
        // paint the corner the fillet rounded off, so the weld clips.
        let root = row([
            leaf(36., 60.).fill(Role::Surface).radius(0.).id("tab"),
            leaf(200., 120.).id("body"),
        ])
        .radius(20.)
        .weld(Role::Surface)
        .id("weld");
        let s = resolve_scene(&SceneSpec::new(root).offered(Size::new(236., 120.))).unwrap();
        let order: Vec<_> = s.paint.iter().map(|p| (&*p.key, p.layer)).collect();
        let at = |key: &str, layer: Layer| order.iter().position(|x| *x == (key, layer)).unwrap();
        assert!(
            at("weld", Layer::Fill) < at("weld", Layer::Clip)
                && at("weld", Layer::Clip) < at("tab", Layer::Fill)
                && at("tab", Layer::Fill) < at("weld", Layer::Unclip),
            "the tab painted outside the welded contour: {order:?}"
        );
        let clip = &s.paint[at("weld", Layer::Clip)].path;
        let corner = clip.flatten(0.1, 20_000).unwrap().concat();
        assert!(
            !corner.iter().any(|p| p.x.abs() < 1e-6 && p.y.abs() < 1e-6),
            "the clip is the square frame, not the filleted contour"
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
        sp.font = Some(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
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
        assert!(matches!(
            s.paint[0].paint,
            Paint::Gradient { kind: crate::GradientKind::Linear { angle }, .. } if angle == 180.
        ));
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
        assert_eq!(s.surfaces().last().map(|s| &*s.key), Some("tip"));
    }

    #[test]
    fn rounded_clip_exposes_cached_path_alongside_rect_bounds() {
        let clip = column([leaf(20., 20.).id("a"), leaf(20., 20.).id("b")])
            .size(40., 40.)
            .radius(10.)
            .clip()
            .id("clip");
        let s = resolve_scene(&SceneSpec::new(clip)).unwrap();
        let parent = s.surface("clip").unwrap();
        let child = s.surface("a").unwrap();
        let sibling = s.surface("b").unwrap();
        assert_eq!(parent.clip, None);
        assert_eq!(child.clip, parent.bounds);
        assert_eq!(sibling.clip, parent.bounds);
        let paths = child.clip_path.as_ref().expect("rounded clip path");
        assert_eq!(paths.len(), 1);
        assert!(
            paths[0]
                .commands
                .iter()
                .any(|c| matches!(c, mui_geometry::PathCommand::ArcTo(_))),
            "clip path lost its rounded corners"
        );
        assert!(
            Arc::ptr_eq(
                child.clip_path.as_ref().unwrap(),
                sibling.clip_path.as_ref().unwrap()
            ),
            "clip path must remain cached for repeated hit tests"
        );
    }

    #[test]
    fn nested_rounded_clips_keep_every_cached_path() {
        let inner = column([leaf(30., 30.).id("leaf")])
            .size(30., 30.)
            .radius(6.)
            .clip()
            .id("inner");
        let outer = column([inner])
            .size(40., 40.)
            .radius(10.)
            .clip()
            .id("outer");
        let s = resolve_scene(&SceneSpec::new(outer)).unwrap();
        let paths = s.surface("leaf").unwrap().clip_path.as_ref().unwrap();
        assert_eq!(
            paths.len(),
            2,
            "inner and outer clips must both filter hits"
        );
        assert!(paths.iter().all(|p| {
            p.commands
                .iter()
                .any(|c| matches!(c, mui_geometry::PathCommand::ArcTo(_)))
        }));
    }

    /// A sticky header paints after the rows that slide under it, and stays
    /// inside the scroll's clip -- a float would escape it.
    #[test]
    fn a_sticky_header_paints_over_its_section_and_keeps_the_clip() {
        let section = column([
            leaf(60., 20.).fill(Role::Surface).id("head").sticky(),
            leaf(60., 60.).id("row"),
        ])
        .id("section");
        let list = column([section]).scroll().size(60., 40.).id("list");
        let s = resolve_scene(&SceneSpec::new(list)).unwrap();
        let order: Vec<_> = s.surfaces().map(|s| &*s.key).collect();
        assert_eq!(
            order,
            ["list", "section", "row", "head"],
            "the header paints last"
        );
        let clip = s.surface("list").unwrap().bounds;
        assert_eq!(s.surface("head").unwrap().clip, clip);
    }

    /// A tagged draw is hit geometry in scene space; an untagged one is
    /// paint and nothing else.
    #[test]
    fn a_tagged_draw_becomes_the_surfaces_hit_shape() {
        let box_ = |w: f64, h: f64| {
            Path::polyline(
                [(0., 0.), (w, 0.), (w, h), (0., h)].map(|(x, y)| Point::new(x, y)),
                true,
            )
        };
        let plot = canvas(move |s| {
            vec![
                Draw::fill(box_(s.width, s.height), Primary),
                Draw::hit(box_(s.width / 2., s.height), "left"),
            ]
        })
        .size(40., 20.)
        .id("plot");
        let root = column([leaf(40., 30.), plot]);
        let s = resolve_scene(&SceneSpec::new(root)).unwrap();
        let surface = s.surface("plot").unwrap();
        let [(tag, path)] = &surface.hits[..] else {
            panic!("one tagged draw, got {:?}", surface.hits.len())
        };
        assert_eq!(&**tag, "left");
        let pts = path.flatten(0.1, 1000).unwrap().concat();
        let top = pts.iter().map(|p| p.y).fold(f64::MAX, f64::min);
        assert_eq!(top, 30., "moved into the node's frame");
        assert!(
            !s.paint
                .iter()
                .any(|p| &*p.key == "plot" && p.layer == Layer::Draw(1)),
            "a hit-only draw paints nothing"
        );
    }

    fn font() -> Font {
        Font::new(epaint_default_fonts::HACK_REGULAR).unwrap()
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

    /// Every text layer's baseline for a key, in paint order.
    fn baselines(s: &ResolvedScene, k: &str) -> Vec<f64> {
        s.paint
            .iter()
            .filter(|p| &*p.key == k && p.layer == Layer::Text)
            .map(|p| p.text.as_ref().unwrap().origin.y)
            .collect()
    }

    #[test]
    fn a_baseline_row_taller_than_its_text_keeps_the_letters_in_their_frames() {
        let root = row([
            text("Kurv").text_size(22.).id("title"),
            text("v1.0").text_size(11.).id("ver"),
            leaf(80., 40.).id("btn"),
        ])
        .baseline()
        .gap(10.);
        let mut sp = SceneSpec::new(root).offered(Size::new(400., 60.));
        sp.font = Some(font());
        let s = resolve_scene(&sp).unwrap();
        let t = s.layout.frame("title").unwrap();
        let b = baselines(&s, "title")[0];
        assert_eq!(b, baselines(&s, "ver")[0], "one baseline, two sizes");
        assert!(
            b > t.y && b < t.bottom(),
            "baseline {b} outside the title's frame {t:?}"
        );
    }

    #[test]
    fn a_wrapping_baseline_row_gives_every_line_its_own_baseline() {
        let root = row([
            text("alpha").text_size(20.).id("a"),
            text("beta").text_size(11.).id("b"),
            text("gamma").text_size(20.).id("c"),
        ])
        .wrap()
        .baseline()
        .gap(8.);
        let mut sp = SceneSpec::new(root).offered(Size::new(120., 200.));
        sp.font = Some(font());
        let s = resolve_scene(&sp).unwrap();
        let (a, c) = (baselines(&s, "a")[0], baselines(&s, "c")[0]);
        assert_eq!(a, baselines(&s, "b")[0], "line one shares a baseline");
        assert!(a < c, "line two sits below line one: {a} {c}");
        let f = s.layout.frame("c").unwrap();
        assert!(c > f.y && c < f.bottom(), "baseline {c} outside {f:?}");
    }

    #[test]
    fn a_wrapped_paragraph_fills_its_column_instead_of_its_longest_line() {
        let long = "wrap ".repeat(40);
        let root = row([
            column([text("About").text_size(18.).id("h"), text(long).id("p")])
                .gap(6.)
                .flex(1.)
                .id("col"),
            leaf(90., 40.).shrink(0.),
        ])
        .gap(10.);
        let mut sp = SceneSpec::new(root).offered(Size::new(320., 200.));
        sp.font = Some(font());
        let s = resolve_scene(&sp).unwrap();
        let (col, p) = (s.layout.frame("col").unwrap(), s.layout.frame("p").unwrap());
        assert_eq!(
            (p.x, p.size.width),
            (col.x, col.size.width),
            "{p:?} {col:?}"
        );
    }

    #[test]
    fn a_stroke_paints_inside_the_frame_it_was_given() {
        let root = overlay([leaf(20., 20.).radius(0.).stroke(Role::Ink).id("k")]);
        let s = resolve_scene(&SceneSpec::new(root).offered(Size::new(20., 20.))).unwrap();
        let st = s.paint.iter().find(|p| p.layer == Layer::Stroke).unwrap();
        let b = Bounds::from_points(st.path.flatten(0.01, 100_000).unwrap().concat()).unwrap();
        let w = st.width / 2.0;
        let f = s.layout.frame("k").unwrap();
        // The painted band is the path grown by half the width; inside means
        // that band is exactly the frame.
        assert!((b.min.x - w - f.x).abs() < 1e-9, "{b:?} {f:?} {w}");
        assert!((b.max.y + w - f.bottom()).abs() < 1e-9, "{b:?} {f:?} {w}");
    }

    #[test]
    fn a_device_scale_puts_every_edge_and_every_baseline_on_the_grid() {
        let long = "wrap ".repeat(40);
        let row = row![
            leaf(0., 20.).grow(1.).id("a"),
            leaf(0., 20.).grow(1.).id("b"),
            leaf(0., 20.).grow(1.).id("c"),
        ];
        let root = column([row, column([text(long).id("p")]).w(120)]);
        let mut sp = SceneSpec::new(root).offered(Size::new(41., 300.)).scale(1.);
        sp.font = Some(font());
        let s = resolve_scene(&sp).unwrap();
        let edges: Vec<[f64; 2]> = ["a", "b", "c"]
            .iter()
            .map(|k| {
                let b = s.surface(k).unwrap().rect.unwrap().bounds();
                [b.min.x, b.max.x]
            })
            .collect();
        for e in edges.iter().flatten() {
            assert_eq!(*e, e.round(), "{edges:?}");
        }
        assert_eq!(edges[0][1], edges[1][0], "no seam between shares");
        assert_eq!(edges[1][1], edges[2][0], "no seam between shares");
        let ys = baselines(&s, "p");
        assert!(ys.len() > 3, "wrapped into {} lines", ys.len());
        let gaps: Vec<f64> = ys.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(
            gaps.windows(2).all(|g| g[0] == g[1]) && ys[0] == ys[0].round(),
            "uneven leading {gaps:?} from {ys:?}"
        );
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
    fn weight_reaches_the_run_and_keys_the_cache() {
        let mut cache = TextCache::default();
        let mut sp = SceneSpec::new(row![
            text("hi").id("a"),
            text("hi").text_weight(Weight::BOLD).id("b")
        ]);
        sp.font = Some(Font::new(ttf_inter::REGULAR).unwrap());
        resolve_scene_with(&sp, &mut cache).unwrap();
        // Same string, two weights of a variable face: two shaped runs, not
        // one reused at the wrong instance.
        assert_eq!(cache.len(), 2);
        // A static face shapes the same at any weight, so it shares one.
        let mut hack = TextCache::default();
        sp.font = Some(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
        resolve_scene_with(&sp, &mut hack).unwrap();
        assert_eq!(hack.len(), 1);
        sp.font = Some(Font::new(ttf_inter::REGULAR).unwrap());
        let s = resolve_scene_with(&sp, &mut cache).unwrap();
        let w: Vec<Option<f32>> = s
            .paint
            .iter()
            .filter(|p| p.layer == Layer::Text)
            .filter_map(|p| p.text.as_ref().map(|t| t.axes.get("wght")))
            .collect();
        assert_eq!(w, [None, Some(700.)]);
    }

    #[test]
    fn hinting_pauses_for_the_frame_after_an_axis_moves() {
        let inter = Font::new(ttf_inter::REGULAR).unwrap();
        let hint_at = |w: f32, cache: &mut TextCache| {
            let mut sp = SceneSpec::new(row([text("hi").text_axis("wght", w).id("t")]));
            sp.font = Some(inter.clone());
            let s = resolve_scene_with(&sp, cache).unwrap();
            s.paint.iter().find_map(|p| p.text.as_ref()).unwrap().hint
        };
        let mut cache = TextCache::default();
        assert!(hint_at(400., &mut cache), "first frame is settled");
        assert!(hint_at(400., &mut cache));
        assert!(!hint_at(500., &mut cache), "the frame it moved on");
        assert!(hint_at(500., &mut cache), "settled again");
        // Two settings that round to the same coordinates are one instance.
        assert!(hint_at(500.001, &mut cache));
    }

    #[test]
    fn a_swapped_readout_keeps_the_reserved_box() {
        let mut sp = SceneSpec::new(row![text("0.0").reserve("-88.8").id("gain")]);
        sp.font = Some(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
        let mut s = resolve_scene(&sp).unwrap();
        let frame = s.surface("gain").unwrap().frame;
        s.set_text("gain", "-88.8").unwrap();
        assert_eq!(s.surface("gain").unwrap().frame, frame);
        let t = s
            .paint
            .iter()
            .find(|p| p.layer == Layer::Text)
            .and_then(|p| p.text.clone())
            .unwrap();
        assert_eq!(t.glyphs.len(), 5);
        // The reserved string is exactly the frame's content, so the run
        // ends inside the box it was measured for.
        let last = t.origin.x + f64::from(t.glyphs[4].x);
        assert!(
            last <= frame.x + frame.size.width + 0.5,
            "{last} in {frame:?}"
        );
    }

    #[test]
    fn set_text_updates_only_an_implicit_accessibility_label() {
        let mut implicit = SceneSpec::new(text("before").role(Kind::Label).id("implicit"));
        implicit.font = Some(font());
        let mut implicit = resolve_scene(&implicit).unwrap();
        assert_eq!(
            implicit
                .surface("implicit")
                .unwrap()
                .semantics
                .as_ref()
                .and_then(|semantics| semantics.label.as_deref()),
            Some("before")
        );
        implicit.set_text("implicit", "after").unwrap();
        assert_eq!(
            implicit
                .surface("implicit")
                .unwrap()
                .semantics
                .as_ref()
                .and_then(|semantics| semantics.label.as_deref()),
            Some("after")
        );

        let mut explicit = SceneSpec::new(
            text("before")
                .role(Kind::Label)
                .label("Stable name")
                .id("explicit"),
        );
        explicit.font = Some(font());
        let mut explicit = resolve_scene(&explicit).unwrap();
        explicit.set_text("explicit", "after").unwrap();
        assert_eq!(
            explicit
                .surface("explicit")
                .unwrap()
                .semantics
                .as_ref()
                .and_then(|semantics| semantics.label.as_deref()),
            Some("Stable name")
        );
    }

    #[test]
    fn set_text_keeps_a_wrapped_node_single_line_without_relayout() {
        let mut spec = SceneSpec::new(text("one two three four").lines(2).id("paragraph"))
            .offered(Size::new(72., 80.));
        spec.font = Some(font());
        let mut scene = resolve_scene(&spec).unwrap();
        let frame = scene.surface("paragraph").unwrap().frame;
        let before = scene
            .paint
            .iter()
            .filter(|paint| &*paint.key == "paragraph" && paint.layer == Layer::Text)
            .count();
        assert_eq!(before, 2);

        scene
            .set_text("paragraph", "a replacement that is much longer")
            .unwrap();
        assert_eq!(scene.surface("paragraph").unwrap().frame, frame);
        let after = scene
            .paint
            .iter()
            .filter(|paint| &*paint.key == "paragraph" && paint.layer == Layer::Text)
            .count();
        assert_eq!(after, 1, "set_text collapses wrapped paint by contract");
    }

    #[test]
    fn text_cache_survives_frames_and_carries_glyphs() {
        let mut cache = TextCache::default();
        let mut sp = SceneSpec::new(row([text("hi").id("t")]));
        sp.font = Some(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
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
        assert!(t.glyphs[1].x > 0.);
        let next = resolve_scene_with(&sp, &mut cache).unwrap();
        let next_text = next.paint.iter().find_map(|p| p.text.as_ref()).unwrap();
        assert!(Arc::ptr_eq(&t.glyphs, &next_text.glyphs));
        assert_eq!(cache.len(), 1);
        assert!(
            s.paint == next.paint,
            "unchanged text must retain its paint"
        );
        let mut changed_face = next_text.clone();
        changed_face.fonts = next_text
            .fonts
            .iter()
            .map(|font| Font::new(font.as_ref().to_vec()).unwrap())
            .collect();
        assert!(t != &changed_face, "a different Font invalidates paint");
    }
}

#[cfg(test)]
mod module_tab_weld {
    use crate::prelude::*;
    use crate::*;
    use mui_layout::{Align, Size, Spacing};
    #[test]
    /// A square port tab next to a square body welds into one contour; a
    /// rounded tab is a pill that only kisses the body and the union splits.
    #[allow(clippy::float_cmp)]
    fn a_square_tab_welds_into_one_contour_with_its_body() {
        let tab = column([leaf(24., 24.)])
            .w(36.)
            .h(36.)
            .pad(6.)
            .gap(0.)
            .shrink(0.)
            .fill(Role::Surface)
            .radius(0.)
            .align(Align::End);
        let body = leaf(400., 200.)
            .grow(1.)
            .shrink(1.)
            .min_width(0.)
            .radius(0.)
            .id("body");
        let root = row([tab, body])
            .align(Align::Start)
            .gap(Spacing::Px(0.0))
            .w(pct(100.))
            .weld(Role::Surface)
            .stroke(Role::Dim)
            .stroke_width(1.5)
            .radius(20.)
            .id("weld");
        let s = resolve_scene(&SceneSpec::new(root).offered(Size::new(500., 200.))).unwrap();
        let pts = s
            .surface("weld")
            .unwrap()
            .path
            .flatten(0.1, 20_000)
            .unwrap()
            .concat();
        let minx = pts.iter().map(|p| p.x).fold(f64::MAX, f64::min);
        // a point on the seam x=36 at y=100: inside the union?
        let contours = s
            .surface("weld")
            .unwrap()
            .path
            .flatten(0.1, 20_000)
            .unwrap();
        assert!(minx < 1.0, "tab dropped from the weld: minx {minx}");
        assert_eq!(contours.len(), 1);
    }
}
