# macOS windowing stability work

Branch: `stab/mac`. Ownership: `vendor/moose-baseview/src/platform/macos/**`,
`src/wrappers/appkit/**`, and minimal AppKit module-root wiring. No `crates/**`
or AccessKit source edits. The shared API commits `8dcc79be` and `19adc751`
were cherry-picked with coordinator approval; there are no additional common-file edits.

## Changes and reasons

- **Bug 2 (DAW crash), unload and multi-image safety:** Objective-C IMPs now use
  `extern "C"`, with `catch_unwind`, selector-specific safe defaults, and a
  callback-local autorelease pool. Timer/notification/main-queue blocks have
  the same guard. A single containment diagnostic is emitted per image
  (tracing, or stderr when tracing is disabled); the host's panic hook is not
  replaced. Panic payloads, the containment logger and handler teardown are
  guarded against a second Rust unwind. Panicking window handlers are quarantined/closed instead
  of being retried every refresh. PR #341's missing Rust-entry autorelease pools
  were ported to context/window/GL operations. Autoreleased selector return
  values leave our pool as retained objects, then autorelease into the caller's
  pool; they are not returned dangling.
- **Bug 2, close/unload/reopen:** pin the current image before registering the
  first NSView class. A failed pin is an editor-creation error, not an unsafe
  fallback. CFUUID suffixes and one cached class per Rust view type per image
  remain; classes are NEVER disposed. This deliberately retains code/class
  metadata for the process lifetime, not editors/renderers. Apple dyld source
  `dyld/DyldAPIs.cpp:1405-1419,1444-1453` confirms RTLD_NODELETE's leaveMapped
  policy (and currently allows dlopen of the macOS main executable); the pin
  also intentionally retains its dlopen handle. This is source confirmation,
  not a native unload test. Hot replacement at the same library path is NOT
  promised: use a host restart or distinct versioned image paths. This pins the
  image containing baseview, normally the statically linked plugin bundle; it
  does not prove safety for callbacks in separate dynamically loaded libraries.
  MUI std TLS destructors first registered BEFORE any view creation are outside
  this guarantee; memo cleanup alone cannot unregister those destructors. The
  coordinator must establish a plugin-load-time policy for that earlier phase.
  Close revokes wakes
  first, invalidates display links and both timer types, unregisters every
  window observer, removes our tracking area, cancels IME focus and restores
  the previous responder BEFORE handler teardown. Late callbacks check closed,
  attachment, visibility and/or the current deadline. Active callbacks retain
  the native view. Handler close is deferred until active borrows finish;
  `WillClose` then handler/surface destruction run without holding the handler
  container borrow, before the native view can deallocate. Close is idempotent.
  Failed initialization tears down a partially attached editor.
- **Issue #124 / retain cycles:** #124 is an ISSUE, not a PR. `gh pr view 124`
  correctly reports no PR. Its final comment says #262 fixed the old
  `retainCount`-based cleanup; our fork already contains weak WindowContext /
  WindowHandle references and explicit close, not that old algorithm. The
  remaining actual cycle was CADisplayLink retaining its target during detach.
  We now invalidate it on detach/close and on pacing suspension. Observer and
  timer closures use weak view references. Cross-thread requesters carry IDs
  and atomic flags, not native objects. Standalone teardown explicitly clears
  delegate/contentView. Debug builds maintain `LIVE_VIEWS` and emit a tracing
  debug count at BaseviewView drop; a host retaining a detached view will
  intentionally delay that count's decrement, but cannot keep a handler active.
- **Bugs 1, 3, 4 (first open, invisible/blurry editor, black flash):** override
  `makeBackingLayer` with CAMetalLayer and `wantsUpdateLayer`, set `wantsLayer`
  BEFORE insertion/handler construction, and fail creation if the backing layer
  could not be established. Layer/view opacity is YES; scale tracks AppKit backing
  changes/window moves; initial colour is AppKit's resolved window background,
  not transparent/black. Frame geometry belongs to AppKit; drawable geometry
  belongs to the renderer. Positive-size gating defers rendering a zero-size
  editor; whole-frame/bounds setters, `setFrameSize:` and autoresizing wake it
  when the host supplies size.
  The flipped child is top-anchored using its superview's BOUNDS and isFlipped,
  including non-zero bounds origins. Standalone content views are NOT reanchored
  into their window's titlebar. Programmatic resize suppresses the native resize
  hook so its notify-host policy is not accidentally duplicated.
