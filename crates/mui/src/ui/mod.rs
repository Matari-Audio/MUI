//! The per-frame runtime: gestures in, animated styles applied, scene out.
use std::any::Any;
mod wake;
use crate::SemanticAction;
use std::collections::{BTreeSet, HashMap};
use wake::CallerWake;

use mui_geometry::{Point, Vec2};
use mui_input::{
    Button, Buttons, FINE_DRAG, Hit, Ime, Input, Interaction as Pointer, Key, KeyPress,
    PointerInput, Response,
};
use mui_layout::SpacingToken::{S, Xs};
use mui_scene::Keys;
use mui_scene::prelude::{Paints as _, Role, stack, text};
use mui_scene::{
    A11y, Appear, Area, Color, Cursor, El, Element, Fill, Font, Id, Layer, Mix, Paint, Painted,
    Palette, Pin, Radius, ResolvedScene, Resolver, SceneError, SceneSpec, Size, Spacing, Spring,
    State, Theme, bar, push_index,
};
use std::sync::Arc;

mod frame;
mod group;
mod input;
pub use input::TextInputState;
mod memo;
mod motion;
mod retain;
mod scroll;
mod settings;
mod text_runs;
use input::*;
use memo::*;
pub use motion::MotionPolicy;
use motion::*;
pub use text_runs::TextRuns;

#[cfg(test)]
mod audit_tests;

/// The id the floated tip carries. A leading `/` keeps it out of hit
/// testing, like every other key the runtime owns.
const TIP_KEY: &str = "/tip";

/// How long the pointer must rest on a surface before its tip is due.
pub const TIP_DELAY: f64 = 0.5;

