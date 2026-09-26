//! The framework-free half of a native window host: the event queue, the
//! frame schedule, zoom and key routing, over a [`View`] and the retained
//! [`Ui`]. A window crate (mui-baseview, or any other) translates its native
//! events into [`Driver`] calls, calls [`Driver::advance`] once per display
//! tick, and presents the scene through `mui::vello::host::Host`.
//!
//! The tree and the pointer are in **scene units**: window points divided
//! by [`View::zoom`]. The surface is physical; [`Driver::ui_scale`] is the
//! paint transform's scale.
//!
//! Native events are queued and each edge gets its own `Ui::frame` on the
//! next tick, so a press and release that land between two ticks are still
//! two frames. Drag samples between two edges fold into one frame whose
//! [`Input::trail`] lists the skipped positions. A tick with no event, no
//! model change, no animation and no deadline due builds nothing, and
//! neither does a hover that [`Ui::inert`] says changes nothing.
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use mui_input::{Button, Input, Key, KeyPress, Mods, PointerInput, Vec2};
use mui_scene::prelude::{A11y, Cursor, El, Point, Size};

use crate::{Clipboard, Ui};

/// A frame after a stall advances time by at most this. Springs are closed
/// form and do not need it; a tooltip timer should not jump a whole idle
/// minute on the first hover after it.
const MAX_DT: f64 = 1.0;

/// What the window asks of whoever owns the model.
pub trait View: Send + 'static {
    /// This frame's tree, and the input the frame is about to take.
    fn build(&mut self, ui: &mut Ui, input: &Input) -> El;
    /// Whether the model moved outside the `Ui` since the last call --
    /// automation, a preset, a meter. Polled every display tick.
    fn changed(&mut self) -> bool;
    /// Ask the host for a new window size in logical points. A host may
    /// refuse, and the window clips rather than overflowing.
    fn request_resize(&mut self, width: u32, height: u32) -> bool;
    /// After each frame that laid out: read what it dispatched.
    fn after_frame(&mut self, ui: &mut Ui) {
        let _ = ui;
    }
    /// The gesture in flight was cancelled: focus left, or the window closes.
    fn cancel(&mut self, ui: &Ui) {
        let _ = ui;
    }
    /// A UI zoom on top of the window's scale: 2 draws everything twice as
    /// big and offers the tree half the logical size. Read once per tick.
    fn zoom(&self) -> f64 {
        1.0
    }
    /// Files dragged over the window at `at` (scene units); `dropped` on
    /// release. `true` accepts them as a copy.
    fn drop_files(&mut self, ui: &Ui, at: Point, paths: &[PathBuf], dropped: bool) -> bool {
        let _ = (ui, at, paths, dropped);
        false
    }
    /// An app-wide shortcut the window keeps even with no focused control
    /// (undo, help, escape). Everything else unfocused goes to the host.
    fn claims_key(&self, key: &Key, mods: Mods) -> bool {
        let _ = (key, mods);
        false
    }
    /// A line for the log: GPU failures, refused layouts, swallowed panics.
    fn log(&mut self, line: &str) {
        eprintln!("{line}");
    }
}

/// The retained `Ui` and the model: shared between the window thread and
/// whoever else touches the model (a plugin's editor locks it to end
/// gestures on close).
pub struct Shared<V> {
    pub ui: Ui,
    pub view: V,
}

/// Lock the shared state, through a poisoned lock.
pub fn lock<V>(shared: &Mutex<Shared<V>>) -> MutexGuard<'_, Shared<V>> {
    // A panic caught at an FFI edge (unwinding builds only) poisons the
    // lock; the state is still the last consistent frame's.
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A key as the window names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeKey {
    /// A key that types: `"a"`, `"A"`, `" "`, `"é"`.
    Text(String),
    /// A named key (`Key::from_name` reads the W3C names).
    Named(Key),
    /// A modifier key itself.
    Modifier(Modifier),
    /// Anything else (CapsLock, media keys): routed, never typed.
    Other,
}

/// A modifier key, for [`NativeKey::Modifier`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modifier {
    Shift,
    Ctrl,
    Alt,
    Cmd,
}

