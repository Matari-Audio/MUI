//! `--tree`: the resolved element tree as JSON, for an agent to read and diff.
//!
//! The ideas (key, path, role, label, rect, colours, hover/press) are Rune's
//! `--tree`; the fields are what a MUI `ResolvedScene` already knows. Surfaces
//! come out in paint order, which is z-order; every number is rounded to two
//! decimals; object keys are sorted. Two dumps of the same state are
//! byte-identical and diff line by line.

use std::collections::HashMap;

use mui::prelude::*;
use mui::scene::{Layer, Paint, ResolvedScene, ResolvedSurface};
use serde_json::{Map, Value, json};

use crate::Error;
use crate::session::Session;

/// Round for output: the dump is read by people and diffed, not re-solved.
pub(crate) fn r2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Times, to a tenth of a millisecond: finer than a frame, coarse enough to read.
pub(crate) fn r4(v: f64) -> f64 {
    (v * 1e4).round() / 1e4
}

/// A surface worth listing: it has an id the author chose, or something to
/// say to a screen reader or the eye. Unnamed layout wrappers are left out.
pub(crate) fn listed(s: &ResolvedSurface) -> bool {
    Id::is_named(&s.key) || s.semantics.is_some() || s.text_value.is_some()
}

/// `#rrggbb`, or `#rrggbbaa` when not opaque.
pub(crate) fn hex(c: Color) -> String {
    let [r, g, b, a] = c.to_srgb().components;
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    if a >= 0.999 {
        format!("#{:02x}{:02x}{:02x}", q(r), q(g), q(b))
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", q(r), q(g), q(b), q(a))
    }
}

/// Each surface's first solid fill and first text ink.
pub(crate) fn colours(scene: &ResolvedScene) -> HashMap<&str, (Option<String>, Option<String>)> {
    let mut map: HashMap<&str, (Option<String>, Option<String>)> = HashMap::new();
    for p in &scene.paint {
        let Paint::Solid(c) = &p.paint else { continue };
        let e = map.entry(&p.key).or_default();
        match p.layer {
            Layer::Fill if e.0.is_none() => e.0 = Some(hex(*c)),
            Layer::Text if e.1.is_none() => e.1 = Some(hex(*c)),
            _ => {}
        }
    }
    map
}

pub(crate) fn rect(f: mui::layout::Frame) -> Value {
    json!([r2(f.x), r2(f.y), r2(f.size.width), r2(f.size.height)])
}

/// The ancestors' ids, outermost first, `/`-joined, ending in this id.
fn path(scene: &ResolvedScene, s: &ResolvedSurface) -> String {
    let mut parts = vec![s.key.to_string()];
    let mut at = s.parent.clone();
    while let Some(p) = at.filter(|_| parts.len() < 64) {
        parts.push(p.to_string());
        at = scene.surface(&p).and_then(|s| s.parent.clone());
    }
    parts.reverse();
    parts.join("/")
}

pub(crate) fn role(s: &ResolvedSurface) -> &'static str {
    match s.semantics.as_ref().map(|m| &m.role) {
        Some(A11y::Button) => "button",
        Some(A11y::Slider { .. }) => "slider",
        Some(A11y::Toggle { .. }) => "toggle",
        Some(A11y::TextInput { .. }) => "textinput",
        Some(A11y::Label) => "label",
        Some(A11y::Scroll) => "scroll",
        Some(A11y::Image) => "image",
        None if s.text_value.is_some() => "label",
        Some(A11y::Group) | None => "group",
    }
}

