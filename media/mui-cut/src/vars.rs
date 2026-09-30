//! Variables and variants. A project may declare typed `variables` and
//! `variants` (a size, an fps, variable values and per-layer overrides).
//! Anywhere under `scenes` a value can be a binding,
//! `{"var": "accent"}`, `{"var": "W", "mul": 0.5, "add": 20}` or
//! `{"var": "theme", "map": {"dark": "#000000", "light": "#ffffff"}}`, and
//! any string can hold `{name}`. `W` and `H` are the variant's frame size.
//!
//! Bindings are resolved on the JSON before it becomes a [`Project`], so any
//! field binds (a key's value, a background, an effect parameter) and no
//! document type changes; a project with variables saves as its own text.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::Rgba;

/// A declared variable: its type and default value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Var {
    Color { value: Rgba },
    Number { value: f64 },
    String { value: String },
    Enum { options: Vec<String>, value: String },
    Bool { value: bool },
}

/// One version of the project: e.g. `light-vertical`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Variant {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fps: Option<f64>,
    /// Variable values, replacing the declared ones.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub vars: BTreeMap<String, Value>,
    /// JSON merged into scenes and layers before bindings resolve: keys
    /// `scene`, `scene/layer`, `*` or `*/layer`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, Value>,
}

impl Var {
    /// `v` as this variable's type, or why not.
    fn accept(&self, name: &str, v: &Value) -> Result<Value, String> {
        let bad = || format!("variable `{name}`: {v} does not fit its type");
        match self {
            Self::Color { .. } => {
                let s = v.as_str().ok_or_else(bad)?;
                Rgba::try_from(s.to_owned()).map_err(|e| format!("variable `{name}`: {e}"))?;
            }
            Self::Number { .. } => {
                v.as_f64().filter(|f| f.is_finite()).ok_or_else(bad)?;
            }
            Self::String { .. } => {
                v.as_str().ok_or_else(bad)?;
            }
            Self::Enum { options, .. } => {
                let s = v.as_str().ok_or_else(bad)?;
                if !options.iter().any(|o| o == s) {
                    return Err(format!(
                        "variable `{name}`: `{s}` is not one of {}",
                        options.join(", ")
                    ));
                }
            }
            Self::Bool { .. } => {
                v.as_bool().ok_or_else(bad)?;
            }
        }
        Ok(v.clone())
    }
    fn value(&self) -> Value {
        match self {
            Self::Color { value } => Value::String((*value).into()),
            Self::Number { value } => Value::from(*value),
            Self::String { value } | Self::Enum { value, .. } => Value::String(value.clone()),
            Self::Bool { value } => Value::Bool(*value),
        }
    }
}

/// Does this project text use variables or variants at all?
pub(crate) fn uses_vars(root: &Value) -> bool {
    ["variables", "variants"].iter().any(|k| {
        root.get(k).is_some_and(|v| {
            v.as_object().is_some_and(|m| !m.is_empty())
                || v.as_array().is_some_and(|a| !a.is_empty())
        })
    })
}

/// `root` with `variant` (or the declared defaults) applied: size, fps,
/// overrides, and every binding and `{name}` under `scenes` replaced.
pub(crate) fn resolve(root: &Value, variant: Option<&str>) -> Result<Value, String> {
    let mut out = root.clone();
    let vars: BTreeMap<String, Var> = match root.get("variables") {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("variables: {e}"))?,
        None => BTreeMap::new(),
    };
    let variants: Vec<Variant> = match root.get("variants") {
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| format!("variants: {e}"))?,
        None => Vec::new(),
    };
    let mut values: BTreeMap<String, Value> =
        vars.iter().map(|(k, v)| (k.clone(), v.value())).collect();
    if let Some(name) = variant {
        let v = variants
            .iter()
            .find(|v| v.name == name)
            .ok_or_else(|| format!("no variant `{name}`"))?;
        if let Some(size) = v.size {
            out["size"] = serde_json::json!(size);
        }
        if let Some(fps) = v.fps {
            out["fps"] = fps.into();
        }
        for (k, val) in &v.vars {
            let decl = vars
                .get(k)
                .ok_or_else(|| format!("variant `{name}`: no variable `{k}`"))?;
            values.insert(k.clone(), decl.accept(k, val)?);
        }
        for (key, patch) in &v.overrides {
            overlay(&mut out, key, patch).map_err(|e| format!("variant `{name}`: {e}"))?;
        }
    }
    let size = out
        .get("size")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (i, n) in ["W", "H"].iter().enumerate() {
        values.insert((*n).into(), size.get(i).cloned().unwrap_or(Value::from(0)));
    }
    if let Some(scenes) = out.get_mut("scenes") {
        bind(scenes, &values)?;
    }
    Ok(out)
}

