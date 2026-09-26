//! The per-frame runtime: gestures in, animated styles applied, scene out.
use std::any::Any;
mod wake;
use crate::SemanticAction;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use mui_geometry::Point;
use mui_input::{
    Button, Buttons, FINE_DRAG, Hit, Ime, Input, Interaction as Pointer, Key, KeyPress,
    PointerInput, Response,
};
use mui_layout::SpacingToken::{S, Xs};
use mui_scene::Keys;
use mui_scene::prelude::{Paints as _, Role, stack, text};
use mui_scene::{
    A11y, Appear, Area, Color, Cursor, El, Element, Fill, Font, Layer, Mix, Paint, Painted,
    Palette, Pin, Radius, ResolvedScene, Resolver, SceneError, SceneSpec, Size, Spacing, Spring,
    State, Theme, bar, push_index,
};
use std::sync::Arc;

mod input;
mod memo;
mod motion;
mod scroll;
use input::*;
use memo::*;
use motion::*;

#[cfg(test)]
mod audit_tests;

/// The id the floated tip carries. A leading `/` keeps it out of hit
/// testing, like every other key the runtime owns.
const TIP_KEY: &str = "/tip";

/// How long the pointer must rest on a surface before its tip is due.
pub const TIP_DELAY: f64 = 0.5;
/// Two presses on one target within this are a double click.
pub const DOUBLE_CLICK: f64 = 0.4;

/// The host's clipboard, for a [`Ui`] that reads and writes it itself.
///
/// Without one, the clipboard is data: a copy comes out on
/// [`Frame::clipboard`] and a paste comes in on [`Input::clipboard`], which
/// the host fills because it saw the paste key. With one, a paste key reads
/// [`Clipboard::get`] when the host handed no text in, a copy or cut goes
/// straight to [`Clipboard::set`], and [`Ui::paste`] answers an app's own
/// Paste button. baseview has no clipboard to read, so a plugin host brings
/// one (arboard, say); a test brings a `String`.
///
/// ```
/// # use mui::{Clipboard, Ui}; use mui::prelude::*;
/// struct Memory(String);
/// impl Clipboard for Memory {
///     fn get(&mut self) -> Option<String> { Some(self.0.clone()) }
///     fn set(&mut self, text: &str) { self.0 = text.to_owned(); }
/// }
/// let mut ui = Ui::new(Theme::DEFAULT).clipboard(Memory("saw".into()));
/// assert_eq!(ui.paste().as_deref(), Some("saw"));
/// ```
pub trait Clipboard: Send {
    /// The clipboard's text, or `None` when it holds none or cannot be read.
    fn get(&mut self) -> Option<String>;
    /// Replace the clipboard's contents with `text`.
    fn set(&mut self, text: &str);
}

/// What one call to [`Ui::frame`] produced.
pub struct Frame<'a> {
    pub scene: &'a ResolvedScene,
    /// A spring or interaction deadline is still moving: schedule another frame.
    /// This includes transitions and a pending tooltip; a focused caret uses
    /// `repaint_after` instead.
    pub animating: bool,
    /// Next timer-driven change; input and model updates invalidate separately.
    pub repaint_after: Option<std::time::Duration>,
    /// A tip that came due this frame, and where to put it. It is already
    /// floated into the scene; this is for a host that would rather place its
    /// own (a native tooltip window, say).
    pub tip: Option<(String, Point)>,
    /// The cursor the hovered surface asks for.
    pub cursor: Cursor,
    /// Every gesture that began or ended this frame, for a host that brackets
    /// automation. [`Ui::edit`] asks about one id.
    pub edits: Vec<(String, Edit)>,
    /// A copy or cut asked for this: put it on the host's clipboard. Always
    /// `None` for a [`Ui`] given a [`Clipboard`], which already has it.
    pub clipboard: Option<String>,
    /// The caret of the field that wants an input method, in scene units.
    /// `Some` means "allow IME and put the candidate window here"; `None`
    /// means no field is composing-capable, so switch IME off.
    pub ime: Option<(Point, Size)>,
}

/// How hovered and how pressed a target is, each 0..1 and spring-smoothed,
/// for a control that eases its look in and out: [`Ui::state`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Interaction {
    pub hover: f64,
    pub press: f64,
}

/// A parameter gesture's two edges. A slider or knob drag is one `Begin`, a
/// run of value changes, and one `End`: exactly the bracket a plugin host
/// wants around touched automation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    Begin,
    End,
}

