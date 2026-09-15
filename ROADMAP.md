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
- [x] Wrapping rows and columns, grid spans, `order`, `SpaceAround`/
      `SpaceEvenly`, and `Node::push` (the tooltip no longer wraps the root).
- [x] Multi-line text: measured line breaking, `.lines(n)`, `.baseline()`
      rows, and text metrics (`ascent`, `x_height`, caret hit testing).
- [x] Selection, copy, cut and paste in `text_input`, through
      `Frame::clipboard` / `Input::clipboard`.
- [x] Motion: springs by response and damping, `.animate()`/`.transition()`
      transitions that retarget mid-flight, and `Ui::tween`.
- [x] Plugin parameter gestures: `Ui::edit` / `Frame::edits` bracket every
      capture, cancelled ones included.
- [x] Images: `Image::rgba` + `Fill::Image` with `Cover`/`Contain`/`Fill`,
      and `Path::from_svg_data` for an icon's `d` attribute. `vello_cpu`
      paints the pixmap; `vello_hybrid` uploads it once through `Gpu`'s
      `Atlas` and paints by id.
- [x] AccessKit: `mui-access` turns a `ResolvedScene` plus a `Semantics` map
      into a `TreeUpdate`.
- [x] `mui_vello::PathCache` / `paint_cached`: a still frame re-encodes
      without reconverting a path.
- [x] Preview: an F12 inspector, `MUI_PREVIEW_THEME` hot reload, a frame-cost
      title bar, and a scene per feature above.

## Missing

- [ ] Kurv rewritten on MUI: the first real plugin editor on this stack, and
      the only honest test of whether the DSL survives a product.
- [ ] IME.
- [ ] UAX#14 line breaking: today a break is ASCII whitespace or a hyphen,
      which is wrong for CJK.
- [ ] Real semantic roles in `mui-core`, so a widget describes itself and
      `mui-access` stops reporting every surface as a group.
- [ ] `mui-access` wired into a window: nothing calls it yet. `accesskit_winit`
      0.33 matches the preview's winit 0.30, so the preview is the first host.
- [ ] `vello_hybrid` against classic `vello`, re-measured on Windows — the
      hybrid choice was made on Linux numbers only.
- [ ] Blurred shadows on welded shapes (still drawn sharp) and blend modes.
- [ ] Image eviction: `ImageIds` never forgets a buffer, so a plugin that
      streams images through the atlas grows it until `upload_image` panics.
      `Renderer::destroy_image` is the other half.
- [ ] Wrap in one pass everywhere: a paragraph squeezed by a flex row still
      needs a hint and a second solve, because the measurer only learns a
      column's or a grid cell's room. The flex pass re-measuring its items at
      their final main size retires `wrap_hints`. Then cache line breaks
      across frames: BENCHMARKS.md puts warm resolve at 2.2 ms, nearly all
      of it re-breaking text that did not change.

## Order

Kurv first: everything above is guesswork until a shipping editor uses it,
and it is the only item left that can change the DSL. Then semantic roles,
since `mui-access` is a crate nobody can use until widgets describe
themselves, then the Windows re-measure before any renderer decision is
locked in.
