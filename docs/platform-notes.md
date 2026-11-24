# Native windowing: how each platform behaves, and what is unproven

What the native windowing and Wave 1B consolidation work established about the
owned native layer: `vendor/moose-baseview`, `mui-baseview`, and the GPU/CPU
host in `mui-vello`. Legacy `mui-truce`, `mui-winit`, `accesskit_winit`, and
the `winit` fork are retired; this documents the active behaviour and reasons.

## Shared rules

- **Frame demand & push-driven 0 Hz idle.** `WindowHandler::frame_demand()`
  returns `FrameDemand::{Continuous, Idle, At(Instant)}` (default `Continuous`).
  `Idle` arms no timer at all (true 0 Hz idle); `At` arms one exact deadline;
  input, resize, visibility restoration, expose, direct AccessKit activation /
  actions, and `FrameRequester::request_frame()` / `Requests::redraw()` each
  wake one frame immediately. Settled `mui-baseview` windows schedule zero
  periodic timers: exact deadlines are armed only for active animations,
  tooltips, carets, explicit live telemetry (`Requests::set_poll_interval`),
  pending asynchronous GPU initialization (25 ms while pending), or GPU/CPU
  retry backoff (250 ms doubling to 8 s). Requests coalesce while a frame is
  queued or running; a request made during a frame survives for the next. A
  `FrameRequester` holds only wake state, never a native window or connection,
  and is inert after close.
- **Image pinning.** Before any detached thread, class registration or hook
  exists, the image containing baseview is pinned (`RTLD_NODELETE` / DLL pin,
  `pin.rs`). A failed pin refuses to open the editor (not the plugin). This
  keeps code mapped for the process lifetime on purpose: detached workers and
  late OS callbacks can never run unmapped code. Hot replacement of a plugin at
  the same path is not supported.
- **Panics never cross an ABI boundary.** Every OS callback (X11/Wayland loop,
  WndProc, hooks, COM drop targets, Objective-C IMPs, timers) runs under
  `catch_unwind` with a safe default and one diagnostic per image.
  `mui-baseview` refuses to build with `panic = "abort"` unless
  `allow-panic-abort` is enabled; release profiles unwind.
- **GPU when ready, fast `vello_cpu` fallback (and `DeviceType::Cpu` rejection).**
  `mui-vello` rejects all `mui_hal::DeviceType::Cpu` adapters (Windows DX12 WARP,
  Linux Vulkan Lavapipe/SwiftShader, GL llvmpipe) unless explicitly opted in
  under `test-support`, so broken or CPU-only drivers go straight to our fast
  dirty-rect `vello_cpu` software path (`MUI_FORCE_CPU=1` or `MUI_RENDERER=cpu`
  also forces CPU rendering). Surface presentation prefers `Mailbox` → `Fifo` →
  `FifoRelaxed` → `Immediate` and reconfigures the swapchain after
  `Acquired::Suboptimal`. On HiDPI / 5K+ / ultrawide windows above 16 Mi
  physical pixels, direct GPU renders at full native resolution (bounded by the
  device texture limit), while CPU and offscreen-readback targets scale
  proportionally within the 16 Mi pixel budget and stretch cleanly to fill the
  window rather than ceasing to paint.
- **Native worker & standalone presenter.** `mui-baseview::open` / `run` runs
  the editor worker on a dedicated thread, presenting directly to the window's
  GPU surface where available (with `MUI_PRESENT=readback` opt-in for offscreen
  readback) and falling back to dirty-rect CPU bitmaps on startup or GPU fault.
  `mui-baseview::Presenter` + `mui-baseview::standalone` drive single-threaded
  standalone apps (`mui-preview`) with CPU-first startup and async GPU
  promotion. Editors of one image share the device, queue and pipelines through
  process-wide sessions; each editor keeps its own targets.
- **Scale belongs to an editor**, never a process global: macOS uses AppKit's
  backing scale, Windows the host's, X11 uses `Xft.dpi` / override, Wayland
  uses `wp_fractional_scale_v1` / output scale.

## X11 & Wayland (Linux)

- **Level-triggered XCB fd (`Mode::Level`) + full event drain.** The X11
  event loop watches the XCB file descriptor with `calloop::Mode::Level` and
  drains `poll_for_event()` to exhaustion after every synchronous reply so
  userspace-buffered events are never stranded under 0 Hz idle.
- **No `XInitThreads()` in plugins.** Calling `XInitThreads()` after a DAW host
  has already called Xlib is undefined behavior. `moose-baseview` calls
  `XInitThreads()` only from `assume_standalone_in_process()` for standalone
  apps; plugin windows use their dedicated X11 worker connection with XCB event
  queue ownership (`XCBOwnsEventQueue`).