/// Retained state for an immediate tree. Build the tree every frame; the
/// runtime remembers what is hovered, held, and mid-animation.
pub struct Ui {
    pub theme: Theme,
    pub font: Option<Font>,
    /// Optional faces tried per grapheme after [`Self::font`].
    pub fallback_fonts: Vec<Font>,
    /// The window's device pixels per logical unit. Set it and every painted
    /// edge lands on a device pixel; `None` paints on layout's raw f64.
    pub scale: Option<f64>,
    pub weld_backend: mui_scene::WeldBackend,
    /// Seconds between two presses on one target that still make a double
    /// click. [`DOUBLE_CLICK`] unless the host matches a platform setting.
    pub double_click: f64,
    interaction: Pointer,
    actions: Vec<SemanticAction>,
    hit: Hit,
    scene: Option<ResolvedScene>,
    /// Per key: hover and press springs, 0..1.
    springs: BTreeMap<String, [Spring; 2]>,
    /// Transition springs per node key, one per paint channel under its
    /// stable id (see [`channels`]), flagged when the current frame visits
    /// them; the rest are dropped at its end.
    motion: BTreeMap<String, Channels>,
    /// [`Styled::animate_layout`](mui_scene::Styled::animate_layout)
    /// springs per node key: x, y, width, height, relative to the nearest
    /// animating ancestor.
    glides: BTreeMap<String, (bool, [Spring; 4])>,
    /// Keys that declared [`Appear`] this frame and last, with the spring
    /// they fade by: a key in `last` and gone from the tree leaves a ghost.
    appearing: BTreeMap<String, Spring>,
    /// Nodes that left the tree, still fading out where they stood.
    ghosts: Vec<Ghost>,
    /// [`Styled::morph`](mui_scene::Styled::morph) state per node key.
    morphs: BTreeMap<String, Morph>,
    /// Last frame's [`Styled::identity`](mui_scene::Styled::identity)
    /// nodes: what each was named and where it sat in the caller's tree.
    identities: HashMap<u64, (Option<String>, String)>,
    /// [`Ui::tween`] springs per id, flagged when the builder reads them.
    tweens: BTreeMap<String, (bool, Spring)>,
    /// [`Ui::play`] start times per id, flagged when the builder reads them.
    plays: BTreeMap<String, (bool, f64)>,
    /// The runtime clock at which the last playing key lands.
    play_until: f64,
    resolver: Resolver,
    /// Per scroll node, per axis: the spring's target is where the wheel
    /// clamped it, its value how far the children are drawn slid.
    scrolls: BTreeMap<String, [Spring; 2]>,
    /// Scratch for the tree path of the node a walk is on: an unnamed node's
    /// key, built without a heap copy per node.
    path: String,
    /// Per text field: the selection's anchor and caret, in characters. They
    /// are equal when nothing is selected.
    sel: BTreeMap<String, (usize, usize)>,
    /// Per multi-line field: how far its lines are scrolled up, in units.
    text_scroll: BTreeMap<String, f64>,
    /// Per surface: what a widget keeps between frames that is not the
    /// caller's value -- a picker's hue at zero saturation, a drag value's
    /// half-typed text. Dropped with the surface.
    stash: BTreeMap<String, Box<dyn Any + Send>>,
    /// The host's clipboard, handed in with a paste key; and what a copy or
    /// cut asked to put back on it.
    pasted: Option<String>,
    copied: Option<String>,
    /// The host's clipboard, when it handed one in: see [`Clipboard`].
    board: Option<Box<dyn Clipboard>>,
    /// The text the input method is composing, and its cursor in bytes. It
    /// belongs to whatever holds the focus; only a `Commit` touches a value.
    preedit: Option<(String, Option<(usize, usize)>)>,
    /// What a field asked for as its caret area this frame: its id and the
    /// caret rect in the field's own space.
    ime_caret: Option<(String, Point, f64)>,
    /// The drag in flight and what it carries: the id that started it and a
    /// payload only the dropping caller knows the type of. Cleared the frame
    /// after the drop, taken by [`Ui::dropped_on`] before that.
    drag: Option<(String, Box<dyn Any + Send>)>,
    /// A press that landed on the same target within [`DOUBLE_CLICK`].
    double: Option<String>,
    last_press: Option<(String, f64)>,
    /// This frame's wheel, for [`Response::wheel`].
    wheel: Point,
    /// Where a button went down this frame, on a target or on nothing, for
    /// [`Ui::clicked_outside`].
    press_at: Option<Point>,
    focus: Option<String>,
    /// Gesture edges waiting for a frame that resolves. A frame that errors
    /// leaves them queued rather than dropping a host's `End`.
    edits: Vec<(String, Edit)>,
    /// The edges the last successful frame handed out, for [`Ui::edit`].
    delivered: Vec<(String, Edit)>,
    /// An `End` owed because the gesture was cancelled, not released.
    cancelled: Option<String>,
    keys: Vec<KeyPress>,
    typed: String,
    pointer: PointerInput,
    /// The hovered key and how long it has been hovered.
    hover: Option<(String, f64)>,
    /// The canvas shape the pointer is on: its node's key and the draw's
    /// tag. Latched while a gesture is held, so a drag that leaves the knot
    /// still reports the knot it grabbed.
    tagged: Option<(String, String)>,
    time: f64,
    /// The last frame resolved: see [`Ui::inert`].
    resolved: bool,
    /// Where along a held scrollbar's thumb the pointer took it, so the
    /// thumb follows the pointer from that point instead of jumping to it.
    bar_grab: f64,
    /// [`Ui::memo`] subtrees, by id.
    kept: HashMap<u64, Kept>,
    /// This `Ui`'s key in [`TREES`].
    me: u64,
    /// Memos the next build must run again, because something inside them
    /// moved; `hot_all` for every one.
    hot: BTreeSet<u64>,
    hot_all: bool,
    /// Memo roots the build under way has handed out.
    issued: usize,
    /// The memos whose closures are running, innermost last, each with where
    /// its reads start in `read`, and whether all it read was at rest.
    building: Vec<(u64, usize, bool)>,
    /// The tween and play ids the running closures read.
    read: Vec<String>,
}

