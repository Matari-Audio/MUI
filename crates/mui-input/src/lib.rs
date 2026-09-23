//! Pointer interaction over MUI geometry.
//!
//! This is the part of an immediate-mode toolkit that is genuinely load-bearing
//! and is not drawing: deciding *what is under the pointer* and *what that
//! means*. In KURV's editor, `ui.painter()` is called 204 times and
//! `ui.interact()` 19 -- the painter is replaceable by Vello, this is not.
//!
//! Two pieces, deliberately separate:
//!
//! * [`Hit`] is geometry. It answers "which target is at this point" against
//!   the same Bezier paths the renderer fills, so hover cannot disagree with
//!   what you can see. Bounding-box hit tests always disagree at the corners,
//!   which is exactly where MUI spends its effort.
//! * [`Interaction`] is state across frames: pointer capture, click versus
//!   drag, and which target is held. It touches no geometry at all and is
//!   tested without a GPU or a window.
//!
//! Coordinates are scene units. If the scene is drawn under a transform, apply
//! its inverse to the pointer before calling -- there is no transform here on
//! purpose, because only the caller knows which of several scenes was clicked.
#![forbid(unsafe_code)]

pub use mui_geometry::Point;

use std::collections::HashMap;
use std::sync::Arc;

use mui_geometry::kurbo::{BezPath, Rect, Shape as _};
use mui_geometry::{Bounds, Error, Path};

/// How far the pointer may travel between press and release and still count as
/// a click. Past this, the gesture is a drag and [`Response::clicked`] never
/// fires. Four scene units is tight enough not to swallow a deliberate drag and
/// loose enough to survive a shaky hand on a trackpad.
pub const DRAG_THRESHOLD: f64 = 4.0;

struct Target {
    id: String,
    /// Which of the target's own shapes this is, for a canvas that named
    /// its draws. `None` for an ordinary surface.
    tag: Option<String>,
    path: BezPath,
    /// Cheap reject. Most pointer positions miss most targets, and a winding
    /// number costs a walk over every segment.
    bounds: Rect,
    /// The nearest clipping ancestor's rect: outside it, the target is not
    /// drawn, so it must not respond either.
    clip: Option<Bounds>,
    /// Cached exact clipping contours, outermost first. Pointer queries only
    /// run winding tests over these already-converted paths.
    clip_paths: Arc<[BezPath]>,
}

/// The targets under the pointer, in paint order.
///
/// Push in the order you draw: later entries sit on top and win ties.
#[derive(Default)]
pub struct Hit {
    targets: Vec<Target>,
    /// The scene shares one `Arc<[Arc<Path>]>` among all descendants of a clip.
    /// Cache its Bézier conversion by slice identity so tagged draws on one
    /// surface do not repeat validation or curve conversion.
    clip_cache: HashMap<(usize, usize), Arc<[BezPath]>>,
}

impl Hit {
    /// Add a target. The path is converted once, here, rather than per query.
    ///
    /// Returns the same error the renderer would: if geometry is malformed it
    /// is better to fail at registration than to leave a region silently dead.
    pub fn push(&mut self, id: impl Into<String>, path: &Path) -> Result<(), Error> {
        self.push_clipped(id, path, None)
    }

    /// Add a target clipped to `clip`: a point outside that rectangle misses,
    /// however the path winds. This is what makes a scrolled row stop
    /// responding once it has slid out of its viewport.
    pub fn push_clipped(
        &mut self,
        id: impl Into<String>,
        path: &Path,
        clip: Option<Bounds>,
    ) -> Result<(), Error> {
        self.push_clipped_paths(id, path, clip, None)
    }

    /// Add a target clipped by the exact contours of all its clipping
    /// ancestors. The paths are converted once while the hit map is built,
    /// never during pointer queries.
    pub fn push_clipped_paths(
        &mut self,
        id: impl Into<String>,
        path: &Path,
        clip: Option<Bounds>,
        clips: Option<&[Arc<Path>]>,
    ) -> Result<(), Error> {
        let clips = self.bez_clips(clips)?;
        self.add(id.into(), None, path, clip, clips)
    }

