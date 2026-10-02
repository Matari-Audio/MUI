//! Any plugin's editor behind the live protocol, with no adapter code in
//! the plugin: mui-cut's generated adapter claims headless editors
//! ([`mui::host::headless`]), has the plugin's framework open its editor as
//! a host would, and hands the parked view to [`run_headless`].
use mui::host::View;
use mui::host::headless::Headless;
use mui::prelude::Size;
use serde_json::{Value, json};

use crate::{
    BLOCK_FRAMES, CaptureStream, Editor, Manual, NoteEvent, Vector, VectorStream, advance_notes,
    note_event, run_live,
};

/// Captured at twice the editor's points, so a part filmed up close stays
/// sharp.
const SCALE: f64 = 2.;

/// Serve `view` (see [`mui::host::headless::take`]) until stdin closes.
/// `edit` applies the host's `set` commands ([`param_set`] reads them).
/// No audio: the plugin's DSP is not running.
pub fn run_headless(
    describe: Value,
    view: Headless,
    edit: impl FnMut(&Value) -> Result<(), String> + 'static,
) -> Result<(), String> {
    run_headless_with(describe, view, edit, |_, _| {}, || Value::Null)
}

/// [`run_headless`] with the plugin's DSP running: `audio` renders the
/// host's notes (see [`run_live`]), and `patch` is the snapshot's `patch`
/// (`null` leaves it out). The editor lays out at the host's `view` size
/// when it sends one, as a window resize would.
pub fn run_headless_with(
    describe: Value,
    view: Headless,
    edit: impl FnMut(&Value) -> Result<(), String> + 'static,
    audio: impl FnMut(&[NoteEvent], &mut [[f32; 2]]) + Send + 'static,
    mut patch: impl FnMut() -> Value + 'static,
) -> Result<(), String> {
    // mui-cut built this adapter to run its commands here, the editor
    // drawn straight from its scene: no wire, no pixels.
    #[cfg(feature = "cut")]
    if std::env::var_os(mui_cut::inproc::ENV).is_some() {
        return mui_cut::inproc::run(Box::new(Live::new(view, edit, audio, patch)));
    }
    let Headless {
        ui,
        mut view,
        size: window,
    } = view;
    let mut editor = Editor::new(ui);
    let mut capture = CaptureStream::default();
    let mut size = window;
    let frame = move |_rev: u64, clock: u64, inputs: &[Value]| {
        draw(&mut editor, &mut view, window, &mut size, inputs, clock)?;
        let scene = editor.ui.scene().ok_or("no scene yet")?;
        let roots = editor.roots(size.width, size.height);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "an editor is a few thousand points"
        )]
        let (w, h) = ((size.width * SCALE) as u16, (size.height * SCALE) as u16);
        let mut value = capture.frame(scene, w, h, SCALE, &roots)?;
        let patch = patch();
        if !patch.is_null() {
            value["patch"] = patch;
        }
        Ok(value)
    };
    run_live(describe, audio, edit, frame)
}

/// One editor frame at sample `clock` with `inputs`: the tree lays out at
/// the window's size over the view's zoom (a design size fitted to the
/// window), as a window would; `size` is what it laid out at.
fn draw(
    editor: &mut Editor,
    view: &mut Box<dyn View + Send>,
    window: Size,
    size: &mut Size,
    inputs: &[Value],
    clock: u64,
) -> Result<(), String> {
    editor.advance(inputs, clock, |ui, input, _dt, sizes| {
        let window = Editor::viewport(sizes, window);
        let zoom = view.zoom(window);
        let zoom = if zoom.is_finite() && zoom > 0. {
            zoom
        } else {
            1.
        };
        *size = Size::new(window.width / zoom, window.height / zoom);
        let tree = view.build(ui, &input);
        // Every tween settled: the pixels depend on the model alone.
        ui.frame(Editor::layout(tree, sizes)?, Some(*size), input, 1.)
            .map_err(|e| format!("{e:?}"))?;
        view.after_frame(ui);
        Ok(())
    })
}

