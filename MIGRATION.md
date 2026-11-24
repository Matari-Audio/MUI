# Migrating to the next MUI release

Every entry is `old -> new`. Crates are listed in dependency order.

## Unreleased

### Intrinsic gaps found by a plug-in editor (`mui/intrinsic-gaps`)

Additive unless marked **breaking**.

- **Layout.** `.pad((x, y))` takes pixels or tokens per axis in any mix:
  `(S, Xs)`, `(M, 4)`, `(16, 8)`. `Pad` gains `Pad::Axes(Spacing, Spacing)`
  (**breaking** only for an exhaustive `match` on `Pad`).
  `Node::at_pct(x, y)` puts the node's anchor point (`.anchor(..)`, centre by
  default) at `x`% / `y`% of the parent's padding box; `.offset` nudges,
  nothing clamps; `at`, `centered_at`, `centered` replace it.
  `Node::turn(Turn::Cw | Turn::Ccw)` is a quarter turn that stays intrinsic:
  the parent sees the box swapped, `Layout::frame` is the upright box centred
  on its slot, and the scene rotates paint and hits onto the slot. Replace
  `.rotation(-FRAC_PI_2)` plus a hand-sized slot with `.turn(Turn::Ccw)`.
  `mui::audit`'s `Escapes` now judges rotated and turned children by what
  they cover.
- **Pinned floats are content-sized** (bug fix, visible change). A
  `.pin(Pin::to(..))` node no longer takes its parent's stretch; it is
  measured at its content size on both axes unless an axis opts back in with
  `Pin::match_width`/`match_height`, a definite length, or
  `.anchor(_, Align::Stretch)`. A content-sized menu now drops under its bar
  instead of filling the window and being pulled back over it. The
  `.anchor(Align::Start, Align::Start)` workaround before `.pin(..)` can go.
  Unpinned `.float()` nodes are unchanged.
- **Text.** `TextAlign { Start, Center, End }` and `Styled::text_align` (both
  preludes): each line is placed across its box's spare width, inside a
  `.reserve`/`.reserve_all` box, a stretched width, or a wrapped paragraph,
  and `ResolvedScene::set_text` re-places a swapped run the same way. Start is
  the default; editable text stays Start. `Extras` gains `text_align`.
  Replace an invisible stacked twin with `.reserve(widest).text_align(TextAlign::Center)`.
- **Canvas paint.** `Draw::stroke(..).cap(Cap::Butt | Round | Square).join(Join::Miter | Round | Bevel)`
  (kurbo's types, in both preludes; Round/Round stays the default).
  `Draw::shadow(path, blur, (dx, dy), fill)` is a backend-blurred fill: exact
  for `Path::rect`, `RoundedRect::path` and the new `Path::circle`, a path
  blur otherwise, on both renderers. `Shadow::halo(blur)` and
  `Elevation::Halo` are centred shadows (**breaking** only for an exhaustive
  `match` on `Elevation`). `Painted` gains `cap`/`join` and `Draw` gains
  `cap`/`join`/`blur` (hand-built literals add them). An element shadow on an
  `.outline(..)` that is exactly a circle or rounded rect now uses the exact
  blur. New: `Path::circle`, `RoundedRect::of(&path)`.
- **Theme.** `Corners::TIGHT` (4/3/4, concave 3) for instrument panels. An
  unset radius still means `box_`; a node that must stay square says
  `.radius(0)`.
- **Icon font.** Opt-in `mui` features `icons` (= `icons-rounded`) and
  `icons-outlined` embed the full Material Symbols variable font (about 15 MB
  Rounded, 11 MB Outlined) and make it the icon face when
  `Theme::icon_font` is `None`; `theme.icon_font` and `.font(f)` still win.
  For a small binary leave them off, cut a subset with
  `tools/subset_symbols.py`, and set `theme.icon_font`. Underneath:
  `mui_symbols::font::{ROUNDED, OUTLINED}` (`font-rounded`/`font-outlined`),
  mui-scene `symbols-rounded`/`symbols-outlined`, and
  `mui_text::Font::from_static`, which opens an embedded face without
  copying it. `sym` is regenerated from upstream 2026-10-23 (20 new symbols,
  none removed).
- **`select` (breaking).** `select(..)` / `Overlay::select(..)` return
  `Response<bool, Select>` (was `Response<bool, El>`). `Select` takes the
  button look (`.variant`, `.role`, `.size`, `.px`) and finishes into the
  trigger itself, with the open list floated from inside it, so `.fill`,
  `.radius`, `.stroke`, `.pad` land on the trigger, open or closed. Where an
  `El` was required write `s.into_el()` (no mui-migrate rule: type-directed).
  The open list's accessible parent is now its trigger; the default head's
  hover lifts its own fill instead of swapping to `Role::Raised`. Every
  `Response` forwards `.radius(..)` and `.stroke(..)`.
- **Tooltips.** `Ui::set_tips(TipPolicy::Off | TipPolicy::After(secs))` and
  `Ui::tips()`; the default `After(0.5)` is unchanged. Off builds, paints and
  wakes for no tip. Tips were never screen-reader text; use `.described(..)`.
- **Controls.** `Control::fill_cell()`: a knob's face takes the largest
  square its cell leaves, so the parent's flex sizes each dial (`.grow(2)` /
  `.grow(1)` in a column, `.flex(2)` / `.flex(1)` across a row); the reading
  keeps the size token's text size.
  **Knobs.** `Control::look(Knob)` sets how a knob is painted; `Knob` holds
  `ring`, `width`, `inset` (shares of the radius), `track`, `arc`, `face`
  (fills, roles by default), `cap`, `shadow: Option<(blur share, Fill)>` and
  `pointer: Option<Pointer { from, to, width, dot, fill }>` (a line and/or dot
  at the value's angle; `fill: None` takes the arc's colour). Presets:
  `Knob::clean()` (the default), `Knob::domed()` (domed face, pointer line),
  `Knob::gradient_ring()` (gradient arc, no face), `Knob::sunken_dot()` (dot
  on the ring).
  They are `fn`s, not `const`s, because a gradient fill boxes.
  **Breaking:** the default knob is now `Knob::clean()` (square-cut thick track, ink
  arc, raised shadowed face); `Dial`, `Dial::DEFAULT`, `Dial::RING` and
  `Control::dial` are gone (`Knob { .. }`, `Control::look`), and
  `Control::dial_color` is `Control::accent`. A bipolar arc still comes from
  the domain's `origin`.
  `Control::icon(sym)` / `.icon_end(sym)` and
  `icon_button(ui, id, label, sym)` (label = accessible name and tooltip).
  `Control::value_align(TextAlign)` places a readout in its reserved width.
  All forwarded on `Response<_, Control>`.
- **Typed entry in units.** `Domain` gains `.format(f)`, `.parse(f)`,
  `.formatted(v)`, `.parsed(s)` (clamped, finite only); its `Debug` is now
  hand-written. Readouts use `format` when no `.value_text` is set.
  `drag_value` takes `impl Into<Domain>` (widening). A knob or slider whose
  domain has a parser opens a field on Enter, and on double click when the
  domain has no `default`; Escape cancels. Without a parser nothing changes.
  mui-truce's `Bridge::knob`/`slider` build a per-parameter `Domain` from
  truce's `format_value`/`parse_value`, falling back to inverting truce's
  default unit text ("1.0 kHz", "1.2k", "250 ms", "50%"); a double click on
  them now opens typing (they set no default to reset to).

`mui::audit::audit(ui, sizes, build)` builds an editor at each size, lets it
settle and returns typed `Finding`s: `Escapes`, `FloorViolated`,
`IdleRebuild`, `Unnamed`, `Occluded` (and `Failed`). A plugin's whole check
is `assert!(audit(..).is_empty(), "{findings:#?}")`. To support it,
`ResolvedSurface` gains `floating: bool` (inside a float); a struct literal
adds `floating: false`.

