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

use mui_geometry::{Bounds, Error, Path, Point};
use vello_common::kurbo::{BezPath, Rect, Shape as _};

/// How far the pointer may travel between press and release and still count as
/// a click. Past this, the gesture is a drag and [`Response::clicked`] never
/// fires. Four scene units is tight enough not to swallow a deliberate drag and
/// loose enough to survive a shaky hand on a trackpad.
pub const DRAG_THRESHOLD: f64 = 4.0;

struct Target {
    id: String,
    path: BezPath,
    /// Cheap reject. Most pointer positions miss most targets, and a winding
    /// number costs a walk over every segment.
    bounds: Rect,
    /// The nearest clipping ancestor's rect: outside it, the target is not
    /// drawn, so it must not respond either.
    clip: Option<Bounds>,
}

/// The targets under the pointer, in paint order.
///
/// Push in the order you draw: later entries sit on top and win ties.
#[derive(Default)]
pub struct Hit {
    targets: Vec<Target>,
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
        let path = mui_vello::bez_path(path, mui_vello::ARC_TOLERANCE)?;
        self.targets.push(Target {
            id: id.into(),
            bounds: path.bounding_box(),
            path,
            clip,
        });
        Ok(())
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
        let q = vello_common::kurbo::Point::new(p.x, p.y);
        let inside = |c: &Option<Bounds>| {
            c.is_none_or(|b| {
                p.x >= b.min.x && p.x <= b.max.x && p.y >= b.min.y && p.y <= b.max.y
            })
        };
        self.targets
            .iter()
            .rev()
            .find(|t| inside(&t.clip) && t.bounds.contains(q) && t.path.winding(q) != 0)
            .map(|t| t.id.as_str())
    }
}

/// One frame of raw pointer state. Whatever the windowing layer reports.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct PointerInput {
    /// `None` when the pointer left the surface entirely.
    pub pos: Option<Point>,
    pub primary_down: bool,
}

/// A key the host reports, already interpreted: a printable character or one
/// of the editing keys a text field has to handle.
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
}

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
    was_down: bool,
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
            was_down: false,
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
        // Edge-triggered flags last exactly one frame.
        self.pressed = None;
        self.released = None;
        self.clicked = None;
        self.dropped = None;
        self.drag_delta = Point::new(0., 0.);

        let over = input.pos.and_then(|p| hit.at(p)).map(str::to_owned);

        if input.primary_down && !self.was_down {
            if let Some(id) = over.clone() {
                self.press_pos = input.pos;
                self.dragging = false;
                self.pressed = Some(id.clone());
                self.active = Some(id);
            }
        } else if !input.primary_down && self.was_down {
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
        self.was_down = input.primary_down;
    }

    /// What happened to `id` this frame. Unknown ids report a default
    /// [`Response`], so a target that disappeared mid-gesture is inert rather
    /// than a panic.
    pub fn get(&self, id: &str) -> Response {
        let is = |slot: &Option<String>| slot.as_deref() == Some(id);
        let held = is(&self.active);
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
}

#[cfg(test)]
mod tests;
