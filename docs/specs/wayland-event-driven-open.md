---
status: in-progress
project: frameworks/mui
owner: claude-opus-5-5
created: 2026-11-25
updated: 2026-11-25
proof_scope: internal
evidence_freshness_days: 180
depends_on: x11-event-driven-open.md
---

# Wayland: the editor opens event-driven, with no host-side deadlines

## Problem & Goal

- **Problem:** `layer::WindowHandle::spawn` (moose-baseview, native Wayland:
  floating `xdg_toplevel` editors and wlr-layer-shell docks) blocked the
  caller in `recv_timeout(3 s)` while the owner thread connected, waited for
  the compositor's first configure *and* built the whole handler/UI. A slow
  build froze the host's GUI thread for up to 3 s and then failed the open.
  The commands (`resize`, `title`, `show`, `hide`) were already one-way.
- **Goal:** the same design as [x11-event-driven-open](x11-event-driven-open.md):
  open starts the owner thread and returns; readiness and failure are events.

## Standards

- CLAP `clap_plugin_gui.create/show` run on the host main thread and must
  return, but do not require a built UI. An `xdg_toplevel` must not attach a
  buffer before acknowledging its first `configure` (xdg-shell); that wait
  stays on the owner thread, where it never blocks the host.
- `Embedded` (VST3 `IWaylandFrame` subsurface, CLAP nested embedding) has no
  thread: the host's run loop drives it on its own thread, and a subsurface
  needs no configure. Its open stays synchronous by design. Non-goal.

## Design

- `spawn` publishes the requested size (scale 1 until the compositor reports
  the output's) into `Shared`, creates the command queue (32) and a calloop
  `Ping` on the host side, starts the owner thread and returns.
- The `Ping` replaces the owner's `LoopSignal` on the host side: it exists
  before the loop does, and its eventfd keeps a wake sent before the loop
  starts. Show/hide/visibility/frame requests set their atomics and ping;
  the first `tick` reads them. Commands queued before readiness apply on the
  first dispatch.
- Owner failures (connect, globals, configure, handler build, panic, run
  error) are logged (`error!`), recorded, turn `is_open()` false, and are
  returned by the next `show`/`hide`/command and by `run_until_closed`. A
  failure after the host closed the window is not reported.
- A close before the surface is configured skips the handler build. Close
  keeps its caller-chosen bound (default 250 ms) and then detaches; the owner
  frees the handler when an in-flight build ends.
- mui-baseview `wayland::open` marks the editor open before creating it, and
  fires the close signal when the handler never took it (failed build or
  earlier failure), so moose's floating editor sees an asynchronous failure
  as a close (`requests.wayland_open()` turns false).

## Acceptance Criteria

1. No `recv_timeout`/reply channel in `layer.rs` production code; no sleep,
   recv, `Duration::from_secs` or `is_finished` in any `WindowHandle` host
   call. The only timed host wait is the bounded close in `Drop`. Enforced by
   `layer::tests::host_paths_have_no_deadlines_or_replies`.
2. `size()` answers the requested size before the surface exists
   (`requested_size_is_answered_before_the_surface_exists`).
3. A 2 s handler build: open, show, resize and title return in < 100 ms;
   the queued show maps and draws; the queued resize applies
   (`native_wayland_slow_build_opens_at_once_and_applies_queued_requests`).
4. Close at once and close 0.5 s into a 2 s build: close returns within its
   bound; the handler or builder is freed; nothing draws
   (`native_wayland_close_during_slow_build_frees_everything`).
5. A failed build is reported by the window's run and turns it closed
   (`native_wayland_failed_build_is_reported`).
6. An unbuilt mui window fires its close signal exactly once
   (`wayland::tests::an_unbuilt_window_signals_close_once`).
7. Existing Wayland lifecycle tests pass unchanged.

## Non-Goals

- `Embedded` open (see Standards). The owner thread's 2 s first-configure
  bound is a worker-side compositor bound, not a host deadline; unchanged.

## Verification Matrix

All commands run in `frameworks/mui/vendor/moose-baseview` unless stated.

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| No deadlines or replies on host paths | Validator/Runtime | linux-x86_64 | `nice -n 19 build-slot cargo test --features opengl --lib host_paths_have_no_deadlines_or_replies` | guard passes | PASS (2026-11-25, local CachyOS, PR #305) |
| Unit tests | Validator/Runtime | linux-x86_64 | `nice -n 19 build-slot cargo test --features opengl --lib` | all pass | PASS (2026-11-25, local CachyOS, PR #305: 72 passed) |
| Native Wayland: 2 s slow-build open, close during build, failed build reported, existing subsurface tests | Validator/Runtime | linux-x86_64 | `cargo test --lib --locked native_wayland -- --ignored --test-threads=1` under headless weston (CI: `ui.yml`) | 5/5; open returns at once during the build; queued requests apply when ready | PASS (2026-11-25, local CachyOS, weston 15 headless, PR #305: 5/5) |
| mui-baseview Wayland open | Validator/Runtime | linux-x86_64 | `nice -n 19 build-slot cargo test -p mui-baseview --lib` plus `native_wayland`, `wayland_frame` (in `frameworks/mui`) | all pass; a late build failure reads as closed | PASS (2026-11-25, local CachyOS, PR #305: 46 passed) |
| Clippy | Compile/ABI | linux-x86_64 | `nice -n 19 build-slot cargo clippy --all-targets --features opengl -- -D warnings` | clean | PASS (2026-11-25, local CachyOS, PR #305) |
| Dock (wlr-layer-shell) path | Hardware/OS-CI | KDE / wlroots compositor | open a dock on a layer-shell compositor | opens without blocking | PENDING_OS_CI (layer-shell) |

## Unverified Boundaries & Promotion Gate

- Docks need wlr-layer-shell, which weston lacks; the dock path shares the
  toplevel open code but has no native test here.
- `Embedded` (VST3 subsurface, CLAP nested) runs on the host's loop and
  stays synchronous by design.
