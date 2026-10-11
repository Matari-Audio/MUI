# Windows windowing stability — work package win

Branch: `stab/win`. Worktree: `/home/derpcat/.t3/worktrees/MUI/stab-win`.

This package changes `vendor/moose-baseview/src/platform/win/**` and credits the
upstream ports in `vendor/moose-baseview/README-MUI.md`. It does not change MUI
crates, macOS, shared Win32 wrappers, or the fork's rendering API.

The coordinator approved two dependency cherry-picks. Local `1e1c9bec` contains
upstream worker commit `8dcc79be`, the shared frame-demand API. Local `db4acb7d`
contains worker commit `19adc751`, the X11 requester method. I made no independent
edits to those workers' files. The second dependency was necessary for Linux
library tests to compile.

## Changes and reasons

### Bug 2: crash on open, close, or unload

- `callback.rs::guard` contains Rust unwinds before the ABI boundary. It uses
  `catch_unwind`, a safe fallback, and one diagnostic per linked image. It also
  prevents a panic payload destructor or tracing subscriber from unwinding.
- `native.rs` owns HWND creation and lifetime. Its window procedure is guarded.
  Hook dispatch and COM drag events use this same ownership implementation.
  Temporary `Rc` references protect callbacks against reentrant destruction.
  `WM_NCDESTROY` clears userdata before releasing the window-owned reference.
  Failed creation also recovers an attached reference if Windows omits cleanup.
- Normal close drops the handler before destroying the HWND. External destruction
  revokes frame posting, the keyboard hook, the drop target, and IMM32 association.
  Handler teardown and host destruction notification also contain panics.
- Creation calls `pin_current_image_for_detached_work` BEFORE publishing callback
  pointers or starting the pacer. Pin failure rejects creation. The pin keeps the
  DLL mapped for the process lifetime. This is an intentional memory-lifetime
  trade-off, not a claim that Windows physically unloads the image after close.
- `FramePacer::drop` revokes its HWND target before waiting. It uses
  `WaitForSingleObject` on the native thread handle with a 25 ms timeout.
  It joins only after kernel completion, including thread-local destructors.
  On timeout, it detaches from the already-pinned image. The worker retains only
  its own pacing state. It cannot call handlers, access host state, or post again.
- Frame messages contain an image marker and a window serial. A different editor
  rejects stale messages even if Windows recycles an HWND.
- Class names contain the DLL image address and a per-image serial. Each window
  owns a separate registration. No shared class-registration counter is necessary.
  The old wrapper already used UUID names, not one fixed class name. Its lifetime
  was the problem: unregister could run during `WM_DESTROY`, while the HWND existed.
  The new registration remains in shared window state beyond `DestroyWindow`.
  Unregister failures produce a diagnostic. The pinned image remains safe if a
  retained context delays class destruction or Windows rejects unregister.
- `HOOK_STATE` now stores one hook and window table per GUI thread, per image.
  Last-window removal unhooks that thread's hook. Other threads remain independent.
  All registry locks tolerate poisoning. No handler or diagnostic runs under the
  registry lock. Failed unhook retains an inert, window-free entry in pinned code.
- A same-process thread hook passes NULL for `SetWindowsHookExW`'s module argument.
  The old host EXE module did not identify our callback image.
- Drop-target callbacks return S_OK with no accepted effect on panic, missing data,
  or late calls. Revocation prevents a retained COM object from accessing userdata.
  COM clients can still release retained objects safely because the image is pinned.
  Drag data now releases its `STGMEDIUM` on all return paths.
- The optional OpenGL bootstrap destroys its temporary HWND after GL/DC guards,
  before the shared wrapper releases the temporary class. Errors do not leak it.
- Keyboard layout conversion uses checked slices. The callback lint exemptions,
  explicit unreachable branches, hook unwraps, and clipboard panic stub are gone.
  Clipboard copying publishes eager UTF-16 memory. It installs no delayed callback.

### Bugs 1 and 3: first open and invisible editor

