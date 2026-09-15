# MUI roadmap

Audited 2026-09-13 against the current working tree, recorded validation, and the
[Zed Decoded review](zed-decoded/README.md). This includes local experimental work;
“implemented” does not mean published, merged to main, or validated in a DAW.
This is the canonical priority list. The [rendering map](GPUI-RENDERING-MAP.md) and
[KURV plugin plan](KURV-PLUGIN-PLAN.md) contain the detailed execution steps.
The [source-audited KURV migration checklist](KURV-MIGRATION.md) tracks each editor brick.

**Current position:** the renderer-independent foundation works, and the GPUI
composition experiment has reusable pieces and native interaction. We do not yet
have a complete public component library or an audible KURV instrument. The current user-authorized work is migrating KURV component by component, preserving MUI’s modern geometry and adopting KURV’s functional layouts. Truce remains the plugin framework; DAW integration is deferred. Rendering/input quality work remains open alongside it. GPUI remains the UI runtime and renderer; Vello
remains a measured alternative at the custom-graphics boundary.

## What is implemented, and what that proves

| Area | Current evidence | Boundary still open |
| --- | --- | --- |
| Core layout | Intrinsic measurement, flex/adaptive/wrapped layout, grid placement/spans, physical alignment and first baselines | Layout rebuilds per resolve; this is not incremental layout or every CSS layout feature |
| Core view/input geometry | Nested clip/scroll policies, affine view state, path picking, occlusion, inherited disabled state and wheel remainder propagation | GPUI adapter supports translated rectangular viewports; arbitrary affine native widgets and path clips are not complete |
| Geometry | Shared Boolean bases, concave/convex fillets, merged outlines and offsets; transactional failures preserve the previous scene | Broader degenerate-input fuzzing and renderer AA remain separate concerns |
| Text | Optional Parley measurement/layout for other hosts; GPUI measures and paints native horizontal text in its reference integration | Rotated labels remain outline-based; native custom `wdth` selection, rich content, bidi and real IME behavior need work/validation |
| Theme | Core transactional derivation/contrast and override preservation; composition has live neutral/OKLCH token updates | Composition theme ownership is per UI thread; complete per-instance semantic states and compositing-aware contrast are not established |
| GPUI runtime reuse | Native entities, text, focus/actions, source click/drag/drop, clipping/scroll handles, anchored/deferred popovers and reduced-motion reuse | Native facilities exist; they are not automatically a finished MUI control API or accessibility implementation |
| Components | Shared pie-container sizing/union/well, shared parameter modulation registration/actions, parent routing, soft snaps and hover cables; oscillator header/power behavior works in the preview | Descriptor/value/pointer/host behavior remains spread across views; module assembly is product-specific; APIs still live in an experiment |
| Gradients | Shared pinned-wgpu correction passes analytic sRGB/Oklab checks for paths/quads at 1×/1.5×/2× on RX 6600; grayscale MAE 50.71 → 0.25/255 | This resolves the reproduced gradient transfer defect, not all AA, shadows or color-compositing requirements |
| AA and performance evidence | Actual pie-container geometry, ADSR snapshot, native/path circles and fractional translations are in the render lab; core/preview caches and CPU profiling exist | Thin-stroke gaps persist; fixture timings are not complete-editor FPS, GPU timestamps, text benchmarks or input latency |
| Plugin proof | A CLAP gain effect, X11 child-window/GPUI pump, host value handoff and balanced automation gesture bridge | Composition oscillators/envelopes/routes are not synth DSP; the synthetic harness bypasses CLAP's GUI extension; no real DAW GUI proof |
| Verification | Core workspace checks and separate GPU/native harnesses have recorded passes | Native wheel assertions and source re-arming have intermittent failures; the corrected Truce wrapper passes CLAP validation (37 passed, 7 skipped) |

