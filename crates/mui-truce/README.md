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
- Native GPU APIs are tried before GL; `WGPU_BACKEND` still restricts which
  APIs may be used. Surface/device recovery stays in the GPU host.
- If GPU initialization or rendering fails, the editor uses Vello CPU and
  native software buffers for the rest of that window's lifetime. Reopening
  tries the GPU again. This does not require a software Vulkan driver.
- `MUI_RENDERER=cpu` skips GPU initialization entirely. For example,
  `MUI_RENDERER=cpu reaper` forces software rendering for MUI editors in that
  process. This is a rendering diagnostic, not a fix for plugin-format or
  native-window initialization errors.
- CPU fallback retains unchanged pixels and uses one rasterization thread
  with runtime SIMD selection. Changed frames currently repaint fully. The
  RGBA target is limited to 64 MiB; presentation buffers and scratch storage
  are additional. GPU weld declarations are rebuilt with the reference CPU
  backend, rather than omitted. Software plugin windows are opaque.

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
cargo truce build --clap --vst3 -p mui-gain-plugin
```

The bundles land in `$CARGO_TARGET_DIR/bundles` (`target/bundles` by default):
`MUI Gain.clap` and `MUI Gain.vst3`. For a quicker unoptimised build, add `--debug`.

A plugin must use panics that unwind. Both workspace `release` and `plugin`
profiles now set `panic = "unwind"`; plain `--release` and the bundle command
are safe from an accidental abort strategy. Size optimization, LTO, one
codegen unit and symbol stripping remain enabled. Unwind adds metadata and
some code size, but saving a few bytes must not make the default plugin build
terminate its host.

```sh
cargo build --release -p mui-gain-plugin         # the bare cdylib
cargo build --profile plugin -p mui-gain-plugin  # compatible explicit profile
```

**Dependency profiles do not propagate.** A downstream plugin must set this
in its final workspace manifest, including any custom shipping profile:

```toml
[profile.release]
panic = "unwind"
```

`mui-truce` emits a compile error under `cfg(panic = "abort")`, including when
an environment override changes the final strategy. The explicit
`allow-panic-abort` feature disables that guard for applications which
intentionally accept process termination. It does not make panic guards work
under abort. Never enable it in a DAW plugin. `catch_unwind` also cannot
contain native access violations, a panicking panic hook, or an aborting
foreign library. MUI does not install a process-global panic hook.

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

Apply and qualify these wrapper fixes in the maintained
[moose framework fork](https://github.com/Matari-Audio/moose).
MUI does not patch or vendor truce's format wrappers.

## The baseview

The window is [moose-baseview](https://github.com/Matari-Audio/moose)
(upstream baseview 0.3.4 plus MOOSE patches), re-exported as
`window::baseview`; depend on it through that, not directly, or the plugin
links two baseviews.

- Keyboard: on Windows the editor takes the keyboard from the host only
  while a text field is focused (`Ui::focus_is_text`); every other key goes
  to the host, so DAW shortcuts work while the editor has focus. On macOS
  and X11 a key the editor ignores goes to the host. Always on; there is no
  feature to enable.
- Scale: the host's scale is a scale override on Windows and X11 (macOS
  follows its backing scale). The same policy applies at open and on late
  scale notifications; a host's default 1x never overrides AppKit Retina.

## Size and scale handshake

Each editor retains its own logical size and host scale. A new editor starts
with the authored nonzero size; an invalid/zero resize is rejected without
overwriting the last committed size. Accepted resizes are retained even
before open. Open creates a fresh request channel from that retained state.
A late scale notification queues both the scale and the latest logical size,
so a stale native/parent size cannot become the scale change's reference size.
Late resize notifications replace that logical size. Neither path requests a
host resize synchronously from `set_size`.

All six orders of set-scale/open/resize converge on the same retained
geometry. That is policy-test evidence, not proof of every host's native
parenting behavior. An initially hidden parent still relies on baseview's
visibility/expose events; see the platform smoke checks.

Close/reopen of the **same editor** retains its scale. If a framework creates
a new editor, it must replay the plugin instance's last host scale.
Truce 6.3 VST3 does this; CLAP stores the scale on its instance but does not
replay it in `gui_create`/`gui_set_parent`. MUI no longer borrows another
editor's scale to hide that wrapper bug.

## Host-thread delivery

Direct synchronous begin/set/end remains the default for compatibility.
**On Linux, truce 6.3 VST3 consequently calls host edit APIs from baseview's
X11 window thread. That is outside VST3's GUI-thread contract.** Its resize
callback, and CLAP's GUI resize callback, can likewise run on that thread.
Calls can occur under MUI's model lock. The fix requires a main-thread pump
in the framework; the current truce CLAP/VST3 wrappers do not call
`Editor::idle`. An idle queue alone would silently strand edits.

Frameworks with that pump can select `MuiEditor::with_host_pump()` (or
`Bridge::with_host_pump()`) before opening. Retain `host_pump()`'s
`Arc<HostPump>` in the main-thread callback and call `flush()` **outside**
the framework's editor/model lock. For event-driven delivery, register
`HostPump::set_waker` with a thread-safe notification such as CLAP's
`request_callback`; it fires once when an empty queue becomes pending and
must never flush/re-enter the editor inline. Re-register it for each open.
`Editor::idle` also flushes, if the
framework can guarantee that entry is safe. A wrong-thread flush does
nothing. No lock in this queue is shared with the audio thread.

Queued sets coalesce per parameter within a pending gesture; begin/end
boundaries remain ordered. Close queues the remaining Ends and flushes after
releasing the model lock. Drop revokes without calling a possibly destroyed
host. A stalled queue is bounded to 4096 commands; overflow revokes further
edits and the next host-thread flush ends already-delivered gestures. Direct
`Bridge::context()` mutations bypass the queue and remain the caller's
responsibility. Deferred resize returns no immediate acceptance: the wrapper
must forward the host's accepted logical size through `Editor::set_size`.
MUI does not optimistically resize the child merely because it queued a
request. Rejected requests keep the old size. Close cancels stale resizes.
Without a pump and authoritative resize replies, do not select deferred mode.

## Shipping a MUI plugin safely

- Set `panic = "unwind"` in the **final** workspace's shipping profile.
  Keep `allow-panic-abort` disabled and check the exact packaging command.
- Use the example's fallible font initialization and MUI's editor guards.
  A build/initialization panic produces no live child or attached bridge;
  truce's `Editor::open` returns `()`, so MUI cannot report a format-level
  open failure until the framework offers a fallible entry.
- Keep parameter/meter storage lock-free for audio. Never hold the UI lock
  during framework-pump host delivery.
- Do not ship an opt-in queued adapter until its main-thread pump exists.
  Fix/qualify the default Linux VST3 thread-contract limitation in moose.
- When recreating an editor, replay the plugin instance's host scale. Test
  late scale/resize, zero/stale parent bounds and hidden-parent first open.
- Before moving a UI across threads or unloading its image, release memo
  captures on their rendering thread (`Ui::release_thread_memos`).
  MUI's truce session does this on native cancellation and preflight handoff.
- Run CLAP/VST3 validation, two same-plugin editors, two different plugin
  binaries/versions, close/reopen and unload tests in real DAWs on each OS.
  Repeat with `MUI_RENDERER=cpu`. Headless tests are not a DAW qualification.

## Tests

`cargo test -p mui-truce -p mui-baseview` runs the tests (the handler tests live in mui-baseview). None of them needs a window or a GPU.

- The handler tests cover logical and physical sizes, modifiers, coalescing,
  the cancel on focus loss, idle skipping and minimised windows.
- The bridge tests use truce's `ClosureBridge`. They record the exact host
  begin/set/end sequence for drags, key steps, toggles, discrete steps, closes
  and state loads. These default-delivery regressions stay unchanged.
- Policy tests cover per-editor scale, all six scale/open/resize orders,
  invalid sizes/scales, panicking open/idle author callbacks and abort guards.
- Optional pump tests cover gesture ordering, coalescing, wrong-thread calls,
  re-entry, wake notifications, overflow cleanup, callback panics and close
  delivery without the model lock. None needs a native window.

Not implemented:

- **File drops.** MUI has no payload for them, and on Linux upstream baseview
  has no XDND.
- **IME composition.** baseview has no API for it.
