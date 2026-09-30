//! `mui-cut check`: everything wrong with a project that loading lets
//! through, each with the JSON path it is about, the times it happens at and
//! a fix. Schema-level mistakes (a typo'd field serde would silently drop, a
//! property the layer's kind ignores), key mistakes (duplicate times, keys
//! past the scene's end, overshooting curves) and what only shows in the
//! pixels (layers never on screen, clipped at rest, text colliding, text
//! too close to what is behind it, motion that strobes, empty frames).
//!
//! Pure apart from the [`Renderer`] it samples contrast with and the
//! `exists` callback that says whether an asset file is there, so it runs
//! the same in the CLI, the MCP server and (later) the web editor.
use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::Value;

use crate::{Anim, Frame, Interp, Kind, Project, Prop, Renderer, Rgba, Scene, eval};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// One finding.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Issue {
    pub severity: Severity,
    /// Stable, for filtering: `unknown_field`, `off_frame`, `low_contrast`...
    pub code: &'static str,
    /// Where in the file: `scenes[0].layers[2].x[1]`.
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    /// Seconds into the scene: the first and last sampled time it holds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t: Option<[f64; 2]>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sev = match self.severity {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        write!(f, "{sev}[{}] {}", self.code, self.path)?;
        if let Some(l) = &self.layer {
            write!(f, " ({l})")?;
        }
        match self.t {
            Some([a, b]) if (b - a).abs() < 1e-9 => write!(f, " t={a:.2}")?,
            Some([a, b]) => write!(f, " t={a:.2}..{b:.2}")?,
            None => {}
        }
        write!(f, ": {}", self.message)?;
        if let Some(fix) = &self.fix {
            write!(f, "\n  fix: {fix}")?;
        }
        Ok(())
    }
}

/// Text needs this contrast against what is behind it (WCAG large text).
pub const MIN_CONTRAST: f64 = 3.0;
/// Faster than this fraction of the frame's long side per frame strobes.
pub const FAST: f64 = 0.08;

/// Check `src` (a project's text). `r` samples the pixels behind text (any
/// size; small is fast) and holds the assets quads are measured with;
/// `exists` says whether a path relative to the project is there.
pub fn check(src: &str, r: &mut Renderer, exists: &dyn Fn(&str) -> bool) -> Vec<Issue> {
    let p = match Project::load(src) {
        Ok(p) => p,
        Err(e) => return vec![load_issue(&e)],
    };
    let mut out = Issues::default();
    if let Ok(raw) = serde_json::from_str::<Value>(src) {
        fields(&raw, &p, &mut out);
    }
    for (si, s) in p.scenes.iter().enumerate() {
        keys_and_assets(&p, si, s, exists, &mut out);
        // The pixel lints measure the flat composite; a 3D scene's shot is
        // somewhere else, so they would only mislead.
        if s.mode != crate::Mode::ThreeD {
            visual(&p, si, s, r, &mut out);
        }
    }
    if p.scenes.is_empty() {
        out.add(
            Severity::Warning,
            "no_scenes",
            "scenes",
            None,
            None,
            "the project has no scenes, so nothing renders".into(),
            Some("add a scene: {\"name\": \"main\", \"duration\": 3, \"layers\": []}".into()),
        );
    }
    out.done()
}

/// Only the field lints (and a load error): what is cheap enough to run
/// on every edit, and what an edit must not introduce, since a save drops
/// unknown fields.
pub fn lint_fields(src: &str) -> Vec<Issue> {
    let p = match Project::load(src) {
        Ok(p) => p,
        Err(e) => return vec![load_issue(&e)],
    };
    let mut out = Issues::default();
    if let Ok(raw) = serde_json::from_str::<Value>(src) {
        fields(&raw, &p, &mut out);
    }
    out.done()
}

/// A load error as an issue, its path split off the message.
pub fn load_issue(e: &str) -> Issue {
    let (path, msg) = match e.split_once(": ") {
        Some((p, m)) if !p.contains(' ') => (p.to_owned(), m.to_owned()),
        _ => (String::new(), e.to_owned()),
    };
    Issue {
        severity: Severity::Error,
        code: "load",
        path,
        scene: None,
        layer: None,
        t: None,
        message: msg,
        fix: Some("`mui-cut schema` prints what every field accepts".into()),
    }
}