Evidence: [view contract](LAYOUT-VIEW.md), [reuse inventory](GPUI-REUSE.md),
[plugin implementation/checks](../experiments/gpui-plugin/README.md),
[latest GPU results](render-lab/rx6600-gradient-fix-2026-09-13/RESULTS.md),
[composition CPU measurements](../experiments/gpui-plugin/results/oscillator-performance.txt).

## Corrections to the previous roadmap

| Previous entry/assumption | Updated decision |
| --- | --- |
| Build the first text/painting/input reference backend | The GPUI reference exists. Complete its missing contracts and robustness; do not restart the integration |
| Scrolling/clipping and gestures are absent | Core view policies and scoped GPUI implementations exist. General component/host coverage remains partial |
| Document a font measurement contract from scratch | Baseline/measurement contracts and Parley/GPUI examples exist. Extend image/rich-content and platform coverage |
| Only egui or software-Vulkan evidence exists | GPUI runs locally on RX 6600, with separate hardware captures and measurements |
| GPUI gradients are still broken | The pinned wgpu transfer defect is corrected through common preparation; preserve the regression check |
| Tighter tessellation or native primitives guarantee AA | Neither guarantees continuous subpixel borders in the measured fixtures. Diagnose coverage separately |
| Build generic focus, events, text, popovers or virtualization | Reuse GPUI facilities; MUI supplies component semantics, geometry and host contracts |
| GPUI Base is the next mandatory step | Compatibility is unproven. Run a bounded input/slider compatibility spike only before expanding generic controls; do not block AA work on an upgrade |
| Everything missing is P0, including publication and cross-platform support | Rendering/input correctness comes first. Public packaging and additional platforms follow stable contracts and chosen support targets |
| Complete/plugin-ready because the mock looks right | Interaction and geometry proof is partial componentization. Stable identity, document ownership, persistence and DSP binding are still required |

## M0 — Rendering, input and frame reliability (current)

Completed within this milestone:

- [x] Add shared pie-container/ADSR and native/path reference fixtures, with fractional translations.
- [x] Diagnose and fix the shared sRGB/Oklab gradient transfer; retain GPU regression checks.
- [x] Reuse GPUI shaping for horizontal text, native solid dots, source drag/drop and overlay placement.
- [x] Retain stable geometry and record CPU preparation/drawing improvements with timing scope.
- [x] Fix modulation hover updates swallowing GPUI's initial drag movement. A source
  dragged directly across an existing route pie now starts a native drag; the
  regression failed before the shared capture-handler fix;
  [14/14 updated checks passed](../experiments/gpui-plugin/results/hover-drag-2026-09-13.txt). Historical wheel and
  source click/re-arming failures remain separate open issues.

- [x] Add an isolated, repeatable input matrix with explicit logical widths, DPI,
  visible-target assertions, completion checks, timeout logs and owned-server cleanup.
  [RX 6600 baseline](../experiments/gpui-plugin/results/input-matrix-2026-09-13.txt):
  14/14 checks passed; the previous intermittent failures were not reproduced or closed.
- [x] End the embedded parameter automation gesture when its window loses focus,
  using GPUI's native activation observer. Releasing and returning cannot resume it.
  Run the input matrix under headless Weston/Xwayland to exclude desktop input;
  previous rootful-window checks were not isolated from desktop focus changes.
  [21/21 headless checks passed](../experiments/gpui-plugin/results/focus-loss-2026-09-13.txt).

Remaining work, in order:

- [ ] Resolve the restricted-viewport source re-arming failure and the separate native
  OS/XTest wheel failures. Distinguish harness assumptions from actual UI defects;
  neither is cleared by a full-desktop or synthetic-event pass.
- [ ] Improve or explicitly bound subpixel-stroke AA using the actual merged contour
  fixtures. Verify seams, inner padding, clipping, layering and fractional position
  at supported scales. Keep the validated gradient fix intact.
- [ ] Audit typography as its own quality surface: measure/paint agreement, baselines,
  rotated labels, actual weight/width-axis behavior, fallback and DPI changes. Do
  not claim a custom native width axis until the pinned API or an adapter supports it.
