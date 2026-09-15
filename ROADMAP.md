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
- [x] Preview: the gallery is one `mui` tree, sidebar included, its text
      renders through the glyph atlas, and winit's wheel, keys, modifiers and
      cursor icon ride through `Input` / `Frame`.
- [x] Core DSL sugar: `row!`/`col!`/`stack!`/`grid!` taking anything
      `IntoEl`, `.w`/`.h`/`.square` on bare integers, `.center`/`.start`/
      `.end`/`.between`, `title`/`label`/`caption`.
- [x] Clips: `.clip()` and `.scroll()` as a `Clip`/`Unclip` layer pair in the
      paint list, honoured by the renderer and by hit testing.
- [x] Floats: `.float()` keeps its layout slot and is painted after the root.
- [x] Text-run cache across frames (`resolve_scene_with`, owned by `Ui`).
- [x] Glyph runs: text reaches Vello as a hinted run through its own atlas,
      not a filled outline; the font blob is interned so the atlas survives.
- [x] Input: `Input` carries wheel, key presses and typed text; hits are
      rejected outside their clip; drag-and-drop reports source and target.
- [x] Runtime: keyboard focus (press, Tab/Shift+Tab, Escape), wheel scrolling
      clamped to content, tooltips after half a second, `Cursor` per surface,
      and a `text_input` widget.
- [x] `canvas(|size| ..)`: your own `Draw` paths in a node's own space.

## Missing

- [ ] Kurv rewritten on MUI: the first real plugin editor on this stack, and
      the only honest test of whether the DSL survives a product.
- [ ] Selection and clipboard in `text_input` (today: caret and insert only);
      IME.
- [ ] Baseline alignment for text rows; multi-line text and wrapping.
- [ ] Wrapping rows and grid spans; `SpaceAround`/`SpaceEvenly`; `order`.
- [ ] Blurred shadows on welded shapes (still drawn sharp) and blend modes.
- [ ] `Node::push`, so the tooltip overlay stops wrapping the root and
      shifting unnamed decoration keys on tip frames.
- [ ] `vello_hybrid` against classic `vello`, re-measured on Windows — the
      hybrid choice was made on Linux numbers only.
- [ ] AccessKit; SVG symbol import; keyframed animation beyond springs.
- [ ] Plugin parameter gestures (begin/end edit) surfaced from `Ui`.

## Order

Kurv first: everything above is guesswork until a shipping editor uses it.
Text selection and baselines next, since every real panel is labels, then
wrap, then the Windows re-measure before any renderer decision is locked in.