    /// Add one named shape of a target: a canvas's drawn ring, a knot, a
    /// cable. The gesture is still the node's -- `id` is what
    /// [`Hit::at`] reports -- and `tag` says which shape it landed on.
    ///
    /// ```
    /// # use mui_geometry::{Path, Point};
    /// # use mui_input::Hit;
    /// let square = [(0., 0.), (10., 0.), (10., 10.), (0., 10.)];
    /// let path = Path::polyline(square.map(|(x, y)| Point::new(x, y)), true);
    /// let mut hit = Hit::default();
    /// hit.push_tagged("plot", "knot-0", &path, None).unwrap();
    /// assert_eq!(hit.at_tagged(Point::new(5., 5.)), Some(("plot", Some("knot-0"))));
    /// assert_eq!(hit.at(Point::new(50., 5.)), None);
    /// ```
    pub fn push_tagged(
        &mut self,
        id: impl Into<String>,
        tag: impl Into<String>,
        path: &Path,
        clip: Option<Bounds>,
    ) -> Result<(), Error> {
        self.push_tagged_paths(id, tag, path, clip, None)
    }

    /// Add a named shape with the exact contours of all its clipping
    /// ancestors. This is the tagged counterpart to
    /// [`Hit::push_clipped_paths`].
    pub fn push_tagged_paths(
        &mut self,
        id: impl Into<String>,
        tag: impl Into<String>,
        path: &Path,
        clip: Option<Bounds>,
        clips: Option<&[Arc<Path>]>,
    ) -> Result<(), Error> {
        let clips = self.bez_clips(clips)?;
        self.add(id.into(), Some(tag.into()), path, clip, clips)
    }

    fn add(
        &mut self,
        id: String,
        tag: Option<String>,
        path: &Path,
        clip: Option<Bounds>,
        clip_paths: Arc<[BezPath]>,
    ) -> Result<(), Error> {
        let path = mui_geometry::bez_path(path, mui_geometry::ARC_TOLERANCE)?;
        self.targets.push(Target {
            id,
            tag,
            bounds: path.bounding_box(),
            path,
            clip,
            clip_paths,
        });
        Ok(())
    }

    fn bez_clips(&mut self, paths: Option<&[Arc<Path>]>) -> Result<Arc<[BezPath]>, Error> {
        let Some(paths) = paths.filter(|paths| !paths.is_empty()) else {
            return Ok(Arc::from([]));
        };
        let key = (paths.as_ptr() as usize, paths.len());
        if let Some(clips) = self.clip_cache.get(&key) {
            return Ok(clips.clone());
        }
        let clips: Arc<[BezPath]> = paths
            .iter()
            .map(|path| mui_geometry::bez_path(path, mui_geometry::ARC_TOLERANCE))
            .collect::<Result<Vec<_>, _>>()?
            .into();
        self.clip_cache.insert(key, clips.clone());
        Ok(clips)
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// The topmost target containing `p`, or `None`.
    ///
    /// Non-zero, matching the renderer: `vello_common` fills non-zero by
    /// default, and `mui-geometry` normalises every ring it emits -- exteriors
    /// wound positive, holes negative -- so a point in a counter is outside
    /// under either rule. Non-zero is the one that also survives geometry
    /// nobody normalised, which is what a run of glyph outlines is.
    pub fn at(&self, p: Point) -> Option<&str> {
        self.at_tagged(p).map(|(id, _)| id)
    }

    /// [`Hit::at`], plus which of that target's shapes was hit when it was
    /// pushed with [`Hit::push_tagged`].
    ///
    /// ```
    /// # use mui_geometry::{Path, Point};
    /// # use mui_input::Hit;
    /// let mut hit = Hit::default();
    /// let square = [(0., 0.), (10., 0.), (10., 10.), (0., 10.)];
    /// hit.push("plain", &Path::polyline(square.map(|(x, y)| Point::new(x, y)), true)).unwrap();
    /// assert_eq!(hit.at_tagged(Point::new(5., 5.)), Some(("plain", None)));
    /// ```
    pub fn at_tagged(&self, p: Point) -> Option<(&str, Option<&str>)> {
        self.at_tagged_with(p, |_, _, _| None)
    }

    /// Override only final containment. Broad bounds, paint order and the exact
    /// ancestor clip stack are still enforced. `None` uses the ordinary path.
    pub fn at_tagged_with(
        &self,
        p: Point,
        contains: impl Fn(&str, Option<&str>, Point) -> Option<bool>,
    ) -> Option<(&str, Option<&str>)> {
        if !(p.x.is_finite() && p.y.is_finite()) {
            return None;
        }
        let q = mui_geometry::kurbo::Point::new(p.x, p.y);
        let inside = |c: &Option<Bounds>| {
            c.is_none_or(|b| p.x >= b.min.x && p.x <= b.max.x && p.y >= b.min.y && p.y <= b.max.y)
        };
        self.targets
            .iter()
            .rev()
            .find(|t| {
                inside(&t.clip)
                    && t.clip_paths.iter().all(|clip| clip.winding(q) != 0)
                    && t.bounds.contains(q)
                    && contains(&t.id, t.tag.as_deref(), p)
                        .unwrap_or_else(|| t.path.winding(q) != 0)
            })
            .map(|t| (t.id.as_str(), t.tag.as_deref()))
    }
}

/// How much of a drag a gesture keeps while the fine modifier is held.
/// KURV's `FINE_DRAG_SCALE`: a tenth, which is the difference between
/// "somewhere near 3 kHz" and "3 kHz".
pub const FINE_DRAG: f64 = 0.1;

/// A pointer button. Primary is the one a click means; secondary opens a
/// menu or resets a parameter; middle is the pan grab.
///
/// ```
/// # use mui_input::{Button, Buttons};
/// assert!(Buttons::default().set(Button::Secondary, true).contains(Button::Secondary));
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    #[default]
    Primary,
    Secondary,
    Middle,
}

