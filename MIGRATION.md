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

## mui-scene

- new: `mui_scene::Font` (the `mui_text::Font` re-export), also in `mui_scene::prelude` and `mui::prelude`
- `SceneSpec.font: Option<Arc<[u8]>>` -> `Option<Font>`
- `SceneSpec.fallback_fonts: Vec<Arc<[u8]>>` -> `Vec<Font>`
- `SceneSpec::font(impl Into<Arc<[u8]>>)` / `fallback_font(..)` -> `font(Font)` / `fallback_font(Font)`
- `Element.font: Option<Arc<[u8]>>` -> `Option<Font>`. Element equality now
  compares the font's id, not its bytes.
- `icon(impl Into<Arc<[u8]>>, char)` -> `icon(Font, char)`
- `Styled::font(impl Into<Arc<[u8]>>)` -> `Styled::font(Font)`
- `Text.font: Arc<[u8]>` -> `Font`
- `Text.fonts: Arc<[Arc<[u8]>]>` -> `Arc<[Font]>`. `Text` equality compares
  font ids instead of `Arc` pointers.

```rust
// old
let spec = SceneSpec::new(root).font(epaint_default_fonts::HACK_REGULAR.to_vec());
// new
let spec = SceneSpec::new(root).font(Font::new(epaint_default_fonts::HACK_REGULAR)?);
```

## mui

- `Ui.font: Option<Arc<[u8]>>` -> `Option<Font>`
- `Ui.fallback_fonts: Vec<Arc<[u8]>>` -> `Vec<Font>`
- `Ui::font(impl Into<Arc<[u8]>>)` / `fallback_font(..)` -> `font(Font)` /
  `fallback_font(Font)`. Invalid bytes used to be dropped silently; now
  `Font::new` rejects them.
- `mui::core` (deprecated alias) -> `mui::scene`
- `mui::tessellate` -> removed, no replacement
- `mui::egui` and the `egui` cargo feature -> removed, no replacement

## mui-truce

- edition 2024 -> 2021, the workspace edition. The public API is unchanged.

## Removed crates and packages

- `mui-egui` -> deleted, no replacement
- `mui-tessellate` -> deleted, no replacement. Fill a glyph or shape path with
  `mui-vello`, or flatten it with `Path::flatten`.
- `packages/mui-ts` -> deleted. The Rust DSL is the only front end.
