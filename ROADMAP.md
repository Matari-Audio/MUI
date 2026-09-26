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
- [x] Scene (was `mui-core`): the `Paints`/`Styled` DSL (`fill`, `stroke`, `radius`, `pill`,
      `shadow`, `shell`, `weld`, `text_size`), `Role`/`Fill`/`Gradient`/`Paint`,
      Oklch palette with checked legibility, the tree walk to a z-ordered
      paint list, `Spring`.
- [x] Input: hit testing against painted paths, capture, hover, click, drag.
- [x] Vello: one `Canvas` trait over classic `vello` (vendored, wgpu 30,
      `effects::GpuRenderer`) and `vello_cpu`, CSS-angle gradients, analytic
      blurred shadows, a CPU pixel snapshot test. (Shipped first over
      `vello_hybrid`; that backend is gone, see MIGRATION.md.)
- [x] `mui::Ui`: the per-frame runtime with spring-smoothed hover and press;
      `slider`, `knob`, `toggle`, `button` as compositions of flex shares.
- [x] Preview: the gallery is one `mui` tree, sidebar included, its text
      renders as hinted glyph runs, and winit's wheel, keys, modifiers and
      cursor icon ride through `Input` / `Frame`.
- [x] Scene DSL sugar: `row!`/`col!`/`stack!`/`grid!` taking anything
      `IntoEl`, `.w`/`.h`/`.square` on bare integers, `.center`/`.start`/
      `.end`/`.between`, `title`/`label`/`caption`.
- [x] Clips: `.clip()` and `.scroll()` as a `Clip`/`Unclip` layer pair in the
      paint list, honoured by the renderer and by hit testing.
- [x] Floats: `.float()` keeps its layout slot and is painted after the root.
- [x] Sticky: `.sticky()` pins a child to the enclosing scroll viewport's
      leading edge for the length of its section, through the same second
      placement pass as a pin, and `Layout::min_size` / `Ui::min_size` report
      the intrinsic floor a host sizes its window against.
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
- [x] A native CLAP/VST3 editor host and parameter bridge: `mui-truce`'s
      `MuiEditor` embeds a baseview + wgpu child window (host scale, resize
      floor, focus-loss cancel, clipboard, cursors, idle skipping, GPU
      recovery), and `Bridge::bind` turns `Ui` edits into the host's
      begin/set/end. `examples/gain-plugin` passes pluginval's editor tests.
- [x] Images: `Image::rgba` + `Fill::Image` with `Cover`/`Contain`/`Fill`,
      and `Path::from_svg_data` for an icon's `d` attribute. `vello_cpu`
      paints the pixmap; classic `vello` uploads it into its image atlas
      once and paints by id.
- [x] AccessKit: `mui-access` turns a `ResolvedScene` into a `TreeUpdate`,
      and the preview feeds it to an `accesskit_winit::Adapter`.
- [x] `mui_vello::PathCache` / `paint_cached`: measured at ~0.08 ms a frame
      on the bench editor, then deleted as not worth its API.
- [x] Image eviction: the renderer-owned `mui_vello::Cache` holds a `Weak`
      per buffer and drops the entries the app has let go of,
      `Renderer::destroy_image` included, and an image the atlas has no room
      for falls back to a solid.
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
- [x] `.min_col` on a grid that hugs: with no offered width the declared
      column count stands, but the grid widens itself to the minimum instead
      of squeezing its cells under it -- the hug grows, the floor does not, so
      a flex ancestor can still squeeze it into fewer columns. Every modal in
      a plugin editor is a hugging container.
- [x] The scale contract, written down in `ARCHITECTURE.md` and held by the
      CPU snapshot example: it renders the gallery at 1x, 1.5x and 2x, and its
      tests assert layout is identical at every scale, that a snapped edge is
      within half a device pixel of it, and that the 2x render is a 2x
      rasterisation rather than an upscaled 1x.
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
- [x] Real semantic roles: `.role(Kind::..)` / `.label(..)` on any node, set
      by every widget, so `mui-access` reports a named button and a slider
      with its range instead of a pile of groups, and the preview keeps an
      `accesskit_winit::Adapter` that publishes after each frame and serves
      Focus and Click action requests.
