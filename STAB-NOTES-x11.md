# x11 work package

**Standalone first-open fix:** `0d765cf1` —
`fix(x11): wait for real MapNotify before the first frame`.
This one-file commit has parent `19adc751` and is on `stab/x11-mapnotify` for
immediate cherry-pick. The identical fix is included in `756047f1` on
`stab/x11`. The companion was created with an isolated Git index, without
rewriting/resetting implementation history. Do not apply both as independent
fixes unless integrating the full branch with the earlier standalone fix.

Branch/worktree: `stab/x11`, `/home/derpcat/.t3/worktrees/MUI/stab-x11`.
First-party windowing layer: selective fixes, no upstream rebase.

Implementation commit order (after the coordinator's `524d059f`):

1. `8dcc79be` — additive shared demand/requester API.
2. `19adc751` — X11 requester handle/latch.
3. `58f776eb` — cross-image error scopes, checked Xlib/XCB connection errors.
4. `756047f1` — real-map visibility, visuals, bounded lifecycle and demand/latch
   pacing with unit/native regressions. This includes the standalone fix above.
5. `2a0d7b43` — loop error reporting, Clippy test layout, timeout caveats.
6. `0d3ed01e` — minimal MUI Adapter demand method, pending-GPU merge marker.
7. `ac572bba` — explicit no-WM a11y fixture documentation.

The following documentation-only commit contains this final report. Graft was
refreshed with `graft build`; it is a local git-ignored cache in this checkout.

## Exact frame-demand API

- `baseview::FrameDemand::{Continuous, Idle, At(std::time::Instant)}`
  (`#[non_exhaustive]`, `Clone + Copy + Debug + PartialEq + Eq + Default`).
- `WindowHandler::frame_demand(&self) -> FrameDemand`, default `Continuous`.
- `baseview::FrameRequester`: `Clone + Send + Sync`.
- `FrameRequester::new(wake: impl Fn() + Send + Sync + 'static) -> Self`.
  Platform wake operations must not block, retain native/editor resources,
  or invoke window handlers.
- `FrameRequester::request_frame(&self)`: coalesced, asynchronous X11 wake.
- `Window::frame_requester(&self) -> Option<FrameRequester>`: X11 returns
  `Some`; Windows/macOS return `None` until their workers' implementations
  are integrated by the coordinator. No `WindowContext` changes required.

X11 asks demand after events/frames. `Idle` removes the timer: no safety
heartbeat or refresh-rate polling. `Continuous` preserves refresh-rate
callbacks; `At` arms one monotonic deadline, bounded by the refresh cadence.
Hidden windows have no timer. Real input, resize, visibility restoration and
Expose request a frame independently. A requester owns only loop/shared wake
state, not an X connection or native window; requests after close are inert.

Only the beginning of an idle pass consumes the request latch (atomic swap).
The demand query and timer re-arm never clear it. An end-of-idle acquire-load
re-notifies pending work, so requests racing the query cannot disappear behind
a stale timer. Tests assert exactly one frame from one explicit request, idle
stability afterward, and a producer request after a stale five-second deadline
is computed but before that deadline is armed. That request draws immediately
and only once, not five seconds later or continuously.

## MUI integration and ownership

This branch adds only `Adapter::frame_demand` to mui-baseview's `lib.rs`.
GPU worker commits `bb1d57e7` / `f78f9da1` own Requests requester storage,
binding after creation, and resize/scale/redraw wakes. Store request atomics
before waking. At merge, the coordinator must complete the marked
`GPU-INIT-DEMAND` check:

```rust
if h.gpu_init.as_ref().is_some_and(|init| init.pending()) {
    at = at.min(recovery_poll);
}
```

The method already combines `Driver::next_wake`, pending redraw, `unpainted`,
`gpu_retry_at`, and named 250 ms model / 25 ms recovery polling intervals.
It never treats `software_only` or CPU presentation as evidence of idle.
The 250 ms heartbeat is intentionally MUI-specific: `View::changed`, plugin
meters/parameters and accessibility requests are polled, not push-notified.
Push notifications are the upgrade path to indefinite MUI sleep. Baseview's
`Idle` itself has zero periodic frame callbacks; a settled MUI adapter can
still poll at 4 Hz. The pending GPU integration is not tested on this branch.

## Changes mapped to reported bugs

- **#1 first open / #3 invisible / #4 black initial window:** a checked
  `MapWindow` can merely mean the WM accepted a redirected request. Show no
  longer marks the window viewable synchronously. Actual MapNotify/Expose
  establishes viewability. This reproduced an existing CPU-first-frame
  GetImage BadMatch failure on desktop XWayland, then made it pass.
- **#2 crashes / #3 invisible:** opaque non-GL windows use the explicit root
  visual/depth and a checked, owned colormap instead of arbitrary ARGB32.
  CreateWindow and reparent are checked XCB requests; zero initial size becomes
  1x1, and the actual visual is reported in raw handles. Colormaps are freed
  even when the host has already destroyed the child.
- **#1/#3/#2:** XEmbed reparent discards the old parent (the previous truncation
  retained it), refreshes even on reparent-to-root, and bounds ancestry
  rediscovery. Empty/destroyed/incomplete ancestry is not viewable. Existing
  destroyed-drawable callback suppression is retained.