/// One native key press or release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    /// The physical key, any stable id: a release is matched to its press
    /// by it.
    pub code: u64,
    pub key: NativeKey,
    pub down: bool,
    /// The modifiers as the window reports them with the event.
    pub mods: Mods,
}

/// A wheel step as the window reports it: y positive scrolls up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Wheel {
    /// Lines; one is a text row of the theme.
    Lines(f64, f64),
    /// Window points.
    Pixels(f64, f64),
}

/// A native event, as the frame it will become. Each carries its pointer
/// snapshot, so a fast click is not coalesced into a motionless button.
/// The queue is unbounded on purpose: dropping an edge loses an automation
/// bracket, and the native event loop is already the bounded producer.
#[derive(Debug)]
enum Pending {
    Input(Input),
    /// Hover with no button: consecutive ones coalesce into the newest.
    Move(Input),
    /// A move with a button held: consecutive ones fold into one frame.
    Drag(Input),
    /// Focus went away mid-gesture.
    Cancel(PointerInput),
}

/// Who a key belongs to: the host, a focused control, a focused text field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyOwner {
    Host,
    Control,
    Text,
}

/// The event queue and the frame schedule of one window.
pub struct Driver {
    pending: VecDeque<Pending>,
    pointer: PointerInput,
    clipboard: Box<dyn Clipboard>,
    /// Physical pixels, and physical per window point.
    size: (u32, u32),
    scale: f64,
    /// [`View::zoom`], as of the last tick.
    zoom: f64,
    /// A wheel line in scene units: the theme's text size, as of last frame.
    line: f64,
    cursor: Cursor,
    last_frame: Instant,
    /// The model changed: rebuild.
    dirty: bool,
    animating: bool,
    wake_at: Option<Instant>,
    asked_to_grow: bool,
    /// A refused layout is reported once per run of refusals.
    failing: bool,
    /// Keys down, and whether the window kept each press.
    held: Vec<(u64, bool)>,
    /// At most one frame per this, when set: events wait for the next.
    pub min_interval: Option<Duration>,
}

impl Driver {
    /// A window of `size` physical pixels at `scale` physical per point.
    pub fn new(size: (u32, u32), scale: f64, clipboard: Box<dyn Clipboard>) -> Self {
        Self {
            pending: VecDeque::new(),
            pointer: PointerInput::default(),
            clipboard,
            size,
            scale,
            zoom: 1.0,
            line: 16.0,
            cursor: Cursor::Arrow,
            last_frame: Instant::now(),
            dirty: true,
            animating: false,
            wake_at: None,
            asked_to_grow: false,
            failing: false,
            held: Vec::new(),
            min_interval: None,
        }
    }

    /// Physical pixels.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Physical pixels per scene unit: the window's scale times the zoom.
    /// The paint transform.
    pub fn ui_scale(&self) -> f64 {
        self.scale * self.zoom
    }

    /// The cursor the last frame asked for.
    pub fn cursor(&self) -> Cursor {
        self.cursor
    }

    /// The pointer as the next frame will see it, scene units.
    pub fn pointer(&self) -> PointerInput {
        self.pointer
    }

    /// When the last frame ran; a headless driver steps from it.
    pub fn last_frame(&self) -> Instant {
        self.last_frame
    }

    /// Rebuild the tree on the next tick even if nothing it polls moved.
    pub fn redraw(&mut self) {
        self.dirty = true;
    }

    /// The window is now `size` physical pixels at `scale` per point.
    pub fn resized(&mut self, size: (u32, u32), scale: f64) {
        self.size = size;
        self.scale = scale;
        self.dirty = true;
    }

    /// The pointer moved to `at`, window points.
    pub fn pointer_moved(&mut self, at: Point, mods: Mods) {
        self.pointer.pos = Some(Point::new(at.x / self.zoom, at.y / self.zoom));
        self.pointer.mods = mods;
        self.push_move();
    }

    /// The pointer left the window.
    pub fn pointer_left(&mut self) {
        self.pointer.pos = None;
        self.push_move();
    }

    /// A button went down or up.
    pub fn button(&mut self, button: Button, down: bool, mods: Mods) {
        self.pointer.buttons = self.pointer.buttons.set(button, down);
        self.pointer.mods = mods;
        self.pending
            .push_back(Pending::Input(Input::from(self.pointer)));
    }

