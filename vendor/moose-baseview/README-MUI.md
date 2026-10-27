# MUI native text-input patch

This directory is the `crates/moose-baseview` source from Matari-Audio/moose
revision **c1e0eed67159b61aea6e2ffeb905d5b4fbd452b0**, the revision in MUI's
previous lockfile. Its original MIT and Apache-2.0 licenses and README are retained.
Only this crate is vendored; it has no workspace-inherited manifest values.

The Linux `Window::close_bounded` API, bounded join, callback revocation and
regressions are ported from Matari-Audio/moose revision
**bffa4677d0b82119d38566ce7e932dc5c463d497**, under the same original licenses.
MUI also checks revocation while draining its added XIM callback queue. X11
now verifies image pinning **before** spawning the editor thread. An unpinnable
shared library logs a diagnostic and refuses editor window open; this does not
fail plugin instance creation. The main executable cannot unload and needs no
pin. Host requests use a nonblocking bounded queue and a 50 ms reply timeout;
a timeout means the request may still execute, but its reply cannot acknowledge
a different request. First open has an explicit 250 ms handshake budget.

Both ordinary X11 close and `close_bounded` now wait only within their budget
(250 ms by default), including embedded windows. Normal completion sends
`WillClose` and drops the handler before native teardown. A timed-out worker
revokes further callbacks and detaches; pinning keeps its **code** mapped until
process exit, not its host state or parent drawable. Callers must revoke the
handler's host access before close. A callback or driver call already running
cannot be cancelled and may finish native/GPU teardown after close returns.
Arbitrary registered `HostCallbacks` / `HostMainThreadCaller` must obey that
lifetime contract too: pinning does not make stale calls into the host safe.
Code in independently supplied handler/callback images needs its own lifetime
guarantee; the baseview image pin does not preserve those images.
This policy avoids a GUI-thread deadlock; it does not promise immediate resource
release or actual image unmapping on a failed close.

MUI additions expose `Event::Ime` and `WindowContext::set_ime_configuration`.
Configuration includes physical client-relative candidate geometry, surrounding
UTF-8 text, byte selection, and marked range. Platforms ignore identical values:
changing the caret cannot repeatedly disable/enable an active composition.

* Cocoa implements `NSTextInputClient` selectors and `interpretKeyEvents`, converts
  UTF-16 ranges, returns attributed surrounding text and screen candidate bounds.
  A synchronous text shadow covers multiple edits between UI ticks; focus loss
  immediately blocks late mutating callbacks.
* Windows uses IMM32 result/preedit strings and UTF-16 caret conversion,
  associates/disassociates the thread input context only on enable changes,
  positions composition/candidate windows, and serves `IMR_DOCUMENTFEED`.
* X11 uses the maintained MIT-licensed `zed-xim` fork of xim-rs, pinned to
  `16f35a2c881b815a2b6cdfd6687988e84f8447d8`, vendored at `../xim-rs` with an
  acknowledgement callback hook and required preedit-start reply. Its transport shares the original
  owned XCB connection, replaces the input context when field identity or focus
  changes, and processes incremental preedit/caret callbacks. Retired ICs remain
  rejected until their destruction reply; fresh creation waits for that reply.
  Negotiated event masks determine press/release forwarding and synchronous flags.
  Synchronous keys queue until the matching IC acknowledgement, without blocking
  the event loop; stale IC acknowledgements cannot release the current queue.
  Unrequested events retain local keyboard handling. It positions the candidate
  spot and reads `XMODIFIERS` and locale variables; it never mutates the host's environment or
  process-wide locale. An XIM server must be running before opening the window.

The platform mechanism was cross-checked with Apache-2.0 GPUI sources at
`a84689073d296dfd39987bc7dd478e43ef76d83a`, paths
`crates/gpui_macos/src/window.rs`, `crates/gpui_windows/src/events.rs`, and
`crates/gpui_linux/src/linux/x11/client.rs`. No GPUI source code is copied here;
platform glue is newly written against baseview's existing ownership model.