- **#2 crashes / #5 freezes:** X11 RPC uses `try_send` on a one-entry queue,
  a 50 ms receive timeout, and a fresh response channel per request. A full
  queue fails immediately. A timeout does not cancel queued work, but a late
  reply can never acknowledge another request. Native window startup has an
  explicit 250 ms Rust handshake budget and wakes/cancels a late startup.
- **#2/#5:** image unload safety is checked before spawning an X thread.
  Unpinnable shared libraries log and refuse editor window open (not plugin
  instance creation). The main executable cannot unload and needs no pin.
  Ordinary close uses a 250 ms budget; close_bounded uses the caller's budget.
  Join only follows `is_finished`; no pin-failure or overflow path joins a
  running thread. Normal close preserves WillClose and handler-before-native
  teardown; timeout revokes further callbacks and detaches pinned work.
- **#2 crashes / multi-instance:** GLX swaps are serialized by a
  poison-tolerant Rust mutex plus flock on `/proc/self`, shared by separate
  plugin images. No created/stale lock file or exported Rust symbol is needed.
  RAII XSync/restoration handles ordinary exit and Rust panic. Foreign-display
  errors chain to the previous handler. Normal XCB windowing/XIM never needs
  an Xlib handler swap. Xlib's C callback catches/logs panics; Rust XIM and
  calloop callbacks are contained by window-thread init/dispatch unwind guards.
- **#5 idle CPU:** demand-driven timers, explicit wake coalescing, real-event
  frame delivery and removal of hidden/idle timers replace unconditional ticks.
  Existing users' default Continuous demand is preserved.

## Pacing: upstream #323 considered, not copied

Read `gh pr diff 323 -R RustAudio/baseview` (captured in
`/tmp/mui-audit/pr323.diff`). It arms the next Present Notify MSC after every
frame and adds fallback timers on map/error: that patch alone still generates
continuous idle frames. Porting it unchanged would not fix demand, and would
add extension/serial/error/unregistration state to this lifecycle change.
This branch keeps the existing monitor-aware timer only while demanded.
Present pacing remains a useful follow-up for active animation: honor Idle,
allow at most one outstanding MSC, and preserve a single demand-aware timer
fallback for servers without Present. No claim of compositor/vsync-perfect
active pacing is made here.

## Verified

The actual desktop has `DISPLAY=:0`, `XDG_SESSION_TYPE=wayland`: native :0
runs exercise **XWayland**, not a pure Xorg session, on this RX 6600 machine.
Transient Xvfb runs are isolated/no-WM fixtures and exit with xvfb-run.

Commands that passed (all cargo processes used the machine-wide lock):

```sh
flock /tmp/mui-cargo.lock cargo check --manifest-path vendor/moose-baseview/Cargo.toml -p moose-baseview --all-targets
flock /tmp/mui-cargo.lock cargo check --manifest-path vendor/moose-baseview/Cargo.toml -p moose-baseview --all-targets --features opengl
flock /tmp/mui-cargo.lock cargo test --manifest-path vendor/moose-baseview/Cargo.toml -p moose-baseview --lib --features opengl
flock /tmp/mui-cargo.lock cargo test --manifest-path vendor/moose-baseview/Cargo.toml -p moose-baseview --lib --features opengl native_ -- --ignored --test-threads=1
flock /tmp/mui-cargo.lock cargo check --manifest-path vendor/moose-baseview/Cargo.toml -p moose-baseview --target x86_64-pc-windows-msvc
flock /tmp/mui-cargo.lock cargo check --manifest-path vendor/moose-baseview/Cargo.toml -p moose-baseview --target aarch64-apple-darwin
flock /tmp/mui-cargo.lock cargo test -p mui-baseview --lib
flock /tmp/mui-cargo.lock cargo test -p mui-baseview --lib native_cpu_fallback_presents_and_reopens -- --ignored --test-threads=1
flock /tmp/mui-cargo.lock cargo test -p mui-baseview --lib native_surface_presents_and_reopens -- --ignored --test-threads=1
xvfb-run -a -s '-screen 0 1024x768x24' flock /tmp/mui-cargo.lock cargo test -p mui-baseview --lib -- --ignored --test-threads=1 --skip native_resize_failure_falls_back_and_presents
flock /tmp/mui-cargo.lock cargo clippy --manifest-path vendor/moose-baseview/Cargo.toml -p moose-baseview --all-targets --features opengl
flock /tmp/mui-cargo.lock cargo clippy -p mui-baseview --all-targets
```

