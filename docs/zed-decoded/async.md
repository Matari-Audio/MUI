# Zed Decoded: Async Rust, mapped to MUI

Article date: **April 9, 2024**<br>
Article: <https://zed.dev/blog/zed-decoded-async-rust>
Series index: <https://zed.dev/blog/tagged/zed-decoded>

This report is about the first Zed Decoded article, “Async Rust”. The article is a historical
description of Zed’s macOS-oriented GPUI implementation at publication time. The MUI mapping
below also checks the current GPUI source and the current MUI checkout as observed on
2026-09-13. Current-main observations are marked as such; conclusions about MUI are explicitly
identified as recommendations or inferences.

## Decision for MUI

Carry the foreground/background boundary, cancellation, bounded-work, and deterministic-testing
ideas into the GPUI adapter. Do not add an async runtime to MUI’s geometry or layout crates.
Those crates already produce synchronous, validated snapshots. The host should use GPUI’s
foreground context for entity/window work, GPUI’s background executor for work that is owned and
`Send`, and an explicit revision check before publishing a result.

For an audio plug-in, the audio callback remains a separate real-time contract. It must not call
GPUI, allocate UI state, resolve layout, derive themes, union paths, tessellate, or wait on a
channel. The current probe follows this boundary: GPUI and its `Rc` state live on one editor
worker, host parameter edits are drained from `Editor::idle`, and the audio path only reads its
parameter and processes samples. See [the probe boundary](../../experiments/gpui-plugin/README.md#L67)
and [the host bridge](../../experiments/gpui-plugin/src/lib.rs#L101).

The first useful async work for MUI is therefore host work such as preset/file discovery,
thumbnail or waveform preparation, device enumeration, or network-backed metadata. Geometry
preparation should remain synchronous until a measured panel exceeds its frame budget. If it is
later moved off the GPUI foreground thread, return an owned immutable result tagged with the
input revision and discard stale completions.

## The article, in a compact paraphrase

Zed demonstrates a cursor-name timeout: foreground state changes call `notify`, a task waits on
a background timer, then a weak entity is updated and notified again. The 2024 GPUI design
separates foreground and background executors on top of platform dispatch rather than adopting
Tokio or Smol wholesale. UI work stays short; potentially blocking work moves to background
threads. Historical search code uses a cheap `Arc` handoff, bounded/chunked production, and
immutable snapshots between phases. The post leaves copy-on-write structures and async
property-testing details to its companion material.

That is the complete article-only summary (about 110 words). The deeper API and testing details
below come from primary source code and current MUI files rather than extending that article’s
prose.

## Historical primary-source corroboration

The historical GPUI and Apple sources independently show the following implementation details.
They are recorded here from primary sources rather than adding more words derived from the
article page:

- The historical app source implements `cx.spawn` as a foreground task and exposes a separate
  `cx.background_executor()` for work that should not block the UI ([GPUI app at the article-linked
  revision](https://github.com/zed-industries/zed/blob/dc98b3cfa19d6bd4eae813ce7dfaf9d9e13c232c/crates/gpui/src/app.rs)).
- The foreground executor uses `async_task::spawn_local` and hands its runnable to a platform
  dispatcher; the historical macOS dispatcher routes foreground work to the main queue and
  background work to a global queue ([dispatcher source](https://github.com/zed-industries/zed/blob/dc98b3cfa19d6bd4eae813ce7dfaf9d9e13c232c/crates/gpui/src/platform/mac/dispatcher.rs),
  [`Runnable`](https://docs.rs/async-task/latest/async_task/struct.Runnable.html), and [Apple
  dispatch documentation](https://developer.apple.com/documentation/DISPATCH)).
- The dispatcher explains the frame constraint: rendering, input, and OS communication share the
  foreground thread, so a synchronous section of a task still consumes that thread’s budget.

The “macOS is the async runtime” phrasing is useful as a historical explanation of one backend,
not as an MUI API requirement. GPUI’s dispatcher is the abstraction to depend on. A plug-in host
may be Linux, Windows, macOS, a reparented editor window, or a test dispatcher; MUI should not
embed assumptions about GCD queues or a particular top-level event loop.

## Current GPUI: what changed since April 2024

Current Zed main still exposes both executor classes, but the internals are more layered than the
2024 `async_task::Task` model. `BackgroundExecutor` wraps
`scheduler::BackgroundExecutor`; `ForegroundExecutor` wraps a scheduler local executor and is
explicitly non-`Send`. Both receive a `PlatformDispatcher`. Background tasks require a `Send +
'static` future and result; foreground tasks are local and run on the main/UI thread.

The current `App::spawn` converts the app into an `AsyncApp` and schedules the closure on the
foreground executor. `AppContext::background_spawn` is the direct convenience method for a
`Send` background future. The public executor still has `background_executor()` and
`foreground_executor()` handles, so code written against the article’s conceptual split remains
valid, but an integration should verify the exact API of its pinned GPUI revision.

Current executor source also exposes `spawn_when_idle`, test clock control, deterministic
`run_until_parked`, task ticking, random-delay simulation, and scoped background work. These are
useful additions for a plug-in host, but they do not make arbitrary work safe on the audio thread.
The current scheduler even has a `RealtimeAudio` priority. Treat that as a GPUI scheduler feature
for code whose owner explicitly controls that real-time contract; it is not a replacement for a
CLAP/VST process callback and should not be introduced into MUI by analogy.

Current Zed’s own glossary makes the threading rule explicit: `App` is not `Send`, entities and
UI rendering run on one foreground thread, and `AsyncApp` is also not `Send`. Background work
must return data that can cross the boundary. A `Task` starts when scheduled; dropping it cancels
the work, while awaiting, detaching, or retaining it changes the lifetime policy. This is the
piece MUI should copy carefully: keep a task field when closing an editor must cancel work, use a
weak entity when a completion may race destruction, and detach only work that is intentionally
independent of the view.

The current GPUI rules also recommend the GPUI executor’s timer in tests rather than
`smol::Timer` when the test is driven by GPUI’s scheduler. That prevents a timer from becoming
invisible to `run_until_parked`.

Primary sources for these current facts:

- [GPUI `App` and `AppContext` source](https://raw.githubusercontent.com/zed-industries/zed/main/crates/gpui/src/app.rs), especially `App::spawn`, `background_spawn`, and executor accessors.
- [GPUI executor source](https://raw.githubusercontent.com/zed-industries/zed/main/crates/gpui/src/executor.rs), especially `BackgroundExecutor`, `ForegroundExecutor`, timers, cancellation, and test controls.
- [Zed’s GPUI glossary](https://github.com/zed-industries/zed/blob/main/docs/src/development/glossary.md), for `App`, `AsyncApp`, `Entity`, `Task`, and thread ownership.
- [Zed’s current development rules](https://raw.githubusercontent.com/zed-industries/zed/main/.rules), for the foreground/background pattern, cancellation choices, and executor timers in tests.
- [Current GPUI manifest](https://github.com/zed-industries/zed/blob/main/crates/gpui/Cargo.toml), which shows the current scheduler and async-task dependencies and the Apache-2.0 GPUI package.

The historical links used by the article are still valuable for understanding the 2024 code path:
[GPUI `app.rs` at the article-linked revision](https://github.com/zed-industries/zed/blob/dc98b3cfa19d6bd4eae813ce7dfaf9d9e13c232c/crates/gpui/src/app.rs),
[the historical macOS dispatcher](https://github.com/zed-industries/zed/blob/dc98b3cfa19d6bd4eae813ce7dfaf9d9e13c232c/crates/gpui/src/platform/mac/dispatcher.rs),
[`async_task::Task`](https://docs.rs/async-task/4.7.0/async_task/struct.Task.html), and
[`async_task::Runnable`](https://docs.rs/async-task/latest/async_task/struct.Runnable.html).
The OS primitives are documented by Apple in [`dispatch_async_f`](https://developer.apple.com/documentation/dispatch/1452834-dispatch_async_f),
[`Dispatch`](https://developer.apple.com/documentation/DISPATCH), and
[`dispatch_get_global_queue`](https://developer.apple.com/documentation/dispatch/1452927-dispatch_get_global_queue?language=objc).

## MUI’s current path and its async boundary

The current checkout is not a GPUI-only core. The reusable MUI crates are renderer-independent;
the GPUI integration lives in `experiments/gpui-plugin`, while the older egui adapter and other
rendering experiments remain separate. MUI owns the item API, measurement contract, layout,
surface dependency resolution, merged outlines, styles, view transforms, clipping metadata, and
shape-aware picking. GPUI owns the native window, text shaping and glyph painting, focus/actions,
scroll containers, frame scheduling, and host event dispatch.

The current synchronous flow is:

```text
GPUI prepaint/render callback
    -> build MUI Ui for the current editor state
    -> resolve_with_baseline using GPUI font metrics
    -> Taffy-backed MUI layout + MUI surface/fillet/offset resolution
    -> one ResolvedScene/View snapshot with scroll, transforms, clips, and hit policy
    -> GPUI path fills, native text, content masks, and native interaction elements
    -> GPUI actions / MUI tap and wheel policy / host parameter bridge
```

`Ui::resolve_with_baseline` accepts the host’s text measurement callback and forwards it into
`resolve_scene_measured_with_baseline`. The latter resolves layout and surfaces together, so text
metrics and connected geometry are from the same snapshot. The GPUI panel calls this synchronously
with `window.text_system()`, calculates a baseline for mixed-size labels, flattens MUI paths at a
scale-aware tolerance, and shapes the final glyphs through GPUI. See
[`Ui::resolve_with_baseline`](../../crates/mui-core/src/item.rs#L446),
[`resolve_scene_measured_with_baseline`](../../crates/mui-core/src/scene.rs#L688),
and [the panel bridge](../../experiments/gpui-plugin/src/panel.rs#L226).

The view layer is already a useful snapshot boundary. `ViewState::scroll_at` applies a wheel
delta to the deepest scrollable item, clamps to MUI’s computed limit, bubbles the remainder to
ancestors, and requires a fresh view after mutation. GPUI’s `ScrollHandle`s are updated from that
state; a second wheel-propagation algorithm is not needed. See
[`ViewState::scroll_at`](../../crates/mui-core/src/view.rs#L151) and
[the current GPUI scroll adapter](../../experiments/gpui-plugin/src/panel.rs#L321).

Geometry preparation is likewise already transactional. `SceneState::commit_measured` publishes
the new scene only after resolution succeeds and increments a revision only on success. The egui
adapter’s `SurfaceState` follows the same rule for union, fillet, inset, band, and tessellation;
its test verifies that an invalid commit keeps the previous result. `PathMeshCache` reuses a mesh
when path and tolerance are unchanged. These are the right primitives for async publication if a
future profile proves geometry needs a worker: compute an owned candidate, compare its input
revision, then publish on the foreground thread. See [`SceneState`](../../crates/mui-core/src/scene.rs#L713)
and [`SurfaceState`/`PathMeshCache`](../../crates/mui-egui/src/lib.rs#L74).

The geometry itself should stay MUI-owned. MUI unions sharp bases before applying convex and
concave fillets, then offsets the final rendered path for geometric insets. GPUI’s ordinary
rounded quads can cover simple rectangles, but they cannot replace the merged concave outline.
The adapter should submit that outline once to GPUI’s path builder, preserve nonzero winding, and
reuse the resulting prepared path while only paint values or parameter readouts change. This is
consistent with the existing GPUI rendering map: native quads for simple boxes and dots, MUI
paths for connected concave shells.

## Mapping the primary-source patterns to MUI

| Article pattern | MUI use | Boundary or limit |
| --- | --- | --- |
| Foreground executor for entity/window work | Build or replace `Ui`, resolve text/layout, update `ViewState`, call `cx.notify`, and submit GPUI elements | Any synchronous work in the foreground task counts against the frame budget |
| Background executor for blocking work | Preset discovery, file I/O, device queries, offline waveform/thumbnail generation, or CPU-only preparation of owned data | A background future must be `Send`; it cannot hold `Window`, `App`, GPUI entities, or borrowed MUI state |
| Weak entity update after `await` | Publish a completed preset or derived view model and notify the editor if the entity still exists | Update can fail after close; treat that as normal cancellation, not a panic |
| Bounded channels and chunking | Stream directory results or host metadata in bounded batches; coalesce intermediate updates | Never let a worker queue unbounded UI or parameter events |
| Copy-on-write/editor snapshots | Pass immutable MUI specs, parameter descriptors, or precomputed geometry inputs across a worker boundary | MUI’s layout snapshot is analogous; Zed’s rope/buffer data structures themselves are editor-only |
| Timer task for temporary state | Delayed tooltip, transient “saved” indicator, or nonessential animation in a GPUI entity | Prefer GPUI’s timer and honor reduced motion; do not use a timer to drive DSP or a high-rate UI poll |
| Property-testing async code | Test parameter gesture lifetimes, cancellation, stale revisions, and channel closure with a deterministic executor | The linked talk does not provide the implementation; start with the smaller contracts below |
| macOS GCD dispatcher | Let the pinned GPUI platform supply dispatch | Do not call GCD, Tokio, or Smol directly from MUI core to solve a host-runtime problem |
| Multi-phase async pipeline | Use the same phase separation for expensive host data: background fetch, foreground snapshot, background transform, foreground publish | `BufferSnapshot`, `ModelContext`, and project-search chunk sizes are editor machinery, not MUI APIs |

The high-value reuse is the ownership pattern, not the editor’s data structures. MUI’s immutable
scene and geometry inputs are a better cross-thread unit than a live GPUI entity. A completion should
carry `{input_revision, result}`; the foreground callback accepts it only when the revision still
matches the current editor/document state.

## Rust patterns worth adopting

### Keep async closures narrow

Clone or snapshot only what must cross the boundary before spawning. Keep lock acquisition out of
the foreground handoff; the historical GPUI code also separates the cheap handoff from background
locking ([article-linked editor source](https://github.com/zed-industries/zed/blob/98ddefc8884d0957ab766b3aea09265c8423684e/crates/editor/src/editor.rs)).
For MUI, build a compact owned request containing
parameter IDs, normalized values, theme revision, viewport size, and the geometry inputs needed by
the worker. Do not capture `&mut Window`, `&mut App`, a `Context<T>`, a `Path` borrowed from a
mutable scene, or a lock whose guard may cross an await point.

The foreground side should do the smallest possible state transition: validate that the request
is still current, replace the immutable result, and notify. It should not re-run a long geometry
pipeline merely because a background task has completed.

### Make lifetime policy visible

Use a task field for work tied to a panel or document; dropping the field should cancel it when
the panel closes. Detach only work that has an independent owner. For results that update an
entity, keep a weak handle and accept a failed update after close. For errors, use GPUI’s
`detach_and_log_err` pattern or an equivalent one so a detached task does not turn a failed preset
load into silent stale UI.

MUI’s transactional revisions give the same lifetime discipline to geometry. A theme change,
resize, port-count change, or merged-shape edit increments the relevant revision. A result prepared
for revision 12 cannot overwrite revision 13, even if it completes later.

### Bound producer and consumer rates

The search example’s bounded channel is directly applicable to preset scanners and asset loaders.
Use a small bounded queue, stop producing once the consumer has enough entries, and coalesce
updates when only the latest value matters. For a continuous parameter drag, the audio/host bridge
must have its own real-time-safe transport; do not route every pointer sample through an async
channel. The UI may display the latest local value while the host bridge applies a bounded,
validated stream of begin/value/end edits.

### Treat blocking as a classification problem

“Async” does not automatically mean “background”. A future that performs a large synchronous MUI
union before its first await still blocks the executor that polls it. Conversely, a short atomic
read or revision check belongs on the foreground side. Classify each operation by worst-case
duration and blocking behavior, then profile the real panel.

Geometry is especially easy to misclassify: filleting has a conservative clearance scan, path
flattening has segment budgets, and text measurement may reflow. These operations have explicit
limits and should be instrumented before being moved. If a worker is justified, split the work at
an owned snapshot boundary rather than moving a live GPUI element tree to another thread.

## Testing and performance lessons for MUI

### Deterministic async tests

Use GPUI’s test-support dispatcher for a GPUI adapter. Advance executor time explicitly, run until
parked, and inject seeded random delays when testing completion order. Use a GPUI executor timer
for timeouts that the GPUI scheduler must observe. Assert all of the following:

1. A successful result publishes exactly once and notifies the entity.
2. A dropped task cannot mutate a closed panel.
3. A stale revision cannot replace a newer scene, style, or parameter descriptor.
4. A bounded producer stops when the consumer drops or reaches its limit.
5. Error completion leaves the previous valid scene and revision intact.
6. A parameter gesture produces one begin/value/end sequence, including cancellation and close.

MUI already has the synchronous half of these contracts: scene and surface commit tests preserve
the previous snapshot on failure; cache tests prove unchanged geometry avoids re-tessellation;
view tests cover shape-aware picking and scroll remainder propagation; and the GPUI probe checks
resize, clipping, focus, text editing, two instances, and close/reopen behavior. Add async tests
only around the host boundary that actually introduces tasks.

The property-testing talk linked by the series is a useful follow-up for randomized completion
order and cancellation. It should supplement, not replace, direct contracts for the MUI revision
and audio gesture invariants. [Property-testing async Rust talk](https://www.youtube.com/watch?v=ms8zKpS_dZE).

### Measure the whole frame

A 120 Hz display gives an approximately 8.33 ms frame budget. That budget covers GPUI dispatch,
MUI resolution, text shaping, path conversion, scene encoding, GPU work, and presentation. A
background task can protect the foreground thread while still causing visible latency if
publication triggers too much layout or if results arrive in an unbounded burst.

Measure separately:

- MUI item construction and layout resolution;
- host font measurement and final shaping;
- union/fillet/offset and flattening;
- GPUI path conversion and scene submission;
- presentation and GPU waits;
- audio callback duration and underruns.

The current composition probe already demonstrates why caches matter: static shell geometry is
cached while values and overlays remain live, and the measured CPU preparation dropped sharply in
the local benchmark. Treat that as a CPU-preparation result, not GPU timing. The render lab also
warns that its presentation-inclusive numbers are not proof of end-to-end DAW latency.

Cache keys should include every input that affects geometry: layout revision, viewport/scale,
port count, corner/offset profile, flatten tolerance, and any shape parameter. Paint-only changes
such as hover color or a displayed value should reuse the path. A theme change that alters radius,
contrast, or stroke must invalidate the relevant prepared path; a text value change should not
invalidate unrelated merged shells.

### Rendering gate before audio/plugin work

Rendering and native interaction remain the gate before expanding the audio/plugin plan. The
repository records a 2026-09-13 native interaction check passing on the RX 6600, but the separate
OS/XTest embedded harness failed wheel assertions on two desktop runs: first scroll reset, then
wheel-movement accounting. That unresolved native-wheel result is recorded in the [GPUI probe
README](../../experiments/gpui-plugin/README.md#L261); it is not a check run for this report. A
restricted-viewport 1.5× interaction run also failed source re-arming in the same historical
record ([README](../../experiments/gpui-plugin/README.md#L292)). Resolve those rendering/input
contracts and keep one authoritative MUI/GPUI scroll path before spending effort on new plugin
async services.

### Recorded validation and audio-specific proof

The repository records a CLAP validation run dated 2026-09-13 in the [probe
README](../../experiments/gpui-plugin/README.md#L1) and its [validator output](../../experiments/gpui-plugin/results/clap-validation.json).
It reported 34 passed, 7 skipped, and 3 failed. The failures are the three
state-reproducibility cases: restored parameter values change without a host rescan notification
because the published Truce wrapper’s `state_load` does not notify. This is historical validation
evidence and was not rerun for this report; keep it visible as a migration blocker. The probe
proves an editor contract and a gain DSP slice; it does not prove a DAW’s GUI extension,
automation, multiple-host behavior, or a full KURV synth.

For a real plug-in, test UI close during a gesture, host automation while the editor is closed,
state restore before processing, sample-rate/block-size changes, multiple instances, and audio
underruns. The async executor must never become a hidden dependency of the process callback.

## Pitfalls and non-applicable machinery

- **Foreground `spawn` is not a worker.** Any synchronous section before or after an await runs on
  the foreground thread. Keep path preparation and text reflow bounded or explicitly move owned
  work to `background_spawn`.
- **Do not depend on GCD.** The historical macOS dispatcher is an implementation detail
  ([dispatcher source](https://github.com/zed-industries/zed/blob/dc98b3cfa19d6bd4eae813ce7dfaf9d9e13c232c/crates/gpui/src/platform/mac/dispatcher.rs)). Linux,
  Windows, WebAssembly, test dispatchers, and a reparented plug-in editor have different wakeup
  behavior.
- **Do not move `App` or `Window` across threads.** Current GPUI documents these as foreground-owned
  and non-`Send`. MUI’s GPUI adapter must perform text measurement, element construction, entity
  updates, and painting on that worker.
- **Do not hold a lock across await.** Acquire locks only in the worker that owns the blocking
  operation. Prefer immutable snapshots or atomics for parameter/readout state; a mutex in the UI
  path can turn a host stall into a dropped frame.
- **Do not confuse cancellation with completion.** Dropping a `Task` cancels it. A detached task
  may outlive the panel. Keep the policy in the owning entity and use weak updates for teardown.
- **Do not let stale geometry win.** A resize or theme update can finish after an older worker
  result. Compare revisions on the foreground thread and retain the last valid published scene.
- **Do not add an editor-style rope or project-search layer.** Copy-on-write buffer snapshots,
  `ModelContext`, and file-search chunk sizes solve Zed’s editor workload. MUI needs immutable UI
  descriptions and prepared geometry, not those structures.
- **Do not duplicate host services.** GPUI already supplies focus, actions, scrolling, text input,
  popovers, drag/drop, and scheduling. MUI should provide geometry, layout, styles, semantic IDs,
  and the host’s parameter-specific gesture policy.
- **Do not add a second renderer for async reasons.** The current GPUI plan uses native GPUI
  primitives for simple boxes/text and MUI paths for merged concave outlines. Vello remains a
  comparison experiment; swapping renderers would add composition, synchronization, clipping,
  and measurement work that async does not solve.
- **Do not assume historical API names are pinned API names.** The 2024 GPUI source uses
  `async_task::Task`; current GPUI’s executor wraps scheduler tasks and adds priorities, scoped
  work, idle scheduling, and deterministic controls. Pin GPUI and compile against that revision.
- **Do not pull in Zed’s whole editor UI crate casually.** Zed’s styled UI components depend on
  editor theme/icon/menu packages and have a different license boundary. Reuse GPUI primitives;
  evaluate a compatible unstyled component package separately. See
  [the existing MUI reuse decision](../GPUI-REUSE.md#L55).

## Recommended implementation order

Complete the [rendering-quality gate](../GPUI-RENDERING-MAP.md) first. The following
items preserve architectural constraints and guide later plugin work.

### P0 — pass the rendering and input gate

1. Resolve the failed native-wheel assertions in the OS/XTest embedded harness and the
   restricted-viewport source re-arming failure. Re-run the real rendered-frame interaction
   checks on the target GPU and record the exact environment.
2. Keep GPUI as the runtime: use native text, quads, SVGs, clipping, and supported effects;
   retain one MUI adapter for merged concave geometry and one scroll/pick snapshot.
3. Do not advance new audio/plugin async services until the rendering and host-input contracts
   pass on the supported harnesses.

### P1 — preserve the current async boundary

1. Keep `mui-layout`, `mui-core`, `mui-geometry`, and `mui-text` synchronous and GPUI-free.
2. Keep all MUI scene/surface commits on the editor/UI worker and retain transactional revision
   behavior.
3. Keep the audio callback limited to real-time-safe parameter/DSP operations. Continue using the
   existing host bridge for begin/value/end gestures.
4. Document one owner for each state: GPUI entity for view state, MUI snapshot for layout and
   geometry, host bridge for parameter transport, and the audio processor for DSP state.

### P2 — add async only where a concrete host need exists

1. Start with one preset/file operation in the GPUI probe. Use the pinned GPUI background executor
   and a weak entity completion.
2. Store the task in the owning entity so close cancels it. Return an owned result carrying an
   input revision and explicit error.
3. Use a bounded channel only for a stream of results. For latest-value work, coalesce instead of
   queueing every intermediate update.
4. Add deterministic tests for timer, close, error, stale revision, and bounded completion order.

### P3 — move geometry only after measurement

If a release-build profile shows MUI union/fillet/flattening or text preparation consuming a
material part of the frame, snapshot the immutable inputs and prepare off-thread. Publish only on
the foreground executor after checking viewport, theme, geometry, and document revisions. Keep
the last valid `ResolvedScene` visible while preparation runs; never expose a partially prepared
outline.

### P4 — profile the real editor and host before resuming plugin work

Measure cold open, resize, hover, continuous parameter drag, route changes, preset load, close,
reopen, and host automation. Record frame percentiles, allocations, GPU/presentation waits, and
audio underruns separately. Only then decide whether `spawn_when_idle`, a worker pool, or a more
specialized geometry cache is justified.

## Source ledger

The following URLs are the sources used for this report. The first is the requested article; the
others are primary code/documentation sources used to distinguish historical explanation from
current implementation.

1. Requested article, **April 9, 2024**: <https://zed.dev/blog/zed-decoded-async-rust>
2. Zed Decoded index: <https://zed.dev/blog/tagged/zed-decoded>
3. Current GPUI app and context: <https://raw.githubusercontent.com/zed-industries/zed/main/crates/gpui/src/app.rs>
4. Current GPUI executors and deterministic controls: <https://raw.githubusercontent.com/zed-industries/zed/main/crates/gpui/src/executor.rs>
5. Current GPUI concurrency/testing rules: <https://raw.githubusercontent.com/zed-industries/zed/main/.rules>
6. Current GPUI glossary: <https://github.com/zed-industries/zed/blob/main/docs/src/development/glossary.md>
7. Current GPUI manifest/license/dependencies: <https://github.com/zed-industries/zed/blob/main/crates/gpui/Cargo.toml>
8. Article-linked GPUI app revision: <https://github.com/zed-industries/zed/blob/dc98b3cfa19d6bd4eae813ce7dfaf9d9e13c232c/crates/gpui/src/app.rs>
9. Article-linked macOS dispatcher revision: <https://github.com/zed-industries/zed/blob/dc98b3cfa19d6bd4eae813ce7dfaf9d9e13c232c/crates/gpui/src/platform/mac/dispatcher.rs>
10. Async task primitive: <https://docs.rs/async-task/4.7.0/async_task/struct.Task.html>
11. Async runnable primitive: <https://docs.rs/async-task/latest/async_task/struct.Runnable.html>
12. Apple `dispatch_async_f`: <https://developer.apple.com/documentation/dispatch/1452834-dispatch_async_f>
13. Apple Dispatch overview: <https://developer.apple.com/documentation/DISPATCH>
14. Apple global queue: <https://developer.apple.com/documentation/dispatch/1452927-dispatch_get_global_queue?language=objc>
15. Async Rust property-testing talk linked by the article: <https://www.youtube.com/watch?v=ms8zKpS_dZE>

Local MUI evidence used for the mapping includes [README.md](../../README.md#L1),
[GPUI reuse decisions](../GPUI-REUSE.md#L1),
[the GPUI rendering map](../GPUI-RENDERING-MAP.md#L1),
[the GPUI plugin probe README](../../experiments/gpui-plugin/README.md#L1),
and the source links cited in the body.