- [x] Presets that merge instead of clobbering: `Style::over`, `.preset(&s)`
      / `.base(&s)` / `.apply(f)`, `Paints` implemented for a bare `Style`,
      `.on(State::Hover, |s| ..)` resolved by the runtime, `.full()`,
      `Spacing::Step` over `SpacingScale::unit`, `Role::alpha`, and
      `mui::presets::{panel, card, glass, chip, tile}`. `.pad(M).pad(12.)` is
      12 px now: a pixel pad clears the token slot.
- [x] Paint the renderer already did: `GradientKind::{Linear, Radial, Conic}`
      behind `Gradient::{linear, radial, conic, vertical}`, `Style.shadow` a
      list, `ShadowKind::Inset` and `Shadow.spread` through
      `fill_blurred_rounded_rect`'s `invert`, `Elevation::{Flat, Raised,
      Floating}`, and `.mask(fill)` as one source-atop layer.
- [x] Semantic widgets: `Variant::{Solid, Soft, Outline, Ghost}` and one
      `Xs..Xl` size scale off `Theme.control`, every widget returning a
      `Control` (`.variant .role .size .px .el`), faces derived from the
      palette rather than a colour table, `knob`'s raw `f64` size gone, and
      `.join()` on a row or column.
- [x] Corners as a theme vocabulary: `Corners { selector, field, box_,
      concave }` with `.radius(Corner::Field)`, and `CornerStyle::Squircle`
      restyling welds, shells and strokes alike (it gives up the analytic
      blur for that node).
- [x] Geometry the DSL could not say: `.cut(el)` / `.keep(el)` exposing
      `boolean`'s difference and intersection as outlines, `Len::Container`
      (`cq()`) resolved against the nearest sized ancestor, and `fits![..]`
      picking the first candidate that clears the offered box in one pass.
- [x] `Pin`: a float placed against another node by name -- nine `Area`s, a
      gap, `Match::{Width, Height}`, and ordered `fallback`s tried until one
      fits the root. `Ui`'s tooltip is a pin now, and `Frame::tip` reads the
      resolved frame back.
- [x] IME: `Input::ime` carries the platform's four events, a preedit paints
      under the caret without ever joining the value, a commit inserts like
      typed text, and `Frame::ime` puts the host's candidate window under the
      field. No selection highlight while a composition is up.

- [x] Pointer modifiers and the second button: `PointerInput` carries a
      `Buttons` set and the shared `Mods`, `Response` reports the button that
      pressed, the modifiers at the press and now, the travel since the press
      and `drag_axis()`, and Shift is the fine drag every parameter wants
      (`Ui::drag` applies `FINE_DRAG`). A click is `clicked_with(button)`, so
      a secondary click resets a control instead of toggling it. The preview
      feeds winit's three buttons and its modifiers; the Pointer gestures
      scene is the proof.

- [x] Canvas hit shapes: a `Draw` carries a `tag`, and a tagged draw is the
      canvas node's hit geometry instead of its frame -- `Draw::hit` is the
      shape that responds without painting. The pointer outside every tagged
      path is outside the node, so a drawn ring answers in the ring and not
      in its hole, and `Ui::tag(id)` names the shape under the pointer,
      latched at the press so a drag keeps the knot it grabbed. The Canvas
      hits scene is the proof.

- [x] `State::Disabled` and `.disabled(flag)`, which are one feature: the
      node paints what it declared for `State::Disabled`, drops out of the
      hit map and out of Tab, reports `disabled` to `mui-access`, and passes
      all of that to its subtree. A gesture already in flight on it is
      cancelled with the `Edit::End` its host is owed.

- [x] `curve(ui, id, &mut Curve)`: the envelope editor over the cubic model
      `mui-motion` already had. One canvas, whose knots (`n{i}`) and tension
      handles (`out{j}`/`in{j}`) are its hit shapes, so the drawn disc is the
      grab disc and the spine between them is not a target. The drag lands on
      `Curve::move_point`/`move_handle`, which clamp a knot to its neighbours
      and a handle to its segment; Shift is the fine drag and Alt at the press
      locks the axis. The path is fed the model's own control points, so what
      is drawn and what `Curve::evaluate` samples for the DSP side are one
      curve. The Curve scene is the proof.

- [x] `bins(ui, id, &Bins)`: the additive bin display, in the spirit of
      Razor's bin view. One canvas and one hit shape -- the pointer's x
      becomes a bin index arithmetically, not through a thousand tagged
      draws -- so a press-drag paints every bin between the last sample and
      this one, Shift refines from the level the press landed on, a
      secondary click resets the bin under the pointer, and the arrows
      select, jump and nudge while it holds the focus. `Bins` carries the
      authored spectrum, the engine's live levels (drawn as a cap line over
      the bars), a linear or log axis, per-bin positions for inharmonic
      partials and the selection; `BinEdit` is what the caller applies to
      its own model. Bars coalesce to one per pixel column at their
      maximum, so 1024 partials at 200 px are 200 draws. The Bins scene is
      the proof.

- [x] `Ui::shortcuts()`: every key this frame, whatever holds the focus, so
      undo/redo and the function keys work with nothing selected. A focused
      `text_input` consumes the stream and nothing else does. `Key` grew
      `Space`, `PageUp`, `PageDown` and `Function(n)`; the preview feeds all
      of them. The Disabled + shortcuts scene is the proof.

- [x] Drag payloads and composed ids. `Ui::start_drag(id, payload)` attaches
      any `Any + Send` value to the gesture in flight, `Ui::dragging::<T>()`
      is the look at it a drop target wants before it lights up, and
      `Ui::dropped_on::<T>(id)` takes it -- once, on the one frame the drop
      is reported, and only from the drag that actually landed there. A ghost
      stays the caller's: it is a `.float()` pinned to the pointer's surface.
      `Id::of("osc").slot(3).field("gain")` composes a name into the same
      `/`-joined key space with no allocation up to 46 bytes, `.id()` takes
      anything `Into<Id>`, and `&str` still works everywhere it did. The
      Pointer gestures scene is the proof.

- [x] The crate split: theme data, motion and the scalar/spacing vocabulary
      sit under the element tree (`mui-style`, `mui-motion`, `mui-geometry`),
      `mui-core` is `mui-scene`, the controls are `mui-widgets` behind a
      `Host` trait (since folded into `mui::widgets`, taking `&mut Ui`
      directly), and `mui` is the facade that owns the runtime and the
      prelude. The graph is acyclic; the old `mui::core` alias is gone,
      use `mui::scene`.

- [x] Live readouts and one-seed palettes: `.reserve("-88.8 dB")` measures a
      text node for the widest value it will ever show, `Ui::set_text(id, s)`
      then swaps what it says while the resolved frame stands -- one glyph run
      re-shapes, the tree is not walked again, which is the whole point at 60
      Hz. `.text_weight(Weight::BOLD)` drives the run's `wght` axis and the
      position reaches the renderer as `Text::font_coords`, so a bold run is drawn
      at the instance it was measured at. `Palette::from_seed(accent, mode)`
      derives every role from one colour, hue-swept in both modes against
      `UI_NONTEXT` and `AA_TEXT` instead of trusting a hex table.

- [x] Motion beyond paint: `.animate_layout()` springs a node's solved
      frame between the solve and the walk (a child that animates springs
      relative to its animating ancestor, one that does not rides along),
      `.appear(Appear::..)` enters from an offset or scale and fades out where
      it stood, `.morph(name)` morphs the outline through
      `mui_geometry::morph` when the name changes, `.identity(x)` carries
      springs, glides, focus, scroll, selection and a drag capture across a
      rename, `Keys`/`Ease` keyframes land motion on a time and `Ui::play`
      runs them on the runtime clock. Paint channels have stable ids, and
      opacity, stroke and shadow colour, shadow offset and spread, shell
      colour and gradient stops spring. `examples/motion_strip.rs` renders
      it as a filmstrip; the Motion scene is the proof.
- [x] Trailers without a screen recorder: `mui-reel` plays a beat-timed
      script (pointer paths, drags, clicks, keys, notes, a springed camera)
      against the real editor on the caller's clock, and writes a
      BT.709-tagged take, sample-locked audio, alpha layers, a per-frame
      surface track, and HyperFrames and Remotion handoffs. `mui-stage`
      shoots the same take on the GPU: layers on lit, extruded slabs under a
      perspective camera that can follow the script's punch-in, extruded 3D
      text, a reflective floor, a WGSL background, depth of field, bloom,
      lens and grain, and subframe motion blur averaged in linear light
      (`gain_reel --stage`). Layers are mipmapped, premultiplied linear
      light, so distant fine detail averages instead of shimmering.

## Missing

- [ ] `Ui::set_text` swaps one line: a wrapped label re-shapes to a single
      run rather than breaking again, because the swap deliberately does no
      layout. Re-breaking needs the measure pass it is avoiding.
- [ ] A pin whose anchor is itself inside another pinned float reads that
      float's first-pass position; a dependency-ordered pin pass is the fix.
- [ ] `mui-truce` in a real DAW: the editor passes pluginval's editor tests
      and clap-validator on Linux, but nobody has yet opened it by hand in
      Bitwig, Reaper or Ableton, on any OS. macOS and Windows are only
      compiled for, never run.
- [ ] `mui-truce` has no IME: baseview has no composition or candidate-window
      API, so `Frame::ime` is dropped and CJK input does not work in a
      plugin text field.
- [ ] `mui-truce` has no AU or AAX: truce builds them, but only CLAP and VST3
      are wired and validated.
- [ ] `mui-truce` swallows every key while focused: upstream baseview does not
      forward unhandled keys to the host, so DAW shortcuts (space for
      transport) stop while the editor has focus. Kurv's vendored baseview
      has the forwarding.
- [ ] The gain plugin fails three clap-validator state tests and Steinberg's
      `vst3 validator`: truce-clap 6.3 never requests a value rescan after a
      state load, and truce-vst3 6.3 declares the wrong
      `IProcessContextRequirements` IID. Both are one-line upstream fixes
      (Kurv vendors them); see `crates/mui-truce/README.md`.
- [x] Keyboard value stepping: a focused slider or knob steps on the arrow,
      Page and Home/End keys, bracketed as one edit.
- [ ] The text cache keeps exactly what the last resolve used, with no byte
      budget; `mui_vello::Cache` never evicts a font while its renderer lives.
- [ ] No fuzzing or property campaigns over layout, welding or text input.
- [ ] Kurv rewritten on MUI: the first real plugin editor on this stack, and
      the only honest test of whether the DSL survives a product.
- [ ] Classic `vello` (the GPU path since `vello_hybrid` was dropped)
      re-measured on Windows: the switch was made on Linux numbers only.
- [ ] A welded shadow is the union of the children's blurred rects, not the
      blur of the welded outline: the seams are rounded where the outline is
      straight or concave-filleted. A blur filter layer
      (`vello_common::filter_effects`) is the exact fix, once `vello_cpu`
      stops panicking on a filter in a multi-threaded context.
- [ ] No selection highlight while an input method is composing: the ends
      were measured against the value and the preedit sits between them, so
      the highlight is dropped for those frames. The commit still replaces
      the selection. Measuring the two runs separately is the fix.
- [ ] The first frame of a freshly-populated over-long `text_input` shows the
      head of the value: the widget has no inner width before its first
      layout. It catches up on the next frame.
- [ ] `mui-access` still keys nodes by name, so a screen reader sees a
      reordered slot as a new node; `.identity()` carries the runtime's own
      state across the rename but not the accessibility tree's.
- [ ] A morph's hit shape and analytic shadow stay the target's for the
      frames it lasts, and shells snap; exits fade on top of the paint list
      rather than at their old depth.
- [ ] `mui-stage` has no shadows cast between slabs, and its floor is a
      perfect mirror faded into its colour: no rough (blurred) reflection.
      Glyph walls follow the flattened outline, so a curve's facets show at
      an extreme close-up.

## Order

Kurv first: everything above is guesswork until a shipping editor uses it,
and it is the only item left that can change the DSL. Then the Windows
re-measure, before any renderer decision is locked in.
