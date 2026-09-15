# GPUI through Truce's plugin editor contract

Framework integration milestone, tested 2026-09-13 on the RX 6600.
This builds a real CLAP gain effect and a runnable editor-host check. It does not
replace KURV's editor or expose a stable GPUI backend crate yet.

| Area | Connected in this panel |
| --- | --- |
| Layout | MUI measures and lays out the entire panel; GPUI supplies a clipped scrolling viewport and native resize handling |
| Geometry | MUI paths are filled with the nonzero rule; hit tests reject rounded cutouts and use the same logical coordinates and flattening tolerance |
| Typography | The same GPUI font/shaping system measures intrinsic and wrapped text and paints the final glyph layouts; the editable field reuses GPUI's pinned text-input example |
| Interaction | Pointer and keyboard activation, captured horizontal dragging, Escape cancellation, Tab/Shift-Tab traversal, text selection, clipboard and host-thread automation gestures |

The reusable core also gains `Path::contains` (including holes, transformed paths,
input validation and a segment budget) and `Item::disabled`, inherited by descendants
and honored by MUI's existing picking and hover-style resolution. Disabled action
items do not pass clicks through to an underlying action item.

The X11 editor check passed resize validation, rounded-corner rejection, pointer and
Return-key activation, drag outside the control, Escape cancellation, Tab traversal,
typing/selection/copy/paste/backspace, scroll clipping and scroll limits, closing during
an active gesture, two instances and close/reopen. GPUI reported RX 6600, Mesa 26.2.2,
`software=false`; see [runtime.txt](results/runtime.txt). The full MUI workspace check
(`tools/verify.sh`: tests, Clippy, native/WASM and generated frontend fixtures) passed.

CLAP validation now reports **37 passed, 7 skipped, 0 failed**. The local
[Truce 6.3.0 correction](../../vendor/truce-clap/MUI-PATCH.md) notifies the host
when session/preset recall changes parameter values. The prior 34/7/3 report is
historical. See [current validation](results/framework-2026-09-13/clap-validation.json).

The probe consumes [`mui-truce`](../../crates/mui-truce/README.md): Truce metadata
and IDs drive a reusable parameter gesture controller and host dispatcher.
`#[persist] Document` belongs to the plugin's Params instance; preset-name edits,
host recall and editor reopening share it. The native harness checks this path.
The larger KURV composition has not yet migrated its view-owned values/routes.

## Run

Linux/X11 only. From the MUI repository root:

```sh
python experiments/gpui-plugin/prepare.py
cargo rustc --locked --manifest-path experiments/gpui-plugin/Cargo.toml --lib --crate-type cdylib
```

Use a dedicated X server because the check moves the pointer and keyboard focus.
Start `Xwayland :89 -geometry 1920x1200 -nolisten tcp` in another terminal, then:

```sh
DISPLAY=:89 GPUI_X11_SCALE_FACTOR=1 MESA_VK_DEVICE_SELECT='1002:73ff!' ZED_DEVICE_ID=0x73ff \
  cargo run --locked --manifest-path experiments/gpui-plugin/Cargo.toml
```

For repeatable input validation, run the isolated matrix (requires Weston and Xwayland):

```sh
MESA_VK_DEVICE_SELECT='1002:73ff!' ZED_DEVICE_ID=0x73ff \
  python3 experiments/gpui-plugin/tools/check_input.py
```

