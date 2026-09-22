//! The per-frame runtime: gestures in, animated styles applied, scene out.
use std::any::Any;
#[path = "wake.rs"]
mod wake;
use crate::SemanticAction;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use mui_geometry::Point;
use mui_input::{Hit, Ime, Input, Interaction, Key, KeyPress, PointerInput, Response, FINE_DRAG};
use mui_layout::SpacingToken::{Xs, S};
use mui_scene::prelude::{overlay, text, Paints as _, Role};
use mui_scene::{
    Area, Color, Cursor, El, Element, Fill, Kind, Paint, Palette, Pin, Radius, ResolvedScene,
    SceneError, SceneSpec, Size, Spacing, Spring, State, TextCache, Theme,
};
use mui_widgets::Host;

#[cfg(test)]
#[path = "audit_tests.rs"]
mod audit_tests;

/// The id the floated tip carries. A leading `/` keeps it out of hit
/// testing, like every other key the runtime owns.
const TIP_KEY: &str = "/tip";

/// How long the pointer must rest on a surface before its tip is due.
pub const TIP_DELAY: f64 = 0.5;
/// Two presses on one target within this are a double click.
pub const DOUBLE_CLICK: f64 = 0.4;

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
    /// A copy or cut asked for this: put it on the host's clipboard.
    pub clipboard: Option<String>,
    /// The caret of the field that wants an input method, in scene units.
    /// `Some` means "allow IME and put the candidate window here"; `None`
    /// means no field is composing-capable, so switch IME off.
    pub ime: Option<(Point, Size)>,
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
    pub font: Option<Arc<[u8]>>,
    /// Optional faces tried per grapheme after [`Self::font`].
    pub fallback_fonts: Vec<Arc<[u8]>>,
    /// The window's device pixels per logical unit. Set it and every painted
    /// edge lands on a device pixel; `None` paints on layout's raw f64.
    pub scale: Option<f64>,
    pub weld_backend: mui_scene::WeldBackend,
    interaction: Interaction,
    actions: Vec<SemanticAction>,
    hit: Hit,
    scene: Option<ResolvedScene>,
    /// Per key: hover and press springs, 0..1.
    springs: BTreeMap<String, [Spring; 2]>,
    /// Transition springs per node id, one per paint channel, and tweens
    /// under a `~` prefix so a widget id cannot collide with one.
    motion: BTreeMap<String, Vec<Option<Spring>>>,
    /// Tweens read by the builder and transitions visited by the current frame.
    motion_seen: BTreeSet<String>,
    text_cache: TextCache,
    weld_cache: mui_scene::WeldCache,
    /// Per scroll node: how far its children are slid.
    scrolls: BTreeMap<String, [f64; 2]>,
    /// Per text field: the selection's anchor and caret, in characters. They
    /// are equal when nothing is selected.
    sel: BTreeMap<String, (usize, usize)>,
    /// The host's clipboard, handed in with a paste key; and what a copy or
    /// cut asked to put back on it.
    pasted: Option<String>,
    copied: Option<String>,
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
}

