# KURV → MUI migration

Source audit: `/home/derpcat/projects/KURV`, 2026-09-13. This is an editor migration, not a DSP rewrite or a pixel-for-pixel port. The real KURV checkout now has an opt-in `mui-editor` development entry point; its existing complete editor remains the default.

MUI owns graphics, layout, controls and editing gestures. Truce owns the plugin boundary, parameters, automation and persistence. KURV owns musical models, routing legality, curve evaluation and sound. `mui-truce` connects these boundaries; MUI core must not acquire KURV or Truce dependencies.

Keep the MUI oscillator shell, inset wells, variable typography, convex/concave joins and expandable pie containers. Adopt KURV's functional panel placement and responsive proportions, especially for LFOs and warps. Do not port its egui painting or rebuild its audio engines.

## Shell wiring checkpoint — 2026-09-14

The new KURV `--shell` path now supplies a `RouteOwner` to the existing MUI
pie/cable controller. Accepted routes stay in KURV, including depth parents,
legality, deletion cleanup and persisted values. UI route IDs are not reused
when a KURV route slot is recycled. The embedded runtime creates one GPUI
application per editor; the controller's application-local storage does not
own a second synth document.

The shell enumerates actual modulator slots and binds LFO/envelope controls,
add/remove/power actions, host recall and cached source-curve previews.
Group envelope previews also use the real output configuration. Non-curve
LFO modes explicitly show preview unavailability. Curve manipulation, full
group controls and warp assignment are not completed by these bindings.

Continue in this order:
1. KURV-compatible curve gestures and group envelope/output editing; complete
   collapse/reorder behavior and independently scrolling racks.
2. Real mode-specific warp controls, response graphs and warp assignments.
3. Remaining oscillator lifecycle controls, Noise and imported assets.
4. Preset browser/history/settings, then full instance/lifecycle/accessibility
   and performance checks. Keep DAW integration separate from editor-only proof.

## Migration inventory and order

A row is complete only when its real data, gestures, recall and integration checks work. Existing mock visuals do not count as completion.

| Brick | KURV source inspected | Migrate / reuse | MUI status |
| --- | --- | --- | --- |
| 1. Curve editing primitive | `editor_lfo/spline_editor.rs:33–106,354–692`; `editor_lfo/source_card.rs:255–393` | Point selection, drag, fine movement, snapping, add/remove, segment handles, shared graph well, owner edit events | **Implemented primitive:** shared cubic Bézier model/evaluator in MUI core; editable anchors/handles; optional bounded gesture undo/redo; LFO and compact unison consumers. KURV spline-format compatibility and persistence pending. |
| 2. Instance state and binding | `generators/stack.rs:185–195,463–532`; `modulators/state.rs:497–809`; existing MUI `mui-truce` contract | Bind actual stable module/parameter/source IDs; external updates; atomic accepted UI document; save/restore; remove index-based UI identity | **In progress:** real KURV VA cards bind its existing host bank, retain module/slot identity, follow host values, and use accepted enable/remove operations. Native parameter metadata is read once per card. Full recall, routing, history and other card types still need migration. |
| 3. LFO card | `editor_lfo.rs:33–39,134–362`; `editor_lfo/controls.rs:311–500,1089–1241` | Collapse/reorder/source/status/remove header; large graph with 20–30% control rail; free/retrigger/sync/one-shot; Hz/ms/beat/key; curve/random hold/random smooth/gate; phase and polarity | Bézier editor replaces LFO's preview-amount bar. Musical controls and real evaluator not bound yet. Keep MUI's left, inward-facing merged source tab. |
| 4. Envelopes and response sources | `editor_lfo/envelope_editor.rs:28–273`; `editor_lfo/controls.rs:503–551,978–1080`; `modulators/state.rs:53–63` | ADSR time/level and curve handles; reset stages; keytrack/velocity/pressure/timbre response graphs; render accepted model rather than decorative signals | Mock envelope and group ADSR need real data. Reuse graph interaction infrastructure, not a separate editor gesture implementation. |
| 5. Warps | `editor_native.rs:10–166`; `editor_filter.rs:69–198,232–452`; `editor_generator/native_processors.rs:70–111,187–299` | Real mode-aware cards and response plots; spectral family (brickwall/disperse/tilt/random/spectral kinds), phase PWM/bend/sync, VA low/high pass and curve comb/smooth; only relevant controls; add/remove and assignment depth | All three rack entries remain preview placeholders. Replace one mode at a time. Preserve merged source tabs on the right edge. Warp assignments are not interchangeable with ordinary modulation routes. |
| 6. Oscillator card | `editor_generator/oscillator_card.rs:82–162,186–234,268–531,683–830` | Keep new MUI shell; connect wave/level/tuning/phase, unison/distribution/jitter, pan/width; real wave evaluator; enable/remove/reset/reorder | Shared shell accepts owner ranges/defaults, external values and a synth-owned unison view. Real KURV entry binds eleven VA controls, supports its 200% level and logarithmic Hz rate, and plots static pitch positions using its DSP helpers. The source waveform now uses KURV’s native table selection and spline-antialiased evaluator, cached by configuration/table generation (513 samples); downstream warp and live-modulation previews remain pending. Interactive distribution and remaining tuning/pan controls remain pending; the mock Bézier is not wired into KURV detuning. |
| 7. Oscillator variants and assets | Same oscillator card; KURV `AGENTS.md` shared oscillator contract | VA, Noise, sampled/resynth variants share identity rail and lifecycle. Noise has level/pan/tilt/gaps/stereo, single lane, no pitch/phase/unison. Asset import/status/errors belong to KURV | Not migrated. Preserve source data and preset compatibility; do not add inactive controls to a generic card. |
| 8. Groups and outputs | `generators/stack.rs:29–55,541–665`; `editor_generator/group_output.rs:56–159,436–626,669–731,957–1143` | Multiple named/reorderable groups; group enable; envelope next to name; gain/pan/pitch; MIDI receive omni/1–16; output pair; dry/send/sidechain routing | The development editor enumerates real groups and their VA cards; the polished mock header is not bound yet. Keep its fillets, gradient ADSR and rounded slope while binding the actual output model. MIDI receive and audio output pairs are different concepts. |
| 9. Pies, ports and parent modulation | `editor_generator/native_processors.rs:70–111,272–299`; `editor_ports/pill_tests.rs:162–265`; existing MUI routing component | Stable route identity, parameter/input/output/parent-depth targets, legal source kinds, deletion cleanup, source-target hover cables, keyboard edits, persistent depths | Geometry and local gestures exist. Replace GPUI-global graph with per-instance accepted state; translate KURV legal assignments explicitly. Never store a visual index as an enduring source ID. |
| 10. Rack editing and macro pack | `editor_lfo.rs:134–362`; `editor_lfo/controls.rs:553–876`; `generators/stack.rs:673–1017` | Add/collapse/duplicate/reset/remove, insertion/reorder and group moves, capacity feedback; packed macros/buttons; auxiliary modules and in-group processors | Mostly missing. State operations must preserve IDs and remove orphan routes; move changes presentation order only. |
| 11. Workspace, presets and history | `editor_shell.rs:199–404,416–676`; `editor_history.rs:332–630`; `editor.rs:213–224,241–314` | Resizable warp/group/modulator columns, independent scrolling, masthead, preset browser/save/dirty state, assets, undo/redo, settings/theme/zoom, help and error feedback | Static three-column composition and live theme exist. Persist split/zoom/theme per editor instance. Reuse KURV preset/history rules instead of inventing a second preset format. |
| 12. Production checks and extraction | KURV lifecycle `editor.rs:110–238`; MUI native input matrix and `mui-truce` contract | Two instances; host updates during drag; recall while open; cancellation/close; multiple scales; clipping/focus/accessibility; idle/drag GPU cost; move validated generic GPUI components into their library target | Existing matrix/adapter tests pass before migration. Repeat for each changed component. DAW integration remains excluded. |

