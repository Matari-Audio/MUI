# MUI editors inside truce plugins

Truce is the plugin framework: it owns the CLAP/VST3 wrappers, the parameter
store, the audio runtime and the state format. This crate adds the editor:

- `MuiEditor` implements truce's `Editor`. It opens a baseview child window
  inside the host's window, puts a wgpu surface on it, and paints each resolved
  `Ui` frame with `HybridEffects`.
- `Bridge` binds widget ids to truce parameters. A drag, a key step or a click
  becomes the host's begin/perform/end, and host automation or a state load is
  the value that the next tree reads.
- `window` is the half that knows no plugin framework: a `View` trait, the
  native event queue and the GPU surface. An adapter for another framework can
  open the same window with its own `View`.

```rust
fn editor(params: Arc<GainParams>) -> Box<dyn Editor> {
    MuiEditor::new(params, Ui::new(Theme::DEFAULT), (300, 200), |ui, bridge| {
        let gain = bridge.bind(ui, "gain", P::Gain, |ui, v| {
            knob(ui, "gain", "Gain", v, 0.0..=1.0).0.el()
        });
        col![gain, title(bridge.text(P::Gain))].pad(L).fill(Surface)
    })
    .resizable((260, 180))
    .into_editor()
}
```

The `v` that `bind` passes in is the parameter's **normalized** value. The
bridge handles gestures as follows:

- A gesture (`Edit::Begin` .. `Edit::End`) is one host begin/end bracket, with
  one `set_param` per value change.
- A key step or a click with no gesture open is wrapped as begin/set/end.
- Discrete parameters send whole steps only. A drag accumulates between steps.
- Read-only and unknown ids are drawn but never reach the host.
- Closing the editor, or a host state load while a gesture is open, ends the
  gesture.
- A parameter or meter changed by the host wakes an idle editor. Otherwise a
  settled editor paints nothing.

What the window does:

- Pointer, wheel, key, text and clipboard input. The Ctrl/Cmd+V paste reads
  the system clipboard.
- A hover burst is coalesced to its last position. Presses, releases and drag
  samples keep their order, one `Ui::frame` each.
- Focus loss cancels the gesture in flight.
- Host scale and resize negotiation are handled. The host may not resize
  below `.resizable(min)`. A window that opens smaller than the tree's
  `ui.min_size()` asks the host once to grow.
- Cursor shapes.
- GPU recovery: a lost device or surface is rebuilt on the next tick.

## Example plugin

`examples/gain-plugin` is a gain knob, a bypass toggle and an output meter.
To build it, install `cargo-truce` (`cargo install cargo-truce`), then run from
the example's directory so that `cargo truce` finds its `truce.toml`:

```sh
cd examples/gain-plugin
CARGO_PROFILE_RELEASE_PANIC=unwind cargo truce build --clap --vst3 -p mui-gain-plugin
```

The bundles land in `$CARGO_TARGET_DIR/bundles` (`target/bundles` by default):
`MUI Gain.clap` and `MUI Gain.vst3`. The workspace release profile sets
`panic = "abort"`. The `CARGO_PROFILE_RELEASE_PANIC=unwind` override keeps
truce's FFI `catch_unwind` working, so a panic in the plugin is contained
instead of aborting the host. For a quicker unoptimised build, add `--debug`.

Validate the bundles (the commands and results below are from 2026-09-23,
Linux, X11 display, clap-validator 0.4.1, pluginval 1.0.4):

```sh
clap-validator validate "target/bundles/MUI Gain.clap"
pluginval --strictness-level 5 --validate "target/bundles/MUI Gain.vst3"
```

- **clap-validator:** 44 run, 34 passed, 7 skipped, 3 failed. The failures are
  `state-reproducibility-basic`, `-binary` and `-buffered`: after
  `clap_plugin_state::load` the values change without a
  `clap_host_params::rescan(CLAP_PARAM_RESCAN_VALUES)`, and truce-clap 6.3's
  `state_load` never requests one. This is upstream, in the truce wrapper.
- **pluginval:** exit 1. Every pluginval test passes, including `Editor`,
  `Open editor whilst processing` and `Editor Automation`. Only Steinberg's
  `vst3 validator` step fails, with "Missing mandatory
  IProcessContextRequirements extension". truce-vst3 6.3's shim declares that
  IID as `0x2A654303, 0xEF764E3C, 0xA8E8C6F3, 0xDBAE0F77`. The SDK's IID is
  `0x2A654303, 0xEF764E3D, 0x95B5FE83, 0x730EF6D0`. This is upstream too.

Kurv vendors truce with both fixes (`vendor/truce-clap-6.3.0`: `state_load`
sets `needs_rescan` and calls `request_callback`; `vendor/truce-vst3-6.3.0`:
the SDK IID). MUI does not vendor 12k lines of wrapper for two one-line
fixes in an example; they belong upstream.

## What Kurv's vendored baseview adds

The editor builds against upstream `baseview-truce`. Kurv's copy adds, and
this crate does without:

- A 4 ms X11 frame poll (upstream: 15 ms). Here an animation ticks at about
  66 Hz on X11.
- Forwarding of keys the editor ignores to the host's window, so DAW
  shortcuts still work while the editor has focus. Upstream has no
  forwarding, so the editor captures every key.
- XDND file drops and a bounded-close detach for blocked renderer threads.

## Tests

`cargo test -p mui-truce` runs the tests. None of them needs a window or a GPU.

- The handler tests cover logical and physical sizes, modifiers, coalescing,
  the cancel on focus loss, idle skipping and minimised windows.
- The bridge tests use truce's `ClosureBridge`. They record the exact host
  begin/set/end sequence for drags, key steps, toggles, discrete steps, closes
  and state loads.

Not implemented:

- **File drops.** MUI has no payload for them, and on Linux upstream baseview
  has no XDND.
- **IME composition.** baseview has no API for it.