impl Button {
    /// In press-priority order: two buttons going down on one frame are read
    /// as the first of these.
    const ALL: [Self; 3] = [Self::Primary, Self::Secondary, Self::Middle];

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// Which buttons are down this frame. A set rather than a field per button,
/// so a fourth one costs a constant and nothing else.
///
/// ```
/// # use mui_input::{Button, Buttons};
/// let b = Buttons::default().set(Button::Primary, true);
/// assert!(b.contains(Button::Primary) && !b.contains(Button::Middle));
/// assert!(Buttons::default().is_empty());
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Buttons(u8);

impl Buttons {
    /// The primary button alone -- what a host reports for an ordinary click.
    ///
    /// ```
    /// # use mui_input::{Button, Buttons};
    /// assert!(Buttons::PRIMARY.contains(Button::Primary));
    /// ```
    pub const PRIMARY: Self = Self(Button::Primary.bit());

    /// Whether `b` is down.
    ///
    /// ```
    /// # use mui_input::{Button, Buttons};
    /// assert!(!Buttons::PRIMARY.contains(Button::Secondary));
    /// ```
    #[must_use]
    pub const fn contains(self, b: Button) -> bool {
        self.0 & b.bit() != 0
    }

    /// The set with `b` added or removed. Hosts report one button at a time,
    /// so this is the shape an event handler wants.
    ///
    /// ```
    /// # use mui_input::{Button, Buttons};
    /// let b = Buttons::PRIMARY.set(Button::Primary, false);
    /// assert!(b.is_empty());
    /// ```
    #[must_use]
    pub const fn set(self, b: Button, down: bool) -> Self {
        Self(if down {
            self.0 | b.bit()
        } else {
            self.0 & !b.bit()
        })
    }

    /// Whether no button is down.
    ///
    /// ```
    /// # use mui_input::Buttons;
    /// assert!(Buttons::default().is_empty());
    /// ```
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// One frame of raw pointer state. Whatever the windowing layer reports.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct PointerInput {
    /// `None` when the pointer left the surface entirely.
    pub pos: Option<Point>,
    pub buttons: Buttons,
    /// The modifiers held *now*, which is not the same question as the
    /// modifiers held at the press: a fine drag reads this one.
    pub mods: Mods,
}

/// A key the host reports, already interpreted: a printable character, one
/// of the editing keys a text field has to handle, or one of the keys only
/// a shortcut ever wants.
///
/// ```
/// # use mui_input::Key;
/// assert_eq!(Key::Function(1), Key::Function(1));
/// assert_ne!(Key::Function(1), Key::Function(2));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    /// The space bar as a *key*: the character also arrives as typed text,
    /// so a field inserts it from there and a shortcut reads it here.
    Space,
    PageUp,
    PageDown,
    /// `F1` is `Function(1)`. One variant rather than twelve, because a
    /// shortcut table compares the number.
    Function(u8),
}

