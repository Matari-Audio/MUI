//! `serve`'s live transport: the scene's sound played as the editor plays
//! it. Every plugin layer's adapter runs on the manual sample clock (the
//! same path as `render`, so it sounds the same), advanced a chunk at a
//! time a little ahead of the output device; audio layers are mixed in.
//! The playhead is the device's: `t` counts the frames it has taken, so
//! the editor that follows `/transport` stays with what is heard.
//!
//! The output is the default cpal device, or a null device (a clock that
//! eats samples in real time) when there is none or `MUI_CUT_AUDIO=null`.
//! While playing, each plugin's UI is captured about 15 times a second
//! into `.cut-cache/live/` and announced over SSE (`event: live`).
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mui_cut::plugin::{CACHE, Step, frame_at, note_events, sample_at};
use mui_cut::{Kind, Project};
use serde_json::{Value, json};

use crate::host::{Session, Sound, Writer};

/// Frames rendered per adapter advance.
const CHUNK: usize = 256;
/// Frames kept queued ahead of the device.
const AHEAD: usize = 768;
/// How long a chunk may take before the adapter counts as stuck.
const PATIENCE: Duration = Duration::from_secs(10);

/// What the editor shows and the transport reports.
#[derive(Clone, Default)]
struct State {
    playing: bool,
    scene: usize,
    /// Scene seconds where play (or the last seek) started.
    t0: f64,
    /// Device frames played when it did.
    played0: u64,
    /// Bumped by every play, pause and seek: the producer restarts.
    generation: u64,
}

/// A note played live: the layer, the key, and on (a velocity) or off.
type LiveNote = (String, u8, Option<u8>);

pub struct Live {
    project: PathBuf,
    rate: u32,
    queue: Mutex<VecDeque<f32>>,
    /// Frames the device has taken from the queue.
    played: AtomicU64,
    /// The device's frames per callback.
    device_frames: AtomicU64,
    device: Mutex<String>,
    state: Mutex<State>,
    notes: Mutex<Vec<LiveNote>>,
    /// A live note on its way: when it came, and the played count at
    /// which its chunk starts sounding.
    heard: Mutex<Option<(Instant, Option<u64>)>>,
    /// The last live note's measured time from `/live/note` to the device
    /// taking its first frame, in microseconds.
    note_us: AtomicU64,
    /// The plugin sessions the capture thread snapshots, by layer.
    sessions: Mutex<Vec<(String, Arc<Mutex<Session>>)>>,
    started: AtomicBool,
    announce: Box<dyn Fn(&str) + Send + Sync>,
}