This builds the probe and composition example, starts a headless Weston compositor
and its own Xwayland display, and checks 1280/1480 logical widths at 1×/1.5×/2× plus the embedded XTest probe at 1×.
It repeats twice by default, requires each check's completion marker, records logs
and JSON results in a printed temporary directory, and tears down both servers.
A rootful Xwayland window on your desktop alone does not isolate physical pointer
and focus changes. The headless compositor has no physical input devices.
Use `--repeat N` and `--output PATH` to retain a longer run. Composition events are
injected through GPUI; only the embedded probe uses OS/XTest input. This does not
validate desktop compositor input, a real DAW, smaller heights, or embedded fractional DPI.
The composition sequence also drags a source across an existing route pie: hover
redraws must not consume the movement that GPUI needs to start the native drag.
The embedded sequence switches keyboard focus to a sibling editor during a held
parameter drag. Automation must end exactly once before mouse-up, and returning
the pointer must not resume the cancelled drag. The editor uses GPUI's native
window-activation observer for this lifecycle.
`MUI_CHECK_WIDTH` sets the composition example's logical width (minimum 1280).

The explicit `cargo rustc --lib --crate-type cdylib` build produces `libmui_gpui_plugin_probe.so` in Cargo's configured target
directory. Copy it to a `.clap` filename to load it as an experimental plugin;
`output/mui-gpui-probe.clap` is the locally built, debug-symbol-stripped artifact.
Run `clap-validator validate output/mui-gpui-probe.clap` from this directory.
The Cargo manifest applies the local Truce correction; keep that patch for this pinned version.

## What this proves, and what remains

GPUI's pinned Linux implementation blocks in `Platform::run`, even through
`Application::run_embedded`. `prepare.py` adds an experimental X11 event pump and
requests frames through GPUI's existing frame callback. Reparented child windows
cannot rely on top-level compositor frame wakeups; without the explicit requests,
scroll state could remain unpainted until another input event. GPUI still checks its
dirty state before drawing. Ordinary application startup remains unchanged.

Preparation reuses the renderer lab's pinned upstream revision and validates patch
anchors. It extracts the reusable portion of GPUI's `examples/input.rs`, excluding
its standalone app and reset button, and adds a constructor, Linux editing bindings,
accessible labeling and a persistent tab-stop focus handle. The generated source
stays in the ignored upstream directory; preparation is repeatable.

GPUI's application and entities live entirely on one editor worker. Host parameter
updates are read directly from Truce's shared parameter atomics; commands are bounded.
UI automation events return to `Editor::idle`. Dragging sends one begin/end pair,
including cancellation or close; the host bridge balances a gesture even if the worker
fails. There are no unsafe Send casts or UI calls in the audio callback.

The current frame pump polls roughly every 8 ms. Idle power, animation scheduling under
load and stalled-GPU shutdown are not characterized. The field is an upstream example,
not a finished MUI text-edit widget: its value is editor-local and not saved in plugin
state, and long single-line text is clipped rather than horizontally scrolled.

This is an editor-contract harness plus a CLAP binary, **not a real DAW GUI test**.
The harness bypasses CLAP's GUI extension. KURV and its vendored Truce patches are untouched.

Still needed within the first four framework areas: arbitrary affine painting of native
text/widgets, rich text, and real IME/bidirectional-text validation. Baselines and nested
MUI clip/scroll policies now drive the reference panel. Screen-reader behavior,
host-negotiated DPI changes, VST3 and other OSes remain unverified.
Parley is still available for other renderers; this backend uses GPUI text consistently
instead of mixing Parley measurements with GPUI glyph placement. No combined
GPUI/Vello compositor or duplicate widget runtime has been introduced.

## Manual panel and newer core layout APIs

Run the probe with `--manual` on your desktop display to keep one panel open until you
close its window. The title is **MUI - interactive GPUI panel**; resizing updates the child
editor, and the automated XTest sequence is skipped in this mode.

MUI now also provides [baseline and viewport APIs](../../docs/LAYOUT-VIEW.md), with a
rendered nested-scroll/transform example. This probe now connects the core view snapshot to GPUI native scroll containers, masks,
font baselines and gain picking. The wheel bridge reuses core propagation and GPUI event
normalization/scroll handles. See [GPUI reuse decisions](../../docs/GPUI-REUSE.md).