fn same_hit_geometry(a: &ResolvedScene, b: &ResolvedScene) -> bool {
    let mut a = a.surfaces();
    let mut b = b.surfaces();
    loop {
        match (a.next(), b.next()) {
            (None, None) => return true,
            (Some(a), Some(b))
                if a.key == b.key
                    && a.disabled == b.disabled
                    && a.path == b.path
                    && a.clip == b.clip
                    && a.clip_paths() == b.clip_paths()
                    && a.hits == b.hits => {}
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
            interaction: Interaction::new(),
            actions: Vec::new(),
            hit: Hit::default(),
            scene: None,
            springs: BTreeMap::new(),
            motion: BTreeMap::new(),
            motion_seen: BTreeSet::new(),
            text_cache: TextCache::default(),
            weld_cache: mui_scene::WeldCache::default(),
            scrolls: BTreeMap::new(),
            sel: BTreeMap::new(),
            pasted: None,
            copied: None,
            preedit: None,
            ime_caret: None,
            drag: None,
            double: None,
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
        }
    }
    /// Set the font blob. Bytes no font parser accepts are dropped: the
    /// fontless path is measured and drawn, a bad blob is not.
    pub fn font(mut self, font: impl Into<Arc<[u8]>>) -> Self {
        let bytes = font.into();
        self.font = mui_text::axes(&bytes).is_ok().then_some(bytes);
        self
    }
    /// Add a font used when the primary face has no glyph for a grapheme.
    /// The selected face is retained in the scene text payload, so CPU and
    /// GPU renderers use the same fallback choice.
    pub fn fallback_font(mut self, font: impl Into<Arc<[u8]>>) -> Self {
        let bytes = font.into();
        if mui_text::axes(&bytes).is_ok() {
            self.fallback_fonts.push(bytes);
        }
        self
    }
    /// Use the analytic GPU backend for `.weld_with` / `weld!` by default.
    /// A node may opt into `.reference_weld` for offline-only general contours.
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
        let (hits, misses) = self.weld_cache.stats();
        (hits, misses, self.weld_cache.bytes())
    }
    /// Drop retained weld assets without changing live interaction state.
    pub fn clear_weld_cache(&mut self) {
        self.weld_cache.clear();
    }

    pub fn layout_stats(&self) -> mui_layout::LayoutStats {
        self.text_cache.layout_stats()
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
    /// let mut ui = Ui::new(Theme::DEFAULT).font(epaint_default_fonts::HACK_REGULAR.to_vec());
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
    /// let el = slider(&mut ui, "cutoff", "Cutoff", &mut cutoff, 0.0..=1.0).el();
    /// ```
    pub fn edit(&self, id: &str) -> Option<Edit> {
        self.delivered
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, e)| *e)
    }

    /// A keyed spring anyone can read while building the tree: pass the value
    /// you want, get the value to draw. A knob's sweep drawn from
    /// `ui.tween("cutoff", v)` glides when a preset changes it and still
    /// tracks a drag, because the spring is retargeted, never restarted.
    /// First call returns `target`, so nothing flies in from zero.
    pub fn tween(&mut self, id: &str, target: f64) -> f64 {
        self.tween_with(id, target, Spring::DEFAULT)
    }
    /// [`Ui::tween`] with your own spring. The spring's shape is taken on
    /// the first call for `id`.
    pub fn tween_with(&mut self, id: &str, target: f64, spring: Spring) -> f64 {
        let key = format!("~{id}");
        self.motion_seen.insert(key.clone());
        let s = self
            .motion
            .entry(key)
            .or_insert_with(|| vec![Some(spring.seeded(target))]);
        let s = s[0].get_or_insert_with(|| spring.seeded(target));
        s.to(target);
        s.value
    }
    /// Last frame's gesture on `id`. Widgets read this while building the
    /// next tree, so a drag lands one frame late and nobody notices.
    pub fn get(&self, id: &str) -> Response {
        let mut response = self.interaction.get(id);
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
        match action {
            SemanticAction::Focus { id } if surface.focusable => {
                self.focus(id);
                true
            }
            SemanticAction::Activate { id }
                if matches!(role, Some(Kind::Button | Kind::Toggle { .. })) =>
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
                let Some(Kind::Slider { min, max, .. }) = role else {
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
    /// Which shape of the canvas `id` the pointer is on, by the tag its
    /// [`Draw`](mui_scene::Draw) carried. `None` when the pointer is over no
    /// tagged shape of that node -- including inside its frame but outside
    /// every drawn path.
    ///
    /// Latched at the press: through a drag it stays the shape the gesture
    /// grabbed, so a knot dragged past its neighbours is still that knot.
    ///
    /// ```
    /// # use mui::Ui; use mui::prelude::*;
    /// # let mut ui = Ui::new(Theme::DEFAULT);
    /// let plot = canvas(|size| {
    ///     let box_ = [(0., 0.), (size.width, 0.), (size.width, size.height)];
    ///     vec![Draw::fill(Path::polyline(box_.map(|(x, y)| Point::new(x, y)), true), Primary)
    ///         .tag("wedge")]
    /// })
    /// .square(80.)
    /// .id("plot");
    /// ui.frame(plot, None, PointerInput::default(), 0.016).unwrap();
    /// assert_eq!(ui.tag("plot"), None, "the pointer is nowhere");
    /// ```
    pub fn tag(&self, id: &str) -> Option<&str> {
        let (k, t) = self.tagged.as_ref()?;
        (k == id).then_some(t.as_str())
    }
    /// Hover and press amounts for `id`, 0..1 and spring-smoothed.
    pub fn state(&self, id: &str) -> (f64, f64) {
        self.springs
            .get(id)
            .map_or((0.0, 0.0), |[h, p]| (h.value, p.value))
    }
    /// The id that holds the keyboard focus, for a host reporting it.
    pub fn focus_key(&self) -> Option<&str> {
        self.focus.as_deref()
    }
    /// Whether `id` holds the keyboard focus.
    pub fn focused(&self, id: &str) -> bool {
        self.focus.as_deref() == Some(id)
    }
    /// Focus `id` from code. No check that it exists: it may not have been
    /// built yet.
    pub fn focus(&mut self, id: impl Into<String>) {
        // A composition belongs to the field that started it.
        self.preedit = None;
        self.focus = Some(id.into());
    }
    /// The keys this frame, if `id` is focused. Empty otherwise, so a widget
    /// may loop over it unconditionally.
    pub fn keys(&self, id: &str) -> &[KeyPress] {
        if self.focused(id) {
            &self.keys
        } else {
            &[]
        }
    }
    /// Every key this frame, whatever holds the focus: the stream a global
    /// shortcut reads. `Ui` has already taken Tab and Escape for focus, and
    /// the keys are here as well as in [`Ui::keys`] -- a shortcut and a
    /// focused widget see the same press.
    ///
    /// The one exception is the rule every editor has: **a focused text
    /// input consumes the stream**, so typing `z` in a search box is a `z`
    /// and not an undo. Nothing else swallows keys; a widget that wants to
    /// claim a key while focused must check [`Ui::focused`] itself.
    ///
    /// ```
    /// # use mui::Ui; use mui::prelude::*;
    /// # let mut ui = Ui::new(Theme::DEFAULT);
    /// # let root = leaf(10., 10.).id("root");
    /// # ui.frame(root, None, PointerInput::default(), 0.016).unwrap();
    /// let undo = ui
    ///     .shortcuts()
    ///     .iter()
    ///     .any(|k| k.key == Key::Char('z') && (k.mods.ctrl || k.mods.cmd));
    /// assert!(!undo, "nothing was pressed");
    /// ```
    pub fn shortcuts(&self) -> &[KeyPress] {
        let typing = self.focus.as_deref().is_some_and(|k| {
            self.scene
                .as_ref()
                .and_then(|s| s.surface(k))
                .is_some_and(|s| {
                    matches!(
                        s.semantics.as_ref().map(|s| &s.role),
                        Some(Kind::TextInput { .. })
                    )
                })
        });
        if typing {
            &[]
        } else {
            &self.keys
        }
    }
    /// The text typed this frame, if `id` is focused.
    pub fn text(&self, id: &str) -> &str {
        if self.focused(id) {
            &self.typed
        } else {
            ""
        }
    }
    /// The pointer relative to `id`'s frame origin, if both exist.
    pub fn local(&self, id: &str) -> Option<Point> {
        let p = self.pointer.pos?;
        let s = self.scene.as_ref()?.surface(id)?;
        Some(Point::new(p.x - s.frame.x, p.y - s.frame.y))
    }
    /// Source and target of a drag released this frame.
    pub fn dropped(&self) -> Option<(&str, &str)> {
        self.interaction.dropped()
    }
    /// Attach a payload to the drag `id` has in flight: the value the drop
    /// target will be handed. Call it while the gesture is dragging -- a
    /// second call replaces what the first attached, so a widget may simply
    /// set it every frame.
    ///
    /// The payload is the caller's type, not MUI's. A drag *ghost* is the
    /// caller's too: pin an `El` to the pointer's surface and `.float()` it.
    ///
    /// ```
    /// # use mui::Ui; use mui::prelude::*;
    /// # let mut ui = Ui::new(Theme::DEFAULT);
    /// struct Wave(&'static str);
    /// if ui.get("saw").dragged {
    ///     ui.start_drag("saw", Wave("saw"));
    /// }
    /// assert!(ui.dragging::<Wave>().is_none(), "nothing is dragging");
    /// ```
    pub fn start_drag(&mut self, id: &str, payload: impl Any + Send) {
        self.drag = Some((id.to_owned(), Box::new(payload)));
    }
    /// The payload of the drag in flight, for anything that wants to look
    /// before it lands: a drop target that highlights only for a payload it
    /// accepts, a ghost that draws what is being carried. `None` once the
    /// pointer is released, or when the payload is not a `T`.
    ///
    /// ```
    /// # use mui::Ui; use mui::prelude::*;
    /// # let ui = Ui::new(Theme::DEFAULT);
    /// struct Wave(&'static str);
    /// assert!(ui.dragging::<Wave>().is_none());
    /// ```
    pub fn dragging<T: Any>(&self) -> Option<&T> {
        self.interaction.held()?;
        self.drag.as_ref()?.1.downcast_ref::<T>()
    }
    /// Take the payload of a drag released over `id` this frame. Delivered
    /// exactly once: the next call, on this frame or any later one, is
    /// `None`. A release anywhere else delivers nothing to `id`, and a
    /// payload of another type is left in place for whoever wants it.
    ///
    /// ```
    /// # use mui::Ui; use mui::prelude::*;
    /// # let mut ui = Ui::new(Theme::DEFAULT);
    /// struct Wave(&'static str);
    /// let mut slot: Option<&'static str> = None;
    /// if let Some(Wave(w)) = ui.dropped_on::<Wave>("slot-0") {
    ///     slot = Some(w);
    /// }
    /// assert_eq!(slot, None, "nothing was dropped");
    /// ```
    pub fn dropped_on<T: Any>(&mut self, id: &str) -> Option<T> {
        let from = self.drag.as_ref().map(|(k, _)| k.as_str());
        if !self
            .interaction
            .dropped()
            .is_some_and(|(src, target)| target == id && from == Some(src))
        {
            return None;
        }
        let (src, payload) = self.drag.take()?;
        match payload.downcast::<T>() {
            Ok(v) => Some(*v),
            Err(payload) => {
                self.drag = Some((src, payload));
                None
            }
        }
    }
    /// The smallest the last resolved tree can be squeezed to, for a host that
    /// owns a window: a plugin refuses a resize below it. `None` before the
    /// first frame resolves.
    ///
    /// ```
    /// # use mui::Ui; use mui::prelude::*;
    /// # let mut ui = Ui::new(Theme::DEFAULT);
    /// # let tree = col![leaf(40., 30.).min_size(Size::new(40., 30.))].pad(8.);
    /// # ui.frame(tree, Some(Size::new(400., 300.)), PointerInput::default(), 0.016).unwrap();
    /// assert_eq!(ui.min_size(), Some(Size::new(56., 46.)));
    /// ```
    pub fn min_size(&self) -> Option<Size> {
        Some(self.scene.as_ref()?.layout.min_size())
    }
    /// How far `id`'s children are scrolled.
    pub fn scroll(&self, id: &str) -> [f64; 2] {
        self.scrolls.get(id).copied().unwrap_or([0.0, 0.0])
    }

    pub(crate) fn sel(&self, id: &str) -> (usize, usize) {
        self.sel.get(id).copied().unwrap_or((0, 0))
    }
    pub(crate) fn set_sel(&mut self, id: &str, anchor: usize, caret: usize) {
        self.sel.insert(id.to_owned(), (anchor, caret));
    }
    /// The clipboard the host handed in because a paste key arrived.
    pub(crate) fn pasted(&self) -> Option<&str> {
        self.pasted.as_deref()
    }
    /// Whether the last press on `id` was the second of a double click.
    pub(crate) fn double_click(&self, id: &str) -> bool {
        self.double.as_deref() == Some(id)
    }
    /// The text the input method is composing, for the focused field to
    /// paint. It is never part of a value.
    pub fn preedit(&self) -> Option<(&str, Option<(usize, usize)>)> {
        self.preedit.as_ref().map(|(t, c)| (t.as_str(), *c))
    }
    /// Tell the host where this field's caret is, in the field's own space,
    /// so the IME candidate window lands under it. Comes back on
    /// [`Frame::ime`].
    pub fn set_ime_caret(&mut self, id: &str, at: Point, height: f64) {
        self.ime_caret = Some((id.to_owned(), at, height));
    }
    /// Ask the host to put `s` on the clipboard: it comes back on the next
    /// frame's [`Frame::clipboard`].
    pub fn set_clipboard(&mut self, s: impl Into<String>) {
        self.copied = Some(s.into());
    }
    /// The character index in `s` nearest `x`, measured in the scene's font.
    pub(crate) fn hit(&self, s: &str, size: f64, x: f64) -> usize {
        match self.font.as_deref() {
            Some(f) => {
                let byte = if self.fallback_fonts.is_empty() {
                    mui_text::hit_index(f, s, size, x)
                } else {
                    let mut fonts = Vec::with_capacity(1 + self.fallback_fonts.len());
                    fonts.push(f);
                    fonts.extend(self.fallback_fonts.iter().map(AsRef::as_ref));
                    mui_text::fallback_hit_index(&fonts, s, size, x)
                };
                byte.map_or(0, |b| s[..b.min(s.len())].chars().count())
            }
            // ponytail: the same 0.6em guess `advance` falls back to.
            None => ((x / (size * 0.6)).round().max(0.0) as usize).min(s.chars().count()),
        }
    }
    /// Where the caret sits when it is `byte` bytes into `s`: the inverse of
    /// [`Ui::hit`], and the advance of the whole string when `byte == s.len()`.
    /// Measured, not shaped -- `mui_text::caret_x` reads advances only, where
    /// `text_run` would build every outline to throw them away.
    pub(crate) fn caret_x(&self, s: &str, size: f64, byte: usize) -> f64 {
        match self.font.as_deref() {
            Some(f) => {
                if self.fallback_fonts.is_empty() {
                    mui_text::caret_x(f, s, size, byte).unwrap_or(0.0)
                } else {
                    let mut fonts = Vec::with_capacity(1 + self.fallback_fonts.len());
                    fonts.push(f);
                    fonts.extend(self.fallback_fonts.iter().map(AsRef::as_ref));
                    mui_text::fallback_caret_x(&fonts, s, size, byte).unwrap_or(0.0)
                }
            }
            // ponytail: the 0.6em guess the scene itself falls back to
            // without a font; set a font and both agree.
            None => s[..byte.min(s.len())].chars().count() as f64 * size * 0.6,
        }
    }
    /// A caret is on for 0.625 s of every 1.25 s.
    pub(crate) fn blink(&self) -> bool {
        (self.time * 1.6) as i64 % 2 == 0
    }

    /// Move the focus to the next (or previous) focusable surface in z-order,
    /// wrapping. Nothing focused yet starts at either end.
    fn cycle_focus(&mut self, back: bool) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let stops: Vec<String> = scene
            .surfaces()
            .filter(|s| s.focusable && !s.disabled)
            .map(|s| s.key.to_string())
            .collect();
        if stops.is_empty() {
            return;
        }
        let at = self
            .focus
            .as_ref()
            .and_then(|c| stops.iter().position(|k| k == c));
        let next = match (at, back) {
            (Some(i), false) => (i + 1) % stops.len(),
            (Some(i), true) => (i + stops.len() - 1) % stops.len(),
            (None, false) => 0,
            (None, true) => stops.len() - 1,
        };
        self.focus = Some(stops[next].clone());
    }

    /// Apply a horizontal or vertical drag on `id` to `value` across `range`,
    /// `px` pixels for the full span. Returns whether it changed.
    ///
    /// Shift is the fine modifier: the same travel moves a tenth as far
    /// ([`mui_input::FINE_DRAG`]), which is how every parameter in a synth
    /// editor is dialled in.
    ///
    /// ```
    /// # use mui::Ui; use mui::prelude::*;
    /// # let ui = Ui::new(Theme::DEFAULT);
    /// let mut cutoff = 0.5;
    /// // Nothing is dragging, so nothing moves.
    /// assert!(!ui.drag("cutoff", &mut cutoff, 0.0..=1.0, 160.0, false));
    /// ```
    pub fn drag(
        &self,
        id: &str,
        value: &mut f64,
        range: std::ops::RangeInclusive<f64>,
        px: f64,
        vertical: bool,
    ) -> bool {
        if !(range.start().is_finite() && range.end().is_finite()) {
            return false;
        }
        if let Some(value_from_action) = self.actions.iter().rev().find_map(|a| match a {
            SemanticAction::SetValue { id: target, value } if target == id => Some(*value),
            _ => None,
        }) {
            let next = value_from_action.clamp(
                range.start().min(*range.end()),
                range.start().max(*range.end()),
            );
            let changed = next != *value;
            *value = next;
            return changed;
        }
        let r = self.get(id);
        if !r.dragged || !px.is_finite() || px <= 0.0 || !value.is_finite() {
            return false;
        }
        let delta = r.drag_fine(FINE_DRAG);
        let d = if vertical { -delta.y } else { delta.x };
        if !d.is_finite() {
            return false;
        }
        // An inverted range (`1.0..=0.0`) is a legitimate downward control and
        // the delta math already reverses for it; only `clamp` needs the
        // bounds in order, since it panics on `min > max`.
        let next = (*value + d / px * (range.end() - range.start())).clamp(
            range.start().min(*range.end()),
            range.start().max(*range.end()),
        );
        let changed = next != *value;
        *value = next;
        changed
    }

    /// Advance gestures and springs, style the tree by state, resolve it.
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
        // The caller has now built its tree and consumed these commands. Drain
        // before resolution so a layout error cannot replay an activation. Gesture
        // edges stay queued in `edits` until delivered by a frame or `close`.
        let active = self.interaction.held().map(str::to_owned);
        let mut atomic = BTreeSet::new();
        for action in std::mem::take(&mut self.actions) {
            let id = action.id().to_owned();
            if active.as_deref() != Some(id.as_str()) && atomic.insert(id.clone()) {
                self.edits.push((id.clone(), Edit::Begin));
                self.edits.push((id, Edit::End));
            }
        }
        let input = input.into();
        self.pointer = input.pointer;
        self.pasted = input.clipboard;
        let previous_blink = self.blink();
        self.time += dt;
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
            .update_with(&self.hit, input.pointer, |key, tag, p| {
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
        let (hovered, held) = (
            self.interaction.hovered().map(str::to_owned),
            self.interaction.held().map(str::to_owned),
        );
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
            self.tagged = input.pointer.pos.and_then(|p| {
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
        // Identity and state ownership are separate. A named layout surface
        // still participates in hit testing, while only an interactive role
        // or an explicitly declared hover/press look earns springs. Resolve
        // the two possible active targets directly so idle frames do not
        // allocate a policy table for the whole tree.
        let hovered_policy = hovered
            .as_deref()
            .map(|id| state_policy(&root, id))
            .unwrap_or([false, false]);
        let held_policy = held
            .as_deref()
            .map(|id| state_policy(&root, id))
            .unwrap_or([false, false]);
        for (k, [h, p]) in &mut self.springs {
            let hovered = hovered.as_deref() == Some(k);
            let held = held.as_deref() == Some(k);
            h.to(f64::from(
                (hovered && hovered_policy[0]) || (held && held_policy[0]),
            ));
            p.to(f64::from(held && held_policy[1]));
        }
        if let Some(k) = hovered.as_deref().filter(|_| hovered_policy[0]) {
            self.springs
                .entry(k.to_owned())
                .or_insert_with(|| [Spring::at(0.0), Spring::at(0.0)])[0]
                .to(1.0);
        }
        if let Some(k) = held.as_deref() {
            let entry = self
                .springs
                .entry(k.to_owned())
                .or_insert_with(|| [Spring::at(0.0), Spring::at(0.0)]);
            if held_policy[0] {
                entry[0].to(1.0);
            }
            if held_policy[1] {
                entry[1].to(1.0);
            }
        }
        // A release is read by the *next* tree, so that frame must come even
        // when nothing is moving.
        let mut animating = prev_held.is_some();
        for s in self.springs.values_mut().flatten() {
            animating |= s.step(dt);
        }
        self.springs
            .retain(|_, [h, p]| h.value > 0.0 || p.value > 0.0 || !h.settled() || !p.settled());

        // Focus follows a press on a focusable surface, and a press on
        // anything else drops it.
        self.double = None;
        if let Some(id) = self.interaction.pressed().map(str::to_owned) {
            if let Some((prev, t)) = self.last_press.take() {
                if prev == id && self.time - t < DOUBLE_CLICK {
                    self.double = Some(id.clone());
                }
            }
            self.last_press = Some((id.clone(), self.time));
            let keeps = self
                .scene
                .as_ref()
                .and_then(|s| s.surface(&id))
                .is_some_and(|s| s.focusable);
            self.focus = keeps.then_some(id);
        }
        for k in &input.keys {
            match k.key {
                Key::Escape => self.focus = None,
                Key::Tab => self.cycle_focus(k.mods.shift),
                _ => {}
            }
        }
        self.keys = input.keys;
        self.typed = input.text;
        for e in input.ime {
            match e {
                // A commit is typed text: it inserts at the caret and
                // replaces the selection exactly as a keystroke would.
                Ime::Commit(s) => {
                    self.preedit = None;
                    self.typed.push_str(&s);
                }
                Ime::Preedit { text, cursor } => {
                    // The platform's byte range is checked here, at the edge,
                    // rather than in every widget that slices by it.
                    let cursor = cursor
                        .filter(|(s, e)| text.is_char_boundary(*s) && text.is_char_boundary(*e));
                    self.preedit = (!text.is_empty()).then_some((text, cursor));
                }
                Ime::Enabled | Ime::Disabled => self.preedit = None,
            }
        }
        if self.focus.is_none() {
            self.preedit = None;
        }

        // A tip is due after the pointer has rested. Both the hover and the
        // surface come from last frame's scene, which is the one the pointer
        // was actually over.
        let hovered = self.interaction.hovered().map(str::to_owned);
        match (&mut self.hover, &hovered) {
            (Some((id, t)), Some(h)) if id == h => *t += dt,
            (_, Some(h)) => self.hover = Some((h.clone(), 0.0)),
            (_, None) => self.hover = None,
        }
        // A host may sleep when a frame is otherwise static. Keep it awake
        // until the tooltip deadline, and while a focused text field's caret
        // is blinking; both are time-driven visual changes rather than paint
        // springs. The previous scene is the one that measured this hover.
        let tip_pending = self.hover.as_ref().is_some_and(|(id, t)| {
            *t < TIP_DELAY
                && self
                    .scene
                    .as_ref()
                    .and_then(|scene| scene.surface(id))
                    .is_some_and(|surface| surface.tip.is_some())
        });
        animating |= tip_pending;
        // A focused caret is a deadline, not an animation: `repaint_after`
        // wakes the host at the next blink edge and one catch-up frame below
        // makes that edge visible.
        let tip = self
            .hover
            .as_ref()
            .filter(|(_, t)| *t >= TIP_DELAY)
            .and_then(|(id, _)| {
                let s = self.scene.as_ref()?.surface(id)?;
                Some((s.tip.clone()?, id.clone()))
            });
        let mut root = match &tip {
            Some((t, anchor)) => {
                // Under the surface, flipping over it at the bottom edge of
                // the window: the placement is the pin's, not arithmetic
                // here. A float is placed in its parent's padding box, but a
                // pinned one is absolute, so the wrapper only keeps the tip
                // out of a root that has no children.
                let float = text(t.clone())
                    .pad(S)
                    .fill(Role::Raised)
                    .radius(6.0)
                    .pin(
                        Pin::to(anchor.clone())
                            .area(Area::BottomStart)
                            .gap(Xs)
                            .fallback(Area::TopStart),
                    )
                    .id(TIP_KEY);
                overlay([root, float])
            }
            None => root,
        };

        let pal = self.theme.palette;
        // Declared state looks first, so a transition springs toward the
        // style the node actually asked for this frame.
        let (springs, focus) = (&self.springs, self.focus.as_deref());
        declared_states(
            &mut root,
            &|k, st| match st {
                State::Hover => springs.get(k).is_some_and(|[h, _]| h.value > 0.5),
                State::Press => springs.get(k).is_some_and(|[_, p]| p.value > 0.5),
                State::Focus => focus == Some(k),
                // Declared by the node, not discovered here: `declared_states`
                // answers this one from the element itself.
                State::Disabled => false,
            },
            false,
        );
        animating |= transitions(&mut root, &pal, &mut self.motion, &mut self.motion_seen, dt);
        for (_, s) in self.motion.iter_mut().filter(|(k, _)| k.starts_with('~')) {
            if let Some(s) = s[0].as_mut() {
                animating |= s.step(dt);
            }
        }
        let springs = &self.springs;
        let scrolls = &self.scrolls;
        state(
            &mut root,
            &pal,
            &|k| springs.get(k).map(|[h, p]| (h.value, p.value)),
            scrolls,
            false,
        );

        let mut spec = SceneSpec::new(root).theme(self.theme);
        spec.offered = offered;
        spec.font = self.font.clone();
        spec.fallback_fonts = self.fallback_fonts.clone();
        spec.device_scale = self.scale;
        spec.weld_backend = self.weld_backend;
        let scene =
            mui_scene::resolve_scene_cached(&spec, &mut self.text_cache, &mut self.weld_cache)?;
        if self
            .scene
            .as_ref()
            .is_none_or(|previous| !same_hit_geometry(previous, &scene))
        {
            // Named nodes are the gesture targets, in z-order. Unnamed ones
            // are decoration. A target clipped away does not respond.
            let mut hit = Hit::default();
            for s in scene
                .surfaces()
                .filter(|s| !s.key.starts_with('/') && !s.disabled)
            {
                if s.hits.is_empty() {
                    hit.push_clipped_paths(s.key.to_string(), &s.path, s.clip, s.clip_paths())?;
                }
                // A canvas that named its draws is hit by those shapes instead
                // of by its frame, so a ring responds in the ring, not its hole.
                for (tag, path) in &s.hits {
                    hit.push_tagged_paths(
                        s.key.to_string(),
                        tag.to_string(),
                        path,
                        s.clip,
                        s.clip_paths(),
                    )?;
                }
            }
            self.hit = hit;
        }
        let live = |id: &str| scene.surface(id).is_some_and(|s| !s.disabled);
        if self.interaction.held().is_some_and(|id| !live(id)) {
            self.cancel();
            self.edits
                .extend(self.cancelled.take().map(|id| (id, Edit::End)));
        }
        if self.focus.as_deref().is_some_and(|id| !live(id)) {
            self.focus = None;
            self.preedit = None;
        }
        self.scrolls.retain(|id, at| {
            let Some(surface) = scene.surface(id) else {
                return false;
            };
            let next = [
                at[0].clamp(
                    0.0,
                    (surface.content.width - surface.frame.size.width).max(0.0),
                ),
                at[1].clamp(
                    0.0,
                    (surface.content.height - surface.frame.size.height).max(0.0),
                ),
            ];
            animating |= *at != next;
            *at = next;
            true
        });
        self.sel.retain(|id, _| scene.surface(id).is_some());
        animating |= self.wheel(&scene, input.wheel);

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
            Some((t, Point::new(f.x, f.y)))
        });
        self.motion.retain(|id, _| self.motion_seen.contains(id));
        self.motion_seen.clear();
        self.delivered = std::mem::take(&mut self.edits);
        self.scene = Some(scene);
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
                            Some(Kind::TextInput { .. })
                        )
                });
        Ok(Frame {
            scene: self.scene.as_ref().expect("just set"),
            animating,
            repaint_after,
            tip,
            cursor,
            edits: self.delivered.clone(),
            clipboard: self.copied.take(),
            ime,
        })
    }

    /// Send the wheel to the innermost scrollable surface under the pointer.
    /// It lands on the next frame's tree, the same frame late a release is.
    fn wheel(&mut self, scene: &ResolvedScene, wheel: Point) -> bool {
        // A non-finite delta would land in `self.scrolls` for good: `clamp`
        // returns a NaN receiver unchanged, and every later frame would fail
        // validation on the offset.
        if !(wheel.x.is_finite() && wheel.y.is_finite()) || (wheel.x == 0.0 && wheel.y == 0.0) {
            return false;
        }
        let Some(p) = self.pointer.pos else {
            return false;
        };
        for s in scene.surfaces().rev() {
            let f = s.frame;
            if p.x < f.x || p.x > f.right() || p.y < f.y || p.y > f.bottom() {
                continue;
            }
            if s.clip.is_some_and(|clip| {
                p.x < clip.min.x || p.x > clip.max.x || p.y < clip.min.y || p.y > clip.max.y
            }) {
                continue;
            }
            // `content` is the frame size for everything but a scroll node,
            // so an overflow here *is* the "is this scrollable" test.
            let max = [
                (s.content.width - f.size.width).max(0.0),
                (s.content.height - f.size.height).max(0.0),
            ];
            if max[0] <= 0.0 && max[1] <= 0.0 {
                continue;
            }
            let at = self.scrolls.entry(s.key.to_string()).or_insert([0.0, 0.0]);
            let next = [
                (at[0] + wheel.x).clamp(0.0, max[0]),
                (at[1] + wheel.y).clamp(0.0, max[1]),
            ];
            if next != *at {
                *at = next;
                return true;
            }
            // An exhausted or perpendicular nested scroller yields to its parent.
        }
        false
    }
}

