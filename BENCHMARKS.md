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
| Commit | `d385f22` plus this one |
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

**The pills are images only on `vello_cpu`.** `vello_hybrid` panics on a
pixmap image source outright (`pixmap image sources are not supported by Vello
Hybrid`) — it wants an atlas id from `Renderer::upload_image`, which is the
GPU registration `mui-vello` has not done — and classic takes a
`peniko::Image`, not a pixmap. On those backends the same four pills are plain
`Raised` fills, so every row is the same 632 ops and the same geometry; only
the paint type of four of them differs.

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
| mui (resolve only) | static | 2.388 | — | — | 2.388 | 419 |
| vello_cpu | cold | 6.476 | 7.109 | 0.798 | 14.383 | 70 |
| vello_cpu | static | 2.491 | 4.855 | 0.789 | 8.135 | 123 |
| vello_cpu | one knob turning | 2.497 | 4.919 | 0.776 | 8.191 | 122 |
| vello_cpu cached | cold | 6.621 | 6.925 | 0.834 | 14.380 | 70 |
| vello_cpu cached | static | 2.562 | 4.557 | 0.815 | 7.934 | 126 |
| vello_cpu cached | one knob turning | 2.552 | 4.472 | 0.796 | 7.821 | 128 |
| vello_hybrid | cold | 6.448 | 7.115 | 0.690 | 14.253 | 70 |
| vello_hybrid | static | 2.582 | 4.974 | 0.721 | 8.278 | 121 |
| vello_hybrid | one knob turning | 2.659 | 5.012 | 0.677 | 8.348 | 120 |
| vello_hybrid cached | cold | 6.512 | 6.824 | 0.716 | 14.052 | 71 |
| vello_hybrid cached | static | 2.588 | 4.550 | 0.703 | 7.841 | 128 |
| vello_hybrid cached | one knob turning | 2.631 | 4.572 | 0.696 | 7.900 | 127 |
| vello (classic) | cold | 6.643 | 1.315 | 5.241 | 13.199 | 76 |
| vello (classic) | static | 2.564 | 1.351 | 4.449 | 8.363 | 120 |
| vello (classic) | one knob turning | 2.657 | 1.450 | 4.531 | 8.638 | 116 |

The bench also times the arc-to-cubic conversion on its own, outside any
backend:

```
bez conversion: 0.475 ms uncached, 0.192 ms from a warm PathCache (631 entries)
```

Memory: classic's `Scene::bump_estimate` still reports **0.5 MiB peak** of GPU
buffer — the new ops are rectangles and glyphs, which cost it nothing. Neither
sparse-strip backend exposes an equivalent; peak RSS of the whole process was
175 MiB, dominated by the font and the wgpu device, so it separates nothing.

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
2.5 ms warm, 6.5 ms cold. The split between classic and the sparse-strip
backends is the same architectural trade as before, now at larger absolute
numbers:

- classic encode 1.35 ms, render 4.45 ms
- hybrid encode 4.97 ms, render 0.72 ms

Classic's `Scene::fill` appends to an encoding buffer and the GPU flattens,
bins and tiles in compute. Hybrid flattens and builds sparse strips on the CPU
during the paint walk, then the GPU does one ordinary render pass. Totals are
still a wash — 8.28 ms hybrid against 8.36 ms classic — so the choice is still
made on what the numbers don't measure: classic needs compute shaders and MUI
has to render inside a plugin host's device, classic's render floor grew from
3.4 to 4.5 ms for 100 more ops while hybrid's render did not move at all, and
`vello_cpu` shares hybrid's pipeline so the no-GPU path is a reference rather
than a fallback (8.13 ms with no GPU, still the fastest total in the table).

**What the new features cost is the finding.** Adding 20 wrapped paragraphs,
4 image pills and 6 spring-driven cards — 102 paint ops, 27% more — moved a
static frame from 5.9 ms to 8.3 ms and a cold frame from 8.7 to 14.4. Split:

- **Warm resolve went 1.67 → 2.49 ms (+0.82).** Wrapping is most of it. When
  any label wraps, `resolve_scene_with` solves the whole layout twice: pass
  one measures every label unwrapped, a hint pass records how much room each
  wrapping label's parent really has, and pass two re-measures those labels
  through `break_lines`. 20 paragraphs put the entire 719-node tree through a
  second solve. The springs are cheap by comparison — 6 cards is 6 channel
  walks per frame and the `static`/`one knob turning` rows are within noise of
  each other, which is what a settled spring should cost.
- **Cold resolve went 3.20 → 6.48 ms (+3.28).** Line breaking measures glyph
  advances one at a time over ~1400 characters with nothing cached, then the
  second solve does it again. This is the one number that genuinely doubled.
- **Encode went 3.29 → 4.86 ms (+1.57) for 102 more ops**, which is Vello's
  strip generation over more glyph runs, on Vello's side of the seam.

The fix for both resolve numbers is the same one-line change, and it is not in
this crate: `mui_layout::resolve_with`'s measurer is `FnMut(&P) -> Size` and is
never handed the offered width, which is the only reason the second solve
exists. Hand the measurer its offered size and the hint pass, the second solve
and the `(String, f64)` hint-collision bug go together.

**The path cache is worth what it was worth, slightly more.** Static encode
drops 4.855 → 4.557 ms on `vello_cpu` and 4.974 → 4.550 on hybrid — about
0.3 to 0.4 ms, 7% of encode, 4% of the frame. The standalone number says why
it cannot be more: converting all 631 paths costs 0.475 ms and the cache
brings it to 0.192 ms. Everything else in the encode column is Vello's, and
the `vello (classic)` rows — encode 1.35 ms because the GPU flattens — are the
same paint walk with that work removed. The cold rows improve too (7.11 →
6.93), because "cold" throws away the `Ui` but rebuilds an identical tree, so
the paths still hit.

So MUI's own share of a static 7.9 ms frame is 2.56 ms of resolve plus 0.19 ms
of cached conversion. The **`mui (resolve only)`** row makes the floor explicit
at 2.39 ms with nothing painted at all. Resolve is now 32% of the frame and
95% of MUI's part of it; after this measurement the wrapping double-solve is
the single largest thing MUI can fix, ahead of anything in the renderer.

Glyph runs still need nothing. `vello_cpu` and `vello_hybrid` both hold a
`GlyphPrepCache` in the `Resources` MUI already threads through every frame
(`vello_cpu-0.2.0/src/render.rs:66`, `vello_hybrid-0.2.0/src/resources.rs:20`),
keyed on font blob id, glyph id, size, hinting and subpixel bucket
(`glifo-0.3.0/src/atlas/key.rs:50`). 474 runs instead of 382 cost only the
strip generation, not re-shaping.

Render on `vello_hybrid` is the quiet column (0.68 to 0.72 ms across every row
in this run, where an earlier run of the same binary wandered to 1.6 ms); it is
GPU submit plus `poll(wait)` and it moves between runs for reasons unrelated to
the paint list.

### What would change the answer

A scene an order of magnitude denser, or a machine whose CPU is much weaker
relative to its GPU. Both push toward classic. Neither describes a plugin UI on
a host's device, which is what MUI is.