    /// A wheel or trackpad scroll.
    pub fn wheel(&mut self, wheel: Wheel, mods: Mods) {
        let (x, y) = match wheel {
            Wheel::Lines(x, y) => (x * self.line, y * self.line),
            Wheel::Pixels(x, y) => (x / self.zoom, y / self.zoom),
        };
        self.pointer.mods = mods;
        let mut input = Input::from(self.pointer);
        input.wheel = Vec2::new(x, -y);
        self.pending.push_back(Pending::Input(input));
    }

    /// The window gained or lost keyboard focus. Losing it cancels the
    /// gesture in flight and forgets the keys held.
    pub fn focus(&mut self, focused: bool) {
        if focused {
            self.pending
                .push_back(Pending::Input(Input::from(self.pointer)));
        } else {
            self.held.clear();
            self.pointer = PointerInput::default();
            self.pending.push_back(Pending::Cancel(self.pointer));
        }
    }

    /// The window closes: no tree is built from here on.
    pub fn close<V: View>(&mut self, s: &mut Shared<V>) {
        self.pending.clear();
        self.pointer = PointerInput::default();
        s.view.cancel(&s.ui);
    }

    /// Files dragged to `at`, window points. Moves the hover; returns
    /// whether the view accepts them.
    pub fn drop_files<V: View>(
        &mut self,
        s: &mut Shared<V>,
        at: Point,
        mods: Mods,
        paths: &[PathBuf],
        dropped: bool,
    ) -> bool {
        self.pointer_moved(at, mods);
        let at = self.pointer.pos.unwrap_or_default();
        s.view.drop_files(&s.ui, at, paths, dropped)
    }

    /// A key event. Returns whether the window keeps it; `false` hands it
    /// back to the host. A focused text field takes every key, a focused
    /// control only its navigation keys, and nothing else but
    /// [`View::claims_key`] -- so Space still starts a host's transport. A
    /// release goes where its press went. A key the host gets still
    /// carries the modifiers into the next frame, so Shift or Alt reaches a
    /// drag.
    pub fn key<V: View>(&mut self, s: &Shared<V>, key: &KeyEvent) -> bool {
        let kept = self.route(s, key);
        self.push_key(key, kept);
        kept
    }

    fn route<V: View>(&mut self, s: &Shared<V>, key: &KeyEvent) -> bool {
        if let Some(i) = self.held.iter().position(|(code, _)| *code == key.code) {
            let kept = self.held[i].1;
            if !key.down {
                self.held.swap_remove(i);
            }
            return kept;
        }
        if !key.down {
            return false;
        }
        let kept = captures(&key.key, key_owner(&s.ui))
            || shortcut_key(&key.key).is_some_and(|k| s.view.claims_key(&k, key.mods));
        self.held.push((key.code, kept));
        kept
    }

    fn push_key(&mut self, key: &KeyEvent, kept: bool) {
        let mut mods = key.mods;
        // X11 reports a press without its own modifier and a release still
        // with it: make the edge explicit, so Shift alone works.
        if let NativeKey::Modifier(m) = key.key {
            *match m {
                Modifier::Shift => &mut mods.shift,
                Modifier::Ctrl => &mut mods.ctrl,
                Modifier::Alt => &mut mods.alt,
                Modifier::Cmd => &mut mods.cmd,
            } = key.down;
        }
        self.pointer.mods = mods;
        let mut input = Input::from(self.pointer);
        if kept && key.down {
            typed(&mut input, &key.key, key.mods);
            if is_paste(&key.key, key.mods) {
                input.clipboard = self.clipboard.get();
            }
        }
        self.pending.push_back(Pending::Input(input));
    }

    /// Hover samples carry no edges: keep only the newest of a run with the
    /// same modifiers. Drag samples all stay; [`Driver::advance`] folds them
    /// into one frame with a trail.
    fn push_move(&mut self) {
        let input = Input::from(self.pointer);
        if input.pointer.buttons.is_empty() {
            if let Some(Pending::Move(previous)) = self.pending.back_mut()
                && previous.pointer.mods == input.pointer.mods
            {
                *previous = input;
                return;
            }
            self.pending.push_back(Pending::Move(input));
        } else {
            self.pending.push_back(Pending::Drag(input));
        }
    }

