# GPUI rendering map for MUI

Audited 2026-09-13 against our local GPUI checkout, pinned to
`7960b2a7c9568e90fbe0727332149e5b2a5fd57a`. This is an integration plan;
the gradient defect is now fixed in the shared wgpu shader; AA remains open.
See [the RX 6600 correction report](render-lab/rx6600-gradient-fix-2026-09-13/RESULTS.md).

## What the article establishes

Zed's [March 2023 article](https://zed.dev/blog/videogame) describes a
Metal-era renderer specialized for UI primitives: rounded rectangles, approximate
Gaussian shadows, cached text shaping and glyph atlases, SVG masks, image textures,
and ordered batches. Its 120 Hz budget includes application work, layout and painting.
The blurred-logo illustration explains convolution; it does not establish a public
backdrop-blur API. Vello appears as related GPU rasterization research, not as a
renderer Zed embeds. The old element API and historical glyph-cache details should
not be copied into our current implementation.

## What our pinned implementation actually provides

Source links below point into the local checkout. These findings concern our wgpu
backend; they are not claims about identical behavior on every GPUI platform.

| Area | Verified implementation | MUI integration decision |
| --- | --- | --- |
| Rounded surfaces and borders | `Window::paint_quad`, [window.rs](../experiments/upstream/crates/gpui/src/window.rs); `fs_quad` with distance-based edge coverage, [shaders.wgsl](../experiments/upstream/crates/gpui_wgpu/src/shaders.wgsl) | Use native quads for rectangles, rounded panels and solid dots. Keep merged concave outlines in the shared geometry adapter. |
| General paths and tessellation | Lyon fill/stroke tessellation and triangle output in [path_builder.rs](../experiments/upstream/crates/gpui/src/path_builder.rs), especially `build_path` at line 322 | Convert MUI geometry once at the shared boundary. Cache stable geometry; avoid rebuilding it for unrelated hover/value changes. |
| Path AA | `RenderingParameters::new` chooses supported samples from 4, 2, 1; paths render through an intermediate target in [wgpu_renderer.rs](../experiments/upstream/crates/gpui_wgpu/src/wgpu_renderer.rs) | This differs from rounded-quad coverage. Lower curve tolerance improves approximation, not the sampling algorithm. Do not promise it will repair thin-stroke gaps. |
| Curve shader coverage | `fs_path_rasterization` handles curve coordinates, but Lyon `build_path` supplies constant coordinates to its triangles | The presence of an analytic curve shader does not mean our general tessellated paths receive analytic curve AA. |
| Gradients | `paint_path` accepts `Background`; `gradient_color` exists in the wgpu shader | Use the existing facility for ADSR and surfaces with the validated shared gradient correction. No separate per-component gradient engine. |
| Drop and inset shadows | `BoxShadow` exposes color, offset, blur, spread and inset in [style.rs](../experiments/upstream/crates/gpui/src/style.rs), line 349; `paint_drop_shadows` / `paint_inset_shadows` in `window.rs` | Native solution for supported rounded boxes. Shadow bounds and corner radii do not describe an arbitrary concave merged shell. |
| Shadow blur | `fs_shadow` integrates along one axis and samples four times along the other; zero blur uses distance-based coverage | Reuse the native shadow primitive. It is a specialized shadow shader, not a general image-filter pipeline. |
| Text shaping and caching | Platform text system, font metrics and line-layout cache in [text_system.rs](../experiments/upstream/crates/gpui/src/text_system.rs); `paint_glyph` in `window.rs` | Horizontal labels already use native shaping and atlas rendering. Rotated outline labels remain an exception to audit. Avoid converting ordinary text to paths. |
| Subpixel text | Current constants are four X variants and one Y variant in `text_system.rs`, lines 44–48 | Verify fractional positioning and DPI with the current backend. Historical cache counts are not today's contract. Native arbitrary font width-axis selection remains a separate limitation. |
| SVG and images | `paint_svg` and `paint_image` in `window.rs`; sprite batches in `wgpu_renderer.rs` | Use GPUI SVG assets for Phosphor icons and native image caching. Do not construct icons from ad hoc geometry when an asset exists. |
| Element lifecycle | `Element::request_layout`, `prepaint`, `paint` in [element.rs](../experiments/upstream/crates/gpui/src/element.rs), lines 51–95 | Keep layout, registered hitboxes and painted geometry in agreement. Current GPUI has three phases, not the historical two-phase example. |
| Clipping, layering, batching | Content masks and `paint_layer` in `window.rs`; primitive batches in `wgpu_renderer.rs`, line 1476 onward | Submit to GPUI's scene. Preserve overlay order and clip boundaries; let GPUI batch compatible work. |
| Runtime and interaction | Existing integration inventory in [GPUI-REUSE.md](GPUI-REUSE.md) | Retain GPUI entities, focus, actions, native drag/drop, scheduling and popovers. Components compose these shared facilities. |

