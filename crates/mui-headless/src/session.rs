//! The virtual clock and the input path.
//!
//! A [`Session`] owns one [`Scene`]'s `Ui` and steps it on a fixed `1/fps`
//! grid: step `k` is the clock `k / fps`, handed to `Ui::frame` as `dt`, and
//! nothing else in this crate reads a clock. Scripted events are delivered the
//! way a real host delivers them, as one `Input` per frame: a pointer position
//! and held buttons, a wheel delta, key presses and typed text.
//!
//! The fixed-step driver is the one `media/mui-reel` plays a beat-timed script
//! with; this is the same loop with JSON-addressed events and no audio.

use std::collections::VecDeque;

use mui::prelude::*;
use mui::scene::ResolvedScene;

use crate::{Error, Scene};

/// Where a pointer event lands.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Pos {
    /// Logical units.
    Xy(f64, f64),
    /// The centre of the surface with this id, resolved when the event fires.
    Key(String),
    /// The pointer position at the last press, plus this: a drag's path.
    Origin(f64, f64),
}

/// One primitive input, due on a frame.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Act {
    Move(Pos),
    Down(Button),
    Up(Button),
    Wheel(f64, f64),
    Key(KeyPress),
    Text(String),
    /// Modifiers held from now on, as the pointer reports them.
    Mods(Mods),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Ev {
    /// The step (1-based) whose input carries this.
    pub frame: u64,
    pub act: Act,
}

/// What one step did, owned so the `Ui` is free again.
pub struct Step {
    pub frame: u64,
    pub animating: bool,
    pub cursor: Cursor,
    /// A tip that came due this frame, and where it was put.
    pub tip: Option<(String, Point)>,
    /// Gesture brackets this frame.
    pub edits: Vec<(String, Edit)>,
}

pub struct Session {
    ui: Ui,
    build: Box<dyn FnMut(&mut Ui) -> El>,
    pub(crate) size: Size,
    pub(crate) scale: f64,
    pub(crate) fps: u32,
    pub(crate) name: String,
    frame: u64,
    pointer: Option<Point>,
    buttons: Buttons,
    mods: Mods,
    origin: Point,
    queue: VecDeque<Ev>,
    last_tip: Option<(String, Point)>,
    cursor: Cursor,
    animating: bool,
    edits: Vec<(String, Edit)>,
}

/// The first step at or after `t` seconds. The epsilon keeps an exact
/// multiple of the frame time on its own step; step 0 is the warm-up, so the
/// earliest an event can land is step 1.
pub(crate) fn due(t: f64, fps: u32) -> u64 {
    ((t * f64::from(fps) - 1e-9).ceil().max(1.0)) as u64
}

impl Session {
    pub(crate) fn new(
        scene: Scene,
        size: Option<(u32, u32)>,
        scale: f64,
        theme: Option<Mode>,
        fps: u32,
    ) -> Result<Self, Error> {
        if fps == 0 || !(scale.is_finite() && scale > 0.0) {
            return Err(Error::new("fps and scale must be positive"));
        }
        let Scene {
            name,
            size: own,
            mut ui,
            build,
        } = scene;
        let (w, h) = size.unwrap_or(own);
        if w == 0 || h == 0 {
            return Err(Error::new("size must be non-zero"));
        }
        if let Some(mode) = theme {
            let t = ui.theme().clone();
            ui.set_theme(Theme {
                palette: t.palette.with_mode(mode),
                ..t
            });
        }
        ui.set_scale(Some(scale));
        let mut s = Self {
            ui,
            build,
            size: Size::new(f64::from(w), f64::from(h)),
            scale,
            fps,
            name,
            frame: 0,
            pointer: None,
            buttons: Buttons::default(),
            mods: Mods::default(),
            origin: Point::new(0.0, 0.0),
            queue: VecDeque::new(),
            last_tip: None,
            cursor: Cursor::Arrow,
            animating: false,
            edits: Vec::new(),
        };
        // Two frames at dt 0: the first has no gesture state, the second is
        // what a real host draws every frame (the same warm-up `snapshot`
        // does). Element-addressed events need this scene to resolve against.
        for _ in 0..2 {
            s.advance(Input::default(), 0.0)?;
        }
        Ok(s)
    }

    pub(crate) fn push(&mut self, events: impl IntoIterator<Item = Ev>) {
        let mut all: Vec<Ev> = self.queue.drain(..).chain(events).collect();
        // Stable: events on one frame keep the order they were written in.
        all.sort_by_key(|e| e.frame);
        self.queue = all.into();
    }

