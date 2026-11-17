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
pub enum Target {
    /// Logical units.
    Xy(f64, f64),
    /// The centre of the surface with this id, resolved when the event fires.
    Key(String),
    /// The pointer position at the last press, plus this: a drag's path.
    Origin(f64, f64),
}

/// One primitive input, as a host delivers it.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Move(Target),
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
    pub act: Event,
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

/// The largest device-pixel side and area a session renders: 64 MiB of RGBA.
const MAX_SIDE: f64 = 16384.0;
const MAX_AREA: f64 = 16_777_216.0;

/// How a [`Session`] runs its scene. `Options::default()` is the scene's
/// own size at scale 1, its own theme, 60 frames a second.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Logical size; `None` is the scene's.
    pub size: Option<(u32, u32)>,
    /// Device scale: layout is logical, pixels are `size * scale`.
    pub scale: f64,
    /// Light or dark over the scene's own palette.
    pub theme: Option<Mode>,
    /// Steps a second of the virtual clock.
    pub fps: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            size: None,
            scale: 1.0,
            theme: None,
            fps: 60,
        }
    }
}

/// One scene on the virtual clock: send it input, step it, look at its tree
/// and its pixels. This is the CLI's engine, for a test harness to embed.
///
/// ```no_run
/// use mui::prelude::*;
/// use mui_headless::{Options, Scene, Session};
///
/// let mut on = false;
/// let scene = Scene::new("bypass", (200, 80), Ui::default(), move |ui| {
///     toggle(ui, "bypass", "Bypass", &mut on).el.into_el()
/// });
/// let mut s = Session::new(scene, Options::default()).unwrap();
/// s.click("bypass").unwrap();
/// s.settle(240).unwrap();
/// let rgba = s.rgba().unwrap();
/// assert_eq!(rgba.len(), 200 * 80 * 4);
/// ```
pub struct Session {
    ui: Ui,
    build: crate::Build,
    raster: Option<crate::Raster>,
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
    /// Open `scene` and run its two warm-up frames (the first has no gesture
    /// state; the second is what a host draws every frame), so an event
    /// addressed by id has a scene to resolve against.
    pub fn new(scene: Scene, opts: Options) -> Result<Self, Error> {
        let Options {
            size,
            scale,
            theme,
            fps,
        } = opts;
        if !(1..=crate::script::MAX_FPS).contains(&fps) || !(scale.is_finite() && scale > 0.0) {
            return Err(Error::new(format!(
                "fps must be 1..={} and scale positive",
                crate::script::MAX_FPS
            )));
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
        // One RGBA frame at the device size must fit the raster budget.
        let (dw, dh) = (f64::from(w) * scale, f64::from(h) * scale);
        if dw.round() > MAX_SIDE || dh.round() > MAX_SIDE || dw.round() * dh.round() > MAX_AREA {
            return Err(Error::new(format!(
                "{w}x{h} at scale {scale} is {dw:.0}x{dh:.0} device pixels; \
                 the limit is {MAX_SIDE} a side and {MAX_AREA} in all"
            )));
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
            raster: None,
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

    /// Queue `script`, its time 0 at the next step.
    pub fn play(&mut self, script: &crate::Script) {
        let now = self.frame;
        self.push(script.events.iter().map(|e| Ev {
            frame: e.frame + now,
            act: e.act.clone(),
        }));
    }

    /// Queue `event` for the next step. Two button changes never share a
    /// step: a press and its release land on consecutive ones.
    pub fn send(&mut self, event: Event) {
        self.push([Ev {
            frame: self.frame + 1,
            act: event,
        }]);
    }

    /// Whether queued input is still waiting for a step.
    pub fn pending(&self) -> bool {
        !self.queue.is_empty()
    }

    /// A primary click at the centre of `id` as last laid out: move, press,
    /// release, then one quiet step for the tree that reads the click.
    pub fn click(&mut self, id: &str) -> Result<(), Error> {
        for ev in [
            Event::Move(Target::Key(id.to_owned())),
            Event::Down(Button::Primary),
            Event::Up(Button::Primary),
        ] {
            self.send(ev);
        }
        while self.pending() {
            self.step()?;
        }
        self.step().map(drop)
    }

    /// Step until nothing animates and no input is queued, at most `max`
    /// steps; whether it settled.
    pub fn settle(&mut self, max: u32) -> Result<bool, Error> {
        for _ in 0..max {
            if !self.animating && !self.pending() {
                return Ok(true);
            }
            self.step()?;
        }
        Ok(!self.animating && !self.pending())
    }

    /// The last frame, rasterised on the CPU at the session's scale:
    /// straight RGBA, [`pixels`](Self::pixels) wide and high.
    pub fn rgba(&mut self) -> Result<Vec<u8>, Error> {
        let (w, h) = self.pixels();
        let scene = self
            .ui
            .scene()
            .ok_or_else(|| Error::new("the Ui has no scene yet"))?;
        self.raster
            .get_or_insert_with(|| crate::Raster::new(w, h))
            .draw(scene, self.scale)
    }

    /// The `Ui`, to focus a node or read its state between steps.
    pub fn ui_mut(&mut self) -> &mut Ui {
        &mut self.ui
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

    fn resolve(&self, p: &Target) -> Result<Point, Error> {
        match p {
            Target::Xy(x, y) => Ok(Point::new(*x, *y)),
            Target::Origin(dx, dy) => Ok(Point::new(self.origin.x + dx, self.origin.y + dy)),
            Target::Key(k) => {
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
            // it: the second button change waits for the next step, and so
            // does a move after it, or a press would land where the next
            // gesture starts.
            let button = matches!(ev.act, Event::Down(_) | Event::Up(_));
            if changed && (button || matches!(ev.act, Event::Move(_))) {
                break;
            }
            changed |= button;
            let Some(ev) = self.queue.pop_front() else {
                break;
            };
            match ev.act {
                Event::Move(p) => self.pointer = Some(self.resolve(&p)?),
                Event::Down(b) => {
                    self.buttons = self.buttons.set(b, true);
                    self.origin = self.pointer.unwrap_or(self.origin);
                }
                Event::Up(b) => self.buttons = self.buttons.set(b, false),
                Event::Wheel(dx, dy) => {
                    input.wheel = Vec2::new(input.wheel.x + dx, input.wheel.y + dy);
                }
                Event::Key(k) => input.keys.push(k),
                Event::Text(s) => input.text.push_str(&s),
                Event::Mods(m) => self.mods = m,
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
        let root = (self.build)(&mut self.ui, &input);
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
