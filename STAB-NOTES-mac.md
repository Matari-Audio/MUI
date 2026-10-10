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
  replaced. Panic payloads and panicking logger/destructors cannot cause a
  second Rust unwind. Panicking window handlers are quarantined/closed instead
  of being retried every refresh. PR #341's missing Rust-entry autorelease pools
  were ported to context/window/GL operations. Autoreleased selector return
  values leave our pool as retained objects, then autorelease into the caller's
  pool; they are not returned dangling.
- **Bug 2, close/unload/reopen:** pin the current image before registering the
  first NSView class. A failed pin is an editor-creation error, not an unsafe
  fallback. CFUUID suffixes and one cached class per Rust view type per image
  remain; classes are NEVER disposed. This deliberately retains code/class
  metadata for the process lifetime, not editors/renderers. Close revokes wakes
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
  could not be established. Layer opacity is YES; scale tracks AppKit backing
  changes/window moves; initial colour is AppKit's resolved window background,
  not transparent/black. Frame geometry belongs to AppKit; drawable geometry
  belongs to the renderer. Positive-size gating defers rendering a zero-size
  editor; `setFrameSize:`/autoresizing wakes it when the host supplies size.
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
None on macOS. Three reasoned dead-code allowances cover that temporary seam.

## Verification (updated after final checks)

Passed so far:

- aarch64 Apple standalone baseview all-target check (default AND all features).
- `cargo check -p mui-baseview --target aarch64-apple-darwin`.
- Seven pure policy/lifecycle tests on Linux via a rustc harness importing the
  real macOS policy module (only the shared FrameDemand enum is supplied by the
  harness). They cover Idle/wake, At, visibility/revocation gating, top anchoring,
  deferred close ordering/panic cleanup and independent editor slots.
- `git diff --check` and scoped rustfmt.

## NOT verified

- No macOS code was executed. Cross-compilation is not native/DAW validation.
- Native macos-15 CI, Objective-C autorelease/retain/dealloc ordering under
  AccessKit/KVO, actual LIVE_VIEWS decrements, Instruments/Metal memory release,
  and callbacks after dlclose/plugin unload.
- Real DAW open/close/unload/reopen, two instances of one plugin, and two
  independently linked plugins closed in every order; focused text/host shortcut
  behavior, IME, tracking under host sheets and inactive windows.
- Completed GPU/CPU pixels and first-open flash, Retina/display transitions,
  zero-size-to-visible construction, flipped parents and live resize.
- macOS <14 timer pacing, macOS 14+ display link invalidation/occlusion,
  deadline accuracy, main-queue coalescing and cross-thread close/wake races.
- End-to-end frame requester through the COMMON cfg seam (coordinator pending).
- Native faults, foreign Objective-C exceptions, OOM, driver faults and
  panic=abort cannot be recovered with Rust catch_unwind.

Sources: RustAudio/baseview PR #341 diff; issue #124 and its #262 resolution;
local objc2/AppKit bindings (0.3.2) and Context7 API references;
wgpu-hal/raw-window-metal registry source; the supplied MUI audits.