pub fn tree(s: &Session) -> Result<Value, Error> {
    let scene = s.scene()?;
    let ui = s.ui();
    let ink = colours(scene);
    let mut surfaces = Vec::new();
    for (z, sf) in scene.surfaces().enumerate().filter(|(_, sf)| listed(sf)) {
        let id = sf.key.to_string();
        let mut o = Map::new();
        o.insert("id".into(), json!(id));
        o.insert("named".into(), json!(Id::is_named(&id)));
        o.insert("path".into(), json!(path(scene, sf)));
        o.insert("role".into(), json!(role(sf)));
        if let Some(p) = &sf.parent {
            o.insert("parent".into(), json!(p.to_string()));
        }
        if let Some(sem) = &sf.semantics {
            let name = sem.label.as_deref().or(sf.text_value.as_deref());
            if let Some(n) = name {
                o.insert("name".into(), json!(n));
            }
            match &sem.role {
                A11y::Slider { value, min, max } => {
                    o.insert("value".into(), json!(r2(*value)));
                    o.insert("range".into(), json!([r2(*min), r2(*max)]));
                }
                A11y::Toggle { on } => {
                    o.insert("value".into(), json!(on));
                }
                A11y::TextInput {
                    value, selection, ..
                } => {
                    o.insert("value".into(), json!(&**value));
                    o.insert("selection".into(), json!([selection.0, selection.1]));
                }
                _ => {}
            }
        } else if let Some(t) = &sf.text_value {
            o.insert("name".into(), json!(&**t));
        }
        if let Some(v) = &sf.value_description {
            o.insert("value_text".into(), json!(&**v));
        }
        if let Some(t) = &sf.text_value {
            o.insert("text".into(), json!(&**t));
        }
        o.insert("rect".into(), rect(sf.frame));
        if let Some(c) = &sf.clip {
            o.insert(
                "clip".into(),
                json!([r2(c.x0), r2(c.y0), r2(c.width()), r2(c.height())]),
            );
        }
        o.insert("z".into(), json!(z));
        o.insert("disabled".into(), json!(sf.disabled));
        o.insert("focusable".into(), json!(sf.focusable));
        o.insert("focused".into(), json!(ui.focused(id.as_str())));
        let r = ui.get(id.as_str());
        o.insert("hovered".into(), json!(r.hovered));
        o.insert("pressed".into(), json!(r.held));
        let st = ui.state(id.as_str());
        o.insert("hover".into(), json!(r2(st.hover)));
        o.insert("press".into(), json!(r2(st.press)));
        if let Some(c) = sf.cursor {
            o.insert("cursor".into(), json!(format!("{c:?}")));
        }
        if let Some(t) = &sf.tip {
            o.insert("tooltip".into(), json!(&**t));
        }
        if let Some((fill, text)) = ink.get(id.as_str()) {
            if let Some(f) = fill {
                o.insert("fill".into(), json!(f));
            }
            if let Some(t) = text {
                o.insert("ink".into(), json!(t));
            }
        }
        surfaces.push(Value::Object(o));
    }
    let (pos, buttons) = s.pointer();
    let mut down = Vec::new();
    for (b, n) in [
        (Button::Primary, "primary"),
        (Button::Secondary, "secondary"),
        (Button::Middle, "middle"),
    ] {
        if buttons.contains(b) {
            down.push(n);
        }
    }
    Ok(json!({
        "scene": s.name,
        "size": [r2(s.size.width), r2(s.size.height)],
        "scale": s.scale,
        "theme": format!("{:?}", ui.theme().palette.mode).to_lowercase(),
        "frame": s.frame(),
        "t": r4(s.time()),
        "focus": ui.focus_key(),
        "pointer": pos.map(|p| json!([r2(p.x), r2(p.y)])),
        "buttons": down,
        "cursor": format!("{:?}", s.cursor()),
        "tip": s.tip().map(|(t, p)| json!({"text": t, "at": [r2(p.x), r2(p.y)]})),
        "surfaces": surfaces,
    }))
}

/// Pretty JSON with arrays of plain values on one line (`"rect": [1, 2, 3, 4]`),
/// so a surface is a dozen lines and a diff names the field that moved.
pub fn pretty(v: &Value) -> String {
    fn go(v: &Value, ind: usize, out: &mut String) {
        let pad = |n: usize| "  ".repeat(n);
        match v {
            Value::Array(a) if a.iter().all(|x| !x.is_object() && !x.is_array()) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&x.to_string());
                }
                out.push(']');
            }
            Value::Array(a) => {
                out.push_str("[\n");
                for (i, x) in a.iter().enumerate() {
                    out.push_str(&pad(ind + 1));
                    go(x, ind + 1, out);
                    out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
                }
                out.push_str(&pad(ind));
                out.push(']');
            }
            Value::Object(o) if !o.is_empty() => {
                out.push_str("{\n");
                for (i, (k, x)) in o.iter().enumerate() {
                    out.push_str(&format!("{}{}: ", pad(ind + 1), Value::from(k.as_str())));
                    go(x, ind + 1, out);
                    out.push_str(if i + 1 < o.len() { ",\n" } else { "\n" });
                }
                out.push_str(&pad(ind));
                out.push('}');
            }
            other => out.push_str(&other.to_string()),
        }
    }
    let mut s = String::new();
    go(v, 0, &mut s);
    s.push('\n');
    s
}
