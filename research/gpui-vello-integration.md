# GPUI foundation with one renderer

Decision under investigation, 2026-10-08. The user authorizes breaking host APIs
only when the replacement reduces unnecessary complexity and improves stability
or performance. [Renderer fixture measurements](renderer-measurements-2026-10-08.md)
now exist, but no host-layer integration benefit has been established. Do not
ship a migration on the strength of GPUI's reputation alone.

The [renderer comparison](plugin-renderer-choice.md) leaves the backend choice
open. Classic Vello has a measured advantage on dense vectors in the RX 6600
fixture; Vello CPU leads ordinary controls. Vello GPU still deserves a material
reuse proof. GPUI native and Skia raster require their own measurable benefits.

## Why the current patch grew

The existing patch is primarily reliability infrastructure: about 1,000 lines
of native journaling/report delivery, 830 of tester/CI/evidence collection,
590 of documentation/lockfile changes, 245 of CPU presentation, and 222 of
native regression tests. It hardens the existing architecture; it does not
remove the duplicated UI foundation. Keep the requested evidence/reporting work
separate from the architectural replacement and judge each on its own cost.

## Candidate integration boundary

If GPUI earns a production replacement, use **GPUI core as the canonical UI runtime**: entities, elements, layout,
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

## Native host contract audit, 2026-10-08

