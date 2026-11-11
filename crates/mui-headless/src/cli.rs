//! The command line: flags, and the one run they describe.
//!
//! Flag names follow Rune's agent tooling (`--snap`, `--scene`, `--at`,
//! `--size`, `--scale`, `--theme`, `--list-scenes`, `--script`, `--frames`,
//! `--start`, `--every`, `--out`, `--cols`, `--strip`, `--tree`, `--trace`) so
//! an agent that knows one knows the other. Hand-rolled: no CLI dependency.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use mui::prelude::*;
use serde_json::{Value, json};

use crate::render::{Raster, contact_sheet, write_png};
use crate::script::Script;
use crate::session::{Session, Step};
use crate::trace::Tracer;
use crate::tree::{r4, tree};
use crate::{Error, Scene};

pub const USAGE: &str = "\
headless agent tooling: no window, no GPU, a fixed virtual clock

  --snap out.png          render one frame (the CPU renderer)
  --tree out.json         the resolved element tree as JSON
  --script file.json      play timestamped events through Ui::frame first
  --trace out.jsonl       one JSON line per frame: what changed, springs in flight
  --frames N|DIR          write numbered PNGs: N frames (see --start, --every, --out),
                          or DIR to write them there for the whole run
  --strip out.png         the sampled frames as one contact sheet (--cols N, default 4)
  --list-scenes           print the scene names and exit
  --scene NAME            which scene (default: the first)
  --at T                  the time to snap or dump: seconds or mark:NAME[+-secs]
                          (default: the end of the script, else 0)
  --start T --every K     first frame of --frames/--strip, and sampling stride
  --out DIR               where --frames writes (default: frames)
  --size WxH --scale S    logical size and device scale (default: the scene's, 1)
  --theme dark|light      the palette's mode
  --fps N                 the virtual clock's frame rate (default 60)
  --cost                  add wall-clock cost_us to trace records (the one
                          non-deterministic field; off by default)

A JSON report (edits, value changes, marks, files written) goes to stdout.
";

#[derive(Default)]
struct Args {
    snap: Option<PathBuf>,
    tree: Option<PathBuf>,
    script: Option<PathBuf>,
    trace: Option<PathBuf>,
    strip: Option<PathBuf>,
    frames: Option<u64>,
    out: Option<PathBuf>,
    scene: Option<String>,
    at: Option<String>,
    start: Option<String>,
    every: Option<u64>,
    cols: Option<usize>,
    size: Option<(u32, u32)>,
    scale: Option<f64>,
    theme: Option<Mode>,
    fps: Option<u32>,
    cost: bool,
    list: bool,
}

fn parse<T: std::str::FromStr>(flag: &str, v: &str) -> Result<T, Error> {
    v.parse()
        .map_err(|_| Error::new(format!("{flag}: cannot read '{v}'")))
}

impl Args {
    fn parse(args: &[String]) -> Result<Self, Error> {
        let mut a = Self::default();
        let mut it = args.iter();
        while let Some(flag) = it.next() {
            if flag == "--list-scenes" {
                a.list = true;
                continue;
            }
            if flag == "--cost" {
                a.cost = true;
                continue;
            }
            let mut value = || {
                it.next()
                    .map(String::as_str)
                    .ok_or_else(|| Error::new(format!("{flag} needs a value")))
            };
            match flag.as_str() {
                "--snap" => a.snap = Some(value()?.into()),
                "--tree" => a.tree = Some(value()?.into()),
                "--script" => a.script = Some(value()?.into()),
                "--trace" => a.trace = Some(value()?.into()),
                "--strip" => a.strip = Some(value()?.into()),
                "--out" => a.out = Some(value()?.into()),
                "--scene" => a.scene = Some(value()?.into()),
                "--at" => a.at = Some(value()?.into()),
                "--start" => a.start = Some(value()?.into()),
                "--every" => a.every = Some(parse(flag, value()?)?),
                "--cols" => a.cols = Some(parse(flag, value()?)?),
                "--scale" => a.scale = Some(parse(flag, value()?)?),
                "--fps" => a.fps = Some(parse(flag, value()?)?),
                // A number is Rune's frame count; anything else is a directory.
                "--frames" => {
                    let v = value()?;
                    match v.parse() {
                        Ok(n) => a.frames = Some(n),
                        Err(_) => {
                            a.out = Some(v.into());
                            a.frames = Some(0);
                        }
                    }
                }
                "--size" => {
                    let v = value()?;
                    let (w, h) = v
                        .split_once(['x', 'X'])
                        .ok_or_else(|| Error::new("--size is WxH, like 620x520"))?;
                    a.size = Some((parse("--size", w)?, parse("--size", h)?));
                }
                "--theme" => {
                    a.theme = Some(match value()? {
                        "dark" => Mode::Dark,
                        "light" => Mode::Light,
                        o => return Err(Error::new(format!("--theme is dark or light, not '{o}'"))),
                    });
                }
                other => {
                    return Err(Error::new(format!(
                        "unknown flag '{other}' (run with --help)"
                    )));
                }
            }
        }
        Ok(a)
    }
}

/// The flags this harness owns: a host with its own CLI (the gallery) hands
/// over when it sees one.
pub fn wants(args: &[String]) -> bool {
    const OWN: [&str; 8] = [
        "--snap",
        "--tree",
        "--script",
        "--trace",
        "--strip",
        "--frames",
        "--list-scenes",
        "--help",
    ];
    args.iter().any(|a| OWN.contains(&a.as_str()))
}

/// `Widgets Gallery` -> `widgets-gallery`: how a scene is named on the command line.
pub fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    s.trim_end_matches('-').to_owned()
}

