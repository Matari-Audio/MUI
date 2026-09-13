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

CLAP validation: **34 passed, 7 skipped, 3 failed**. The failures are
`state-reproducibility-basic`, `-binary`, and `-buffered`: restored parameter
values changed without a host rescan notification. The probe uses published
Truce 6.3.0; its `state_load` applies parameters without notifying a rescan.
Keep this as a migration blocker, not a passing plugin validation result.
See [the complete validator output](results/clap-validation.json).

## Run

Linux/X11 only. From the MUI repository root:

```sh
python experiments/gpui-plugin/prepare.py
cargo build --locked --manifest-path experiments/gpui-plugin/Cargo.toml
```

Use a dedicated X server because the check moves the pointer and keyboard focus.
Start `Xwayland :89 -geometry 1920x1200 -nolisten tcp` in another terminal, then:

```sh
DISPLAY=:89 GPUI_X11_SCALE_FACTOR=1 MESA_VK_DEVICE_SELECT='1002:73ff!' ZED_DEVICE_ID=0x73ff \
  cargo run --locked --manifest-path experiments/gpui-plugin/Cargo.toml
```

The build produces `libmui_gpui_plugin_probe.so` in Cargo's configured target
directory. Copy it to a `.clap` filename to load it as an experimental plugin;
`output/mui-gpui-probe.clap` is the locally built, debug-symbol-stripped artifact.
Run `clap-validator validate output/mui-gpui-probe.clap` from this directory.
Expect the three recorded state failures until the wrapper issue is resolved.

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
updates use one atomic value instead of queued stale snapshots; commands are bounded.
UI automation events return to `Editor::idle`. Dragging sends one begin/end pair,
including cancellation or close; the host bridge balances a gesture even if the worker
fails. There are no unsafe Send casts or UI calls in the audio callback.

The current frame pump polls roughly every 8 ms. Idle power, animation scheduling under
load and stalled-GPU shutdown are not characterized. The field is an upstream example,
not a finished MUI text-edit widget: its value is editor-local and not saved in plugin
state, and long single-line text is clipped rather than horizontally scrolled.

This is an editor-contract harness plus a CLAP binary, **not a real DAW GUI test**.
The harness bypasses CLAP's GUI extension. The three state-validation failures above
remain a migration blocker. KURV and its vendored Truce patches are untouched.

Still needed within the first four framework areas: baseline alignment between text
and adjacent controls, nested MUI scrolling/clip policies, generalized transformed and
occluding-item hit testing, rich text, and real IME/bidirectional-text validation. The
sample viewport's clipping does not establish a general core overflow contract. Screen
reader behavior, host-negotiated DPI changes, VST3 and other OSes remain unverified.
Parley is still available for other renderers; this backend uses GPUI text consistently
instead of mixing Parley measurements with GPUI glyph placement. No combined
GPUI/Vello compositor or duplicate widget runtime has been introduced.

## Manual panel and newer core layout APIs

Run the probe with `--manual` on your desktop display to keep one panel open until you
close its window. The title is **MUI - interactive GPUI panel**; resizing updates the child
editor, and the automated XTest sequence is skipped in this mode.

MUI now also provides [baseline and viewport APIs](../../docs/LAYOUT-VIEW.md), with a
rendered nested-scroll/transform example. This probe still uses GPUI's own scroll container;
adopting the new core view snapshot in its painter and input dispatch is a separate step.
