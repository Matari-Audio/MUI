# Migrating to the next MUI release

Every entry is `old -> new`. Crates are listed in dependency order.

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

- `Hit::push_clipped_paths(.., clips: Option<&[Path]>)` and
  `push_tagged_paths(..)` -> `clips: Option<&[Arc<Path>]>`, which is what
  `ResolvedSurface::clip_paths()` returns.

## mui-vello

The process-global font and image caches are gone. Each renderer owns a
`mui_vello::Cache` and hands it to every `Gpu`/`Cpu` canvas it builds. Keep
one per renderer and drop it with that renderer: an atlas id means nothing to
another one. It holds only a `Weak` to each image buffer and frees a dropped
buffer's pixmap or atlas slot on the next image lookup.

```rust
// old
let mut ids = ImageIds::default();
let mut canvas = Gpu { scene: &mut scene, resources: &mut res,
    atlas: Some(Atlas { renderer: &mut r, device: &d, queue: &q, ids: &mut ids }) };
mui_vello::paint_cached(&mut canvas, &resolved, xf, &mut paths)?;
// new
let mut cache = mui_vello::Cache::default(); // beside the renderer
let mut canvas = Gpu { scene: &mut scene, resources: &mut res, cache: &mut cache,
    atlas: Some(Atlas { renderer: &mut r, device: &d, queue: &q }) };
mui_vello::paint(&mut canvas, &resolved, xf)?;
```

- `Gpu { scene, resources, atlas }` -> `Gpu { scene, resources, cache: &mut Cache, atlas }`
- `Cpu { ctx, resources }` -> `Cpu { ctx, resources, cache: &mut Cache }`
- `Atlas { renderer, device, queue, ids }` -> `Atlas { renderer, device, queue }`
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
  `&mut mui::Ui`. `Ui::sel`, `set_sel`, `pasted`, `double_click`, `hit`,
  `caret_x` and `blink` were reachable only through `Host` and are now
  crate-private.
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
now the editor: the document went back to Kurv, and a `Bridge` replaces the
per-parameter wrapper.

- edition 2024 -> 2021, the workspace edition
- `#![forbid(unsafe_code)]` -> `#![deny(unsafe_code)]`, with two allowed
  blocks (the wgpu surface on the host's window and `Send` for the window
  handle)
- `Document`, `EditorState`, `Module`, `Route`, `Target`, `Error` -> deleted.
  The editor document was Kurv's schema; keep it in the plugin as a
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

## Removed crates and packages

- `mui-widgets` -> folded into `mui::widgets` (see above)
- `mui-egui` -> deleted, no replacement
- `mui-tessellate` -> deleted, no replacement. Fill a glyph or shape path with
  `mui-vello`, or flatten it with `Path::flatten`.
- `packages/mui-ts` -> deleted. The Rust DSL is the only front end.

## Examples and tools

- `mui-vello` example `gpu_matrix --backend classic|hybrid` -> hybrid only;
  the flag and its `bench-classic` requirement are gone
- `tools/native-gpu/compare.py` -> deleted, with the classic/hybrid pairing in
  `run_matrix.py`