## Current brick: real KURV instance binding

`KURV/src/editor_mui.rs` implements the shared `EmbeddedView` runtime contract against `KurvParams`; `create_mui_editor` is exported only by the Linux `mui-editor` feature. It is a development entry point, not yet a replacement for the complete editor. No DAW integration was added.

The existing MUI probe is now a regular `rlib` dependency. Build its CLAP artifact explicitly with `cargo rustc --lib --crate-type cdylib`; a combined `rlib`/`cdylib` target produced an unhashed library collision between KURV's Rust 1.97 and MUI's Rust 1.98 in the shared target directory.

Run `python3 tools/check_mui_editor.py` from KURV for the private Weston/Xwayland check. The script clears desktop display variables, builds before opening its private display, and tests native parameter recall, resize, painting and close. The RX 6600 native check passed alongside the existing seven-case MUI input matrix; see [checkpoint evidence](../experiments/gpui-plugin/results/kurv-binding-2026-09-13/README.md). These are correctness checks, not a frame-rate benchmark.

The dependency paths currently point at the active `/home/derpcat/projects/MUI` checkout because the adjacent Windows checkout is stale; make workspace paths portable before release.

## Shared Bézier curve system

Implemented in `crates/mui-core/src/curve.rs`, exposed as `mui::curve`, with the shared GPUI editor in `experiments/gpui-plugin/src/curve_editor.rs`. See [component usage](CURVE-COMPONENTS.md).

The LFO uses the graph plus readout rail. Each oscillator embeds the same editor in compact form as its unison response shaper; the voice count controls the sample markers. Curve events are forwarded through the oscillator's existing owner event stream.

- Core has no renderer or plugin dependencies. It validates anchors and handles, evaluates x-to-y cubics, supports periodic lookup and caller-buffer sampling, and splits segments without changing their shape.
- Editor adds handle selection/dragging/reset, knot editing, fine movement, snapping, keyboard access and balanced owner events. Native tests exercise both consumers at 100/150/200% scale.
- KURV's spline format/evaluator may differ from this cubic representation. Exact compatibility, musical controls, preset persistence and DSP binding remain pending. The shaper currently edits the normalized response and samples, not audible voice detuning.

Next: finish KURV card binding and adapt its existing curve semantics explicitly, including parent routing and preset/history ownership. Do not equate a generic Bézier model with KURV preset compatibility.

## Acceptance gate for every subsequent card

1. Source-specific fields and ranges come from KURV/Truce; IDs survive reorder and recall.
2. Every visible control changes accepted model data, uses one balanced gesture, and can cancel/reset.
3. The graph represents that model. Approximate previews are identified, never implied to be playback output.
4. Save/load and two independent instances retain values, routes and presentation without leakage.
5. Existing routing, hover, focus and 100/150/200% scale checks keep passing. Record performance separately from correctness; passing input tests does not prove 60 fps.

The real editor can also be opened with `cargo run --offline --features mui-editor --example mui_editor` from KURV. This is an editor-only X11 host with native parameter state, resize and close handling; it does not run audio. Place it on the requested secondary monitor through the compositor before mapping. Automated checks continue to use only the private headless display.