/// Every numeric paint channel of `e`, in a fixed order, replaced by
/// `ch(index, declared)`. Shape padding/bend and border widths share this clock.
fn channels(e: &mut Element, pal: &Palette, ch: &mut impl FnMut(usize, f64) -> f64) {
    if let Some(Paint::Solid(c)) = e.style.fill.paint(pal, pal.background()) {
        let v = [c.lightness(), c.chroma(), c.hue(), c.alpha()];
        let o = std::array::from_fn::<f32, 4, _>(|i| ch(i, f64::from(v[i])) as f32);
        e.style.fill = Fill::Color(Color::oklcha(o[0], o[1], o[2], o[3]));
    }
    if let Some(w) = e.style.stroke.as_mut().and_then(|s| s.width.as_mut()) {
        *w = ch(4, *w).max(0.0);
    }
    if let Radius::Px(r) = &mut e.style.radius {
        *r = ch(5, *r).max(0.0);
    }
    if let Some(t) = e.text_size.as_mut() {
        *t = ch(6, *t).max(0.0);
    }
    for (i, s) in e.style.shadow.iter_mut().enumerate() {
        s.blur = ch(7 + i, s.blur).max(0.0);
    }
    // After the shadows, so a two-shadow node's shells keep their own slots.
    let shells = 7 + e.style.shadow.len();
    for (i, (d, _)) in e.style.shells.iter_mut().enumerate() {
        if let Spacing::Px(v) = d {
            *v = ch(shells + i, *v).max(0.0);
        }
    }
    let shape = shells + e.style.shells.len();
    if let Some(Spacing::Px(pad)) = &mut e.inside {
        if pad.is_finite() && *pad >= 0. {
            *pad = ch(shape, *pad).max(0.);
        }
    }
    if e.bend.is_finite() && e.bend.abs() <= 0.45 {
        e.bend = ch(shape + 1, e.bend).clamp(-0.45, 0.45);
    }
    if let Some(ramp) = &mut e.border_ramp {
        for (i, width) in [&mut ramp.from.1, &mut ramp.to.1].into_iter().enumerate() {
            if width.is_finite() && *width >= 0. {
                *width = ch(shape + 2 + i, *width).max(0.);
            }
        }
    }
}

