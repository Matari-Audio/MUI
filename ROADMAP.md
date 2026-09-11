# MUI roadmap

An inventory of the library as it stands, and of what is deliberately not in it
yet. Written against the tree, not against intentions: every ticked line below
has a crate, a public function and a test behind it, and every unticked line is
absent from the source rather than merely undocumented.

`README.md` describes what MUI *is*. This file is the honest gap list. If the
two ever disagree, this one is wrong and should be corrected from the code.

## Done

### Geometry — `mui-geometry`

- [x] Boolean union, intersection, difference and XOR over placed shapes, with
      holes, topology cleanup and validation.
- [x] Adaptive fillets on both convex and concave corners.
- [x] Exact analytic rounded-rectangle inset and outset.
- [x] General parallel path offset. Circular paths use a bounded polygon
      approximation; rounded rectangles take the exact route above.
- [x] Parent-relative corner derivation — `NestedRadius::{Concentric,
      Proportional, Independent}`, which keep the distinction that only
      `Concentric` also derives the child's bounds and so is the only one that
      guarantees a uniform band.
- [x] The arithmetic nobody wants to redo by hand: `inset_arc_radius`,
      `inset_for_stroked_gap`.
- [x] Paths: flatten to a tolerance, validate against a command budget, rigid
      transform, SVG export.

### Layout — `mui-layout`

- [x] Intrinsic row, column and overlay solver with hug-content sizing.
- [x] Padding (uniform, per-axis and per-side), gaps, min and max size,
      weighted growth, fill.
- [x] `Align::{Start, Center, End, Stretch}` on the cross axis and
      `Justify::{Start, Center, End, SpaceBetween}` on the main axis.
- [x] Transactional commit behind a revision counter, so a rejected layout
      never half-replaces the previous one.
- [x] No dependencies at all. `#![forbid(unsafe_code)]`, as everywhere else.

### Scene derivation — `mui-core`

- [x] A surface is a layout frame, a Boolean merge of other surfaces, or a
      parallel inset/outset **of another resolved surface**. That is the
      parent-relative graph, and children are derived from the final merged
      outline rather than from guessed radii.
- [x] Cycle and missing-parent detection.
- [x] `Theme` carrying a `CornerProfile` and a `SpacingScale`, with
      `Spacing::Token` resolving through it.
- [x] The same transactional revision model as layout.

### Rendering and input

- [x] GPU: `mui-vello` -> `vello_hybrid` -> wgpu, in a windowed live preview.
- [x] `mui-tessellate` -> Lyon triangle meshes, renderer-independent, so a
      second backend does not start from paths again.
- [x] `mui-text`: variable-font glyph and string outlines as MUI paths in the
      same coordinate space as every other surface. No atlas, which is what
      makes animating Material Symbols `FILL` 0 -> 1 an axis value rather than
      an asset pipeline.
- [x] `mui-input`: hit testing against real paths, press capture, hover, click
      and drag with a threshold.
- [x] The whole workspace except the preview binary compiles to
      `wasm32-unknown-unknown`.

## Missing

### Colour and derived styling

The largest gap. `Theme` today is corners, spacing and one stroke width; there
is no colour type anywhere in the library. Every colour in the gallery is a
hardcoded constant in `crates/mui-preview/src/ui.rs`, and the fact that
`ACCENT` and `ACCENT_LIT` are both written out by hand is the symptom this
section exists to fix.

- [ ] A colour type and named roles (surface, on-surface, accent, error) in
      `Theme`.
- [ ] Derived states: hover, pressed and disabled computed from a base colour
      instead of enumerated. `peniko`'s `color` already ships Oklch, so the
      perceptual step costs a dependency we have.
- [ ] Elevation tint, light/dark pairing, contrast guarantees.
- [ ] A `Style` binding a surface to its fill, stroke and radius rule, so a
      scene is styled once rather than at every draw call.
- [ ] Per-surface stroke width. There is currently one global number.
- [ ] Inheritance: spacing and stroke do not cascade or derive from a parent
      the way radii do.

### Layout

- [ ] Asymmetric three-slot alignment. `SpaceBetween` on children of 50, 30 and
      20 px spreads them edge to edge, which does not put the middle child's
      centre on the container's centre; three `grow(1)` cells do, but then each
      side child is confined to a third. What is missing is a mode where the
      outer children hug their content at the edges and the middle child is
      centred on the *container*.
- [ ] Fractional and percentage sizing. `Size` is absolute pixels; weighted
      growth is the only relative mechanism.
- [ ] Aspect-ratio constraints.
- [ ] Baseline alignment, so text sits on a shared baseline instead of being
      centred as a box.
- [ ] Wrapping rows, grid, spans.
- [ ] `SpaceAround` and `SpaceEvenly`.
- [ ] Anchored positioning inside `Overlay`. It centres; a child cannot be
      pinned to a corner with an offset.
- [ ] Text as a first-class leaf. Nothing calls `mui-text` to measure a node,
      so every text node is hand-sized today.

### Rendering

- [ ] A CPU backend. `vello_cpu` is a sibling crate and `mui-tessellate`
      already emits backend-agnostic triangles, so this is an adapter rather
      than a project — and it is what headless snapshot testing needs.
- [ ] Gradients, shadows, blurs, blend modes, clip stacks. `mui-vello` is fill
      and stroke in flat colour.
- [ ] Damage tracking and partial redraw. The preview reshapes every label
      every frame; that cost only stopped being visible because dependencies
      are now optimised in dev builds, which is a mitigation and not a fix.
- [ ] Path and tessellation caching across frames at the library level.

### Everything built on top

- [ ] SVG symbol import. `vello_common::pico_svg` parses SVG and nothing maps
      it into a `mui_geometry::Path` yet. This is the Material Symbols path.
- [ ] Animation: a clock, easing, interpolation, springs. Needed before an
      unfilled symbol can fill.
- [ ] A widget library with retained state. The preview's `Chrome` is a
      prototype that proves the pieces connect, not an API.
- [ ] Keyboard input beyond a single character; focus traversal; IME.
- [ ] Accessibility (AccessKit).
- [ ] Scroll and virtualization as library concepts. The preview hand-rolls a
      scroll offset for its own sidebar.
- [ ] Hot reload and on-the-fly component testing.
- [ ] Plugin parameter gestures.

## Order

Colour and derived roles first: it unblocks everything visual and deletes the
constant block in the preview. Then text as a layout leaf, which is what makes
the layout solver usable for real content. Then the `vello_cpu` adapter, since
headless snapshot tests are what every later feature wants to be checked by.
Then SVG import and a clock, which together are the Material Symbols goal that
started this.
