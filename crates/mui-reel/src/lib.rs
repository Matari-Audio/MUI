//! A deterministic, offline trailer renderer for a MUI editor.
//!
//! A screen capture of a plugin is wall-clock footage: drag speed is however
//! fast the input driver happened to run, frames drop, audio is a separate
//! recording, and nothing can be seeked, re-themed or re-timed afterwards.
//! MUI does not need any of that. [`Ui::frame`] takes the caller's `dt`, the
//! springs are closed-form, the CPU rasteriser needs no window and [`Input`]
//! is a plain struct -- so a [`Script`] of pointer, key and camera cues can be
//! played against the *real* editor tree on a fixed step, and every frame is a
//! pure function of the script and the model. Two renders are byte-identical.
//!
//! One [`Reel::render`] writes the handoff folder a HyperFrames or Remotion
//! composition consumes: `take.mp4` (or a PNG sequence), `audio.wav`
//! sample-locked to the picture, `track.json` with every named surface's
//! rect per frame in video pixels, a dependency-free `mui-track.js`, a
//! HyperFrames `clip.html` sub-composition and a `manifest.json`.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use mui::motion::curve::Curve;
use mui::prelude::*;
use mui::vello::kurbo::{Affine, BezPath, Rect, Stroke};
use mui::vello::vello_cpu::{Pixmap, RenderContext, Resources};
use serde_json::{json, Value};

pub type Error = Box<dyn std::error::Error>;

/// A point on the script's clock: seconds, or beats at the reel's bpm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum At {
    Secs(f64),
    Beats(f64),
}
impl From<f64> for At {
    fn from(s: f64) -> Self {
        At::Secs(s)
    }
}
/// `n` beats from the top, at [`Reel::bpm`].
pub fn beat(n: f64) -> At {
    At::Beats(n)
}
/// A length of `n` beats -- the same thing as [`beat`], read as a duration.
pub fn beats(n: f64) -> At {
    At::Beats(n)
}

/// How a pointer path is paced between its ends.
#[derive(Clone, Debug, Default)]
pub enum Ease {
    /// Smootherstep: zero velocity and acceleration at both ends, which is
    /// what a hand looks like.
    #[default]
    Smooth,
    Linear,
    /// A normalized curve from the plugin's own curve editor.
    Curve(Curve),
    /// A spring released from 0 towards 1 at the gesture's start. Honest:
    /// the path ends where the spring is at the end of the duration, so pick
    /// a response shorter than the gesture.
    Spring(Spring),
}
impl Ease {
    fn at(&self, u: f64, secs: f64) -> f64 {
        let u = u.clamp(0.0, 1.0);
        match self {
            Ease::Smooth => u * u * u * (u * (u * 6.0 - 15.0) + 10.0),
            Ease::Linear => u,
            Ease::Curve(c) => f64::from(c.evaluate(u as f32)),
            Ease::Spring(s) => {
                let mut s = s.seeded(0.0);
                s.to(1.0);
                s.step(u * secs);
                s.value
            }
        }
    }
}

/// Something the audio closure and `track.json` hear about, on the video
/// frame it happened. Frame-quantised: it applies from the frame's first
/// sample.
#[derive(Clone, Debug, PartialEq)]
pub enum ReelEvent {
    NoteOn { key: u8, velocity: u8 },
    NoteOff { key: u8 },
    /// A gesture edge from [`Frame::edits`]: the automation bracket.
    Edit { id: String, begin: bool },
    /// A slider-, knob- or toggle-role surface changed its reported value.
    /// Read from the scene's semantics, so it needs nothing from the plugin.
    Value { id: String, value: f64 },
    Custom { name: String, value: f64 },
}
impl ReelEvent {
    fn json(&self, t: f64) -> Value {
        let t = round(t * 1000.0) / 1000.0;
        match self {
            ReelEvent::NoteOn { key, velocity } => {
                json!({"t": t, "kind": "note_on", "key": key, "velocity": velocity})
            }
            ReelEvent::NoteOff { key } => json!({"t": t, "kind": "note_off", "key": key}),
            ReelEvent::Edit { id, begin } => {
                json!({"t": t, "kind": if *begin { "edit_begin" } else { "edit_end" }, "id": id})
            }
            ReelEvent::Value { id, value } => {
                json!({"t": t, "kind": "value", "id": id, "value": value})
            }
            ReelEvent::Custom { name, value } => {
                json!({"t": t, "kind": "custom", "name": name, "value": value})
            }
        }
    }
}

