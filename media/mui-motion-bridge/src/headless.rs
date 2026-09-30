//! Any plugin's editor behind the live protocol, with no adapter code in
//! the plugin: mui-cut's generated adapter claims headless editors
//! ([`mui::host::headless`]), has the plugin's framework open its editor as
//! a host would, and hands the parked view to [`run_headless`].
use mui::host::headless::Headless;
use serde_json::{Value, json};

use crate::{CaptureStream, Editor, run_live};

/// Captured at twice the editor's points, so a part filmed up close stays
/// sharp.
const SCALE: f64 = 2.;

/// Serve `view` (see [`mui::host::headless::take`]) until stdin closes.
/// `edit` applies the host's `set` commands ([`param_set`] reads them).
/// No audio: the plugin's DSP is not running.
pub fn run_headless(
    describe: Value,
    view: Headless,
    edit: impl FnMut(&Value) -> Result<(), String>,
) -> Result<(), String> {
    let Headless {
        ui,
        mut view,
        size,
    } = view;
    let mut editor = Editor::new(ui);
    let mut capture = CaptureStream::default();
    let frame = move |_rev: u64, clock: u64, inputs: &[Value]| {
        editor.advance(inputs, clock, |ui, input, _dt, sizes| {
            let tree = view.build(ui, &input);
            // Every tween settled: the pixels depend on the model alone.
            ui.frame(Editor::layout(tree, sizes)?, Some(size), input, 1.)
                .map_err(|e| format!("{e:?}"))?;
            view.after_frame(ui);
            Ok(())
        })?;
        let scene = editor.ui.scene().ok_or("no scene yet")?;
        let roots = editor.roots(size.width, size.height);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "an editor is a few thousand points"
        )]
        let (w, h) = (
            (size.width * SCALE) as u16,
            (size.height * SCALE) as u16,
        );
        capture.frame(scene, w, h, SCALE, &roots)
    };
    run_live(describe, |_, _| {}, edit, frame)
}

/// A parameter `set`, read: `{"op": "set", "id": .., "field": .., "value": ..}`
/// with `id` a parameter id or name (`find` maps names), and `field`
/// `norm` for 0..1 or `value`/`plain` for plain units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamSet {
    pub id: u32,
    pub value: f64,
    pub norm: bool,
}

pub fn param_set(c: &Value, find: impl Fn(&str) -> Option<u32>) -> Result<ParamSet, String> {
    if c["op"] != "set" {
        return Err("this adapter takes `set` only".into());
    }
    let id = match &c["id"] {
        Value::Number(n) => n
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or("a parameter id is a whole number")?,
        Value::String(s) => find(s).ok_or_else(|| format!("no parameter `{s}`"))?,
        _ => return Err("`id` is a parameter id or name".into()),
    };
    let norm = match c["field"].as_str().unwrap_or("value") {
        "norm" => true,
        "value" | "plain" => false,
        f => return Err(format!("field `{f}`: `norm`, `value` or `plain`")),
    };
    let value = c["value"]
        .as_f64()
        .filter(|v| v.is_finite() && (!norm || (0.0..=1.0).contains(v)))
        .ok_or("value must be a finite number (0..1 for `norm`)")?;
    Ok(ParamSet { id, value, norm })
}

/// The describe object for a generic adapter.
pub fn describe(name: &str) -> Value {
    json!({"name": name, "notes": false, "addKinds": [], "move": false, "delete": false})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_by_id_or_name_in_plain_or_normalized_units() {
        let find = |s: &str| (s == "Cutoff").then_some(7);
        let set = |v| param_set(&v, find);
        assert_eq!(
            set(json!({"op": "set", "id": 3, "field": "norm", "value": 0.5})),
            Ok(ParamSet {
                id: 3,
                value: 0.5,
                norm: true
            })
        );
        assert_eq!(
            set(json!({"op": "set", "id": "Cutoff", "value": 1200.})),
            Ok(ParamSet {
                id: 7,
                value: 1200.,
                norm: false
            })
        );
        for bad in [
            json!({"op": "set", "id": "Nope", "value": 1}),
            json!({"op": "set", "id": 1, "field": "norm", "value": 2}),
            json!({"op": "set", "id": 1, "field": "gain", "value": 0}),
            json!({"op": "add", "id": 1, "value": 0}),
        ] {
            assert!(set(bad).is_err());
        }
    }
}