- [ ] Profile the actual composition in release mode: cold open, warm parameter/ADSR
  drag, resize, route growth, hover cables, theme changes and multiple groups. Record
  p50/p95/p99 frame intervals, preparation/submission, cache churn and memory, and
  GPU time when supported. Separate focused, inactive and embedded-pump behavior;
  characterize idle cost and shutdown when the GPU stalls.
- [ ] Use supported native quads, SVGs and box shadows where they match the design.
  Keep one shared geometry adapter. A rounded-box shadow is not a concave-shell
  shadow, and a blurred shadow is not a general backdrop-filter API.

**Exit:** supported viewport/DPI cases pass real input and visual checks together;
remaining renderer limits and the supported border policy are explicit. A 120 Hz
target is an 8.33 ms complete-frame budget, not a promise established by draw-time
medians. Fix correctness first; compare speed only with equivalent presentation costs.
If GPUI cannot meet the chosen quality target, make a bounded Vello composition
experiment include texture transfer/sharing, clip/order, synchronization and memory.
Do not silently choose a hybrid architecture from isolated vector timings.

## M1 — Complete the reusable component and document contract


Implemented framework foundation: [`mui-truce`](../crates/mui-truce/README.md).
Truce remains authoritative for descriptors, IDs, atomic values, automation and
persistence. The crate supplies the shared gesture adapter and validated document.

- [x] Test float/discrete controls, explicit IDs across reordering, quantization,
  reset/cancel/disabled semantics and balanced host-thread automation dispatch.
- [x] Version per-instance module/route/theme/name state through Truce `#[persist]`;
  reject invalid edits/loads, cycles and ID reuse; prune dependent parent routes.
- [x] Consume the adapter in the native probe; persist name edits, recall while
  open, preserve state through editor destruction, and check instance isolation.

The following items remain full-composition integration work:

- [ ] Introduce stable semantic node/parameter/route identity independent of view
  order; define how deleted/recreated modules interact with host automation IDs.
- [ ] Consolidate parameter descriptors and the shared control: range/default,
  quantization/normalization, display/parser/units, modulation capability, pointer
  and keyboard gestures, reset/cancel, disabled behavior and host begin/change/end.
  Keep automatic modulation/pie behavior in this control, with an explicit opt-out.
- [ ] Use two different module types to establish reusable shells/headers/ports/pies
  and parameter APIs. Then extract the working adapter/components into a library;
  keep KURV-specific module definitions outside it. Avoid a speculative widget suite.
- [ ] Own persistent values, enabled states, routes, theme and group data per editor
  document. Keep hover, focus and drag previews transient. Add validated edits,
  gesture-grouped undo/redo, versioned save/load and route legality/DSP semantics.
- [ ] Keep one resolved snapshot for layout, painting, clipping and picking. Name
  logical/window/device/local/normalized spaces; convert once at each boundary.
- [ ] Define semantic appearance and accessibility together: pressed/selected/disabled/
  focus roles, readout/icon overrides, keyboard access, labels/current values,
  reduced motion and supported hit-target sizes. Verify the actual platform tree;
  role metadata and contrast checks alone do not establish accessibility.
- [ ] Make the pinned GPUI revision and local patches reproducible. Evaluate compatible
  GPUI Base input/slider behavior before writing missing generic equivalents;
  preserve the embedding/gesture tests during any dependency alignment.

**Exit:** two modules consume the same descriptor-backed controls without duplicating
pie/gesture/host wiring; reorder and save/reload preserve identity; undo restores
routes; two editor documents cannot change each other's theme or values. Public
API extraction follows these consumers rather than preceding them.

## M2 — One real KURV plugin slice

Follow [the detailed plugin plan](KURV-PLUGIN-PLAN.md): one oscillator, MIDI, ADSR,
gain/pan and one modulation route, then parent-depth semantics. Bind UI and host
automation to the same descriptors/model. Reuse the existing native bridge.

