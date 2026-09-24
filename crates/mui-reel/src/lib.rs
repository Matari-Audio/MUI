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

/// How a pointer path is paced: the keyframe eases from `mui::motion`
/// (`Ease::IN_OUT`, `Ease::Cubic(..)` from a design tool, `Ease::Spring`).
pub use mui::motion::Ease;
use mui::motion::Keys;

/// Something the audio closure and `track.json` hear about, on the video
/// frame it happened. Frame-quantised: it applies from the frame's first
/// sample.
#[derive(Clone, Debug, PartialEq)]
pub enum ReelEvent {
    NoteOn {
        key: u8,
        velocity: u8,
    },
    NoteOff {
        key: u8,
    },
    /// A gesture edge from [`Frame::edits`]: the automation bracket.
    Edit {
        id: String,
        begin: bool,
    },
    /// A slider-, knob- or toggle-role surface changed its reported value.
    /// Read from the scene's semantics, so it needs nothing from the plugin.
    Value {
        id: String,
        value: f64,
    },
    Custom {
        name: String,
        value: f64,
    },
}
impl ReelEvent {
    fn json(&self, t: f64) -> Value {
        let t = (t * 1000.0).round() / 1000.0;
        match self {
            ReelEvent::NoteOn { key, velocity } => {
                json!({"t": t, "kind": "note_on", "key": key, "velocity": velocity})
            }
            ReelEvent::NoteOff { key } => json!({"t": t, "kind": "note_off", "key": key}),
            ReelEvent::Edit { id, begin } => {
                json!({"t": t, "kind": if *begin { "edit_begin" } else { "edit_end" }, "id": id})
            }
            ReelEvent::Value { id, value } => {
                json!({"t": t, "kind": "value", "id": id, "value": (value * 1e4).round() / 1e4})
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
            ease: Ease::IN_OUT,
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

/// Draws the take's picture for [`Reel::render_through`]: one subframe in,
/// `Reel::pixels()` straight RGBA out.
pub type Look<'a> = &'a mut dyn FnMut(&Take<'_>) -> Result<Vec<u8>, Error>;

/// One subframe, as a [`Look`] sees it.
pub struct Take<'a> {
    pub scene: &'a mui::scene::ResolvedScene,
    /// Absolute seconds.
    pub t: f64,
    /// The script's camera: the logical point at the frame's centre, and the
    /// zoom (1 is the whole canvas).
    pub camera: [f64; 3],
    /// The pointer in logical canvas units and whether it is pressed, when
    /// the reel draws a cursor. [`with_cursor`] paints it into the scene.
    pub pointer: Option<(Point, bool)>,
}

/// `scene` with the reel's arrow pointer painted on top at `at` (logical
/// units), dipping when `down`: a look that puts the UI on a slab gets the
/// pointer on the same surface.
pub fn with_cursor(
    scene: &mui::scene::ResolvedScene,
    at: Point,
    down: bool,
) -> mui::scene::ResolvedScene {
    use mui::scene::{Layer, Paint, Painted};
    let k = if down { 0.85 } else { 1.0 };
    let arrow = std::sync::Arc::new(mui::geometry::Path::polyline(
        ARROW.map(|(x, y)| Point::new(at.x + x * k, at.y + y * k)),
        true,
    ));
    let mut out = scene.clone();
    for (layer, color, width) in [
        (Layer::Fill, Color::srgb(1.0, 1.0, 1.0), 0.0),
        (Layer::Stroke, Color::srgb(0.0, 0.0, 0.0), 1.2),
    ] {
        out.paint.push(Painted {
            key: "mui-reel/cursor".into(),
            layer,
            path: arrow.clone(),
            paint: Paint::Solid(color),
            rect: None,
            width,
            blur: 0.0,
            text: None,
        });
    }
    out
}

/// A classic arrow, tip at the origin, in logical units.
const ARROW: [(f64, f64); 7] = [
    (0.0, 0.0),
    (0.0, 17.0),
    (4.5, 13.0),
    (7.5, 19.5),
    (10.0, 18.5),
    (7.0, 12.0),
    (12.5, 12.0),
];
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
    master: bool,
    ten_bit: bool,
    blur: u32,
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
            master: false,
            ten_bit: false,
            blur: 1,
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

    /// Also write `take.mov`, ProRes 4444 with alpha, as the grading master;
    /// layers then go to ProRes 4444 `.mov` too, the alpha container every
    /// platform decodes (VP9 alpha in WebM does not decode on Windows).
    pub fn master(mut self, on: bool) -> Self {
        self.master = on;
        self
    }
    /// Deliver the take as 10-bit H.265 (`yuv420p10le`) instead of 8-bit H.264.
    pub fn ten_bit(mut self, on: bool) -> Self {
        self.ten_bit = on;
        self
    }
    /// Render `n` subframes per frame, at `t + k / (n * fps)`, and average
    /// them. The UI is stepped at `dt / n`, so springs and drags blur along
    /// their real paths. `1` (the default) is off.
    pub fn motion_blur(mut self, n: u32) -> Self {
        self.blur = n.max(1);
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
    /// The first frame at or after `t`: a cue between frames (116 bpm is
    /// 31.03 frames a beat) fires on the next one, never early.
    pub fn frame_of(&self, t: At) -> Result<usize, Error> {
        Ok(first(self.secs(t)?, f64::from(self.fps)))
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
        self.render_through(script, dir, state, build, audio, None)
    }

    /// [`Reel::render`], with the take's pictures drawn by `look` instead of
    /// the CPU rasteriser: `mui-stage` puts the scripted UI on a 3D slab.
    /// It gets every subframe's scene and absolute time and returns
    /// `pixels()`-sized RGBA; the reel still averages the subframes, writes
    /// alpha layers on the CPU, and records the 2D track. The cursor is the
    /// look's to draw.
    pub fn render_through<S>(
        &self,
        script: &Script<S>,
        dir: &Path,
        state: &mut S,
        build: impl FnMut(&mut Ui, &mut S) -> El,
        audio: Option<Audio<'_, S>>,
        mut look: Option<Look<'_>>,
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
        let open = |dir: &Path, name: &str, codec| {
            Sink::open(ffmpeg.then_some(codec), dir, name, w, h, self.fps)
        };
        let layer_codec = if self.master {
            Codec::ProRes
        } else {
            Codec::Vp9
        };
        let mut take = open(
            dir,
            "take",
            if self.ten_bit {
                Codec::H265
            } else {
                Codec::H264
            },
        )?;
        let mut master = match (ffmpeg, self.master) {
            (true, true) => Some(open(dir, "take", Codec::ProRes)?),
            _ => None,
        };
        let mut layers = script
            .layers
            .iter()
            .map(|s| s.replace('/', "_"))
            .chain((!script.layers.is_empty()).then(|| "rest".to_string()))
            .map(|n| open(&dir.join("layers"), &n, layer_codec))
            .collect::<Result<Vec<_>, _>>()?;
        let mut samples = Vec::new();
        let has_audio = audio.is_some();
        let track = self.run(script, state, build, audio, &mut look, &mut |shot| {
            take.write(&shot.rgba)?;
            if let Some(m) = &mut master {
                m.write(&shot.rgba)?;
            }
            for (sink, rgba) in layers.iter_mut().zip(&shot.layers) {
                sink.write(rgba)?;
            }
            samples.extend_from_slice(&shot.samples);
            Ok(())
        })?;
        let mut files = vec![take.finish()?];
        if let Some(m) = master {
            files.push(m.finish()?);
        }
        let layer_files = layers
            .into_iter()
            .map(Sink::finish)
            .collect::<Result<Vec<_>, _>>()?;
        files.extend([
            "track.json".into(),
            "track.js".into(),
            "mui-track.js".into(),
        ]);
        if has_audio {
            std::fs::write(dir.join("audio.wav"), wav(&samples, self.rate))?;
            files.push("audio.wav".into());
            if ffmpeg {
                let ok = Command::new("ffmpeg")
                    .args([
                        "-y",
                        "-loglevel",
                        "error",
                        "-i",
                        "take.mp4",
                        "-i",
                        "audio.wav",
                    ])
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
        // The same track as a classic script, for a composition that must
        // not fetch at render time: `<script src="take/track.js">`.
        std::fs::write(
            dir.join("track.js"),
            format!("window.MUI_TRACK_DATA = {track_json};\n"),
        )?;
        let frames = track["frames"].as_u64().unwrap_or(0);
        let duration = frames as f64 / f64::from(self.fps);
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
            "encoder": if ffmpeg {
                format!("ffmpeg: take {}; layers {}; bt709 tv-range, converted and tagged",
                    if self.ten_bit { Codec::H265 } else { Codec::H264 }.describe(),
                    layer_codec.describe())
            } else {
                "none: ffmpeg missing or disabled, PNG sequences written instead".into()
            },
            "files": files,
            "layers": layer_files.iter().enumerate()
                .map(|(i, f)| json!({"id": script.layers.get(i), "file": format!("layers/{f}")}))
                .collect::<Vec<_>>(),
            "fps": self.fps,
            "size": [w, h],
            "logical": [self.size.width, self.size.height],
            "scale": self.scale,
            "bpm": self.bpm,
            "beats": self.bpm.map(|b| (duration * b / 60.0 * 1000.0).round() / 1000.0),
            "duration": duration,
            "frames": frames,
            "sample_rate": has_audio.then_some(self.rate),
            "cursor": self.cursor,
            "motion_blur": self.blur,
        });
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_string_pretty(&manifest)?,
        )?;
        Ok(manifest)
    }

    /// The whole fixed-step loop, handing each finished frame to `out`.
    /// Returns `track.json`'s value.
    ///
    /// Time is absolute: subframe `g` is at `g / (fps * blur)`, a cue fires
    /// on the first frame at or after its time, and pointer paths and the
    /// camera are evaluated at the subframe's own time, not by counting
    /// frames -- so a cue between frames moves nothing early and loses
    /// nothing late.
    fn run<S>(
        &self,
        script: &Script<S>,
        state: &mut S,
        mut build: impl FnMut(&mut Ui, &mut S) -> El,
        mut audio: Option<Audio<'_, S>>,
        look: &mut Option<Look<'_>>,
        out: &mut dyn FnMut(Shot) -> Result<(), Error>,
    ) -> Result<Value, Error> {
        if self.fps == 0 || !(self.scale.is_finite() && self.scale > 0.0) {
            return Err("fps and scale must be positive".into());
        }
        let fps = f64::from(self.fps);
        let n = self.blur.max(1) as usize;
        let sub = fps * n as f64;
        let dt = 1.0 / sub;
        let mut cues = script
            .cues
            .iter()
            .map(|(t, a)| Ok((self.secs(*t)?, a)))
            .collect::<Result<Vec<_>, Error>>()?;
        // Stable: same-time cues keep the order they were written in.
        cues.sort_by(|a, b| a.0.total_cmp(&b.0));
        let frames = match script.end {
            Some(t) => self.frame_of(t)?,
            None => cues.last().map_or(0, |c| first(c.0, fps)) + self.fps as usize,
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
        // The time the camera springs currently stand at.
        let mut cam_t = 0.0;
        let mut motion: Option<Motion> = None;
        let mut pos: Option<Point> = None;
        let mut raster = Raster::new(w, h);
        let mut values: BTreeMap<String, f64> = BTreeMap::new();
        let mut surfaces: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        let (mut pointer, mut camera, mut events) = (Vec::new(), Vec::new(), Vec::new());
        let mut next = 0;
        for i in 0..frames {
            let mut input = Input::default();
            // Heard by the audio closure this frame; and (time, event) for the track.
            let mut evs = Vec::new();
            while let Some(&(te, action)) = cues.get(next) {
                if first(te, fps) > i {
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
                        // The path starts where the cue fires, not at its
                        // off-grid time: a press lands on the target and the
                        // whole delta is travelled.
                        let t0 = i as f64 / fps;
                        motion = Some(Motion {
                            from: if *hold { c } else { pos.unwrap_or(c) },
                            to: Point::new(c.x + delta.0, c.y + delta.1),
                            t0,
                            secs,
                            ease: *ease,
                            hold: *hold,
                            // The first subframe at or after the path's end
                            // still holds the button, at the end point.
                            last: first(t0 + secs, sub).max(i * n),
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
                        let c = centre(r);
                        settle(&mut cam, &mut cam_t, te);
                        aim(&mut cam, [c.x, c.y, (lw / fw).min(lh / fh)], *spring);
                    }
                    Action::Reset(spring) => {
                        settle(&mut cam, &mut cam_t, te);
                        aim(&mut cam, home, *spring);
                    }
                    Action::Event(e) => {
                        evs.push(e.clone());
                        events.push(e.json(te));
                    }
                    Action::Call(f) => f(state),
                }
            }
            let mut acc: Vec<Vec<f32>> = Vec::new();
            for k in 0..n {
                let g = i * n + k;
                let t = g as f64 / sub;
                let mut down = false;
                if let Some(m) = &motion {
                    let e = if m.secs > 0.0 {
                        Keys::new(0.0).to(m.secs, 1.0, m.ease).at(t - m.t0)
                    } else {
                        1.0
                    };
                    pos = Some(Point::new(
                        m.from.x + (m.to.x - m.from.x) * e,
                        m.from.y + (m.to.y - m.from.y) * e,
                    ));
                    down = m.hold && g <= m.last;
                }
                let mut input = if k == 0 {
                    std::mem::take(&mut input)
                } else {
                    Input::default()
                };
                input.pointer = PointerInput {
                    pos,
                    buttons: if down {
                        Buttons::PRIMARY
                    } else {
                        Buttons::default()
                    },
                    mods: Mods::default(),
                };
                settle(&mut cam, &mut cam_t, t);
                let [cx, cy, zoom] = cam.map(|s| s.value);
                let view = Affine::translate((f64::from(w) / 2.0, f64::from(h) / 2.0))
                    * Affine::scale(self.scale * zoom)
                    * Affine::translate((-cx, -cy));

                let root = build(&mut ui, state);
                let frame = ui
                    .frame(root, Some(self.size), input, dt)
                    .map_err(|e| format!("frame {i}: {e}"))?;
                for (id, e) in &frame.edits {
                    let e = ReelEvent::Edit {
                        id: id.clone(),
                        begin: *e == Edit::Begin,
                    };
                    events.push(e.json(t));
                    evs.push(e);
                }
                for s in frame.scene.surfaces() {
                    let value = match s.semantics.as_ref().map(|m| &m.role) {
                        Some(Kind::Slider { value, .. }) => *value,
                        Some(Kind::Toggle { on }) => f64::from(u8::from(*on)),
                        _ => continue,
                    };
                    match values.insert(s.key.to_string(), value) {
                        Some(old) if old != value => {
                            let e = ReelEvent::Value {
                                id: s.key.to_string(),
                                value,
                            };
                            events.push(e.json(t));
                            evs.push(e);
                        }
                        _ => {}
                    }
                }
                let cursor = self
                    .cursor
                    .then_some(pos)
                    .flatten()
                    .map(|p| (to_device(view, p), down));
                let mut shots = vec![match look {
                    Some(look) => {
                        let rgba = look(&Take {
                            scene: frame.scene,
                            t,
                            camera: [cx, cy, zoom],
                            pointer: self.cursor.then_some(pos).flatten().map(|p| (p, down)),
                        })?;
                        if rgba.len() != usize::from(w) * usize::from(h) * 4 {
                            return Err(format!(
                                "the look returned {} bytes, not {w}x{h} RGBA",
                                rgba.len()
                            )
                            .into());
                        }
                        rgba
                    }
                    None => raster.draw(frame.scene, view, cursor.map(|c| (c, self.scale)))?,
                }];
                if !script.layers.is_empty() {
                    let ids: Vec<&str> = script.layers.iter().map(String::as_str).collect();
                    for id in &ids {
                        shots.push(raster.draw(&frame.scene.isolate(&[id])?, view, None)?);
                    }
                    shots.push(raster.draw(&frame.scene.without(&ids)?, view, None)?);
                }
                if n == 1 {
                    // No blur: the bytes as drawn, bit-exact.
                    acc = shots
                        .iter()
                        .map(|s| s.iter().map(|&b| f32::from(b)).collect())
                        .collect();
                } else {
                    acc.resize_with(shots.len(), || vec![0.0; shots[0].len()]);
                    for (a, s) in acc.iter_mut().zip(&shots) {
                        accumulate(a, s);
                    }
                }
                if k > 0 {
                    continue;
                }
                // The track samples the frame's own time, subframe 0.
                for s in frame.scene.surfaces() {
                    let named = !s.key.is_empty() && !s.key.starts_with('/');
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
                    col.push(json!([
                        round(r.x0),
                        round(r.y0),
                        round(r.width()),
                        round(r.height())
                    ]));
                }
                pointer.push(pos.map_or(Value::Null, |p| {
                    let d = to_device(view, p);
                    json!([round(d.x), round(d.y), down])
                }));
                camera.push(json!([
                    round(cx * self.scale),
                    round(cy * self.scale),
                    (zoom * 1000.0).round() / 1000.0
                ]));
            }
            for col in surfaces.values_mut() {
                col.resize(i + 1, Value::Null);
            }
            let mut shots = acc.into_iter().map(|a| {
                if n == 1 {
                    a.into_iter().map(|v| v as u8).collect()
                } else {
                    resolve(&a, n)
                }
            });
            let rgba = shots.next().unwrap_or_default();
            // Integer sample boundaries: frame i owns [i*rate/fps, (i+1)*rate/fps),
            // so any fps/rate pair sums exactly with no drift.
            let rate = u64::from(self.rate);
            let len = ((i as u64 + 1) * rate / u64::from(self.fps)
                - i as u64 * rate / u64::from(self.fps)) as usize;
            let mut samples = Vec::new();
            if let Some(a) = audio.as_mut() {
                samples = vec![[0.0f32; 2]; len];
                a(state, &evs, &mut samples);
            }
            out(Shot {
                rgba,
                layers: shots.collect(),
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

/// A pointer path in flight: from `t0` for `secs`, and the last subframe
/// that holds the button.
struct Motion {
    from: Point,
    to: Point,
    t0: f64,
    secs: f64,
    ease: Ease,
    hold: bool,
    last: usize,
}

/// The first tick of a `rate` Hz clock at or after `t` seconds. The epsilon
/// keeps an exact multiple (beat 1 at 120 bpm, 30 fps) on its own tick.
fn first(t: f64, rate: f64) -> usize {
    (t * rate - 1e-9).ceil().max(0.0) as usize
}

/// Advance the camera springs to absolute time `t`. Exact for any step: the
/// springs are closed-form.
fn settle(cam: &mut [Spring; 3], at: &mut f64, t: f64) {
    if t > *at {
        for s in cam.iter_mut() {
            s.step(t - *at);
        }
        *at = t;
    }
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

/// sRGB byte to linear light.
fn linear(b: u8) -> f32 {
    let c = f32::from(b) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Add one straight-alpha sRGB subframe to `acc` as premultiplied linear
/// light, which is what a shutter integrates: a white edge sweeping over
/// black blurs to the grey a camera sees, not a darker sRGB mean.
fn accumulate(acc: &mut [f32], rgba: &[u8]) {
    for (a, p) in acc
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(rgba.as_chunks::<4>().0)
    {
        let alpha = f32::from(p[3]) / 255.0;
        for c in 0..3 {
            a[c] += linear(p[c]) * alpha;
        }
        a[3] += alpha;
    }
}

/// The sum of `n` subframes back to straight-alpha sRGB bytes.
fn resolve(acc: &[f32], n: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(acc.len());
    for a in acc.as_chunks::<4>().0 {
        let w = a[3];
        for &sum in &a[..3] {
            let v = if w > 0.0 { sum / w } else { 0.0 };
            let e = if v <= 0.0031308 {
                v * 12.92
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            };
            out.push((e.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
        out.push((w / n as f32 * 255.0).round().clamp(0.0, 255.0) as u8);
    }
    out
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
            let pts = ARROW;
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

/// What ffmpeg encodes a stream to. Every one is converted *and* tagged
/// BT.709 limited range: tagging alone leaves the matrix to a guess, and
/// players guess differently.
#[derive(Clone, Copy)]
enum Codec {
    /// The delivery take: H.264 High, 8-bit 4:2:0, CRF 14.
    H264,
    /// `.ten_bit(true)`: H.265 Main 10.
    H265,
    /// The master and alpha layers with `.master(true)`: ProRes 4444 + alpha.
    ProRes,
    /// Alpha layers by default: VP9 with a yuva420p alpha plane.
    Vp9,
}
impl Codec {
    fn describe(self) -> &'static str {
        match self {
            Codec::H264 => "H.264 High yuv420p crf 14",
            Codec::H265 => "H.265 Main10 yuv420p10le crf 14",
            Codec::ProRes => "ProRes 4444 yuva444p10le",
            Codec::Vp9 => "VP9 yuva420p crf 24",
        }
    }
    fn ext(self) -> &'static str {
        match self {
            Codec::H264 | Codec::H265 => "mp4",
            Codec::ProRes => "mov",
            Codec::Vp9 => "webm",
        }
    }
    // Flag/value pairs read best as pairs.
    #[rustfmt::skip]
    fn args(self) -> &'static [&'static str] {
        match self {
            Codec::H264 => &[
                "-c:v", "libx264", "-profile:v", "high", "-pix_fmt", "yuv420p", "-crf", "14",
                "-preset", "slow", "-movflags", "+faststart",
            ],
            Codec::H265 => &[
                "-c:v", "libx265", "-pix_fmt", "yuv420p10le", "-crf", "14", "-preset", "slow",
                "-tag:v", "hvc1", "-movflags", "+faststart",
            ],
            Codec::ProRes => &[
                "-c:v", "prores_ks", "-profile:v", "4444", "-pix_fmt", "yuva444p10le",
                "-alpha_bits", "16",
            ],
            Codec::Vp9 => &[
                "-c:v", "libvpx-vp9", "-pix_fmt", "yuva420p", "-b:v", "0", "-crf", "24",
                "-row-mt", "1", "-deadline", "good", "-cpu-used", "4",
            ],
        }
    }
}

/// Where frames go: an ffmpeg child reading raw RGBA on stdin, or a PNG
/// sequence directory.
enum Sink {
    Ffmpeg(Child, String),
    Png(PathBuf, u32, u32, usize, String),
}
impl Sink {
    /// `codec: None` is the PNG fallback: `frames/` for the take, `<name>/`
    /// for a layer.
    fn open(
        codec: Option<Codec>,
        dir: &Path,
        name: &str,
        w: u16,
        h: u16,
        fps: u32,
    ) -> Result<Self, Error> {
        std::fs::create_dir_all(dir)?;
        let Some(codec) = codec else {
            let seq = if name == "take" { "frames" } else { name };
            std::fs::create_dir_all(dir.join(seq))?;
            return Ok(Sink::Png(
                dir.join(seq),
                w.into(),
                h.into(),
                0,
                format!("{seq}/%05d.png"),
            ));
        };
        let file = format!("{name}.{}", codec.ext());
        let fps = fps.to_string();
        let child = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
            ])
            .args(["-color_range", "pc", "-colorspace", "rgb"])
            .args(["-s", &format!("{w}x{h}"), "-r", &fps, "-i", "-"])
            // Convert, then stamp the frames: ffmpeg writes the stream's
            // colour tags from the frames, so output flags alone are lost.
            .args([
                "-vf",
                "scale=out_color_matrix=bt709:out_range=tv,\
                setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv",
            ])
            .args(codec.args())
            .args(["-colorspace", "bt709", "-color_primaries", "bt709"])
            .args(["-color_trc", "bt709", "-color_range", "tv", "-r", &fps])
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
