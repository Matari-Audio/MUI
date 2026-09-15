# Architecture

The authoring direction is deliberately compact:

```text
Rust Item / generic DSL ─┐
Frozen TypeScript input ─┼─> validated layout + item metadata
                         v
                    mui-layout
                 measured frames
                         v
                    mui-core
              layer and surface graph
                 /          \
             sharp basis   final path
                 \          /
                  renderer adapters
```

`mui_layout::generic` supplies short row, column and overlay trees for
CSS-like layout authoring. Those trees lower into the maintained keyed layout
engine; `Item` adds text, actions, semantic colour and merge metadata. A host
measures content through the resolve callback and reacts by resolving a new
snapshot. This keeps the API useful for general interfaces without making the
core a widget framework or claiming full CSS coverage.

The perceptual source theme (`mui_core::theme::SourceTheme`) owns pigment-based
light/dark derivation. It resolves to the existing runtime `Theme` and RGB
palette at the host boundary, so GPUI, egui and other adapters share one
style contract.

Rust authors use `Item` and the generic layout DSL. The frozen TypeScript compiler
remains a build-time compatibility reference for the same older `SceneSpec` and
item schemas; it is not a runtime dependency. The maintained compiler creates the
validated scene, scoped content/action metadata, and an ordered set of merged
outlines. The same item key identifies logical layout, initial geometry, text and
actions. Internal merge IDs never need to appear in authored code. The older
surface/node builders remain the lower layer.
Resolution proceeds through independent stages:

1. `mui-layout` validates keys, spacing and budgets, then asks Taffy for intrinsic and
   constrained layouts. An intrinsic prepass is only needed for adaptive flows.
   Grid adds equal fractional tracks, explicit tracks, auto placement and cell/span overrides.
   Physical positioning is remapped when a flex container changes direction.
   Ancestor auto-direction decisions precede descendant decisions; each flow switches
   at most once. Measurement callbacks supply real width-sensitive content sizes.
2. `mui-core` compiles surface dependencies into an iterative topological traversal.
   Frame extension reads completed layout frames in shared coordinates. It introduces
   no layout feedback and no surface dependency on the target.
3. Frame bases, Boolean merges and final-path offsets produce resolved surfaces.
   Offset bases retain their final material, while frame bases remain sharp for merging.
   Geometry limits also apply at frame construction and to resolved outputs.
4. The host paints through its renderer adapter. `mui-tessellate` / `mui-egui`
   provide one path; the GPUI experiment converts merged MUI paths and uses GPUI
   native text/primitives. Decorative extensions do not expand logical actions.
5. `ViewState` resolves scroll, affine transforms and ancestor clips over the
   scene. Hosts use the same snapshot for paint and `View` path-aware picking.
   The GPUI reference currently implements translated rectangular viewports;
   broader core affine semantics do not imply arbitrary native-widget transforms.

Layout allocates content space. Geometry determines the painted outline. An extension
may cross any number of padded ancestors without moving content. Hosts should paint
shared surfaces under a common clip, rather than clipping each fragment to its source.

Palette resolution is separate from layout. Theme spacing is cheap to resolve during
layout; derived OKLab colors should be cached by the host until palette/mode changes.
Contrast guarantees concern the final opaque sRGB colors and named background pairs.

A scene commit publishes only after successful layout and geometry resolution. No
geometry or text measurement belongs on the real-time audio callback. The current
core has no widget event dispatcher. GPUI owns native event/focus/task facilities
in the reference integration; transient component state currently lives in the experiment.

`Ui::set_theme` stages a new theme, re-resolves stored rounding rules and spacing tokens,
validates geometry and contrast, and publishes the update transactionally. Tap metadata
is declarative: the host recognizes gestures and uses the matching resolved view
for transformed/clipped path picking. Legacy `Ui::tap_at` remains rectangle-based.
Paint extensions and logical action bounds are intentionally independent. `outlines` replaces
merged member paint paths with one union while leaving content metadata intact.


## GPUI and plugin boundary

The reusable crates remain renderer-independent. GPUI is the selected experimental
native runtime and renderer; Vello is isolated in the render lab. MUI owns layout
contracts, merged geometry, themes and component semantics. Reuse GPUI text, focus,
actions, drag/drop, clipping and scheduling through shared integration points;
components should not each implement a backend bridge.

Window logical pixels, device pixels, local geometry and normalized parameter
values are different spaces. Apply each transform/scale once. Measurement, glyph
placement and picking must agree on the committed view and font configuration.

The CLAP probe has gain DSP, a GPUI worker and a host-thread edit bridge. It is
separate from the composition editor's local oscillator/envelope/routing state.
Consolidating that state into per-instance documents and stable descriptors is
planned, not already complete. No UI work belongs in the audio callback.

For future background work, use owned immutable input/results and GPUI's executor
facilities. A completion must check identity/revision before publishing; view-owned
work must be cancelled or ignored after close. Do not introduce background geometry
preparation until profiling justifies it. Keep existing transactional publication.

Current priorities and evidence live in [the roadmap](docs/ROADMAP.md), with the
[rendering map](docs/GPUI-RENDERING-MAP.md) and
[plugin plan](docs/KURV-PLUGIN-PLAN.md) as detailed follow-ups.

## Truce plugin boundary

`mui-truce` reads Truce's `ParamInfo` and parameter atomics, translates shared UI
gestures into balanced host-thread edits, and supplies a validated `#[persist]`
Document. Values stay in Truce; module/route IDs, enabled state, theme seed and
name belong to each plugin instance. Document locks and graph validation stay off
the audio callback. The embedded GPUI probe consumes this contract. The larger
KURV composition still needs migration before its view-owned state can be removed.