**Stock GPUI cannot replace the current embedded host using its public window
options.** The pinned [`WindowOptions`/`WindowParams`](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui/src/platform.rs#L2259)
have no foreign parent or borrowed native view/connection field. Its
[`WindowKind`](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui/src/platform.rs#L2461)
offers top-level windows, dialogs, popups and layer-shell windows, not a DAW
editor child. `run_embedded` retains an application driven by a custom platform;
it does not change the stock platform's window or event-loop behavior.

| Platform | Stock GPUI behavior | Missing plugin contract |
| --- | --- | --- |
| Windows | Creates its own native window; dialog parenting selects the active window. | Explicit foreign HWND, `WS_CHILD` semantics, host-owned dispatch, focus/IME/accessibility and per-instance teardown. |
| macOS | Creates its own NSWindow/NSPanel and inserts its view into that window's content view. | An NSView attached to the supplied host view, no takeover of the host application delegate, safe Objective-C class identities across separately loaded plugin images. |
| X11 | Creates a window under the display's root; popup/dialog ownership uses GPUI windows. | Foreign X11 child and XEmbed focus/activation/lifecycle; transient ownership is not embedding. |
| Wayland | Creates xdg toplevels, internally parented popups or layer-shell surfaces. | Host-provided connection and parent proxy with a `wl_subsurface` role, plus host run-loop/input/lifetime ownership. |

Primary implementations: [Windows creation](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_windows/src/window.rs#L463),
[macOS content view](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_macos/src/window.rs#L1155),
[X11 root child](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_linux/src/linux/x11/window.rs#L549),
[Wayland surface roles](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_linux/src/linux/wayland/window.rs#L151).

### Wayland needs both a host protocol and a framework implementation

CLAP's pinned [`gui.h`](https://github.com/free-audio/clap/blob/a47f6badb49d948fd009998f28309cdab78979c9/include/clap/ext/gui.h#L52)
explicitly specifies floating windows for Wayland. Advertising embedded
`wayland` support would misrepresent that protocol. An X11 parent supplied by
a Wayland-session DAW may still support XWayland embedding, but a native
Wayland surface cannot be converted into an X11 parent.

VST3 3.8 defines native Wayland embedding. The DAW acts as a compositor for the
plugin and provides [`IWaylandHost::openWaylandConnection`](https://github.com/steinbergmedia/vst3_pluginterfaces/blob/4f547e8e102b47de4a8b8aaf343c73b700786372/gui/iwaylandframe.h#L30).
The plugin uses this host-created connection, attaches its own `wl_surface`
as a `wl_subsurface` to the supplied parent, and closes the connection through
the matching host method. [`IWaylandFrame`](https://github.com/steinbergmedia/vst3_pluginterfaces/blob/4f547e8e102b47de4a8b8aaf343c73b700786372/gui/iwaylandframe.h#L89)
maps parent and popup objects to the plugin's display. The plugin must not
mutate those parent objects. This is not permission to use a pointer from an
unrelated compositor connection or disconnect a host-owned display directly.

The framework currently locked by this repository is **`truce-vst3` 6.3.0**,
not a `moose-vst3` dependency. Its
[`pv_isPlatformTypeSupported`](https://docs.rs/crate/truce-vst3/6.3.0/source/shim/vst3_shim.cpp)
only accepts NSView, HWND or X11EmbedWindowID on the respective platforms;
its attached callback passes an untyped parent to `gui_open`, and its Linux
Rust callback constructs an X11 handle. Neither `IWaylandHost` nor
`IWaylandFrame` is exposed. This was checked against the actual registry
source selected by `Cargo.lock`, rather than an unrelated newer framework
checkout.

A working native VST3 Wayland backend therefore needs changes in the plugin
framework and an actual DAW implementing those optional interfaces: discover
and retain the host interface, preserve the negotiated platform type, obtain
the host-created connection and connection-specific parent, expose that
complete lifetime to the editor, integrate fd readiness into the host run loop,
and release callbacks/surfaces before closing the connection. Only then can a
native window substrate and wgpu surface be created against the correct
display/child pair. No such host integration has been demonstrated here.

GPUI also needs a custom embedded platform/window. Its
[`WaylandConnection::attach`](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_linux/src/linux/wayland/client.rs#L930)
can connect using an inherited socket internally, but that is not a public
VST3 `wl_display*` adoption API; its surface role implementation has no
subsurface branch. Opening a separate socket, an xdg toplevel or an exported
transient parent would not close this gap. No speculative baseview Wayland
backend or fake native-child probe was added.

### Native GPUI and Vello GPU are separate integration choices

Public [`WgpuContext`](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_wgpu/src/wgpu_context.rs#L11)
does expose its instance, adapter, device and queue. Sharing an already
selected wgpu device is possible at that boundary; claiming that every GPUI
GPU object is private would be incorrect. However:

- Pinned GPUI uses [wgpu 29](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/Cargo.toml#L953),
  while MUI/Vello in this repository use wgpu 30. Rust resource types cannot
  cross that version boundary without aligning dependencies or implementing
  a separate native interop path.
- [`WgpuRenderer::draw`](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_wgpu/src/wgpu_renderer.rs#L1123)
  acquires its target, renders a GPUI scene and presents internally. It has no
  public native frame-target callback; `new_from_surface` is wasm-only.
  [`PaintSurface`](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui/src/scene.rs#L768)
  carries a CoreVideo pixel buffer on Apple platforms, not a portable
  externally rendered wgpu texture.
- Keeping native Windows D3D11 or macOS Metal while composing a Vello GPU
  target requires texture import, alpha/color-space agreement and explicit
  synchronization across the relevant APIs. WGSL portability does not supply
  those contracts. Switching GPUI presentation to wgpu/Vello instead gives up
  the native-renderer comparison; measure it as a different candidate.
- The existing renderer benchmark builds rectangles and stroked paths and
  performs headless readback. It has no real editor text, image/material
  fixture, parenting, focus or unload evidence. Its timings do not prove the
  missing native integration. GPUI's pinned wgpu draw path also contains a
  panic after repeated GPU errors; the framework name alone is no stability
  guarantee.

For the canonical-scene design, the minimal missing seams are retained path
contours, typed public brush inspection and retained source-backed atlas
identities; `PlatformWindow::draw(&Scene)` already supplies the draw boundary.
Align wgpu versions if that renderer is chosen. For a texture-composition
experiment instead, expose the active renderer's frame target or a typed
external-texture primitive with device-generation and synchronization rules.
That second route retains a pixel/texture bridge and cannot count as deletion
of the duplicate UI runtime. Neither route removes the custom native-parent
platform and plugin-framework work above.

### Isolated CPU-to-native-GPUI probe

[`tools/gpui-host-probe`](../tools/gpui-host-probe/README.md) contains the public
API route that is implementable without those seams: the exact existing MUI
gallery fixtures → MUI Vello CPU renderer → straight BGRA `RenderImage` →
GPUI's native image atlas/presenter. It forwards pointer button edges and
keyboard navigation, sizes the complete raster from native viewport/scale,
reuses unchanged images and removes superseded atlas entries. It includes
tests for the alpha/channel seam and malformed image dimensions. Text,
materials, effects and images are rasterized by the existing MUI implementation.

This remains a **standalone prototype**. The hosted macOS run compiled and
passed 19 tests; editor/effects/material each completed the scripted image,
gesture and native resize stages. Shutdown-hook validation and native screenshot
parity remain pending, and no performance benefit is claimed. JSON metrics distinguish resolve/raster/image
preparation CPU time from a next-frame scheduling interval; the latter is not
GPU completion or presentation latency. GPU memory/upload timing needs driver
capture. At 1200×800, each changed frame carries 3.84 MB; 200% scale increases
that to 15.36 MB, before atlas/staging/scratch overhead.

Bounded `--test-ui[=<scene>]` checks a CPU image change after scripted input,
image fixture replacement and an observed native resize. On Linux X11 it can
request five native screenshots using the existing collector's capture/ack
protocol; each acknowledgement gates the next phase. Those screenshots compare
fully opaque pixels against the CPU reference with a four-channel-value RGB
tolerance, and reject blank native images. Transparent compositing and GPU
completion still need separate evidence.
Windows/macOS without that capture protocol retain callback evidence. The
summary keeps `native_gpu_submission_verified=false`. The completion journal
records an actual native-loop return as `run_end`. On macOS, GPUI invokes
[`NSApplication terminate:`](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_macos/src/platform.rs#L594-L611),
which can terminate the process before that return. The probe's public
[`App::on_app_quit`](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui/src/app.rs#L2590-L2608)
future records `native_shutdown_hook` with `native_application_returned=false`.
GPUI polls this future after clearing windows and flushing released entities
([shutdown order](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui/src/app.rs#L1093-L1124));
a final-field guard observes the probe's owned-resource teardown. This boundary
does not prove all driver allocations were destroyed or the GPU completed.
Incomplete tests or shutdown failures explicitly exit nonzero. The first
macOS run ended after `test_complete`, correctly failing the older return-only
collector contract; the revised lifecycle evidence still needs a hosted rerun.

This probe can investigate GPUI's D3D11/Metal image presentation on affected
hardware without feeding it MUI's compute renderer. It still needs GPUI's GPU
device, preserves MUI's UI/runtime and pays full-frame CPU raster/copy/upload
costs. It is not a native embedded replacement, a GPU-free fallback or proof
that merging the frameworks improves stability/performance.

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
