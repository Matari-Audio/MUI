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
      renders as hinted glyph runs, and winit's wheel, keys, modifiers and
      cursor icon ride through `Input` / `Frame`.
- [x] Core DSL sugar: `row!`/`col!`/`stack!`/`grid!` taking anything
      `IntoEl`, `.w`/`.h`/`.square` on bare integers, `.center`/`.start`/
      `.end`/`.between`, `title`/`label`/`caption`.
- [x] Clips: `.clip()` and `.scroll()` as a `Clip`/`Unclip` layer pair in the
      paint list, honoured by the renderer and by hit testing.
- [x] Floats: `.float()` keeps its layout slot and is painted after the root.
- [x] Text-run cache across frames (`resolve_scene_with`, owned by `Ui`).
- [x] Glyph runs: text reaches Vello as a hinted run, not a filled outline;
      the font blob is interned so Vello's hinted-outline cache survives.
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
- [x] Image eviction: both image caches key on the buffer's `Arc` and drop
      the entries the app has let go of, `Renderer::destroy_image` included,
      and an image too big for an atlas tile falls back to a solid.
- [x] Preview: an F12 inspector, `MUI_PREVIEW_THEME` hot reload, a frame-cost
      title bar, and a scene per feature above.
- [x] Responsive without breakpoints: `clamp(min, pct, max)` lengths,
      `.min_col(px)` auto-fit grids, a grid cell clamped to its track, a float
      pulled back inside the box it floats in, `SpaceBetween` on one child as
      `flex-start`, and `Error::InsufficientSpace` carrying the tree's floor
      so a host can scale by `offered / needs`. The preview's Responsive
      editor is the same tree at 240x600, 800x500 and 2000x300.
- [x] A squeezed wrapping row raises `InsufficientSpace` instead of painting
      over its neighbour: `arrange` sums the lines it just broke.
- [x] Device-grid paint: `SceneSpec::scale` / `Ui::scale` snap every edge,
      clip and baseline through one `bounds` and one `snap`, so abutting
      fills have no seam and a hinted paragraph has even leading.
- [x] Per-line baselines (a `.baseline()` row taller than its text keeps its
      letters in their frames), a wrapped paragraph reporting its column
      rather than its longest line, and a stroke painted inside its frame.
- [x] A gradient shadow keeps its paint instead of going black; a tooltip
      lands where it was measured under a padded root; a long `text_input`
      value scrolls under a clip instead of wrapping.
- [x] Wrap in one pass everywhere: the flex pass re-measures a squeezed item
      at the main size it was dealt, so a paragraph beside another wraps in
      the one solve that sizes the row, `.min_col` works inside a share, and
      `wrap_hints` and the second solve are gone. Caching line breaks across
      frames stays unbuilt: `break_lines` is 0.051 ms of a 1.23 ms resolve.
- [x] A welded shadow blurs: the walk emits one analytic blurred rect per
      welded child instead of a rect-less entry the renderer dropped. Blend
      modes and opacity too -- `.blend(Mix::Multiply)` / `.opacity(0.5)`
      become a `Layer::Blend`/`Unblend` pair the renderer pushes as a Vello
      compositing layer.
- [x] UAX#14 line breaking: `break_lines` takes its opportunities from
      `unicode-linebreak`, so CJK breaks between ideographs and a no-break
      space or an emoji ZWJ sequence holds together. A word wider than the
      line still overflows at a char, not a grapheme cluster.

## Missing

- [ ] Kurv rewritten on MUI: the first real plugin editor on this stack, and
      the only honest test of whether the DSL survives a product.
- [ ] Real semantic roles in `mui-core`, so a widget describes itself and
      `mui-access` stops reporting every surface as a group.
- [ ] `mui-access` wired into a window: nothing calls it yet. `accesskit_winit`
      0.33 matches the preview's winit 0.30, so the preview is the first host.
- [ ] `vello_hybrid` against classic `vello`, re-measured on Windows — the
      hybrid choice was made on Linux numbers only.
- [ ] A welded shadow is the union of the children's blurred rects, not the
      blur of the welded outline: the seams are rounded where the outline is
      straight or concave-filleted. A blur filter layer
      (`vello_common::filter_effects`) is the exact fix, once `vello_cpu`
      stops panicking on a filter in a multi-threaded context.
- [ ] `.min_col` on a grid that hugs: with no width offered at all there is
      nothing to drop columns against, so it keeps its declared count. A flex
      share now counts as a width; a hugging grid still needs one.
- [ ] The first frame of a freshly-populated over-long `text_input` shows the
      head of the value: the widget has no inner width before its first
      layout. It catches up on the next frame.
- [ ] Node identity: `ResolvedScene::keys` is gone (`surfaces()` yields paint
      order and every surface carries its key), but focus rings, scroll
      offsets and `mui-access` still key on `String` paths, so renaming a
      node silently resets its state.

## Order

Kurv first: everything above is guesswork until a shipping editor uses it,
and it is the only item left that can change the DSL. Then semantic roles,
since `mui-access` is a crate nobody can use until widgets describe
themselves, then the Windows re-measure before any renderer decision is
locked in.