    /// Run the queued events and whatever else is due through `Ui::frame`.
    /// Returns whether there is a new scene to paint.
    pub fn advance<V: View>(&mut self, s: &mut Shared<V>, now: Instant) -> bool {
        self.dirty |= s.view.changed();
        let zoom = s.view.zoom();
        if zoom.is_finite() && zoom > 0.0 && zoom != self.zoom {
            self.zoom = zoom;
            self.dirty = true;
        }
        if self.size.0 == 0 || self.size.1 == 0 {
            // Minimised: hold the input edges until there is a size again.
            return false;
        }
        let due = self.wake_at.is_some_and(|at| now >= at);
        if s.ui.scene().is_some() && !self.dirty && !self.animating && !due {
            // A hover that lands on what it already hovers changes nothing.
            let ui = &mut s.ui;
            if self
                .pending
                .iter()
                .all(|p| matches!(p, Pending::Move(i) if ui.inert(i)))
            {
                self.pending.clear();
                return false;
            }
        }
        if self
            .min_interval
            .is_some_and(|min| s.ui.scene().is_some() && now < self.last_frame + min)
        {
            return false;
        }
        let dt = (now - self.last_frame).as_secs_f64().min(MAX_DT);
        self.last_frame = now;
        // Hit testing needs a scene: the first frame is neutral, and the
        // queued events then land on what it laid out.
        if s.ui.scene().is_none() && self.resolve(s, Input::default(), 0.0, now).is_err() {
            return false;
        }
        let mut timed = false;
        let mut laid_out = true;
        while let Some(event) = self.pending.pop_front() {
            let input = match event {
                Pending::Input(input) | Pending::Move(input) => input,
                Pending::Drag(mut input) => {
                    while let Some(Pending::Drag(next)) = self.pending.front()
                        && (next.pointer.buttons, next.pointer.mods)
                            == (input.pointer.buttons, input.pointer.mods)
                    {
                        let Some(Pending::Drag(next)) = self.pending.pop_front() else {
                            unreachable!()
                        };
                        input.trail.extend(input.pointer.pos);
                        input.pointer = next.pointer;
                    }
                    input
                }
                Pending::Cancel(pointer) => {
                    s.ui.cancel();
                    s.view.cancel(&s.ui);
                    Input::from(pointer)
                }
            };
            let last = self.pending.is_empty();
            // Only the last event of the tick carries the elapsed time.
            let event_dt = if last { dt } else { 0.0 };
            // A refused layout has still taken the event's pointer edge,
            // focus change and keys, and queued its gesture edges for the
            // next frame that resolves: replaying it would press or type
            // twice. Only its wheel is lost.
            laid_out = self.resolve(s, input, event_dt, now).is_ok();
            timed |= last;
        }
        if !timed {
            laid_out = self.resolve(s, Input::from(self.pointer), dt, now).is_ok();
        }
        if !laid_out {
            return false;
        }
        self.dirty = false;
        self.ask_to_grow(s);
        true
    }

    fn resolve<V: View>(
        &mut self,
        s: &mut Shared<V>,
        input: Input,
        dt: f64,
        now: Instant,
    ) -> Result<(), ()> {
        s.ui.set_scale(Some(self.ui_scale()));
        self.line = s.ui.theme().text;
        let root = s.view.build(&mut s.ui, &input);
        let offered = logical_size(self.size, self.ui_scale());
        match s.ui.frame(root, Some(offered), input, dt) {
            Ok(frame) => {
                self.failing = false;
                self.cursor = frame.cursor;
                // An edge is dispatched by the tree after the frame that
                // delivered it: that tree has to come even if nothing moves.
                self.animating = frame.animating || !frame.edits.is_empty();
                self.wake_at = frame.repaint_after.map(|after| now + after);
                if let Some(text) = frame.clipboard {
                    self.clipboard.set(&text);
                }
                // ponytail: `frame.ime` is dropped; no window crate here has
                // a candidate-window API to hand it to.
                s.view.after_frame(&mut s.ui);
                Ok(())
            }
            Err(e) => {
                if !self.failing {
                    s.view
                        .log(&format!("mui: layout refused at {offered:?}: {e}"));
                }
                self.failing = true;
                Err(())
            }
        }
    }

