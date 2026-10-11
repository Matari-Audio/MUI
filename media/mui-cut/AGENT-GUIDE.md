# Authoring with mui-cut from an AI agent

A practical guide for an agent driving `mui-cut mcp`. It comes from
dogfooding the server on three pieces (`examples/graphite-title.cut.json`,
`examples/glass-orbit.cut.json`, `examples/synth-explode.cut.json`) using
only MCP tool calls, logging what slowed it down, and fixing the worst of
it. The README's "Using mui-cut from an AI agent" section has the
reference.

## The loop

1. **`open`** with `create: true` and the piece's shape in one go:
   `{"path": "p.cut.json", "create": true, "size": [1280, 720], "scene": "title", "duration": 7, "mode": "3d", "background": "#2b2d31"}`.
2. **One `batch`** for the first draft: every `add_layer` (keys inline),
   `motion`, `key`, `set` and `patch` in order, applied all or nothing,
   written once, checked once. Later calls see earlier ones, so a layer
   added in call 1 can get a motion in call 2.
3. **Look**: `sheet` with explicit `times` at the beats you care about
   (`"times": [0.5, 1.2, 3.5, 6.1]`, `"width": 1280`), then `still` on the
   frame that looks wrong. Both take well under a second for 2D and 3D at
   720p and say how long they took.
4. **Fix** in another `batch`; repeat 3 and 4. Each edit's reply already
   carries the `check` summary, so a separate `check` call is rarely
   needed.
5. **Finish**: `still` with `samples: 32` for a 3D beauty frame,
   `plugin_play` to hear a plugin piece, `render` for the video.

A whole piece is 3 or 4 calls this way (open, batch, sheet, one more).

## Cheat sheet

| need | call |
| --- | --- |
| a new project | `open` `{path, create: true, size, scene, duration, mode, background}` |
| many edits at once | `batch` `{calls: [{tool, args}, ...]}` (patch, set, key, motion, add_layer, remove_layer, notes_set, notes_add, source_add, layer_parent) |
| an entrance or exit | `motion` `{layer, preset, t, dur}` |
| one field's schema | `schema` `{def: "Animator"}` (the whole schema is ~85 KB) |
| what is in the file | `list` (outline with key times), `get` `{pointer}` |
| numbers at a time | `eval` `{t, layer}` |
| a grid of frames | `sheet` `{times or n, width}` |
| one frame | `still` `{t, width, samples}` |
| one layer's path | `strip` `{layer}` |
| a plugin's parts and patch | `plugin_parts`, `patch_get`, `sources_list` |
| a plugin's sound | `notes_set` / `notes_add`, then `plugin_play` |

### Motion presets

`motion` keys a preset from `t` for `dur` (default 0.6 s) onto the layer's
own values, eased (expo out for entrances, in for exits), and merges it
with the keys already there: an exit after an entrance keeps both.

- Whole layer: `fade_in`, `fade_out`, `rise_in`, `rise_out` (fade while
  moving up by `distance`, default 40), `slide_in`, `slide_out` (from or to
  `dir`: left, right, up, down; `distance` default 160), `pop_in`,
  `pop_out` (scale with a small overshoot).
- Per glyph (text) or copy (duplicator), appended as an animator:
  `typewriter`, `cascade`, `cascade_out`, `pop`; `stagger` sets the
  seconds between glyphs.

## Gotchas

- **Animators are targets, not offsets you key.** An animator's `y`,
  `opacity`, `scale` are what a selected glyph *becomes*; key its `amount`
  from 1 to 0 to animate in. Keying `opacity` 0 on the animator hides the
  text for good (`check` says `never_visible`). The `motion` presets get
  this right; copy them.
- **3D depth:** `z` is away from the viewer. A backdrop goes at positive
  `z`, glass in front at negative `z`. `check`'s pixel lints skip 3D
  scenes, so look at a `sheet` for clipping there.
- **Glass:** `material.transmission` near 1 with `roughness` near 0 is
  clear; roughness 0.2 and up is frosted. A model is tinted by its
  `fill`, so set `"fill": "#ffffff"` for clear glass on a coloured glTF.
  `still` with `samples` gives the beauty frame (soft shadows, depth of
  field) the final render will have. A flat pane does not bend a far
  sky (rightly): give it `bevel` for rims that do. A dark UI turned to
  glass wants `print: 1` (its dark goes clear, its light stays as ink);
  key `transmission` from 0, which crossfades, and per part
  (`parts.<id>.material`) for a sweep. A scene's `sky` puts a sunlit,
  clouded sky behind it all for glass to refract (mui-stage only).
  For glass that warps it hard, press it: `material.texture` takes
  `ribbed` (reeds), `hammered` (dimples) and `ripple`, each a keyable
  `strength` (0.3 subtle, 1 wild) and `scale` (pixels per reed or
  dimple); a high `ior` (1.8 to 2.4) and `dispersion` 2 to 4 fringe the
  reeds in rainbows. Key one pattern's strength down while another's
  goes up to melt one into the other. A scene's `bloom` makes glints and
  an in-frame sun glow. `render.glass` `trace` (or `--glass trace`) traces
  glass through the layers' slabs on any GPU: glass behind glass bends
  right and mirrors show what is off screen, at one ray per pixel.