pub fn run(mut scenes: Vec<Scene>, args: &[String]) -> Result<Value, Error> {
    let a = Args::parse(args)?;
    if a.list {
        return Ok(json!({"scenes": scenes.iter().map(|s| slug(&s.name)).collect::<Vec<_>>()}));
    }
    if scenes.is_empty() {
        return Err(Error::new("no scenes"));
    }
    let at_scene = match &a.scene {
        None => 0,
        Some(n) => scenes
            .iter()
            .position(|s| slug(&s.name) == slug(n))
            .ok_or_else(|| {
                Error::new(format!(
                    "no scene '{n}' (scenes: {})",
                    scenes.iter().map(|s| slug(&s.name)).collect::<Vec<_>>().join(", ")
                ))
            })?,
    };
    let fps = a.fps.unwrap_or(60);
    let script = match &a.script {
        Some(p) => Script::parse(&std::fs::read_to_string(p)?, fps)?,
        None => Script::parse("[]", fps)?,
    };
    let mut s = Session::new(
        scenes.swap_remove(at_scene),
        a.size,
        a.scale.unwrap_or(1.0),
        a.theme,
        fps,
    )?;
    s.push(script.events.iter().cloned());

    let to_frame = |t: f64| (t.max(0.0) * f64::from(fps)).round() as u64;
    let end = match &a.at {
        Some(t) => to_frame(script.time(t)?),
        None if a.script.is_some() => to_frame(script.end),
        None => 0,
    };
    let first = match &a.start {
        Some(t) => to_frame(script.time(t)?),
        None => 0,
    };
    let every = a.every.unwrap_or(1).max(1);
    // --frames N is N frames of run from --start; 0 or absent is through the end.
    let span = match a.frames {
        Some(n) if n > 0 => n,
        _ => end.saturating_sub(first) + 1,
    };
    let wants_frames = a.frames.is_some() || a.strip.is_some();
    let sampled = |f: u64| wants_frames && f >= first && f < first + span && (f - first) % every == 0;
    let last = if wants_frames { end.max(first + span - 1) } else { end };

    let mut raster = None;
    let (w, h) = s.pixels();
    let mut draw = |s: &Session| -> Result<Vec<u8>, Error> {
        raster
            .get_or_insert_with(|| Raster::new(w, h))
            .draw(s.scene()?, s.scale())
    };
    let out_dir = a.out.clone().unwrap_or_else(|| "frames".into());
    let mut wrote: Vec<String> = Vec::new();
    let mut sheet: Vec<Vec<u8>> = Vec::new();
    let mut tree_json = None;
    let mut trace = Vec::new();
    let mut tracer = Tracer::default();
    let mut edits = Vec::new();
    let mut values = Vec::new();
    let mut seen: BTreeMap<String, f64> = BTreeMap::new();

    // Everything that wants this frame: the snap, the tree, a sampled frame.
    let mut capture = |s: &Session| -> Result<(), Error> {
        let f = s.frame();
        if f == end {
            if let Some(p) = &a.snap {
                write_png(p, w.into(), h.into(), &draw(s)?)?;
                wrote.push(p.display().to_string());
            }
            if a.tree.is_some() {
                tree_json = Some(tree(s)?);
            }
        }
        if sampled(f) {
            let rgba = draw(s)?;
            if a.frames.is_some() {
                let p = out_dir.join(format!("frame_{f:04}.png"));
                write_png(&p, w.into(), h.into(), &rgba)?;
                wrote.push(p.display().to_string());
            }
            if a.strip.is_some() {
                sheet.push(rgba);
            }
        }
        Ok(())
    };
    capture(&s)?;
    while s.frame() < last {
        let t0 = a.cost.then(Instant::now);
        let step: Step = s.step()?;
        let cost = t0.map(|t| t.elapsed().as_micros());
        for (id, e) in &step.edits {
            edits.push(json!({
                "t": r4(s.time()),
                "frame": step.frame,
                "id": id,
                "edit": if *e == Edit::Begin { "begin" } else { "end" },
            }));
        }
        for sf in s.scene()?.surfaces() {
            let v = match sf.semantics.as_ref().map(|m| &m.role) {
                Some(A11y::Slider { value, .. }) => *value,
                Some(A11y::Toggle { on }) => f64::from(u8::from(*on)),
                _ => continue,
            };
            if let Some(old) = seen.insert(sf.key.to_string(), v)
                && old != v
            {
                values.push(json!({
                    "t": r4(s.time()),
                    "frame": step.frame,
                    "id": sf.key.to_string(),
                    "value": r4(v),
                }));
            }
        }
        if a.trace.is_some() {
            trace.push(tracer.record(&s, &step, cost)?);
        }
        capture(&s)?;
    }

    if let Some(p) = &a.tree {
        let v = tree_json.unwrap_or(Value::Null);
        std::fs::write(p, crate::tree::pretty(&v))?;
        wrote.push(p.display().to_string());
    }
    if let Some(p) = &a.trace {
        let mut text = String::new();
        for r in &trace {
            text += &serde_json::to_string(r)?;
            text.push('\n');
        }
        std::fs::write(p, text)?;
        wrote.push(p.display().to_string());
    }
    if let Some(p) = &a.strip {
        let (px, sw, sh) = contact_sheet(&sheet, w.into(), h.into(), a.cols.unwrap_or(4));
        write_png(p, sw, sh, &px)?;
        wrote.push(p.display().to_string());
    }
    Ok(json!({
        "scene": slug(&s.name),
        "size": [s.size().width, s.size().height],
        "scale": s.scale(),
        "fps": fps,
        "frame": s.frame(),
        "t": r4(s.time()),
        "marks": script.marks,
        "edits": edits,
        "values": values,
        "animating": s.animating(),
        "settled_after_frame": a.trace.is_some().then_some(tracer.last_moving.unwrap_or(0)),
        "wrote": wrote,
    }))
}