- **Focus stealing:** attaching a child no longer calls makeFirstResponder or
  changes the DAW window's acceptsMouseMovedEvents. A view-local NSTrackingArea
  handles visible-rect move/enter/exit/drag/cursor traffic. Mouse-down or explicit
  keyboard capture acquires focus; capture release/detach/close restores it.
  Ignored keys use the fixed NSView superclass, not a dynamic AccessKit subclass
  (which could recurse into our own IMP). Cursor hit testing uses local bounds.
- **Frame pacing:** Continuous uses the view display link on macOS 14+, or the
  existing main-run-loop refresh timer on older macOS (NOT CVDisplayLink).
  Idle invalidates the pacer, with no refresh polling. At(deadline) arms one
  common-mode run-loop timer, then requests one display-paced frame and asks
  for demand again. Input/resize/show/exposure and explicit wakes request a
  frame. Hidden/minimized/occluded/detached/zero-size editors have no pacer or
  deadline timer; a pending request is retained for their next reveal/attach.
  Window notifications are scoped to our current NSWindow, not process-wide
  window traffic or the DAW's delegate. Requests from any thread are coalesced
  onto the main dispatch queue; revocation is checked before posting AND after
  dispatch. The main-thread weak-view registry is consulted without holding
  its borrow during callbacks. No dispatch_sync, render worker, or join is added.

## For the GPU worker

- Keep the raw AppKit NSView target. Its ROOT backing layer is CAMetalLayer;
  do not replace that layer or add a second presentation sublayer. Runtime
  Objective-C class lookup avoids adding renderer-specific Cargo features.
- Confirmed in local registry: wgpu-hal 30.0.1 `metal/mod.rs:149-186` routes
  AppKit through raw-window-metal 1.1.0. `raw-window-metal/src/lib.rs:330-353`
  adopts an existing root CAMetalLayer; `from_ns_view:398-425` makes the view
  layer-backed and passes that root through the same path. Otherwise the helper
  creates an observer sublayer. We deliberately avoid the latter path.
- AppKit/baseview own point-space frame/bounds and backing contentsScale. The
  GPU worker owns drawableSize, surface configuration, device/pixel format,
  colour space and first-frame presentation. Do not configure zero-size
  surfaces. Resize/backing-change callbacks must still update physical extent.
- The native neutral background does NOT prove completed GPU presentation and
  cannot hide a stalled driver/pipeline build. First-open GPU/CPU work still
  needs the GPU worker's bounded/non-blocking initialization policy.
- Drop the surface in handler teardown. The view remains alive for active
  callbacks and externally retained host views; do not rely on NSView dealloc
  to release your GPU resources. After close, no event/frame reaches the handler.
- `crates/mui-baseview/src/platform.rs::reanchor_to_superview_top` is read-only
  here. The fork now handles top anchoring natively on insertion/autoresize/
  resize, including flipped parents. The crate helper SHOULD be removed: it
  unconditionally applies bottom-origin arithmetic every tick, so on a flipped
  parent it can undo the correct native anchor. Please coordinate that change.
- Poll-based models should return At(deadline), not Idle; Idle means no periodic
  polling for automation/accessibility/device loss. The shared GPU Requests
  wake must use the coordinator's macOS frame_requester dispatch once merged.

## For the coordinator: cfg dispatch

`platform::macos::window::WindowHandle::frame_requester(&self) -> FrameRequester`
is implemented. Add macOS to common `Window::frame_requester` dispatch and
wrap it in Some, just as Linux. The platform function routes through
`BaseviewView::frame_requester(ViewRef)` and `BaseviewView::request_frame(ViewRef)`.
No common files were changed beyond the approved shared-API cherry-picks.
Until that cfg dispatch is merged, public Window::frame_requester still returns
None on macOS. Reasoned dead-code allowances cover that temporary seam.

## Verification

All commands below passed from the worktree root. Every Cargo command used
`flock /tmp/mui-cargo.lock`. Baseview uses its standalone manifest so its own
minimal feature set and native test cfg are checked, rather than relying on
workspace feature unification.

