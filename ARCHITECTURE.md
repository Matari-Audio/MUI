# Architecture

Rust and TypeScript authors use `Item`. Its compiler creates the validated `SceneSpec`,
scoped content/action metadata, and an ordered set of merged outlines. The same item key
identifies logical layout, initial geometry, text and actions. Internal merge IDs never
need to appear in authored code. The older surface/node builders remain the lower layer.
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
4. The host paints paths via `mui-tessellate` / `mui-egui`, choosing clip and hit-test
   policy independently of decorative geometry.

Layout allocates content space. Geometry determines the painted outline. An extension
may cross any number of padded ancestors without moving content. Hosts should paint
shared surfaces under a common clip, rather than clipping each fragment to its source.

Palette resolution is separate from layout. Theme spacing is cheap to resolve during
layout; derived OKLab colors should be cached by the host until palette/mode changes.
Contrast guarantees concern the final opaque sRGB colors and named background pairs.

A scene commit publishes only after successful layout and geometry resolution. No
geometry or text measurement belongs on the real-time audio callback. The current
foundation has no event system or retained widget state.

`Ui::set_theme` stages a new theme, re-resolves stored rounding rules and spacing tokens,
validates geometry and contrast, and publishes the update transactionally. Tap metadata
is declarative: the host recognizes gestures and calls `tap_at` on the matching resolved
scene. Outlines and hit rectangles are intentionally independent. `outlines` replaces
merged member paint paths with one union while leaving content metadata intact.