/// Issues keyed by code and path, so one found at many times is reported
/// once with the time range it covers.
#[derive(Default)]
struct Issues(BTreeMap<(String, &'static str), Issue>);

impl Issues {
    #[expect(clippy::too_many_arguments, reason = "an issue has this many parts")]
    fn add(
        &mut self,
        severity: Severity,
        code: &'static str,
        path: &str,
        at: Option<(&Scene, &str)>,
        t: Option<f64>,
        message: String,
        fix: Option<String>,
    ) {
        let e = self
            .0
            .entry((path.to_owned(), code))
            .or_insert_with(|| Issue {
                severity,
                code,
                path: path.to_owned(),
                scene: at.map(|(s, _)| s.name.clone()),
                layer: at.map(|(_, l)| l.to_owned()).filter(|l| !l.is_empty()),
                t: t.map(|t| [t, t]),
                message,
                fix,
            });
        if let (Some(t), Some([a, b])) = (t, &mut e.t) {
            *a = a.min(t);
            *b = b.max(t);
        }
    }
    fn done(self) -> Vec<Issue> {
        let mut v: Vec<Issue> = self.0.into_values().collect();
        v.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| a.path.cmp(&b.path))
        });
        v
    }
}

// ---------------------------------------------------------------- fields

/// `properties` of a schema node, and of every `oneOf`/`anyOf`/`allOf`
/// member inlined in it (how a flattened enum shows up).
fn props_of(node: &Value) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = node["properties"]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    for k in ["oneOf", "anyOf", "allOf"] {
        for m in node[k].as_array().into_iter().flatten() {
            out.extend(props_of(m));
        }
    }
    out
}

/// A tagged enum's variant for `kind`, as its own property set.
fn variant(node: &Value, kind: &str) -> BTreeSet<String> {
    let top: BTreeSet<String> = node["properties"]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    let v = node["oneOf"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|m| m["properties"]["kind"]["const"].as_str() == Some(kind));
    top.into_iter()
        .chain(v.map(props_of).unwrap_or_default())
        .collect()
}

/// The closest known name, if it is close enough to be a typo.
fn nearest<'a>(name: &str, known: impl IntoIterator<Item = &'a String>) -> Option<&'a String> {
    known
        .into_iter()
        .map(|k| (distance(name, k), k))
        .filter(|(d, k)| *d <= 2.max(k.len() / 4))
        .min()
        .map(|(_, k)| k)
}

fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

fn unknown(
    out: &mut Issues,
    obj: &Value,
    known: &BTreeSet<String>,
    path: &str,
    at: Option<(&Scene, &str)>,
) {
    for k in obj.as_object().into_iter().flat_map(|o| o.keys()) {
        if known.contains(k) {
            continue;
        }
        let fix = nearest(k, known).map_or_else(
            || format!("remove it; known fields: {}", join(known)),
            |n| format!("did you mean `{n}`?"),
        );
        out.add(
            Severity::Warning,
            "unknown_field",
            &format!("{path}.{k}"),
            at,
            None,
            format!("`{k}` is not a field here, so loading ignores it"),
            Some(fix),
        );
    }
}

fn join(s: &BTreeSet<String>) -> String {
    s.iter().map(String::as_str).collect::<Vec<_>>().join(", ")
}

