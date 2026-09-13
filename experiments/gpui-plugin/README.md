# GPUI through Truce's plugin editor contract

First compatibility milestone, tested 2026-09-13 on the RX 6600.
This builds a real CLAP gain effect with a GPUI view and a runnable editor-host
check. It does not replace KURV's existing editor.

The editor check passed X11 parenting, accepted/rejected sizes, pointer and
Return-key activation, host-thread `begin_edit → set_param → end_edit`, two
simultaneous editor instances, and close/reopen. GPUI reported the RX 6600,
Mesa 26.2.2, `software=false`; see [runtime.txt](results/runtime.txt).
Text is rendered by GPUI's actual text system. The button uses GPUI's accessible
role, focus handling, focus outline and built-in keyboard activation.

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
DISPLAY=:89 MESA_VK_DEVICE_SELECT='1002:73ff!' ZED_DEVICE_ID=0x73ff \
  cargo run --locked --manifest-path experiments/gpui-plugin/Cargo.toml
```

The build produces `libmui_gpui_plugin_probe.so` in Cargo's configured target
directory. Copy it to a `.clap` filename to load it as an experimental plugin;
`output/mui-gpui-probe.clap` is the locally built, debug-symbol-stripped artifact.
Run `clap-validator validate output/mui-gpui-probe.clap` from this directory.
Expect the three recorded state failures until the wrapper issue is resolved.

## What this proves, and what remains

GPUI's pinned Linux implementation blocks in `Platform::run`, even when called
through `Application::run_embedded`. `prepare.py` adds an experimental X11
nonblocking event pump and leaves ordinary application startup unchanged.
It reuses the renderer lab's pinned upstream checkout and verifies patch anchors.
Running preparation twice is supported.

GPUI's `Rc`-based application lives entirely on an editor worker thread. Truce's
`Editor: Send` contains channels and a join handle, without unsafe Send casts.
A bounded command queue carries host updates; discrete button edits return to
`Editor::idle` for host callbacks. Close joins the worker and drains pending edits.
The probe polls at roughly 125 Hz; idle power and stalled-GPU shutdown are not
characterized. Parenting currently reparents a GPUI X11 window after creation.

This is an editor-contract harness plus a CLAP binary, **not a real DAW GUI test**.
The harness bypasses CLAP's GUI extension; validator checks do not establish DAW
embedding compatibility. KURV and its vendored Truce patches are untouched.
Host-specific DPI changes, IME/text editing, screen-reader behavior, window
unload/leak testing, VST3 and other operating systems remain unverified.

Next: test the CLAP GUI path in a DAW and resolve the state-validation failures;
then render an existing MUI layout/geometry fixture in the GPUI view. This probe
uses GPUI layout for its two elements. MUI's existing layout, geometry and Parley
code remain intact; no duplicate widget framework or GPUI+Vello composition layer
has been introduced before the embedding decision is established.
