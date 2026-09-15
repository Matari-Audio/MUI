# From the composition editor to a real KURV plugin

The [main roadmap](ROADMAP.md) owns current priorities. This document details
its M1–M3 plugin work. Current scope is the reusable framework contract, using
Truce as the plugin framework. DAW integration is explicitly deferred. M0 AA,
typography and remaining intermittent input reports remain open alongside this.

The new [`mui-truce` foundation](../crates/mui-truce/README.md) reuses Truce
metadata/parameter IDs and persistence, and supplies balanced control gestures
and a validated per-instance document. The native probe consumes it; migration
of the larger composition's controls, values, routes and theme is still needed.
CLAP validation with the local Truce correction is now 37 passed / 7 skipped / 0 failed.

The [KURV migration inventory](KURV-MIGRATION.md) now tracks the source audit and
component sequence. The first curve-editing primitive is integrated into the LFO
card; musical bindings and persistence remain pending.

## Current boundary

This is partially componentized, not yet a complete reusable UI library. The reusable
MUI geometry/layout crates and the GPUI composition experiment are different layers.
The current CLAP probe processes a gain parameter; the composition editor's
oscillators, envelopes and modulation routes are not yet connected to synth DSP.

| Area | Current state | Work still needed |
| --- | --- | --- |
| Layout, paths, merged corners | Shared MUI crates | Keep the GPUI adapter independent of KURV |
| Pie/tab containers | Shared sizing, padding, growth and geometry | Public, documented component API |
| Parameters | Shared constructor attaches routing, pies, focus/actions; oscillator, group and rack controls use it | Shared descriptors, stable IDs, pointer gestures, formatting and host binding |
| Routing | Shared interaction/connection logic, parent pies and soft snapping | Document-owned graph, persistence, DSP semantics and evaluation |
| Module shells/headers | Oscillator and rack views still contain product-specific assembly | Reuse common shell/header layout without erasing module-specific content |
| Theme | Derived live tokens | Editor/document ownership rather than process/thread-local appearance state |
| Plugin runtime | Existing GPUI embedding, host edit bridge, CLAP gain probe | Mount this editor against real synth state and DSP |
| Validation | Geometry/routing tests and native interaction driver | Resolve native wheel and restricted-viewport failures; composition migration; actual DAW, automation and audio tests |

A parameter must opt into one parameter component, not manually install a marker,
pie painter, routing key handler and gesture implementation. A non-modulatable
parameter should be an explicit descriptor capability. An unconnected parameter
has routing support but does not display an unsolicited permanent route.

## 1. Establish the parameter and component contract

- Define stable `NodeId`, `ParameterId` and `RouteId`; stop using view positions as
  persistent identities. Reordering/removing modules must not retarget automation.
- Define parameter descriptors: range, default, quantization, normalization,
  display/parser, units, modulation capability and automation identity.
- Finish the shared parameter control: pointer + keyboard + reset + cancel,
  exactly one begin/change/end gesture, disabled behavior and accessibility.
  It owns modulation registration and pie interaction by default.
- Use that component for oscillator, unison, ADSR, gain/pan and module amounts.
  Keep the current preview as an example consuming the same components.
- Extract the working GPUI components/adapter into a library crate when these
  consumers establish its real API. Keep KURV module definitions outside it.

**Done when:** two different modules declare controls from descriptors without
copying gesture or pie wiring; reordering preserves values and route targets.

## 2. Give the editor one authoritative document

- Move module values, enabled states, envelope settings and routes out of view
  structs into a document owned by each editor/plugin instance.
- Use the same resolved snapshot for paint, clipping and picking; distinguish
  logical/device/local coordinates and convert only at their owning boundary.
- Keep hover, focus, animation and drag previews in transient UI state. Replace
  preview-global routing/theme ownership where it prevents instance isolation.
- Apply validated document edits and publish changes to the views. Add undo/redo
  for user edits and structural operations, with drag changes grouped together.
- Version saved state; validate ranges and references on load. Decide how module
  deletion, missing IDs and parameter-slot reuse affect existing host automation.
- If background operations are introduced, use GPUI executors, owned results,
  cancellation and identity/revision checks; stale completions cannot replace a
  newer document or mutate a closed editor. No extra async runtime is required.
- Define routing semantics before DSP: source units, bipolar/unipolar depths,
  parent-depth multiplication, input routing and explicit cycle policy.

**Done when:** save/reload reproduces the editor and routes, undo restores a
removed module with its connections, and two instances cannot alter each other.

## 3. Ship one audible CLAP slice

Use the existing plugin/editor bridge rather than inventing another runtime.

- Completed: correct Truce session/preset value rescans; keep the passing CLAP
  validator gate while integrating further modules.
- One oscillator, MIDI note on/off, ADSR, gain and pan connected to audio.
- Bind the shared descriptors to host parameters and existing begin/change/end
  edits. Host automation must update the same model the UI displays.
- Add one LFO-to-parameter route, including the parent-depth mechanism, with
  explicitly defined signal rates and smoothing.
- Keep GPUI, allocation, locks and graph construction off the audio callback.
  Reuse the plugin framework's parameter transport; publish validated routing
  snapshots at block boundaries and reclaim old data away from the audio thread.
- Restore plugin state before processing; handle sample-rate/block-size changes,
  bypass, editor close/reopen and automation while the editor is closed.

**Done when:** the editor opens through CLAP's actual GUI extension in the selected
DAW (the synthetic editor harness bypasses it), a MIDI note produces the expected waveform/envelope,
level automation and LFO modulation work, and reopening the project restores it.
This is the first real plugin milestone; more visual panels are not a substitute.

## 4. Expand KURV through the same contracts

Add unison/polyphony, multiple oscillators/groups, remaining LFO/envelope shapes,
warps and macros one at a time. Each module supplies descriptors, DSP and custom
content while reusing controls, shells, ports, pies and host binding. Validate
voice limits and modulation graph costs before raising supported module counts.

**Done when:** adding a module requires no new generic control or routing engine.

## 5. Make the plugin dependable in hosts

- Resolve the existing OS/XTest wheel failures and distinguish automation-tool
  failures from editor behavior with reproducible host checks.
- Test multiple instances, focus/keyboard/IME, drag cancellation outside the
  window, resizing, display scaling, reopening and state-version migration.
- Measure release-build DSP and UI separately: idle, parameter drag, dense routes,
  voice stress and automation. Record hardware, viewport, block size and sample
  rate; track frame percentiles, allocations and audio underruns.
- Recheck text/curves at supported scale factors, keyboard access and labels.
- Package and validate the existing CLAP target first; add other host formats
  and operating systems when they are selected as supported deliverables.

## Next implementation milestone

Migrate the KURV composition consumers onto the existing `mui-truce` contract:
Truce Params define fixed host IDs and values; the plugin's persisted Document
owns module identity, routes and appearance. Keep GPUI focus/actions/drag/drop,
text, scrolling and popup placement. Finish automatic pie binding through the
same descriptor-backed control, then extract the GPUI component layer. Do not
start DAW integration in this phase.