/// The modifier keys held. Shared by [`KeyPress`] and [`PointerInput`]: a
/// gesture and a shortcut ask the same question.
///
/// ```
/// # use mui_input::Mods;
/// let fine = Mods { shift: true, ..Mods::default() };
/// assert!(fine.shift && !fine.alt);
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub cmd: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPress {
    pub key: Key,
    pub mods: Mods,
}

/// What the platform's input method reported. Mirrors winit's `Ime`: a
/// preedit is the text being composed and is *not* part of the field's value;
/// only `Commit` inserts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ime {
    Enabled,
    /// The composing text and, in bytes into it, the cursor range the IME
    /// wants shown. `None` hides the cursor; an empty string clears. The
    /// range comes straight off the platform: it is not trusted to fall on
    /// char boundaries, and whoever consumes it must check.
    Preedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    Commit(String),
    Disabled,
}

/// One frame of everything the host saw: pointer, wheel, keys, and the text
/// the platform's input method produced. `PointerInput` converts into one, so
/// a caller with no keyboard passes the pointer alone.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct Input {
    pub pointer: PointerInput,
    /// Scroll delta in scene units, positive right and down.
    pub wheel: Point,
    pub keys: Vec<KeyPress>,
    /// Composed text this frame -- not derivable from `keys`, which is why
    /// both exist.
    pub text: String,
    /// The host's clipboard contents, read *because* a paste key arrived this
    /// frame. `None` otherwise: nothing here reads the clipboard speculatively,
    /// and a field must not paste stale bytes it was handed last frame.
    pub clipboard: Option<String>,
    /// Input-method events since the last frame, in order.
    pub ime: Vec<Ime>,
}
impl From<PointerInput> for Input {
    fn from(pointer: PointerInput) -> Self {
        Self {
            pointer,
            ..Self::default()
        }
    }
}

/// What happened to one target this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Response {
    /// Pointer is over this target and no *other* target is capturing.
    pub hovered: bool,
    /// The press landed this frame.
    pub pressed: bool,
    /// Captured by a press and not yet released, wherever the pointer is now.
    pub held: bool,
    /// The capture ended this frame, click or not.
    pub released: bool,
    /// Pressed and released on this target without exceeding the threshold.
    pub clicked: bool,
    /// Held, and the pointer has moved past the threshold since the press.
    pub dragged: bool,
    /// Pointer movement since the previous frame while dragging. Zero
    /// otherwise, so a caller may add it unconditionally.
    pub drag_delta: Point,
    /// A drag started elsewhere is in flight and the pointer is over this
    /// target: highlight yourself, something is about to land.
    pub drop_target: bool,
    /// A drag was released over this target this frame.
    pub dropped_on: bool,
    /// Which button opened the capture, while this target has one.
    pub button: Option<Button>,
    /// The modifiers held now. A fine drag reads these, because letting go of
    /// Shift mid-drag must coarsen the rest of it.
    pub mods: Mods,
    /// The modifiers held when the press landed. A gesture whose *kind* was
    /// chosen at the press -- bend versus move, add versus remove -- reads
    /// these, so releasing Alt halfway does not change what it is doing.
    pub press_mods: Mods,
    /// Pointer travel since the press, zero unless dragging. Distinct from
    /// [`Response::drag_delta`], which is this frame alone.
    pub drag_total: Point,
}

/// Which way a drag is mostly going. Returned by [`Response::drag_axis`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

impl Response {
    /// This frame's drag movement with `fine` applied while Shift is held --
    /// pass [`FINE_DRAG`] unless the control wants its own ratio.
    ///
    /// ```
    /// # use mui_input::{Mods, Point, Response, FINE_DRAG};
    /// let r = Response {
    ///     drag_delta: Point::new(10.0, 0.0),
    ///     mods: Mods { shift: true, ..Mods::default() },
    ///     ..Response::default()
    /// };
    /// assert_eq!(r.drag_fine(FINE_DRAG).x, 1.0);
    /// ```
    #[must_use]
    pub fn drag_fine(&self, fine: f64) -> Point {
        self.drag_delta * if self.mods.shift { fine } else { 1.0 }
    }

