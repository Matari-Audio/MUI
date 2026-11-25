---
status: in-progress
project: frameworks/mui
owner: claude-opus-5-5
created: 2026-11-25
updated: 2026-11-25
proof_scope: internal
evidence_freshness_days: 180
---

# X11: the editor opens event-driven, with no host-side deadlines

## Problem & Goal

- **Problem:** On Linux under REAPER, a user on a busy machine got a blank
  editor. `WindowThreadControl::spawn` (moose-baseview, X11) blocked the
  host's GUI thread in `recv_timeout(OPEN_TIMEOUT = 250 ms)` while the
  worker created the X window *and* built the whole handler/UI. A slower
  build failed the open and the editor was abandoned. Every other host call
  (`show`, `hide`, `resize`, `set_parent`, title, scale) was an RPC with a
  50 ms `REQUEST_TIMEOUT`: the same bug, smaller window.
- **Stopgap superseded:** public MUI#193 raised the open deadline to 5 s and
  let `show()` tolerate a timeout. Any deadline can be missed under load.
- **Goal:** the host call does only what has to be synchronous (start the
  worker and publish answers the host reads back), then returns. Window
  creation, handler build, readiness and failure are events.

## Standards

- CLAP `clap_plugin_gui.create/set_parent/show` and VST3
  `IPlugView::attached` run on the host main thread and must return a
  result, but do not require a built UI. Upstream baseview, JUCE and DPF
  create cheaply and draw when ready.

## Design

- `spawn` publishes the requested size, scale and sizing strategy from the
  settings into `WindowThreadShared`, starts the worker and returns. There
  is no startup channel and no wait.
- Host requests are one-way messages on a bounded queue (32). `Ok` means
  queued. A full queue fails at once (`Busy`); a stopped worker fails at
  once with the failure it recorded (`Closed(reason)`). Messages queued
  before the window is ready apply when the event loop starts.
- Worker failures (X window creation, handler build, panic, run error) go
  to `final_error`, are logged (`error!`), turn `is_open()` false, and are
  returned by the next request and by `run_until_closed`. mui logs a failed
  `show()` through `View::log`.
- Stop handshake: the host's stop and the worker's loop-signal publish both
  take `host_stopped`, so a stop before the signal exists is never lost; a
  worker stopped before its window exists skips the handler build.
- Close: an embedded window still joins (the host parent must outlive every
  use of it, a build in flight included). That wait is the build's own
  length, not a deadline. A floating window's `close_bounded` revokes and
  detaches as before.

## Acceptance Criteria

1. No `recv_timeout`, reply channel, `OPEN_TIMEOUT` or `REQUEST_TIMEOUT` in
   host-facing X11 code; the only clock-based wait is `close_bounded`, with
   a caller-chosen bound. Enforced by
   `window_thread::close_tests::host_paths_have_no_deadlines_or_replies`.
2. Host requests return immediately while the worker is busy, fail fast when
   the queue is full, and report the worker's recorded failure once it has
   stopped (`host_requests_never_wait_for_the_worker`).
3. `size()` answers the requested size before the window exists
   (`requested_size_is_answered_before_the_window_exists`).
4. A 2 s handler build: open, show and resize return in < 100 ms; the window
   becomes ready; the queued show maps it and the queued resize sizes it
   (`native_slow_build_opens_at_once_and_applies_queued_requests`, MUI#193's
   regression intent).
5. Close during a 2 s build: embedded close returns once the build ends,
   frees the handler on the worker and leaves no child under the parent;
   floating close returns at once and the detached worker frees the handler
   when its build ends (`native_close_during_slow_build_frees_everything`).
6. Existing X11 lifecycle tests pass unchanged in intent.

## Non-Goals

- The Wayland layer backend (`platform/layer.rs`) still waits up to 3 s for
  its worker at open; its commands are already one-way. Follow-up.
- mui-baseview's render worker idle tick (`native_worker.rs`) is not a host
  path.

## Verification Matrix

All commands run in `frameworks/mui/vendor/moose-baseview` unless stated.

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| No deadlines or replies on host paths | Validator/Runtime | linux-x86_64 | `nice -n 19 build-slot cargo test --features opengl --lib host_paths_have_no_deadlines_or_replies` | guard passes; fails if `recv_timeout`, `.recv()`, a timeout constant or a reply channel returns | PASS (2026-11-25, local CachyOS, PR #295) |
| Unit tests | Validator/Runtime | linux-x86_64 | `nice -n 19 build-slot cargo test --features opengl --lib` | all pass | PASS (2026-11-25, local CachyOS, PR #295: 70 passed) |
| Native X11 lifecycle, incl. 2 s slow-build open and close-during-build | Validator/Runtime | linux-x86_64 | `xvfb-run -a nice -n 19 build-slot cargo test --features opengl --lib -- --ignored --test-threads=1 platform::x11` | 15/15; open/show/resize < 100 ms during a 2 s build; queued show maps at 80x48; nothing left under the parent after close | PASS (2026-11-25, local CachyOS, PR #295: 15/15 in 4 runs, one with all cores saturated) |
| Clippy | Compile/ABI | linux-x86_64 | `nice -n 19 build-slot cargo clippy --all-targets --features opengl -- -D warnings` | clean | PASS (2026-11-25, local CachyOS, PR #295) |
| moose editor lifecycle | Validator/Runtime | linux-x86_64 | `nice -n 19 build-slot cargo test -p moose-mui` (in `frameworks/moose`) | all pass | PASS (2026-11-25, local CachyOS, PR #295) |
| Busy machine in a real DAW | Hardware/OS-CI | Ubuntu 26.04 + REAPER | user opens the editor while REAPER restores a session | editor never opens blank | PENDING_OS_CI (ubuntu-reaper) |

## Unverified Boundaries & Promotion Gate

- The real-DAW row needs the reporting user's machine; Xvfb with saturated
  cores stands in for it here.
- Wayland (`platform/layer.rs`) still waits up to 3 s at open: tracked as
  follow-up, not covered by this spec.