### Blur is several different requirements

1. **Rounded-box shadow:** already native, including blur and inset configuration.
2. **Shadow following a merged concave outline:** not covered by the box-shadow
   interface. If needed, evaluate a cached shape mask and blur/composite pass at
   the shared geometry boundary. A shadow of its bounding rectangle is incorrect.
3. **Blurring an image or content behind an individual panel:** no general widget
   backdrop-filter API was found in the inspected GPUI/wgpu source. This needs a
   separate capability investigation before promising glass effects.
4. **OS window material:** platform backdrop settings are a different capability;
   they do not provide arbitrary per-component blur inside a plugin editor.

Do not build items 2–3 merely to reproduce an illustration. First use native
gradients and rounded-box shadows where they match the actual design.

## Why the existing comparison does not contradict Zed being fast

Our [RX 6600 report](render-lab/rx6600-2026-09-13/RESULTS.md) compares vector
fixtures, not editor workloads or text. GPUI's small prebuilt fixture measured
0.374–0.404 ms versus Vello area's 0.460–0.500 ms. At 2,560 marks, GPUI measured
4.294–4.767 ms versus 0.755–0.961 ms. These include presentation and device waits;
they are not GPU timestamps or end-to-end interaction latency.

That report also reproduces gradient mismatch and quarter-pixel stroke gaps in
our pinned GPUI path. Tighter tessellation did not repair the gaps. The historical
gradient defect is corrected by the subsequent shared shader patch. Neither result
establishes that replacing GPUI, or adding Vello to this editor, improves the
complete application.

The wgpu `PrimitiveBatch::Surfaces` arm is unimplemented (renderer line 1560).
It is not a ready-made external GPU texture integration hook. A Vello experiment
would need a concrete composition path and measurements including its costs.

## Execution order before the plugin plan

The [main roadmap](ROADMAP.md) owns sequencing. Current status: component fixtures
and the shared gradient correction are complete, including RX 6600 regression checks.
Subpixel AA, typography, actual-editor frame profiling and the known native wheel /
restricted-viewport input failures remain open. The steps below retain their technical
scope; completed reproductions should be extended, not rebuilt.

### 1. Isolate the rendering defects

- **Done:** the existing render lab includes the actual pie-container geometry,
  inner dark padding, border and an ADSR curve snapshot, with native quad/circle
  controls beside equivalent paths to identify the failing primitive.
- Compare at 1×, 1.5× and 2×, and quarter-pixel translations. Check gradient values,
  alpha/color transfer, edge continuity, shared seams and matching physical scale.
- **Done:** the gradient discrepancy was traced from sRGB stops through shader
  interpolation and Unorm output; the shared shader correction passes analytic
  checks. Preserve those checks while changing AA or other rendering behavior.
- Separate curve approximation errors from AA coverage errors. Make any fix at
  the shared adapter or backend, then verify every component using that boundary.

**Exit:** reproducible fixtures explain the defect and demonstrate the fix, or
document a remaining backend limit. A visually nicer screenshot alone is insufficient.

### 2. Finish native primitive reuse

- Audit remaining simple shapes and icons; use native quads, glyphs and SVGs where
  they represent the intended shape exactly. Preserve real concave unions.
- Apply native gradients and box shadows through shared theme/component styling.
- Keep one geometry adapter. Oscillators, envelopes, warps and pie containers
  must not acquire independent renderer integrations.
- Retain geometry across unchanged frames and invalidate it when size, shape or
  relevant style changes. Do not cache away interactive updates.

**Exit:** the same reusable primitive gives consistent rendering in every module,
with no regressions in hitboxes, clipping, layering or modulation gestures.

### 3. Measure the actual editor

- Profile cold start and warm interaction separately: layout/prepaint, geometry
  preparation, scene submission, GPU work where timestamps are available, and presentation.
- Exercise resize, continuous parameter drag, growing modulation routes, hover
  cables, ADSR editing and multiple groups. Record median and tail latency.
- Check the active-window frame cadence separately from inactive-window throttling.
  A 120 Hz target permits 8.33 ms for the complete frame, not each stage.
- Compare against the existing [composition CPU measurements](../experiments/gpui-plugin/results/oscillator-performance.txt).
  Do not label CPU draw time as GPU time or achieved display FPS.

**Exit:** visual quality and interaction latency pass together on the RX 6600.
Only if a measured path-rendering limit remains, evaluate Vello for that boundary
with texture composition, clipping, ordering, scale and synchronization included.

GPUI remains the framework foundation. MUI owns component semantics and merged
geometry. Additional renderers or custom effect pipelines require a demonstrated gap.