XIM has no standard surrounding-text selection protocol: X11 retains that
configuration for change detection, but only sends supported spot attributes.
This is not a native Wayland backend; XWayland does not provide Wayland
text-input-v3 support. Cocoa explicit replacement ranges are translated to ordered
UTF-8 selection events before their associated preedit/commit. Input identity
changes cancel the preceding native context composition. XIM forwarding follows
[X.Org XIM protocol event masks](https://xorg.freedesktop.org/archive/current/doc/libX11/XIM/xim.html).

## Keyboard capture and pointer gestures

Surrendering text keyboard capture is not a user focus loss. On Windows, a
scoped `WindowState` flag surrounds `SetFocus(parent)` in the posted capture
message handler; its synchronous `WM_KILLFOCUS` does not emit `Unfocused`.
Keyboard routing still follows the hook's capture flag immediately. Cocoa uses
the same scoped guard around restoring the previous first responder, preserving
pointer gestures while disabling native IME ownership. Genuine key-window loss
still emits `Unfocused`, even after that responder handoff. X11 capture setters
are already no-ops because ignored keys propagate to the host; no change is needed.
The guard restores its previous value after nested calls and Rust unwinding, so
later genuine focus losses still reach MUI and cancel the in-flight gesture.

The platform-independent policy tests run on Linux as well:

```sh
cargo test --manifest-path vendor/moose-baseview/Cargo.toml --lib platform::focus_handoff::tests --locked
cargo test -p mui --lib host::tests::keyboard_capture_tests
```

Real Windows/macOS validation: focus a popup search field, press a row and
release without moving the pointer; the first click must work. Also switch
application/window focus during a held press: releasing must not activate the
row. Host keyboard shortcuts must continue working after text capture ends.

## Frame demand and X11 creation

`WindowHandler::frame_demand()` defaults to `FrameDemand::Continuous`, preserving
existing callers. X11 implements `Idle` with no frame timer and `At(Instant)`
with a single paced deadline timer. Input, resize, map restoration, Expose and
a `FrameRequester::request_frame()` through `Window::frame_requester()` wake it. The returned optional
`FrameRequester` is cloneable and thread-safe, does not retain the native window
or X connection, and is harmless after close. Windows/macOS implementations are integrated
separately. MUI reports a slow model-polling deadline because external model,
plugin meter and accessibility changes are not yet push-notified.

Opaque non-GL X11 windows now use the explicit root visual/depth and their own
checked colormap, not arbitrary ARGB32. Creation and reparent are checked XCB
requests. Zero initial extents become 1x1 (X11 cannot create zero-sized windows).
Children may map before their parent; frames wait for the whole ancestry to
become mapped. XEmbed reparent discards the old ancestor chain and recomputes
visibility even when the new parent is the root. Destroyed drawables stop
callbacks. XWayland remains X11, not a native Wayland/text-input backend.

The optional GLX error scope serializes swaps with a poison-tolerant mutex and
an advisory `flock` on `/proc/self`, which is shared across plugin images.
RAII restores the previous handler after XSync even on a Rust panic. Errors on
foreign connections chain to the old handler; the C callback catches panics.
A wedged GLX/XSync call can keep its scope active beyond the close timeout;
restoration happens when it returns, and its code stays pinned meanwhile.
Only GLX needs this global scope; normal windowing/XIM uses checked XCB without
installing an Xlib error handler. A host or unrelated library that independently
changes `XSetErrorHandler` without participating in the lock cannot be made
race-free by this fork. Use of this mutex is not a claim of host cooperation.

## Ported upstream fixes

This vendored fork is MUI's first-party windowing layer. We retain its `on_frame`
contract alongside the current `draw`/`poll`/`damage`/waker/timer APIs. A common
scheduler adapts these APIs onto the existing native lifetime guards and pacers.
`draw` delegates to `on_frame` by default, and legacy `Continuous` demand remains
the default. New idle handlers can override `frame_demand` to `Idle`. See
[UPSTREAM.md](UPSTREAM.md) for exact upstream provenance and adapted behavior.

* [RustAudio/baseview #345](https://github.com/RustAudio/baseview/pull/345):
  Windows DPI virtualization. Unaware hosts remain at 96 DPI; per-monitor-v1
  children honour host scale suggestions, and non-client scaling is enabled
  only for v1. Initial physical sizing uses parent DPI and preserves MUI's scale
  override. The fork already had the upstream thread-context restore fix.
* [RustAudio/baseview #347](https://github.com/RustAudio/baseview/pull/347):
  reset the mouse-button capture counter on `WM_CAPTURECHANGED`. Unlike the
  original patch, we do not release capture after it has moved to a DAW window.
* [RustAudio/baseview #351](https://github.com/RustAudio/baseview/pull/351):
  defer the initial host resize request until the visible `WM_SHOWWINDOW`
  notification, including parents created at non-100% DPI. Resize rejection
  restores the previous physical size and notifies the handler of that size.

Windows publishes only guarded callbacks from `src/platform/win/native.rs`.
It pins the containing image before registering window/hook/COM callbacks:
retained COM clients and a driver blocked in `DwmFlush` can outlive editor close.
This deliberately keeps the DLL mapped for the process lifetime, but normal
close still revokes hooks/drop targets/IME, destroys the HWND, and stops the
pacer (25 ms bounded join, safe detach after target revocation).

## Maintaining this patch

When updating baseview, diff against the exact revision above first, retain the
plugin keyboard hooks, parent-window behavior, original display connection and
handler-before-native-window teardown. Keep native callbacks outside MUI's model
lock. Do not silently replace these APIs with no-ops on an OS.

The standalone `Cargo.lock` files here and in `../xim-rs` pin the regression
builds separately from MUI's workspace lockfiles. `tools/verify.sh root-test`
runs both library suites on Linux; CI also runs baseview's native library suite
on Windows and macOS. CI fetches each standalone lock before testing offline,
so XIM's test-only dependencies do not depend on a developer's Cargo cache.
Update and commit the corresponding standalone lock when changing these manifests.

Run the vendored unit tests and platform checks separately:

```
cargo fetch --manifest-path vendor/moose-baseview/Cargo.toml --locked
cargo fetch --manifest-path vendor/xim-rs/Cargo.toml --locked
cargo test --manifest-path vendor/moose-baseview/Cargo.toml --lib --locked --offline
cargo test --manifest-path vendor/xim-rs/Cargo.toml -p zed-xim --lib --features x11rb-client,x11rb-xcb --locked --offline
cargo check --manifest-path vendor/moose-baseview/Cargo.toml --target x86_64-pc-windows-gnu
cargo check --manifest-path vendor/moose-baseview/Cargo.toml --target x86_64-pc-windows-msvc
cargo check --manifest-path vendor/moose-baseview/Cargo.toml --target x86_64-apple-darwin
cargo check --manifest-path vendor/moose-baseview/Cargo.toml --target aarch64-apple-darwin
cargo test -p mui-baseview
```

## Native acceptance recipe

Run `cargo run -p mui-baseview --example ime` for two editable fields and a
native commit counter. Use the same view embedded under a real host's raw
parent, then repeat after closing/reopening the editor. Start Japanese/Chinese/
Korean system IMEs (IBus/Fcitx XIM on X11, `XMODIFIERS=@im=...` at process startup).
Check that preedit is visible without editing the value, candidate movement
changes the underline/cursor, Enter commits exactly once, Escape cancels, and
ordinary typing/dead keys still work. Try `a😀é`, non-Latin text and selection
replacement; candidates must follow the actual caret at 1x/1.5x/2x, after zoom,
resize, scrolling and parent movement. Switch field focus and native window focus
mid-composition; no stale preedit should reach the newly focused field. With
keyboard capture off, host shortcuts and held-key releases must keep working.
Open two windows, compose in one and disable text input in the other; IMM32 must
not cancel the first window's shared-thread composition. Confirm close/reopen
recreates independent native input contexts and never keeps a dead connection.

Cross-compilation proves API/type compatibility, not native IME usability; these
manual checks remain required on each operating system and real plugin hosts.

## Desktop edge surfaces

`WindowSettings::with_dock(DockOptions::new(DockEdge::Top, 160))` requests
an independent full-length edge surface. The OS/compositor owns the accepted
rectangle. `DockEdge::Bottom` selects the opposite horizontal edge, and
`DockEdge::Left` / `DockEdge::Right` select full-height vertical panels. For
`DockOptions`, `height` is the strip thickness: the panel height on top/bottom
and the panel width on left/right. `with_output` optionally selects a native
output name. Parent/wait-for-parent settings are rejected.

Linux uses a native SCTK layer-shell surface (requiring Rust 1.86) when WAYLAND_DISPLAY is present,
with its own toolkit-owned queue, on-demand focus, fractional-scale viewport,
cursor-shape and text-input-v3 support. Its data-device drag uses the same seat,
surface and initiating serial; outgoing file transfers and incoming URI lists
are bounded, nonblocking and cancelled during close. Ordinary embedded editors
continue using X11/XWayland. Without Wayland, EWMH-capable window managers get a
managed DOCK/STRUT_PARTIAL surface. Unsupported protocols, missing named outputs
and unsafe internal X11 monitor edges return creation errors for the application
to handle explicitly.

Windows uses Shell AppBars and reconciles accepted bounds, activation,
fullscreen, display/DPI changes and Explorer restart. Hide removes registration.
AppKit creates a borderless floating strip, including left and right vertical
strips, and preserves the chosen edge when its thickness changes. macOS has no
work-area reservation in this implementation.

`Window::visibility_control()` is a weak Send/Sync request handle. It posts to
the native owner without invoking product callbacks or extending window life;
Windows validates a generation token before acting on a posted native message.
The Wayland controller coalesces visibility and wakes its native queue. Its
close waits at most the configured close budget (250 ms by default), releases
its exclusive zone immediately and rejects further product input. A stalled
callback retains its handler/native resources and pinned image until retirement,
subject to the same host-access revocation contract described above for X11.

Local probe: `cargo run --manifest-path vendor/moose-baseview/Cargo.toml --example
dock` paints both halves through softbuffer and closes after two seconds.

`WindowContext::dock_outputs()` lists the native output names accepted by dock
placement. XWayland callers query the prospective Wayland dock backend, avoiding
XWayland monitor aliases that layer-shell would reject. That probe carries no
editor/host state and returns within 100 ms; the native layer's own list follows
output notifications. AppKit, Windows and pure X11 enumerate their native names.
Query when opening a picker rather than on every frame.
