# Coordinate mapping and task execution lessons from Zed Decoded

This note applies two Zed Decoded articles to MUI’s GPUI audio-plugin path and
records where the analogy stops.

Source status:

- **Historical:** [Text Coordinate Systems](https://zed.dev/blog/zed-decoded-text-coordinate-systems), June 27, 2024, and [Syntax-Aware Task Spawning With Tree-Sitter](https://zed.dev/blog/zed-decoded-tasks), May 21, 2024. Both were reached from the [Zed Decoded index](https://zed.dev/blog/tagged/zed-decoded). The article summaries below are paraphrased and kept short; the links carry the source detail.
- **Current as checked 2026-09-13:** [Zed task documentation](https://zed.dev/docs/tasks), [task template resolution](https://github.com/zed-industries/zed/blob/main/crates/task/src/task_template.rs#L146-L284), [task scheduling](https://github.com/zed-industries/zed/blob/main/crates/workspace/src/tasks.rs#L24-L162), and [language task context](https://github.com/zed-industries/zed/blob/main/crates/language/src/task_context.rs#L10-L40). `main` is mutable; these links document the observed implementation on that date.
- **Current MUI as checked 2026-09-13:** local links point at the `feat/gpui-plugin-runtime` worktree. Recommendations labeled **inference** are design guidance derived from these sources and the existing MUI seams, not claims about Zed behavior.

## The useful rule

Keep one semantic snapshot for layout, custom geometry, painting, and picking.
Give every coordinate a named space and cross a space boundary exactly once.
Keep asynchronous work keyed by semantic identity and an explicit context or
revision, never by a stale pixel location. This is already close to MUI’s
direction; the work left is mostly making the boundaries explicit at the GPUI
adapter and host bridge.

## Coordinate systems

### What the article establishes — historical (about 135 words)

Zed separates logical text coordinates from rendered coordinates. A `Point` is
row and column; an `Offset` is a byte position suited to multiline ranges.
UTF-16 variants exist at the language-server boundary. `DisplayPoint` has the
same shape as `Point`, but its rows and columns describe the displayed map after
wrapping, folds, tabs, inlays, and blocks. A display snapshot maps it back to
buffer coordinates. An `Anchor` is a logical position tied to an immutable
insertion and a left/right bias, so edits before it do not retarget a background
operation. The transferable lesson is to name coordinate spaces and make their
conversions explicit. See the article’s linked [historical `Point`](https://github.com/zed-industries/zed/blob/dea928b00caf853b60fc19890dcb557beb814936/crates/rope/src/point.rs)
and [`Anchor`](https://github.com/zed-industries/zed/blob/dea928b00caf853b60fc19890dcb557beb814936/crates/text/src/anchor.rs)
implementations; their rope, display-map, UTF-16, and CRDT layers are editor
specific.

### Apply it to MUI — current MUI plus inference

MUI already has the important mapping chain:

| Space | Meaning | Current owner/boundary |
| --- | --- | --- |
| GPUI `Point<Pixels>` | Logical window/UI units used by pointer events and custom painting | `Window` and the GPUI adapter |
| GPUI `DevicePixels` | Physical drawable size used to configure the renderer surface; not a picking coordinate | Render-lab renderer setup ([main.rs](../../experiments/render-lab/src/main.rs#L271-L280)) |
| MUI view coordinates | Position after origin, scroll, visibility, and local affine transforms | `View::hit_item`, which inverse-transforms the point before testing the layout frame and path ([view.rs](../../crates/mui-core/src/view.rs#L216-L265)) |
| Custom geometry | The actual filled/stroked path, including concave or rounded shapes | `surface.path.contains`; GPUI path construction converts MUI geometry to `Pixels` once ([oscillator.rs](../../experiments/gpui-plugin/src/oscillator.rs#L1497-L1542)) |
| Semantic control ID | The result consumed by a gesture or host action | MUI item IDs and the plugin’s `gain` lookup ([panel.rs](../../experiments/gpui-plugin/src/panel.rs#L91-L98)) |

The hit path iterates the committed draw order in reverse, checks visibility and
viewport clipping, applies the inverse transform, checks the layout frame, and
then checks the real path. The modulation helper uses the same topmost-first
rule for its GPUI hit rectangles ([modulation.rs](../../experiments/gpui-plugin/src/modulation.rs#L284-L290)).
That is the MUI equivalent of Zed’s logical-to-display mapping: hit testing must
consume the same resolved geometry that painting consumed. Do not reconstruct a
second rectangle from control constants after layout or scrolling has changed.

**Inference for the adapter:** keep a small, explicit conversion chain such as
`GPUI window px → local content px → MUI logical point → inverse-transformed
path → semantic ID`. Use distinct Rust types or wrapper functions for window
`Pixels`, MUI geometry units, and normalized audio values. Do not multiply a
`Point<Pixels>` by DPI before `gain_hit` or `View::tap_at`; convert to
`DevicePixels` only when configuring a physical renderer surface. Scale affects
geometry tolerance at its owning boundary ([panel.rs](../../experiments/gpui-plugin/src/panel.rs#L268-L269)),
not pointer coordinates. The existing panel uses GPUI scroll handles for the
resolved MUI offsets; preserve that one-way relationship instead of adding a
second wheel or coordinate system ([panel.rs](../../experiments/gpui-plugin/src/panel.rs#L84-L100),
[GPUI reuse decisions](../GPUI-REUSE.md)).

### Stable identity

Zed’s anchor is useful as a question: “what survives while the thing moves?”
For MUI, the answer for a control is its stable item ID, not its frame, index, or
path vertex. The current `Ui` stores item metadata by string ID and a separate
paint/hit order ([item.rs](../../crates/mui-core/src/item.rs#L363-L370)); the
view returns that ID after geometric picking. Keep IDs stable across resize,
scroll, theme changes, and sibling reordering. If an async operation can outlive
a frame, pair the ID with a monotonically increasing view/state revision and
discard a completion whose revision is no longer current. **Inference:** this is
the useful subset of anchor semantics for an audio plugin.

Do not add Lamport timestamps, tombstones, bias-aware anchors, or CRDT indexing
unless MUI grows collaborative text or an editable document whose positions must
survive concurrent edits. Do not add `PointUtf16` unless an LSP or another
UTF-16 protocol is actually introduced; the [LSP 3.17 specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)
explains that boundary, but it is not an audio-parameter requirement.

## Task execution

### What the article establishes — historical (about 130 words)

The tasks article presents a small pipeline: a task template contains a command
and context variables; Zed resolves it against the current file, symbol, row,
column, selection, and worktree; and rerun can either reuse or reevaluate that
context. Local/global JSON templates and language-provided templates feed the
same picker. Tree-sitter queries tag syntax nodes as runnable, and a task with a
matching tag supplies the command. The Rust example tags test functions, but the
durable idea is the separation between syntax recognition, context resolution,
and execution. The picker, shell, terminal, and editor variables are product
specific. See the complete [historical article](https://zed.dev/blog/zed-decoded-tasks)
and its [Tree-sitter documentation](https://tree-sitter.github.io/tree-sitter/).

### Current Zed implementation — checked 2026-09-13

Current docs specify missing-variable filtering/defaults, argument quoting,
serialized versus concurrent runs, and completion/terminal behavior ([task
docs](https://zed.dev/docs/tasks)).

The current Rust implementation makes two useful boundaries concrete. A
`TaskTemplate` is not spawnable until `resolve_task` substitutes its context;
the resolved task ID hashes the template and task variables, so the same input
has a repeatable identity ([task_template.rs](https://github.com/zed-industries/zed/blob/main/crates/task/src/task_template.rs#L146-L284)).
Scheduling then resolves, records history, performs any requested save, spawns
asynchronously, and reports success, failure, spawn failure, or cancellation
([workspace/tasks.rs](https://github.com/zed-industries/zed/blob/main/crates/workspace/src/tasks.rs#L24-L162)).

### Apply it to the audio plugin — current MUI plus inference

Use the pipeline, not the editor product:

1. **Intent:** a typed Rust action such as `BeginParameterEdit`,
   `SetParameter { id, value }`, `EndParameterEdit`, `CreateRoute`, or
   `CancelGesture`.
2. **Context:** resolve the semantic control ID, current value/range, host
   instance, and view/state revision at the event boundary. Capture the context
   for a gesture; do not let a later layout pass change what a drag means.
3. **Schedule:** send the smallest command through the existing GPUI-to-host
   handoff. UI layout and path work can run on the GPUI side; DSP state and host
   callbacks stay on their required host/audio thread.
4. **Completion:** report accepted, rejected, cancelled, or stale. Apply a
   completion only if its control ID and revision still match current state.

The plugin’s existing probe demonstrates the right thread boundary: it uses a
bounded command channel and worker editor thread, while the host bridge asserts
that `begin_edit`, `set_param`, and `end_edit` execute on the host thread
([lib.rs](../../experiments/gpui-plugin/src/lib.rs#L135-L185),
[main.rs](../../experiments/gpui-plugin/src/main.rs#L35-L55)). Preserve the
event order `begin → zero or more set → end`; cancellation must close or revert
the gesture according to the host contract. The synthetic interaction check
already dispatches GPUI mouse/key events and records resulting oscillator events
([modulation.rs](../../experiments/gpui-plugin/src/modulation.rs#L1313-L1580)).

The `reevaluate_context` distinction maps cleanly to two plugin behaviors:

- A new pointer event resolves fresh coordinates and current control state.
- A repeated semantic action may either replay its captured context or resolve
  current state, but that choice must be explicit.

For parameter drags, serialize updates per control so host order is preserved.
Allow concurrency only for independent work such as background geometry
preparation or analysis, and still reject stale completions. **Inference:** this
is the audio equivalent of Zed’s `allow_concurrent_runs` and context capture.

### What to leave out

Shell commands, terminals, dirty-buffer save policies, worktree variables,
command-palette history, and language-extension task discovery solve editor
workflow problems. JSON task templates are unnecessary for fixed plugin actions;
add user-configurable automation only when a real product requirement exists.
Tree-sitter runnable queries are unnecessary for knobs, ports, modulation routes,
and custom geometry. They become relevant only if MUI ships an editable code,
patcher, or structured text surface where syntax nodes themselves are runnable.

## Testing lessons

The articles use small coordinate examples because the hard part is the mapping
contract. Current Zed tests extend that approach: template resolution rejects
blank label/command, deterministic IDs are stable for identical template/context,
workspace tests cover save policy and completion status, and task/runnable
bindings are tested at their integration seam ([template tests](https://github.com/zed-industries/zed/blob/main/crates/task/src/task_template.rs#L482-L680),
[workspace task tests](https://github.com/zed-industries/zed/blob/main/crates/workspace/src/tasks.rs#L314-L408)).

MUI already has the right testing shape:

- The demo contract resolves generated UI, checks frame alignment and tap action
  identity, and includes a deliberately unusual Unicode ID. It also verifies
  merged custom geometry ([mui-demo main](../../crates/mui-demo/src/main.rs#L40-L70)).
- `View::hit_item` exercises the real transformed path, frame, clip, and reverse
  order; retain this as the single picking seam. Geometry tests separately check
  transformed area and valid arcs ([geometry tests](../../crates/mui-geometry/src/tests.rs#L324-L347)).
- The GPUI interaction check drives synthetic platform events and observes
  semantic events, rather than asserting private widget fields.
- Render-lab fixtures verify geometry/rendering behavior across scale and backend;
  they should remain separate from host-thread gesture tests ([render-lab main](../../experiments/render-lab/src/main.rs#L209-L447)).

The minimum additional checks suggested by these sources are:

1. At 1×, 1.5×, 2×, scroll offsets, and a non-identity transform, a painted
   control and a hit at the same local point resolve to the same ID.
2. A transformed custom path rejects a point outside the path even when its
   bounding frame contains it; overlapping shapes select the last painted item.
3. Resize, reorder, and scene switch preserve semantic IDs and cannot apply a
   stale completion to a new control instance.
4. A gesture records `begin → set* → end`, host-thread assertions hold, and
   cancellation never emits a late `set` or `end` for the wrong revision.
5. Repeating an action documents whether it reuses captured context or resolves
   current state, with one test for that choice.

These checks cover the transferable Zed lessons. A rope, display map, CRDT
anchor, Tree-sitter query engine, shell runner, or editor task picker would add
surface area without covering a current MUI audio-plugin failure mode.
