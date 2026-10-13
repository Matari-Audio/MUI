# Owned baseview forward integration

Package `moose-baseview 0.4.1-moose.1` derives its public API and applicable
behavior from RustAudio/baseview tag **v0.4.1**, exact source commit
`8c6e6bab4b3c10f34b23dc12e878814349578777` (2026-10-12 audit).
The original vendored crates.io base was 0.3.4,
`bc987b96c22cbdb36507196dbbcad00c768e135b`. This is a forward integration into
MUI's owned fork, with intentional backend adaptations, not a stock-source copy.
Licenses remain MIT OR Apache-2.0.

The complete upstream commit range and changed production surfaces were compared.
Every upstream change in that range is classified below; version bumps, comments
and Clippy spelling-only changes are grouped separately.

| Upstream change | Integration |
| --- | --- |
| #338 `8d6e451`: size negotiation | `NativeSize::IS_LOGICAL`, `Window::adjust_size` and upstream physical/logical `SizingStrategy::adjust_size` preserve current DPI, fixed sizes and min/max constraints. |
| #323 `306409a`, `4dce047`: X11 Present Notify pacing/setup | Intentionally adapted: retained the owned monitor-aware deadline pacer and original X11 display/visibility barriers. It uses RandR mode timing, coalesced explicit wakes and bounded close, rather than introducing Present extension callbacks that would need the same unload/parent revocation guarantees. |
| #325 `c2c00ec`: X11 EGL | Intentionally excluded optional backend: retained guarded GLX compatibility. MUI's Vello native GPU path rejects GL and does not consume EGL; adding a second native GL ownership path is outside this fork's owned renderer contract. The optional `opengl` feature does **not** claim upstream EGL support. |
| #336 `8c06986`: common-mode AppKit timer | Already owned: native deadlines and retained timer sources run in common run-loop modes, including tracking/dragging. |
| #341 `5c814da`: autorelease pools | Already owned: AppKit calls and OpenGL current-context/swap/resize operations run inside pools. |
| `0fdebac`, `e685430`, `5a61091`: GL configuration cloning | Already owned: `GlConfig` is non-Copy and cloned explicitly. |
| #345 `def39ec`: Win32 DPI virtualization | Already integrated with host scale override, guarded thread DPI context restoration and parent DPI initialization. |
| #347 `9065740`: capture loss | Already integrated with capture-counter reset and host pointer ownership preservation. |
| #351 `4805e4d`: initial parent resize | Already integrated: Win32 defers host resize until shown; rejected resize restores the previous extent and handler size. |
| #340 `b98a9a6`: draw/poll and wake API | Added current `WindowHandler::draw/poll`, `WindowContext::request_redraw/request_redraw_after/waker`, `Window::request_poll/waker`, and Send+Sync weak `WindowWaker`. The common scheduler routes these through existing native wake handles, separating poll-only updates from presentation and deferring requests made inside draw until the next frame. |
| #344 `7ab2272`: external damage | Added current `DamageArea`/`DamageRect` and `WindowHandler::damage`. Initial/resize damage is full-window; X11 Expose and Win32 WM_PAINT carry physical rectangles before poll/draw. The retained AppKit CALayer renderer uses full-window invalidation at initial/resize boundaries; this fork does not claim AppKit partial-rectangle redraw support. |
| #350 `ffb5942`: timer API | Added current `WindowContext::create_timer`, weak comparable `TimerHandle`, and `WindowHandler::on_timer`. Timers execute on the guarded native callback thread and are destroyed on close. Missed intervals coalesce rather than flooding callbacks. |
| `61e2413`, `72bd822`: host/API documentation | Integrated initialization-safe host callback documentation and documented the public wake, draw and timer contracts. |
| #339 `6df3c64`; new timers/wake/partial-rendering/pacing examples | Stock examples are not vendored. MUI owns its renderer/editor examples; portable and actual native regressions cover adopted APIs rather than adding independent clack/femtovg example dependency graphs. |
| `dcc83bb`, `712ebfe`, `095d89b`, `8c6e6ba`, `bc249c9`, `ed6211f` | Release metadata, lint spelling and pacing-example cleanup. Downstream version identifies 0.4.1 + moose; retained native package dependencies and strict Clippy conventions, with calloop pinned to =0.14.4 to preserve upstream MSRV 1.82. |

## Compatibility and lifetime contracts

The default remains `FrameDemand::Continuous` and `draw` calls legacy `on_frame`.
This preserves existing BUFFR polling of background preferences, meters and GPU
initialization. Handlers implementing current draw-only logic may override demand
to `Idle` and wake explicitly. Existing input callbacks still trigger a frame,
including for Idle handlers: that is the fork's input-barrier compatibility
behavior, rather than upstream's fully lazy default. A consumed `At` deadline is
remembered before asking the handler for its next deadline, so relative model
polling deadlines cannot drift indefinitely without a callback.

Polls, delayed draws and timers share the owned native frame/deadline scheduler.
They pause while hidden and can be delayed by frame pacing or an occupied native
callback; timers are GUI timers, not MIDI or hardware clocks. Timer intervals
must be positive and representable, and the per-window registry has a 1024-timer
bound with an error on exhaustion. Dropping a handle does not stop its timer.
There is no additional thread per timer and no catch-up callback burst.

Work queues contain no native handles. Wakers weakly reference the work state;
its native wake operation already uses revocable lifetime tokens. Close revokes
work before host-owned native teardown, and a close requested during poll/timer
prevents subsequent drawing. Existing handler-before-native teardown, Windows
callback leases, AppKit ownership, X11 bounded requests/close and unload pinning
remain in force. Image pinning preserves code, not a host-owned drawable.

MUI's reentrant adapter coalesces only adjacent pointer-motion samples whose
modifiers agree. Key, IME, button, wheel, focus and drop transitions remain FIFO
barriers; native accessibility uses its existing independent admission path.
There is no fixed queue cap that discards these lossless transitions. Extreme
barrier-only producer traffic can still grow the GUI queue until its active
callback returns; imposing backpressure requires a host-compatible delivery
contract rather than silently dropping text or key events.

CPU-first startup may use the existing one-frame placeholder. Failed/unavailable
GPU initialization and device-loss recovery use full software scene quality;
transient classification continues GPU retries rather than keeping placeholder
pixels or selecting permanent software-only mode.
