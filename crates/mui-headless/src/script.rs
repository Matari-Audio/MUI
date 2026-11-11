//! `--script`: a JSON list of timestamped events, compiled to per-frame input.
//!
//! The shape follows Rune's script files (`crates/rune/src/tooling/script.rs`,
//! MIT, anomalyco/rune @ 469f7b0): `[{"t": 0.5, "type": "click", "key":
//! "bypass"}, ...]`, or `{"events": [...]}`. Times are seconds on the virtual
//! clock. A target is `"x"`/`"y"` in logical units or `"key"`, the id of an
//! element, resolved to its centre the frame the event fires.
//!
//! | `type`                  | fields                                                    |
//! |-------------------------|-----------------------------------------------------------|
//! | `move`                  | target                                                    |
//! | `down`, `up`            | target (optional), `button`: primary (default), secondary, middle |
//! | `click` (`tap`)         | target, `hold` seconds (0.08), `button`                   |
//! | `double-click`          | target, `gap` (0.15), `hold` (0.06)                       |
//! | `drag`                  | target, `dx`, `dy`, `dur` seconds (0.3); eased            |
//! | `wheel`                 | target (optional), `dx`, `dy` (positive is down/right)    |
//! | `key`                   | `name` (a character, `Enter`, `ArrowLeft`, `F5`, ...)     |
//! | `text` (`type`)         | `text`                                                    |
//! | `mark`                  | `name`: names a time, does nothing else                   |
//! | `wait`                  | `dur`: keeps the run going until `t + dur`                |
//!
//! Any pointer or key event also takes `"shift"`, `"ctrl"`, `"alt"`, `"cmd"`
//! booleans. A bare `move` after `down` is a drag by hand.

use std::collections::BTreeMap;

use mui::prelude::*;
use serde_json::Value;

use crate::Error;
use crate::session::{Act, Ev, Pos, due};

/// A compiled script.
pub struct Script {
    pub(crate) events: Vec<Ev>,
    /// Named times, seconds.
    pub marks: BTreeMap<String, f64>,
    /// When the run ends by default: the `end` mark, else a second after the
    /// last thing the script does.
    pub end: f64,
}

fn num(e: &Value, k: &str, d: f64) -> Result<f64, Error> {
    match e.get(k) {
        None | Some(Value::Null) => Ok(d),
        Some(v) => v
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or_else(|| Error::new(format!("'{k}' must be a number, got {v}"))),
    }
}

fn target(e: &Value) -> Result<Option<Pos>, Error> {
    if let Some(k) = e.get("key").and_then(Value::as_str)
        && !is_key_event(e)
    {
        return Ok(Some(Pos::Key(k.to_owned())));
    }
    match (e.get("x"), e.get("y")) {
        (Some(_), Some(_)) => Ok(Some(Pos::Xy(num(e, "x", 0.0)?, num(e, "y", 0.0)?))),
        (None, None) => Ok(None),
        _ => Err(Error::new("a target needs both 'x' and 'y'")),
    }
}

/// `{"type": "key", "key": ...}` is not a target: the key event names its key
/// with `name`, but be forgiving about `key` there.
fn is_key_event(e: &Value) -> bool {
    e.get("type").and_then(Value::as_str) == Some("key")
}

fn mods(e: &Value) -> Mods {
    let b = |k| e.get(k).and_then(Value::as_bool).unwrap_or(false);
    Mods {
        shift: b("shift"),
        ctrl: b("ctrl"),
        alt: b("alt"),
        cmd: b("cmd"),
    }
}

fn button(e: &Value) -> Result<Button, Error> {
    Ok(match e.get("button").and_then(Value::as_str) {
        None | Some("primary") => Button::Primary,
        Some("secondary") => Button::Secondary,
        Some("middle") => Button::Middle,
        Some(o) => {
            return Err(Error::new(format!(
                "unknown button '{o}' (primary, secondary, middle)"
            )));
        }
    })
}

fn key(name: &str) -> Result<Key, Error> {
    let mut chars = name.chars();
    match (chars.next(), chars.next()) {
        (Some(' '), None) => Ok(Key::Space),
        (Some(c), None) => Ok(Key::Char(c)),
        _ => Key::from_name(name).ok_or_else(|| {
            Error::new(format!(
                "unknown key '{name}' (a character, Enter, Escape, Tab, Backspace, Delete, \
                 ArrowLeft/Right/Up/Down, Home, End, PageUp, PageDown, Space, F1..F24)"
            ))
        }),
    }
}

/// Seconds as whole frames, at least `min`.
fn frames(secs: f64, fps: u32, min: u64) -> u64 {
    ((secs * f64::from(fps)).round().max(0.0) as u64).max(min)
}