Fork: **27 unit tests + 5 explicitly enabled native tests** passed. The latter
cover idle/exact-request wake, deterministic request-vs-timer race, simultaneous
host-thread open/close/reopen with default cadence, 24/root-depth child under a
32-bit parent when available, checked invalid parent, zero extent, XEmbed move,
and concurrent Xlib error restoration including panic. Unit regressions cover
actual request() timeout/full queue, isolated late replies, bounded stalled
worker close, executable lifetime and independent process-lock descriptors.
MUI: **15 normal tests**, **6 isolated native tests** passed; CPU and GPU
surface/recreation/reopen also passed on :0. Clippy passes, with pre-existing
IME/test and vendored XIM lifetime warnings; introduced warnings were fixed.

**Red -> green reproduction:**
`flock /tmp/mui-cargo.lock cargo test -p mui-baseview --lib native_cpu_fallback_presents_and_reopens -- --ignored --test-threads=1`.
Before the real-MapNotify fix, the :0 run failed at GetImage with BadMatch.
Afterward it passed. Logs: `/tmp/mui-audit/x11-mui-native.log` (red),
`/tmp/mui-audit/x11-mui-cpu.log` (green).

The a11y absolute-position fixture is now explicitly documented as requiring
an isolated no-WM X server, with its xvfb-run command. Desktop XWayland's WM
can redirect ConfigureWindow; its expected absolute coordinates do not apply.

## NOT verified / limitations

- The full ignored MUI suite is **not green**: the existing GPU resize-recovery
  fixture fails the immediate device-loss check on :0, and fails the recovered
  CPU pixel check under Xvfb (black `[0,0,0,0]`). Reported to GPU worker/
  coordinator; not weakened or changed here. Logs: x11-mui-native.log and
  x11-mui-xvfb.log in `/tmp/mui-audit`. The successful six-test command explicitly
  skips that unresolved test, not a claim of full native acceptance.
- No real Bitwig/Reaper/VST3 attachment, simultaneous performEdit/reopen,
  dlclose/unload test, independent loaded-DSO test, manual IME acceptance,
  native Windows/macOS execution, or pure-Xorg desktop run.
- The two-window native test and fake blocked-callback request regression prove
  this transport does not wait forever on GUI-marshalled work. They do not
  prove arbitrary host callbacks/model locks cannot create another deadlock.
- Pinning preserves the image containing baseview, not host state, parent
  drawables, or code in independently supplied handler/callback images. Such
  images require their own lifetime guarantee. Callers must revoke host access
  before close. An already-running callback/driver call is not cancellable;
  exceptional teardown can finish later, while the pinned image remains mapped
  until process exit. This is not leak-free or immediate actual image unload.
- Optional GLX can be wedged inside an error scope. In that case its temporary
  handler can remain active past the close timeout until GLX/XSync returns;
  code is pinned, and restoration follows scope completion. Forcibly restoring
  while GLX may emit errors would be unsafe. MUI's default non-GL path installs
  no handler at all. Hosts/libraries that independently swap XSetErrorHandler
  without participating in the advisory lock cannot be serialized by this fork.
- Rust wait budgets do not impose hard deadlines on native loader pinning,
  Xlib/driver calls, arbitrary destructors, or host callbacks. Pinning is done
  before spawn specifically to avoid an unpinned detach or close-time loader
  lock inversion. Native faults and panic=abort cannot be caught by unwind guards.
- GPU pending-demand check and combined GPU/Windows/macOS integration still
  require coordinator merge and validation. Active Present/vsync pacing and
  push-based MUI model notifications are follow-ups, not implemented here.
