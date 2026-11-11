# Native windowing: how each platform behaves, and what is unproven

What the October 2026 stabilisation work (formerly five `STAB-NOTES-*.md`
files at the root) established about the native layer: the vendored baseview
fork (`vendor/moose-baseview`), `mui-baseview`, `mui-truce` and the GPU host in
`mui-vello`. Branch names, commit lists and command logs from that work are in
git history; this keeps the behaviour and the reasons.

## Shared rules

- **Frame demand.** `WindowHandler::frame_demand()` returns
  `FrameDemand::{Continuous, Idle, At(Instant)}` (default `Continuous`).
  `Idle` arms no timer at all; `At` arms one deadline; input, resize,
  visibility restoration, expose and `FrameRequester::request_frame()` each wake
  one frame. Requests coalesce while a frame is queued or running; a request
  made during a frame survives for the next. A `FrameRequester` holds only wake
  state, never a native window or connection, and is inert after close.
- **MUI still polls at 4 Hz when settled.** `View::changed`, plugin meters and
  parameters and accessibility requests are polled (250 ms), GPU init at 25 ms
  while pending, adapter retries back off from 250 ms to 8 s. Push
  notification of model changes is the path to indefinite sleep.
- **Image pinning.** Before any detached thread, class registration or hook
  exists, the image containing baseview is pinned (`RTLD_NODELETE` / DLL pin,
  `pin.rs`). A failed pin refuses to open the editor (not the plugin). This
  keeps code mapped for the process lifetime on purpose: detached workers and
  late OS callbacks can never run unmapped code. Hot replacement of a plugin at
  the same path is not supported.
- **Panics never cross an ABI boundary.** Every OS callback (X11 loop, WndProc,
  hooks, COM drop targets, Objective-C IMPs, timers) runs under `catch_unwind`
  with a safe default and one diagnostic per image. `mui-truce` and
  `mui-baseview` refuse to build with `panic = "abort"` unless
  `allow-panic-abort` is enabled; release profiles unwind.
<!-- coderabbit-ai-generated:v1 -->

<!-- coderabbit-ai-generated-section:v1:88400618d1d55a66123a41abc6bda8f857238936761723dd0dafe0e831860b5a -->

- **Native rendering.** The `mui-baseview` native worker polls GPU
  initialisation before rendering its first frame, so a CPU frame is not
  guaranteed first. It renders offscreen and sends bitmap patches to the
  window thread for presentation. The separate surface-based handler starts
  GPU initialisation after a successful CPU frame and owns its surface on the
  window thread; its startup frame above 1,048,576 pixels is background only.
  Compatible pinned editors share device, queue and pipelines through strong
  static session references, with separate slots where needed; each keeps its
  own targets. Device loss can trigger a shared context rebuild. A submitted
  frame is not proof of scanout.

<!-- /coderabbit-ai-generated:v1 -->
- **Scale belongs to an editor**, never a process global: macOS uses AppKit's
  backing scale, Windows the host's, X11 defaults to 1.

## X11

- A checked `MapWindow` only means the WM accepted the request: viewability
  comes from a real `MapNotify`/`Expose` (the first-open black-window and
  `GetImage BadMatch` failures).
- Opaque non-GL windows use the root visual and depth with an owned colormap,
  not ARGB32. Creation and reparent are checked XCB requests; zero extents
  become 1x1. XEmbed reparent discards the old ancestry.
- Window RPC is a one-entry `try_send` queue with a 50 ms reply timeout and a
  fresh reply channel per request; startup has a 250 ms handshake budget, close
  250 ms. A thread is joined only after it has finished.
- Only GLX installs an Xlib error handler, serialised across plugin images by a
  mutex plus `flock` on `/proc/self`. The default (non-GL) path installs none.
- Present-extension pacing (upstream baseview #323) was read and not taken: it
  frames continuously when idle. Pacing is a demand-aware monitor-rate timer.

## Windows

- HWND lifetime is owned by `platform/win/native.rs`: userdata cleared on
  `WM_NCDESTROY`, handler dropped before `DestroyWindow`, stale frame messages
  rejected by an image marker and window serial. Class names carry the image
  address and a serial; registrations outlive the HWND.
- Keyboard hooks are per GUI thread per image, removed with the last window,
  and only intercept keys for a focused, capturing editor.
- Ported upstream baseview #345 (DPI awareness), #351 (initial resize waits
  for a visible `WM_SHOWWINDOW`; rejection restores the old size) and #347
  (`WM_CAPTURECHANGED`).