enum Action<S> {
    /// Every pointer cue is one shape: go to `target` (+ `delta` over the
    /// duration when held), optionally with the primary button down for the
    /// whole path and a wheel delta on the first frame. A click is a held
    /// path of zero frames: down on one frame, up on the next.
    Pointer {
        target: String,
        delta: (f64, f64),
        dur: At,
        ease: Ease,
        hold: bool,
        wheel: Option<f64>,
    },
    Key(Key),
    Text(String),
    Focus {
        target: String,
        pad: f64,
        spring: Spring,
    },
    Reset(Spring),
    Event(ReelEvent),
    Call(Box<dyn Fn(&mut S)>),
}

/// What happens when. `.at(t)` moves the write head; every cue after it fires
/// on the frame nearest `t` until the next `.at`. Cues on one frame apply in
/// the order written, so give a click and a wheel on different targets their
/// own `.at`.
pub struct Script<S> {
    cues: Vec<(At, Action<S>)>,
    head: At,
    end: Option<At>,
    layers: Vec<String>,
    track: Option<Vec<String>>,
}
impl<S> Default for Script<S> {
    fn default() -> Self {
        Self {
            cues: Vec::new(),
            head: At::Secs(0.0),
            end: None,
            layers: Vec::new(),
            track: None,
        }
    }
}
impl<S> Script<S> {
    pub fn new() -> Self {
        Self::default()
    }
    fn cue(mut self, a: Action<S>) -> Self {
        self.cues.push((self.head, a));
        self
    }
    pub fn at(mut self, t: impl Into<At>) -> Self {
        self.head = t.into();
        self
    }
    /// Glide the pointer (button up) to a named surface's centre, resolved
    /// from the scene on that frame. A zero duration is a jump.
    pub fn move_to(self, id: &str, dur: impl Into<At>) -> Self {
        self.cue(Action::Pointer {
            target: id.into(),
            delta: (0.0, 0.0),
            dur: dur.into(),
            ease: Ease::Smooth,
            hold: false,
            wheel: None,
        })
    }
    /// Press on a surface's centre, move by `delta` logical units along the
    /// eased path, release. The runtime sees exactly a user's drag.
    pub fn drag(self, id: &str, delta: (f64, f64), dur: impl Into<At>, ease: Ease) -> Self {
        self.cue(Action::Pointer {
            target: id.into(),
            delta,
            dur: dur.into(),
            ease,
            hold: true,
            wheel: None,
        })
    }
    pub fn click(self, id: &str) -> Self {
        self.cue(Action::Pointer {
            target: id.into(),
            delta: (0.0, 0.0),
            dur: At::Secs(0.0),
            ease: Ease::Linear,
            hold: true,
            wheel: None,
        })
    }
    /// A wheel delta (scene units, positive down) over a surface's centre.
    pub fn wheel(self, id: &str, dy: f64) -> Self {
        self.cue(Action::Pointer {
            target: id.into(),
            delta: (0.0, 0.0),
            dur: At::Secs(0.0),
            ease: Ease::Linear,
            hold: false,
            wheel: Some(dy),
        })
    }
    pub fn key(self, key: Key) -> Self {
        self.cue(Action::Key(key))
    }
    pub fn text(self, s: &str) -> Self {
        self.cue(Action::Text(s.into()))
    }
    /// Spring the camera onto a named surface's frame plus `pad` logical
    /// units, fitted to the output. The camera is a paint transform only:
    /// input stays in scene space.
    pub fn camera_focus(self, id: &str, pad: f64, spring: Spring) -> Self {
        self.cue(Action::Focus {
            target: id.into(),
            pad,
            spring,
        })
    }
    pub fn camera_reset(self, spring: Spring) -> Self {
        self.cue(Action::Reset(spring))
    }
    pub fn note_on(self, key: u8, velocity: u8) -> Self {
        self.cue(Action::Event(ReelEvent::NoteOn { key, velocity }))
    }
    pub fn note_off(self, key: u8) -> Self {
        self.cue(Action::Event(ReelEvent::NoteOff { key }))
    }
    /// A named marker for the audio closure and the track, e.g. a cut.
    pub fn event(self, name: &str, value: f64) -> Self {
        self.cue(Action::Event(ReelEvent::Custom {
            name: name.into(),
            value,
        }))
    }
    /// Mutate the model before this frame is built: a preset, a theme swap.
    pub fn call(self, f: impl Fn(&mut S) + 'static) -> Self {
        self.cue(Action::Call(Box::new(f)))
    }
    /// The last frame is the one before `t`. Default: a second after the last cue.
    pub fn end(mut self, t: impl Into<At>) -> Self {
        self.end = Some(t.into());
        self
    }
    /// Also render these named parts, and the rest, as alpha layers aligned
    /// with the take.
    pub fn layers(mut self, ids: &[&str]) -> Self {
        self.layers = ids.iter().map(|s| s.to_string()).collect();
        self
    }
    /// Only these surfaces in `track.json`. Default: every named surface.
    pub fn track(mut self, ids: &[&str]) -> Self {
        self.track = Some(ids.iter().map(|s| s.to_string()).collect());
        self
    }
}

