---
status: in-progress
project: frameworks/mui
owner: claude-opus-5-5
created: 2026-11-25
updated: 2026-11-25
proof_scope: external-api
evidence_freshness_days: 180
---

# X11: a focus loss keeps the click

## Problem & Goal

- **Problem:** A user on Ubuntu 26.04 (GNOME, REAPER under XWayland,
  embedded X11 editor) reports the editor draws but no click does anything.
  Each click asks for the keyboard (`take_focus`, `SetInputFocus` on the
  child). A window manager that keeps the X focus on its managed toplevel
  sets it back, so the child gets `FocusOut` with a detail other than
  `Pointer` while the button is still down. baseview turned that into
  `WindowEvent::Unfocused`; MUI's `Driver::focus(false)` cancels the gesture
  in flight and forgets the pointer position. The click never completes, and
  the next click with no motion before it lands nowhere. #190 only skipped
  the `Pointer`-detail `FocusOut`.
- **Reproduction:** REAPER 7.86 on Xvfb with the editor embedded, plus a
  helper client that sets the focus back to the REAPER toplevel on the
  child's `FocusIn`. Every XTest click changed 0 pixels; without the helper,
  clicks work.
- **Second loss (MUI core):** baseview then delivers press, `Focused`,
  release, `Unfocused`, `CursorMoved`. `Driver::focus(false)` always queued
  `Pending::Cancel`, whose `Ui::cancel` resets `Interaction`, wiping the
  release's `clicked` response before the next `build` reads it. Widgets
  (KONTRA's tabs, MUI buttons) read `ui.get(id)` in `build`, so the click
  was gone; the earlier core tests read it in `after_frame` and missed this.
- **Goal:** A focus loss while a button is held is delivered after the last
  release; after any `Unfocused` the pointer position is re-sent. In MUI
  core, a focus loss with no button held forgets the keys but cancels
  nothing and keeps the pointer: only a gesture still in flight is
  cancelled. A
  `MUI_INPUT_TRACE=1` stderr trace of pointer and focus events lets a user
  show where their clicks go.

## Primary-Source & Version Proof

| Standard / Vendor API | Pinned Version / Spec Rev | Primary-Source URL | Verified Date | Key Invariants / Deprecations Checked |
|---|---|---|---|---|
| X11 core protocol: `ButtonPress` implicit grab, `FocusIn`/`FocusOut` details | X11R7.7 | https://www.x.org/releases/X11R7.7/doc/xproto/x11protocol.html | 2026-11-25 | A press activates a pointer grab for the client that selected it; the release goes to that window whatever the focus does |

## Pinned Crate & SDK Ground Truth

| Dependency / Tool | Pinned Version | Lockfile / Header Source | Verified Symbols / Features |
|---|---|---|---|
| `x11rb` | 0.13.2 | `frameworks/mui/vendor/moose-baseview/Cargo.lock` | `FocusInEvent`, `ButtonPressEvent`, `KeyButMask`, `NotifyDetail` |

## Invariants & Non-Goals

### Hard Invariants
- A focus loss is never dropped: it is delivered at once, or right after
  the last held button's release, unless focus comes back first.
- Keyboard state (pressed keys, forwarded keys, IME) is released exactly as
  before when `Unfocused` is delivered.

### Non-Goals
- Changing what a focus loss does while a button is held: it still cancels
  that gesture on every platform.
- Not taking focus on click at all: a host that leaves the keyboard to the
  pointer needs it (#190).

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Regression fails before, passes after | Validator/Runtime | linux-x86_64 | `DISPLAY=:97 build-slot cargo test --lib -- --ignored --test-threads=1 native_a_window_manager` (in `vendor/moose-baseview`, under Xvfb) | Before: `["press","focused","unfocused","release"]` panics; after: ok | PASS (2026-11-25, local CachyOS, PR #198) |
| #190 focus test unchanged | Validator/Runtime | linux-x86_64 | same, `native_a_click_takes` | ok | PASS (2026-11-25, local CachyOS, PR #198) |
| Clippy | Compile/ABI | linux-x86_64 | `build-slot cargo clippy --all-targets -- -D warnings` | clean | PASS (2026-11-25, local CachyOS, PR #198) |
| A click read by the next build survives a focus loss after its release | Validator/Runtime | linux-x86_64 | `build-slot cargo test -p mui --lib keyboard_capture_tests` | `a_refocus_mid_click_then_focus_loss_after_release_is_one_click` and `a_focus_loss_after_the_release_keeps_the_click_*`: one click seen by `build`; before the core fix `built` was 0 | PASS (2026-11-25, local CachyOS, PR #198) |
| Experiment: deferred `Unfocused` never sent | Validator/Runtime | linux-x86_64 | scratch `e2e.sh` with a temporary switch skipping the deferred `focus_out` | tells `Focused`-while-held from the cancel path | PASS (2026-11-25, local CachyOS, PR #198: tab switched, so the cancel path) |
| End to end: REAPER 7.86 + editor on Xvfb, focus-reclaiming helper, 150 ms XTest click on a tab | Validator/Runtime | linux-x86_64 | scratch `e2e.sh` (REAPER `-cfgfile`, `focus_fighter.py`) | tab switches | PASS (2026-11-25, local CachyOS, PR #198: the trace shows press, Focused, release, Unfocused, CursorMoved and Rack→Mixer switches, 859 px changed; the baseview-only build changes 5 px) |
| GNOME on real hardware | Hardware/OS-CI | Ubuntu 26.04 GNOME + REAPER | user runs the build with `MUI_INPUT_TRACE=1` | clicks work | PENDING_OS_CI (ubuntu-gnome) |

## Unverified Boundaries & Promotion Gate

- **Resolved:** the experiment (no deferred `Unfocused`) made the tab
  switch, so the loss was the cancel path, not the `Focused` while held.
  Fixed in `Driver::focus` (`crates/mui/src/host.rs`).

- The live X11 tests share one display's focus: run them with
  `--test-threads=1`, or the #190 test can see the other's focus change.

- Mutter's exact refocus behaviour is emulated, not run: XTest does not move
  the pointer in rootless Xwayland under headless weston or kwin here. The
  user's trace confirms or refutes it (`focus-out ... detail=...` lines
  between `press` and `release`).