/// Keys every JSON object in the file may have, by the schema.
fn fields(raw: &Value, p: &Project, out: &mut Issues) {
    let schema = Project::json_schema();
    let def = |n: &str| schema["$defs"][n].clone();
    unknown(out, raw, &props_of(&schema), "", None);
    binding_fields(out, &raw["scenes"], &props_of(&def("Binding")), "scenes");
    let (scene, layer, animator, deformer) =
        (def("Scene"), def("Layer"), def("Animator"), def("Deformer"));
    let key = props_of(&def("Key_double"));
    let all_layer = props_of(&layer);
    let top_layer: BTreeSet<String> = layer["properties"]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    for (si, (rs, s)) in raw["scenes"]
        .as_array()
        .into_iter()
        .flatten()
        .zip(&p.scenes)
        .enumerate()
    {
        let sp = format!("scenes[{si}]");
        unknown(out, rs, &props_of(&scene), &sp, Some((s, "")));
        for (li, (rl, l)) in rs["layers"]
            .as_array()
            .into_iter()
            .flatten()
            .zip(&s.layers)
            .enumerate()
        {
            let lp = format!("{sp}.layers[{li}]");
            let at = Some((s, l.id.as_str()));
            let kind = rl["kind"].as_str().unwrap_or("");
            let mine = variant(&layer, kind);
            let three = s.mode == crate::Mode::ThreeD;
            let used: BTreeSet<String> = l.props_in(three).into_iter().map(|(n, _)| n).collect();
            for k in rl.as_object().into_iter().flat_map(|o| o.keys()) {
                let path = format!("{lp}.{k}");
                if !all_layer.contains(k) {
                    unknown(out, &serde_json::json!({ k: null }), &all_layer, &lp, at);
                } else if !mine.contains(k) {
                    out.add(
                        Severity::Info,
                        "wrong_kind",
                        &path,
                        at,
                        None,
                        format!("`{k}` belongs to another kind of layer; a `{kind}` ignores it"),
                        Some(format!("remove `{k}`")),
                    );
                } else if top_layer.contains(k)
                    && !used.contains(k)
                    && !matches!(
                        k.as_str(),
                        "id" | "name" | "kind" | "animators" | "deformers"
                    )
                    && !(three && matches!(k.as_str(), "cast_shadows" | "receive_shadows"))
                {
                    out.add(
                        Severity::Info,
                        "ignored_prop",
                        &path,
                        at,
                        None,
                        format!("a `{kind}` layer does not use `{k}`"),
                        Some(format!(
                            "remove `{k}`; this layer's properties: {}",
                            used.iter()
                                .filter(|u| !u.contains('.'))
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                    );
                }
                keys_fields(out, &rl[k], &key, &path, at);
            }
            for (ai, ra) in rl["animators"].as_array().into_iter().flatten().enumerate() {
                let ap = format!("{lp}.animators[{ai}]");
                unknown(out, ra, &props_of(&animator), &ap, at);
                for (k, v) in ra.as_object().into_iter().flatten() {
                    keys_fields(out, v, &key, &format!("{ap}.{k}"), at);
                }
            }
            for (di, rd) in rl["deformers"].as_array().into_iter().flatten().enumerate() {
                let dp = format!("{lp}.deformers[{di}]");
                let dk = rd["kind"].as_str().unwrap_or("");
                unknown(out, rd, &variant(&deformer, dk), &dp, at);
                for (k, v) in rd.as_object().into_iter().flatten() {
                    keys_fields(out, v, &key, &format!("{dp}.{k}"), at);
                }
            }
        }
    }
}

/// Unknown fields inside every binding (an object with `var`) under `v`.
fn binding_fields(out: &mut Issues, v: &Value, known: &BTreeSet<String>, path: &str) {
    match v {
        Value::Object(m) if m.contains_key("var") => unknown(out, v, known, path, None),
        Value::Object(m) => {
            for (k, x) in m {
                binding_fields(out, x, known, &format!("{path}.{k}"));
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                binding_fields(out, x, known, &format!("{path}[{i}]"));
            }
        }
        _ => {}
    }
}

/// Unknown fields inside a key list.
fn keys_fields(
    out: &mut Issues,
    v: &Value,
    key: &BTreeSet<String>,
    path: &str,
    at: Option<(&Scene, &str)>,
) {
    for (i, k) in v.as_array().into_iter().flatten().enumerate() {
        if k.get("t").is_some() {
            unknown(out, k, key, &format!("{path}[{i}]"), at);
        }
    }
}

// ---------------------------------------------------------------- keys

/// `animators.0.x` as a JSON path: `animators[0].x`.
fn json_path(prop: &str) -> String {
    prop.split('.')
        .map(|s| {
            if s.parse::<usize>().is_ok() {
                format!("[{s}]")
            } else {
                format!(".{s}")
            }
        })
        .collect::<String>()
        .trim_start_matches('.')
        .to_owned()
}

fn keys_and_assets(
    p: &Project,
    si: usize,
    s: &Scene,
    exists: &dyn Fn(&str) -> bool,
    out: &mut Issues,
) {
    let sp = format!("scenes[{si}]");
    if s.duration * p.fps < 1. {
        out.add(
            Severity::Warning,
            "short_scene",
            &format!("{sp}.duration"),
            Some((s, "")),
            None,
            format!(
                "`{}` is shorter than one frame at {} fps, so it renders one frame",
                s.name, p.fps
            ),
            Some(format!("make duration at least {:.3}", 1. / p.fps)),
        );
    }
    if s.layers.is_empty() {
        out.add(
            Severity::Info,
            "empty_scene",
            &format!("{sp}.layers"),
            Some((s, "")),
            None,
            format!("`{}` has no layers: only its background shows", s.name),
            None,
        );
    }
    let dt = 1. / p.fps;
    for (li, l) in s.layers.iter().enumerate() {
        let lp = format!("{sp}.layers[{li}]");
        let at = Some((s, l.id.as_str()));
        if let Some(a) = l.asset()
            && !exists(a)
        {
            out.add(
                Severity::Error,
                "missing_asset",
                &format!("{lp}.path"),
                at,
                None,
                format!("`{a}` is not there (paths are relative to the project file)"),
                Some("fix the path or add the file".into()),
            );
        }
        if let Kind::Text { text, .. } = &l.kind
            && text.trim().is_empty()
        {
            out.add(
                Severity::Warning,
                "empty_text",
                &format!("{lp}.text"),
                at,
                None,
                "the text is empty, so the layer draws nothing".into(),
                None,
            );
        }
        for (name, prop) in l.props_in(true) {
            let path = format!("{lp}.{}", json_path(&name));
            let times: Vec<f64> = match prop {
                Prop::Num(Anim::Keys(k)) => k.iter().map(|k| k.t).collect(),
                Prop::Color(Anim::Keys(k)) => k.iter().map(|k| k.t).collect(),
                _ => continue,
            };
            for (i, w) in times.windows(2).enumerate() {
                if (w[1] - w[0]).abs() < dt * 0.5 {
                    out.add(
                        Severity::Warning,
                        "duplicate_key",
                        &format!("{path}[{}]", i + 1),
                        at,
                        Some(w[1]),
                        format!("two keys less than half a frame apart (t {} and {}): the second is a jump no frame shows", w[0], w[1]),
                        Some("remove one, or move it at least a frame away".into()),
                    );
                }
            }
            for (i, &t) in times.iter().enumerate() {
                if t < -1e-9 || t > s.duration + 1e-9 {
                    out.add(
                        Severity::Warning,
                        "key_outside_scene",
                        &format!("{path}[{i}]"),
                        at,
                        Some(t),
                        format!(
                            "key at t={t} is outside the scene (0..{}), so it never plays",
                            s.duration
                        ),
                        Some(format!(
                            "move it into 0..{} or lengthen the scene",
                            s.duration
                        )),
                    );
                }
            }
            if let Prop::Num(a @ Anim::Keys(k)) = prop {
                overshoot(out, &name, &path, a, k, at);
            }
        }
    }
}

/// A bezier segment leaving the range between its two keys. Often meant
/// (a bounce); worth a warning where the value is clamped anyway.
fn overshoot(
    out: &mut Issues,
    name: &str,
    path: &str,
    a: &Anim<f64>,
    keys: &[crate::Key<f64>],
    at: Option<(&Scene, &str)>,
) {
    let last = name.rsplit('.').next().unwrap_or(name);
    for (i, w) in keys.windows(2).enumerate() {
        let (k0, k1) = (&w[0], &w[1]);
        if k0.interp != Interp::Bezier || k1.t <= k0.t {
            continue;
        }
        let (lo, hi) = (k0.v.min(k1.v), k0.v.max(k1.v));
        let (mut worst, mut when) = (0f64, k0.t);
        for j in 1..32 {
            let t = k0.t + (k1.t - k0.t) * f64::from(j) / 32.;
            let v = a.at(t);
            let o = (lo - v).max(v - hi);
            if o > worst {
                (worst, when) = (o, t);
            }
        }
        let span = (hi - lo).max(1e-9);
        if worst <= 1e-6 + 0.02 * span {
            continue;
        }
        let v = a.at(when);
        let clamped = match last {
            "opacity" => !(0. ..=1.).contains(&v),
            "scale" | "width" | "height" | "radius" | "stroke_width" | "font_size" => v < 0.,
            _ => false,
        };
        out.add(
            if clamped { Severity::Warning } else { Severity::Info },
            "overshoot",
            &format!("{path}[{i}]"),
            at,
            Some(when),
            if clamped {
                format!("the curve reaches {v:.3} at t={when:.2}, outside what `{last}` can be, so it flattens there")
            } else {
                format!("the curve overshoots its keys ({:.3}..{:.3}) to {v:.3} at t={when:.2}", k0.v, k1.v)
            },
            Some("shorten the handles' value offsets (`out`/`in` dv) if this is not a deliberate bounce".into()),
        );
    }
}

// ---------------------------------------------------------------- pixels

/// The time of every key of every property of `l`, unsorted.
pub fn key_times(l: &crate::Layer) -> Vec<f64> {
    let mut ts = Vec::new();
    for (_, prop) in l.props_in(true) {
        match prop {
            Prop::Num(Anim::Keys(k)) => ts.extend(k.iter().map(|k| k.t)),
            Prop::Color(Anim::Keys(k)) => ts.extend(k.iter().map(|k| k.t)),
            _ => {}
        }
    }
    ts
}

/// An axis-aligned box: `[x0, y0, x1, y1]`.
type Bbox = [f64; 4];

fn bbox(pts: &[[f64; 2]; 4]) -> Bbox {
    let xs = pts.map(|p| p[0]);
    let ys = pts.map(|p| p[1]);
    let min = |v: [f64; 4]| v.into_iter().fold(f64::INFINITY, f64::min);
    let max = |v: [f64; 4]| v.into_iter().fold(f64::NEG_INFINITY, f64::max);
    [min(xs), min(ys), max(xs), max(ys)]
}
fn area(b: Bbox) -> f64 {
    (b[2] - b[0]).max(0.) * (b[3] - b[1]).max(0.)
}
fn meet(a: Bbox, b: Bbox) -> Bbox {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]
}

/// The times worth looking at: both ends, every key, and an even spread.
fn sample_times(p: &Project, s: &Scene) -> Vec<f64> {
    let end = (s.duration - 1. / p.fps).max(0.);
    let mut ts: Vec<f64> = (0..=12).map(|i| end * f64::from(i) / 12.).collect();
    for l in &s.layers {
        ts.extend(key_times(l));
    }
    ts.retain(|t| (0. ..=end).contains(t));
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|a, b| (*a - *b).abs() < 0.5 / p.fps);
    ts
}

