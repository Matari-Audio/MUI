# MUI editors inside truce plugins

Truce is the plugin framework: it owns the CLAP/VST3 wrappers, the parameter
store, the audio runtime and the state format. This crate adds the editor:

- `MuiEditor` implements truce's `Editor`. It opens a baseview child window
  inside the host's window, puts a wgpu surface on it, and paints each resolved
  `Ui` frame with `GpuRenderer`.
- `Bridge` binds widget ids to truce parameters. A drag, a key step or a click
  becomes the host's begin/perform/end, and host automation or a state load is
  the value that the next tree reads.
- `window` is the half that knows no plugin framework: the `mui-baseview`
  crate, re-exported. A `View` trait, the native event queue and the GPU
  surface. An adapter for another framework can open the same window with its
  own `View`, and an app can `run` it top-level.

```rust
fn editor(params: Arc<GainParams>) -> Box<dyn Editor> {
    MuiEditor::new(params, Ui::default(), (300, 200), |ui, bridge| {
        let gain = bridge.bind(ui, P::Gain, |ui, id, v| {
            knob(ui, id, "Gain", v, 0.0..=1.0)
        });
        col![gain, title(bridge.text(P::Gain))].pad(L).fill(Surface)
    })
    .resizable((260, 180))
    .into_editor()
}
```

`bind` derives the widget id from the parameter (`widget_id(P::Gain)`, which
is `param/0`) and hands it to the closure, so the id the widget reports its
gesture under cannot differ from the one the parameter listens for. The `v`
it passes in is the parameter's **normalized** value. `bind_bool` is the same
for a switch, with a `&mut bool`. The
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

## Accessibility

On Linux the editor publishes its tree over AT-SPI with `accesskit_unix`,
which needs no window handle: names, roles, values and focus reach Orca, and
a reader's click, focus, set-value, step and text-selection requests become
`Ui` actions. A frame that did not change the tree sends an empty update
(`mui_access::Publisher`). Bounds are window-relative, because baseview does
not report the child window's screen position.

Windows and macOS are not wired:

- Windows: `accesskit_windows::SubclassingAdapter` refuses a window that is
  already visible, and baseview creates the child `WS_VISIBLE` before any
  editor code runs. The plain `accesskit_windows::Adapter` needs the window
  procedure's `WM_GETOBJECT`, and baseview has no hook for raw messages.
  Either fix belongs in baseview: create hidden, or forward `WM_GETOBJECT`.
- macOS: `accesskit_macos::SubclassingAdapter` over baseview's `NSView`
  looks workable, but it cannot be built or tried from this repo's Linux CI,
  and an untested subclass of the host's view hierarchy is a crash inside a
  DAW. It is the next step when there is a Mac to test on.

## Example plugin

`examples/gain-plugin` is a gain knob, a bypass toggle and an output meter.
To build it, install `cargo-truce` (`cargo install cargo-truce`), then run from
the example's directory so that `cargo truce` finds its `truce.toml`:

```sh
cd examples/gain-plugin
CARGO_PROFILE_RELEASE_PANIC=unwind cargo truce build --clap --vst3 -p mui-gain-plugin
```

The bundles land in `$CARGO_TARGET_DIR/bundles` (`target/bundles` by default):
`MUI Gain.clap` and `MUI Gain.vst3`. For a quicker unoptimised build, add `--debug`.

A plugin must be built with panics that unwind. The workspace release profile
sets `panic = "abort"`, which turns every `catch_unwind` at the FFI edge (truce's
and `mui-truce`'s window callbacks) into dead code: a panic in the editor then
aborts the host and the user's session with it. The workspace has a `plugin`
profile for this, release with `panic = "unwind"`:

```sh
cargo build --profile plugin -p mui-gain-plugin   # the bare cdylib
```

`cargo truce build` always builds `release`, hence the
`CARGO_PROFILE_RELEASE_PANIC=unwind` override on the bundle command above.

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

`cargo test -p mui-truce -p mui-baseview` runs the tests (the handler tests live in mui-baseview). None of them needs a window or a GPU.

- The handler tests cover logical and physical sizes, modifiers, coalescing,
  the cancel on focus loss, idle skipping and minimised windows.
- The bridge tests use truce's `ClosureBridge`. They record the exact host
  begin/set/end sequence for drags, key steps, toggles, discrete steps, closes
  and state loads.

Not implemented:

- **File drops.** MUI has no payload for them, and on Linux upstream baseview
  has no XDND.
- **IME composition.** baseview has no API for it.