/// Whether a hovered [`.tip()`](mui_scene::Styled::tip) shows, and after
/// how long: a plug-in's "show tooltips" preference. See [`Ui::set_tips`].
///
/// A tip is a hover hint only. It is not reported to a screen reader, on or
/// off: accessible help is [`.described(..)`](mui_scene::Styled::described),
/// which no tip policy touches.
///
/// ```
/// use mui::prelude::*;
/// assert_eq!(TipPolicy::default(), TipPolicy::After(0.5));
/// let mut ui = Ui::default();
/// ui.set_tips(TipPolicy::Off);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TipPolicy {
    /// No tip is floated, painted or woken for.
    Off,
    /// A tip comes due once the pointer has rested this many seconds.
    After(f64),
}
impl Default for TipPolicy {
    fn default() -> Self {
        Self::After(TIP_DELAY)
    }
}
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
/// let mut ui = Ui::default().clipboard(Memory("saw".into()));
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
    pub edits: &'a [(String, Edit)],
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
    theme: Theme,
    motion_policy: MotionPolicy,
    system_reduced_motion: bool,
    /// [`Ui::snap_pointer_states`].
    snap_pointer: bool,
    font: Option<Font>,
    /// Optional faces tried per grapheme after [`Self::font`].
    fallback_fonts: Vec<Font>,
    /// See [`Ui::text_run`].
    runs: TextRuns,
    /// See [`Ui::set_scale`].
    scale: Option<f64>,
    weld_backend: mui_scene::WeldBackend,
    gpu_welding_available: bool,
    /// See [`Ui::set_double_click`].
    double_click: f64,
    /// See [`Ui::set_tips`].
    tips: TipPolicy,
    interaction: Pointer,
    capture_pose: Option<mui_scene::LocalPose>,
    actions: Vec<SemanticAction>,
    hit: Hit,
    scene: Option<Arc<ResolvedScene>>,
    /// Everything kept under a node key between frames: see [`NodeState`].
    nodes: Nodes,
    /// Nodes that left the tree, still fading out where they stood.
    ghosts: Vec<Ghost>,
    /// Last frame's [`Styled::identity`](mui_scene::Styled::identity)
    /// nodes: what each was named and where it sat in the caller's tree.
    identities: HashMap<u64, (Option<String>, String)>,
    /// The runtime clock at which the last playing key lands.
    play_until: f64,
    /// The per-window budget used before traversal and during scene resolution.
    layout_limits: mui_layout::Limits,
    resolver: Resolver,
    /// Scratch for the tree path of the node a walk is on: an unnamed node's
    /// key, built without a heap copy per node.
    path: String,
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
    last_click: Option<ClickPress>,
    pending_click: Option<ClickPress>,
    /// Host timestamps are independent of per-frame animation delta.
    pointer_time: Option<std::time::Instant>,
    /// This frame's wheel, for [`Response::wheel`].
    wheel: Vec2,
    wheel_turned: Option<f64>,
    wheel_capture: Option<Id>,
    popup_wheel_scopes: Vec<Id>,
    /// Open modals, innermost last, each with the focus it took over and
    /// gives back when it goes. Only the innermost holds the keyboard.
    focus_traps: Vec<(Id, Option<String>)>,
    /// Ids that read [`Ui::wheel`] while the tree now being handed in was
    /// built: no scroll node they sit in takes the wheel over them.
    wheel_claims: Vec<String>,
    /// Where a button went down this frame, on a target or on nothing, for
    /// [`Ui::clicked_outside`].
    press_at: Option<Point>,
    focus: Option<String>,
    /// The focus came from the keyboard or from code, not a pointer press:
    /// see [`Ui::focus_visible`].
    focus_visible: bool,
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
    wake: Option<CallerWake>,
    /// The last frame resolved: see [`Ui::inert`].
    resolved: bool,
    /// Where along a held scrollbar's thumb the pointer took it, so the
    /// thumb follows the pointer from that point instead of jumping to it.
    bar_grab: f64,
    /// [`Ui::memo`] subtrees, by id.
    kept: HashMap<u64, Kept>,
    /// Each kept memo's id and the number the scene knows it by.
    memo_ids: rustc_hash::FxHashMap<Id, u64>,
    /// Scratch IDs for validating expanded memo trees, reused between frames.
    expanded_memos: std::collections::HashSet<u64>,
    next_memo: u64,
    /// This `Ui`'s key in [`TREES`].
    me: u64,
    /// Memos the next build must run again, because something inside them
    /// moved; `hot_all` for every one that reads input (discrete input
    /// arrived), `cold_all` for every one (theme, font, scale changed).
    hot: BTreeSet<u64>,
    hot_all: bool,
    cold_all: bool,
    /// Input reads so far ([`Ui::saw_input`]): a memo whose build read none
    /// is not rebuilt for input.
    input_reads: std::cell::Cell<u64>,
    /// Compare each frame's tree with the last and reuse what did not
    /// change; see [`Ui::set_auto_retain`].
    auto_retain: bool,
    /// Something every subtree reads changed (theme, font, scale): nothing
    /// is reused automatically on the next frame.
    auto_cold: bool,
    /// Keys whose runtime state moved outside a spring's flight since
    /// the last judgment (`set_scroll`, a tween snapping home): read by
    /// [`Ui::stirred`], cleared by `retain_auto`.
    touched: Vec<Id>,
    /// Frames judged for automatic retention.
    frames: u64,
    /// Each automatic boundary by its identity across frames.
    boundaries: rustc_hash::FxHashMap<u64, retain::Boundary>,
    /// Memo roots the build under way has handed out.
    issued: usize,
    /// The memos whose closures are running, innermost last, each with where
    /// its reads start in `read`, and whether all it read was at rest.
    building: Vec<(u64, usize, bool)>,
    /// The tween and play ids the running closures read.
    read: Vec<mui_scene::Id>,
    // A fully settled, whole-root memo can keep the scene allocation too.
    retained_root: Option<(u64, Option<Size>)>,
    retained_frame: bool,
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
                    && a.transform == b.transform
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
/// `Ui::new(Theme::DEFAULT)`.
impl Default for Ui {
    fn default() -> Self {
        Self::new(Theme::DEFAULT)
    }
}
impl Ui {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            motion_policy: MotionPolicy::default(),
            system_reduced_motion: false,
            snap_pointer: false,
            font: None,
            fallback_fonts: Vec::new(),
            runs: TextRuns::default(),
            scale: None,
            weld_backend: mui_scene::WeldBackend::Reference,
            gpu_welding_available: true,
            interaction: Pointer::new(),
            capture_pose: None,
            actions: Vec::new(),
            hit: Hit::default(),
            scene: None,
            nodes: Nodes::default(),
            ghosts: Vec::new(),
            identities: HashMap::new(),
            play_until: 0.0,
            layout_limits: mui_layout::Limits::default(),
            resolver: Resolver::default(),
            path: String::new(),
            pasted: None,
            copied: None,
            preedit: None,
            ime_caret: None,
            drag: None,
            double: None,
            double_click: DOUBLE_CLICK,
            tips: TipPolicy::default(),
            wheel: Vec2::ZERO,
            wheel_turned: None,
            wheel_capture: None,
            popup_wheel_scopes: Vec::new(),
            focus_traps: Vec::new(),
            wheel_claims: Vec::new(),
            press_at: None,
            last_click: None,
            pending_click: None,
            pointer_time: None,
            focus: None,
            focus_visible: false,
            edits: Vec::new(),
            delivered: Vec::new(),
            cancelled: None,
            keys: Vec::new(),
            typed: String::new(),
            pointer: PointerInput::default(),
            hover: None,
            tagged: None,
            time: 0.0,
            wake: None,
            resolved: false,
            board: None,
            bar_grab: 0.0,
            kept: HashMap::new(),
            memo_ids: rustc_hash::FxHashMap::default(),
            expanded_memos: std::collections::HashSet::new(),
            next_memo: 0,
            me: NEXT_UI.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            hot: BTreeSet::new(),
            hot_all: false,
            cold_all: false,
            input_reads: std::cell::Cell::new(0),
            auto_retain: true,
            auto_cold: false,
            touched: Vec::new(),
            frames: 0,
            boundaries: rustc_hash::FxHashMap::default(),
            issued: 0,
            building: Vec::new(),
            read: Vec::new(),
            retained_root: None,
            retained_frame: false,
        }
    }
    /// Borrow the last successfully resolved scene.
    pub fn scene(&self) -> Option<&ResolvedScene> {
        self.saw_input();
        self.scene.as_deref()
    }
    /// Confine wheel input to this popup and its descendants while it is mounted.
    pub fn capture_popup_wheel(&mut self, id: impl Into<Id>) {
        let id = id.into();
        if !self.popup_wheel_scopes.contains(&id) {
            self.popup_wheel_scopes.push(id);
        }
    }
    /// Keep keyboard focus inside this modal and its descendants while it is
    /// mounted: focus moves to its first stop, Tab cycles only its stops, and
    /// whatever was focused before gets focus back when it goes.
    pub fn trap_focus(&mut self, id: impl Into<Id>) {
        let id = id.into();
        if !self.focus_traps.iter().any(|(t, _)| *t == id) {
            let before = self.focus.clone();
            self.focus_traps.push((id, before));
        }
    }
    /// Retain the last scene in O(1), without borrowing this `Ui`.
    ///
    /// Hosts can take this snapshot under their model lock, then release the
    /// lock before native presentation or callbacks. Repeated snapshots share
    /// the same allocation until a frame resolves or a direct scene update is
    /// applied. Later updates never change a retained snapshot.
    ///
    /// Drop snapshots after use so the next frame can recycle their buffers.
    /// Direct [`Self::set_text`] and weld updates copy the scene only when a
    /// snapshot is still retained; normal frame commits never clone it.
    pub fn scene_snapshot(&self) -> Option<Arc<ResolvedScene>> {
        self.scene.as_ref().map(Arc::clone)
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
    /// let mut ui = Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    /// let tree = row![text("0.0").reserve("-88.8").id("gain")];
    /// ui.frame(tree, Some(Size::new(200., 40.)), PointerInput::default(), 0.016).unwrap();
    /// ui.set_text("gain", "-12.4").unwrap();
    /// ```
    pub fn set_text(
        &mut self,
        id: impl Into<Id>,
        s: impl AsRef<str>,
    ) -> Result<(), mui_scene::SceneError> {
        self.retained_root = None;
        let id: Id = id.into();
        let id = id.as_str();
        self.scene
            .as_mut()
            .map(Arc::make_mut)
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
        self.double = None;
        self.last_click = None;
        self.pending_click = None;
        self.interaction.cancel();
    }

    /// The first gesture edge on `id`, on the same frame boundary as [`Ui::get`].
    /// Use [`Ui::edits_for`] or [`Frame::edits`] for host dispatch: an atomic
    /// semantic action can begin AND end on the same frame.
    /// Pointer-only example:
    ///
    /// ```
    /// # use mui::{Edit, Ui}; use mui::prelude::*;
    /// # let mut ui = Ui::default();
    /// # let mut cutoff = 0.5;
    /// match ui.edit("cutoff") {
    ///     Some(Edit::Begin) => { /* host.begin_gesture(CUTOFF) */ }
    ///     Some(Edit::End) => { /* host.end_gesture(CUTOFF) */ }
    ///     None => {}
    /// }
    /// let el = slider(&mut ui, "cutoff", "Cutoff", &mut cutoff, 0.0..=1.0).el;
    /// ```
    pub fn edit(&self, id: impl Into<Id>) -> Option<Edit> {
        self.saw_input();
        let id: Id = id.into();
        let id = id.as_str();
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
    /// # let ui = Ui::default();
    /// let r = ui.get("lane");
    /// let zoom = 1.0 - r.wheel.y * 0.01;
    /// if r.double_clicked { /* reset */ }
    /// # assert_eq!(zoom, 1.0);
    /// ```
    pub fn get(&self, id: impl Into<Id>) -> Response {
        self.saw_input();
        let id: Id = id.into();
        let id = id.as_str();
        let mut response = self.interaction.get(id);
        if self.disabled_target(id) {
            // A release may close a custom control's bookkeeping even when
            // the target was disabled on that frame. Never turn it into an
            // activation or expose stale motion/keys.
            return Response {
                released: response.released,
                button: response.button.filter(|_| response.released),
                ..Response::default()
            };
        }
        if self.interaction.held() == Some(id)
            && let Some(pose) = self.capture_pose
        {
            response.drag_delta = pose.delta(response.drag_delta);
            response.drag_total = pose.delta(response.drag_total);
            response.drag_shifted = pose.delta(response.drag_shifted);
        }
        response.double_clicked = self.double.as_deref() == Some(id);
        response.key_activated = self
            .keys(id)
            .iter()
            .any(|k| matches!(k.key, Key::Enter | Key::Space));
        if self.wheel != Vec2::ZERO
            && let (Some(p), Some(s)) = (
                self.pointer.pos,
                self.scene.as_deref().and_then(|s| s.surface(id)),
            )
            && self.inside_surface(s, p)
            && self
                .scene
                .as_deref()
                .is_some_and(|scene| self.owns_wheel(scene, s))
        {
            response.wheel = s.local_pose().delta(self.wheel);
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
        let Some(surface) = self.scene.as_deref().and_then(|s| s.surface(action.id())) else {
            return false;
        };
        if surface.disabled {
            return false;
        }
        // Behind an open modal nothing answers, whatever route asks: the
        // scrim stops presses and the trap keys, this stops assistive tech.
        if let Some(trap) = self.focus_trap()
            && !self.in_group(trap.as_str(), Some(action.id()))
        {
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
                self.focus(Id::runtime(&id));
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
                    self.hot.extend(
                        self.scene
                            .as_ref()
                            .into_iter()
                            .flat_map(|s| s.memos_at(&id).map(|(id, _)| id)),
                    );
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
                self.hot.extend(
                    self.scene
                        .as_ref()
                        .into_iter()
                        .flat_map(|s| s.memos_at(&id).map(|(id, _)| id)),
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
                let step = surface.numeric_step.map_or_else(
                    || crate::widgets::step(&(min..=max)),
                    |step| step.copysign(max - min),
                ) * sign;
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
    pub fn edits_for(&self, id: impl Into<Id>) -> impl Iterator<Item = Edit> + '_ {
        self.saw_input();
        let id: Id = id.into();
        self.delivered
            .iter()
            .filter(move |(k, _)| *k == *id)
            .map(|(_, e)| *e)
    }

    /// Every gesture edge the last frame delivered, in order, for a host
    /// that dispatches them all rather than asking per control.
    pub fn edits(&self) -> &[(String, Edit)] {
        self.saw_input();
        // `edits` is the queue still to deliver; this is what went out.
        self.delivered.as_slice()
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
    /// # let mut ui = Ui::default();
    /// # let tree = col![block(40., 30.).min_size(Size::new(40., 30.))].pad(8.);
    /// # ui.frame(tree, Some(Size::new(400., 300.)), PointerInput::default(), 0.016).unwrap();
    /// assert_eq!(ui.min_size(), Some(Size::new(56., 46.)));
    /// ```
    pub fn min_size(&self) -> Option<Size> {
        Some(self.scene.as_deref()?.layout.min_size())
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
    /// `text` shaped and outlined in `font` (then the fallbacks) at `size`,
    /// for a canvas to draw: shaped once and kept, so a canvas redrawn every
    /// frame does not reshape its labels. `None` if the text cannot shape.
    /// [`TextRuns::get`] at the face's default axes.
    pub fn text_run(
        &mut self,
        font: &Font,
        text: &str,
        size: f64,
    ) -> Option<Arc<mui_text::TextRun>> {
        self.runs.get(font, text, size, &[])
    }
    /// The run cache behind [`Ui::text_run`] as a handle a canvas closure
    /// can keep: clone it in, call [`TextRuns::get`] there.
    pub fn text_runs(&self) -> TextRuns {
        self.runs.clone()
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
    /// Byte offsets and logical x positions for custom single-line text editors.
    /// Uses the scene's primary font and fallbacks at `size`, ending at the
    /// string's advance. Boundaries inside one shaping cluster share its
    /// leading edge. Without a font, positions use the scene's 0.6em estimate.
    /// Returns an empty vector when configured fonts cannot shape the text.
    pub fn carets(&self, s: &str, size: f64) -> Vec<(usize, f64)> {
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
}

/// Every node key's retained state, one entry per key.
///
/// ponytail: an unordered map; the one pass whose order shows (the ghosts'
/// paint order) sorts what it takes out of it.
type Nodes = rustc_hash::FxHashMap<mui_scene::Id, NodeState>;

/// What the runtime keeps under one node key between frames, each part
/// absent until something asks for it. The key is the node's id, its tree
/// path, or an id only [`Ui::tween`] and [`Ui::play`] read. A `bool` beside a
/// part is its seen flag: set by whatever reads it this frame, cleared by
/// [`NodeState::prune`], which drops a part left unseen.
#[derive(Default)]
struct NodeState {
    /// Hover and press springs, 0..1.
    springs: Option<[Spring; 2]>,
    /// Transition springs, one per paint channel under its stable id (see
    /// [`channels`]).
    motion: Option<Channels>,
    /// [`Styled::animate_layout`](mui_scene::Styled::animate_layout)
    /// springs: x, y, width, height, relative to the nearest animating
    /// ancestor.
    glide: Option<(bool, [Spring; 4])>,
    /// The spring an [`Appear`]ing node fades by: one gone from the tree
    /// leaves a ghost.
    appearing: Option<(bool, Spring)>,
    /// [`Styled::morph`](mui_scene::Styled::morph) state.
    morph: Option<Box<Morph>>,
    /// [`Ui::tween`]'s spring.
    tween: Option<(bool, Spring)>,
    /// [`Ui::play`]'s start time.
    play: Option<(bool, f64)>,
    /// A scroll node's springs per axis: the target is where the wheel
    /// clamped it, the value how far the children are drawn slid.
    scroll: Option<[Spring; 2]>,
    /// A text field's selection anchor and caret, in characters; equal when
    /// nothing is selected.
    sel: Option<(usize, usize)>,
    /// How far a multi-line field's lines are scrolled up, in units.
    text_scroll: Option<f64>,
    text_edit: Option<Box<TextEditState>>,
    /// What a widget keeps that is not the caller's value -- a picker's hue
    /// at zero saturation, a drag value's half-typed text.
    stash: Option<Box<dyn Any + Send>>,
}

#[derive(Default)]
struct TextEditState {
    goal: Option<f64>,
    upstream: bool,
    native: Vec<Ime>,
}
impl NodeState {
    /// End-of-frame upkeep: drop what nothing keeps. A seen part survives
    /// once more; so does an unseen one inside a reused memo (`kept`), whose
    /// nodes were not visited. Surface state goes with its surface (`here`).
    /// Returns whether anything is left.
    fn prune(&mut self, kept: impl Fn() -> bool, here: bool) -> bool {
        fn seen<T>(
            part: &mut Option<T>,
            flag: fn(&mut T) -> &mut bool,
            keep: &mut dyn FnMut() -> bool,
        ) {
            if let Some(v) = part
                && !std::mem::take(flag(v))
                && !keep()
            {
                *part = None;
            }
        }
        let mut memo = None;
        let mut kept = || *memo.get_or_insert_with(&kept);
        if self
            .springs
            .is_some_and(|[h, p]| h.value <= 0.0 && p.value <= 0.0 && h.settled() && p.settled())
        {
            self.springs = None;
        }
        seen(&mut self.motion, |m| &mut m.0, &mut kept);
        seen(&mut self.glide, |g| &mut g.0, &mut kept);
        seen(&mut self.appearing, |a| &mut a.0, &mut kept);
        seen(&mut self.morph, |m| &mut m.seen, &mut kept);
        seen(&mut self.tween, |t| &mut t.0, &mut || false);
        seen(&mut self.play, |p| &mut p.0, &mut || false);
        if !here {
            self.scroll = None;
            self.sel = None;
            self.text_scroll = None;
            self.text_edit = None;
            self.stash = None;
        }
        self.springs.is_some()
            || self.motion.is_some()
            || self.glide.is_some()
            || self.appearing.is_some()
            || self.morph.is_some()
            || self.tween.is_some()
            || self.play.is_some()
            || self.scroll.is_some()
            || self.sel.is_some()
            || self.text_scroll.is_some()
            || self.text_edit.is_some()
            || self.stash.is_some()
    }
}

/// `nodes[k]`, inserted empty when absent: the key is copied once, on
/// insertion, and not on every frame's lookup.
fn node<'m>(nodes: &'m mut Nodes, k: &str) -> &'m mut NodeState {
    if !nodes.contains_key(k) {
        nodes.insert(mui_scene::Id::runtime(k), NodeState::default());
    }
    nodes.get_mut(k).expect("inserted above")
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
            .field("nodes", &self.nodes.len())
            .finish()
    }
}

#[cfg(test)]
mod tests;