The current panel adds a larger **GPUI** header label to demonstrate mixed-font baselines,
and a nested scroll area below the text field. Scroll over that area to move it independently;
after its range is exhausted, remaining wheel movement reaches the outer panel. The runtime
check also compares painted and native scroll offsets to catch stale frames.

[GPU-readback panel preview](results/panel.png). The smoke test also renders the 320×180
minimum editor size and rejects any panel-layout error.

## Composable KURV study

```sh
cargo run --locked --manifest-path experiments/gpui-plugin/Cargo.toml --example oscillator -- --kurv
# Group-only showcase, or regenerate the shared geometry/glyph SVGs:
cargo run --locked --manifest-path experiments/gpui-plugin/Cargo.toml --example oscillator
cargo run --locked --manifest-path experiments/gpui-plugin/Cargo.toml --example oscillator -- --export
cargo test --locked --manifest-path experiments/gpui-plugin/Cargo.toml --lib oscillator_contract
```

`KurvWorkspace` composes separate `ModuleRack` and `OscillatorGroup` GPUI entities:
warps left, oscillator groups center, modulators right, masthead above, as in
KURV's `editor_shell::draw`. Its minimum width is 1280 logical pixels. Racks scroll
independently. Oscillator-row gaps are zero. Each oscillator has one merged shell with a
12-pixel inset around its waveform and unison panels; each panel has one row of
controls. The identity header and input/output tabs belong to the shell. The group border is a single MUI union/fillet path including every
input/output tab, with no border across their interiors.

Oscillators are independent components with local drag values, fine movement,
double-click reset, keyboard values, power and removal. Groups add/remove instances,
collapse, and preserve individual power states under group bypass. `GroupEvent`
forwards indexed oscillator gestures for a later plugin binding. Side racks are
explicit UI-local amount previews, not implemented filter/LFO/envelope editors.
`modulation.rs` provides the shared UI route model, source/target registration,
soft-snap cables, depth gestures, animated parameter dots/pies, and height-limited
port columns. It references the functional HTML in KURV history
(`3b14ee6f:index.html`) without copying the oscillator shell.
Click a source to arm it, then drag a target + or input hole; release preserves
modulation mode until Escape or an outside click. Drag a source directly to a
parameter/input to create a zero-depth connection. Parameter dots expand to depth
pies on hover outside modulation mode; input/output pies remain visible. Pies
float over panel edges and reserve no parameter-row space. Double-click removes a
route; Shift gives fine adjustment. Keyboard: focus a source and Enter to arm;
focus a target and Enter to connect; Alt left/right selects its routes, Alt up/down
edits depth, Alt Backspace removes. Routes are UI-local, not audio bindings.

The masthead OKLCH field updates derived tokens on every valid edit. Try
`0.65 0.12 240` or `oklch(65% 0.12 240)`. Invalid/incomplete input retains the last
valid palette. The default is achromatic. The core palette assigns lightness per
semantic role and enforces text/outline contrast after gamut mapping.
KURV source was consulted for placement and control names; its shell was not copied.