/// The headless editor and its DSP in this process, on the sample clock:
/// what [`run_headless_with`] serves over the wire, as calls. A frame is
/// the editor's paint, split into parts ([`VectorStream`]), not pixels.
pub struct Live<E, A, P> {
    editor: Editor,
    view: Box<dyn View + Send>,
    window: Size,
    size: Size,
    edit: E,
    audio: A,
    patch: P,
    clock: Manual,
    /// Notes played since the last advance: they land at its start.
    played: Vec<(Value, NoteEvent)>,
    /// Pointer and editor inputs for the next frame.
    pending: Vec<Value>,
    stream: VectorStream,
}

impl<E, A, P> Live<E, A, P>
where
    E: FnMut(&Value) -> Result<(), String>,
    A: FnMut(&[NoteEvent], &mut [[f32; 2]]),
    P: FnMut() -> Value,
{
    pub fn new(view: Headless, edit: E, audio: A, patch: P) -> Self {
        let Headless { ui, view, size } = view;
        Self {
            editor: Editor::new(ui),
            view,
            window: size,
            size,
            edit,
            audio,
            patch,
            clock: Manual::default(),
            played: Vec::new(),
            pending: Vec::new(),
            stream: VectorStream::default(),
        }
    }

    /// `{"op": "advance", "to": sample, "notes": [..]}`: run the clock to
    /// `to` and return the sound up to it, stereo interleaved, as the
    /// wire's audio packets carry it.
    pub fn advance(&mut self, command: &Value) -> Result<Vec<f32>, String> {
        let to = command["to"].as_u64().ok_or("advance needs `to`, a sample")?;
        let timed = advance_notes(command)?;
        let mut out = Vec::new();
        let mut samples = [[0.; 2]; BLOCK_FRAMES];
        let audio = &mut self.audio;
        let played = std::mem::take(&mut self.played);
        self.clock.run_to(
            to,
            played,
            timed,
            |_, n, events| {
                let s = &mut samples[..n];
                s.fill([0.; 2]);
                audio(events, s);
                out.extend(s.iter().flatten().map(|v| {
                    if v.is_finite() {
                        v.clamp(-1., 1.)
                    } else {
                        0.
                    }
                }));
                true
            },
            |_, _, _| {},
        );
        Ok(out)
    }

    /// A command as the wire takes it: a note plays at the next advance,
    /// an input waits for the next frame, anything else is the plugin's
    /// edit (a `set`).
    pub fn command(&mut self, command: &Value) -> Result<(), String> {
        match command["op"].as_str() {
            Some("note_on" | "note_off" | "panic") => {
                self.played.push((command.clone(), note_event(command)?));
            }
            Some("input") => self.pending.push(command.clone()),
            Some("snapshot") => {}
            _ => (self.edit)(command)?,
        }
        Ok(())
    }

    /// The editor now: the capture manifest (with the plugin's `patch`)
    /// and the fragments it names.
    pub fn frame(&mut self) -> Result<(Value, Vec<Vector>), String> {
        let inputs = std::mem::take(&mut self.pending);
        draw(
            &mut self.editor,
            &mut self.view,
            self.window,
            &mut self.size,
            &inputs,
            self.clock.frame,
        )?;
        let scene = self.editor.ui.scene().ok_or("no scene yet")?;
        let roots = self.editor.roots(self.size.width, self.size.height);
        let (mut manifest, vectors) = self.stream.frame(scene, self.size, &roots)?;
        let patch = (self.patch)();
        if !patch.is_null() {
            manifest["patch"] = patch;
        }
        Ok((manifest, vectors))
    }
}

#[cfg(feature = "cut")]
impl<E, A, P> mui_cut::inproc::LivePlugin for Live<E, A, P>
where
    E: FnMut(&Value) -> Result<(), String>,
    A: FnMut(&[NoteEvent], &mut [[f32; 2]]),
    P: FnMut() -> Value,
{
    fn advance(&mut self, command: &Value) -> Result<Vec<f32>, String> {
        Live::advance(self, command)
    }
    fn command(&mut self, command: &Value) -> Result<(), String> {
        Live::command(self, command)
    }
    fn frame(
        &mut self,
    ) -> Result<(Value, Vec<(String, std::sync::Arc<mui_scene::ResolvedScene>)>), String> {
        let (manifest, vectors) = Live::frame(self)?;
        Ok((
            manifest,
            vectors.into_iter().map(|v| (v.name(), v.scene)).collect(),
        ))
    }
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