- Ported [baseview #345](https://github.com/RustAudio/baseview/pull/345).
  Unaware hosts use 96 DPI. Per-monitor-v1 hosts control scale through their hints.
  Only v1 enables manual non-client scaling. Initial sizing reads parent DPI and
  retains MUI's scale override. Invalid or unrepresentable DPI hints return errors.
  The fork already contained the upstream thread-context restore correction.
- Ported [baseview #351](https://github.com/RustAudio/baseview/pull/351).
  Initial host resize waits for visible `WM_SHOWWINDOW`. Hidden notifications do
  not consume the request. Rejection restores the OLD physical size and reports
  that size to the handler, rather than reporting the rejected new size again.
  Resize requests made before parenting also update the deferred initializer.
- Added `WS_CLIPCHILDREN | WS_CLIPSIBLINGS` to editor styles. Child creation remains
  `WS_CHILD`, without `WS_VISIBLE`, until handler creation succeeds. `show()` uses
  `SW_SHOWNOACTIVATE` after initialization. It explicitly invalidates and paints
  before the queued renderer callback. This avoids visible half-created windows.
- The child does not use `WS_EX_NOREDIRECTIONBITMAP`. The GDI/CPU presentation path
  still needs ordinary HWND redirection.
- `WM_ERASEBKGND` returns nonzero. `WM_PAINT` validates through BeginPaint/EndPaint
  before invoking the handler. The first paint fills the update region with a solid
  dark background. Later paints do not erase presented content.
- A successful `on_frame` stops the placeholder fill. Baseview has no first-present
  acknowledgment. An OK callback is NOT proof of GPU presentation. The initial
  placeholder remains until a presenter overwrites it.
- Hidden children suspend the pacer. Due work behind hidden or iconic ancestors
  remains pending and checks visibility at 50 ms intervals. Settled Idle demand
  has no periodic wake. A minimize race cannot discard the requested frame.
- Zero-size windows remain zero-size until the host supplies a real extent. The
  windowing layer does not invent surface dimensions or bypass GPU initialization.

### Focus, keyboard, and capture

- Capture is false before the first frame. Only a user mouse press implicitly
  acquires focus. Showing, resizing, and enabling capture do not acquire focus.
  Explicit `WindowContext::focus()` remains available to callers.
- Capture changes update the hook immediately. Repeated unchanged capture values
  no longer post focus messages every frame. Releasing capture returns focus to
  the parent only if the child still owns focus.
- The hook only intercepts keys for a registered HWND on its thread, with capture
  enabled and actual HWND focus. Otherwise it preserves the host message pump.
  Keys addressed to a focused child with capture disabled target the host parent.
  Captured keys still dispatch before Ableton/FL-style host interception.
- Ported [baseview #347](https://github.com/RustAudio/baseview/pull/347).
  `WM_CAPTURECHANGED` resets the mouse-button counter. This adaptation does not call
  ReleaseCapture after capture moves elsewhere, which would release the new owner.
- After close, retained contexts cannot post close, keyboard, or IME requests to a
  recycled HWND. IMM32 teardown disassociates this child only. It does not destroy
  the host thread's default input context or cancel another editor's composition.

## For the coordinator: cfg dispatch

The exact platform entry point is:

```rust
platform::win::WindowHandle::frame_requester(&self) -> crate::FrameRequester
```

Change the common `Window::frame_requester()` dispatch to return
`Some(self.inner.frame_requester())` on Windows. I did not edit common dispatch.
Until that change, the Windows requester methods produce expected dead-code warnings.

`FrameRequester` retains only `Arc<FrameSignal>`. Requests from any thread signal
its condition variable. They do not dereference Rust window memory or post to an
unverified HWND. The pacer serializes posting with target revocation.

`BaseviewWindow::update_frame_demand` queries the handler after events and window
messages. This includes frames, resize, show, and COM drag events. It uses the
shared `FrameDemand` type:

- `Continuous`: DwmFlush cadence, with the existing bounded sleep fallback.
- `Idle`: an untimed condition-variable wait when no explicit frame is pending.
- `At(deadline)`: one deadline wait and one frame. A consumed identical deadline
  cannot create a tight loop if the handler reports it again.
- An input, expose, or explicit request wakes one frame even with Idle demand.
  Requests coalesce while one frame is queued or executing. A request during that
  callback survives for the next frame.

Normal editor creation and hook dispatch no longer call the unguarded generic
`wrappers::win32::window::wnd_proc` or `create_window`. COM no longer uses its
`WindowData`. The optional WGL dummy uses Windows' own `DefWindowProcW`, not a
Rust callback. No SetTimer/TIMERPROC exists on this Windows path.

Suggested merge cleanup outside my ownership: remove the unused generic
`window::create_window`, `window/data.rs`, and `window/proc.rs`. Retain `WindowImpl`,
which the new native layer uses. Gate the legacy UUID/class helper to OpenGL,
where the dummy window still uses it. Remove unused activating resize/show methods
from `window/handle.rs`. These dead-code warnings are newly exposed, not suppressed.
Do not restore a call to the old generic procedure without an ABI panic guard.

## For the GPU worker

- A live child HWND is not proof of a configured or presented surface. Wait for
  parenting, nonzero physical client size, and the effective DPI strategy.
  Request another frame after asynchronous GPU preparation completes.
- Use the Windows requester after the coordinator adds common cfg dispatch.
  The requester is thread-safe, not a real-time audio API. It acquires a short
  platform mutex. Keep it outside audio callbacks and preserve atomic audio transport.
  An editor can now sleep indefinitely after it reports Idle. GPU recovery,
  model changes, accessibility, and initialization completion need explicit wakes
  or future `At` demand. Do not depend on a permanent display-rate callback.
- Retain the GDI/CPU presenter. The initial native placeholder only demonstrates
  HWND painting. It does not recover a failed GPU surface or display MUI content.
  Baseview lacks a first-present acknowledgment. Do not treat on_frame OK as one.
- Keep clipping styles. Do not add `WS_EX_NOREDIRECTIONBITMAP` or `WS_EX_LAYERED`
  to the child to solve alpha behavior without completed-pixel evidence.
- Start with opaque presentation for a regular HWND swapchain. Do not assume
  composition-swapchain premultiplied alpha also works for CreateSwapChainForHwnd.
  Check the DXGI alpha contract for the chosen backend and surface type:
  https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgifactory2-createswapchainforhwnd
  https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/ns-dxgi1_2-dxgi_swap_chain_desc1
- Inspect parent and ancestor `WS_EX_LAYERED` state when diagnosing invisible
  output. The child style cannot repair the parent's composition method. Test
  layered hosts separately. Their GPU presentation behavior is NOT verified here.
- Keep render/present work outside WM_PAINT and native resize callbacks. WM_PAINT
  validates the region and schedules work. It does not configure or submit wgpu.
- Keep presentation nonblocking on the DAW GUI thread. DwmFlush only runs on the
  helper thread. It does not prove GPU completion or repair WARP shader faults.
- Keep surface/handler destruction before normal DestroyWindow. Forced external
  parent destruction during a handler remains a native-host acceptance case.
  The pinned pacer never retains surfaces, handlers, or pointers to host instances.

## Verification

All cargo commands used the machine-wide lock. Actual package name: `moose-baseview`.

Passed on the final Windows source revision:

```sh
flock /tmp/mui-cargo.lock cargo check -p moose-baseview --manifest-path vendor/moose-baseview/Cargo.toml --target x86_64-pc-windows-msvc --all-targets
flock /tmp/mui-cargo.lock cargo check -p moose-baseview --manifest-path vendor/moose-baseview/Cargo.toml --target x86_64-pc-windows-msvc --all-targets --features opengl,tracing
flock /tmp/mui-cargo.lock cargo clippy -p moose-baseview --manifest-path vendor/moose-baseview/Cargo.toml --target x86_64-pc-windows-msvc --all-targets
flock /tmp/mui-cargo.lock cargo clippy -p moose-baseview --manifest-path vendor/moose-baseview/Cargo.toml --target x86_64-pc-windows-msvc --all-targets --features opengl,tracing
flock /tmp/mui-cargo.lock cargo check -p mui-baseview --target x86_64-pc-windows-msvc --all-targets
flock /tmp/mui-cargo.lock cargo clippy -p mui-baseview --target x86_64-pc-windows-msvc --all-targets
flock /tmp/mui-cargo.lock cargo test -p moose-baseview --manifest-path vendor/moose-baseview/Cargo.toml --lib
rustc --edition=2021 --test /tmp/mui-win-policy-tests.rs -o /tmp/mui-win-policy-tests
/tmp/mui-win-policy-tests
rustfmt --edition 2021 vendor/moose-baseview/src/platform/win/*.rs
git diff --check
graft build
```

Clippy exited successfully WITH warnings. These include existing IME numeric/test
warnings, the shared API's exhaustive-enum warning, the pending requester dispatch,
and the obsolete wrapper helpers. This is not a clean `-D warnings` claim.

The Linux library suite passed 22 tests. The standalone policy harness passed 15
additional tests. It imports the actual `frame_state.rs` and `callback.rs` sources.
It copies the exact shared FrameDemand definition and uses the default no-op
tracing macro. It covers panic defaults/payloads, DPI hint bounds, class names,
last-window hook removal, visible-show negotiation, Idle/Continuous/deadline
policy, coalescing, hidden work retention, and requester revocation.

Windows `--all-targets` checks compiled the native unit tests. They did NOT execute
those tests. The standalone harness does not exercise any Win32 ABI or OS call.

To regenerate the standalone harness from this checkout:

```sh
python3 - <<'PY'
from pathlib import Path
s = Path('vendor/moose-baseview/src/handler.rs').read_text()
start = s.index('#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]')
end = s.index('\npub trait WindowHandler', start)
r = Path.cwd() / 'vendor/moose-baseview/src/platform/win'
Path('/tmp/mui-win-policy-tests.rs').write_text(
    s[start:end] + '\n'
    + f'#[path = "{r}/frame_state.rs"] mod frame_state;\n'
    + f'#[path = "{r}/callback.rs"] mod callback;\n'
    + '#[macro_export] macro_rules! error { ($($args:tt)*) => { { let _ = ($($args)*); } }; }\n'
)
PY
rustc --edition=2021 --test /tmp/mui-win-policy-tests.rs -o /tmp/mui-win-policy-tests
/tmp/mui-win-policy-tests
```

## NOT verified — required Windows acceptance

- No Windows binary ran here. No native Win32 test, DLL unload test, or DAW test ran.
- Real open/close/reopen/unload in Ableton, FL Studio, Bitwig, Reaper, and Cubase.
- Two editors on one GUI thread and on two GUI threads. Close them in both orders.
- Two distinct MUI DLL images in one host, including one closing before the other.
  Check class deregistration and last-thread-hook removal with native diagnostics.
- DPI-unaware, system-aware, per-monitor-v1/v2, mixed-hosting, and 125%/150%/200%
  displays. Check initial show, host resize rejection, moving monitors, and overrides.
- A layered host parent, parent hide/show, minimize/restore, zero-size attachment,
  and forced parent destruction inside a callback. Check completed GPU/CPU pixels.
- Actual idle CPU wake counts and Deadline/Continuous cadence. The tests prove
  policy outcomes, not compositor timing. Due work behind hidden ancestors can
  still poll every 50 ms until it becomes viewable.
- A DwmFlush stall and native TLS cleanup. Check the 25 ms timeout and confirm
  that detached work never posts after close. Pinning prevents physical DLL unload.
- Retained IDropTarget calls after revoke, late Release, and actual drag/drop.
- Japanese/Chinese/Korean IMM32 composition in two editors on one thread.
  Check focus loss, candidate placement, field switches, and close mid-composition.
- DAW key interception, held keys across focus changes, host shortcuts with capture
  disabled, stolen mouse capture, Unicode clipboard copying, and optional WGL reuse.
- Native access violations, driver faults, aborting panic hooks, and panic=abort
  cannot be recovered by Rust catch_unwind. Cross-compilation does not prove safety
  against those failures.