/// The per-frame audio callback: the model, this frame's events, and exactly
/// this frame's samples to fill (stereo, silence on entry).
pub type Audio<'a, S> = &'a mut dyn FnMut(&mut S, &[ReelEvent], &mut [[f32; 2]]);

/// The render settings. Logical size, device scale, frame rate.
#[derive(Clone)]
pub struct Reel {
    size: Size,
    scale: f64,
    fps: u32,
    bpm: Option<f64>,
    rate: u32,
    theme: Theme,
    font: Option<Font>,
    cursor: bool,
    encode: bool,
}

impl Reel {
    pub fn new(size: Size) -> Self {
        Self {
            size,
            scale: 1.0,
            fps: 60,
            bpm: None,
            rate: 48_000,
            theme: Theme::DEFAULT,
            font: None,
            cursor: false,
            encode: true,
        }
    }
    /// Device pixels per logical unit. The tree is laid out at `size` and
    /// rasterised through the scale, never upscaled.
    pub fn scale(mut self, scale: f64) -> Self {
        self.scale = scale;
        self
    }
    pub fn fps(mut self, fps: u32) -> Self {
        self.fps = fps;
        self
    }
    pub fn bpm(mut self, bpm: f64) -> Self {
        self.bpm = Some(bpm);
        self
    }
    pub fn sample_rate(mut self, rate: u32) -> Self {
        self.rate = rate;
        self
    }
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }
    pub fn font(mut self, font: Font) -> Self {
        self.font = Some(font);
        self
    }
    /// Draw a pointer sprite into the take (never into layers).
    pub fn cursor(mut self, on: bool) -> Self {
        self.cursor = on;
        self
    }
    /// `false` skips ffmpeg and writes PNG sequences even when it is there.
    pub fn encode(mut self, on: bool) -> Self {
        self.encode = on;
        self
    }

    fn secs(&self, t: At) -> Result<f64, Error> {
        match t {
            At::Secs(s) => Ok(s),
            At::Beats(b) => match self.bpm {
                Some(bpm) if bpm > 0.0 => Ok(b * 60.0 / bpm),
                _ => Err("a beat time needs Reel::bpm".into()),
            },
        }
    }
    /// The frame nearest `t`.
    pub fn frame_of(&self, t: At) -> Result<usize, Error> {
        Ok((self.secs(t)? * f64::from(self.fps)).round().max(0.0) as usize)
    }
    /// Video size in device pixels.
    pub fn pixels(&self) -> (u16, u16) {
        (
            (self.size.width * self.scale).round() as u16,
            (self.size.height * self.scale).round() as u16,
        )
    }

    /// Play `script` against `build` and write the handoff folder to `dir`.
    /// Returns the manifest that was written.
    pub fn render<S>(
        &self,
        script: &Script<S>,
        dir: &Path,
        state: &mut S,
        build: impl FnMut(&mut Ui, &mut S) -> El,
        audio: Option<Audio<'_, S>>,
    ) -> Result<Value, Error> {
        std::fs::create_dir_all(dir)?;
        let ffmpeg = self.encode
            && Command::new("ffmpeg")
                .arg("-version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
        let (w, h) = self.pixels();
        let layer_names: Vec<String> = script
            .layers
            .iter()
            .map(|s| s.replace('/', "_"))
            .chain((!script.layers.is_empty()).then(|| "rest".to_string()))
            .collect();
        let mut take = Sink::open(ffmpeg, dir, "take", w, h, self.fps, false)?;
        let mut layers = layer_names
            .iter()
            .map(|n| Sink::open(ffmpeg, &dir.join("layers"), n, w, h, self.fps, true))
            .collect::<Result<Vec<_>, _>>()?;
        let mut samples = Vec::new();
        let has_audio = audio.is_some();
        let track = self.run(script, state, build, audio, &mut |shot| {
            take.write(&shot.rgba)?;
            for (sink, rgba) in layers.iter_mut().zip(&shot.layers) {
                sink.write(rgba)?;
            }
            samples.extend_from_slice(&shot.samples);
            Ok(())
        })?;
        let video = take.finish()?;
        let layer_files = layers
            .into_iter()
            .map(Sink::finish)
            .collect::<Result<Vec<_>, _>>()?;
        let mut files = vec![video.clone(), "track.json".into(), "mui-track.js".into()];
        if has_audio {
            std::fs::write(dir.join("audio.wav"), wav(&samples, self.rate))?;
            files.push("audio.wav".into());
            if ffmpeg {
                let ok = Command::new("ffmpeg")
                    .args(["-y", "-loglevel", "error", "-i", "take.mp4", "-i", "audio.wav"])
                    .args(["-c:v", "copy", "-c:a", "aac", "-b:a", "320k", "-shortest"])
                    .arg("take-with-audio.mp4")
                    .current_dir(dir)
                    .status()?
                    .success();
                if ok {
                    files.push("take-with-audio.mp4".into());
                }
            }
        }
        let track_json = serde_json::to_string(&track)?;
        std::fs::write(dir.join("track.json"), &track_json)?;
        std::fs::write(dir.join("mui-track.js"), MUI_TRACK_JS)?;
        let duration = f64::from(track["frames"].as_u64().unwrap_or(0) as u32) / f64::from(self.fps);
        if ffmpeg {
            std::fs::write(
                dir.join("clip.html"),
                clip_html(w, h, duration, has_audio, &track_json),
            )?;
            files.push("clip.html".into());
        }
        let manifest = json!({
            "tool": "mui-reel",
            "version": env!("CARGO_PKG_VERSION"),
            "encoder": if ffmpeg { "ffmpeg: take H.264 High yuv420p crf 14; layers VP9 yuva420p" }
                       else { "none: ffmpeg missing or disabled, PNG sequences written instead" },
            "files": files,
            "layers": script.layers.iter().zip(&layer_names).map(|(id, f)| json!({"id": id, "file": format!("layers/{f}")}))
                .chain(layer_files.last().map(|f| json!({"id": null, "file": f})))
                .collect::<Vec<_>>(),
            "fps": self.fps,
            "size": [w, h],
            "logical": [self.size.width, self.size.height],
            "scale": self.scale,
            "bpm": self.bpm,
            "beats": self.bpm.map(|b| round(duration * b / 60.0 * 1000.0) / 1000.0),
            "duration": duration,
            "frames": track["frames"],
            "sample_rate": has_audio.then_some(self.rate),
            "cursor": self.cursor,
        });
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest)?,
        )?;
        Ok(manifest)
    }

    /// The whole fixed-step loop, handing each finished frame to `out`.
    /// Returns `track.json`'s value.
    fn run<S>(
        &self,
        script: &Script<S>,
        state: &mut S,
        mut build: impl FnMut(&mut Ui, &mut S) -> El,
        mut audio: Option<Audio<'_, S>>,
        out: &mut dyn FnMut(Shot) -> Result<(), Error>,
    ) -> Result<Value, Error> {
        if self.fps == 0 || !(self.scale.is_finite() && self.scale > 0.0) {
            return Err("fps and scale must be positive".into());
        }
        let fps = f64::from(self.fps);
        let dt = 1.0 / fps;
        let mut cues = script
            .cues
            .iter()
            .map(|(t, a)| Ok((self.frame_of(*t)?, a)))
            .collect::<Result<Vec<_>, Error>>()?;
        // Stable: same-frame cues keep the order they were written in.
        cues.sort_by_key(|c| c.0);
        let frames = match script.end {
            Some(t) => self.frame_of(t)?,
            None => cues.last().map_or(0, |c| c.0) + self.fps as usize,
        };
        let (w, h) = self.pixels();
        let mut ui = Ui::new(self.theme);
        if let Some(f) = &self.font {
            ui = ui.font(f.clone());
        }
        ui.scale = Some(self.scale);
        let (lw, lh) = (self.size.width, self.size.height);
        let home = [lw / 2.0, lh / 2.0, 1.0];
        let mut cam = home.map(Spring::at);
        let mut motion: Option<Motion> = None;
        let mut pos: Option<Point> = None;
        let mut raster = Raster::new(w, h);
        let mut values: BTreeMap<String, f64> = BTreeMap::new();
        let mut surfaces: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        let (mut pointer, mut camera, mut events) = (Vec::new(), Vec::new(), Vec::new());
        let mut next = 0;
        for i in 0..frames {
            let mut input = Input::default();
            let mut evs = Vec::new();
            while let Some(&(f, action)) = cues.get(next) {
                if f > i {
                    break;
                }
                next += 1;
                match action {
                    Action::Pointer {
                        target,
                        delta,
                        dur,
                        ease,
                        hold,
                        wheel,
                    } => {
                        let c = centre(surface(&ui, target, i)?);
                        let secs = self.secs(*dur)?;
                        motion = Some(Motion {
                            from: if *hold { c } else { pos.unwrap_or(c) },
                            to: Point::new(c.x + delta.0, c.y + delta.1),
                            start: i,
                            frames: (secs * fps).round() as usize,
                            secs,
                            ease: ease.clone(),
                            hold: *hold,
                        });
                        if let Some(dy) = wheel {
                            input.wheel = Point::new(0.0, *dy);
                        }
                    }
                    Action::Key(key) => input.keys.push(KeyPress {
                        key: *key,
                        mods: Mods::default(),
                    }),
                    Action::Text(s) => input.text.push_str(s),
                    Action::Focus {
                        target,
                        pad,
                        spring,
                    } => {
                        let r = surface(&ui, target, i)?;
                        let (fw, fh) = (r.size.width + 2.0 * pad, r.size.height + 2.0 * pad);
                        let zoom = (lw / fw).min(lh / fh);
                        let c = centre(r);
                        aim(&mut cam, [c.x, c.y, zoom], *spring);
                    }
                    Action::Reset(spring) => aim(&mut cam, home, *spring),
                    Action::Event(e) => evs.push(e.clone()),
                    Action::Call(f) => f(state),
                }
            }
            let mut down = false;
            if let Some(m) = &motion {
                let k = i - m.start;
                let u = if m.frames == 0 {
                    1.0
                } else {
                    k as f64 / m.frames as f64
                };
                let e = m.ease.at(u, m.secs);
                pos = Some(Point::new(
                    m.from.x + (m.to.x - m.from.x) * e,
                    m.from.y + (m.to.y - m.from.y) * e,
                ));
                down = m.hold && k <= m.frames;
            }
            input.pointer = PointerInput {
                pos,
                buttons: if down {
                    Buttons::PRIMARY
                } else {
                    Buttons::default()
                },
                mods: Mods::default(),
            };
            for s in &mut cam {
                s.step(dt);
            }
            let [cx, cy, zoom] = cam.map(|s| s.value);
            let view = Affine::translate((f64::from(w) / 2.0, f64::from(h) / 2.0))
                * Affine::scale(self.scale * zoom)
                * Affine::translate((-cx, -cy));

            let root = build(&mut ui, state);
            let frame = ui.frame(root, Some(self.size), input, dt)?;
            for (id, e) in &frame.edits {
                evs.push(ReelEvent::Edit {
                    id: id.clone(),
                    begin: *e == Edit::Begin,
                });
            }
            for s in frame.scene.surfaces() {
                let value = match s.semantics.as_ref().map(|m| &m.role) {
                    Some(Kind::Slider { value, .. }) => *value,
                    Some(Kind::Toggle { on }) => f64::from(u8::from(*on)),
                    _ => continue,
                };
                if let Some(old) = values.insert(s.key.to_string(), value) {
                    if old != value {
                        evs.push(ReelEvent::Value {
                            id: s.key.to_string(),
                            value,
                        });
                    }
                }
            }
            let cursor = (self.cursor).then_some(pos).flatten().map(|p| (to_device(view, p), down));
            let rgba = raster.draw(frame.scene, view, cursor.map(|c| (c, self.scale)))?;
            let mut layers = Vec::new();
            if !script.layers.is_empty() {
                let ids: Vec<&str> = script.layers.iter().map(String::as_str).collect();
                for id in &ids {
                    layers.push(raster.draw(&frame.scene.isolate(&[id])?, view, None)?);
                }
                layers.push(raster.draw(&frame.scene.without(&ids)?, view, None)?);
            }
            for s in frame.scene.surfaces() {
                let named = !s.key.starts_with('/');
                let wanted = script
                    .track
                    .as_ref()
                    .map_or(named, |t| t.iter().any(|k| **k == *s.key));
                if !wanted {
                    continue;
                }
                let f = s.frame;
                let r = view.transform_rect_bbox(Rect::new(
                    f.x,
                    f.y,
                    f.x + f.size.width,
                    f.y + f.size.height,
                ));
                let col = surfaces.entry(s.key.to_string()).or_default();
                col.resize(i, Value::Null);
                col.push(json!([round(r.x0), round(r.y0), round(r.width()), round(r.height())]));
            }
            drop(frame);
            for col in surfaces.values_mut() {
                col.resize(i + 1, Value::Null);
            }
            pointer.push(pos.map_or(Value::Null, |p| {
                let d = to_device(view, p);
                json!([round(d.x), round(d.y), down])
            }));
            camera.push(json!([
                round(cx * self.scale),
                round(cy * self.scale),
                round(zoom * 1000.0) / 1000.0
            ]));
            // Integer sample boundaries: frame i owns [i*rate/fps, (i+1)*rate/fps),
            // so any fps/rate pair sums exactly with no drift.
            let rate = u64::from(self.rate);
            let n = ((i as u64 + 1) * rate / u64::from(self.fps)
                - i as u64 * rate / u64::from(self.fps)) as usize;
            let mut samples = Vec::new();
            if let Some(a) = audio.as_mut() {
                samples = vec![[0.0f32; 2]; n];
                a(state, &evs, &mut samples);
            }
            let t = i as f64 * dt;
            events.extend(evs.iter().map(|e| e.json(t)));
            out(Shot {
                rgba,
                layers,
                samples,
            })?;
        }
        Ok(json!({
            "fps": self.fps,
            "size": [w, h],
            "scale": self.scale,
            "bpm": self.bpm,
            "duration": frames as f64 / fps,
            "frames": frames,
            "surfaces": surfaces,
            "pointer": pointer,
            "camera": camera,
            "events": events,
        }))
    }
}