fn same_hit_geometry(a: &ResolvedScene, b: &ResolvedScene) -> bool {
    // The scene's caches hand an unchanged outline back as the same Arc:
    // compare by pointer before walking the commands.
    fn same<'p>(
        a: impl ExactSizeIterator<Item = &'p Arc<mui_geometry::Path>>,
        mut b: impl ExactSizeIterator<Item = &'p Arc<mui_geometry::Path>>,
    ) -> bool {
        a.len() == b.len() && a.zip(&mut b).all(|(a, b)| Arc::ptr_eq(a, b) || a == b)
    }
    let mut a = a.surfaces();
    let mut b = b.surfaces();
    loop {
        match (a.next(), b.next()) {
            (None, None) => return true,
            (Some(a), Some(b))
                if a.key == b.key
                    && a.disabled == b.disabled
                    && a.pointer_states == b.pointer_states
                    && same([&a.path].into_iter(), [&b.path].into_iter())
                    && a.offset == b.offset
                    && a.clip == b.clip
                    && same(
                        a.clip_paths().unwrap_or(&[]).iter().map(|c| &c.0),
                        b.clip_paths().unwrap_or(&[]).iter().map(|c| &c.0),
                    )
                    && a.clip_paths().unwrap_or(&[]).iter().map(|c| c.1).eq(b
                        .clip_paths()
                        .unwrap_or(&[])
                        .iter()
                        .map(|c| c.1))
                    && a.hits.iter().map(|h| &h.0).eq(b.hits.iter().map(|h| &h.0))
                    && same(a.hits.iter().map(|h| &h.1), b.hits.iter().map(|h| &h.1)) => {}
            _ => return false,
        }
    }
}
impl Ui {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            font: None,
            fallback_fonts: Vec::new(),
            scale: None,
            weld_backend: mui_scene::WeldBackend::Reference,
            interaction: Pointer::new(),
            actions: Vec::new(),
            hit: Hit::default(),
            scene: None,
            springs: BTreeMap::new(),
            motion: BTreeMap::new(),
            glides: BTreeMap::new(),
            appearing: BTreeMap::new(),
            ghosts: Vec::new(),
            morphs: BTreeMap::new(),
            identities: HashMap::new(),
            tweens: BTreeMap::new(),
            plays: BTreeMap::new(),
            play_until: 0.0,
            resolver: Resolver::default(),
            scrolls: BTreeMap::new(),
            path: String::new(),
            sel: BTreeMap::new(),
            text_scroll: BTreeMap::new(),
            stash: BTreeMap::new(),
            pasted: None,
            copied: None,
            preedit: None,
            ime_caret: None,
            drag: None,
            double: None,
            double_click: DOUBLE_CLICK,
            wheel: Point::ZERO,
            press_at: None,
            last_press: None,
            focus: None,
            edits: Vec::new(),
            delivered: Vec::new(),
            cancelled: None,
            keys: Vec::new(),
            typed: String::new(),
            pointer: PointerInput::default(),
            hover: None,
            tagged: None,
            time: 0.0,
            resolved: false,
            board: None,
            bar_grab: 0.0,
            kept: HashMap::new(),
            me: NEXT_UI.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            hot: BTreeSet::new(),
            hot_all: false,
            issued: 0,
            building: Vec::new(),
            read: Vec::new(),
        }
    }
    /// Read and write the host's clipboard through `board`: copy, cut and
    /// paste in a text field then need nothing from the host. See
    /// [`Clipboard`].
    pub fn clipboard(mut self, board: impl Clipboard + 'static) -> Self {
        self.board = Some(Box::new(board));
        self
    }
    /// Set the font.
    pub fn font(mut self, font: Font) -> Self {
        self.font = Some(font);
        self
    }
    /// Add a font used when the primary face has no glyph for a grapheme.
    /// The selected face is retained in the scene text payload, so CPU and
    /// GPU renderers use the same fallback choice.
    pub fn fallback_font(mut self, font: Font) -> Self {
        self.fallback_fonts.push(font);
        self
    }
    /// Run `.weld(..)` / `weld!` on the analytic GPU backend: this `Ui`'s
    /// [`SceneSpec::weld_backend`](mui_scene::SceneSpec::weld_backend).
    pub fn gpu_welding(mut self) -> Self {
        self.weld_backend = mui_scene::WeldBackend::AnalyticGpu;
        self
    }
    /// Morph-only render update: keep the application declaration synchronized.
    /// Input reads the live scene's analytic predicate, so no hit-mask or closure
    /// has to be recreated on every animation tick.
    pub fn set_weld_morph(&mut self, id: &str, progress: f64) -> Result<bool, SceneError> {
        self.scene
            .as_mut()
            .ok_or(SceneError::UnsupportedWeld("no resolved scene"))?
            .set_weld_morph(id, progress)
    }
    pub fn set_weld_solid_material(
        &mut self,
        id: &str,
        index: usize,
        fill: Option<Color>,
        border: Option<Color>,
        width: f64,
    ) -> Result<bool, SceneError> {
        self.scene
            .as_mut()
            .ok_or(SceneError::UnsupportedWeld("no resolved scene"))?
            .set_weld_solid_material(id, index, fill, border, width)
    }
    pub fn set_weld_material_blend(&mut self, id: &str, blend: f64) -> Result<bool, SceneError> {
        self.scene
            .as_mut()
            .ok_or(SceneError::UnsupportedWeld("no resolved scene"))?
            .set_weld_material_blend(id, blend)
    }
    /// What the last frame resolved to, for anything drawn on top of it.
    /// Cache hits, misses, and conservative retained bytes for material welding.
    pub fn weld_cache_stats(&self) -> (u64, u64, usize) {
        let (hits, misses) = self.resolver.welds.stats();
        (hits, misses, self.resolver.welds.bytes())
    }
    /// Drop retained weld assets without changing live interaction state.
    pub fn clear_weld_cache(&mut self) {
        self.resolver.welds.clear();
    }

    pub fn layout_stats(&self) -> mui_layout::LayoutStats {
        self.resolver.layout_stats()
    }
    pub fn scene(&self) -> Option<&ResolvedScene> {
        self.scene.as_ref()
    }
    /// Swap what one text node says without resolving the tree again: the
    /// last frame's layout stands and only that node's glyphs are shaped.
    ///
    /// For readouts that change every frame -- a meter, a value under a
    /// knob -- where the tree is otherwise identical. Give the node a
    /// [`reserve`](mui_scene::Styled::reserve) string so the box was
    /// measured for the widest value it will ever hold.
    ///
    /// ponytail: takes `AsRef<str>`, not `Into<Arc<str>>` -- the string is
    /// shaped and dropped, never stored, so an `Arc` would only allocate.
    /// See [`ResolvedScene::set_text`] for the rest of the ceiling: one
    /// line. Accessible text follows the update; explicit labels are preserved.
    ///
    /// ```
    /// # use mui::prelude::*;
    /// let mut ui = Ui::new(Theme::DEFAULT).font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    /// let tree = row![text("0.0").reserve("-88.8").id("gain")];
    /// ui.frame(tree, Some(Size::new(200., 40.)), PointerInput::default(), 0.016).unwrap();
    /// ui.set_text("gain", "-12.4").unwrap();
    /// ```
    pub fn set_text(&mut self, id: &str, s: impl AsRef<str>) -> Result<(), mui_scene::SceneError> {
        self.scene
            .as_mut()
            .ok_or(mui_scene::SceneError::NoTextLayer)?
            .set_text(id, s.as_ref())
    }
    /// Drop the gesture in flight, for focus loss. The held target still
    /// gets its [`Edit::End`] on the next frame: a host that was told a
    /// gesture began must be told it ended.
    pub fn cancel(&mut self) {
        if let Some(id) = self.interaction.held().map(str::to_owned) {
            // Repeated cancellation must not erase an End already owed to the host.
            if let Some(previous) = self.cancelled.replace(id) {
                self.edits.push((previous, Edit::End));
            }
        }
        self.preedit = None;
        self.drag = None;
        self.tagged = None;
        self.interaction.cancel();
    }

    /// The first gesture edge on `id`, on the same frame boundary as [`Ui::get`].
    /// Use [`Ui::edits_for`] or [`Frame::edits`] for host dispatch: an atomic
    /// semantic action can begin AND end on the same frame.
    /// Pointer-only example:
    ///
    /// ```
    /// # use mui::{Edit, Ui}; use mui::prelude::*;
    /// # let mut ui = Ui::new(Theme::DEFAULT);
    /// # let mut cutoff = 0.5;
    /// match ui.edit("cutoff") {
    ///     Some(Edit::Begin) => { /* host.begin_gesture(CUTOFF) */ }
    ///     Some(Edit::End) => { /* host.end_gesture(CUTOFF) */ }
    ///     None => {}
    /// }
    /// let el = slider(&mut ui, "cutoff", "Cutoff", &mut cutoff, 0.0..=1.0).el;
    /// ```
    pub fn edit(&self, id: &str) -> Option<Edit> {
        self.delivered
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, e)| *e)
    }

    /// Last frame's gesture on `id`. Widgets read this while building the
    /// next tree, so a drag lands one frame late and nobody notices.
    ///
    /// The runtime fills in what the gesture machine cannot see: whether
    /// the press was a double click, the wheel while the pointer is inside
    /// `id`'s frame, and Enter or Space while `id` has the focus.
    ///
    /// ```
    /// # use mui::Ui; use mui::prelude::*;
    /// # let ui = Ui::new(Theme::DEFAULT);
    /// let r = ui.get("lane");
    /// let zoom = 1.0 - r.wheel.y * 0.01;
    /// if r.double_clicked { /* reset */ }
    /// # assert_eq!(zoom, 1.0);
    /// ```
    pub fn get(&self, id: &str) -> Response {
        let mut response = self.interaction.get(id);
        response.double_clicked = self.double.as_deref() == Some(id);
        response.key_activated = self
            .keys(id)
            .iter()
            .any(|k| matches!(k.key, Key::Enter | Key::Space));
        if self.wheel != Point::ZERO
            && let (Some(p), Some(s)) = (
                self.pointer.pos,
                self.scene.as_ref().and_then(|s| s.surface(id)),
            )
        {
            let f = s.frame;
            let inside = p.x >= f.x && p.x <= f.right() && p.y >= f.y && p.y <= f.bottom();
            let clipped = s
                .clip
                .is_some_and(|c| p.x < c.x0 || p.x > c.x1 || p.y < c.y0 || p.y > c.y1);
            if inside && !clipped {
                response.wheel = self.wheel;
            }
        }
        if self
            .actions
            .iter()
            .any(|a| matches!(a, SemanticAction::Activate { id: target } if target == id))
        {
            response.clicked = true;
            response.button = Some(mui_input::Button::Primary);
        }
        response
    }

    /// Accept an accessibility/platform action against the last presented scene.
    /// No pointer warp or bounding-box click is synthesized. Unknown, disabled,
    /// incompatible and non-finite requests are rejected without changing state.
    /// Custom controls must consume the matching action through `get`/`drag`.
    pub fn request_action(&mut self, action: SemanticAction) -> bool {
        let Some(surface) = self.scene.as_ref().and_then(|s| s.surface(action.id())) else {
            return false;
        };
        if surface.disabled {
            return false;
        }
        let role = surface.semantics.as_ref().map(|s| &s.role);
        let sign = if matches!(action, SemanticAction::Decrement { .. }) {
            -1.0
        } else {
            1.0
        };
        match action {
            SemanticAction::Focus { id } if surface.focusable => {
                self.focus(id);
                true
            }
            SemanticAction::Activate { id }
                if matches!(role, Some(A11y::Button | A11y::Toggle { .. })) =>
            {
                // One activation per control per frame, matching `Response::clicked`.
                if !self
                    .actions
                    .iter()
                    .any(|a| matches!(a, SemanticAction::Activate { id: old } if old == &id))
                {
                    self.actions.push(SemanticAction::Activate { id });
                }
                true
            }
            SemanticAction::SetValue { id, value } if value.is_finite() => {
                let Some(A11y::Slider { min, max, .. }) = role else {
                    return false;
                };
                if !(min.is_finite() && max.is_finite()) {
                    return false;
                }
                let value = value.clamp(min.min(*max), min.max(*max));
                // Coalesce repeated requests, while leaving pointer capture untouched.
                self.actions.retain(
                    |a| !matches!(a, SemanticAction::SetValue { id: old, .. } if old == &id),
                );
                self.actions.push(SemanticAction::SetValue { id, value });
                true
            }
            SemanticAction::Increment { id } | SemanticAction::Decrement { id } => {
                let Some(&A11y::Slider { value, min, max }) = role else {
                    return false;
                };
                // Two steps in one frame are two steps: step from the value
                // already queued, not twice from the presented one.
                let value = self
                    .actions
                    .iter()
                    .find_map(|a| match a {
                        SemanticAction::SetValue { id: old, value } if old == &id => Some(*value),
                        _ => None,
                    })
                    .unwrap_or(value);
                let step = crate::widgets::step(&(min..=max)) * sign;
                self.request_action(SemanticAction::SetValue {
                    id,
                    value: value + step,
                })
            }
            // Lands straight in the field's selection, which its next build
            // reads: nothing about it is an edit a host brackets.
            SemanticAction::SetSelection { id, anchor, caret } => {
                let Some(A11y::TextInput { value, .. }) = role else {
                    return false;
                };
                if anchor.max(caret) > value.chars().count() {
                    return false;
                }
                self.set_sel(&id, anchor, caret);
                true
            }
            _ => false,
        }
    }

    /// All gesture edges for a control. Unlike `edit`, this preserves an atomic
    /// Begin/End pair, such as one accessibility value change in one frame.
    pub fn edits_for<'a>(&'a self, id: &'a str) -> impl Iterator<Item = Edit> + 'a {
        self.delivered
            .iter()
            .filter(move |(k, _)| k == id)
            .map(|(_, e)| *e)
    }

    /// End an editor session without requiring another successful layout/frame.
    /// The host must dispatch the returned edges before destroying its editor.
    /// Calling this again returns no duplicate End events.
    pub fn close(&mut self) -> Vec<(String, Edit)> {
        self.cancel();
        self.edits
            .extend(self.cancelled.take().map(|id| (id, Edit::End)));
        self.actions.clear();
        self.focus = None;
        self.keys.clear();
        self.typed.clear();
        self.delivered.clear();
        std::mem::take(&mut self.edits)
    }
    /// The smallest the last resolved tree can be squeezed to, for a host that
    /// owns a window: a plugin refuses a resize below it. `None` before the
    /// first frame resolves.
    ///
    /// ```
    /// # use mui::Ui; use mui::prelude::*;
    /// # let mut ui = Ui::new(Theme::DEFAULT);
    /// # let tree = col![block(40., 30.).min_size(Size::new(40., 30.))].pad(8.);
    /// # ui.frame(tree, Some(Size::new(400., 300.)), PointerInput::default(), 0.016).unwrap();
    /// assert_eq!(ui.min_size(), Some(Size::new(56., 46.)));
    /// ```
    pub fn min_size(&self) -> Option<Size> {
        Some(self.scene.as_ref()?.layout.min_size())
    }
    /// The scene's font then its fallbacks, borrowed when there are none.
    fn fonts(&self) -> Option<std::borrow::Cow<'_, [Font]>> {
        let font = self.font.as_ref()?;
        Some(if self.fallback_fonts.is_empty() {
            std::slice::from_ref(font).into()
        } else {
            std::iter::once(font)
                .chain(&self.fallback_fonts)
                .cloned()
                .collect::<Vec<_>>()
                .into()
        })
    }
    /// The character index in `s` nearest `x`, measured in the scene's font.
    pub(crate) fn hit(&self, s: &str, size: f64, x: f64) -> usize {
        match self.fonts() {
            Some(fonts) => mui_text::hit_index(&fonts, s, size, &[], x)
                .map_or(0, |b| s[..b.min(s.len())].chars().count()),
            // ponytail: the same 0.6em guess `advance` falls back to.
            None => ((x / (size * 0.6)).round().max(0.0) as usize).min(s.chars().count()),
        }
    }
    /// Every char boundary of `s` with the x of a caret there, ascending by
    /// byte: the inverse of [`Ui::hit`], ending at the whole string's
    /// advance. Shaped once, however many carets a field asks for.
    pub(crate) fn carets(&self, s: &str, size: f64) -> Vec<(usize, f64)> {
        match self.fonts() {
            Some(fonts) => mui_text::caret_positions(&fonts, s, size, &[]).unwrap_or_default(),
            // ponytail: the 0.6em guess the scene itself falls back to
            // without a font; set a font and both agree.
            None => (s.char_indices().map(|(b, _)| b).chain([s.len()]))
                .enumerate()
                .map(|(i, b)| (b, i as f64 * size * 0.6))
                .collect(),
        }
    }
    /// `s` broken into lines no wider than `width`, as byte ranges: a
    /// `'\n'` always breaks and is in no range. What a multi-line field lays
    /// its rows out by.
    pub(crate) fn lines(&self, s: &str, size: f64, width: f64) -> Vec<std::ops::Range<usize>> {
        if let (Some(fonts), true) = (self.fonts(), width.is_finite() && width > 0.0)
            && let Ok(lines) = mui_text::break_lines(&fonts, s, size, &[], width)
        {
            return lines.into_iter().map(|l| l.text_range).collect();
        }
        // ponytail: without a font, hard breaks only; set a font to wrap.
        let mut at = 0;
        s.split('\n')
            .map(|l| {
                let r = at..at + l.len();
                at = r.end + 1;
                r
            })
            .collect()
    }
    /// The pitch one line of text at `size` is stacked at: the font's line
    /// height on the device grid, as the scene measures a text node.
    pub(crate) fn line_height(&self, size: f64) -> f64 {
        let h = self
            .fonts()
            .and_then(|fonts| mui_text::shape_run(&fonts, "M", size, &[]).ok())
            .map_or(size * 1.25, |r| r.line_height);
        match self.scale {
            Some(s) if s > 0.0 => (h * s).round() / s,
            _ => h,
        }
    }
    /// A caret is on for 0.625 s of every 1.25 s.
    pub(crate) fn blink(&self) -> bool {
        (self.time * 1.6) as i64 % 2 == 0
    }

    /// Advance gestures and springs, style the tree by state, resolve it.
    ///
    /// The phases run in this order, each reading what the one before left:
    /// commands and input in, capture reconciled against last frame's hit
    /// map, focus and keys, the tip, the state and motion sweep over the new
    /// tree, the resolve, then what the new scene says about the old state,
    /// and finally the commit.
    pub fn frame(
        &mut self,
        root: El,
        offered: Option<Size>,
        input: impl Into<Input>,
        dt: f64,
    ) -> Result<Frame<'_>, SceneError> {
        if !(dt.is_finite() && dt >= 0.0 && (self.time + dt).is_finite()) {
            return Err(SceneError::InvalidFrameDelta);
        }
        self.resolved = false;
        self.bracket_commands();
        let Input {
            pointer,
            wheel,
            keys,
            text,
            clipboard,
            ime,
        } = input.into();
        let was = std::mem::replace(&mut self.pointer, pointer).buttons;
        let was_buttons = was;
        // A non-finite delta reaches no widget, as it reaches no scroller.
        self.wheel = if wheel.x.is_finite() && wheel.y.is_finite() {
            wheel
        } else {
            Point::ZERO
        };
        // A paste key with no text handed in reads the host's clipboard, if
        // it gave one: nothing here reads it speculatively.
        let paste_key = keys
            .iter()
            .any(|k| matches!(k.key, Key::Char('v' | 'V')) && (k.mods.ctrl || k.mods.cmd));
        self.pasted = match (clipboard, self.board.as_mut()) {
            (None, Some(b)) if paste_key => b.get(),
            (c, _) => c,
        };
        let previous_blink = self.blink();
        self.time += dt;
        // A release is read by the *next* tree, so that frame must come even
        // when nothing is moving.
        // A key still to land wants the frame that shows it; the clock that
        // decides is the one this frame advances to.
        let was_hovered = self.interaction.hovered().map(str::to_owned);
        let before = [
            was_hovered.clone(),
            self.interaction.held().map(str::to_owned),
            self.tagged.as_ref().map(|t| t.0.clone()),
        ];
        // The kept memo subtrees go back in first: everything below reads
        // the whole tree.
        let mut root = root;
        let mut memos = self.splice(&mut root);
        let mut animating = self.reconcile() | (self.time < self.play_until);
        // The tree just built read last frame's hover: a new target is owed
        // the tree that knows it, even with no spring to carry it there.
        animating |= self.interaction.hovered() != was_hovered.as_deref();
        animating |= self.drag_bar();
        self.follow_identities(&root);
        animating |= self.hover_springs(&root, dt);
        self.intake_focus(was, keys, text, ime);
        let hovered = self.interaction.hovered().map(str::to_owned);
        let (tip, tip_pending) = self.tip_due(hovered.as_deref(), dt);
        animating |= tip_pending;
        if tip.is_some() && !root.is_container() {
            // The tip is about to wrap this leaf: everything moves down one.
            memos.iter_mut().for_each(|(p, _)| p.insert(0, 0));
        }
        let mut root = Self::wrap_tip(root, tip.as_ref());
        let (moving, shaped) = self.sweep(&mut root, dt);
        animating |= moving;
        let (mut scene, glided, root) = self.resolve(root, offered, dt)?;
        self.capture(root, &memos);
        animating |= glided;
        animating |= self.after_motion(&mut scene, shaped, dt);
        animating |= self.settle(&scene, wheel);
        self.heat(&scene, &before, self.pointer.buttons != was_buttons);
        Ok(self.commit(scene, hovered, tip, previous_blink, animating))
    }

    /// The caller has now built its tree and consumed these commands. Drain
    /// before resolution so a layout error cannot replay an activation.
    /// Gesture edges stay queued in `edits` until delivered by a frame or
    /// `close`.
    fn bracket_commands(&mut self) {
        let active = self.interaction.held().map(str::to_owned);
        let mut atomic = BTreeSet::new();
        // So are the keys the focused control just read: an activation or a
        // step is one edit, bracketed like the semantic action it mirrors.
        let keyed = self
            .focus
            .as_deref()
            .filter(|id| keyed_edit(self.scene.as_ref(), id, &self.keys))
            .map(str::to_owned);
        let ids = std::mem::take(&mut self.actions)
            .into_iter()
            .map(|action| action.id().to_owned())
            .chain(keyed);
        for id in ids {
            if active.as_deref() != Some(id.as_str()) && atomic.insert(id.clone()) {
                self.edits.push((id.clone(), Edit::Begin));
                self.edits.push((id, Edit::End));
            }
        }
    }

    /// Run this frame's pointer against last frame's hit map: drop a capture
    /// or focus whose target was switched off, move the capture, queue its
    /// edges and latch the tagged shape under a new press. Returns whether a
    /// capture was held coming in, which owes the next tree its release.
    fn reconcile(&mut self) -> bool {
        // Switched off mid-gesture: the hit map stopped reporting it when it
        // resolved disabled, and a drag must not outlive its target. The
        // cancel still owes the host an `Edit::End`.
        let off = |scene: Option<&ResolvedScene>, k: &str| {
            scene.and_then(|s| s.surface(k)).is_some_and(|s| s.disabled)
        };
        if self
            .interaction
            .held()
            .is_some_and(|k| off(self.scene.as_ref(), k))
        {
            self.cancel();
        }
        // The same for a focus it held when it was switched off: Tab already
        // steps over it, and a key stream into a dead node is worse.
        if self
            .focus
            .as_deref()
            .is_some_and(|k| off(self.scene.as_ref(), k))
        {
            self.focus = None;
        }
        let prev_held = self.interaction.held().map(str::to_owned);
        let last_scene = self.scene.as_ref();
        self.interaction
            .update_with(&self.hit, self.pointer, |key, tag, p| {
                if tag.is_some() {
                    return None;
                }
                last_scene
                    .and_then(|s| s.external_weld(key))
                    .map(|e| e.contains(p))
            });
        // A payload outlives its gesture by exactly the one frame the drop is
        // reported in -- the frame the target's tree reads it from.
        if self.interaction.held().is_none() && self.interaction.dropped().is_none() {
            self.drag = None;
        }
        let held = self.interaction.held().map(str::to_owned);
        // A gesture is exactly the span a target is captured for, so the two
        // edges are the two ends of that capture -- plus the one a `cancel`
        // stole before this frame could see it.
        // Extend rather than assign: an edge computed for a frame that then
        // failed to resolve stays queued for the next one that does.
        self.edits
            .extend(self.cancelled.take().map(|k| (k, Edit::End)));
        if prev_held != held {
            self.edits.extend(prev_held.clone().map(|k| (k, Edit::End)));
            self.edits.extend(held.clone().map(|k| (k, Edit::Begin)));
        }
        // Last frame's hit map and this frame's pointer, exactly as the
        // interaction above: a new capture latches the shape under the press.
        if held.is_none() || prev_held != held {
            self.tagged = self.pointer.pos.and_then(|p| {
                let (id, tag) = self.hit.at_tagged_with(p, |key, tag, p| {
                    if tag.is_some() {
                        return None;
                    }
                    self.scene
                        .as_ref()
                        .and_then(|s| s.external_weld(key))
                        .map(|e| e.contains(p))
                })?;
                Some((id.to_owned(), tag?.to_owned()))
            });
        }
        prev_held.is_some()
    }

    /// Carry state kept under a name to the node's new name when a node with
    /// the same identity comes back renamed or moved. Runs after this frame's
    /// pointer has been matched against last frame's names, so a drag that
    /// caused the reorder keeps its capture under the new name.
    fn follow_identities(&mut self, root: &El) {
        fn remap<V>(m: &mut BTreeMap<String, V>, to: &dyn Fn(&str) -> Option<String>) {
            *m = std::mem::take(m)
                .into_iter()
                .map(|(k, v)| (to(&k).unwrap_or(k), v))
                .collect();
        }
        // Child indices down to here: a path is spelled out only for a node
        // that has an identity, and most trees have none.
        fn visit(n: &El, at: &mut Vec<usize>, out: &mut HashMap<u64, (Option<String>, String)>) {
            if let Some(id) = n.payload().extras().identity {
                let mut path = String::new();
                at.iter().for_each(|&j| push_index(&mut path, j));
                out.insert(id, (n.key().map(str::to_owned), path));
            }
            for (j, c) in n.children().iter().enumerate() {
                at.push(j);
                visit(c, at, out);
                at.pop();
            }
        }
        let mut now = HashMap::new();
        visit(root, &mut Vec::new(), &mut now);
        let mut moves: Vec<(String, String)> = Vec::new();
        for (id, (key, path)) in &now {
            let Some((was_key, was_path)) = self.identities.get(id) else {
                continue;
            };
            if let (Some(a), Some(b)) = (was_key, key)
                && a != b
            {
                moves.push((a.clone(), b.clone()));
            }
            if was_path != path {
                moves.push((was_path.clone(), path.clone()));
            }
        }
        self.identities = now;
        if moves.is_empty() {
            return;
        }
        // The deepest match wins: a renamed slot inside a renamed rack.
        moves.sort_by_key(|m| std::cmp::Reverse(m.0.len()));
        let to = |k: &str| -> Option<String> {
            moves.iter().find_map(|(a, b)| {
                if k == a {
                    return Some(b.clone());
                }
                // A name prefixes the names composed under it, a path the
                // paths below it; "osc/30" is not under "osc/3".
                let rest = k.strip_prefix(a.as_str())?;
                rest.starts_with('/').then(|| format!("{b}{rest}"))
            })
        };
        remap(&mut self.springs, &to);
        remap(&mut self.motion, &to);
        remap(&mut self.glides, &to);
        remap(&mut self.morphs, &to);
        remap(&mut self.scrolls, &to);
        remap(&mut self.sel, &to);
        remap(&mut self.appearing, &to);
        for k in [&mut self.focus, &mut self.double] {
            if let Some(new) = k.as_deref().and_then(to) {
                *k = Some(new);
            }
        }
        for k in [
            self.hover.as_mut().map(|h| &mut h.0),
            self.tagged.as_mut().map(|t| &mut t.0),
            self.drag.as_mut().map(|d| &mut d.0),
            self.last_press.as_mut().map(|p| &mut p.0),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(new) = to(k) {
                *k = new;
            }
        }
        self.interaction.rename(&to);
    }

    /// Resolve the styled tree, and rebuild the hit map when the hit
    /// geometry changed.
    fn resolve(
        &mut self,
        root: El,
        offered: Option<Size>,
        dt: f64,
    ) -> Result<(ResolvedScene, bool, El), SceneError> {
        let mut spec = SceneSpec::new(root).theme(self.theme);
        spec.offered = offered;
        spec.font = self.font.clone();
        spec.fallback_fonts = self.fallback_fonts.clone();
        spec.device_scale = self.scale;
        spec.weld_backend = self.weld_backend;
        let mut glided = false;
        let glides = &mut self.glides;
        let scene = self.resolver.resolve_animated(
            &spec,
            &mut |key, e, target| {
                let spring = e.extras().layout_transition.unwrap_or(Spring::DEFAULT);
                let t = [target.x, target.y, target.size.width, target.size.height];
                let (seen, s) = slot(glides, key, || (false, enter(spring, t, e.extras().appear)));
                *seen = true;
                for (s, t) in s.iter_mut().zip(t) {
                    s.to(t);
                    glided |= s.step(dt);
                }
                mui_scene::Frame {
                    x: s[0].value,
                    y: s[1].value,
                    size: Size::new(s[2].value, s[3].value),
                }
            },
            self.scene.as_ref(),
        )?;
        if self
            .scene
            .as_ref()
            .is_none_or(|previous| !same_hit_geometry(previous, &scene))
        {
            // Named nodes are the gesture targets, in z-order, and so is an
            // unnamed one that declared a hover or press look, by its tree
            // path: it routes exactly as it would with an id. Every other
            // unnamed node, the root included, is decoration the pointer
            // passes through. A target clipped away does not respond.
            //
            // ponytail: a capture an unnamed node takes on the frame a tip
            // wraps or unwraps the root keeps its old path; it releases
            // normally but reports no click. Key positional targets by
            // something stabler than the path if that ever shows.
            // The last map's conversions stay: a moved outline is the same
            // `Arc`, so only new shapes convert.
            let mut hit = std::mem::take(&mut self.hit);
            hit.clear();
            for s in scene
                .surfaces()
                .filter(|s| (named(&s.key) || s.pointer_states) && !s.disabled)
            {
                let key = || s.key.clone();
                if s.hits.is_empty() {
                    hit.push_placed(key(), None, &s.path, s.offset, s.clip, s.clip_paths())?;
                }
                // A canvas that named its draws is hit by those shapes instead
                // of by its frame, so a ring responds in the ring, not its hole.
                for (tag, path) in &s.hits {
                    let tag = Some(tag.clone());
                    hit.push_placed(key(), tag, path, s.offset, s.clip, s.clip_paths())?;
                }
            }
            self.hit = hit;
        }
        Ok((scene, glided, spec.root))
    }

    /// Keep the scene, hand out the edges, and say what the host should do
    /// next: the cursor, the caret area, where the tip landed, and whether
    /// to schedule another frame.
    fn commit(
        &mut self,
        scene: ResolvedScene,
        hovered: Option<String>,
        tip: Option<(Arc<str>, String)>,
        previous_blink: bool,
        mut animating: bool,
    ) -> Frame<'_> {
        let held = self.interaction.held().map(str::to_owned);
        let cursor = held
            .clone()
            .or(hovered)
            .and_then(|k| scene.surface(&k).and_then(|s| s.cursor))
            .unwrap_or(Cursor::Arrow);
        let cursor = match cursor {
            Cursor::Grab if held.is_some() => Cursor::Grabbing,
            c => c,
        };

        // The caret area a field asked for, moved into the scene's space.
        let ime = self.ime_caret.take().and_then(|(id, at, h)| {
            let f = scene.surface(&id)?.frame;
            Some((Point::new(f.x + at.x, f.y + at.y), Size::new(1.0, h)))
        });
        // Where the pin actually put it, so a host placing its own tooltip
        // window agrees with the one in the scene.
        let tip = tip.and_then(|(t, _)| {
            let f = scene.surface(TIP_KEY)?.frame;
            Some((t.to_string(), Point::new(f.x, f.y)))
        });
        // A reused memo's nodes were not visited, and are still there.
        let kept = |k: &str| scene.memos_at(k).any(|(_, reused)| reused);
        self.motion
            .retain(|k, (seen, _)| std::mem::take(seen) || kept(k));
        self.glides
            .retain(|k, (seen, _)| std::mem::take(seen) || kept(k));
        self.tweens.retain(|_, (seen, _)| std::mem::take(seen));
        self.plays.retain(|_, (seen, _)| std::mem::take(seen));
        self.delivered = std::mem::take(&mut self.edits);
        if let Some(old) = self.scene.replace(scene) {
            self.resolver.recycle(old);
        }
        let repaint_after = self.repaint_after();
        // Widgets read the clock while constructing the tree, before this frame
        // advances it. One catch-up frame makes the blink edge visible now,
        // rather than one whole half-period late. It then sleeps again.
        animating |= previous_blink != self.blink()
            && self
                .focus
                .as_deref()
                .and_then(|key| self.scene.as_ref()?.surface(key))
                .is_some_and(|surface| {
                    !surface.disabled
                        && matches!(
                            surface.semantics.as_ref().map(|sem| &sem.role),
                            Some(A11y::TextInput { .. })
                        )
                });
        self.resolved = true;
        Frame {
            scene: self.scene.as_ref().expect("just set"),
            animating,
            repaint_after,
            tip,
            cursor,
            edits: self.delivered.clone(),
            clipboard: match (self.copied.take(), self.board.as_mut()) {
                (Some(s), Some(b)) => {
                    b.set(&s);
                    None
                }
                (s, _) => s,
            },
            ime,
        }
    }
}

