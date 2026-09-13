# Reusable MUI: remaining work

This is a usable renderer-independent layout/geometry/color foundation. It is not yet a
complete plugin UI toolkit or a hardened public crate release. The table distinguishes
implementation gaps from authoring conveniences and integration work.

## GPUI integration checkpoint (2026-09-13)

The [plugin panel](../experiments/gpui-plugin/README.md) now connects MUI layout and
path geometry to GPUI text, scrolling/clipping, text editing and pointer/keyboard
interaction. Its X11 check exercises dragging/cancellation, focus traversal, clipboard,
scroll boundaries and editor lifecycle on the RX 6600. Core `Path::contains` adds
nonzero-fill picking; `Item::disabled` inherits through the item tree and suppresses
picking/hover styles. The core changes pass the full workspace verification.

This advances the first four areas through one experimental reference integration;
it does not complete them universally. Rich spans and platform IME validation remain.

The [layout/view contract](LAYOUT-VIEW.md) now adds first-baseline font measurements,
explicit nested clip/scroll policies and affine view state shared by renderers and picking.
The SVG example exercises that contract; the GPUI probe has not yet adopted it.
The capability table below describes the renderer-independent core unless noted.

| Area | Present | Missing / priority |
| --- | --- | --- |
| Layout | Intrinsic sizing, flex row/column, adaptive direction, wrapping, grid tracks/placement/spans, measured leaves, explicit clip/scroll policies and nested viewport limits | **P0:** adopt the core view contract in the GPUI renderer. **P1:** virtualization, aspect ratios, per-axis gaps, richer min/max-content/percentage sizing, grid auto-fit/auto-fill |
| Alignment | Physical x/y alignment, main-axis distribution, cross-axis alignment, grid cell overrides, first text baselines | **P1:** RTL/writing-direction semantics, per-item main-axis placement policy; broader nested flex/grid interoperability tests |
| Content | Host measurement callback, optional Parley shaping/wrapping with final glyph layouts, single-line playground text | **P0:** documented host font/image measurement contract and reference adapter. **P1:** rich spans, images and SVG content; connect Parley output to the renderer and playground |
| Geometry | Inferred joining, concave/convex rounding, Boolean operations, offsets, affine view transforms and clipped path picking | **P1:** arbitrary path clips, broader rounding/degeneracy fuzzing. Explicit radius overrides must remain distinct from theme defaults |
| Interaction | Action IDs, legacy rectangle hits, view-space path hits with ancestor clips and occlusion | **P0:** connect the GPUI reference interaction path to reusable MUI components: pointer capture, press/release/cancel, focus, keyboard activation, tab order and accessibility. Inherited disabled state is present. The legacy rectangle API remains; the new `View` API applies transforms, path tests, occlusion and clips |
| Colors | Opaque sRGB/OKLab derivation, normal/hover contrast checks, scoped inheritance and merged fills | **P0:** pressed/selected/disabled/focus roles, text/icon role overrides, host compositing contract. **P1:** alpha/images/gradients, high-contrast preferences, independent rounding/type/spacing scales, transitions. Color checks are not full WCAG certification |
| Theme updates | Transactional color/spacing/rounding update with stable IDs and override preservation | **P1:** cache invalidation split by color/layout/geometry, incremental updates and profiling. Current theme update clones the UI and validates each hover state; this can become expensive on large trees |
| Rendering | Path-to-mesh and a thin egui paint adapter | **P0:** a reference integration that measures text, draws item styles, handles clipping/DPI and dispatches actions. **P1:** additional backend adapters; no claim of universal one-call integration with every Rust UI framework |
| Audio plugins | No audio dependency or global mutable UI state | **P0:** parameter bindings including begin/change/end automation gestures, host-driven parameter changes and UI-thread handoff. Never layout, allocate geometry, derive themes or tessellate in the audio callback |
| Packaging | Workspace crates and facade | **P0:** versioned local dependency declarations for publication, crate-name availability check, semver/API compatibility policy, public API documentation/examples and MSRV declaration. Current Rust 1.98.1 pin is a tested toolchain, not proof of the minimum compiler version |
| Verification | Concrete tests, Clippy, native/WASM builds, generated frontend fixtures | **P0:** Linux/macOS/Windows host integration and real renderer checks. **P1:** property/fuzz tests, pathological nested-layout benchmarks, cache/memory and code-size measurements. Existing tests are correctness examples, not exhaustive coverage |

## Recommended implementation order

1. One complete reference backend and plugin integration: measurement, painting, pointer/focus
   state, accessibility and parameter gestures. This reveals missing core contracts quickly.
2. Adopt the new core view and baseline APIs in the reference backend; validate nested scrolling with native input.
3. Stateful semantic appearance: focus, pressed, selected, disabled; compositing-aware contrast.
4. Profile large real plugin layouts and optimize incremental work before promising performance.
5. Package/version/publish the crates after those integration contracts stabilize.

The playground is useful now for learning and reproducing layout/geometry issues. It uses
actual Rust compiled to WASM, but its small authoring DSL and browser text renderer are not
a substitute for a full Rust compiler or a production plugin backend.