- [x] Fix session/preset value-rescan notifications in the pinned Truce wrapper.
  [Current validation](../experiments/gpui-plugin/results/framework-2026-09-13/clap-validation.json):
  37 passed, 7 skipped, 0 failed; the previous 34/7/3 result is historical.
- [ ] Deferred by user: validate through CLAP's actual GUI extension in a selected DAW, not only the
  synthetic editor contract. Exercise automation with the editor closed, project
  reload, multiple instances, resizing, focus/IME and close during a gesture.
- [ ] Define signal rates, smoothing, polyphony limits and bounded graph evaluation.
  Prepare and publish validated DSP changes at a safe boundary; reclaim away from
  the callback. GPUI, locks, allocation, layout and graph construction stay off it.
- [ ] Test sample-rate/block-size changes, MIDI timing, bypass, shutdown and error
  recovery; record audio callback/underrun evidence separately from UI timings.

**Exit:** the DAW plays the expected oscillator/envelope, automation and modulation
work, project reload restores them, and editor lifecycle does not disrupt audio.
No UI mock or GPU-only embedding result substitutes for this milestone.

## M3 — Expand and harden; then publish

Add unison/polyphony, multiple groups, remaining envelopes/LFOs, warps and macros
through the same contracts. Define supported module/route/voice counts from measured
costs. Select additional OS/host-format targets explicitly; validate parent handles,
DPI, focus, clipboard/IME, event pumping and teardown for each. X11 proof is not
Wayland/macOS/Windows support.

Before a public release, finish API examples/docs, dependency version declarations,
name availability, semver and MSRV policy, licensing/asset/patch inventory and
reproducible packaging. Keep core native/WASM/frontend checks separate from the
opt-in physical-GPU and DAW checks. `tools/verify.sh` does not run the excluded
GPUI/plugin/render-lab workspaces. Record each check's command, revision, environment,
scope and result; a single green workspace run is not a complete release gate.

## Backlog retained, with a reason to activate it

| Work | Activate when |
| --- | --- |
| Rich spans, image/SVG measurement in the core, Parley renderer/playground coverage | A selected reusable content component needs it; keep each backend's measurement and glyph placement consistent |
| Aspect ratio, per-axis gaps, richer percentage/min/max-content sizing, grid auto-fit/auto-fill, RTL and per-item placement | A real layout cannot express its needs with the existing API; use Taffy's capability before inventing a solver |
| Arbitrary affine native-widget painting and arbitrary path clips | Required by a supported component; preserve exact transformed clips rather than replacing them with bounding boxes |
| Incremental layout, cache eviction, virtualization, summary trees or bitmaps | Profiling shows meaningful tree rebuilds, churn or scans; prefer retained Taffy state/GPUI lists and test unchanged paint/pick results |
| Async preparation | A real file/preset/background operation requires it. Use GPUI executors, owned immutable input/results, cancellation and stale-revision rejection; foreground `spawn` is not background execution |
| Concave shape shadows or per-component backdrop blur | A concrete design needs them and native effects do not cover it; measure a shared mask/composition path |
| More fuzz/property cases | Expand with real boundaries: degenerate geometry, nested layouts, failed commits, stale completions, input cancellation and instance isolation |

Keep ropes, CRDTs, editor display maps, Tree-sitter task machinery, Wasm extension
hosting and a second async/runtime framework outside the plan unless a product
requirement emerges. The [Zed findings](zed-decoded/README.md) add ownership,
coordinate, cancellation and measurement discipline—not a mandate to import Zed's
editor internals.

## Next concrete task

Reproduce and fix the **restricted-viewport modulation activation and native wheel
failures**, then continue AA/typography/frame profiling with the existing fixtures.
Do not restart the now-completed gradient work or jump to DSP while the editor's
rendering/input gate is still open.
