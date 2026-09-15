# Zed Decoded lessons for MUI

Research date: 2026-09-13. The two requested articles are historical sources
from 2024. The MUI sections below are current facts from this checkout and are
labelled separately from conclusions drawn from the articles.

## Sources

Historical articles:

- [Linux when?](https://zed.dev/blog/zed-decoded-linux-when) — May 7, 2024.
- [Why not just embed Neovim?](https://zed.dev/blog/zed-decoded-vim) — June 13, 2024.

Primary sources inspected for implementation detail:

- [GPUI `Platform` and `PlatformWindow` traits at the article-linked snapshot](https://github.com/zed-industries/zed/blob/98ea5e172ec114004116f996f51b46cdf5c438e4/crates/gpui/src/platform.rs).
- [Linux port via Blade, PR #7343](https://github.com/zed-industries/zed/pull/7343).
- [Blade](https://github.com/kvark/blade), the low-level Rust graphics library named by the Linux article.
- [Optimizing the Metal pipeline to maintain 120 FPS in GPUI](https://zed.dev/blog/120fps) — February 7, 2024.
- [Zed Vim tests using a Neovim-backed context](https://github.com/zed-industries/zed/blob/a965dd62ead42a1624b229c262bef4c1fb4a3ec8/crates/vim/src/normal/search.rs), at the article-linked snapshot.

The primary sources are used for architecture and historical behavior. They do
not establish the current API or rendering behavior of every GPUI platform.

## Historical lessons from the articles

### Linux when?

The May 7, 2024 article describes Linux support as a platform integration
problem rather than a consequence of Rust portability. GPUI put windowing,
text, events, executors, clipboard, dialogs, and lifecycle behind explicit
platform seams; a Linux client still had to account for X11, Wayland,
distribution packaging, desktop environments, and audio services. Its frame
story was scene primitives reaching `PlatformWindow::draw`, then a Linux
Blade/Vulkan renderer. The article also treats toolkit dialogs as a narrow
native service, rather than a reason to make the whole application a GTK or Qt
application. These status notes and TODOs describe 2024 work. **Inference for
MUI:** keep parent handles, frame scheduling, input/text services, and teardown
as an explicit adapter boundary around the reusable scene and geometry model.

### Why not just embed Neovim?

The June 13, 2024 article rejects Neovim embedding because the editor models
already differ: Vim commands address character positions, while Zed’s native
model works between characters. That invariant affects newlines, selection,
operators, counts, and many commands. Neovim would also duplicate or displace
Zed’s text structures, collaboration state, renderer, and async runtime. Zed
therefore implements Vim semantics in its own model while using headless Neovim
as a keystroke-and-state comparison oracle in tests. This is a historical
design explanation, not evidence about current Neovim compatibility. **Inference
for MUI:** keep layout, merged outlines, IDs, and parameter state authoritative
inside MUI; use a small replay oracle for subtle semantics rather than adding a
second UI/runtime model to the CLAP editor.

## Current MUI facts

The current architecture is documented in [`ARCHITECTURE.md`](../../ARCHITECTURE.md):
Rust authors produce validated `Item`/generic DSL data; MUI resolves intrinsic
layout, compiles surface dependencies, unions sharp bases, rounds the final
boundary, and derives offsets from that final path. The frozen TypeScript
package remains a build-time compatibility reference. The host then paints
resolved outlines and applies its own clip and hit policy. Scene commit is
transactional, and no geometry or text measurement belongs on the real-time
audio callback.

The current GPUI choice is documented in [`GPUI-REUSE.md`](../GPUI-REUSE.md):
the plugin experiment uses GPUI as both renderer and native UI runtime. Vello is
kept in the render lab for comparison; the plugin panel does not compose two
renderers. GPUI supplies path painting, glyph shaping/atlas, frame invalidation,
focus, actions, native drag/drop, scrolling, clipping, popovers, and text-input
plumbing. MUI supplies layout/measurement contracts, resolved styles, merged
geometry, logical IDs, path hit policy, and begin/value/end audio-parameter
gestures.

The shared geometry seam is deliberately small. The GPUI adapter validates each
MUI path, preserves arcs until conversion, converts arc segments to cubics, and
uses the same converted path for fill and border. Simple rounded rectangles and
solid modulation dots use GPUI native primitives. Concave merged shells,
shoulders, holes, and other MUI-derived outlines stay on the path adapter. A
component must not grow an independent geometry-to-renderer implementation.

The panel builds one view snapshot for geometry, text, scrolling, clipping, and
input. Wheel events go through the existing MUI scroll propagation and only the
remaining delta reaches an ancestor; this avoids the common nested-scroll bug
where both native listeners move the same containers. GPUI handles focus,
keyboard actions, typed source drag/drop, text selection, clipboard, and IME
plumbing. MUI retains route legality, soft snapping, cable drawing, and audio
parameter gesture boundaries.

## Linux GPU and frame-quality findings

The current hardware evidence is in
[`docs/render-lab/rx6600-2026-09-13/RESULTS.md`](../render-lab/rx6600-2026-09-13/RESULTS.md).
It tests the pinned Zed/GPUI revision `7960b2a7c9568e90fbe0727332149e5b2a5fd57a`,
Vello 0.10.0, and wgpu 29.0.4 on an AMD RX 6600 with RADV/Vulkan. These are
presentation-inclusive CPU wall-clock measurements with device waits, not GPU
timestamps, end-to-end latency, or DAW measurements.

For a small prebuilt 768x512 scene with 40 marks, GPUI measured 0.374–0.404 ms
median versus 0.460–0.500 ms for Vello area AA. For a 2,560-mark path-heavy
stress scene, GPUI measured 4.294–4.767 ms versus 0.755–0.961 ms for Vello area
AA. GPUI therefore has a small-scene advantage in this harness, while Vello
area scales much better for the custom path-heavy fixture. Scene encoding is a
separate cost and is excluded from those figures.

The pre-fix render-lab baseline recorded an sRGB gray-ramp MAE of 50.71. The
root-turn correction now records 0.25; six GPUI default/tight captures at
scales 1, 1.5, and 2 pass the analytic sRGB/Oklab quad and path checks with a
maximum error of 2/255. See the [render-lab procedure and correction notes](../../experiments/render-lab/README.md)
and its [gradient checker](../../experiments/render-lab/tools/check_gradients.py).
The gradient gate is therefore closed for those fixtures. Anti-aliasing remains
open: circle-coverage diagnostics and subpixel border gaps still need a design
decision and broader merged-surface captures. These fixture checks do not cover
typography, glyph rasterization, full invalidation, memory/power, or host input.

The fixture does not contain typography. Text shaping, glyph rasterization,
full application invalidation, memory/power, and host input remain separate
gates. Do not choose a whole framework from these vector numbers, and do not
promise 120 Hz from a draw-time median. The 120 FPS article’s practical lesson
is to measure presentation synchronization and frame delivery separately from
CPU preparation: it encountered compositor mode, display refresh behavior,
command-buffer synchronization, and buffer reuse issues even when measured
draw time looked low. MUI’s plugin profile should keep active interaction,
inactive-window throttling, resize, and presentation intervals as separate
measurements.

## Embedded CLAP editor: current boundary

The current probe is deliberately outside MUI’s stable API. In
[`experiments/gpui-plugin/src/lib.rs`](../../experiments/gpui-plugin/src/lib.rs),
`GpuiEditor::open` accepts an X11 parent, starts one editor worker, and passes a
host value through an atomic. The worker creates `gpui_linux::EmbeddedX11`, calls
`Application::run_embedded`, opens a child window, reparents it with X11, and
then pumps runtime events. It polls commands with an 8 ms timeout, checks the
host value, and calls `runtime.pump()`; this is an explicit embedded event loop,
not ordinary top-level application startup.

The host owns parameter edits. The worker emits `Begin`, `Value`, and `End`; the
host drains those in `Editor::idle`. Close balances an unfinished edit even if
the worker fails. The audio callback only reads the plugin parameter and
processes samples. This is the correct shape for an audio plugin: no GPUI
entities, allocations, locks, graph construction, or UI calls on the audio
thread.

The repository’s September 13, 2026 RX 6600 record reports passing X11 resize,
pointer/keyboard, drag cancellation, focus traversal, text
selection/editing/clipboard, nested scroll/clipping, two instances,
close-during-drag, and close/reopen checks. It is still X11-only and is not a
real DAW GUI test. The same dated [CLAP experiment record](../../experiments/gpui-plugin/README.md)
reports 34 passed, 7 skipped, and 3 failed; the failures are state
reproducibility cases where restored parameter values do not trigger the host
rescan. This is a recorded result, not a fresh verification here. A separate
OS/XTest embedded harness also failed different wheel assertions on two desktop
runs (scroll reset and wheel-movement accounting), so the embedded checks are
not currently all green.

The separate render-lab embedding probe verifies two X11 child windows sharing a
GPUI GPU context, sibling survival after one closes, and three resizes. It does
not validate the GPUI `App` runtime, focus/IME, automation, a DAW lifecycle,
Wayland, or other operating systems. The current embedded proof is therefore a
renderer/window-handle proof plus an editor-contract harness, not portable CLAP
embedding.

## Applicability and pitfalls

1. Keep one MUI-to-GPUI boundary. Resolve layout and final merged geometry once,
   cache stable paths, and invalidate on size, shape, scale, or relevant style
   changes. Let GPUI batch native quads, glyphs, SVGs, clipping, and redraws.
2. Keep logical content separate from decorative merged paint geometry. Merge
   outlines for appearance, but preserve item IDs and tap/action metadata for
   interaction. A bounding-box hit test or a second flattened path can disagree
   with a concave or holed surface.
3. Use native GPUI primitives only where their shape matches the design: quads,
   rounded boxes, solid dots, glyphs, SVGs, text, shadows, and gradients that
   have passed the quality checks. A box shadow cannot represent an arbitrary
   concave MUI shell; use the shared path or a measured mask/effect path.
4. Treat `Platform` as a conceptual boundary, not a stable public embedding API.
   The article-linked trait is an internal GPUI trait at its snapshot. The
   pinned Linux experiment currently needs an explicit pump because embedded
   startup can block in `Platform::run`, and reparented child windows cannot
   rely on top-level compositor wakeups. A production host needs a platform
   adapter for parent handles, event pumping, frame requests, resize, DPI,
   focus, IME, clipboard, and close.
5. “Linux” means a support matrix. The [GPUI platform source](https://github.com/zed-industries/zed/blob/98ea5e172ec114004116f996f51b46cdf5c438e4/crates/gpui/src/platform.rs)
   exposes platform services, while the MUI probe currently proves X11 only.
   Packaging, display servers, desktop environments, window managers, dialogs,
   and audio services all need an explicit support decision.
6. Do not add a second input/runtime system for the editor. GPUI should own
   focus, actions, text editing, drag/drop, scrolling, and popover placement.
   MUI should own parameter descriptors, stable IDs, merged geometry, route
   semantics, and host begin/value/end mapping. Keep semantic replay at the
   boundary instead of putting another editor runtime in the plugin.
7. Keep state authoritative per plugin/editor instance. Host automation and
   saved state must update the same model the UI displays. Use stable
   `NodeId`/`ParameterId`/`RouteId`; never let view order become automation
   identity. Publish validated snapshots at block boundaries and reclaim old
   data away from the audio thread.

## Actionable priorities

1. **Finish the rendering gate.** Preserve the corrected gradient checks on the
   actual merged pie/tab, inner inset, border, and ADSR fixtures at 1x, 1.5x,
   2x, and quarter-pixel translations. Resolve or document the remaining
   anti-aliasing and subpixel border gaps before compensating theme colors or
   globally tightening tessellation. Measure text separately.
2. **Make the embedded pump a deliberate platform layer.** Preserve the worker,
   bounded command bridge, explicit frame requests, and balanced edit lifecycle;
   then test resize, close/reopen, DPI, focus/IME, and idle/active scheduling on
   the selected platforms. Keep X11-only code out of MUI’s stable API.
3. **Finish one audible CLAP slice.** Define descriptor-backed parameters and
   per-instance document state, fix state restoration/rescan behavior, then bind
   one oscillator/ADSR/gain path to the existing runtime. Validate automation,
   MIDI, block boundaries, sample-rate changes, bypass, editor close/reopen, and
   two instances before expanding KURV visuals.
4. **Add semantic replay oracles.** Replay parameter gestures, cancellation,
   route edits, scroll transfer, and host automation through the real GPUI input
   boundary and compare stable model state. Keep renderer image checks and host
   lifecycle checks separate.
5. **Evaluate Vello only for a measured gap.** If GPUI quality remains below the
   target, prototype composition at the actual texture/clip/layer/synchronization
   boundary and include text, input, memory, and presentation costs. Do not
   combine renderers in the production panel as an unmeasured compromise.