    /// The axis this drag has travelled furthest along since its press, for a
    /// caller that wants to lock to one. `None` until it has moved at all, so
    /// a lock never picks an axis from a single noisy frame.
    ///
    /// ```
    /// # use mui_input::{Axis, Point, Response};
    /// let r = Response {
    ///     dragged: true,
    ///     drag_total: Point::new(2.0, 40.0),
    ///     ..Response::default()
    /// };
    /// assert_eq!(r.drag_axis(), Some(Axis::Y));
    /// ```
    #[must_use]
    pub fn drag_axis(&self) -> Option<Axis> {
        let d = self.drag_total;
        (self.dragged && (d.x != 0.0 || d.y != 0.0)).then(|| {
            if d.x.abs() >= d.y.abs() {
                Axis::X
            } else {
                Axis::Y
            }
        })
    }

    /// Clicked by one particular button. `clicked` alone says nothing about
    /// which, so a button widget asks for [`Button::Primary`] and a reset
    /// gesture asks for [`Button::Secondary`].
    ///
    /// ```
    /// # use mui_input::{Button, Response};
    /// let r = Response { clicked: true, button: Some(Button::Secondary), ..Response::default() };
    /// assert!(r.clicked_with(Button::Secondary) && !r.clicked_with(Button::Primary));
    /// ```
    #[must_use]
    pub fn clicked_with(&self, b: Button) -> bool {
        self.clicked && self.button == Some(b)
    }
}

/// Interaction state carried between frames.
///
/// The one rule worth stating: a press *captures* its target. Until release,
/// that target keeps receiving the gesture even if the pointer wanders off it,
/// and nothing else hovers. Without capture a slider stops tracking the instant
/// your hand drifts past its edge, which is the single most common way a
/// hand-rolled UI feels broken.
#[derive(Debug, Clone)]
pub struct Interaction {
    hovered: Option<String>,
    over: Option<String>,
    dropped: Option<(String, String)>,
    active: Option<String>,
    pressed: Option<String>,
    released: Option<String>,
    clicked: Option<String>,
    press_pos: Option<Point>,
    last_pos: Option<Point>,
    drag_delta: Point,
    dragging: bool,
    /// The buttons that were down last frame, for edge detection.
    was: Buttons,
    /// Which button opened the capture, and the modifiers it opened under.
    press_button: Option<Button>,
    press_mods: Mods,
    mods: Mods,
    threshold: f64,
}

impl Default for Interaction {
    fn default() -> Self {
        Self {
            hovered: None,
            over: None,
            dropped: None,
            active: None,
            pressed: None,
            released: None,
            clicked: None,
            press_pos: None,
            last_pos: None,
            drag_delta: Point::new(0., 0.),
            dragging: false,
            was: Buttons::default(),
            press_button: None,
            press_mods: Mods::default(),
            mods: Mods::default(),
            threshold: DRAG_THRESHOLD,
        }
    }
}

impl Interaction {
    pub fn new() -> Self {
        Self::default()
    }

    /// Override [`DRAG_THRESHOLD`] -- a touch screen wants a larger one.
    ///
    /// The threshold must be finite and non-negative. Zero is valid and makes
    /// any non-zero movement a drag; invalid values indicate a programmer
    /// configuration error and panic immediately.
    pub fn with_drag_threshold(mut self, px: f64) -> Self {
        assert!(
            px.is_finite() && px >= 0.0,
            "drag threshold must be finite and non-negative"
        );
        self.threshold = px;
        self
    }

    /// Cancel the current gesture and all one-frame edge state.
    ///
    /// The configured drag threshold is preserved so focus loss can cancel a
    /// capture without changing the interaction policy.
    pub fn cancel(&mut self) {
        let threshold = self.threshold;
        *self = Self {
            threshold,
            ..Self::default()
        };
    }

    /// Advance one frame. Call once, before reading any [`Response`].
    pub fn update(&mut self, hit: &Hit, input: PointerInput) {
        self.update_with(hit, input, |_, _, _| None);
    }

