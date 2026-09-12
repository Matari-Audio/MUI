# Reusable MUI: remaining work

This is a usable renderer-independent layout/geometry/color foundation. It is not yet a
complete plugin UI toolkit or a hardened public crate release. The table distinguishes
implementation gaps from authoring conveniences and integration work.

| Area | Present | Missing / priority |
| --- | --- | --- |
| Layout | Intrinsic sizing, flex row/column, adaptive direction, wrapping, grid tracks/placement/spans, measured leaves | **P0:** explicit overflow/scroll/clip policy shared with painting and hit testing. **P1:** virtualization, aspect ratios, per-axis gaps, richer min/max-content/percentage sizing, grid auto-fit/auto-fill |
| Alignment | Physical x/y alignment, main-axis distribution, cross-axis alignment, grid cell overrides | **P1:** text baseline alignment, RTL/writing-direction semantics, per-item main-axis placement policy; broader nested flex/grid interoperability tests |
| Content | Host measurement callback, single-line playground text | **P0:** documented host font/image measurement contract and reference adapter. **P1:** wrapped paragraphs, font metrics/baselines, bidi/shaping, images and SVG content |
| Geometry | Inferred joining, concave/convex rounding, Boolean operations, offsets | **P1:** transformed item coordinates through the authoring layer, connected geometry + clipping + hit policy, broader rounding/degeneracy fuzzing. Explicit radius overrides must remain distinct from theme defaults |
| Interaction | Action IDs, hover detection, original-rectangle hits | **P0:** pointer capture, press/release/cancel, disabled state, focus, keyboard activation, tab order, accessibility tree. Current rectangle picking does not account for noninteractive occluding siblings, arbitrary path hits or host clips |
| Colors | Opaque sRGB/OKLab derivation, normal/hover contrast checks, scoped inheritance and merged fills | **P0:** pressed/selected/disabled/focus roles, text/icon role overrides, host compositing contract. **P1:** alpha/images/gradients, high-contrast preferences, independent rounding/type/spacing scales, transitions. Color checks are not full WCAG certification |
| Theme updates | Transactional color/spacing/rounding update with stable IDs and override preservation | **P1:** cache invalidation split by color/layout/geometry, incremental updates and profiling. Current theme update clones the UI and validates each hover state; this can become expensive on large trees |
| Rendering | Path-to-mesh and a thin egui paint adapter | **P0:** a reference integration that measures text, draws item styles, handles clipping/DPI and dispatches actions. **P1:** additional backend adapters; no claim of universal one-call integration with every Rust UI framework |
| Audio plugins | No audio dependency or global mutable UI state | **P0:** parameter bindings including begin/change/end automation gestures, host-driven parameter changes and UI-thread handoff. Never layout, allocate geometry, derive themes or tessellate in the audio callback |
| Packaging | Workspace crates and facade | **P0:** versioned local dependency declarations for publication, crate-name availability check, semver/API compatibility policy, public API documentation/examples and MSRV declaration. Current Rust 1.98.1 pin is a tested toolchain, not proof of the minimum compiler version |
| Verification | Concrete tests, Clippy, native/WASM builds, generated frontend fixtures | **P0:** Linux/macOS/Windows host integration and real renderer checks. **P1:** property/fuzz tests, pathological nested-layout benchmarks, cache/memory and code-size measurements. Existing tests are correctness examples, not exhaustive coverage |

## Recommended implementation order

1. One complete reference backend and plugin integration: measurement, painting, pointer/focus
   state, accessibility and parameter gestures. This reveals missing core contracts quickly.
2. Overflow/clipping/scrolling, consistent hit testing, and baseline-aware text layout.
3. Stateful semantic appearance: focus, pressed, selected, disabled; compositing-aware contrast.
4. Profile large real plugin layouts and optimize incremental work before promising performance.
5. Package/version/publish the crates after those integration contracts stabilize.

The playground is useful now for learning and reproducing layout/geometry issues. It uses
actual Rust compiled to WASM, but its small authoring DSL and browser text renderer are not
a substitute for a full Rust compiler or a production plugin backend.
