# hybrid vs cpu vs classic — 2026-09-15

One scene, three backends, three cases. The question is narrow: MUI renders
through `vello_hybrid`, and classic `vello` is the obvious alternative. This
file is the number that answers "why". A second question rides along: what the
`PathCache` is worth, measured as two extra rows per sparse-strip backend.

## Machine

| | |
|---|---|
| CPU | AMD Ryzen 7 7800X3D (16 logical) |
| GPU | AMD Radeon RX 6600 (RADV NAVI23), Vulkan, discrete |
| OS | Linux x86-64 |
| Commit | `f42f2e8` (sparse-strip rows); `d385f22` for the classic rows, whose scene has not changed since |
| Profile | workspace `release`: `opt-level = "s"`, LTO, 1 codegen unit |

`opt-level = "s"` is the repository's choice for binary size and it costs the
CPU-side numbers something. Every column here pays it equally.

## Scene

A Kurv-sized editor: 40 knobs, 8 sliders, 200 labels, a clipped 60-row
scrolling list, two 50-cubic response curves, three floats, **20 wrapped
paragraphs** (each ~70 characters in a 150 px column, capped at 4 lines),
**4 image-filled pills** sharing one 2x2 RGBA buffer, and **6 cards carrying a
spring transition**. Resolved at 1280x800, it is **719 surfaces, 632 paint
ops, 474 glyph runs** — up from 674/530/382 before those three features
existed.

Reproduce:

```
cargo run -p mui-vello --release --features cpu --example bench
cargo run -p mui-vello --release --features cpu,bench-classic --example bench
```

**The pills are images on `vello_cpu` and `vello_hybrid`.** Hybrid wants an
atlas id, so `mui_vello::Gpu` uploads each buffer once through its `Atlas`
(`Renderer::upload_image`) and paints by id from then on; the four pills share
one buffer, so that is one upload per renderer, outside the timed frames.
Classic takes a `peniko::Image`, not a pixmap, and this bench does not build
one, so the classic row keeps plain `Raised` fills: the same 632 ops and the
same geometry, only the paint type of four of them differs.

## What is being timed

| Phase | What runs |
|---|---|
| resolve | `Ui::frame`: style, layout, text shaping, line breaking, springs, the z-ordered paint list |
| encode | `mui_vello::paint` (or `paint_cached`) onto a `Canvas` — arcs to cubics, plus whatever the backend does eagerly |
| render | rasterise and wait: GPU submit + `poll(wait)`, or `RenderContext::render` into a pixmap |

Cases: **cold** is a fresh `Ui` every frame, so nothing is shaped and no text
cache is warm. **static** is the same tree again, unchanged. **one knob
turning** changes one `f64` per frame.

Median of 50 frames after 5 warm-ups. Milliseconds.

## Results

| backend | case | resolve | encode | render | total | fps |
|---|---|---:|---:|---:|---:|---:|
| mui (resolve only) | static | 2.210 | — | — | 2.210 | 452 |
| vello_cpu | cold | 6.124 | 6.962 | 0.816 | 13.902 | 72 |
| vello_cpu | static | 2.219 | 4.853 | 0.816 | 7.887 | 127 |
| vello_cpu | one knob turning | 2.201 | 4.852 | 0.776 | 7.829 | 128 |
| vello_cpu cached | cold | 6.129 | 6.613 | 0.825 | 13.567 | 74 |
| vello_cpu cached | static | 2.216 | 4.296 | 0.799 | 7.311 | 137 |
| vello_cpu cached | one knob turning | 2.178 | 4.286 | 0.778 | 7.242 | 138 |
| vello_hybrid | cold | 6.034 | 6.970 | 0.647 | 13.651 | 73 |
| vello_hybrid | static | 2.220 | 4.876 | 0.649 | 7.746 | 129 |
| vello_hybrid | one knob turning | 2.220 | 4.906 | 0.634 | 7.759 | 129 |
| vello_hybrid cached | cold | 6.146 | 6.730 | 0.802 | 13.678 | 73 |
| vello_hybrid cached | static | 2.256 | 4.484 | 0.658 | 7.397 | 135 |
| vello_hybrid cached | one knob turning | 2.273 | 4.491 | 0.647 | 7.410 | 135 |
| vello (classic) | cold | 6.643 | 1.357 | 4.870 | 12.870 | 78 |
| vello (classic) | static | 2.626 | 1.393 | 4.455 | 8.474 | 118 |
| vello (classic) | one knob turning | 2.823 | 1.504 | 4.963 | 9.290 | 108 |

The bench also times the arc-to-cubic conversion on its own, outside any
backend:

```
bez conversion: 0.481 ms uncached, 0.188 ms from a warm PathCache (631 entries)
```

Memory: classic's `Scene::bump_estimate` still reports **0.5 MiB peak** of GPU
buffer — the new ops are rectangles and glyphs, which cost it nothing. Neither
sparse-strip backend exposes an equivalent; peak RSS of the whole process was
166 MiB, dominated by the font and the wgpu device, so it separates nothing.

### Before the features, for reference

The same bench on the same machine at commit `19a59b5`, when the scene was
674 surfaces / 530 paint ops / 382 glyph runs and `paint_cached` did not exist:

| backend | case | resolve | encode | render | total | fps |
|---|---|---:|---:|---:|---:|---:|
| vello_cpu | cold | 3.204 | 4.804 | 0.672 | 8.679 | 115 |
| vello_cpu | static | 1.670 | 3.294 | 0.669 | 5.633 | 178 |
| vello_hybrid | static | 1.664 | 3.302 | 0.933 | 5.899 | 170 |
| vello (classic) | static | 1.632 | 0.961 | 3.402 | 5.995 | 167 |

## Verdict

**The renderer comparison is unchanged, and it is still the smaller half of
the story.** Resolve is MUI's and is identical across all three backends —
2.2 ms warm, 6.1 ms cold. The split between classic and the sparse-strip
backends is the same architectural trade as before, now at larger absolute
numbers:

- classic encode 1.39 ms, render 4.46 ms
- hybrid encode 4.94 ms, render 0.68 ms

Classic's `Scene::fill` appends to an encoding buffer and the GPU flattens,
bins and tiles in compute. Hybrid flattens and builds sparse strips on the CPU
during the paint walk, then the GPU does one ordinary render pass. Totals are
still a wash — 7.75 ms hybrid against 8.47 ms classic — so the choice is still
made on what the numbers don't measure: classic needs compute shaders and MUI
has to render inside a plugin host's device, classic's render floor grew from
3.4 to 4.5 ms for 100 more ops while hybrid's render did not move at all, and
`vello_cpu` shares hybrid's pipeline so the no-GPU path is a reference rather
than a fallback (7.89 ms with no GPU, within noise of the GPU rows).

**What the new features cost is the finding.** Adding 20 wrapped paragraphs,
4 image pills and 6 spring-driven cards — 102 paint ops, 27% more — moved a
static frame from 5.9 ms to 7.7 ms and a cold frame from 8.7 to 13.7. Split:

- **Warm resolve went 1.67 → 2.22 ms (+0.55).** Wrapping is most of it, and
  it is the wrapping itself, not a second solve. At `0d523b1` this row read
  2.36 ms and `resolve_scene_with` solved the whole layout twice whenever a
  label wrapped: an unwrapped measure, a hint pass, a re-measure through
  `break_lines`. `f42f2e8` threads the room a node has (the narrowest definite
  inner width above it, a grid's column, nothing past a scroll) into the
  measurer, so a paragraph in a column or a grid cell wraps on the first
  solve. That removed 0.14 ms — 6% of resolve — which says the second pass
  was cheap and the per-frame line breaking is the cost. The springs are
  cheap by comparison — 6 cards is 6 channel walks per frame and the
  `static`/`one knob turning` rows are within noise of each other, which is
  what a settled spring should cost.
- **Cold resolve went 3.20 → 6.03 ms (+2.83).** Line breaking measures glyph
  advances one at a time over ~1400 characters with nothing cached. The
  single solve moved this by 0.1 ms, so the earlier reading that the second
  solve doubled it was wrong: the second pass hit a warm `Runs` cache. What
  doubles cold resolve is shaping the paragraphs at all.
- **Encode went 3.29 → 4.95 ms (+1.66) for 102 more ops**, which is Vello's
  strip generation over more glyph runs, on Vello's side of the seam.

What is left of the two-pass wrap is one case: a paragraph squeezed by a flex
row (`shrink` sharing a width with siblings) does not know its final main size
until the flex pass runs, so `resolve_scene_with` still records a hint and
solves again for those nodes only. The bench scene has none; a Kurv editor
with side-by-side descriptions would. The upgrade is the flex pass
re-measuring its items at their final main size, in `mui-layout`, and it is
listed in ROADMAP.md.

**The path cache is worth what it was worth, slightly more.** Static encode
drops 4.953 → 4.547 ms on `vello_cpu` and 4.944 → 4.598 on hybrid — about
0.3 to 0.4 ms, 7% of encode, 4% of the frame. The standalone number says why
it cannot be more: converting all 631 paths costs 0.517 ms and the cache
brings it to 0.192 ms. Everything else in the encode column is Vello's, and
the `vello (classic)` rows — encode 1.39 ms because the GPU flattens — are the
same paint walk with that work removed. The cold rows improve too (7.10 →
6.86), because "cold" throws away the `Ui` but rebuilds an identical tree, so
the paths still hit.

So MUI's own share of a static 7.7 ms frame is 2.22 ms of resolve plus 0.19 ms
of cached conversion. The **`mui (resolve only)`** row makes the floor explicit
at 2.21 ms with nothing painted at all. Resolve is 29% of the frame and 92% of
MUI's part of it; after this measurement the largest thing MUI can fix is
caching a paragraph's line breaks across frames (the text and the room rarely
change), ahead of anything in the renderer.

Glyph runs still need nothing. `vello_cpu` and `vello_hybrid` both hold a
`GlyphPrepCache` in the `Resources` MUI already threads through every frame
(`vello_cpu-0.2.0/src/render.rs:66`, `vello_hybrid-0.2.0/src/resources.rs:20`),
keyed on font blob id, glyph id, size, hinting and subpixel bucket
(`glifo-0.3.0/src/atlas/key.rs:50`). 474 runs instead of 382 cost only the
strip generation, not re-shaping.

Render on `vello_hybrid` is the quiet column (0.68 to 0.74 ms across every row
in this run, where an earlier run of the same binary wandered to 1.6 ms); it is
GPU submit plus `poll(wait)` and it moves between runs for reasons unrelated to
the paint list.

### What would change the answer

A scene an order of magnitude denser, or a machine whose CPU is much weaker
relative to its GPU. Both push toward classic. Neither describes a plugin UI on
a host's device, which is what MUI is.
