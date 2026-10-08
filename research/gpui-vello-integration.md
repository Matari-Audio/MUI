# GPUI foundation with one renderer

Decision under investigation, 2026-10-08. The user authorizes breaking host APIs
only when the replacement reduces unnecessary complexity and improves stability
or performance. No improvement has been measured yet. Do not ship a migration
on the strength of GPUI's reputation alone.

The [renderer comparison](plugin-renderer-choice.md) supersedes classic Vello
as the assumed backend. Vello GPU is the first proof for material reuse; GPUI
native and Skia raster are alternatives with different deletion opportunities.

## Why the current patch grew

The existing patch is primarily reliability infrastructure: about 1,000 lines
of native journaling/report delivery, 830 of tester/CI/evidence collection,
590 of documentation/lockfile changes, 245 of CPU presentation, and 222 of
native regression tests. It hardens the existing architecture; it does not
remove the duplicated UI foundation. Keep the requested evidence/reporting work
separate from the architectural replacement and judge each on its own cost.

## The integration boundary

Use **GPUI core as the canonical UI runtime**: entities, elements, layout,
focus, input dispatch and frame scheduling. Its draw boundary encodes the
resulting scene into the chosen renderer. A Vello GPU backend owns one wgpu device/queue and presentation
path. Preserve MUI widget behavior, styling and vector/material algorithms
where useful; retire their duplicate UI runtime after callers migrate.

```text
DAW editor lifecycle / standalone application
  → instance-owned embedded platform and native child window
  → GPUI application handle, elements, layout, focus and input
  → GPUI scene encoded by the chosen renderer (Vello GPU first proof)
  → one wgpu device/queue → window render target → native presentation
```

This encoding is a renderer backend. There is no second persistent MUI UI scene,
second hit map, texture bridge, CPU frame readback or old-host API shim.
MUI's scene/text/layout crates may retain independent media/offscreen consumers;
their existence must not create a second per-window UI owner.

The prior alternative—keeping all MUI UI machinery and adding GPUI native
callbacks—does not earn an architecture replacement. It mainly adds an adapter.

## What upstream actually provides

