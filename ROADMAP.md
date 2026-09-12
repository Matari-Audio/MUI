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
- [x] Naming a node is optional. `leaf`/`row`/`column`/`overlay` build the
      tree and `.id()` names only the nodes you look up or hang a surface
      off; the same id is the surface id, so there is nothing to keep in
      step. `Kind::Branch` replaces `Kind::Stack`, which used to mean
      row-or-column while `dsl::stack` meant overlay.

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

`Theme` carries a `Palette`: four colours -- surface, accent, ink, error --
and two steps. Everything else is derived from them, so a light theme is
`step` and `hover` negated and two colours swapped, not a second table of
literals to keep in step with the first.

Colour is Oklch because every derivation here is a move in lightness or
chroma, and only a perceptual space makes the same move look the same on
every hue. The `color` crate supplies the space; it adds no crates to the
graph, since vello already pulls it in through peniko. It has no gamut
mapping -- only per-component clipping, which its own docs call perceptually
poor -- so CSS Color 4 13.2 bisection lives in `mui-core`. Measured: a +0.15 L
lift on the accent swings hue -27.5 degrees under clipping and -7.3 under
bisection.

- [x] A colour type and named roles (surface, accent, ink, error) in `Theme`.
- [x] Derived states: `hover`, `pressed`, `disabled` and `on` computed from a
      base colour instead of enumerated. The nine hand-typed constants in
      `crates/mui-preview/src/ui.rs` became one `Palette::DARK`, and each
      widget's state table became one `lift(base, &response)` call.
- [x] Elevation as `layer(level)`: one perceptual step per layer off the
      surface, so a raised control and a recessed well are a number rather
      than two more constants.
- [x] Light/dark pairing by construction. `step` and `hover` carry the sign,
      and `on(bg)` picks whichever of ink and surface sits further from `bg`
      in perceptual lightness, so legibility survives the flip.
- [ ] Contrast guarantees -- `on` picks the further of two, which is not the
      same as clearing a WCAG or APCA ratio.
- [ ] A palette on the wire is only the four colours and two steps; a theme
      that wants an off-palette colour (the preview's debug frame overlay) has
      nowhere to put it.
- [ ] A `Style` binding a surface to its fill, stroke and radius rule, so a
      scene is styled once rather than at every draw call.
- [ ] Per-surface stroke width. There is currently one global number.
- [ ] Inheritance: spacing and stroke do not cascade or derive from a parent
      the way radii do.

### Layout

- [x] `basis`, so a child can claim a share of the axis rather than a share of
      the surplus -- CSS `flex-basis`, with `Node::flex(w)` as the
      `grow(w).basis(0.0)` shorthand. This is what the left/centre/right bar
      needed: with children of 50, 30 and 20 px in a 400 px row, both
      `SpaceBetween` and `grow(1)` side cells put the middle child's centre at
      215 rather than 200, out by exactly half the asymmetry between the outer
      two. Equal shares put it at 200.
- [x] `shrink`. A deficit now comes back weighted by `shrink` scaled by basis,
      the way flexbox weights it, floored at each child's declared `minimum`;
      a child that freezes at its floor drops out and the rest redistributes.
      Previously surplus was clamped at zero and `arrange` returned
      `InsufficientSpace`, so a window dragged narrower than its content failed
      the whole layout rather than compressing. Default is 1, as in CSS.
- [x] `align_self`, overriding the parent's `align` for one child.
- [x] The flex fraction in intrinsic sizing. A hugging row is sized so that the
      hungriest flexible child's *share* still clears its content, rather than
      by summing children: the left/centre/right bar hugs to 130, not the 100 a
      sum would give, which would have squashed a 50 px end to 35. It is
      therefore centred at its hugging size too, and `basis` needs no
      special case for indefinite axes.
- [x] Floors that derive from children. A node's floor is every `minimum` in
      its subtree, summed along the axis they sit on, rather than only what the
      node declared itself. It binds while the deficit is being shared out and
      not merely as a check afterwards, so a squeezable sibling absorbs what a
      frozen child will not give up. Before this, a parent could be shrunk to a
      width its own contents then overflowed, silently.
- [ ] An automatic content-based minimum for *leaves*. A leaf is opaque here --
      it is the content measurement, so there is no smaller version of it to
      discover -- and its floor is whatever `min_size` it declares, defaulting
      to zero. CSS derives one from min-content instead. In practice this wants
      a real min-content pass, which wants text measurement, which is the text
      leaf below.
- [ ] Fractional and percentage sizing. `Size` is absolute pixels.
- [ ] Per-child margins, and the auto-margin idiom that goes with them. Note
      that auto margins would not have solved the centred bar either: they
      split free space equally, which lands the middle child in the same wrong
      place that `SpaceBetween` does.
- [ ] `SpaceAround` and `SpaceEvenly`. Same match arm as `SpaceBetween`.
- [ ] `order`, so visual order can differ from document order.
- [ ] Aspect-ratio constraints.
- [ ] Baseline alignment, so text sits on a shared baseline instead of being
      centred as a box.
- [ ] Wrapping rows, grid, spans.
- [ ] Anchored positioning inside `Overlay`. It centres; a child cannot be
      pinned to a corner with an offset.
- [ ] Text as a first-class leaf. Nothing calls `mui-text` to measure a node,
      so every text node is hand-sized today.
- [ ] `gap` and `padding` take raw `f64`, so a themed `.gap(M)` is
      impossible. They need `impl Into<Spacing>` and the theme threaded into
      `resolve`, which is the same plumbing the colour work needs.

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

`basis`, `shrink`, `align_self` and colour are done. Next: text as a layout
leaf, which is what makes the solver usable for real content. Then the `vello_cpu` adapter, since headless snapshot tests are what
every later feature wants to be checked by. Then SVG import and a clock, which
together are the Material Symbols goal that started this.