- **Plugins:** import once with `source_add`, then a plugin layer's
  `source` may be that id (it is written out in full). Parameters are
  `{"id": "filter", "field": "cutoff"}`; `patch_get` reports the same one
  as `filter.cutoff` in group `filter`. Notes drive the UI too, so a
  plugin layer with notes is captured at every frame (241 states for 8 s):
  the first capture of a debug build takes 5 to 60 s, later ones are
  cached.
- **Captures are keyed to the adapter binary's size and mtime.** Two
  checkouts that build the same plugin example into one shared
  `CARGO_TARGET_DIR` relink it in turn and throw away each other's
  captures (a 70 s `patch_get`). Give each checkout its own target dir, or
  point the source at a copy: `{"bin": "synth-bin"}`. `MUI_CUT_CACHE` moves
  the generated adapters of `plugin_add`.
- **Every tool reads the file and every edit writes it.** Edits that do
  not load or have a misspelt field are refused with the reason and a
  "did you mean"; so are unknown tool arguments, layer ids, scene names,
  tools, presets and schema definitions.
- `low_contrast` fires on deliberately subtle background text; it is a
  warning, and the piece is fine if the `sheet` reads.

## Friction log (dogfood, 2026-10-01)

Three pieces, 6 to 8 s each, from an empty project, over stdio JSON-RPC.
Before is the first authoring pass with the old tools (redundant `open`s
from restarting the client left out); after is the same piece with the
fixes. The after pass knew what it wanted, so compare the edit calls:

| piece | calls before | edit calls before | calls after | edit calls after |
| --- | --- | --- | --- | --- |
| kinetic title, graphite | 16 | 8 | 3 | 1 |
| 3D glass, camera orbit | 8 (+2 CLI beauty stills) | 4 | 4 | 1 |
| plugin explode with notes | 18 | 8 | 4 | 1 |

What got in the way, worst first, and what changed:

1. **No way to send several edits at once.** `set` and `key` change one
   property each, so nudging x and y, or adding exits to four layers, was
   one call per key or a `patch` restating whole key lists. Fixed:
   `batch`.
2. **No motion presets.** Every fade, rise and pop was hand-written keys
   and handles; the per-glyph presets existed only in the web editor.
   Fixed: `motion`.
3. **Animator semantics.** Keying an animator's `opacity` to 0 made the
   title invisible for good; the right idiom (key `amount`) took reading
   the source. Fixed by the presets doing it; documented above.
4. **Plugin captures redone after unrelated relinks** (70 s `patch_get`,
   47 s `patch`, 28 s `sheet`), from a shared target dir. Documented; the
   stamp is the binary's, so the fix is a private target dir.
5. **A plugin layer could not use an imported source by id** ("invalid
   type: string, expected struct Source"), so the cargo path was written
   twice. Fixed: ids resolve.
6. **No beauty preview over MCP.** The CLI had `--quality beauty`; glass
   had to be checked outside MCP. Fixed: `still` `samples`.
7. **Misses gave no hint**: a misspelt layer id, scene, tool or argument
   said only "nothing with id `wrod`" or "missing field `t`" (an unknown
   `time` was silently dropped). Fixed: did-you-mean everywhere, unknown
   arguments refused.
8. **`schema` is 85 KB in one reply**, and the field docs sit inside
   `anyOf` wrappers. Fixed: `schema` `def`.
9. **`open` creates a 1920x1080, 3 s scene named main**, so every piece
   began with a patch. Fixed: size, fps, scene, duration, mode and
   background on create.
10. **Noisy numbers**: `sources_list` was 10 KB with
    `125.56818199157716`-style floats. Fixed: rounded to 0.01 in `list`,
    `eval`, `plugin_parts`, `sources_list`.

Not fixed: `check`'s pixel lints skip 3D (clipped 3D text goes
unflagged); no way to mark a `low_contrast` as intended; `plugin_parts`
repeats `rects` next to `frame`.
