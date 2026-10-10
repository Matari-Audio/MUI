# x11 work package — integration contract (implementation in progress)

Branch: `stab/x11`. First-party baseview: selective fixes, no upstream rebase.

## Exact frame-demand API

- `baseview::FrameDemand::{Continuous, Idle, At(std::time::Instant)}`.
- `WindowHandler::frame_demand(&self) -> FrameDemand`, default `Continuous`.
- `baseview::FrameRequester`: `Clone + Send + Sync`.
- `FrameRequester::new(wake: impl Fn() + Send + Sync + 'static) -> Self`.
  The platform callback must be nonblocking, must not retain native/editor
  resources and must not call the window handler.
- `FrameRequester::request_frame(&self)`: coalesced, asynchronous X11 wake.
- `Window::frame_requester(&self) -> Option<FrameRequester>`: X11 returns
  `Some`; other platforms return `None` until their workers' implementation is
  integrated by the coordinator. No `WindowContext` changes required.

X11 asks demand after events/frames, removes its timer for `Idle`, arms one
paced timer for `Continuous`, and one deadline timer for `At`. Real input,
resize, visibility restoration and Expose request a frame independently.
A requester owns only loop wake state, not the native window. After close,
requests are harmless. Existing users need not change their handler.

## MUI integration ownership

This branch adds only `Adapter::frame_demand` in mui-baseview. GPU worker owns
`Requests` storage/binding and resize/scale/redraw wakes. Bind the requester
right after window creation, before show. Store request atomics before wake.
The adapter combines `Driver::next_wake`, `unpainted`, `gpu_retry_at`, and a
250 ms polling heartbeat. The heartbeat is necessary because `View::changed`,
plugin meters/parameters and accessibility requests are still polled; push
notifications would let MUI sleep indefinitely. Baseview `Idle` has **no**
safety heartbeat. At merge, also poll at 25 ms while `gpu_init.pending()`:
CPU fallback is NOT evidence that asynchronous recovery is idle.

## Lifecycle caveats being addressed

X11 host RPC must have a bounded send/response path and per-request response
identity. No delayed reply may acknowledge a different request. Explicit
standalone `run_until_closed` is intentionally blocking, not a plugin API.
Pin the editor image before spawning a worker. If this fails for a shared
library, log and fail editor window open, never create an unpinned worker or
wait unboundedly on close. The main executable cannot unload and needs no pin.

Bounded close cannot force a callback/driver call already running to return.
Revocation prevents further callbacks; timeout detaches only with unload-safe
code. Parent/native/GPU teardown may finish later. Callers must revoke all
handler host access before bounded close. Image pinning does NOT retain host
state, parent drawables, or host callback implementations. Arbitrary registered
`HostCallbacks`/`HostMainThreadCaller` must obey this same lifetime rule; no
claim that pinning makes stale host calls safe. More verification and final
results follow in later commits.
