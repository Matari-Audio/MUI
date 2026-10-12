# Resident fallback fonts (2026-10-12)

Adding an unused emoji face to an Inter-only label changed its measured line
height from 19.363636 to 21.789063 at 16 px, including empty text. The new
`unused_fallback_faces_do_not_change_primary_line_metrics` regression fails
before the fix. It checks empty text, whitespace and Latin labels, and preserves
both glyph face indices and advances. A companion regression checks fallback-only
and mixed Latin/emoji metrics against the faces that actually contributed.

Metrics now come only from contributing faces, with primary metrics for empty
or whitespace runs. The contributing-face check adds no heap mask. Variable
locations, shaping instances and shapers are created lazily per used face;
adding resident fallback faces no longer allocates variation buffers for unused
faces on every uncached measurement. Coverage still searches the full chain,
and public glyph indices keep their original meaning. All 47 mui-text unit
tests pass, including variable axes, fallback clusters, RTL, combining marks,
FeatureVariations and the bounded plan cache. BUFR owns the product-level idle
allocation regression; this change does not relax that threshold.