- **Native Wayland.** `moose-baseview` supports standalone and floating
  `xdg_toplevel` windows (with server-side or fallback CSD decorations,
  `set_title`, `wp_fractional_scale_v1`, `text-input-v3`, cursor shape, and
  data-device drag-and-drop), plus `wl_subsurface` and CLAP nested compositor
  embedding (`mui_baseview::wayland`).
- **X11 details.** A checked `MapWindow` only means the WM accepted the
  request: viewability comes from a real `MapNotify`/`Expose`. Opaque non-GL
  windows use the root visual and depth with an owned colormap, not ARGB32.
  Creation and reparent are checked XCB requests; zero extents become 1x1.
  XEmbed reparent discards the old ancestry. `SuggestScaleFactor` rescales from
  the previous scale factor rather than compounding. Window titles set both
  `WM_NAME` and UTF-8 `_NET_WM_NAME`. Window RPC is a one-entry `try_send`
  queue with a 50 ms reply timeout and a fresh reply channel per request;
  startup has a 250 ms handshake budget, close 250 ms. Only GLX installs an
  Xlib error handler, serialised across plugin images by a mutex plus `flock`
  on `/proc/self`. Present-extension pacing was not taken because it frames
  continuously when idle; pacing is a demand-aware monitor-rate timer.

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
- Child windows use `WS_CLIPCHILDREN | WS_CLIPSIBLINGS`. They start hidden,
  are shown with `SW_SHOWNOACTIVATE` after handler construction, and receive a
  dark placeholder on the first `WM_PAINT`. They use neither
  `WS_EX_NOREDIRECTIONBITMAP` nor `WS_EX_LAYERED`: the native worker presents
  bitmap patches with GDI and needs redirection.

- The pacer thread waits with `DwmFlush` (`Continuous`) or a condition
  variable (`Idle`/`At`); hidden ancestors re-check visibility every 50 ms.
- Focus is taken only by a user press or an explicit `focus()`; capture
  release returns focus to the parent only if the child still owns it.

## macOS

- The root backing layer is a `CAMetalLayer`, established via
  `makeBackingLayer` and `wantsLayer` before insertion. The native worker
  presents bitmap patches in CoreAnimation sublayers; the separate
  surface-based handler lets wgpu adopt the root. The initial colour is the
  window background, never black.

- Classes are registered once per view type per image and never disposed.
- Close revokes wakes, invalidates display links and timers, removes
  observers and tracking areas and restores the previous responder before the
  handler is torn down; the `CADisplayLink` retain cycle is broken on detach.
- Attaching never steals focus or changes the DAW window's mouse-move
  setting. Pacing: display link on macOS 14+, a run-loop timer before.
- Native top anchoring handles flipped children; the old per-tick MUI
  re-anchor was removed because it undid that behavior on flipped parents.
  The surface-based handler removes `mui-present`'s CPU sublayer when switching
  to GPU.

## Host threading

MUI's editor runs `build()` and so makes parameter `begin/set/end` and resize
calls on its own worker thread, on every OS. What each format needs:

- **VST3** requires `IComponentHandler` and `IPlugFrame::resizeView` on the
  host's UI thread. The moose VST3 shim queues any such call made off that
  thread, in order, and delivers it from the UI thread: through the host's
  `IRunLoop` event handler (an eventfd) on Linux, the main-queue dispatch source
  on macOS, and the UI-thread timer on Windows. A detach or teardown delivers
  what is still queued, so the host never stays mid-gesture.
- **CLAP** parameter edits are queued and flushed with `request_flush`, and
  `request_resize` is thread-safe: nothing to do.
- **AU** `AUEventListenerNotify` is thread-safe.
- **AAX** parameter gestures and resize requests route through `moose-aax` and
  `moose-mui` onto `mui-baseview`.

## Open: native qualification

Nothing above has run in a real DAW, and Windows and macOS were only
cross-compiled when this work landed. Still required: open/close/reopen/unload
in Ableton, FL Studio, Bitwig, Reaper and Cubase; two editors on one and on two
GUI threads; two independently linked plugin images; DPI-unaware through
per-monitor-v2 hosts at 125-200 %; layered host parents; minimize/restore and
zero-size attachment; IME in two editors; macOS Retina transitions and live
resize; Present/vsync timing; unload while AT-SPI is active. Native faults,
driver crashes and `panic = "abort"` cannot be contained by unwinding.