`mui_layout::Intrinsic` gains `fluid: bool`: whether another room could
change the measured size. A content leaf that says `false` is not measured
again at its flex share, and a row of such leaves isn't either. `From<Size>`
says `true`, so a measurer returning a bare `Size` behaves as before; a
struct literal adds `fluid: true` (or `false` when the size ignores the room).
`Id` keeps up to 40 bytes inline (was 46); longer names allocate once.

Multi-rect damage. A frame whose changes sit far apart repaints up to
`mui_vello::quad::RECTS` (4) separate rects instead of their union.
`quad::Frame` gains `rects: Rects` (each rect is `[x, y, w, h]` in device
pixels; `Rects::as_slice`, `Rects::bounds`); `Frame::damage` stays the bounds
of all of them. A presenter that copies or presents damage should walk
`rects`, not `damage`.

Per-entry CPU fallback. A vector, a bitmap or colour glyph run, an image
the device cannot hold, or a third nested clip no longer sends the whole
frame to `vello_cpu`: that entry (or the clip with everything inside it)
alone is rasterised, cached, and drawn as an image by the quad pass.
`EffectStats::cpu_patches` counts them; `cpu_fallback` and
`GpuRenderer::fallback` now count only whole-frame fallbacks (a backdrop or
weld inside a deep clip, a malformed layer list, an exhausted budget).

Native editors present from the render worker. The window thread makes the
wgpu surface and lends it to the worker, which presents to it: no readback,
no blit. Closing waits (at most 500 ms) for the worker to drop the surface.
macOS keeps the readback transport for now; `MUI_PRESENT=readback` asks for
it anywhere. `mui_baseview::{Presenter, frost, sticky}` are new: a direct
presenter for hosts whose model runs on the window thread, blur-behind and
keep-above window policies (moved from a host plugin).

GPU crash guard removed. Every editor or window open tries the GPU; a failed
adapter, device or surface puts that window on the CPU and the next open tries
again. Nothing on disk decides the renderer: `gpu_denials`,
`clear_gpu_denials`, `GpuDenial`, `GPU_DENIAL_*` and `MUI_GPU_IGNORE_DENYLIST`
are gone, and a leftover `gpu-trials` state directory is deleted on start.
Accepted cost: a driver that hard-crashes during GPU start-up crashes the host
on every open, not only the first.

Automatic retention (`perf/auto-retain`). No builder call changes: `Ui`,
`El`, `Ui::memo` and widgets build what they did. Three things a caller can
notice:

- `Canvas` is one pointer wide and its closure is private:
  `canvas.0(size)` -> `canvas.draw(size)`; `Canvas(Arc::new(f))` ->
  `Canvas::new(f)`. A `Canvas` from `canvas_keyed` compares equal to a
  later one from the same cache with an equal key (it was `Arc` identity).
- `Node::same_shell` ignores children entirely (it compared their count).
- `Ui::set_auto_retain(false)` turns automatic retention off; it is on by
  default and paints exactly what a full rebuild does. A frame bench that
  measured the full rebuild sets it off.


GPU renderer: the instanced SDF quad pass replaces compute-shader Vello, and
Vello's GPU path is deleted. The checklist is the diff of the public API,
features and manifests between `0c865588` (the last tree with Vello on the
GPU) and this one; a plugin that reaches MUI only through `mui` widgets,
`mui-truce` and `mui-baseview` needs steps 1 and 2 and nothing else.

1. Cargo features. The GPU feature is `gpu-quad`.
   - On `mui`: `gpu-effects` -> `gpu-quad`. The deprecated alias was removed
     on 2026-11-12, once every plugin named `gpu-quad`.
   - On `mui-vello`: `gpu-effects` is gone (no alias), `gpu-quad` replaces it.
     `gpu-quad` now implies `cpu` (it was `gpu-effects` alone), because the
     quad pass rasterises glyph masks and refused frames on `vello_cpu`;
     naming `cpu` beside it is harmless. A manifest with `features =
     ["cpu", "gpu-effects"]` becomes `["gpu-quad"]`.
   - On `mui-baseview`, `mui-winit`, `mui-preview` and `mui-truce` nothing
     changes for a user: each turns the feature on for `mui` itself (their own
     manifests moved from `gpu-effects` to `gpu-quad`).
   - `cpu`, `cpu-threads`, `software-window`, `test-support` and `reporting`
     are unchanged. There is no `vello-classic`.

   ```toml
   # before
   mui = { git = "https://github.com/Matari-Audio/MUI", rev = "0c865588", features = ["gpu-effects"] }
   mui-vello = { git = "https://github.com/Matari-Audio/MUI", rev = "0c865588", features = ["cpu", "gpu-effects"] }
   # after
   mui = { git = "https://github.com/Matari-Audio/MUI", rev = "<new>", features = ["gpu-quad"] }
   mui-vello = { git = "https://github.com/Matari-Audio/MUI", rev = "<new>", features = ["gpu-quad"] }
   ```

2. Bump the `rev` (or follow `main`) and run `cargo update -p mui`. The lock
   loses `vello`, `vello_encoding`, `vello_shaders`, `futures-intrusive` and
   `svg_fmt`; check with `cargo tree -i vello` (finds nothing). `vello_cpu`,
   `vello_common` and `glifo` stay.
3. Only if you call the renderer directly (`mui::vello::effects`,
   `mui::vello::host`, `mui_vello::*`):
   - Removed: `mui_vello::Classic` (also `mui::vello::Classic`) and the
     re-exported crate `mui_vello::vello` (`mui::vello::vello`). Draw overlays
     through the `Canvas` trait. They are rasterised on the CPU and never
     retained:

     ```rust
     // before
     host.present_with_overlay(scene, xf, |c: &mut mui::vello::Classic| draw(c))?;
     renderer.render_with_overlay(scene, xf, view, |c: &mut Classic| draw(c))?;
     // after
     host.present_with_overlay(scene, xf, |c: &mut dyn mui::vello::Canvas| draw(c))?;
     renderer.render_with_overlay(scene, xf, view, |c: &mut dyn Canvas| draw(c))?;
     ```

     (`Host::present_with_overlay<F: FnOnce(&mut Classic<'_>)>` and
     `GpuRenderer::render_with_overlay<F: FnOnce(&mut Classic<'_>)>` are now
     `FnOnce(&mut dyn Canvas)`.)
   - `effects::GpuRenderer` keeps its constructor and methods: `new(device,
     queue, format, size, budget).await`, `render`, `resize`, `invalidate`,
     `is_current`, `damage`, `set_straight_alpha`, `set_texture`,
     `remove_texture`.
   - A texture handed to `set_texture` needs `TextureUsages::TEXTURE_BINDING`
     (it was `COPY_SRC`); it is sampled in place instead of copied into
     Vello's atlas.
   - `effects::Error::Busy` is removed. It was only returned by the shared
     compute renderer; delete the match arm.
   - `host::target_size` no longer clamps to 65,535 (that was Vello's
     `Scene` limit); the device's texture limit is applied where the size is
     used, as before.
   - `EffectStats` counts differently: an unchanged list is 0 renders; an
     in-place change redraws one box (the hull of all changes); inserting or
     removing paint redraws the whole frame; `encoded_scenes` counts quad
     frames, morphs included.
   - New: `GpuRenderer::fallback() -> &Fallback { frames, last, active }` (frames
     the pass refused, by entry and reason, drawn on `vello_cpu` instead:
     vector paints, colour/bitmap glyphs, rotated or sheared text, a path
     clip inside a path clip, over-budget frames); `GpuRenderer::quad_frame()`;
     `effects::Fallback`; `effects::PRESENT_SHADER`; the `mui_vello::quad`
     module (`QuadRenderer`, `QuadRenderer::with_cache`, `INSTANCE_SLOTS`,
     `classify`, `Reason`, `Frame`, ...). `QuadRenderer::render_scene`
     returns `Result<EffectStats, _>`.