    /// The tree's measured floor, once per window, to the host: `min_size`
    /// is a hint a host may ignore, so a window opened below the floor gets
    /// one polite request to grow.
    fn ask_to_grow<V: View>(&mut self, s: &mut Shared<V>) {
        if std::mem::replace(&mut self.asked_to_grow, true) {
            return;
        }
        let (Some(floor), available) = (s.ui.min_size(), logical_size(self.size, self.scale))
        else {
            return;
        };
        // The floor is in scene units; the window is in points, zoom times.
        let (fw, fh) = (floor.width * self.zoom, floor.height * self.zoom);
        if available.width < fw || available.height < fh {
            let w = fw.max(available.width).ceil() as u32;
            let h = fh.max(available.height).ceil() as u32;
            s.view.request_resize(w, h);
        }
    }
}

/// `physical` pixels at `scale` per unit, in units.
pub fn logical_size(physical: (u32, u32), scale: f64) -> Size {
    Size::new(f64::from(physical.0) / scale, f64::from(physical.1) / scale)
}

fn key_owner(ui: &Ui) -> KeyOwner {
    let Some(id) = ui.focus_key() else {
        return KeyOwner::Host;
    };
    let text = ui
        .scene()
        .and_then(|s| s.surface(id))
        .and_then(|s| s.semantics.as_ref())
        .is_some_and(|sem| matches!(sem.role, A11y::TextInput { .. }));
    if text {
        KeyOwner::Text
    } else {
        KeyOwner::Control
    }
}

/// The generic policy; app shortcuts are [`View::claims_key`].
fn captures(key: &NativeKey, owner: KeyOwner) -> bool {
    match owner {
        KeyOwner::Text => true,
        KeyOwner::Control => matches!(
            key,
            NativeKey::Named(
                Key::Enter
                    | Key::Tab
                    | Key::Left
                    | Key::Right
                    | Key::Up
                    | Key::Down
                    | Key::Home
                    | Key::End
                    | Key::PageUp
                    | Key::PageDown
                    | Key::Delete
                    | Key::Backspace
            )
        ),
        KeyOwner::Host => false,
    }
}

/// A key as [`View::claims_key`] sees it: a named key, or a character as
/// `Key::Char`, lowercase, so Ctrl+Shift+Z is `z`.
fn shortcut_key(key: &NativeKey) -> Option<Key> {
    match key {
        NativeKey::Text(s) if s == " " => Some(Key::Space),
        NativeKey::Text(s) => s
            .chars()
            .next()
            .map(|c| c.to_ascii_lowercase())
            .map(Key::Char),
        NativeKey::Named(k) => Some(*k),
        NativeKey::Modifier(_) | NativeKey::Other => None,
    }
}

/// Space is both streams: text for a focused field, a key for a shortcut.
/// With Ctrl or Cmd held a character is a shortcut key; otherwise it is
/// text, never both, or every character would type twice.
fn typed(input: &mut Input, key: &NativeKey, mods: Mods) {
    match key {
        NativeKey::Text(s) if s == " " => {
            input.keys.push(KeyPress {
                key: Key::Space,
                mods,
            });
            if !mods.ctrl && !mods.cmd {
                input.text.push(' ');
            }
        }
        NativeKey::Text(s) if mods.ctrl || mods.cmd => {
            input.keys.extend(s.chars().map(|c| KeyPress {
                key: Key::Char(c),
                mods,
            }));
        }
        NativeKey::Text(s) => input.text.push_str(s),
        NativeKey::Named(key) => input.keys.push(KeyPress { key: *key, mods }),
        NativeKey::Modifier(_) | NativeKey::Other => {}
    }
}

fn is_paste(key: &NativeKey, mods: Mods) -> bool {
    (mods.ctrl || mods.cmd) && matches!(key, NativeKey::Text(s) if s.eq_ignore_ascii_case("v"))
}

#[cfg(test)]
mod tests;