/// `map[k]`, inserted by `new` when absent: the key reaches the heap once, on
/// insertion, and not on every frame's lookup.
fn slot<'m, V>(map: &'m mut BTreeMap<String, V>, k: &str, new: impl FnOnce() -> V) -> &'m mut V {
    if !map.contains_key(k) {
        map.insert(k.to_owned(), new());
    }
    map.get_mut(k).expect("inserted above")
}

/// root) or a key the runtime owns (`/tip`).
fn named(k: &str) -> bool {
    mui_scene::Id::is_named(k)
}

/// The node `key` names in `root`: an id anywhere in the tree, or a tree
/// path walked by index.
fn find<'a>(root: &'a El, key: &str) -> Option<&'a El> {
    fn by_id<'a>(n: &'a El, id: &str) -> Option<&'a El> {
        if n.key() == Some(id) {
            return Some(n);
        }
        n.children().iter().find_map(|c| by_id(c, id))
    }
    if named(key) {
        return by_id(root, key);
    }
    key.split('/')
        .skip(1)
        .try_fold(root, |n, i| n.children().get(i.parse::<usize>().ok()?))
}
impl std::fmt::Debug for Ui {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ui")
            .field("springs", &self.springs.len())
            .finish()
    }
}

#[cfg(test)]
mod tests;