fn luminance(c: [f64; 3]) -> f64 {
    let lin = |v: f64| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

fn visual(p: &Project, si: usize, s: &Scene, r: &mut Renderer, out: &mut Issues) {
    let sp = format!("scenes[{si}]");
    let [fw, fh] = p.size.map(f64::from);
    let screen = [0., 0., fw, fh];
    let dt = 1. / p.fps;
    let n = s.layers.len();

    // Speed: every frame, from the evaluator alone.
    let frames = (s.duration * p.fps).round().max(1.) as usize;
    let limit = FAST * fw.max(fh);
    for (li, l) in s.layers.iter().enumerate() {
        let mut worst = (0f64, 0f64);
        let mut last = (l.x.at(0.), l.y.at(0.));
        for i in 1..frames {
            let t = i as f64 * dt;
            let now = (l.x.at(t), l.y.at(t));
            let v = (now.0 - last.0).hypot(now.1 - last.1);
            last = now;
            if v > limit && l.opacity.at(t) > 0.01 {
                out.add(
                    Severity::Warning,
                    "fast_motion",
                    &format!("{sp}.layers[{li}]"),
                    Some((s, &l.id)),
                    Some(t),
                    String::new(),
                    Some(
                        "ease the keys over more time, or render with motion blur (`--mb 8`)"
                            .into(),
                    ),
                );
                if v > worst.0 {
                    worst = (v, t);
                }
            }
        }
        if worst.0 > 0. {
            let key = (format!("{sp}.layers[{li}]"), "fast_motion");
            if let Some(i) = out.0.get_mut(&key) {
                i.message = format!(
                    "moves up to {:.0} px a frame (at t={:.2}); above {limit:.0} px a frame motion strobes",
                    worst.0, worst.1
                );
            }
        }
    }

    // Placement, overlap and contrast at the sampled times.
    let mut seen = vec![false; n];
    let mut contrast: Vec<Option<(f64, f64, bool)>> = vec![None; n];
    let mut contrast_samples = vec![0usize; n];
    let (mut shown, mut empty) = (Vec::new(), Vec::new());
    for t in sample_times(p, s) {
        let f = eval(p, s, t);
        let next = eval(p, s, t + dt);
        let (Ok(q), Ok(qn)) = (r.assets.layers(&f), r.assets.layers(&next)) else {
            continue;
        };
        let boxes: Vec<Bbox> = q.quads.iter().map(|q| bbox(&q.pts)).collect();
        // At rest: not moving and not fading, so what shows is meant.
        let still: Vec<bool> = (0..n)
            .map(|i| {
                let (a, b) = (&q.quads[i].pts, &qn.quads[i].pts);
                let fixed = a
                    .iter()
                    .zip(b)
                    .all(|(a, b)| (a[0] - b[0]).abs() < 0.5 && (a[1] - b[1]).abs() < 0.5);
                fixed && (f.layers[i].opacity - next.layers[i].opacity).abs() < 1e-3
            })
            .collect();
        let shows = |i: usize| {
            let d = &f.layers[i];
            let own_paint = matches!(
                d.kind,
                Kind::Svg { .. } | Kind::Lottie { .. } | Kind::Image { .. }
            );
            let stroked = d.stroke.0[3] > 0 && d.stroke_width > 0.;
            d.opacity > 0.01 && d.scale != 0. && (own_paint || stroked || d.fill.0[3] > 0)
        };
        let mut any = false;
        for i in 0..n {
            let l = &s.layers[i];
            let lp = format!("{sp}.layers[{i}]");
            let b = boxes[i];
            let on = area(meet(b, screen));
            if !shows(i) || area(b) <= 0. {
                continue;
            }
            if on > 0. {
                seen[i] = true;
                any = true;
            }
            let frac = on / area(b);
            let background = on > 0.9 * fw * fh;
            if still[i] && frac < 0.6 && on > 0. && !background {
                out.add(
                    Severity::Warning,
                    "clipped",
                    &lp,
                    Some((s, &l.id)),
                    Some(t),
                    format!(
                        "at rest with only {:.0}% of it inside the frame",
                        frac * 100.
                    ),
                    Some(format!(
                        "move it inside 0..{fw} x 0..{fh} (its box is {:.0},{:.0} to {:.0},{:.0})",
                        b[0], b[1], b[2], b[3]
                    )),
                );
            }
            if !matches!(l.kind, Kind::Text { .. }) || !still[i] {
                continue;
            }
            for j in 0..i {
                if !matches!(s.layers[j].kind, Kind::Text { .. }) || !still[j] || !shows(j) {
                    continue;
                }
                let o = area(meet(b, boxes[j]));
                if o > 0.15 * area(b).min(area(boxes[j])) {
                    out.add(
                        Severity::Warning,
                        "text_overlap",
                        &lp,
                        Some((s, &l.id)),
                        Some(t),
                        format!(
                            "overlaps text layer `{}` while both are at rest",
                            s.layers[j].id
                        ),
                        Some("move one of them, or fade one out before the other arrives".into()),
                    );
                }
            }
            // Contrast against the layers below, a few times per layer.
            if on > 0. && contrast_samples[i] < 6 {
                contrast_samples[i] += 1;
                if let Some(c) = contrast_at(r, &f, i, meet(b, screen)) {
                    let worse = contrast[i].is_none_or(|(w, ..)| c.0 < w);
                    if worse {
                        contrast[i] = Some((c.0, t, c.1));
                    }
                }
            }
        }
        if any {
            shown.push(t);
        } else if n > 0 {
            empty.push(t);
        }
    }
    // A lead-in or tail on the bare background is a choice; a gap between
    // things showing is what looks broken.
    let (first, last) = (shown.first().copied(), shown.last().copied());
    for t in empty {
        if first.is_some_and(|a| a < t) && last.is_some_and(|b| t < b) {
            out.add(
                Severity::Warning,
                "empty_frame",
                &format!("{sp}.layers"),
                Some((s, "")),
                Some(t),
                "no layer is visible in the frame: only the background shows".into(),
                Some("check opacity keys and positions around these times".into()),
            );
        }
    }
    for (i, l) in s.layers.iter().enumerate() {
        let lp = format!("{sp}.layers[{i}]");
        if !seen[i] {
            out.add(
                Severity::Warning,
                "never_visible",
                &lp,
                Some((s, &l.id)),
                None,
                "never visible inside the frame at any sampled time (off-frame, transparent or scaled to 0)".into(),
                Some(format!("place it inside 0..{fw} x 0..{fh} with some opacity")),
            );
        }
        if let Some((ratio, t, dark_bg)) = contrast[i]
            && ratio < MIN_CONTRAST
        {
            out.add(
                Severity::Warning,
                "low_contrast",
                &format!("{lp}.fill"),
                Some((s, &l.id)),
                Some(t),
                format!("contrast {ratio:.2}:1 against what is behind it (at least {MIN_CONTRAST}:1 reads)"),
                Some(if dark_bg {
                    "use a light fill such as #ffffff, or darken what is behind it".into()
                } else {
                    "use a dark fill such as #101014, or lighten what is behind it".into()
                }),
            );
        }
    }
}

/// Text layer `i`'s contrast against the frame drawn without it and
/// everything above it, averaged over its box; and whether that is dark.
fn contrast_at(r: &mut Renderer, f: &Frame, i: usize, b: Bbox) -> Option<(f64, bool)> {
    let below = Frame {
        size: f.size,
        background: f.background,
        layers: f.layers[..i].to_vec(),
        view: None,
        effects: f.effects.clone(),
        t: f.t,
        seed: f.seed,
    };
    let (px, _) = r.draw(&below).ok()?;
    let (rw, rh) = r.size();
    let (sx, sy) = (
        f64::from(rw) / f64::from(f.size[0]),
        f64::from(rh) / f64::from(f.size[1]),
    );
    let (x0, y0) = (
        (b[0] * sx).floor().max(0.) as usize,
        (b[1] * sy).floor().max(0.) as usize,
    );
    let x1 = ((b[2] * sx).ceil() as usize).min(usize::from(rw));
    let y1 = ((b[3] * sy).ceil() as usize).min(usize::from(rh));
    let mut sum = [0f64; 3];
    let mut count = 0f64;
    for y in y0..y1 {
        for x in x0..x1 {
            let o = (y * usize::from(rw) + x) * 4;
            for c in 0..3 {
                sum[c] += f64::from(px[o + c]) / 255.;
            }
            count += 1.;
        }
    }
    if count == 0. {
        return None;
    }
    let bg = sum.map(|v| v / count);
    let d = &f.layers[i];
    let Rgba([r8, g8, b8, a8]) = d.fill;
    let k = d.opacity * f64::from(a8) / 255.;
    let text = [r8, g8, b8].map(|v| f64::from(v) / 255.);
    let fg = std::array::from_fn(|c| bg[c] + (text[c] - bg[c]) * k);
    let (l1, l2) = (luminance(fg), luminance(bg));
    let ratio = (l1.max(l2) + 0.05) / (l1.min(l2) + 0.05);
    Some((ratio, l2 < 0.18))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEMO: &str = include_str!("../examples/demo.cut.json");
    const SHOWCASE: &str = include_str!("../examples/showcase.cut.json");
    const STAGE3D: &str = include_str!("../examples/stage3d.cut.json");
    const PLUGIN: &str = include_str!("../examples/plugin.cut.json");

    fn run(src: &str) -> Vec<Issue> {
        let mut r = Renderer::new(320, 180);
        for f in ["mark.svg", "spin.json"] {
            let bytes =
                std::fs::read(format!("{}/examples/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap();
            r.add_asset(f, &bytes).unwrap();
        }
        check(src, &mut r, &|_| true)
    }

    #[test]
    fn the_examples_have_no_warnings() {
        for src in [DEMO, SHOWCASE, STAGE3D, PLUGIN] {
            let bad: Vec<String> = run(src)
                .iter()
                .filter(|i| i.severity >= Severity::Warning)
                .map(ToString::to_string)
                .collect();
            assert!(bad.is_empty(), "{}", bad.join("\n"));
        }
    }

    #[test]
    fn planted_issues_are_found_with_paths_and_fixes() {
        let src = r##"{
          "size": [1280, 720], "fps": 30,
          "scenes": [{
            "name": "s", "duration": 2, "background": "#ffffff",
            "layers": [
              { "id": "gone", "kind": "rect", "x": 5000, "y": 360 },
              { "id": "edge", "kind": "rect", "x": 1270, "y": 360, "width": 200, "height": 200, "fill": "#ff0000" },
              { "id": "a", "kind": "text", "text": "HELLO", "x": 640, "y": 300, "fill": "#eeeeee", "opacty": 0.5 },
              { "id": "b", "kind": "text", "text": "WORLD", "x": 650, "y": 310, "fill": "#000000", "width": 40 },
              { "id": "zip", "kind": "ellipse", "x": [{"t": 0, "v": 0, "interp": "linear"}, {"t": 0.1, "v": 1280}], "y": 600 },
              { "id": "keys", "kind": "rect", "x": 100, "y": 100, "text": "no",
                "opacity": [{"t": 0, "v": 0, "out": [0.2, 3]}, {"t": 1, "v": 1}, {"t": 1.01, "v": 1}, {"t": 5, "v": 0}] },
              { "id": "pic", "kind": "image", "path": "missing.png", "x": 200, "y": 600 }
            ]
          }]
        }"##;
        let issues = check(src, &mut Renderer::new(320, 180), &|p| p != "missing.png");
        let text: Vec<String> = issues.iter().map(ToString::to_string).collect();
        let text = text.join("\n");
        let has = |code: &str, path: &str| {
            assert!(
                issues.iter().any(|i| i.code == code && i.path == path),
                "no {code} at {path}:\n{text}"
            );
        };
        has("never_visible", "scenes[0].layers[0]");
        has("clipped", "scenes[0].layers[1]");
        has("unknown_field", "scenes[0].layers[2].opacty");
        assert!(text.contains("did you mean `opacity`?"), "{text}");
        has("text_overlap", "scenes[0].layers[3]");
        has("low_contrast", "scenes[0].layers[2].fill");
        has("ignored_prop", "scenes[0].layers[3].width");
        has("fast_motion", "scenes[0].layers[4]");
        has("wrong_kind", "scenes[0].layers[5].text");
        has("overshoot", "scenes[0].layers[5].opacity[0]");
        has("duplicate_key", "scenes[0].layers[5].opacity[2]");
        has("key_outside_scene", "scenes[0].layers[5].opacity[3]");
        has("missing_asset", "scenes[0].layers[6].path");
        // The dark text on white reads fine.
        assert!(
            !issues
                .iter()
                .any(|i| i.code == "low_contrast" && i.layer.as_deref() == Some("b")),
            "{text}"
        );
        let json = serde_json::to_value(&issues).unwrap();
        assert!(json[0]["fix"].is_string(), "{json}");
    }

    #[test]
    fn typos_in_and_around_bindings_are_named() {
        let i = run(r#"{"size": [200, 100], "fps": 30,
            "variables": {"k": {"type": "number", "value": 2}},
            "scenes": [{"name": "s", "duration": 1, "layers": [{"id": "a", "kind": "rect",
                "x": {"var": "W", "mull": 0.5}, "widh": {"var": "k"},
                "y": [{"t": 0, "v": {"var": "H", "ad": 1}}]}]}]}"#);
        let text: Vec<String> = i.iter().map(ToString::to_string).collect();
        let text = text.join("\n");
        assert!(!i.iter().any(|i| i.code == "load"), "{text}");
        for (path, fix) in [
            ("scenes[0].layers[0].x.mull", "`mul`"),
            ("scenes[0].layers[0].widh", "`width`"),
            ("scenes[0].layers[0].y[0].v.ad", "`add`"),
        ] {
            let hit = i
                .iter()
                .find(|i| i.code == "unknown_field" && i.path == path)
                .unwrap_or_else(|| panic!("no unknown_field at {path}:\n{text}"));
            assert!(hit.fix.as_ref().unwrap().contains(fix), "{hit}");
        }
    }

    #[test]
    fn a_load_error_is_one_issue_with_its_path() {
        let i = run(
            r#"{"size": [10, 10], "fps": 30, "scenes": [{"name": "s", "duration": 1, "layers": [{"id": "a", "kind": "rect", "x": [{"t": 0, "v": "no"}]}]}]}"#,
        );
        assert_eq!(i.len(), 1);
        assert_eq!(i[0].code, "load");
        assert_eq!(i[0].path, "scenes[0].layers[0].x");
        assert!(i[0].message.contains("key [0]"), "{}", i[0].message);
    }

    #[test]
    fn empty_frames_and_short_scenes_are_found() {
        let i = run(r#"{"size": [100, 100], "fps": 30, "scenes": [
            {"name": "blank", "duration": 1, "layers": [{"id": "a", "kind": "rect", "x": 50, "y": 50, "opacity": [{"t": 0, "v": 1, "interp": "hold"}, {"t": 0.25, "v": 0, "interp": "hold"}, {"t": 0.5, "v": 1}]}]},
            {"name": "blip", "duration": 0.01, "layers": []}]}"#);
        let codes: Vec<(&str, &str)> = i.iter().map(|i| (i.code, i.path.as_str())).collect();
        assert!(
            codes.contains(&("empty_frame", "scenes[0].layers")),
            "{codes:?}"
        );
        assert!(
            codes.contains(&("short_scene", "scenes[1].duration")),
            "{codes:?}"
        );
        assert!(
            codes.contains(&("empty_scene", "scenes[1].layers")),
            "{codes:?}"
        );
        let e = i.iter().find(|i| i.code == "empty_frame").unwrap();
        let [a, b] = e.t.unwrap();
        assert!(a >= 0.25 && b < 0.5, "{e}");
    }

    #[test]
    fn the_schema_accepts_the_examples_and_names_what_is_wrong() {
        let schema = Project::json_schema();
        let v = jsonschema::validator_for(&schema).unwrap();
        let effects = include_str!("../examples/effects.cut.json");
        for src in [DEMO, SHOWCASE, effects, PLUGIN] {
            let doc: Value = serde_json::from_str(src).unwrap();
            let errs: Vec<String> = v.iter_errors(&doc).map(|e| e.to_string()).collect();
            assert!(errs.is_empty(), "{errs:?}");
            // And what the schema says survives a load and save unchanged.
            let p = Project::load(src).unwrap();
            let again: Value = serde_json::from_str(&p.to_json()).unwrap();
            assert!(v.is_valid(&again));
        }
        let bad: Value = serde_json::json!({"size": [10, 10], "fps": 30, "scenes": [
            {"name": "s", "duration": 1, "layers": [{"id": "a", "kind": "rect", "fill": "red"}]}]});
        assert!(!v.is_valid(&bad));
        let bad: Value = serde_json::json!({"size": [10, 10], "fps": 30, "scenes": [
            {"name": "s", "duration": 1, "layers": [{"id": "a", "kind": "blob"}]}]});
        assert!(!v.is_valid(&bad));
        // Descriptions come from the doc comments.
        assert!(
            schema["$defs"]["Key_double"]["description"]
                .as_str()
                .unwrap()
                .contains("Handles")
        );
    }
}
