# MUI glifo vendor

Source: crates.io `glifo` 0.4.0, archive SHA-256
`9f1218dedd5348c1b166ec3b8b3cbca49d2f527bd43b3fa072ea5123c523ed3f`.
Apache-2.0 OR MIT; upstream copyright headers and both licenses are retained.
Only src/, the normalized manifest, README and licenses are imported.

Local change, all in `src/glyph.rs` and marked `MUI patch`: a run drawn
through the atlas paid for three font-table collections (outlines, COLR,
bitmap strikes) and a second outline collection for its hinting-instance
lookup, every frame, even when every glyph was already in the atlas. On a
redrawn editor frame that setup was two thirds of all text time.

- `GlyphRunRenderer::draw_glyphs` builds each collection in a `OnceCell` on
  the first glyph that misses the atlas.
- `prepare_glyph_run` asks `HintCache::find` (font id, index, size, coords,
  across the glyf/CFF/VARC lists; a face has one format) before building the
  outline collection `HintCache::get` needs to make an instance.

Rendering is unchanged: the same keys select the same cached slots and
instances. The dependency versions are unchanged.