impl Live {
    /// A transport for `project` at `rate`; nothing runs until it plays.
    /// `announce` sends one SSE message to the editors.
    pub fn new(
        project: &Path,
        rate: u32,
        announce: impl Fn(&str) + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            project: project.to_owned(),
            rate,
            queue: Mutex::new(VecDeque::new()),
            played: AtomicU64::new(0),
            device_frames: AtomicU64::new(0),
            device: Mutex::new(String::new()),
            state: Mutex::new(State::default()),
            notes: Mutex::new(Vec::new()),
            heard: Mutex::new(None),
            note_us: AtomicU64::new(0),
            sessions: Mutex::new(Vec::new()),
            started: AtomicBool::new(false),
            announce: Box::new(announce),
        })
    }

    /// Scene seconds being heard now.
    fn now(&self, s: &State, duration: f64) -> f64 {
        if !s.playing {
            return s.t0;
        }
        let heard = (self.played.load(Ordering::Acquire) - s.played0) as f64 / f64::from(self.rate);
        (s.t0 + heard) % duration.max(1e-9)
    }

    /// Play, pause or seek: `{"playing": bool, "t": seconds, "scene": index}`.
    pub fn control(self: &Arc<Self>, v: &Value) -> Value {
        let duration = self.duration();
        {
            let mut s = self.state.lock().expect("no panic holds it");
            let t = self.now(&s, duration);
            s.t0 = v["t"]
                .as_f64()
                .filter(|t| t.is_finite())
                .unwrap_or(t)
                .clamp(0., duration);
            if let Some(i) = v["scene"].as_u64() {
                s.scene = i as usize;
            }
            if let Some(p) = v["playing"].as_bool() {
                s.playing = p;
            }
            s.played0 = self.played.load(Ordering::Acquire);
            s.generation += 1;
            self.queue.lock().expect("no panic holds it").clear();
        }
        if !self.started.swap(true, Ordering::AcqRel) {
            self.start();
        }
        self.report()
    }

    /// `{"playing", "t", "scene", "latency_ms", "device", "rate"}`.
    pub fn report(&self) -> Value {
        let s = self.state.lock().expect("no panic holds it").clone();
        let t = self.now(&s, self.duration());
        json!({
            "playing": s.playing, "t": t, "scene": s.scene,
            "latency_ms": self.latency_ms(),
            "device": *self.device.lock().expect("no panic holds it"),
            "rate": self.rate,
            "played": self.played.load(Ordering::Acquire),
            "note_ms": self.note_us.load(Ordering::Acquire) as f64 / 1000.,
        })
    }

    /// From a note sent to it being heard, worst case: the queue ahead of
    /// the device and the device's own buffer.
    pub fn latency_ms(&self) -> f64 {
        let frames = (AHEAD + CHUNK) as u64 + self.device_frames.load(Ordering::Acquire);
        frames as f64 * 1000. / f64::from(self.rate)
    }

    /// A key played on the editor's keyboard (or MIDI) into `layer`.
    pub fn note(&self, v: &Value) -> Result<(), String> {
        let layer = v["layer"].as_str().ok_or("a note needs its `layer`")?;
        let key = v["note"]
            .as_u64()
            .filter(|n| *n <= 127)
            .ok_or("`note` is 0..127")? as u8;
        let on = v["on"].as_bool().ok_or("`on` is true or false")?;
        let vel = v["velocity"].as_u64().unwrap_or(100).clamp(1, 127) as u8;
        self.notes.lock().expect("no panic holds it").push((
            layer.to_owned(),
            key,
            on.then_some(vel),
        ));
        *self.heard.lock().expect("no panic holds it") = Some((Instant::now(), None));
        Ok(())
    }

    fn duration(&self) -> f64 {
        let i = self.state.lock().expect("no panic holds it").scene;
        crate::load(&self.project)
            .ok()
            .and_then(|p| p.scenes.get(i).map(|s| s.duration))
            .unwrap_or(1.)
    }

    fn start(self: &Arc<Self>) {
        let out = self.clone();
        std::thread::spawn(move || out.output());
        let me = self.clone();
        std::thread::spawn(move || {
            if let Err(e) = me.produce() {
                eprintln!("mui-cut: live sound stopped: {e}");
            }
        });
        let me = self.clone();
        std::thread::spawn(move || me.capture());
    }

    /// Take up to `out.len() / 2` frames off the queue, silence after.
    fn pull(&self, out: &mut [f32]) {
        let mut q = self.queue.lock().expect("no panic holds it");
        let n = q.len().min(out.len()) & !1;
        for (o, v) in out.iter_mut().zip(q.drain(..n)) {
            *o = v;
        }
        out[n..].fill(0.);
        let played = self.played.fetch_add((n / 2) as u64, Ordering::AcqRel) + (n / 2) as u64;
        drop(q);
        if let Ok(mut h) = self.heard.try_lock()
            && let Some((at, Some(frame))) = *h
            && played > frame
        {
            // The device has the note's first frame: in its buffer now,
            // out of the speaker a buffer later.
            let buffer = self.device_frames.load(Ordering::Acquire) as f64 / f64::from(self.rate);
            let us = (at.elapsed().as_secs_f64() + buffer) * 1e6;
            self.note_us.store(us as u64, Ordering::Release);
            *h = None;
        }
    }

    /// The device thread: a cpal stream at the project's rate, else a
    /// null device that takes frames in real time.
    fn output(self: Arc<Self>) {
        const TICK: usize = 240;
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let real = std::env::var("MUI_CUT_AUDIO").as_deref() != Ok("null");
        let stream = real
            .then(|| {
                let device = cpal::default_host().default_output_device()?;
                let config = cpal::StreamConfig {
                    channels: 2,
                    sample_rate: self.rate,
                    buffer_size: cpal::BufferSize::Fixed(256),
                };
                let me = self.clone();
                let stream = device
                    .build_output_stream(
                        &config,
                        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                            me.device_frames
                                .fetch_max((data.len() / 2) as u64, Ordering::AcqRel);
                            me.pull(data);
                        },
                        |e| eprintln!("mui-cut: audio device: {e}"),
                        None,
                    )
                    .map_err(|e| eprintln!("mui-cut: audio device: {e}; playing to a null device"))
                    .ok()?;
                stream.play().ok()?;
                let name = device
                    .description()
                    .map_or_else(|_| "audio device".into(), |d| d.name().to_owned());
                Some((stream, name))
            })
            .flatten();
        if let Some((_stream, name)) = stream {
            *self.device.lock().expect("no panic holds it") = name;
            // The stream plays while this thread keeps it.
            loop {
                std::thread::park();
            }
        }
        *self.device.lock().expect("no panic holds it") = "null".into();
        self.device_frames.store(TICK as u64, Ordering::Release);
        let mut buf = vec![0f32; 2 * TICK];
        let start = Instant::now();
        let mut done = 0u64;
        loop {
            let due = (start.elapsed().as_secs_f64() * f64::from(self.rate)) as u64;
            while done + TICK as u64 <= due {
                self.pull(&mut buf);
                done += TICK as u64;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// The producer: keeps [`AHEAD`] frames of the scene queued.
    fn produce(&self) -> Result<(), String> {
        let mut generation = u64::MAX;
        let mut text = String::new();
        let mut read = Instant::now() - Duration::from_secs(1);
        let mut mix = Mix::default();
        loop {
            let s = self.state.lock().expect("no panic holds it").clone();
            if !s.playing {
                if generation != s.generation {
                    mix.panic();
                    generation = s.generation;
                }
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            // Outside edits (and the editor's saves) while playing.
            if read.elapsed() > Duration::from_millis(250) {
                read = Instant::now();
                let now = std::fs::read_to_string(&self.project).map_err(|e| e.to_string())?;
                if now != text {
                    text = now;
                    let p = Project::load(&text)?;
                    mix.load(p, &self.project, s.scene)?;
                    *self.sessions.lock().expect("no panic holds it") = mix.sessions();
                    generation = u64::MAX;
                }
            }
            if generation != s.generation {
                generation = s.generation;
                mix.seek(sample_at(s.t0, self.rate))?;
            }
            if self.queue.lock().expect("no panic holds it").len() / 2 >= AHEAD {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            let live = std::mem::take(&mut *self.notes.lock().expect("no panic holds it"));
            let chunk = mix.chunk(self.rate, &live)?;
            // A seek that came while rendering makes this chunk stale.
            if self.state.lock().expect("no panic holds it").generation == generation {
                let mut q = self.queue.lock().expect("no panic holds it");
                let start = self.played.load(Ordering::Acquire) + (q.len() / 2) as u64;
                q.extend(chunk);
                if !live.is_empty()
                    && let Some((_, frame @ None)) =
                        &mut *self.heard.lock().expect("no panic holds it")
                {
                    *frame = Some(start);
                }
            }
        }
    }

    /// The capture thread: while playing, each plugin's UI as it is now.
    fn capture(&self) {
        let cache = self.project.parent().unwrap_or(Path::new(".")).join(CACHE);
        let _ = std::fs::create_dir_all(cache.join("live"));
        let _ = std::fs::create_dir_all(cache.join("img"));
        let mut n = 0u64;
        let mut last: HashMap<String, VecDeque<String>> = HashMap::new();
        let mut was = false;
        loop {
            let started = Instant::now();
            let playing = self.state.lock().expect("no panic holds it").playing;
            if playing {
                let sessions = self.sessions.lock().expect("no panic holds it").clone();
                for (layer, session) in sessions {
                    n += 1;
                    let key = format!("live/{}-{n}", layer.replace('/', "_"));
                    let mut errs = Vec::new();
                    let saved = {
                        let mut s = session.lock().expect("no panic holds it");
                        s.snapshot(&mut errs).and_then(|packet| {
                            crate::host::save(&cache, &key, "live", &packet, &s.textures)
                        })
                    };
                    match saved {
                        Ok(()) => {
                            (self.announce)(&json!({"layer": layer, "state": key}).to_string());
                            // A few stay for an editor still fetching them.
                            let kept = last.entry(layer).or_default();
                            kept.push_back(key);
                            if kept.len() > 4
                                && let Some(old) = kept.pop_front()
                            {
                                let _ = std::fs::remove_file(cache.join(format!("{old}.json")));
                            }
                        }
                        Err(e) => eprintln!("mui-cut: live capture of `{layer}`: {e}"),
                    }
                }
            } else if was {
                for layer in last.keys() {
                    (self.announce)(&json!({"layer": layer, "state": null}).to_string());
                }
            }
            was = playing;
            std::thread::sleep(Duration::from_millis(66).saturating_sub(started.elapsed()));
        }
    }
}

/// A plugin layer playing: its adapter, the clock offset between the
/// scene and the adapter's clock, and its scene notes and steps.
struct Voice {
    layer: String,
    source: String,
    session: Arc<Mutex<Session>>,
    writer: Writer,
    sound: Receiver<Sound>,
    /// The adapter's clock.
    clock: u64,
    /// The adapter's clock at scene sample 0.
    offset: u64,
    events: Vec<(u64, Value)>,
    steps: Vec<Step>,
    /// The last grid frame whose commands were sent.
    frame: usize,
    gain: mui_cut::Layer,
}

#[derive(Default)]
struct Mix {
    project: Option<Project>,
    scene: usize,
    voices: Vec<Voice>,
    files: crate::audio::Files,
    /// The project file (audio paths are relative to it).
    path: PathBuf,
    /// The scene sample the next chunk starts at.
    at: u64,
}

impl Mix {
    fn sessions(&self) -> Vec<(String, Arc<Mutex<Session>>)> {
        self.voices
            .iter()
            .map(|v| (v.layer.clone(), v.session.clone()))
            .collect()
    }

    /// The project as it is now: adapters kept for layers whose source
    /// did not change, started for new ones.
    fn load(&mut self, p: Project, path: &Path, scene: usize) -> Result<(), String> {
        let s = p.scenes.get(scene).ok_or("no such scene")?;
        let dir = path.parent().unwrap_or(Path::new("."));
        let last = frame_at(s.duration, p.fps);
        let mut old: Vec<Voice> = std::mem::take(&mut self.voices);
        for l in &s.layers {
            let Kind::Plugin { source, notes, .. } = &l.kind else {
                continue;
            };
            let key = serde_json::to_string(source).unwrap_or_default();
            let v = if let Some(i) = old.iter().position(|v| v.layer == l.id && v.source == key) {
                old.swap_remove(i)
            } else {
                let (exe, _) = crate::host::executable(source, dir)?;
                let mut session = Session::open(&exe, &source.args, dir, p.sample_rate)?;
                let sound = session.sound.take().ok_or("no sound")?;
                Voice {
                    layer: l.id.clone(),
                    source: key,
                    writer: session.writer(),
                    session: Arc::new(Mutex::new(session)),
                    sound,
                    clock: 0,
                    offset: 0,
                    events: Vec::new(),
                    steps: Vec::new(),
                    frame: usize::MAX,
                    gain: l.clone(),
                }
            };
            self.voices.push(Voice {
                events: note_events(notes, p.sample_rate),
                steps: l.plugin_track(p.fps, p.sample_rate, last),
                gain: l.clone(),
                ..v
            });
        }
        self.project = Some(p);
        self.scene = scene;
        self.path = path.to_owned();
        Ok(())
    }

    fn panic(&mut self) {
        for v in &self.voices {
            let _ = crate::host::write(&v.writer, &json!({"op": "panic"}));
        }
    }

    /// Play from scene sample `at`: held notes let go, and every
    /// parameter, pointer and view the grid has sent by then sent again.
    fn seek(&mut self, at: u64) -> Result<(), String> {
        self.panic();
        self.at = at;
        let Some(p) = &self.project else {
            return Ok(());
        };
        let f = frame_at(at as f64 / f64::from(p.sample_rate), p.fps);
        for v in &mut self.voices {
            v.offset = v.clock.saturating_sub(at);
            v.frame = f;
            for step in v.steps.iter().filter(|s| s.frame <= f) {
                for c in step.commands.iter().filter(|c| c["op"] != "advance") {
                    crate::host::write(&v.writer, c)?;
                }
            }
        }
        Ok(())
    }

    /// The next chunk of the scene's sound, stereo, `live` notes played
    /// at its start. Wraps at the scene's end.
    fn chunk(&mut self, rate: u32, live: &[LiveNote]) -> Result<Vec<f32>, String> {
        let Some(p) = &self.project else {
            return Ok(vec![0.; 2 * CHUNK]);
        };
        let Some(s) = p.scenes.get(self.scene) else {
            return Ok(vec![0.; 2 * CHUNK]);
        };
        let end = p.samples(s);
        let n = CHUNK.min((end - self.at.min(end)) as usize).max(1);
        let (from, to) = (self.at, self.at + n as u64);
        let mut out = vec![0f32; 2 * n];
        for v in &mut self.voices {
            let f = frame_at(from as f64 / f64::from(rate), p.fps);
            if f != v.frame {
                for step in v
                    .steps
                    .iter()
                    .filter(|s| s.frame > v.frame.min(f) && s.frame <= f)
                {
                    for c in step.commands.iter().filter(|c| c["op"] != "advance") {
                        crate::host::write(&v.writer, c)?;
                    }
                }
                v.frame = f;
            }
            let mut notes: Vec<Value> = v
                .events
                .iter()
                .filter(|e| (from..to).contains(&e.0))
                .map(|(at, c)| {
                    let mut c = c.clone();
                    c["at"] = json!(v.offset + at);
                    c
                })
                .collect();
            for (_, key, on) in live.iter().filter(|n| n.0 == v.layer) {
                notes.push(match on {
                    Some(vel) => json!({"at": v.offset + from, "op": "note_on", "note": key, "velocity": vel}),
                    None => json!({"at": v.offset + from, "op": "note_off", "note": key}),
                });
            }
            let target = v.offset + to;
            crate::host::write(
                &v.writer,
                &json!({"op": "advance", "to": target, "notes": notes}),
            )?;
            let mut pcm = Vec::with_capacity(2 * n);
            loop {
                match v.sound.recv_timeout(PATIENCE) {
                    Ok(Sound::Samples(x)) => pcm.extend(x),
                    Ok(Sound::Advanced) => break,
                    Err(RecvTimeoutError::Timeout) => {
                        return Err(format!("`{}` stopped playing", v.layer));
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        return Err(format!("`{}` exited", v.layer));
                    }
                }
            }
            v.clock = target;
            crate::audio::add(&mut out, from as usize, rate, &v.gain, |i, _| {
                let i = i - from as usize;
                [
                    pcm.get(2 * i).copied().unwrap_or(0.),
                    pcm.get(2 * i + 1).copied().unwrap_or(0.),
                ]
            });
        }
        for l in &s.layers {
            if let Kind::Audio { path } = &l.kind {
                let pcm = self.files.get(&self.path, path, rate).ok();
                if let Some(pcm) = pcm {
                    crate::audio::add(&mut out, from as usize, rate, l, |i, t| {
                        crate::audio::file_at(&pcm, l, rate, i, t)
                    });
                }
            }
        }
        for x in &mut out {
            *x = x.clamp(-1., 1.);
        }
        self.at = to;
        if self.at >= end {
            // Around again: from the top, as the editor loops.
            self.seek(0)?;
        }
        Ok(out)
    }
}
