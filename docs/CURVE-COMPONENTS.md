# Reusable Bézier curve components

One normalized response model, one GPUI editor, two existing consumers: LFO and unison. The graphics library does not know what a voice, envelope stage, plugin parameter or MIDI note is.

## MUI core model

`mui::curve::{Curve, CurvePoint, CurveHandles, Handle, CubicSegment}` is renderer-independent. `Curve` holds 2–64 ordered anchors and two controls per segment. Input and output are normalized to 0–1. Handles stay horizontally ordered inside their segment, so the curve remains a single-valued function; it is not a free-form vector drawing path.

```rust
use mui::curve::{Curve, CurvePoint, Handle};

let mut response = Curve::linear();
response.move_handle(0, Handle::Outgoing, CurvePoint { phase: 0.15, value: 0.8 });

// Transfer function / envelope / response lookup: clamps outside 0–1.
let level = response.evaluate(0.4);
// LFO-style lookup: wraps input. The owner decides whether endpoint values match.
let looping = response.evaluate_wrapped(1.4);
// Unison or step-like consumer: no allocation inside sample_into.
let mut voices = [0.0; 7];
response.sample_into(&mut voices);
let detune_positions = voices.map(|v| v * 2.0 - 1.0);

// Subdivide the existing cubic without changing its shape (within float precision).
let knot = response.split(0.5).unwrap();
response.move_point(knot, 0.5, 0.7);

// Host-owned serialization can store these arrays in its existing format.
let restored = Curve::from_parts(response.points().to_vec(), response.handles().to_vec()).expect("valid snapshot");
```

`evaluate` solves the cubic's horizontal coordinate before reading its vertical coordinate. Using the input directly as Bézier t would give incorrect values whenever horizontal handles bend. Evaluation uses bounded bisection, no allocation or lock. `segment` supplies the identical cubic controls used for drawing.

`new` creates linear segments; `Default` is a smooth four-knot example; `linear` is an identity response. `insert` splits and moves the new knot to the requested position. `remove` joins neighbours using surviving outer handles and can change the shape. `reset_segment` restores a straight segment. `from_parts` rejects malformed data before replacing a live model.

A single response sample is taken at input 0.5; multiple samples include both endpoints. Empty output buffers are supported. NaN lookup chooses the first value; infinite clamped input chooses its endpoint. Nonfinite wrapped input chooses the start. Imported coordinates and edit coordinates must be finite.

## Shared GPUI editor

`CurveEditor::new(curve, cx)` provides a graph and readouts. `.compact(140.)` embeds just the graph in another panel. `.axis_labels("INPUT", "OUTPUT")` changes the readout labels. `set_samples(count, cx)` adds response markers without changing curve data.

Owners subscribe to `CurveEdit::Begin`, `Changed(Curve)`, and `End`. The oscillator forwards these as `OscillatorEvent::UnisonCurve`. `replace(curve, cx)` closes an active gesture and displays externally recalled data without echoing a Changed event. The owner must update its own accepted state when recalling; the component does not invent a save format.

- Click an anchor to select it; attached square handles appear. Drag either axis of a handle or anchor.
- Shift gives fine movement. Alt disables grid snapping.
- Double-click empty graph to add a point at the cursor; Alt-double-click subdivides at its input without moving the curve.
- Double-click an interior anchor removes it. Double-click a handle resets its segment to linear.
- `[` / `]` select anchors; `H` cycles their handles; arrows move selection; Shift-arrows use a finer step.
- Delete removes a selected interior anchor or resets a selected handle's segment. Escape rolls back an active drag, including its handles.
- Disable or window deactivation cancels an active drag. Endpoints keep their input positions.

Painting uses GPUI cubic paths and quads. No permanent timer or audio work runs in the editor. Sample markers read the same model as the path. The existing native input matrix verifies anchor and handle edits in both the LFO and compact unison placements.

## Gesture history

`mui::curve::CurveHistory` is an optional, bounded history of 32 completed curve edits. It ignores unchanged commits, clears redo after a new edit, and refuses to restore a snapshot over an unexpected externally changed curve.

The standalone GPUI editor records one entry per completed drag or discrete edit. Ctrl/Cmd-Z undoes; Ctrl/Cmd-Shift-Z or Ctrl/Cmd-Y redoes. Undo and redo emit the same balanced Begin/Changed/End events as ordinary edits, so the owner receives the restored curve. Cancelled drags are not recorded. External `replace` clears local history.

Use `CurveEditor::new(curve, cx).without_local_history()` when an enclosing plugin/document owns undo. In that mode these shortcuts are left for the parent and the component still emits ordinary edit events. This local history is not a second KURV preset or whole-patch history implementation.

## Boundaries

The model is already in MUI core. The GPUI editor still lives in the integration experiment alongside the existing theme/controls; a separate stable GPUI component crate has not been published. Core evaluation can serve a plugin, but this change does not wire it into KURV audio, claim audio-rate performance, or translate KURV's existing spline/preset format. Truce still owns plugin persistence and automation.