```sh
flock /tmp/mui-cargo.lock cargo check -p moose-baseview --manifest-path vendor/moose-baseview/Cargo.toml --target aarch64-apple-darwin --all-targets
flock /tmp/mui-cargo.lock cargo check -p moose-baseview --manifest-path vendor/moose-baseview/Cargo.toml --target x86_64-apple-darwin --all-targets
flock /tmp/mui-cargo.lock cargo check -p moose-baseview --manifest-path vendor/moose-baseview/Cargo.toml --target aarch64-apple-darwin --all-targets --all-features
flock /tmp/mui-cargo.lock cargo check -p mui-baseview --target aarch64-apple-darwin
flock /tmp/mui-cargo.lock cargo clippy -p moose-baseview --manifest-path vendor/moose-baseview/Cargo.toml --target aarch64-apple-darwin --all-targets --all-features
flock /tmp/mui-cargo.lock cargo clippy -p mui-baseview --target aarch64-apple-darwin --all-targets
flock /tmp/mui-cargo.lock cargo test -p moose-baseview --manifest-path vendor/moose-baseview/Cargo.toml --lib --locked --offline
rustc --edition=2021 --test /tmp/mui-audit/mac-policy-test.rs -o /tmp/mui-audit/mac-policy-test
/tmp/mui-audit/mac-policy-test
git diff --check
```

- Ten pure policy/lifecycle tests passed on Linux via the rustc harness importing
  the REAL macOS `policy.rs`. Only the three-case shared FrameDemand enum is
  supplied by the harness. Coverage: Idle/wake, deadlines/stale timers,
  visibility/revocation gating, top anchoring, deferred-close/panic cleanup,
  independent editor slots, coalesced wakes and a background wake after close.
- The native Linux baseview library suite passed 22 tests; it does NOT execute
  any AppKit code. Two AppKit callback-guard tests are cross-compiled, not run.
- macOS baseview Clippy has no findings in files changed by this work package.
  It still reports inherited `ime.rs`/test warnings and the shared FrameDemand
  exhaustive-enum warning; common files are owned by other workers. mui-baseview
  Clippy passed without warnings. Linux tests have inherited xim lifetime warnings.
- Scoped rustfmt and `git diff --check` passed. Vendor graft graph refreshed.

To reproduce the Linux-only harness, put the shared enum in a temporary Rust
file and import policy.rs by its absolute path:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameDemand { Continuous, Idle, At(std::time::Instant) }
#[path = "/absolute/worktree/vendor/moose-baseview/src/platform/macos/policy.rs"]
mod policy;
fn main() {}
```

## NOT verified

- No macOS code was executed. Cross-compilation is not native/DAW validation.
- Native macos-15 CI, Objective-C autorelease/retain/dealloc ordering under
  AccessKit/KVO, actual LIVE_VIEWS decrements, Instruments/Metal memory release,
  and callbacks after dlclose/plugin unload. The two AppKit panic/payload
  regression tests are type-checked only; run the native library suite in CI.
- Real DAW open/close/unload/reopen, two instances of one plugin, and two
  independently linked plugins closed in every order; focused text/host shortcut
  behavior, IME, tracking under host sheets and inactive windows.
- Completed GPU/CPU pixels and first-open flash, Retina/display transitions,
  zero-size-to-visible construction, flipped parents and live resize.
- macOS <14 timer pacing, macOS 14+ display link invalidation/occlusion,
  deadline accuracy, main-queue coalescing and cross-thread close/wake races.
- End-to-end frame requester through the COMMON cfg seam (coordinator pending).
- Pre-view MUI TLS destructors, callback images in separately loaded dylibs,
  hot library replacement at the same path, and complete plugin-load policy.
- Native faults, foreign Objective-C exceptions, OOM, driver faults and
  panic=abort cannot be recovered with Rust catch_unwind. A double panic
  inside caller code before control reaches our guard also aborts.

Sources: RustAudio/baseview PR #341 diff; issue #124 and its #262 resolution;
local objc2/AppKit bindings (0.3.2) and Context7 API references;
wgpu-hal/raw-window-metal registry source; the supplied MUI audits.