/// Merge `patch` into the scenes or layers `key` names.
fn overlay(root: &mut Value, key: &str, patch: &Value) -> Result<(), String> {
    let (scene, layer) = match key.split_once('/') {
        Some((s, l)) => (s, Some(l)),
        None => (key, None),
    };
    let mut hit = false;
    for s in root
        .get_mut("scenes")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if scene != "*" && s.get("name").and_then(Value::as_str) != Some(scene) {
            continue;
        }
        match layer {
            None => {
                merge(s, patch);
                hit = true;
            }
            Some(id) => {
                for l in s
                    .get_mut("layers")
                    .and_then(Value::as_array_mut)
                    .into_iter()
                    .flatten()
                {
                    if l.get("id").and_then(Value::as_str) == Some(id) {
                        merge(l, patch);
                        hit = true;
                    }
                }
            }
        }
    }
    if hit {
        Ok(())
    } else {
        Err(format!("override `{key}` matches nothing"))
    }
}

fn merge(into: &mut Value, patch: &Value) {
    if let (Some(a), Some(b)) = (into.as_object_mut(), patch.as_object()) {
        for (k, v) in b {
            a.insert(k.clone(), v.clone());
        }
    }
}

/// A binding object: `var` and nothing but `mul`, `add` and `map`.
fn binding(m: &Map<String, Value>) -> bool {
    m.contains_key("var")
        && m.keys()
            .all(|k| matches!(k.as_str(), "var" | "mul" | "add" | "map"))
}

fn bind(v: &mut Value, values: &BTreeMap<String, Value>) -> Result<(), String> {
    match v {
        Value::Object(m) if binding(m) => *v = eval(m, values)?,
        Value::Object(m) => {
            for x in m.values_mut() {
                bind(x, values)?;
            }
        }
        Value::Array(a) => {
            for x in a {
                bind(x, values)?;
            }
        }
        Value::String(s) if s.contains('{') => *s = interpolate(s, values),
        _ => {}
    }
    Ok(())
}

fn eval(m: &Map<String, Value>, values: &BTreeMap<String, Value>) -> Result<Value, String> {
    let name = m["var"].as_str().ok_or("`var` must name a variable")?;
    let base = values
        .get(name)
        .ok_or_else(|| format!("no variable `{name}`"))?;
    if let Some(map) = m.get("map") {
        let key = match base {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        return map
            .get(&key)
            .cloned()
            .ok_or_else(|| format!("`{name}` map has no `{key}`"));
    }
    let (mul, add) = (m.get("mul"), m.get("add"));
    if mul.is_none() && add.is_none() {
        return Ok(base.clone());
    }
    let n = match base {
        Value::Bool(b) => f64::from(u8::from(*b)),
        v => v
            .as_f64()
            .ok_or_else(|| format!("`{name}` is not a number to scale"))?,
    };
    let num = |v: Option<&Value>, d: f64| {
        v.map_or(Ok(d), |v| v.as_f64().ok_or("`mul`/`add` must be numbers"))
    };
    Ok(Value::from(n * num(mul, 1.)? + num(add, 0.)?))
}

/// `{name}` replaced by the variable's value; unknown names stay.
fn interpolate(s: &str, values: &BTreeMap<String, Value>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        match tail.find('}').map(|j| (&tail[1..j], j)) {
            Some((name, j)) if values.contains_key(name) => {
                match &values[name] {
                    Value::String(v) => out.push_str(v),
                    v => out.push_str(&v.to_string()),
                }
                rest = &tail[j + 1..];
            }
            _ => {
                out.push('{');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Compact or hand-spaced JSON as serde's pretty printer lays it out (two
/// spaces, `"key": value`), keys in the order written.
pub(crate) fn pretty(json: &str) -> String {
    let mut out = String::with_capacity(json.len() * 2);
    let mut depth = 0usize;
    let (mut in_str, mut escaped) = (false, false);
    let mut chars = json.chars().peekable();
    let indent = |out: &mut String, d: usize| {
        out.push('\n');
        out.extend(std::iter::repeat_n(' ', d * 2));
    };
    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            (in_str, escaped) = (!(c == '"' && !escaped), c == '\\' && !escaped);
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
            }
            '{' | '[' => {
                out.push(c);
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                if chars.peek().is_some_and(|n| matches!(n, '}' | ']')) {
                    out.push(chars.next().expect("peeked"));
                } else {
                    depth += 1;
                    indent(&mut out, depth);
                }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                indent(&mut out, depth);
                out.push(c);
            }
            ',' => {
                out.push(c);
                indent(&mut out, depth);
            }
            ':' => out.push_str(": "),
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    out
}