    /// Steps taken so far; the clock is `frame / fps`.
    pub fn frame(&self) -> u64 {
        self.frame
    }
    pub fn time(&self) -> f64 {
        self.frame as f64 / f64::from(self.fps)
    }
    pub fn ui(&self) -> &Ui {
        &self.ui
    }
    pub fn size(&self) -> Size {
        self.size
    }
    pub fn scale(&self) -> f64 {
        self.scale
    }
    /// The last frame's pointer position and held buttons.
    pub fn pointer(&self) -> (Option<Point>, Buttons) {
        (self.pointer, self.buttons)
    }
    pub fn cursor(&self) -> Cursor {
        self.cursor
    }
    pub fn tip(&self) -> Option<&(String, Point)> {
        self.last_tip.as_ref()
    }
    /// The scene the last step resolved.
    pub fn scene(&self) -> Result<&ResolvedScene, Error> {
        self.ui
            .scene()
            .ok_or_else(|| Error::new("the Ui has no scene yet"))
    }
    /// Device pixels.
    pub fn pixels(&self) -> (u16, u16) {
        (
            (self.size.width * self.scale).round().clamp(1.0, 65535.0) as u16,
            (self.size.height * self.scale).round().clamp(1.0, 65535.0) as u16,
        )
    }

    fn resolve(&self, p: &Pos) -> Result<Point, Error> {
        match p {
            Pos::Xy(x, y) => Ok(Point::new(*x, *y)),
            Pos::Origin(dx, dy) => Ok(Point::new(self.origin.x + dx, self.origin.y + dy)),
            Pos::Key(k) => {
                let scene = self.scene()?;
                let f = scene
                    .surface(k)
                    .ok_or_else(|| {
                        let named: Vec<String> = scene
                            .surfaces()
                            .filter(|s| Id::is_named(&s.key))
                            .map(|s| s.key.to_string())
                            .take(24)
                            .collect();
                        Error::new(format!(
                            "frame {}: no element with id '{k}' (named ids: {})",
                            self.frame + 1,
                            named.join(", ")
                        ))
                    })?
                    .frame;
                Ok(Point::new(
                    f.x + f.size.width / 2.0,
                    f.y + f.size.height / 2.0,
                ))
            }
        }
    }

    /// Advance one frame: deliver what is due, build the tree, `Ui::frame`.
    pub fn step(&mut self) -> Result<Step, Error> {
        let k = self.frame + 1;
        let mut input = Input::default();
        let mut changed = false;
        while let Some(ev) = self.queue.front() {
            if ev.frame > k {
                break;
            }
            // Down and up in one frame would be one frame with no press in
            // it: the second button change waits for the next step.
            let button = matches!(ev.act, Act::Down(_) | Act::Up(_));
            if button && changed {
                break;
            }
            changed |= button;
            let Some(ev) = self.queue.pop_front() else {
                break;
            };
            match ev.act {
                Act::Move(p) => self.pointer = Some(self.resolve(&p)?),
                Act::Down(b) => {
                    self.buttons = self.buttons.set(b, true);
                    self.origin = self.pointer.unwrap_or(self.origin);
                }
                Act::Up(b) => self.buttons = self.buttons.set(b, false),
                Act::Wheel(dx, dy) => {
                    input.wheel = Vec2::new(input.wheel.x + dx, input.wheel.y + dy);
                }
                Act::Key(k) => input.keys.push(k),
                Act::Text(s) => input.text.push_str(&s),
                Act::Mods(m) => self.mods = m,
            }
        }
        self.advance(input, 1.0 / f64::from(self.fps))?;
        self.frame = k;
        // Ui::frame's `edits` are borrowed from it; `advance` copied them.
        Ok(Step {
            frame: k,
            animating: self.animating,
            cursor: self.cursor,
            tip: self.last_tip.clone(),
            edits: std::mem::take(&mut self.edits),
        })
    }

    pub fn animating(&self) -> bool {
        self.animating
    }

    fn advance(&mut self, mut input: Input, dt: f64) -> Result<(), Error> {
        input.pointer = PointerInput {
            pos: self.pointer,
            buttons: self.buttons,
            mods: self.mods,
        };
        let root = (self.build)(&mut self.ui);
        let frame = self
            .ui
            .frame(root, Some(self.size), input, dt)
            .map_err(|e| Error::new(format!("frame {}: {e}", self.frame + 1)))?;
        self.animating = frame.animating;
        self.cursor = frame.cursor;
        self.last_tip = frame.tip;
        self.edits = frame.edits.iter().map(|(id, e)| (id.clone(), *e)).collect();
        Ok(())
    }
}