4. Behaviour, no code:
   - The first GPU open after upgrading rebuilds the pipeline cache (file
     `*-wgpu30-quad1-mui*.bin`; it was `*-wgpu30-vello0.11-checked2-mui*.bin`).
   - Adapters without compute shaders or storage buffers (GLES 3 / WebGL2
     class) are accepted; the renderer asks only for WebGL2-class limits at
     the adapter's own texture size. WARP still gets the CPU, with the message
     "WARP shader compilation can crash; use CPU rendering" (it was "WARP
     compute shader compilation can crash; ..."): update a test that matches
     the text. "Vello requires compute shaders; downlevel GL is unsupported"
     no longer occurs.
   - A frame the quad pass refuses is drawn by `vello_cpu` and uploaded, and
     is retried on the GPU as soon as the paint list or transform changes.
5. Tools and examples (only if you used them): `mui-vello` example
   `quad_bench` needs `--features gpu-quad` and has no Vello column;
   `tools/renderer-bench` has no `classic` engine (`vello-gpu`, `vello-cpu`,
   `skia-cpu`, `gpui` remain); `mui-cut --renderer classic`, `?renderer=classic`
   and the web `classic` engine still work and name `vello_gpu`.
   `mui_cut::Engine::Classic` is removed (`Engine::Sparse` is the only
   variant).
   `mui-cut`'s plugin builder ignores `vello` and `vello_encoding` in a
   plugin's lock (a plugin pinned to an older MUI), since nothing needs them
   once `mui-vello` is patched to this tree.

Checked by: the diff of `crates/*` since `0c865588` outside `mui-vello` is
the `Cargo.toml` feature lines above and nothing else; the `mui-vello` public
diff is the items above (`git diff 0c865588 HEAD -- crates/mui-vello/src`,
`pub` lines). A grep of the in-tree plugins for `Classic`,
`GpuRenderer`, `present_with_overlay`, `render_with_overlay`, `set_texture` and
`mui::vello::vello`, and for who names `gpu-effects` (one plugin, and moose's
`crates/moose-mui`), was done by an earlier worker on this branch and not
repeated here; Polaar was not found on that machine and is unchecked.

Breaking (by hand):

- mui-baseview and mui-truce run on
  [moose-baseview](https://github.com/Matari-Audio/moose) (upstream baseview
  0.3.4 plus MOOSE patches), no longer baseview-truce 0.1.1. Reach it through
  `mui_truce::window::baseview` (or `mui_baseview::baseview`); a plugin that
  also depends on baseview directly must depend on moose-baseview, or it
  links two.
  - `mui_baseview::open(parent: &impl HasRawWindowHandle, .., scale:
    WindowScalePolicy, ..) -> WindowHandle` -> `open(parent: &impl
    HasWindowHandle, .., scale: Option<f64>, ..) -> Option<baseview::Window>`.
    `parent` is raw-window-handle 0.6; `scale` is the scale override (`None`
    follows the OS); `None` back means no window, with the reason in
    `View::log`. Dropping the `Window` closes it; `Window::close(self)` too.
  - `HostScale::policy() -> WindowScalePolicy` -> `-> Option<f64>`.
  - `ParentWindow` implements raw-window-handle 0.6 `HasWindowHandle`
    (was 0.5 `HasRawWindowHandle`); a null parent is `Unavailable`.
  - `Handler` (headless tests): a resize is `Handler::resized(WindowSize)`,
    not `on_event_inner(&Event::Window(WindowEvent::Resized(info)))`, and
    pointer positions are `baseview::dpi::PhysicalPosition` pixels, not
    logical `Point`s. Keys are keyboard-types 0.8:
    `Key::Escape` -> `Key::Named(NamedKey::Escape)`.
- Feature `keyboard-capture` on mui-baseview and mui-truce: removed; drop it
  from your `features`. Capture is always on: on Windows the window takes the
  keyboard from the host while a text field is focused and hands every other
  key to the host (DAW shortcuts keep working). On macOS and Linux an
  ignored key already goes to the host.

- `mui_text::TextRun::path: Path` -> `Arc<Path>`, so a cached run draws
  without copying its outlines. Reads deref as before; a caller that moved
  the path out takes `Arc::unwrap_or_clone(run.path)`.

Additions:

- `Ui::text_runs() -> TextRuns`: the `Ui::text_run` cache as a cloneable,
  thread-safe handle for canvas closures and helpers without a `&mut Ui`.
  `TextRuns::get(font, text, size, axes)` keys on and shapes with the
  variation axes. A run's baseline is `y = 0`: place its top-left at `(x, y)`
  with `.at(pt(x, y + run.ascent))`.

Behaviour:

- `Ui::text_run` outlines at 0.1 px tolerance (was 0.05) and keeps up to
  2048 runs (was 512).

Overlays and tabs take a look (`mui::widgets`):

- `Overlay` (new, in the prelude) is `popover`, `menu`, `select` and
  `dialog` with every look a slot: `.frame(&f)` replaces the card (fill,
  radius, padding and shadow; nothing is merged under it), `.row(&f)` dresses
  a `MenuRow { label, index, selected }`, `.head(&f)` a `SelectHead { label,
  open }`, `.scrim(&f)` the dialog's window layer. Placement is `.area`,
  `.fallback`, `.gap`, `.match_width`, `.min_w` (was a fixed 140) and
  `.row_h` (was a fixed 32); `.persistent()` turns off the outside-press and
  Escape close; `.weld()` opens a select's list over its own field, headed by
  the open head. A face is only the look: MUI adds the id, focus, role, name
  and cursor and reads the activation. The free functions are
  `Overlay::default()` and keep their signatures.
- `Overlay::select(ui, id, options, &mut selected, &mut open)` takes the
  open state from the caller (one slot per panel, say);
  `select(ui, id, options, &mut selected)` still keeps it in the `Ui`.
- `select`'s head is one pressable field (label and drop-down arrow) where
  it was a `button` `Control` beside an arrow icon: a test that reads the
  button's look, or `.variant`/`.size` on it, must restyle through
  `.head(&f)`. Its id is still `id`; the list is still `"{id}/list"`, rows
  `"{id}/list/{i}"`.
- `Tabs` (new) is `tabs` with `.vertical()` (Up/Down step it, the strip is a
  column) and `.face(&f)` dressing a `Tab { label, index, selected }`, so an
  icon sidebar is a tab strip. `.items(..)` returns the tabs loose
  (`Response<bool, Vec<El>>`) for a sidebar that puts other rows between
  them; `.show(..)` strings them. `tabs(ui, id, labels, &mut i)` is
  `Tabs::default().show(..)`, unchanged.

`mui-headless` is a library as well as a CLI:

- `Session::new(scene, Options { size, scale, theme, fps })` is public
  (`Options::default()`: the scene's size, scale 1, 60 fps); it was
  `pub(crate)` with positional arguments. New on `Session`: `send(Event)`,
  `play(&Script)` (time 0 at the next step), `click(id)`, `settle(max)`,
  `pending()`, `rgba()` (a `Raster` kept by the session) and `ui_mut()`.
  `Event` and `Target` (were the private `Act` and `Pos`) are exported.
- `Scene::with_input(name, size, ui, |ui, input| ..)`: the builder sees each
  frame's `Input` before `Ui::frame`, for an editor that reads raw input.
- `Raster` keeps its `mui::vello::Cache` across `draw` calls (it built a
  fresh one per frame, re-interning every font).

`mui_baseview::native` carries every event translation `mui-baseview` uses,
public (they were private in `lib.rs`): `key_event(&KeyboardEvent) ->
KeyEvent`, `mouse_button(MouseButton) -> Option<Button>`, `mods(Modifiers) ->
Mods`, `native_cursor(Cursor) -> MouseCursor` and `native_ime(config, scale)
-> baseview::ImeConfiguration`, beside the existing `ime_event` and
`ime_configuration`. A host of your own deletes its copies.

GPU quad pass refuses less (`mui-vello`, feature `gpu-quad`):

- Text under a rotation, shear or non-uniform scale, glyphs over
  `MAX_GLYPH_PX`, and glyphs the atlas has no room for are drawn from their
  outlines as path records instead of the whole frame falling back to the
  CPU. `quad::Reason::{TextTransform, TextSize, GlyphBudget}` are removed
  (`Reason::ALL` has 11 entries); delete those match arms.
- A rounded or rotated clip nested in another takes the path-clip slot as its
  outline, so two such clips nest on the GPU; `Reason::ClipDepth` now means a
  third, or a path clip inside a path clip.
- `EffectStats::cpu_fallback: Option<Reason>` (new field) is set on a frame
  the pass refused and `vello_cpu` drew, so a host sees each whole-window CPU
  frame in the stats `Host::present` returns. A struct literal of
  `EffectStats` needs the field (or `..Default::default()`).
- `GpuRenderer::present_again()` (new): the next render presents the
  retained frame to a new view without redrawing. Use it instead of
  `render_with_overlay(.., |_| {})` / `Host::present_with_overlay(.., |_|
  {})` as a forced present, which redraws the frame on the CPU. (`Host`
  gains the matching one-line forwarder on the window-host branch.)

## v0.4.1

Breaking (by hand; `mui-migrate` has no rule for it):

- `mui::host::View::zoom(&self) -> f64` -> `zoom(&self, window: Size) -> f64`.
  `window` is the logical window size before zoom, so a view can fit its
  design to the window (`window.width / 800.0`). A fixed zoom ignores it:
  `fn zoom(&self, _: Size) -> f64 { self.zoom }`.
- `mui_scene::State` gains `FocusVisible`: an exhaustive `match` on `State`
  needs an arm for it.

Additions (nothing to migrate):

- `Ui::wheel(id) -> Option<Vec2>`: the wheel over `id`, claimed, so an
  enclosing scroll node does not also scroll.
- `Ui::focus_visible(id)` and `State::FocusVisible`: focused by Tab or
  `Ui::focus`, not by a click. Put focus rings on `FocusVisible` so a click
  does not light one. (The text field keeps `State::Focus`: like CSS
  `:focus-visible`, a text input shows its focus however it got it.)
- `Ui::edits() -> &[(String, Edit)]`: every gesture edge the last frame
  delivered.
- `mui_text::Weight::{THIN, EXTRA_LIGHT, LIGHT}`.
- `mui_baseview::baseview`: the baseview it runs on, re-exported.
- `Ui::focus_is_text()`: the focus is on a text field (keys are typing).
- Feature `keyboard-capture` on mui-baseview (passed through by mui-truce):
  on Windows, takes the keyboard from the host window while a text field is
  focused, so typing does not hit the DAW's shortcuts. Needs a
  baseview-truce with `Window::set_keyboard_capture`; stock builds lack it.
  (Removed after 0.4.1: always on, see Unreleased.)
- `CanvasCache<K>` is `Send` for a `Send` key.
- `Ui::text_run(font, text, size) -> Option<Arc<TextRun>>`: a canvas label
  shaped once and kept by the `Ui` (bounded), instead of reshaped per frame.

Behaviour:

- A wrapping row with no width of its own breaks against the room and is as
  wide as its longest line (it used to measure as one line), and one inside a
  flex share arranges the lines it measured.
- The tooltip is an opaque box; it used to paint no background.
- `cut`/`keep` on a node with a custom `.outline(..)` carves that outline (it
  used to be an error).
- A drop shadow on a custom `.outline(..)` is that path blurred (it used to be
  an error). The CPU canvas and `GpuRenderer` blur it; the classic GPU canvas (gone)
  without effects leaves it out, as it does backdrop blur. An inset shadow
  on a custom outline is still an error.
- A grid with `min_col` floors at one column, not all of them, since it can
  drop to one: a flex share holding one no longer refuses to shrink past the
  full grid width.

## v0.4 (DSL v2)

The vocabulary in [`docs/DSL-V2.md`](docs/DSL-V2.md) is applied by a tool.
Run it over your code before fixing anything by hand:

```sh
cargo run --release --manifest-path tools/mui-migrate/Cargo.toml -- --dry-run --diff path/to/crate
cargo run --release --manifest-path tools/mui-migrate/Cargo.toml -- path/to/crate
```

It rewrites `.rs` files, the Rust code blocks in doc comments and `.md`
files (`--no-docs` skips them) and moved crates in `Cargo.toml`. The widget
rules are on too (`--no-widgets` skips them). A code block holding a
`// old` or `// before` line is a before/after example and is left alone. A
second run changes nothing. See
[`tools/mui-migrate/README.md`](tools/mui-migrate/README.md).

It prints `file:line: note` where a human has to look:

- `.gpu_weld(w)` / `.reference_weld(w)` become `.weld(w)`; choose the backend
  with `SceneSpec::weld_backend(WeldBackend::..)` (or `Ui::gpu_welding`).
- `resolve_scene_cached` / `_animated` / `_retained`: keep one `Resolver`
  across frames and call `resolve` (it keeps the last scene and reuses memos
  from it) or `resolve_after(&spec, glide, prev)` if you keep scenes yourself.
- `Bridge::bind(`: the closure now receives the id; pass it to the widget.
  It may return the widget's whole `Response` (anything `IntoEl`).
- Bare `Role` variants (`Surface`, `Primary`, ...) are qualified only in files
  whose one glob import is mui's; with another glob (`truce::prelude::*`) write
  `Role::X` yourself. `Role::*` left the prelude.
- A local item named like a new name (`block`, `col`, `stack`, `resolve`, ...)
  collides with the prelude; rename yours.

Changes the tool cannot express:

- `TextCache` -> `Resolver`. `recycle` and `layout_stats` are on the
  `Resolver`, `len` is `text_runs()` (`is_empty` is gone); its weld cache is
  `Resolver::welds`. `Resolver::resolve` returns `&ResolvedScene` and keeps
  it for the next call's memo reuse: `.clone()` one you hold across calls.
- `Element`'s switches are one `flags: u16`: `e.focusable`, `e.disabled`,
  `e.captures_wheel`, `e.tracks_pointer`, `e.baseline`, `e.segmented`,
  `e.weld_excluded`, `e.scroll_bar_off` -> `e.has(Element::FOCUSABLE)` (and
  `DISABLED`, `CAPTURES_WHEEL`, `TRACKS_POINTER`, `BASELINE`, `SEGMENTED`,
  `WELD_EXCLUDED`, `SCROLL_BAR_OFF`); writes are `e.set(Element::X, on)`.
  The builders (`.focusable()`, `.disabled(..)`, ...) are unchanged.
- The scrollbar heat left the tree: `SceneSpec::scroll_bars:
  Option<FxHashMap<Id, f64>>`, by scroll node key (0 rest, 1 hot). `None`
  paints no bars. `mui::Ui` fills it; a host resolving by hand sets it.
- The material verbs moved to the `mui-material` crate as the `Material`
  extension trait: `.union`, `.shell`, `.border_ramp`, `.surface_layout`,
  `.inset_surface`, `.inset_surface_of`, `.join_border`, `.segmented`,
  `.cut`, `.keep`. The `mui` prelude re-exports it; with `mui-scene` alone,
  add `mui-material` and `use mui_material::Material` (or its prelude).
- The `glide` callback of `Resolver::resolve_after` takes `&Id`, not
  `&str`: `|key: &Id, e, f| ..` (an `Id` derefs to `&str`).
- `Stroke::fill` is `Option<Fill>`; `None` is unset and `Style::over` merges
  a stroke's paint and width per field.
- `Style` fields that may be unset are `Option<T>` (`fill`, `stroke`, `radius`,
  `corners`, `shadow`, `shells`, `union`, `mask`, `backdrop_blur`, ...).
  Build styles with the builder
  (`Style::default().fill(Role::Raised)`); readers write `s.fill.as_ref()`
  or `unwrap_or_default()`. Watch `s.fill.is_none()`: it now asks whether the
  field is unset, not whether it is `Fill::None`.
- `material_symbols::codepoint("home")` -> `mui_symbols::sym::HOME` (crate
  `mui-symbols`; `codepoint(name)` stays there for runtime names).
- A dangling cross-reference is `SceneError::MissingId { what, id }`.

Widgets (`mui`), rewritten by the tool unless `--no-widgets`:

- Every widget returns `Response<C = bool, E = El> { el, changed }`:
  `f(..).0` -> `.el`, `.1` -> `.changed`, `let (a, _) = f(..)` ->
  `let a = f(..).el`, `let (a, b) = f(..)` -> `let Response { el: a,
  changed: b } = f(..)`, `.0.el()` / `.0.into()` -> `.el.into_el()`. A
  control's (`button`, `toggle`, `slider`, `knob`, `drag_value`) `el` is the
  `Control`, still taking `.size`/`.variant`/`.role`; a `Response` drops into
  `row![..]` whole. `text_edit`'s `changed` is the `TextEdit`, `curve`'s and
  `bins`' the `Option<CurveEdit/BinEdit>`.
- `Ui::state(id)` returns `Interaction { hover, press }`; `.0`/`.1` and
  `let (h, p) =` are rewritten.
- `toggle(ui, id, on)` -> `toggle(ui, id, label, on)` and
  `drag_value(ui, id, v, range)` -> `drag_value(ui, id, label, v, range)`:
  every control takes a label (its accessible name). The tool inserts `""`;
  give it a real one.
- `color_picker(.., alpha: bool)` -> `color_picker(.., ColorOpts { alpha })`.
- `bins(ui, id, &Bins)` -> `bins(ui, id, &mut Bins)`, `Bins::authored` is
  `&mut [f32]`: the widget applies the paint, reset and selection itself, as
  `curve` does, and `changed` says what it did. An old caller's own apply
  code now applies every edit a second time; delete it.
- `text_input`, `text_edit`, `bins`, `bins_hover`, `curve` and
  `color_picker` take `id: impl Into<Id>` (a `&str` still works).
- `mui::prelude::Response` is the widget `Response`; the pointer's
  per-target report is `mui::input::Response`. A file importing both names
  by hand has to qualify one.
- The `mui` prelude lists its names instead of globbing
  `mui_scene::prelude`; the names are the same, so nothing to rewrite. The
  crates stay reachable by name: `mui::scene`, `mui::layout`, `mui::input`,
  `mui::motion`, `mui::vello`, `mui::geometry`.

DSL v2.1 (the ergonomics pass), rewritten by the tool:

- `.width(l)` / `.height(l)` -> `.w(l)` / `.h(l)`; `.min_width` /
  `.min_height` -> `.min_w` / `.min_h`; `.expand()` -> `.grow(1)`.
- `.pad_xy(x, y)` -> `.pad((x, y))`, `.insets(i)` -> `.pad(i)`.
- `.border(paint, w)` -> `.stroke(paint).stroke_width(w)`, `.no_border()` ->
  `.no_stroke()`; `.apply(f)` -> `.when(true, f)`.
- `.centered_at(0., 0.)` and `.anchor(Align::Center, Align::Center)` ->
  `.centered()`.
- `icon(font, sym)` -> `icon(sym).font(font)`; set `Theme::icon_font` once
  instead where every icon uses the same face (the tool notes the call).
- `Ui::new(Theme::DEFAULT)` -> `Ui::default()`.

Not rewritten, because the old spelling still compiles or the change is a
type: every numeric builder argument takes any number (`.at(8, 4)`,
`block(28, 28)`); a widget's `Response` takes its control's look and the
common element verbs (`knob(..).size(L).tip("Gain")`, no `.el`); every `Ui`
method naming a node takes `impl Into<Id>` (a runtime key is
`Id::runtime("/0")`, a `&str` starting with `/` is refused in debug);
`Theme` is `Clone`, not `Copy`; `MuiEditor::new` and `.resizable` take
`impl Into<Size>`; `sym` is in the prelude.

## mui-text

Fonts are a type now. `Font::new` parses the bytes once and returns an error
for bytes no parser accepts. Build one `Font` per face and clone it (an `Arc`
bump). Every cache keys on its id, so a `Font` built again every frame misses
all of them.

```rust
// old
let bytes: &[u8] = epaint_default_fonts::HACK_REGULAR;
let run = mui_text::text_run(bytes, "Hi", 16., &[], 0.05)?;
// new
let hack = mui_text::Font::new(epaint_default_fonts::HACK_REGULAR)?;
let run = mui_text::text_run(&[hack.clone()], "Hi", 16., &[], 0.05)?;
```

A single face is a fallback chain of one, so each single-face/fallback pair
became one function that takes `&[Font]`, primary face first.

- new: `mui_text::Font` (`Clone`, `Eq`/`Hash` by identity, `AsRef<[u8]>`, `Font::id()`)
- `axes(&[u8]) -> Result<Vec<AxisInfo>, Error>` -> `axes(&Font) -> Vec<AxisInfo>`
- `normalized_coords(&[u8], size, axes)` -> `normalized_coords(&Font, size, axes)`
- `glyph_path(&[u8], ch, size, axes, tol)` -> `glyph_path(&Font, ch, size, axes, tol)`
- `text_run(&[u8], ..)`, `fallback_text_run(&[&[u8]], ..)` -> `text_run(&[Font], text, size, axes, tol)`
- `caret_x(&[u8], ..)`, `fallback_caret_x(&[&[u8]], ..)` -> `caret_x(&[Font], text, size, axes, byte)`
- `hit_index(&[u8], ..)`, `fallback_hit_index(&[&[u8]], ..)` -> `hit_index(&[Font], text, size, axes, x)`
- `break_lines(&[u8], text, size, max)`, `break_lines_with_axes(&[u8], ..)`,
  `fallback_break_lines(&[&[u8]], ..)` -> `break_lines(&[Font], text, size, axes, max_width)`.
  Pass `&[]` for `axes` where you called the old four-argument `break_lines`.
- `TextRun { glyphs: Vec<(u32, f64)>, glyph_offsets: Vec<(f64, f64)>, .. }` ->
  `TextRun { glyphs: Vec<Glyph>, .. }`; `glyph_offsets` is gone, the offset is in `Glyph::x`/`y`
- `FallbackTextRun` -> removed, use `TextRun`
- `FallbackGlyph { font, glyph, x, y, advance, cluster }` -> `Glyph { font, id, x, y }`
- new: `shape_run(&[Font], text, size, axes)`, a `TextRun` with the glyphs and
  metrics and an empty `path`. Use it wherever the outlines go unread.

```rust
// old
for &(id, x) in &run.glyphs { /* .. */ }
// new
for g in &run.glyphs { let (id, x, y, face) = (g.id, g.x, g.y, g.font); }
```

Behaviour: `hit_index` left of a right-to-left run now returns the run's end,
and `break_lines` no longer splits a grapheme cluster that is wider than the
line at its combining mark.

## mui-layout

- `resolve_cached_with(.., key: impl FnMut(&P) -> Vec<u8>, ..)` ->
  `key: impl FnMut(&P, &mut Vec<u8>)`. Append the key to the buffer; the cache
  reuses it across frames.

  ```rust
  // old
  |p| p.text.as_bytes().to_vec()
  // new
  |p, out| out.extend_from_slice(p.text.as_bytes())
  ```
- `LayoutState` (`revision`/`current`/`commit`) -> removed. Use `resolve_with`
  or `resolve_cached_with`.
- new: `resolve_boxed_with(root, size, padding, ..)`: the root laid out as
  exactly `size` with `padding`, instead of `resolve_with` on a clone of the
  tree restyled with `.size(..).pad(..)`.

## mui-geometry

- `NestedRadius` -> removed. `RoundedRect::inset` covers the concentric case.
- `inset_arc_radius` -> removed
- `inset_for_stroked_gap` -> removed. It computed `gap + outer / 2 + inner / 2`.
- `offset_path` / `inset_path` / `outset_path` input fill rule: `EvenOdd` ->
  `NonZero`, matching the renderer. Overlapping same-winding subpaths are one
  region now. A hole must be wound opposite to its exterior.

## mui-motion

- `Spring::step`: 240 Hz semi-implicit Euler -> the exact closed-form solution.
  Trajectories change slightly, are the same at any frame rate, and `dt` is no
  longer clamped to 64/240 s. Stiff springs that used to diverge now settle.

## mui-weld

- `AnalyticWeld::set_channels` -> removed
- `AnalyticSource::rotated` -> removed
- `Boundary::width` -> removed
- `analytic::PARAM_BYTES` 352 -> 336. The uniform `crop` lane at offset 48 is
  gone and the sources start at offset 48. A shader or buffer layout written
  against the old ABI must drop that `vec4`; the weld always covers its whole
  texture.

## mui-style

- `Style.weld: bool` -> `Style.union: bool`
- `Style::over(&self, &Style) -> Style` -> `Style::over(self, Style) -> Style`.
  Both styles are moved, so the merge copies nothing; clone one to keep it.

```rust
// old
let s = mine.over(&card);
// new
let s = mine.over(card); // or mine.clone().over(card.clone())
```

## mui-scene

- new: `mui_scene::Font` (the `mui_text::Font` re-export), also in `mui_scene::prelude` and `mui::prelude`
- `SceneSpec.font: Option<Arc<[u8]>>` -> `Option<Font>`
- `SceneSpec.fallback_fonts: Vec<Arc<[u8]>>` -> `Vec<Font>`
- `SceneSpec::font(impl Into<Arc<[u8]>>)` / `fallback_font(..)` -> `font(Font)` / `fallback_font(Font)`
- `Element.font: Option<Arc<[u8]>>` -> `Option<Font>`. Element equality now
  compares the font's id, not its bytes.
- `icon(impl Into<Arc<[u8]>>, char)` -> `icon(Font, char)`
- `Styled::font(impl Into<Arc<[u8]>>)` -> `Styled::font(Font)`
- `Text.font: Arc<[u8]>` -> removed. It was always `fonts[0]`; read that.
- `Text.coords: Arc<[i16]>` -> removed. It was always `font_coords[0]`; read
  `font_coords[glyph.font]`.
- `Painted.path: Path` -> `Arc<Path>`, `ResolvedSurface.path: Path` ->
  `Arc<Path>`: a node's fill, clip, mask and surface share one outline.
  Reading through `&p.path` is unchanged; build one with `path.into()`.
- `ResolvedSurface.clip_path: Option<Arc<[Path]>>` -> `Option<Arc<[Arc<Path>]>>`,
  `clip_paths() -> Option<&[Path]>` -> `Option<&[Arc<Path>]>`.
- `ResolvedSurface.hits: Vec<(Arc<str>, Path)>` -> `Vec<(Arc<str>, Arc<Path>)>`.
- `Outline(Arc<dyn Fn(Size) -> Path>)` -> `Outline(Arc<dyn Fn(Size) -> Path + Send + Sync>)`,
  and `.outline(..)` takes a `Send + Sync` closure: the weld cache holds it by
  identity. Capture `Arc`s, not `Rc`s.
- `Text.fonts: Arc<[Arc<[u8]>]>` -> `Arc<[Font]>`, never empty. `Text`
  equality compares font ids instead of `Arc` pointers.
- `SceneSpec.tolerance` -> removed. The scene lays text out as glyphs and
  never outlined it for drawing, so the tolerance changed nothing but the
  cache. `mui_text::text_run` still takes one for its path.

```rust
// old
let spec = SceneSpec::new(root).font(epaint_default_fonts::HACK_REGULAR.to_vec());
// new
let spec = SceneSpec::new(root).font(Font::new(epaint_default_fonts::HACK_REGULAR)?);
```

- `SceneState` (`revision`/`current`/`commit`) -> removed. Keep the
  `ResolvedScene` that `resolve_scene*` returns yourself.
- `SceneError::RevisionExhausted` -> removed
- new: `SceneError::InvalidScale`. `SceneSpec::device_scale` of
  `Some(0 | negative | NaN | inf)` used to resolve into NaN geometry; every
  `resolve_scene*` now returns `Err(SceneError::InvalidScale)`. A `match` over
  `SceneError` needs the new arm.
- `material_symbols::names()` -> removed, no replacement. `codepoint(name)`
  stays. Regenerate the table with `python3 tools/material_symbols_table.py`.
- `TextCache` retention: runs, line breaks, coordinates, outlines, borders and
  regions were capped at 4096/256 entries and flushed when full -> the cache
  now keeps exactly what the last successful resolve used and drops the rest
  at its end. `TextCache::len()` counts the shaped runs currently held.
- `Paints::weld(fill)` -> `Paints::union(fill)`. The vector outline union
  (children share one filleted outline, each keeps its own paint) is now named
  beside `.cut` and `.keep`. The material weld (`weld!`, `.weld_with`,
  `.gpu_weld`) keeps the word and is unchanged.

  ```rust
  // old
  row![tab, body].weld(Surface)
  // new
  row![tab, body].union(Surface)
  ```
- `Paints::preset(self, &Style)` / `Paints::base(self, &Style)` ->
  `preset(self, Style)` / `base(self, Style)`: `.preset(&card())` ->
  `.preset(card())`. Pass `s.clone()` to keep a named style.
- `SceneError::UnsupportedWeld` messages: `"custom/legacy-weld outline on the
  group; ..."` -> `"custom or union outline on the group; ..."`, and
  `"inside requires vector welding"` -> `"inside takes a vector union, not a
  material weld"`. Only code matching on the text is affected.
- `ResolvedScene`'s `PartialEq` and `Debug` no longer include the spec's
  `font`/`fallback_fonts`: the scene stopped copying them, since each `Text`
  carries its own faces.
- Behaviour: a union or carve whose outline includes a custom `.outline(..)`
  is cached by the closure's identity: the same `Outline` hits, a new
  closure reshapes. A tree that rebuilds its closures every frame reshapes
  every frame. A text node's
  fill is never pushed as a `Layer::Fill` entry (it used to be pushed and
  removed again); its colour is the ink, as before.
- `Kind::TextInput { value }` -> `Kind::TextInput { value, selection,
  carets }`: the selection's anchor and caret in characters, and a caret x
  per character boundary in the field's space (`Vec::new()` when unknown).
  `text_input` fills both.
- `ResolvedSurface` gains `pointer_states: bool`: the node declared a Hover
  or Press look.

```rust
// old
let mut state = SceneState::default();
state.commit(&spec)?;
let scene = state.current().unwrap();
// new
let scene = resolve_scene(&spec)?;
```


## mui-input

- `Input` gained `trail: Vec<Point>` (folded drag samples); a struct
  literal needs `..Default::default()`.
- `Hit::push_clipped_paths(.., clips: Option<&[Path]>)` and
  `push_tagged_paths(..)` -> `clips: Option<&[Arc<Path>]>`, which is what
  `ResolvedSurface::clip_paths()` returns.

## mui-vello

- `host::Frame` gained `Current` (the screen already shows the scene;
  nothing acquired): match it like `Presented`. New on `Host`: `device()`,
  `generation()`, `set_texture(key, texture)`.
The process-global font and image caches are gone. Each renderer owns a
`mui_vello::Cache` and hands it to every `Cpu` canvas it builds
(`GpuRenderer` owns its own; the `Gpu` canvas is gone with `vello_hybrid`,
below). Keep
one per renderer and drop it with that renderer: an atlas id means nothing to
another one. It holds only a `Weak` to each image buffer and frees a dropped
buffer's pixmap or atlas slot on the next image lookup.

```rust
// old
let mut ids = ImageIds::default();
let mut canvas = Gpu { scene: &mut scene, resources: &mut res,
    atlas: Some(Atlas { renderer: &mut r, device: &d, queue: &q, ids: &mut ids }) };
mui_vello::paint_cached(&mut canvas, &resolved, xf, &mut paths)?;
// new: the quad pass, retained, owning its cache
let mut gpu = GpuRenderer::new(&d, &q, format, size, Budget::default()).await?;
gpu.render(&resolved, xf, &target_view)?;
// new: the CPU canvas takes a cache kept beside it
let mut cache = mui_vello::Cache::default();
mui_vello::paint(&mut Cpu { ctx: &mut ctx, resources: &mut res, cache: &mut cache }, &resolved, xf)?;
```

- `Gpu { scene, resources, atlas }` -> removed; `effects::GpuRenderer`
- `Cpu { ctx, resources }` -> `Cpu { ctx, resources, cache: &mut Cache }`
- `Atlas { renderer, device, queue, ids }` -> removed with `Gpu`
- `ImageIds` -> `Cache`
- `PathCache` -> removed
- `paint_cached(canvas, scene, xf, &mut PathCache)` -> `paint(canvas, scene, xf)`
- `Canvas::image` default method (built a pixmap from the global cache) ->
  returns `None`, so a custom canvas paints the solid stand-in unless it
  overrides it
- `brush(&Paint::Image { .. }, bounds)` (a pixmap brush, or transparent when
  too big) -> `PaintType::Solid` of `Paint::solid()`, the stand-in colour.
  Images go through `Canvas::image`.
- A full image atlas: `upload_image` used to panic -> the image paints as its
  solid stand-in
- `effects::OutputEncoding` -> removed; the one entry point is `fs_hybrid`
- `weld.wgsl` `fs_classic` entry point -> removed
- `WeldTextures::new(device, queue, encoding, budget)` -> `WeldTextures::new(device, queue, budget)`
- `WeldTextures::encoding` / `texture` / `sample_size` / `clear` -> removed
- `WeldTextures::resident_bytes` -> private
- `WeldTextures::begin` with a key listed twice: `Err(Unsupported)` -> accepted
  (the second `encode` for that key in the frame is still refused)
- Behaviour: `WeldTextures::begin` used to free every texture not wanted this
  frame -> textures stay resident while the budget allows and the least
  recently wanted are evicted when it does not. `TiledEffects` renders only
  the welds inside the viewport plus its guard.
- `HybridEffects::enable_profiling` / `collect_timings` / `timing_losses` /
  `resident_effect_bytes` / `release_effects` -> removed. `GpuTimer` remains
  and can be driven directly.
- `EffectStats` gains `renders: u64`, the Vello passes recorded. A struct
  literal needs it; `..Default::default()` covers it.
- `Canvas` gains `begin_frame()`, a default no-op; `paint` calls it first.
  `Gpu` and `Cpu` age the cache's fonts there: a font unused for 64 frames
  lets go of its `FontData`.
- new: `Cache::for_atlas(AtlasConfig)` for a renderer built with
  `Renderer::new_with`; `Cache::default()` matches `Renderer::new`.
- new: `WeldTextures::forget_absent(in_scene)` and `ABSENT_FRAMES` (3). Both
  retained renderers call it: a weld gone from the scene for more than 3
  frames frees its texture instead of waiting for budget pressure.
- Behaviour: `HybridEffects` and `TiledEffects` record no Vello pass (and
  `HybridEffects` submits nothing) when the frame is unchanged and the
  target is the view presented last. A host that draws into that target
  itself between frames calls `invalidate()`.
- Behaviour: the `Cpu` canvas draws a font's glyphs from `vello_cpu`'s glyph
  atlas from the font's second frame on, instead of per-glyph paths;
  antialiasing can differ slightly from the first frame's. `Gpu` is
  unchanged.
- Behaviour: a `Gpu` image that does not fit the atlas is sized, not
  premultiplied, before it is refused, so a refused photo costs nothing per
  frame.
- `DamageTracker::plan(&self, scene, xf, tiles) -> DamagePlan` ->
  `plan(&self, scene, xf, tiles, out: &mut DamagePlan)`. Reuse one plan.
- `DamagePlan { dirty, total_tiles, full, dirty_pixels }` struct literal ->
  `DamagePlan::default()`; it has a private field now

```rust
// old
let plan = tracker.plan(&scene, xf, &tiles);
// new
let mut plan = DamagePlan::default(); // kept across frames
tracker.plan(&scene, xf, &tiles, &mut plan);
```

- `vello_hybrid` is gone; the GPU path was classic Vello on wgpu 30 (was 29),
  and is now the quad pass (see the top of this file).
  `effects::HybridEffects` and `effects::TiledEffects` -> `effects::GpuRenderer`
  (same `new(device, queue, format, size, budget).await`, `render`, `resize`,
  `invalidate`). The target view needs `RENDER_ATTACHMENT`. `DamageTracker`,
  `DamagePlan` and the `Gpu`/`Atlas` canvas go with the tiles and the atlas;
  draw through the `Canvas` trait or `GpuRenderer` instead.

## mui-access

- `tree_update(scene, focus)` -> `tree_update(scene, focus, scale: f64)`.
  `scale` is the window's device pixels per scene unit; it becomes the window
  node's transform and bounds stay in scene units. Pass `1.0` for the old
  output.
- An unlabeled node's label: its surface id -> none, except a control
  (button, slider, toggle, text input) with neither label nor text, which
  keeps its id as its name
- Sliders gain `Action::Increment`/`Decrement` and a `numeric_value_step`
  (a hundredth of the range)
- A text input gains a `TextRun` child (`run_id(key)`) with its characters'
  lengths, positions and widths, a `text_selection`, and
  `Action::SetTextSelection`; route that to
  `SemanticAction::set_selection(key, anchor.character_index,
  focus.character_index)`

```rust
// old
adapter.update_if_active(|| tree_update(&scene, focus));
// new
adapter.update_if_active(|| tree_update(&scene, focus, ui.scale.unwrap_or(1.0)));
```

## mui-widgets

The crate is folded into `mui` as `mui::widgets` (`mui::presets` and the
`mui::prelude` names still resolve). The `Host` trait is gone: every widget
takes `&mut Ui` and returns its element with what happened.

```rust
// old
let knob_el = knob(&mut ui, "cut", "Cutoff", &mut v, 0.0..=1.0);
let field = text_input(&mut ui, "name", &mut name);
// new
let (knob_el, changed) = knob(&mut ui, "cut", "Cutoff", &mut v, 0.0..=1.0);
let (field, edited) = text_input(&mut ui, "name", &mut name);
```

- crate `mui-widgets` / `mui_widgets::*` -> `mui::widgets::*`
- `mui_widgets::Host` / `mui::prelude::Host` -> removed; widgets take
  `&mut mui::Ui`. `Ui::sel`, `set_sel`, `pasted`, `hit`, `caret_x` and
  `blink` were reachable only through `Host` and are now crate-private.
  `Host::double_click(ui, id)` -> `ui.double_click(id)`, still public: an app
  control that resets on a double click needs it.
- `button(&impl Host, id, label) -> (Control, bool)` ->
  `button(&mut Ui, id, label) -> (Control, bool /* clicked */)`
- `toggle(&impl Host, id, &mut bool) -> Control` ->
  `toggle(&mut Ui, id, &mut bool) -> (Control, bool /* flipped */)`
- `slider(&mut impl Host, ..) -> Control` / `knob(&mut impl Host, ..) -> Control`
  -> `slider(&mut Ui, ..)` / `knob(&mut Ui, ..) -> (Control, bool /* changed */)`.
  A NaN value handed in is not a change.
- `text_input(&mut impl Host, id, &mut String) -> El` ->
  `text_input(&mut Ui, id, &mut String) -> (El, bool /* changed */)`
- `curve(&impl Host, ..)` / `bins(&impl Host, ..)` -> `curve(&mut Ui, ..)` /
  `bins(&mut Ui, ..)`, same return types
- `bins_hover(&impl Host, ..)` -> `bins_hover(&Ui, ..)`
- `slider`: the id, `Kind::Slider` role, label and focus moved from the thumb
  to the whole lane. The surface named by the slider id is now the track, and
  the thumb is unnamed. A press on the track jumps the value there; drag
  travel is the lane's width, not 160 px.
- Behaviour: a focused `slider` or `knob` steps on the arrow keys (a
  hundredth of the range, a tenth of that with Shift), Page Up/Down (ten
  steps) and Home/End.

## mui

- new: `mui::host`, the window-framework-free host: `View`, `Shared`,
  `Driver` (queue, schedule, zoom, key routing). A window crate translates
  its events into `Driver` calls; mui-baseview is one.
- `Ui.font: Option<Arc<[u8]>>` -> `Option<Font>`
- `Ui.fallback_fonts: Vec<Arc<[u8]>>` -> `Vec<Font>`
- `Ui::font(impl Into<Arc<[u8]>>)` / `fallback_font(..)` -> `font(Font)` /
  `fallback_font(Font)`. Invalid bytes used to be dropped silently; now
  `Font::new` rejects them.
- `mui::core` (deprecated alias) -> `mui::scene`
- `mui::tessellate` -> removed, no replacement
- `mui::egui` and the `egui` cargo feature -> removed, no replacement
- `SemanticAction` gains `SetSelection { id, anchor, caret }`, with
  `SemanticAction::set_selection(id, anchor, caret)`. A `match` over it needs
  the new arm.
- `SemanticAction` gains `Increment { id }` and `Decrement { id }`, with
  `SemanticAction::increment(id)` / `decrement(id)`. A `match` over it needs
  the new arms. Both land as the equivalent `SetValue`.
- Behaviour: `.scroll()`, `.animate()`/`.transition()` and
  `.on(State::Focus | State::Disabled)` now work on a node without an id,
  keyed by its `/0/2` tree path. So does `.on(State::Hover | State::Press)`:
  an unnamed node that declares one is a pointer target by its tree path and
  takes presses exactly where the same node with an id would.
- Behaviour: an unnamed root is no longer a pointer target keyed `""`: it
  does not hover, and its cursor and tip no longer show over the bare
  background. A press that lands on no target still drops the focus, now as
  a rule rather than through the root. A tip needs an id.
- `button`, `toggle`, `slider`, `knob`: `id: &str` -> `id: impl Into<Id>`.
  `&str`, `String`, `&String`, `Id` and `&Id` all pass; a `&mut String`
  needs `&**s`.
- Behaviour: while a tip is shown the root sits under a wrapper, so positional
  keys move under `/0` and back when it goes: ask `ui.scroll("/0/1")`, not
  `ui.scroll("/1")`, while the tip is up. Named keys do not move.
- Behaviour: Enter/Space on a focused button or toggle, and a stepping key on
  a focused slider, are delivered as an `Edit::Begin`/`End` pair.

## mui-playground

- DSL method `weld` -> `union`: `.weld(Surface)` -> `.union(Surface)`, as in
  the Rust API

## mui-truce

The crate was a state-document and gesture contract with no editor. It is
now the editor: the document went back to the plugin, and a `Bridge` replaces the
per-parameter wrapper.

- edition 2024 -> 2021, the workspace edition
- `#![forbid(unsafe_code)]` -> `#![deny(unsafe_code)]`, with two allowed
  blocks (the wgpu surface on the host's window and `Send` for the window
  handle)
- `Document`, `EditorState`, `Module`, `Route`, `Target`, `Error` -> deleted.
  The editor document was one plugin's schema; keep it in the plugin as a
  `#[persist]` field of its own type.
- `Parameter` -> `Bridge`. `Parameter::new(params, id, modulatable, edits)`
  / `new_many(params, ids, edits)` -> `Bridge::new(params)`, one per editor,
  with no channel: the bridge calls the host directly. `value()` / `text()`
  -> `bridge.value(id)` / `bridge.text(id)`. `begin` / `set` / `drag` /
  `end` / `cancel` / `step` / `reset` -> `bridge.bind(ui, widget, id, |ui, v|
  ..)`, which sends the host's begin/set/end from the `Ui` edits of that
  widget. `parse(text)`, `set_enabled` and `modulatable` -> no replacement.
- `Automation::dispatch(context, edit)` / `Automation::close(context)` ->
  `Bridge::bind` / `Bridge::close`. `MuiEditor` calls `close` when the host
  closes the editor.
- `mui_truce::Edit::{Begin(id), Value(id, v), End(id)}` -> gone; `Ui` reports
  `mui::Edit::{Begin, End}` per widget and `bind` supplies the id and value.
- new: `MuiEditor::new(params, ui, size, build).resizable(min).into_editor()`
  is a truce `Editor`, and `mui_truce::window` is the host-agnostic window
  (`View`, `Shared`, `Requests`, `open`).
- `mui_truce::window` -> the `mui-baseview` crate, re-exported under the same
  name, so every `mui_truce::window::..` path still compiles. New there:
  `run(title, size, shared, requests)`, a standalone app window.
- `View::build(&mut self, ui)` -> `build(&mut self, ui, input: &Input)`.
  `View`, `Shared` and `lock` now live in `mui::host` (re-exported from
  `mui_baseview`); new default hooks: `after_frame`, `cancel`, `zoom`,
  `drop_files`, `claims_key`, `log`.
- keys: a focused control keeps only Enter/Tab/arrows/Home/End/PgUp/PgDn/
  Delete/Backspace and a focused field every key; Space and the rest go to
  the host unless `View::claims_key` takes them.
- new: `HostScale` and `ParentWindow` are public in `mui_truce`.

## Removed crates and packages

- `mui-widgets` -> folded into `mui::widgets` (see above)
- `mui-egui` -> deleted, no replacement
- `mui-tessellate` -> deleted, no replacement. Fill a glyph or shape path with
  `mui-vello`, or flatten it with `Path::flatten`.
- `packages/mui-ts` -> deleted. The Rust DSL is the only front end.

## Examples and tools

- `mui-vello` example `gpu_matrix` -> deleted with `vello_hybrid`
- `mui-vello` example `bench` -> `vello_cpu` and `GpuRenderer` rows only;
  needs `--features cpu,gpu-quad` (was `cpu,gpu-effects`). The `vello_hybrid`, tiled and classic
  rows and `bench-classic` are gone
- `mui-vello` example `gpu_contract` -> the tile contract went with
  `TiledEffects`
- `tools/native-gpu/compare.py` -> deleted, with the classic/hybrid pairing in
  `run_matrix.py`
- `tools/native-gpu/run_matrix.py`, `report.py` -> deleted with `gpu_matrix`
- `mui-preview` feature `gpu-effects` -> removed; the gallery always renders
  through `GpuRenderer`
