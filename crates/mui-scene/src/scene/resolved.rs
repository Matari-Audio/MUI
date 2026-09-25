//! What a resolve hands back: the paint list and the surfaces.
use rustc_hash::FxHashMap as HashMap;
use std::sync::Arc;

use mui_geometry::{Bounds, Path, Point, RoundedRect};
use mui_layout::{Frame, Layout, Size};
use mui_text::{Axes, Font};

use super::text::CachedRun;
use super::SceneError;
use crate::{Cursor, Mix, Paint, Semantics, ShadowKind};

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
    /// Everything painted before this entry, again, blurred by `blur` and
    /// clipped to `path`; the paint is meaningless. The first entry of a
    /// node with [`Style::backdrop_blur`](crate::Style::backdrop_blur), inside
    /// its own blend layer, so a fading modal fades its blur with it.
    Backdrop,
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
    /// Primary face followed by any fallback faces used by this run; never
    /// empty.
    pub fonts: Arc<[Font]>,
    pub size: f32,
    /// Baseline origin.
    pub origin: Point,
    /// Shaped glyph id, x/y offset and face index from the origin.
    pub glyphs: Arc<[TextGlyph]>,
    /// The axis settings the run was shaped at; `font_coords` is the same
    /// thing in the form a glyph cache wants, and
    /// [`ResolvedScene::set_text`] re-shapes from this one.
    pub axes: Axes,
    /// Each face's normalized axis coordinates this run was measured at,
    /// indexed like [`Self::fonts`], from [`mui_text::normalized_coords`].
    /// Empty for a static face. A renderer with its own glyph cache has to
    /// pass these on, or it paints the default instance under a bold run's
    /// advances.
    pub font_coords: Arc<[Arc<[i16]>]>,
    /// Whether a renderer should hint this run. Off for the frame after its
    /// axes moved: a glyph mid-morph gains nothing from stem snapping and
    /// would cost a fresh hinting instance per frame.
    pub hint: bool,
}

/// One thing to draw. `key` is the node's id, or its tree path (`/0/2`)
/// when it has none: hit-testing and state keep working without names.
#[derive(Clone, Debug)]
pub struct Painted {
    pub key: Arc<str>,
    pub layer: Layer,
    /// Shared with the node's surface and every other layer drawn along the
    /// same outline, so a filled, clipped node holds one path.
    pub path: Arc<Path>,
    pub paint: Paint,
    /// Analytic form when the path is a plain rounded rectangle: a renderer
    /// with a fast path (blurred rects, say) can take it.
    pub rect: Option<RoundedRect>,
    /// Stroke width; `0` fills.
    pub width: f64,
    /// Gaussian blur radius: a shadow's, or a [`Layer::Backdrop`]'s.
    pub blur: f64,
    /// Present on `Layer::Text` whenever [`SceneSpec::font`](crate::SceneSpec::font) is set: the
    /// layer's ink, as glyphs. `path` is then empty -- a renderer that draws
    /// glyphs never looks at it, and translating every run's outline into a
    /// fresh path is the most expensive thing the walk can do.
    pub text: Option<Text>,
}

/// The same `Arc` is the same value: a still scene hands its paths and
/// glyph runs back by pointer, and comparing them command by command is
/// what a renderer's "did anything change" would otherwise spend its frame on.
fn same<T: PartialEq + ?Sized>(a: &Arc<T>, b: &Arc<T>) -> bool {
    Arc::ptr_eq(a, b) || a == b
}
// By hand for `same`; destructured without `..` so a new field cannot be
// left out of the comparison.
impl PartialEq for Text {
    fn eq(&self, o: &Self) -> bool {
        let Self {
            fonts,
            size,
            origin,
            glyphs,
            axes,
            font_coords,
            hint,
        } = self;
        same(fonts, &o.fonts)
            && *size == o.size
            && *origin == o.origin
            && same(glyphs, &o.glyphs)
            && *axes == o.axes
            && same(font_coords, &o.font_coords)
            && *hint == o.hint
    }
}
impl PartialEq for Painted {
    fn eq(&self, o: &Self) -> bool {
        let Self {
            key,
            layer,
            path,
            paint,
            rect,
            width,
            blur,
            text,
        } = self;
        same(key, &o.key)
            && *layer == o.layer
            && same(path, &o.path)
            && *paint == o.paint
            && *rect == o.rect
            && *width == o.width
            && *blur == o.blur
            && *text == o.text
    }
}

/// A node's outline, for hit-testing and for anything that derives from it.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedSurface {
    pub key: Arc<str>,
    pub frame: Frame,
    pub path: Arc<Path>,
    pub bounds: Option<Bounds>,
    /// Exact rounded rectangle when the outline is one (not welded).
    pub rect: Option<RoundedRect>,
    /// A shell collapsed or a merge changed ring counts.
    pub topology_changed: bool,
    pub cursor: Option<Cursor>,
    pub tip: Option<String>,
    pub focusable: bool,
    /// Keeps the wheel from the scrollers around it.
    pub captures_wheel: bool,
    /// Declares a [`State::Hover`](crate::State::Hover) or
    /// [`State::Press`](crate::State::Press) look, so the runtime makes it a
    /// pointer target even without an id.
    pub pointer_states: bool,
    /// Switched off by itself or by an ancestor: not a hit target, not a Tab
    /// stop, and reported disabled to a screen reader. See
    /// [`Styled::disabled`](crate::Styled::disabled).
    pub disabled: bool,
    /// The role and name this surface reports to a screen reader.
    pub semantics: Option<Semantics>,
    /// The name came from this node's text because no explicit `.label(..)`
    /// was supplied. Live text swaps update this name; an explicit label does
    /// not move with the paint.
    pub(super) semantic_label_implicit: bool,
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
    /// clip and preserves every nested clip boundary. Each path is the
    /// clipping ancestor's own outline, shared, not a copy.
    pub clip_path: Option<Arc<[Arc<Path>]>>,
    /// Nearest explicitly named ancestor in the authored tree, not a containing
    /// rectangle. A floating node keeps this parent even when it escapes clipping.
    pub parent: Option<Arc<str>>,
    /// A scroll node's children extent inside its padding, unscrolled;
    /// the frame size otherwise.
    pub content: Size,
    /// The tagged shapes a `canvas` drew, in scene space. Non-empty means
    /// *these* are the surface's hit geometry, not its outline: the pointer
    /// outside all of them is outside the node. See [`Draw::tag`](crate::Draw::tag).
    pub hits: Vec<(Arc<str>, Arc<Path>)>,
}
impl ResolvedSurface {
    /// Borrow the cached clip outlines without exposing their shared
    /// allocation. Paths are ordered outermost to innermost.
    pub fn clip_paths(&self) -> Option<&[Arc<Path>]> {
        self.clip_path.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedScene {
    pub layout: Layout,
    pub paint: Vec<Painted>,
    /// Every surface in paint order, which is also z-order.
    pub(super) surfaces: Vec<ResolvedSurface>,
    pub(super) at: HashMap<Arc<str>, usize>,
    pub(crate) external_welds: HashMap<Arc<str>, crate::ExternalWeld>,
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
        let mut text = self.paint[first]
            .text
            .clone()
            .ok_or(SceneError::NoTextLayer)?;
        // The faces the run was resolved with, per-node font included.
        let run = mui_text::shape_run(&text.fonts, s, f64::from(text.size), &text.axes.to_vec())?;
        text.glyphs = CachedRun::from_run(run).glyphs;
        self.paint[first].text = Some(text);
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
#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::prelude::*;

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
}