<!-- coderabbit-ai-generated:v1 -->

<!-- coderabbit-ai-generated-section:v1:1090bd3aa4ec51de3d52c7d3e356c1cac232969e88e76400dd7b3e0b8998afdc -->

- Child windows use `WS_CLIPCHILDREN | WS_CLIPSIBLINGS`. They start hidden,
  are shown with `SW_SHOWNOACTIVATE` after handler construction, and receive a
  dark placeholder on the first `WM_PAINT`. They use neither
  `WS_EX_NOREDIRECTIONBITMAP` nor `WS_EX_LAYERED`: the native worker presents
  bitmap patches with GDI and needs redirection.

<!-- /coderabbit-ai-generated:v1 -->
- The pacer thread waits with `DwmFlush` (`Continuous`) or a condition
  variable (`Idle`/`At`); hidden ancestors re-check visibility every 50 ms.
- Focus is taken only by a user press or an explicit `focus()`; capture
  release returns focus to the parent only if the child still owns it.

## macOS

<!-- coderabbit-ai-generated:v1 -->

<!-- coderabbit-ai-generated-section:v1:c736252a1dc86f43887f19422810d24253ca8f08a7cdc096118fe898f4cb0bc5 -->

- The root backing layer is a `CAMetalLayer`, established via
  `makeBackingLayer` and `wantsLayer` before insertion. The native worker
  presents bitmap patches in CoreAnimation sublayers; the separate
  surface-based handler lets wgpu adopt the root. The initial colour is the
  window background, never black.

<!-- /coderabbit-ai-generated:v1 -->
- Classes are registered once per view type per image and never disposed.
- Close revokes wakes, invalidates display links and timers, removes
  observers and tracking areas and restores the previous responder before the
  handler is torn down; the `CADisplayLink` retain cycle is broken on detach.
- Attaching never steals focus or changes the DAW window's mouse-move
  setting. Pacing: display link on macOS 14+, a run-loop timer before.
<!-- coderabbit-ai-generated:v1 -->

<!-- coderabbit-ai-generated-section:v1:e6bc3ac21003220fe69ec34a6f05995e147403a855cb70da153501b989dc7d9c -->

- Native top anchoring handles flipped children; the old per-tick MUI
  re-anchor was removed because it undid that behavior on flipped parents.
  The surface-based handler removes Softbuffer's CPU sublayer when switching
  to GPU.

<!-- /coderabbit-ai-generated:v1 -->

## Open: host threading (the largest known gap)

<!-- coderabbit-ai-generated:v1 -->

<!-- coderabbit-ai-generated-section:v1:363139674f935786275ca11dc4932e9c3b6def8006ce227dbca091990d9de285 -->

On Linux, the X11 window thread drives parameter `begin/set/end` and resize.
VST3 requires these calls on the host GUI thread, while truce 6.3 forwards them
synchronously. CLAP parameter writes are queued safely, but resize is not.
`HostPump` in `mui-truce` queues and coalesces these operations while preserving
gesture ends. Native `MuiEditor` attaches and flushes it on the native window
thread. On X11, host GUI thread delivery also requires changing where the pump
attaches: a host-thread `Editor::idle` call alone cannot flush a pump attached
to the window thread. The wrapper must pump `Editor::idle` from VST3's
`IRunLoop` timer and CLAP's timer/posix-fd support or `on_main_thread`, flush
outside the editor and model locks, and replay the instance scale when CLAP
creates a new editor. The moose convergence plan is in
`docs/windows-editor-thread.md`.

<!-- /coderabbit-ai-generated:v1 -->

## Open: native qualification

Nothing above has run in a real DAW, and Windows and macOS were only
cross-compiled when this work landed. Still required: open/close/reopen/unload
in Ableton, FL Studio, Bitwig, Reaper and Cubase; two editors on one and on two
GUI threads; two independently linked plugin images; DPI-unaware through
per-monitor-v2 hosts at 125-200 %; layered host parents; minimize/restore and
zero-size attachment; IME in two editors; macOS Retina transitions and live
resize; Present/vsync timing; unload while AT-SPI is active. Native faults,
driver crashes and `panic = "abort"` cannot be contained by unwinding.