/// Spring every transitioning node's paint toward what it declared this
/// frame. The springs hold last frame's declaration as their target, so a new
/// target mid-flight retargets the live spring instead of restarting it.
fn transitions(
    n: &mut El,
    pal: &Palette,
    motion: &mut BTreeMap<String, Vec<Option<Spring>>>,
    seen: &mut BTreeSet<String>,
    dt: f64,
) -> bool {
    let mut animating = false;
    if let (Some(k), Some(spring)) = (n.key().map(str::to_owned), n.payload().transition) {
        seen.insert(k.clone());
        let list = motion.entry(k).or_default();
        let coupled_gap = n.payload().inside.is_some_and(|p| p == *n.gap_mut());
        channels(n.payload_mut(), pal, &mut |i, declared| {
            // Each slot is seeded from its own declared value the first time
            // it is touched: `channels` skips channels a node has no paint
            // for, so a blanket resize would seed them from another channel.
            if list.len() <= i {
                list.resize(i + 1, None);
            }
            let s = list[i].get_or_insert_with(|| spring.seeded(declared));
            // Hue is an angle: take the short way round rather than
            // sweeping 350 degrees back to 10.
            if i == 2 {
                s.value += ((declared - s.value) / 360.0).round() * 360.0;
            }
            s.to(declared);
            animating |= s.step(dt);
            s.value
        });
        if coupled_gap {
            *n.gap_mut() = n.payload().inside.expect("coupled inside");
        }
    }
    for c in n.children_mut() {
        animating |= transitions(c, pal, motion, seen, dt);
    }
    animating
}

/// Whether a semantic role owns the runtime's default pointer looks.
fn interactive(e: &Element) -> bool {
    e.semantics.as_ref().is_some_and(|s| {
        matches!(
            &s.role,
            Kind::Button | Kind::Slider { .. } | Kind::Toggle { .. } | Kind::TextInput { .. }
        )
    })
}

/// Which pointer states deserve springs for one key. Structural IDs remain
/// hit-testable, but do not keep the host animating merely because the pointer
/// rests on them. Only the active targets are searched, so this adds no
/// per-frame policy allocation.
fn state_policy(root: &El, id: &str) -> [bool; 2] {
    fn visit(n: &El, id: &str) -> Option<[bool; 2]> {
        if n.key().is_some_and(|k| k == id) {
            let e = n.payload();
            let mut policy = [interactive(e), interactive(e)];
            for (state, _) in &e.states {
                match state {
                    State::Hover => policy[0] = true,
                    State::Press => policy[1] = true,
                    State::Focus | State::Disabled => {}
                }
            }
            return Some(policy);
        }
        n.children().iter().find_map(|child| visit(child, id))
    }

    visit(root, id).unwrap_or([false, false])
}

/// Replace every named node's style with what it declared for the states it
/// is in, in declaration order. Keep the declarations on the node: the later
/// automatic-state pass uses them to avoid applying the same state twice.
/// `off` is the enclosing subtree's disabled flag, `false` at the root: a card
/// that switched itself off greys the controls inside it too, which is the same
/// rule the hit gate uses.
fn declared_states(n: &mut El, is: &dyn Fn(&str, State) -> bool, off: bool) {
    let off = off || n.payload().disabled;
    if let Some(k) = n.key().map(str::to_owned) {
        let e = n.payload_mut();
        let states = std::mem::take(&mut e.states);
        for (st, f) in &states {
            let on = match st {
                State::Disabled => off,
                // A disabled node is never hovered or pressed -- it is not in
                // the hit map -- and a focus it held before it was switched
                // off is not a reason to paint it lit.
                _ => !off && is(&k, *st),
            };
            if on {
                e.style = f.0(e.style.clone());
            }
        }
        e.states = states;
    }
    for c in n.children_mut() {
        declared_states(c, is, off);
    }
}

/// Push automatic hover and press into interactive surfaces' fills,
/// proportionally, and slide the ones the wheel has scrolled. A named layout
/// node is an identity and hit-test surface, not automatically a control. An
/// explicit state owns its channel so a declared look is applied once.
/// `off` is the enclosing subtree's disabled flag, `false` at the root: a
/// hover spring still decaying from before the node was switched off must not
/// tint it.
fn state(
    n: &mut El,
    pal: &Palette,
    of: &dyn Fn(&str) -> Option<(f64, f64)>,
    scrolls: &BTreeMap<String, [f64; 2]>,
    off: bool,
) {
    let off = off || n.payload().disabled;
    if let Some([x, y]) = n.key().and_then(|k| scrolls.get(k)).copied() {
        // `scrolled` is a builder and a built node cannot be reopened.
        let node = std::mem::replace(n, mui_scene::leaf(0.0, 0.0));
        *n = node.scrolled(x, y);
    }
    if let Some((h, p)) = n.key().and_then(of).filter(|_| !off) {
        let bg = pal.background();
        let (auto_hover, auto_press) = {
            let e = n.payload();
            (
                interactive(e) && !e.states.iter().any(|(state, _)| *state == State::Hover),
                interactive(e) && !e.states.iter().any(|(state, _)| *state == State::Press),
            )
        };
        let e = n.payload_mut();
        if !e.style.fill.is_none() && ((auto_hover && h > 0.0) || (auto_press && p > 0.0)) {
            e.style.fill = e.style.fill.map(pal, bg, |c| {
                let c = if auto_hover {
                    c.mix(pal.hover(c), h as f32)
                } else {
                    c
                };
                if auto_press {
                    c.mix(pal.pressed(c), p as f32)
                } else {
                    c
                }
            });
        }
    }
    for c in n.children_mut() {
        state(c, pal, of, scrolls, off);
    }
}
impl std::fmt::Debug for Ui {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ui")
            .field("springs", &self.springs.len())
            .finish()
    }
}