[Roboto variable font](https://github.com/google/fonts/tree/main/ofl/roboto) is
bundled with its OFL license. Horizontal live text now uses GPUI shaping and its
glyph atlas, with native weight selection. The 90° headers and SVG export retain
actual `wght`/`wdth` outlines. Native horizontal text does not expose a custom
`wdth` axis through this pinned GPUI API; it uses the font's standard width. This is UI rendering, not KURV DSP or
host automation integration. [Group SVG](../../docs/oscillator-group.svg).


### Mock rendering performance

`--bench` runs 120 changing-value frames through the same oscillator drawing
preparation used by the live preview. On this machine's debug build at width 1440,
preparation fell from 31.306 ms/frame to 1.345 ms/frame after caching static shell
geometry and bounded variable-font runs (about 23×). This measures CPU preparation,
not end-to-end GPU frame time. The interactive launcher must run without
`MUI_LAB_CAPTURE`: that diagnostic copies GPU frames back to CPU and stalls painting.
The normal preview now runs without it. `--export`/`--bench` exit after completion.


`MUI_ROUTING_PROFILE=1` requests 210 redraws (30 warm-up, 180 measured), reports
refresh cadence, then returns to demand-driven drawing. Add `--features profile`
to use GPUI's native CPU-draw/presentation histograms. This avoids GPU frame readback;
refresh cadence is not a GPU timestamp measurement. `--bench` also compares cached
and uncached group-outline preparation.

The routing preview now uses GPUI cached rack views and invalidates only the overlay
for pointer/depth animation. Anchor registration replaces existing bounds, so resized
or cached racks cannot accumulate stale hitboxes. Shell/group unions are rebuilt only
when size or port counts change. The dev profile optimizes GPUI and Taffy while keeping
the application debuggable. On the measured 1256×1382 viewport, CPU draw median fell
from 18.32 to 1.66 ms and the refresh probe rose from 39.8 to 105.9 redraws/sec.
See `results/oscillator-performance.txt` for scope and reproduction.

### Group and route interaction study

Group name opens the name/OKLCH picker. The gradient envelope sits beside it;
drag its attack, decay or release endpoint horizontally, or sustain vertically.
Only the edited stage shows its label/value; arrow keys work on focused stages.
MIDI menus select Omni or channels 1–16. These values are editor-local mock state.
The bundled regular icon font is from [Phosphor](https://github.com/phosphor-icons/web),
with its MIT license in `assets/PHOSPHOR-LICENSE`.

Theme presets and valid OKLCH edits immediately update derived surfaces, contrast
roles and accents. Neutral chroma intentionally ignores hue. Pies are opaque;
parameter dots reveal their depth on hover. Hover links matching route endpoints
with a dashed cable and highlights the source. Drag another source near a pie to
parent its depth; dashed candidate slots displace obstructing parameter pies.
Rack tabs merge into their cards and grow outward; their section labels sit faintly
in the background, following KURV's `section_body` treatment.

`pie_container::PieContainer` owns slot spacing, height-limited columns, partial
last columns, and the optional inner well. `port(slots, height, left)` packs
vertically; `parameters(slots, columns, background)` packs centered horizontal
rows. Add `outer_shapes(anchor)` to any host polygon and call
`pie_container::rounded` once for their merged fill/border. Paint `well()` above
that host with the waveform surface token. All live oscillator and rack ports
use this path; they do not construct their own tab rectangles. `slot(index)` is
also the source of the painted pie positions. Parameter backgrounds are off in
this mock, and can be requested through the same container constructor.

### Rendering quality and robustness pass

Horizontal live labels now use GPUI text shaping/rasterization. MUI arcs reach GPUI
as cubic curves rather than pre-flattened polylines; fill and 1px border share that
conversion. Full-circle modulation dots use native rounded quads, and partial
sectors use curves. `MUI_TEXT_OUTLINES=1` keeps the old horizontal text path for
local A/B comparisons; SVG export always retains outline text.

Resize notifications run on the next GPUI frame, avoiding notifications lost during
cached prepaint. Parent placement stops once no further children can be placed.
Custom pie motion respects GPUI's reduced-motion preference. Group popovers use
`anchored` and `deferred`, stay within the window, and dismiss on Escape/outside
click. Escape also restores a group control's value during an unfinished drag.

[Before](results/quality-before.png), [after](results/quality-after.png), and
[1.5× DPI with the native anchored picker](results/quality-150-percent.png).
See [measured CPU/frame results](results/oscillator-performance.txt).
`MUI_ROUTING_STRESS=1` seeds 111 valid routes, including twelve parent routes;
combine with `MUI_ROUTING_PROFILE=1` to reproduce the stress measurement.

### Native interactions

The composition shares GPUI action bindings instead of matching raw key strings:
Tab/Shift-Tab traverse controls, arrows adjust values, Enter activates a source or
connects a focused target, Alt-arrows select/edit routes, and Alt-Delete removes one.
Shift-arrows use finer increments for continuous group/rack values; oscillator
parameters retain their declared quantization step. Escape restores a dragged
value or cancels the active routing gesture. Focus has a visible highlight and
returns to the workspace when its focused child disappears.

Source cables now use GPUI typed drag/drop and native click/drag recognition.
MUI still resolves soft snaps, parent targets and route legality. Pies retain their
custom depth gestures and the retained overlay paints the cable. GPUI drag-view
release clears unfinished source gestures, including releases outside a target.

Run the native interaction check on a working GPU display:

```sh
cargo run --offline --manifest-path experiments/gpui-plugin/Cargo.toml --features profile --example oscillator -- --kurv --check-interactions
```

It drives GPUI input dispatch across actual rendered frames and asserts focus
traversal, keyboard activation, balanced oscillator begin/value/end events,
route actions, native cable drops, cancellation and empty-space drops. It exits
on success; use the embedded-editor harness separately for OS/host input checks.

2026-09-13 validation: five library contracts and local Clippy pass. The native
interaction check passes on the RX 6600. The separate OS/XTest embedded harness
failed different wheel assertions on two desktop runs (scroll reset, then wheel
movement accounting); no panel/scroll implementation changed in this integration.
Treat that OS-level scroll check as unresolved, rather than evidence that the new
interaction check covers host wheel delivery.

### Parameter composition and plugin boundary

`controls::parameter` installs the modulation marker, pie behavior and route
keyboard actions automatically. Oscillator/unison, group ADSR/gain/pan and rack
amounts use this path. The Truce-backed shared gesture/document contract now lives
in `mui-truce` and is consumed by the embedded probe. These composition views still
need migration onto it; their existing routing/appearance remains experimental.

Oscillator headers use centered, bottom-to-top labels, 12 px padding/gaps and
30 px end-control hitboxes. Their painted square uses the same center as its
button. Clicking it while routing is armed exits routing and toggles in one click.
The native interaction check covers off/on and automatic group/rack registration.

See [the real-plugin plan](../../docs/KURV-PLUGIN-PLAN.md) for the current boundary,
implementation order and acceptance criteria. The existing plugin probe is gain
DSP; the composition editor is not yet an audible KURV instrument.


## Shared gradient correction

The common preparation path now patches the pinned GPUI wgpu shader's incorrect
sRGB/linear transfer around gradient interpolation. The native plugin and render
lab use the same correction, including Oklab gradients. RX 6600 checks cover
paths/quads at 1×/1.5×/2×; the standard gray-ramp mean error falls from 50.71 to
0.25 / 255. See [the report](../../docs/render-lab/rx6600-gradient-fix-2026-09-13/RESULTS.md).
Subpixel stroke AA remains open. A 1.5× interaction run on the restricted lab
viewport failed at source re-arming; the full-desktop run passed. This is separate
from the successful multi-scale gradient checks and existing wheel-harness issues.

## Shared Bézier curve components

See [usage and contracts](../../docs/CURVE-COMPONENTS.md) and the
[KURV migration checklist](../../docs/KURV-MIGRATION.md).
The LFO and oscillator unison panels use the same curve editor. The unison view
is compact and samples its response once per voice. MUI core owns the cubic model,
validation and evaluator; GPUI owns editing and drawing. No Truce dependency was
added to the graphics core.

Drag anchors or their square handles. Double-click adds/removes anchors or resets
a handle's segment; Alt-double-click inserts a point without changing the curve.
Shift is fine movement, Alt bypasses snapping. Brackets select points, H cycles
handles, arrows edit, Delete removes/resets, Escape cancels.

`tools/check_input.py` exercises both editors' native anchor/handle drags and owner
event balance before existing composition checks at all three scale factors.
KURV DSP and preset bindings remain pending.