/// One event as per-frame primitives, and the time it is done.
fn compile(e: &Value, ty: &str, t: f64, fps: u32) -> Result<(Vec<Ev>, f64), Error> {
    let f0 = due(t, fps);
    let frame_secs = 1.0 / f64::from(fps);
    let m = mods(e);
    let to = target(e)?;
    let mut out: Vec<Ev> = Vec::new();
    let mut ev = |frame: u64, act: Act| out.push(Ev { frame, act });
    let mut done = t;
    match ty {
        "move" => {
            let p = to.ok_or_else(|| Error::new("'move' needs a target"))?;
            ev(f0, Act::Move(p));
        }
        "down" | "up" => {
            let b = button(e)?;
            if let Some(p) = to {
                ev(f0, Act::Move(p));
            }
            ev(f0, Act::Mods(m));
            ev(f0, if ty == "down" { Act::Down(b) } else { Act::Up(b) });
        }
        "click" | "tap" | "double-click" => {
            let b = button(e)?;
            let dbl = ty == "double-click";
            let hold = frames(num(e, "hold", if dbl { 0.06 } else { 0.08 })?, fps, 1);
            let gap = frames(num(e, "gap", 0.15)?, fps, hold + 1);
            let presses: u64 = if dbl { 2 } else { 1 };
            for n in 0..presses {
                let at = f0 + gap * n;
                if let Some(p) = &to {
                    ev(at, Act::Move(p.clone()));
                }
                ev(at, Act::Mods(m));
                ev(at, Act::Down(b));
                ev(at + hold, Act::Up(b));
                ev(at + hold, Act::Mods(Mods::default()));
            }
            done = t + (gap * (presses - 1) + hold) as f64 * frame_secs;
        }
        "drag" => {
            let b = button(e)?;
            let (dx, dy) = (num(e, "dx", 0.0)?, num(e, "dy", 0.0)?);
            let steps = frames(num(e, "dur", 0.3)?, fps, 1);
            if let Some(p) = to {
                ev(f0, Act::Move(p));
            }
            ev(f0, Act::Mods(m));
            ev(f0, Act::Down(b));
            for k in 1..=steps {
                let x = k as f64 / steps as f64;
                // Smoothstep: the hand starts and stops gently.
                let f = x * x * (3.0 - 2.0 * x);
                ev(f0 + k, Act::Move(Pos::Origin(dx * f, dy * f)));
            }
            // One frame after the last move, so the release sees the pointer
            // where the drag ended.
            ev(f0 + steps + 1, Act::Up(b));
            ev(f0 + steps + 1, Act::Mods(Mods::default()));
            done = t + (steps + 1) as f64 * frame_secs;
        }
        "wheel" => {
            if let Some(p) = to {
                ev(f0, Act::Move(p));
            }
            ev(f0, Act::Wheel(num(e, "dx", 0.0)?, num(e, "dy", 0.0)?));
        }
        "key" => {
            let name = e
                .get("name")
                .or_else(|| e.get("key"))
                .and_then(Value::as_str)
                .ok_or_else(|| Error::new("'key' needs a 'name'"))?;
            let k = key(name)?;
            ev(f0, Act::Key(KeyPress { key: k, mods: m }));
            // A host sends printable keys as text too.
            if !(m.ctrl || m.cmd) {
                match k {
                    Key::Char(c) => ev(f0, Act::Text(c.to_string())),
                    Key::Space => ev(f0, Act::Text(" ".into())),
                    _ => {}
                }
            }
        }
        "text" | "type" => {
            let text = e
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::new("'text' needs a 'text' string"))?;
            ev(f0, Act::Text(text.to_owned()));
        }
        other => {
            return Err(Error::new(format!(
                "unknown event type '{other}' (move, down, up, click, double-click, drag, wheel, \
                 key, text, mark, wait)"
            )));
        }
    }
    Ok((out, done))
}

impl Script {
    pub fn parse(src: &str, fps: u32) -> Result<Self, Error> {
        let v: Value = serde_json::from_str(src)?;
        let list = match &v {
            Value::Array(a) => a,
            Value::Object(o) => o
                .get("events")
                .and_then(Value::as_array)
                .ok_or_else(|| Error::new("a script is an array of events or {\"events\": [...]}"))?,
            _ => return Err(Error::new("a script is an array of events")),
        };
        let mut s = Self {
            events: Vec::new(),
            marks: BTreeMap::new(),
            end: 0.0,
        };
        let mut last = 0.0f64;
        let mut explicit_end = None;
        for (i, e) in list.iter().enumerate() {
            let at = |m: String| Error::new(format!("script event {i}: {m}"));
            let t = num(e, "t", 0.0).map_err(|x| at(x.to_string()))?;
            if t < 0.0 {
                return Err(at("'t' must not be negative".into()));
            }
            let ty = e.get("type").and_then(Value::as_str).unwrap_or("click");
            let mut done = t;
            match ty {
                "mark" => {
                    let name = e
                        .get("name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| at("'mark' needs a 'name'".into()))?;
                    s.marks.insert(name.to_owned(), t);
                    if name == "end" {
                        explicit_end = Some(t);
                    }
                }
                "wait" => done = t + num(e, "dur", 0.0).map_err(|x| at(x.to_string()))?.max(0.0),
                _ => {
                    let (events, end) = compile(e, ty, t, fps).map_err(|x| at(x.to_string()))?;
                    s.events.extend(events);
                    done = end;
                }
            }
            last = last.max(done);
        }
        s.end = explicit_end.unwrap_or(last + 1.0);
        Ok(s)
    }

    /// A time as the CLI writes it: seconds, or `mark:NAME`, `mark:NAME+0.5`,
    /// `mark:NAME-0.2`. An exact mark name wins over an offset reading.
    pub fn time(&self, spec: &str) -> Result<f64, Error> {
        let Some(m) = spec.strip_prefix("mark:") else {
            return spec
                .parse()
                .map_err(|_| Error::new(format!("bad time '{spec}' (seconds or mark:NAME[+-secs])")));
        };
        if let Some(t) = self.marks.get(m) {
            return Ok(*t);
        }
        let cut = m.rfind(['+', '-']).filter(|&i| i > 0);
        if let Some(i) = cut
            && let (Some(t), Ok(off)) = (self.marks.get(&m[..i]), m[i..].parse::<f64>())
        {
            return Ok(t + off);
        }
        Err(Error::new(format!(
            "no mark '{m}' (marks: {})",
            self.marks.keys().cloned().collect::<Vec<_>>().join(", ")
        )))
    }
}