The audit pins GPUI to
[`f1a10a5227a331e86bdb006301e1090a21d33e7a`](https://github.com/zed-industries/zed/tree/f1a10a5227a331e86bdb006301e1090a21d33e7a).
Two independent GPT-6.1-Sol researchers examined device/render and embedding
boundaries, then reconciled the scene-ownership decision.

- [`Application::with_platform` and `run_embedded`](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui/src/app.rs#L147)
  provide an instance-owned application driven by a foreign loop. They do not
  make stock desktop platforms suitable for a DAW.
- [`PlatformWindow::draw(&Scene)`](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui/src/platform.rs#L1213)
  is the renderer boundary. A custom embedded platform must implement native
  parenting, input/IME/accessibility and owned dispatch without taking over
  the host's application delegate or posting host `WM_QUIT`.
- Stock macOS GPUI [registers fixed Objective-C window classes](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_macos/src/window.rs#L125).
  Independently linked plugin images and unload make that unsafe to adopt
  unchanged. The stock platform is not the plugin substrate.
- GPUI's [external-compositor proposal](https://github.com/zed-industries/zed/pull/60573)
  is closed and unmerged. It is not a ready cross-platform Vello texture API.
  Its cross-API alternatives add the interoperability machinery this migration
  is intended to avoid.

## Renderer work that cannot be hand-waved

| Area | Required implementation |
| --- | --- |
| Paths | GPUI stores analytic coverage/tessellation data. Replace the path producer with retained canonical contours in a small core change; drawing the triangles as ordinary Vello polygons is incorrect. |
| Brushes | Add a typed brush inspection seam for GPUI's private gradient/pattern/color-space fields. No serialization workaround. |
| Glyphs/images | Implement the atlas contract using retained source bytes and stable image/mask identities. Preserve cached-scene replay, tint, crop, opacity, clipping and alpha. Disable subpixel rendering initially and use GPUI's grayscale path. |
| Effects | Implement borders, patterned fills, underlines and shadows deliberately. Reject unsupported primitives in the proof; never silently omit them. |
| Device/loss | For Vello GPU, use one wgpu version and one resource generation. Recovery invalidates every device-bound target/atlas and forces fresh drawing. Glyph work may submit additional ordered buffers to the same queue. |
| Native parent | HWND, NSView and X11/XEmbed first. CLAP embedded Wayland is unsupported by its protocol; VST3 Wayland needs the host's supplied connection/subsurface contract. |

Sources: [GPUI path data](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui/src/scene.rs#L788),
[brush data](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui/src/color.rs#L744),
[atlas contract](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui/src/platform.rs#L1690),
[CLAP GUI protocol](https://github.com/free-audio/clap/blob/a47f6badb49d948fd009998f28309cdab78979c9/include/clap/ext/gui.h#L52),
[VST3 GUI protocol](https://github.com/steinbergmedia/vst3_pluginterfaces/blob/4f547e8e102b47de4a8b8aaf343c73b700786372/gui/iplugview.h#L76).

## Deletion ledger and acceptance

| Existing ownership | Replace, then remove |
| --- | --- |
| `mui::host::Driver`, queued input and key ownership | GPUI input/focus/frame machinery |
| MUI UI layout/resolution and per-window scene lifetime | GPUI elements/layout and canonical scene |
| `mui-baseview` UI handler, snapshots and rendering loop | Embedded platform window; native child-window substrate may remain temporarily |
| `mui-winit` UI adapter | GPUI standalone platform with the same chosen renderer |
| `mui-truce` editor/session UI plumbing | Embedded application handle; retain parameter/automation gestures |
| Window-level adapter/device/surface recovery in parallel hosts | One renderer/context owner with coordinated resource invalidation |

First port one real editor with parameter gestures, an updating meter, a label,
image, clipping, text editing/IME, focus traversal and responsive layout.
Measure it against the same existing editor. Do not migrate the entire widget
library before the ownership and renderer boundaries are proven.

Acceptance requires **net reduction in owned runtime code**, counting platform
trait glue and renderer conversion; no permanent 4 ms polling pump; no duplicate
UI cache. Compare idle wakeups, startup time, active-frame CPU time and memory.
GPU completion/presentation latency need actual GPU timestamps or external
measurement, not just CPU submission timing.

Open two editors simultaneously, give them different input/sizes, close either
with resize/animation/delayed tasks queued, verify the other continues, reopen,
unload the plugin image, and load it again. Close must stop dispatch, detach
callbacks, cancel/drain owned work and release native/GPU resources in order.
A weak application pointer does not make a callback into an unloaded DLL safe.

Only after parity, lifecycle and performance gates pass should the old APIs
and their callers be removed. Failing the benefit gate means this GPUI design
has not earned a breaking replacement.

Classic Vello's compute requirements would remain in a GPUI/classic merge;
Vello GPU removes that requirement. GPUI Windows currently uses D3D11 raster
shaders; selecting wgpu/DX12 still gives up that native backend advantage.
Affected NVIDIA/Intel hardware and DAW crash causes remain unproven.
Local Rust validation is blocked by the user's instruction to leave existing
Rust jobs running. The current graphics PR stays draft; it is infrastructure
and investigation, not a completed GPUI migration.

The first hosted run already demonstrates why measurement matters: the
Microsoft Basic Render Driver (DX12/WARP) exits with `0xc0000005` after its
first submitted frame. That is a reproducible VM failure, not an NVIDIA/Intel
reproduction. The Metal probe selected Apple's paravirtual adapter but stalled
on the tester's exact resize requirement: the desktop clamped its requested
height. The tester now fits each aspect ratio inside the monitor and accepts
synchronous native resize results, and the
Windows probe enables [per-executable WER minidumps](https://learn.microsoft.com/en-us/windows/win32/wer/collecting-user-mode-dumps).
Neither probe outcome establishes that GPUI plus Vello is faster or more stable.
