# MUI roadmap

An inventory written against the tree. Every ticked line has a crate, a
public function and a test behind it.

## Done

- [x] Layout: intrinsic row/column/overlay/grid, `basis`/`grow`/`shrink`,
      `align`/`justify`/`align_self`/`anchor`/`offset`, percentage and
      aspect sizes, spacing tokens resolved in the solver, content leaves
      measured by a callback, frames returned in tree order.
- [x] Geometry: Booleans with holes, adaptive convex/concave fillets, exact
      rounded-rect inset/outset, general parallel offsets, validation.
- [x] Text: glyph and string outlines from variable fonts, as paths.
- [x] Core: the `Styled` DSL (`fill`, `stroke`, `radius`, `pill`, `shadow`,
      `shell`, `weld`, `text_size`), `Role`/`Fill`/`Gradient`/`Paint`,
      Oklch palette with checked legibility, the tree walk to a z-ordered
      paint list, `Spring`.
- [x] Input: hit testing against painted paths, capture, hover, click, drag.
- [x] Vello: one `Canvas` trait over `vello_hybrid` and `vello_cpu`, CSS-angle
      gradients, analytic blurred shadows, a CPU pixel snapshot test.
- [x] `mui::Ui`: the per-frame runtime with spring-smoothed hover and press;
      `slider`, `knob`, `toggle`, `button` as compositions of flex shares.
- [x] Preview: the gallery is one `mui` tree, sidebar included.

## Missing

- [ ] Baseline alignment for text rows; multi-line text and wrapping.
- [ ] Wrapping rows and grid spans; `SpaceAround`/`SpaceEvenly`; `order`.
- [ ] Clips and blend modes; blurred shadows on welded shapes (drawn sharp).
- [ ] Text-run cache across frames (each `resolve_scene` shapes afresh).
- [ ] Keyboard focus traversal, IME, scroll as a library concept, AccessKit.
- [ ] SVG symbol import; keyframed animation beyond springs.
- [ ] Plugin parameter gestures (begin/end edit) surfaced from `Ui`.

## Order

Text baseline and a run cache first, since every real panel is labels.
Then clips, then SVG symbols.