    /// Same gesture state machine with an optional non-path containment rule.
    pub fn update_with(
        &mut self,
        hit: &Hit,
        input: PointerInput,
        contains: impl Fn(&str, Option<&str>, Point) -> Option<bool>,
    ) {
        // Edge-triggered flags last exactly one frame.
        self.pressed = None;
        self.released = None;
        self.clicked = None;
        self.dropped = None;
        self.drag_delta = Point::new(0., 0.);

        let over = input
            .pos
            .and_then(|p| hit.at_tagged_with(p, &contains))
            .map(|(key, _)| key.to_owned());
        self.mods = input.mods;
        // A capture belongs to one button: a second one going down mid-drag
        // is ignored, and only the one that pressed can end the gesture.
        let newly = Button::ALL
            .into_iter()
            .find(|&b| input.buttons.contains(b) && !self.was.contains(b));

        if self.active.is_none() {
            if let (Some(b), Some(id)) = (newly, over.clone()) {
                self.press_pos = input.pos;
                self.press_button = Some(b);
                self.press_mods = input.mods;
                self.dragging = false;
                self.pressed = Some(id.clone());
                self.active = Some(id);
            }
        } else if self
            .press_button
            .is_some_and(|b| !input.buttons.contains(b))
        {
            if let Some(id) = self.active.take() {
                if self.dragging {
                    // The pointer may have left every target; a drag that
                    // lands nowhere is a drop back onto its source.
                    let target = over.clone().unwrap_or_else(|| id.clone());
                    self.dropped = Some((id.clone(), target));
                } else if over.as_deref() == Some(id.as_str()) {
                    self.clicked = Some(id.clone());
                }
                self.released = Some(id);
            }
            self.press_pos = None;
            self.dragging = false;
        }

        if self.active.is_some() {
            if let (Some(now), Some(before)) = (input.pos, self.last_pos) {
                self.drag_delta = now - before;
            }
            if let (Some(now), Some(origin)) = (input.pos, self.press_pos) {
                // Distance from the press, not accumulated travel: a wobble
                // that returns to where it started is still a click.
                if now.distance(origin) > self.threshold {
                    self.dragging = true;
                }
            }
        }

        // A captured target hovers only while the pointer is actually on it;
        // everything else stops hovering for the duration of the gesture.
        self.hovered = match &self.active {
            Some(a) if over.as_deref() == Some(a.as_str()) => over.clone(),
            Some(_) => None,
            None => over.clone(),
        };

        self.over = over;
        self.last_pos = input.pos;
        self.was = input.buttons;
    }

    /// What happened to `id` this frame. Unknown ids report a default
    /// [`Response`], so a target that disappeared mid-gesture is inert rather
    /// than a panic.
    pub fn get(&self, id: &str) -> Response {
        let is = |slot: &Option<String>| slot.as_deref() == Some(id);
        let held = is(&self.active);
        // Everything that names a button is about *this* target's gesture;
        // the edge flags only ever fire on the target that was captured.
        let mine = held || is(&self.pressed) || is(&self.released) || is(&self.clicked);
        Response {
            hovered: is(&self.hovered),
            pressed: is(&self.pressed),
            held,
            released: is(&self.released),
            clicked: is(&self.clicked),
            dragged: held && self.dragging,
            drag_delta: if held && self.dragging {
                self.drag_delta
            } else {
                Point::new(0., 0.)
            },
            drop_target: self.dragging && !held && is(&self.over),
            dropped_on: self.dropped.as_ref().is_some_and(|(_, t)| t == id),
            button: if mine { self.press_button } else { None },
            mods: self.mods,
            press_mods: if mine {
                self.press_mods
            } else {
                Mods::default()
            },
            drag_total: match (held && self.dragging, self.last_pos, self.press_pos) {
                (true, Some(now), Some(origin)) => now - origin,
                _ => Point::ZERO,
            },
        }
    }

    /// The press that landed this frame, before any capture moved.
    pub fn pressed(&self) -> Option<&str> {
        self.pressed.as_deref()
    }

    /// Source and target of a drag released this frame, for one frame. The
    /// target may be the source, when the drag ended where it began.
    pub fn dropped(&self) -> Option<(&str, &str)> {
        self.dropped.as_ref().map(|(a, b)| (a.as_str(), b.as_str()))
    }

    /// The target under the pointer, capture rules applied.
    pub fn hovered(&self) -> Option<&str> {
        self.hovered.as_deref()
    }

    /// The captured target, if a gesture is in progress.
    pub fn held(&self) -> Option<&str> {
        self.active.as_deref()
    }

    /// The button holding the gesture in flight, if there is one.
    pub fn held_button(&self) -> Option<Button> {
        self.active.as_ref().and(self.press_button)
    }
}

#[cfg(test)]
mod tests;