/// The runtime, seen from a widget: every method forwards to the inherent one
/// of the same name, so `mui-widgets` can stay ignorant of the event loop.
impl Host for Ui {
    fn theme(&self) -> &Theme {
        &self.theme
    }
    fn scene(&self) -> Option<&ResolvedScene> {
        Ui::scene(self)
    }
    fn get(&self, id: &str) -> Response {
        Ui::get(self, id)
    }
    fn tag(&self, id: &str) -> Option<&str> {
        Ui::tag(self, id)
    }
    fn state(&self, id: &str) -> (f64, f64) {
        Ui::state(self, id)
    }
    fn drag(
        &self,
        id: &str,
        value: &mut f64,
        range: std::ops::RangeInclusive<f64>,
        px: f64,
        vertical: bool,
    ) -> bool {
        Ui::drag(self, id, value, range, px, vertical)
    }
    fn tween_with(&mut self, id: &str, target: f64, spring: Spring) -> f64 {
        Ui::tween_with(self, id, target, spring)
    }
    fn focused(&self, id: &str) -> bool {
        Ui::focused(self, id)
    }
    fn double_click(&self, id: &str) -> bool {
        Ui::double_click(self, id)
    }
    fn local(&self, id: &str) -> Option<Point> {
        Ui::local(self, id)
    }
    fn keys(&self, id: &str) -> &[KeyPress] {
        Ui::keys(self, id)
    }
    fn text(&self, id: &str) -> &str {
        Ui::text(self, id)
    }
    fn sel(&self, id: &str) -> (usize, usize) {
        Ui::sel(self, id)
    }
    fn set_sel(&mut self, id: &str, anchor: usize, caret: usize) {
        Ui::set_sel(self, id, anchor, caret);
    }
    fn pasted(&self) -> Option<&str> {
        Ui::pasted(self)
    }
    fn set_clipboard(&mut self, s: String) {
        Ui::set_clipboard(self, s);
    }
    fn preedit(&self) -> Option<(&str, Option<(usize, usize)>)> {
        Ui::preedit(self)
    }
    fn set_ime_caret(&mut self, id: &str, at: Point, height: f64) {
        Ui::set_ime_caret(self, id, at, height);
    }
    fn hit(&self, s: &str, size: f64, x: f64) -> usize {
        Ui::hit(self, s, size, x)
    }
    fn caret_x(&self, s: &str, size: f64, byte: usize) -> f64 {
        Ui::caret_x(self, s, size, byte)
    }
    fn blink(&self) -> bool {
        Ui::blink(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui_geometry::Point;
    use mui_input::{Button, Buttons, Mods};
    use mui_scene::prelude::*;

    fn at(x: f64, y: f64, down: bool) -> PointerInput {
        PointerInput {
            pos: Some(Point::new(x, y)),
            buttons: Buttons::default().set(Button::Primary, down),
            ..PointerInput::default()
        }
    }
    fn key(k: Key) -> Input {
        Input {
            keys: vec![KeyPress {
                key: k,
                mods: Mods::default(),
            }],
            ..Input::default()
        }
    }
    fn paint_color(frame: &Frame<'_>, id: &str) -> Color {
        frame
            .scene
            .paint
            .iter()
            .find(|p| &*p.key == id)
            .unwrap_or_else(|| panic!("missing paint for {id}"))
            .paint
            .solid()
    }
    fn role_color(role: Role) -> Color {
        let pal = Theme::DEFAULT.palette;
        match Fill::from(role).paint(&pal, pal.background()) {
            Some(Paint::Solid(c)) => c,
            _ => panic!("role has no solid paint"),
        }
    }

    /// A ring: the outer circle one way round, the inner the other, so the
    /// hole is outside under the non-zero rule the renderer fills by.
    fn ring(c: Point, outer: f64, inner: f64) -> mui_geometry::Path {
        let arc = move |r: f64, rev: bool| {
            (0..64).map(move |i| {
                let k = if rev { 64 - i } else { i };
                let a = std::f64::consts::TAU * f64::from(k) / 64.0;
                Point::new(c.x + r * a.cos(), c.y + r * a.sin())
            })
        };
        let mut p = mui_geometry::Path::polyline(arc(outer, false), true);
        for (i, q) in arc(inner, true).enumerate() {
            p = if i == 0 { p.move_to(q) } else { p.line_to(q) };
        }
        p.close()
    }

    /// A source and a target side by side, 50 px each.
    fn two() -> El {
        row([
            leaf(50., 50.).fill(Role::Field).id("src"),
            leaf(50., 50.).fill(Role::Field).id("dst"),
        ])
    }

    #[derive(Debug, PartialEq)]
    struct Wave(&'static str);

    /// Drag `src` out and back to `x`, then let go, attaching a payload once
    /// the gesture is a drag. Returns the `Ui` on the frame the drop is reported in.
    fn drag_to(x: f64) -> Ui {
        let mut ui = Ui::new(Theme::DEFAULT);
        for _ in 0..2 {
            ui.frame(two(), None, at(25., 25., false), 0.016).unwrap();
        }
        ui.frame(two(), None, at(25., 25., true), 0.016).unwrap();
        // Out past the drag threshold first, then wherever this drag ends.
        ui.frame(two(), None, at(80., 25., true), 0.016).unwrap();
        ui.frame(two(), None, at(x, 25., true), 0.016).unwrap();
        assert!(ui.get("src").dragged, "the gesture is a drag by now");
        ui.start_drag("src", Wave("saw"));
        assert_eq!(ui.dragging::<Wave>(), Some(&Wave("saw")));
        ui.frame(two(), None, at(x, 25., false), 0.016).unwrap();
        ui
    }

    /// The payload reaches the target it was dropped on, once. A second ask
    /// -- another widget, a later frame -- gets nothing, so a drop can never
    /// be applied twice.
    #[test]
    fn a_dropped_payload_is_delivered_exactly_once() {
        let mut ui = drag_to(80.);
        assert_eq!(ui.dropped_on::<Wave>("dst"), Some(Wave("saw")));
        assert_eq!(ui.dropped_on::<Wave>("dst"), None, "already taken");
        assert!(ui.dragging::<Wave>().is_none(), "the gesture is over");
    }

    /// A payload nobody took does not survive its drag, and a release that
    /// landed somewhere else was never that target's to take.
    #[test]
    fn a_release_elsewhere_delivers_nothing() {
        // Back onto the source: `dst` sees no drop.
        let mut ui = drag_to(25.);
        assert_eq!(ui.dropped_on::<Wave>("dst"), None);
        assert_eq!(ui.dropped_on::<Wave>("src"), Some(Wave("saw")));

        // And the frame after a drop nobody took, the payload is gone.
        let mut ui = drag_to(80.);
        ui.frame(two(), None, at(80., 25., false), 0.016).unwrap();
        assert_eq!(ui.dropped_on::<Wave>("dst"), None, "one frame only");

        // A target asking for the wrong type leaves it for the right one.
        let mut ui = drag_to(80.);
        assert!(ui.dropped_on::<f64>("dst").is_none(), "not an f64");
        assert_eq!(ui.dropped_on::<Wave>("dst"), Some(Wave("saw")));
    }

    /// The drawn shape is the hit shape: the ring responds, the hole it
    /// leaves does not, and the tag says which draw was hit.
    #[test]
    fn a_tagged_canvas_responds_in_its_drawn_ring_only() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || {
            canvas(|s| {
                let c = Point::new(s.width / 2., s.height / 2.);
                vec![Draw::fill(ring(c, 50., 25.), Role::Primary).tag("band")]
            })
            .size(100., 100.)
            .id("dial")
        };
        // Two frames per move: the hit map a gesture reads is the last built.
        let hover = |ui: &mut Ui, x: f64, y: f64| {
            for _ in 0..2 {
                ui.frame(tree(), None, at(x, y, false), 0.016).unwrap();
            }
        };
        hover(&mut ui, 50., 50.);
        assert!(!ui.get("dial").hovered, "the hole is not the dial");
        assert_eq!(ui.tag("dial"), None);
        hover(&mut ui, 50., 12.);
        assert!(ui.get("dial").hovered, "the band is");
        assert_eq!(ui.tag("dial"), Some("band"));
        hover(&mut ui, 2., 2.);
        assert!(!ui.get("dial").hovered, "and the frame's corner is not");
    }

    /// The tag a press grabbed survives a drag that leaves the shape.
    #[test]
    fn the_tag_is_latched_for_the_length_of_the_gesture() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || {
            canvas(|s| {
                let c = Point::new(s.width / 2., s.height / 2.);
                vec![Draw::fill(ring(c, 50., 25.), Role::Primary).tag("band")]
            })
            .size(100., 100.)
            .id("dial")
        };
        for _ in 0..2 {
            ui.frame(tree(), None, at(50., 12., false), 0.016).unwrap();
        }
        ui.frame(tree(), None, at(50., 12., true), 0.016).unwrap();
        ui.frame(tree(), None, at(50., 50., true), 0.016).unwrap();
        assert_eq!(ui.tag("dial"), Some("band"), "still the shape it grabbed");
        ui.frame(tree(), None, at(50., 50., false), 0.016).unwrap();
        ui.frame(tree(), None, at(50., 50., false), 0.016).unwrap();
        assert_eq!(ui.tag("dial"), None, "released over the hole");
    }

    /// The two halves of `.disabled` are one feature: the look it declared
    /// for `State::Disabled` is painted, the look it declared for a hover is
    /// not, and the pointer sitting on it produces no gesture at all.
    #[test]
    fn a_disabled_node_paints_its_off_look_and_hits_nothing() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = |off: bool| {
            leaf(100., 100.)
                .fill(Role::Field)
                .on(State::Hover, |s| s.fill(Role::Primary))
                .on(State::Disabled, |s| s.fill(Role::Dim))
                .disabled(off)
                .focusable()
                .role(Kind::Button)
                .id("bypass")
        };
        let fill = |f: &Frame<'_>| {
            f.scene
                .paint
                .iter()
                .find(|p| &*p.key == "bypass")
                .expect("painted")
                .paint
                .solid()
        };
        let pal = Theme::DEFAULT.palette;
        let role = |r: Role| match Fill::from(r).paint(&pal, pal.background()) {
            Some(Paint::Solid(c)) => c,
            _ => panic!("a role paints solid"),
        };
        // Live: two frames, because a gesture reads the previous hit map.
        ui.frame(tree(false), None, at(50., 50., false), 0.016)
            .unwrap();
        for _ in 0..12 {
            ui.frame(tree(false), None, at(50., 50., false), 0.016)
                .unwrap();
        }
        let frame = ui
            .frame(tree(false), None, at(50., 50., false), 0.016)
            .unwrap();
        let lit = fill(&frame);
        assert!(ui.get("bypass").hovered, "live, and under the pointer");
        assert_eq!(lit, role(Role::Primary), "the explicit hover look, once");

        for _ in 0..2 {
            ui.frame(tree(true), None, at(50., 50., true), 0.016)
                .unwrap();
        }
        let off = fill(
            &ui.frame(tree(true), None, at(50., 50., true), 0.016)
                .unwrap(),
        );
        let r = ui.get("bypass");
        assert!(!r.hovered && !r.pressed && !r.held, "no gesture: {r:?}");
        assert_eq!(off, role(Role::Dim), "the disabled look, unlifted");