/// One finished frame: the take's straight RGBA, each layer's, and its audio.
struct Shot {
    rgba: Vec<u8>,
    layers: Vec<Vec<u8>>,
    samples: Vec<[f32; 2]>,
}

struct Motion {
    from: Point,
    to: Point,
    start: usize,
    frames: usize,
    secs: f64,
    ease: Ease,
    hold: bool,
}

fn to_device(view: Affine, p: Point) -> Point {
    let d = view * mui::vello::kurbo::Point::new(p.x, p.y);
    Point::new(d.x, d.y)
}

fn round(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn centre(f: mui::layout::Frame) -> Point {
    Point::new(f.x + f.size.width / 2.0, f.y + f.size.height / 2.0)
}

/// A named surface in the scene the last frame resolved: the one this
/// frame's input is hit-tested against.
fn surface(ui: &Ui, id: &str, i: usize) -> Result<mui::layout::Frame, Error> {
    ui.scene()
        .and_then(|s| s.surface(id))
        .map(|s| s.frame)
        .ok_or_else(|| {
            format!("frame {i}: no surface '{id}' (the first frame has no scene yet)").into()
        })
}

/// Retarget the camera, carrying each channel's velocity so a new aim
/// mid-flight bends the path instead of kinking it.
fn aim(cam: &mut [Spring; 3], to: [f64; 3], spring: Spring) {
    for (s, t) in cam.iter_mut().zip(to) {
        *s = Spring {
            value: s.value,
            velocity: s.velocity,
            target: t,
            ..spring
        };
    }
}

/// The CPU rasteriser, reused across frames.
struct Raster {
    w: u16,
    h: u16,
    ctx: RenderContext,
    res: Resources,
}
impl Raster {
    fn new(w: u16, h: u16) -> Self {
        Self {
            w,
            h,
            ctx: RenderContext::new(w, h),
            res: Resources::default(),
        }
    }
    /// Straight RGBA of `scene` through `view`, with an optional pointer
    /// sprite at a device point and size scale.
    fn draw(
        &mut self,
        scene: &mui::scene::ResolvedScene,
        view: Affine,
        cursor: Option<((Point, bool), f64)>,
    ) -> Result<Vec<u8>, Error> {
        self.ctx.reset();
        mui::vello::paint(
            &mut mui::vello::Cpu {
                ctx: &mut self.ctx,
                resources: &mut self.res,
                cache: &mut mui::vello::Cache::default(),
            },
            scene,
            view,
        )
        .map_err(|e| format!("paint: {e:?}"))?;
        if let Some(((p, down), s)) = cursor {
            // A classic arrow, tip at the pointer; it dips when pressed.
            let mut path = BezPath::new();
            let pts = [
                (0.0, 0.0),
                (0.0, 17.0),
                (4.5, 13.0),
                (7.5, 19.5),
                (10.0, 18.5),
                (7.0, 12.0),
                (12.5, 12.0),
            ];
            let k = if down { 0.85 } else { 1.0 } * s;
            for (j, (x, y)) in pts.iter().enumerate() {
                let q = (p.x + x * k, p.y + y * k);
                if j == 0 {
                    path.move_to(q);
                } else {
                    path.line_to(q);
                }
            }
            path.close_path();
            self.ctx.set_transform(Affine::IDENTITY);
            self.ctx.set_paint(mui::vello::peniko::Color::WHITE);
            self.ctx.fill_path(&path);
            self.ctx.set_paint(mui::vello::peniko::Color::BLACK);
            self.ctx.set_stroke(Stroke::new(1.2 * s));
            self.ctx.stroke_path(&path);
        }
        self.ctx.flush();
        let mut pix = Pixmap::new(self.w, self.h);
        self.ctx.render(&mut pix, &mut self.res);
        Ok(pix
            .take_unpremultiplied()
            .iter()
            .flat_map(|p| [p.r, p.g, p.b, p.a])
            .collect())
    }
}

/// Where frames go: an ffmpeg child reading raw RGBA on stdin, or a PNG
/// sequence directory.
enum Sink {
    Ffmpeg(Child, String),
    Png(PathBuf, u32, u32, usize, String),
}
impl Sink {
    fn open(
        ffmpeg: bool,
        dir: &Path,
        name: &str,
        w: u16,
        h: u16,
        fps: u32,
        alpha: bool,
    ) -> Result<Self, Error> {
        std::fs::create_dir_all(dir)?;
        if !ffmpeg {
            let seq = dir.join(if alpha { name } else { "frames" });
            std::fs::create_dir_all(&seq)?;
            let rel = format!("{}/%05d.png", if alpha { name } else { "frames" });
            return Ok(Sink::Png(seq, w.into(), h.into(), 0, rel));
        }
        let file = format!("{name}.{}", if alpha { "webm" } else { "mp4" });
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgba"])
            .args(["-s", &format!("{w}x{h}"), "-r", &fps.to_string(), "-i", "-"]);
        if alpha {
            cmd.args(["-c:v", "libvpx-vp9", "-pix_fmt", "yuva420p", "-b:v", "0"])
                .args(["-crf", "24", "-row-mt", "1", "-deadline", "good", "-cpu-used", "4"]);
        } else {
            cmd.args(["-c:v", "libx264", "-profile:v", "high", "-pix_fmt", "yuv420p"])
                .args(["-crf", "14", "-preset", "slow", "-movflags", "+faststart"]);
        }
        let child = cmd
            .args(["-r", &fps.to_string()])
            .arg(&file)
            .current_dir(dir)
            .stdin(Stdio::piped())
            .spawn()?;
        Ok(Sink::Ffmpeg(child, file))
    }
    fn write(&mut self, rgba: &[u8]) -> Result<(), Error> {
        match self {
            Sink::Ffmpeg(child, _) => child
                .stdin
                .as_mut()
                .ok_or("ffmpeg stdin closed")?
                .write_all(rgba)?,
            Sink::Png(dir, w, h, n, _) => {
                let file = std::fs::File::create(dir.join(format!("{n:05}.png")))?;
                let mut enc = png::Encoder::new(std::io::BufWriter::new(file), *w, *h);
                enc.set_color(png::ColorType::Rgba);
                enc.write_header()?.write_image_data(rgba)?;
                *n += 1;
            }
        }
        Ok(())
    }
    /// Close the stream; the file or pattern it produced, relative to its dir.
    fn finish(self) -> Result<String, Error> {
        match self {
            Sink::Ffmpeg(mut child, file) => {
                drop(child.stdin.take());
                if !child.wait()?.success() {
                    return Err(format!("ffmpeg failed writing {file}").into());
                }
                Ok(file)
            }
            Sink::Png(.., rel) => Ok(rel),
        }
    }
}

/// 32-bit float stereo WAV. `fmt ` tag 3 (IEEE float), no `fact` chunk:
/// every reader that matters accepts it.
fn wav(samples: &[[f32; 2]], rate: u32) -> Vec<u8> {
    let data = (samples.len() * 8) as u32;
    let mut b = Vec::with_capacity(44 + data as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&3u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * 8).to_le_bytes());
    b.extend_from_slice(&8u16.to_le_bytes());
    b.extend_from_slice(&32u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for [l, r] in samples {
        b.extend_from_slice(&l.to_le_bytes());
        b.extend_from_slice(&r.to_le_bytes());
    }
    b
}

const MUI_TRACK_JS: &str = include_str!("mui-track.js");

/// The HyperFrames sub-composition: the take as a framework-owned clip, the
/// audio as its own `<audio>`, and the track inlined (no fetch at render
/// time) as `window.MUI_TRACK`.
fn clip_html(w: u16, h: u16, duration: f64, audio: bool, track: &str) -> String {
    let audio = if audio {
        format!(
            r#"<audio id="mui-reel-audio" src="audio.wav" data-start="0" data-duration="{duration}" data-track-index="1" data-volume="1"></audio>"#
        )
    } else {
        String::new()
    };
    include_str!("clip.html")
        .replace("{W}", &w.to_string())
        .replace("{H}", &h.to_string())
        .replace("{DURATION}", &duration.to_string())
        .replace("{AUDIO}", &audio)
        .replace("{TRACK_JS}", MUI_TRACK_JS)
        .replace("{TRACK}", track)
}

#[cfg(test)]
mod tests;
