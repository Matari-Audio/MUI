# Original bitmap font fixture

`non_apple_sbix.ttf` is authored for MUI by `generate_sbix.py`; it contains no
third-party font outlines or artwork. The sole visible glyph is an opaque green
8×8 PNG with sbix offsets `(6, 4)` and the non-Apple PostScript name
`MUIBitmapFixture`. U+E000 selects it at 32 ppem.

The CPU/GPU test checks those authored bearings and confirms an unchanged scene
reuses the retained frame. The fixed font timestamps make regeneration
deterministic. To regenerate with FontTools and Pillow installed:

```sh
python3 crates/mui-vello/tests/fixtures/generate_sbix.py
```

The generated font and generator are covered by the repository's MIT license.