        // And it is no Tab stop -- nor does it keep a focus it already had.
        ui.frame(tree(true), None, key(Key::Tab), 0.016).unwrap();
        assert!(!ui.focused("bypass"));
        ui.focus("bypass");
        ui.frame(tree(true), None, PointerInput::default(), 0.016)
            .unwrap();
        assert!(!ui.focused("bypass"), "a focus on a dead node is dropped");
    }

    /// A shortcut is not focus-gated -- except by a field that is typing.
    #[test]
    fn a_shortcut_fires_unfocused_and_never_while_a_field_has_the_focus() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let mut value = String::new();
        let undo = |ui: &Ui| {
            ui.shortcuts()
                .iter()
                .any(|k| k.key == Key::Function(1) || k.key == Key::Space)
        };
        let tree = |ui: &mut Ui, v: &mut String| mui_widgets::text_input(ui, "f", v);

        let root = tree(&mut ui, &mut value);
        ui.frame(root, None, key(Key::Function(1)), 0.016).unwrap();
        assert!(undo(&ui), "nothing is focused, so the shortcut is ours");

        ui.focus("f");
        let root = tree(&mut ui, &mut value);
        ui.frame(root, None, key(Key::Space), 0.016).unwrap();
        assert!(!undo(&ui), "the field is typing: the key is its own");
        assert_eq!(ui.keys("f").len(), 1, "and it still gets it");
    }

    #[test]
    fn tab_walks_the_focusable_surfaces_in_scene_order() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || {
            column([
                leaf(20., 20.).focusable().id("a"),
                leaf(20., 20.).id("plain"),
                leaf(20., 20.).focusable().id("b"),
            ])
        };
        ui.frame(tree(), None, PointerInput::default(), 0.016)
            .unwrap();
        for want in ["a", "b", "a"] {
            ui.frame(tree(), None, key(Key::Tab), 0.016).unwrap();
            assert!(ui.focused(want), "expected {want}");
        }
        ui.frame(tree(), None, key(Key::Escape), 0.016).unwrap();
        assert!(!ui.focused("a") && !ui.focused("b"));
    }

    /// A shipped button is a real Tab stop and Enter reaches the same action
    /// path as a primary click.
    #[test]
    fn a_button_can_be_focused_and_activated_from_the_keyboard() {
        fn tree(ui: &Ui) -> El {
            mui_widgets::button(ui, "button", "Save").0.el()
        }

        let mut ui = Ui::new(Theme::DEFAULT);
        ui.frame(tree(&ui), None, PointerInput::default(), 0.016)
            .unwrap();
        ui.frame(tree(&ui), None, key(Key::Tab), 0.016).unwrap();
        assert_eq!(ui.focus_key(), Some("button"));

        // Keys are delivered to the tree on the following frame, exactly as
        // mouse edges are, so the widget sees the same activation boundary.
        ui.frame(tree(&ui), None, key(Key::Enter), 0.016).unwrap();
        let (_, activated) = mui_widgets::button(&ui, "button", "Save");
        assert!(activated, "Enter activates the focused button");
    }

    #[test]
    fn nested_scrollers_yield_and_shorter_content_clamps_the_offset() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = |tall| {
            column([
                row([leaf(100., 20.)])
                    .size(30., 20.)
                    .scroll()
                    .id("horizontal"),
                leaf(30., if tall { 150. } else { 10. }),
            ])
            .size(30., 40.)
            .scroll()
            .id("outer")
        };
        ui.frame(tree(true), None, PointerInput::default(), 0.016)
            .unwrap();
        let input = Input {
            pointer: at(10., 10., false),
            wheel: Point::new(0., 80.),
            ..Input::default()
        };
        let frame = ui.frame(tree(true), None, input, 0.016).unwrap();
        assert!(frame.animating, "wheel changes need a catch-up frame");
        assert_eq!(ui.scroll("horizontal"), [0., 0.]);
        assert_eq!(ui.scroll("outer"), [0., 80.]);
        let frame = ui
            .frame(tree(false), None, PointerInput::default(), 0.016)
            .unwrap();
        assert!(frame.animating, "clamping needs a catch-up frame");
        assert_eq!(ui.scroll("outer"), [0., 0.]);
    }

    #[test]
    fn nested_and_floating_content_does_not_extend_the_outer_scroll() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = column([
            column([leaf(30., 1000.)])
                .size(30., 40.)
                .scroll()
                .id("inner"),
            leaf(30., 20.),
            leaf(30., 900.).float(),
        ])
        .gap(0.)
        .size(30., 50.)
        .scroll()
        .id("outer");
        ui.frame(tree, None, PointerInput::default(), 0.016)
            .unwrap();
        assert_eq!(
            ui.scene().unwrap().surface("outer").unwrap().content.height,
            60.
        );
    }

    #[test]
    fn an_exhausted_inner_scroll_yields_to_its_parent() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || {
            column([
                column([leaf(30., 80.)]).size(30., 40.).scroll().id("inner"),
                leaf(30., 100.),
            ])
            .size(30., 60.)
            .scroll()
            .id("outer")
        };
        let input = || Input {
            pointer: at(10., 10., false),
            wheel: Point::new(0., 100.),
            ..Input::default()
        };
        ui.frame(tree(), None, input(), 0.016).unwrap();
        assert_eq!(ui.scroll("inner"), [0., 40.]);
        ui.frame(tree(), None, input(), 0.016).unwrap();
        assert!(ui.scroll("outer")[1] > 0.);
    }

    #[test]
    fn the_wheel_scrolls_a_column_and_stops_at_its_end() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || {
            column([leaf(20., 100.), leaf(20., 100.)])
                .height(50.)
                .scroll()
                .id("list")
        };
        let wheel = |y: f64| Input {
            pointer: at(10., 10., false),
            wheel: Point::new(0., y),
            ..Input::default()
        };
        ui.frame(tree(), None, PointerInput::default(), 0.016)
            .unwrap();
        ui.frame(tree(), None, wheel(30.), 0.016).unwrap();
        assert_eq!(ui.scroll("list"), [0., 30.]);
        ui.frame(tree(), None, wheel(1000.), 0.016).unwrap();
        let [_, y] = ui.scroll("list");
        assert!(y > 30. && y <= 200., "clamped to the overflow, got {y}");
        ui.frame(tree(), None, wheel(-1e6), 0.016).unwrap();
        assert_eq!(ui.scroll("list"), [0., 0.], "and not past the top");
    }

    #[test]
    fn typing_reaches_the_focused_field() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let mut value = String::new();
        let tree = |ui: &mut Ui, v: &mut String| mui_widgets::text_input(ui, "f", v);
        let root = tree(&mut ui, &mut value);
        ui.frame(root, None, PointerInput::default(), 0.016)
            .unwrap();
        ui.focus("f");
        let typed = Input {
            text: "hi".into(),
            ..Input::default()
        };
        let root = tree(&mut ui, &mut value);
        ui.frame(root, None, typed, 0.016).unwrap();
        let root = tree(&mut ui, &mut value);
        ui.frame(root, None, key(Key::Backspace), 0.016).unwrap();
        let root = tree(&mut ui, &mut value);
        ui.frame(root, None, PointerInput::default(), 0.016)
            .unwrap();
        assert_eq!(value, "h");
    }

    #[test]
    fn a_composition_paints_without_editing_the_value_and_the_commit_inserts() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let mut value = "ab".to_owned();
        let tree = |ui: &mut Ui, v: &mut String| mui_widgets::text_input(ui, "f", v);
        let ime = |e: mui_input::Ime| Input {
            ime: vec![e],
            ..Input::default()
        };
        let root = tree(&mut ui, &mut value);
        ui.frame(root, None, PointerInput::default(), 0.016)
            .unwrap();
        ui.focus("f");
        ui.set_sel("f", 1, 1);

        let root = tree(&mut ui, &mut value);
        let f = ui
            .frame(
                root,
                None,
                ime(mui_input::Ime::Preedit {
                    text: "xy".into(),
                    cursor: Some((1, 1)),
                }),
                0.016,
            )
            .unwrap();
        let (at, _) = f.ime.expect("the host is told where the caret is");
        assert!(
            at.x > 0.0,
            "and the caret is in the scene's space, not the field's"
        );
        // The preedit reaches the *next* tree, as every input does here.
        let root = tree(&mut ui, &mut value);
        let f = ui
            .frame(root, None, PointerInput::default(), 0.016)
            .unwrap();
        assert_eq!(value, "ab", "a preedit never touches the value");
        let under = f.scene.surface("/3").expect("underline").frame;
        assert!(under.size.width > 0., "the composing span is underlined");

        let root = tree(&mut ui, &mut value);
        ui.frame(root, None, ime(mui_input::Ime::Commit("xy".into())), 0.016)
            .unwrap();
        let root = tree(&mut ui, &mut value);
        let f = ui
            .frame(root, None, PointerInput::default(), 0.016)
            .unwrap();
        assert_eq!(value, "axyb", "the commit landed at the caret");
        assert!(
            f.scene.surface("/3").is_none(),
            "and the composition, with it the underline, is gone"
        );
    }

    #[test]
    fn a_long_value_scrolls_under_the_clip_instead_of_wrapping() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let mut value = "x".repeat(60);
        let win = Some(Size::new(200., 60.));
        let tree = |ui: &mut Ui, v: &mut String| mui_widgets::text_input(ui, "f", v);
        let root = tree(&mut ui, &mut value);
        ui.frame(root, win, PointerInput::default(), 0.016).unwrap();
        ui.set_sel("f", 60, 60);
        let root = tree(&mut ui, &mut value);
        let f = ui.frame(root, win, PointerInput::default(), 0.016).unwrap();
        // The value, the caret and the field: unnamed children are keyed by
        // their slot under the root.
        let field = f.scene.surface("f").expect("field").frame;
        let text = f.scene.surface("/1").expect("value").frame;
        let caret = f.scene.surface("/2").expect("caret").frame;
        assert!(
            text.size.height < 2. * ui.theme.text,
            "one line, not wrapped: {text:?}"
        );
        assert!(
            caret.right() <= field.right() && caret.x >= field.x,
            "the caret stayed in the field: {caret:?} in {field:?}"
        );
    }

    #[test]
    fn a_selection_is_extended_by_shift_and_deleted_as_one() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let mut value = String::from("hello");
        let run = |ui: &mut Ui, v: &mut String, input: Input| {
            let root = mui_widgets::text_input(ui, "f", v);
            ui.frame(root, None, input, 0.016).unwrap();
        };
        run(&mut ui, &mut value, Input::default());
        ui.focus("f");
        let shift = |k| Input {
            keys: vec![KeyPress {
                key: k,
                mods: Mods {
                    shift: true,
                    ..Mods::default()
                },
            }],
            ..Input::default()
        };
        run(&mut ui, &mut value, shift(Key::Right));
        run(&mut ui, &mut value, shift(Key::Right));
        run(&mut ui, &mut value, key(Key::Backspace));
        run(&mut ui, &mut value, Input::default());
        assert_eq!(value, "llo", "two characters selected, one Backspace");
    }

    #[test]
    fn copy_asks_the_host_for_the_clipboard_and_paste_takes_it_back() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let mut value = String::from("hi");
        let run = |ui: &mut Ui, v: &mut String, input: Input| {
            let root = mui_widgets::text_input(ui, "f", v);
            ui.frame(root, None, input, 0.016)
                .unwrap()
                .clipboard
                .clone()
        };
        run(&mut ui, &mut value, Input::default());
        ui.focus("f");
        let ctrl = |c: char, clipboard: Option<String>| Input {
            keys: vec![KeyPress {
                key: Key::Char(c),
                mods: Mods {
                    ctrl: true,
                    ..Mods::default()
                },
            }],
            clipboard,
            ..Input::default()
        };
        run(&mut ui, &mut value, ctrl('a', None));
        run(&mut ui, &mut value, ctrl('c', None));
        let out = run(&mut ui, &mut value, Input::default());
        assert_eq!(out.as_deref(), Some("hi"), "the copy reached the frame");
        run(&mut ui, &mut value, ctrl('v', Some("yo".into())));
        run(&mut ui, &mut value, Input::default());
        assert_eq!(value, "yo", "and a paste replaced the selection");
    }

    #[test]
    fn a_tip_comes_due_after_half_a_second_of_hover() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || leaf(40., 40.).fill(Role::Raised).tip("why").id("b");
        // In a window, not hugging: a float is kept inside the box it floats
        // in, so the room under the surface has to exist.
        let win = || Some(Size::new(240., 300.));
        ui.frame(tree(), win(), at(10., 10., false), 0.016).unwrap();
        let f = ui.frame(tree(), win(), at(10., 10., false), 0.4).unwrap();
        assert!(f.tip.is_none(), "the pointer has not rested long enough");
        assert!(f.animating, "the tooltip deadline keeps idle hosts awake");
        let f = ui.frame(tree(), win(), at(10., 10., false), 0.6).unwrap();
        let (t, at) = f.tip.clone().expect("due");
        assert_eq!(t, "why");
        assert!(at.y > 40., "below the surface");
        assert!(!f.animating, "a settled tooltip does not spin the host");
        assert!(
            f.scene.surfaces().any(|s| s.frame.y > 40.),
            "and floated into the scene"
        );
        let f = ui
            .frame(tree(), win(), PointerInput::default(), 0.016)
            .unwrap();
        assert!(f.tip.is_none(), "gone when the pointer leaves");
    }

    #[test]
    fn a_tip_lands_where_it_was_measured_even_under_a_padded_root() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || column([leaf(40., 40.).fill(Role::Raised).tip("why").id("b")]).pad(L);
        let win = || Some(Size::new(240., 300.));
        let p = at(110., 30., false);
        ui.frame(tree(), win(), p, 0.016).unwrap();
        ui.frame(tree(), win(), p, 0.016).unwrap();
        let f = ui.frame(tree(), win(), p, 0.6).unwrap();
        let (_, at) = f.tip.clone().expect("due");
        let tip = f
            .scene
            .surfaces()
            .find(|s| s.frame.y == at.y)
            .expect("the tip sits where it was measured, not padded away");
        assert_eq!((tip.frame.x, tip.frame.y), (at.x, at.y));
    }

    fn solid(f: &Frame) -> mui_scene::Paint {
        f.scene.paint[0].paint.clone()
    }

    #[test]
    fn a_transition_lands_between_the_two_fills_and_settles() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = |on: bool| {
            leaf(40., 40.)
                .fill(if on { Role::Primary } else { Role::Field })
                .animate()
                .id("b")
        };
        let from = solid(
            &ui.frame(tree(false), None, Input::default(), 0.016)
                .unwrap(),
        );
        let mid = solid(&ui.frame(tree(true), None, Input::default(), 0.016).unwrap());
        assert_ne!(mid, from, "it left the old fill");
        let mut t = 0.0;
        let to = loop {
            let f = ui.frame(tree(true), None, Input::default(), 0.016).unwrap();
            t += 0.016;
            let paint = solid(&f);
            assert!(t < 2.0, "never settled");
            if !f.animating {
                break paint;
            }
        };
        assert_ne!(mid, to, "and the middle was not the end");
        let mut fresh = Ui::new(Theme::DEFAULT);
        let want = solid(
            &fresh
                .frame(tree(true), None, Input::default(), 0.016)
                .unwrap(),
        );
        assert_eq!(to, want, "it settles on the declared fill");
    }

    #[test]
    fn retargeting_mid_flight_does_not_jump() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = |on: bool| {
            leaf(40., 40.)
                .fill(if on { Role::Primary } else { Role::Field })
                .animate()
                .id("b")
        };
        let home = solid(
            &ui.frame(tree(false), None, Input::default(), 0.016)
                .unwrap(),
        );
        for _ in 0..3 {
            ui.frame(tree(true), None, Input::default(), 0.016).unwrap();
        }
        let before = solid(&ui.frame(tree(true), None, Input::default(), 0.016).unwrap());
        let after = solid(
            &ui.frame(tree(false), None, Input::default(), 0.016)
                .unwrap(),
        );
        assert_ne!(after, home, "a retarget carries velocity, it does not snap");
        assert_ne!(after, before, "and it keeps moving");
    }

    #[test]
    fn a_tween_walks_to_its_target_and_never_back() {
        let mut ui = Ui::new(Theme::DEFAULT);
        assert_eq!(ui.tween("cutoff", 0.0), 0.0, "it starts where it is told");
        let mut prev = 0.0;
        for _ in 0..180 {
            ui.frame(leaf(1., 1.), None, Input::default(), 0.016)
                .unwrap();
            let v = ui.tween("cutoff", 1.0);
            assert!(v >= prev, "went backwards: {v} after {prev}");
            assert!(v <= 1.0 + 1e-9, "overshot to {v}");
            prev = v;
        }
        assert!(prev > 0.99, "arrived: {prev}");
    }

    #[test]
    fn a_press_and_its_release_bracket_the_gesture() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || leaf(40., 40.).fill(Role::Raised).id("b");
        ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        let f = ui.frame(tree(), None, at(10., 10., true), 0.016).unwrap();
        assert_eq!(f.edits, vec![("b".to_owned(), Edit::Begin)]);
        assert_eq!(ui.edit("b"), Some(Edit::Begin));
        let f = ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        assert_eq!(f.edits, vec![("b".to_owned(), Edit::End)]);
        assert_eq!(ui.edit("b"), Some(Edit::End));
        let f = ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        assert!(f.edits.is_empty());
    }

    /// A look declared beside the resting one, applied because the runtime
    /// knows which node the pointer is on -- no `ui.state` in the tree.
    #[test]
    fn a_declared_hover_style_is_applied_while_hovered() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || {
            leaf(40., 40.)
                .fill(Role::Raised)
                .on(State::Hover, |s| s.radius(3.))
                .id("b")
        };
        let corner = |ui: &mut Ui, p| {
            ui.frame(tree(), None, p, 0.016)
                .unwrap()
                .scene
                .paint
                .iter()
                .find_map(|p| p.rect.map(|r| r.radius()))
                .expect("the box paints a rounded rect")
        };
        let cold = corner(&mut ui, PointerInput::default());
        // Hover long enough that the spring passes the halfway mark.
        let mut warm = cold;
        for _ in 0..12 {
            warm = corner(&mut ui, at(10., 10., false));
        }
        assert_ne!(
            cold, 3.,
            "the resting radius is the theme's, not the hover one"
        );
        assert_eq!(warm, 3., "hovered, the declared radius is what paints");
    }

    /// A structural id remains a hit target for gestures, but it does not
    /// warm or animate. The semantic child still gets the default look.
    #[test]
    fn a_named_layout_surface_stays_cold_while_an_interactive_child_warms() {
        let tree = || {
            column([leaf(40., 40.)
                .fill(Role::Raised)
                .role(Kind::Button)
                .focusable()
                .id("child")])
            .size(100., 100.)
            .fill(Role::Background)
            .id("panel")
        };

        let mut ui = Ui::new(Theme::DEFAULT);
        let cold = ui
            .frame(tree(), None, PointerInput::default(), 0.016)
            .unwrap();
        let panel_cold = paint_color(&cold, "panel");
        let child_cold = paint_color(&cold, "child");
        let mut child_warm = child_cold;
        for _ in 0..12 {
            let frame = ui.frame(tree(), None, at(40., 10., false), 0.016).unwrap();
            child_warm = paint_color(&frame, "child");
        }
        assert!(ui.get("child").hovered, "the child is the topmost target");
        assert!(!ui.get("panel").hovered, "the parent is only underneath");
        assert_ne!(child_warm, child_cold, "the control warms");
        // The child loop's final frame also proves that the parent never
        // received an automatic tint while it sat underneath the child.
        let parent_warm = ui
            .scene()
            .map(|scene| {
                scene
                    .paint
                    .iter()
                    .find(|p| &*p.key == "panel")
                    .expect("panel paint")
                    .paint
                    .solid()
            })
            .expect("scene");
        assert_eq!(parent_warm, panel_cold, "the layout stays cold");

        // Resting on the empty part of the named parent remains interactive
        // for hit testing, without starting a useless animation loop.
        let mut ui = Ui::new(Theme::DEFAULT);
        ui.frame(tree(), None, PointerInput::default(), 0.016)
            .unwrap();
        let (panel_warm, animating) = {
            let frame = ui.frame(tree(), None, at(80., 80., false), 0.016).unwrap();
            (paint_color(&frame, "panel"), frame.animating)
        };
        assert!(ui.get("panel").hovered, "the parent still receives the hit");
        assert_eq!(panel_warm, panel_cold);
        assert!(!animating, "a structural hover has no spring");
    }

    /// A child rectangle can extend into the transparent corner of a rounded
    /// clipped parent, but that corner was never drawn and must not hit.
    #[test]
    fn a_rounded_clip_rejects_a_child_corner() {
        let tree = || {
            overlay([leaf(40., 40.)
                .fill(Role::Primary)
                .anchor(Align::Start, Align::Start)
                .id("child")])
            .size(100., 100.)
            .fill(Role::Field)
            .radius(20.)
            .clip()
            .id("panel")
        };
        let mut ui = Ui::new(Theme::DEFAULT);
        ui.frame(tree(), None, PointerInput::default(), 0.016)
            .unwrap();
        ui.frame(tree(), None, at(5., 5., false), 0.016).unwrap();
        assert!(
            !ui.get("child").hovered,
            "the child is outside the parent's rounded contour"
        );
        ui.frame(tree(), None, at(25., 25., false), 0.016).unwrap();
        assert!(ui.get("child").hovered, "the child is inside the clip");
    }

    /// Every nested clipped ancestor contributes its contour. A point in the
    /// outer rounded parent but outside the inner one cannot reach a child.
    #[test]
    fn nested_rounded_clips_intersect_for_hit_testing() {
        let tree = || {
            overlay([overlay([leaf(80., 80.)
                .fill(Role::Primary)
                .anchor(Align::Center, Align::Center)
                .id("target")])
            .size(60., 60.)
            .fill(Role::Raised)
            .radius(15.)
            .clip()
            .anchor(Align::Center, Align::Center)
            .id("inner")])
            .size(100., 100.)
            .fill(Role::Field)
            .radius(20.)
            .clip()
            .id("outer")
        };
        let mut ui = Ui::new(Theme::DEFAULT);
        ui.frame(tree(), None, PointerInput::default(), 0.016)
            .unwrap();
        ui.frame(tree(), None, at(21., 21., false), 0.016).unwrap();
        assert!(
            !ui.get("target").hovered,
            "the outer clip contains this point but the inner clip does not"
        );
        ui.frame(tree(), None, at(30., 30., false), 0.016).unwrap();
        assert!(ui.get("target").hovered, "the point is inside both clips");
    }

    /// An explicit hover look owns its fill channel. The automatic semantic
    /// fallback must not remap that already-resolved color a second time.
    #[test]
    fn an_explicit_hover_look_is_applied_once() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || {
            leaf(40., 40.)
                .fill(Role::Field)
                .role(Kind::Button)
                .on(State::Hover, |s| s.fill(Role::Primary))
                .id("button")
        };
        ui.frame(tree(), None, PointerInput::default(), 0.016)
            .unwrap();
        let mut warm;
        let frame = ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        warm = paint_color(&frame, "button");
        for _ in 0..12 {
            let frame = ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
            warm = paint_color(&frame, "button");
        }
        assert!(ui.get("button").hovered);
        assert_eq!(warm, role_color(Role::Primary));
    }

    /// The whole of M1 as one widget sees it: Shift is fine, and a
    /// secondary click is a click the caller can tell apart.
    #[test]
    fn shift_drags_a_value_fine_and_a_secondary_click_is_distinguishable() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || leaf(40., 40.).fill(Role::Raised).id("b");
        let drag = |ui: &mut Ui, mods: Mods| {
            // The hit map is last frame's, so a press needs a frame to land on.
            ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
            let press = PointerInput {
                mods,
                ..at(10., 10., true)
            };
            ui.frame(tree(), None, press, 0.016).unwrap();
            let moved = PointerInput {
                mods,
                ..at(90., 10., true)
            };
            ui.frame(tree(), None, moved, 0.016).unwrap();
            let mut v = 0.0;
            assert!(ui.drag("b", &mut v, 0.0..=1.0, 80.0, false));
            ui.frame(tree(), None, at(90., 10., false), 0.016).unwrap();
            v
        };
        let coarse = drag(&mut ui, Mods::default());
        let fine = drag(
            &mut ui,
            Mods {
                shift: true,
                ..Mods::default()
            },
        );
        assert!((coarse - 1.0).abs() < 1e-9, "80 px is the full span");
        assert!((fine - coarse * FINE_DRAG).abs() < 1e-9, "a tenth of it");

        ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        let secondary = PointerInput {
            buttons: Buttons::default().set(Button::Secondary, true),
            ..at(10., 10., false)
        };
        ui.frame(tree(), None, secondary, 0.016).unwrap();
        ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        let r = ui.get("b");
        assert!(r.clicked_with(Button::Secondary), "the reset gesture");
        assert!(!r.clicked_with(Button::Primary));
    }

    #[test]
    fn a_cancelled_gesture_still_ends() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || leaf(40., 40.).fill(Role::Raised).id("b");
        ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        ui.frame(tree(), None, at(10., 10., true), 0.016).unwrap();
        ui.cancel();
        let f = ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        assert_eq!(f.edits, vec![("b".to_owned(), Edit::End)]);
    }

    #[test]
    fn hover_warms_the_fill_and_a_press_is_reported_next_frame() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || leaf(40., 40.).fill(Role::Raised).role(Kind::Button).id("b");
        let base = ui
            .frame(tree(), None, PointerInput::default(), 0.016)
            .unwrap()
            .scene
            .paint[0]
            .paint
            .clone();
        ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        let f = ui.frame(tree(), None, at(10., 10., true), 0.016).unwrap();
        assert!(f.animating);
        assert_ne!(f.scene.paint[0].paint, base);
        let _ = ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        assert!(ui.get("b").released);
    }

    #[test]
    fn a_bouncy_transition_never_undershoots_a_channel_below_zero() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = |r: f64| {
            leaf(40., 40.)
                .fill(Role::Raised)
                .radius(r)
                .stroke(Role::Primary)
                .stroke_width(r / 4.)
                .transition(Spring::new(0.3, 0.6))
                .id("card")
        };
        ui.frame(tree(24.), None, PointerInput::default(), 0.016)
            .unwrap();
        for i in 0..120 {
            ui.frame(tree(0.), None, PointerInput::default(), 0.016)
                .unwrap_or_else(|e| panic!("frame {i} failed: {e:?}"));
        }
    }

    #[test]
    fn a_font_no_parser_accepts_is_dropped_rather_than_bricking_every_frame() {
        let mut ui = Ui::new(Theme::DEFAULT).font(vec![0u8; 64]);
        ui.frame(
            column([text("hello")]),
            None,
            PointerInput::default(),
            0.016,
        )
        .expect("the fontless path, not an error");
    }

    #[test]
    fn a_transitioning_node_seeds_each_channel_from_its_own_declaration() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let stroked = || {
            leaf(40., 40.)
                .stroke(Role::Primary)
                .stroke_width(12.)
                .transition(Spring::DEFAULT)
                .id("n")
        };
        ui.frame(stroked(), None, PointerInput::default(), 0.016)
            .unwrap();
        let f = ui
            .frame(
                stroked().fill(Role::Primary),
                None,
                PointerInput::default(),
                0.016,
            )
            .unwrap();
        let Some(Paint::Solid(c)) = f.scene.paint.iter().find_map(|p| match &p.paint {
            Paint::Solid(c) => Some(Paint::Solid(*c)),
            _ => None,
        }) else {
            panic!("expected a solid fill");
        };
        assert!(
            c.lightness() <= 1.0 && c.alpha() <= 1.0,
            "fill sprang in from the stroke width: {c:?}"
        );
    }

    #[test]
    fn a_gesture_edge_survives_a_frame_that_failed_to_resolve() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let good = || leaf(40., 40.).fill(Role::Raised).id("b");
        let bad = || good().radius(-1.);
        ui.frame(good(), None, at(10., 10., false), 0.016).unwrap();
        assert!(
            ui.frame(bad(), None, at(10., 10., true), 0.016).is_err(),
            "a negative radius does not resolve"
        );
        let f = ui.frame(good(), None, at(10., 10., true), 0.016).unwrap();
        assert_eq!(f.edits, vec![("b".to_owned(), Edit::Begin)]);
    }

    #[test]
    fn a_non_finite_wheel_delta_is_ignored() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || {
            column([leaf(20., 100.), leaf(20., 100.)])
                .height(50.)
                .scroll()
                .id("list")
        };
        let wheel = |y: f64| Input {
            pointer: at(10., 10., false),
            wheel: Point::new(0., y),
            ..Input::default()
        };
        ui.frame(tree(), None, PointerInput::default(), 0.016)
            .unwrap();
        ui.frame(tree(), None, wheel(f64::NAN), 0.016).unwrap();
        assert_eq!(ui.scroll("list"), [0., 0.]);
        ui.frame(tree(), None, wheel(30.), 0.016)
            .expect("and the surface still scrolls afterwards");
        assert_eq!(ui.scroll("list"), [0., 30.]);
    }

    #[test]
    fn a_degenerate_or_inverted_range_resolves_and_clamps() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let mut v = 1.0;
        let el = mui_widgets::slider(&mut ui, "fixed", "Fixed", &mut v, 1.0..=1.0).el();
        ui.frame(el, None, PointerInput::default(), 0.016)
            .expect("a fixed parameter is still a tree");
        let mut down = 0.5;
        let el = mui_widgets::slider(&mut ui, "down", "Down", &mut down, 1.0..=0.0).el();
        ui.frame(el, None, PointerInput::default(), 0.016)
            .expect("and so is a downward one");
    }

    #[test]
    fn a_live_readout_swaps_its_glyphs_without_resolving_again() {
        use std::cell::Cell;
        use std::rc::Rc;

        let walks = Rc::new(Cell::new(0));
        let (w, seen) = (walks.clone(), walks.clone());
        let tree = move || {
            let w = w.clone();
            row![
                text("0.0").reserve("-88.8").id("gain"),
                canvas(move |_| {
                    w.set(w.get() + 1);
                    Vec::new()
                })
                .size(10., 10.)
            ]
        };
        let mut ui = Ui::new(Theme::DEFAULT).font(epaint_default_fonts::HACK_REGULAR.to_vec());
        ui.frame(tree(), Some(Size::new(300., 40.)), Input::default(), 0.016)
            .unwrap();
        assert_eq!(seen.get(), 1, "the one resolve");

        let glyphs = |ui: &Ui| {
            let t = ui
                .scene()
                .unwrap()
                .paint
                .iter()
                .find(|p| p.layer == mui_scene::Layer::Text);
            t.unwrap().text.clone().unwrap().glyphs
        };
        let (before, frame) = (
            glyphs(&ui),
            ui.scene().unwrap().surface("gain").unwrap().frame,
        );
        let hit_geometry = ui.scene().unwrap().clone();
        for i in 0..32 {
            ui.set_text("gain", format!("-{i}.5")).unwrap();
        }
        assert_eq!(seen.get(), 1, "32 readouts, still one layout resolve");
        assert_ne!(glyphs(&ui), before, "and the glyphs did change");
        assert_eq!(ui.scene().unwrap().surface("gain").unwrap().frame, frame);
        assert!(same_hit_geometry(&hit_geometry, ui.scene().unwrap()));
    }

    #[test]
    fn set_text_says_so_when_there_is_nothing_to_set() {
        let mut ui = Ui::new(Theme::DEFAULT).font(epaint_default_fonts::HACK_REGULAR.to_vec());
        assert!(matches!(
            ui.set_text("gain", "1"),
            Err(SceneError::NoTextLayer)
        ));
        let tree = row![leaf(20., 20.).id("box")];
        ui.frame(tree, Some(Size::new(80., 40.)), Input::default(), 0.016)
            .unwrap();
        assert!(matches!(
            ui.set_text("box", "1"),
            Err(SceneError::NoTextLayer)
        ));
        assert!(matches!(
            ui.set_text("nope", "1"),
            Err(SceneError::NoTextLayer)
        ));
    }

    #[test]
    fn fallback_text_input_caret_uses_the_fallback_advance() {
        let ui = Ui::new(Theme::DEFAULT)
            .font(epaint_default_fonts::HACK_REGULAR.to_vec())
            .fallback_font(epaint_default_fonts::NOTO_EMOJI_REGULAR.to_vec());
        let value = "A😀";
        let emoji = value.char_indices().nth(1).unwrap().0;
        let before = ui.caret_x(value, 16., emoji);
        let end = ui.caret_x(value, 16., value.len());
        assert!(end > before, "fallback glyph has no caret advance");
        assert_eq!(ui.hit(value, 16., end), value.chars().count());
    }
}
