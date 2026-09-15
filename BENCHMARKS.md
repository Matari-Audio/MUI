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
| Commit | this one (`feat/idiomatic`): every row re-measured in one run after the mui-core walk changes |
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
| mui (resolve only) | static | 1.234 | — | — | 1.234 | 810 |
| vello_cpu | cold | 5.111 | 5.852 | 0.777 | 11.740 | 85 |
| vello_cpu | static | 1.269 | 3.635 | 0.792 | 5.696 | 176 |
| vello_cpu | one knob turning | 1.250 | 3.621 | 0.770 | 5.641 | 177 |
| vello_cpu cached | cold | 5.130 | 5.814 | 0.788 | 11.732 | 85 |
| vello_cpu cached | static | 1.266 | 3.585 | 0.799 | 5.650 | 177 |
| vello_cpu cached | one knob turning | 1.254 | 3.581 | 0.774 | 5.608 | 178 |
| vello_hybrid | cold | 5.188 | 5.986 | 0.644 | 11.818 | 85 |
| vello_hybrid | static | 1.258 | 3.712 | 0.654 | 5.623 | 178 |
| vello_hybrid | one knob turning | 1.270 | 3.709 | 0.654 | 5.632 | 178 |
| vello_hybrid cached | cold | 5.160 | 5.885 | 0.631 | 11.676 | 86 |
| vello_hybrid cached | static | 1.268 | 3.685 | 0.656 | 5.609 | 178 |
| vello_hybrid cached | one knob turning | 1.263 | 3.645 | 0.657 | 5.565 | 180 |
| vello (classic) | cold | 5.168 | 0.280 | 4.285 | 9.733 | 103 |
| vello (classic) | static | 1.274 | 0.277 | 4.307 | 5.858 | 171 |
| vello (classic) | one knob turning | 1.270 | 0.279 | 4.318 | 5.867 | 170 |

The bench also times the arc-to-cubic conversion on its own, outside any
backend:

```
bez conversion: 0.089 ms uncached, 0.023 ms from a warm PathCache (571 entries)
```

Memory: classic's `Scene::bump_estimate` still reports **0.5 MiB peak** of GPU
buffer — the new ops are rectangles and glyphs, which cost it nothing. Neither
sparse-strip backend exposes an equivalent; peak RSS of the whole process was
149 MiB, dominated by the font and the wgpu device, so it separates nothing.

### `cpu-threads`, measured

`vello_cpu` builds a `SingleThreadedDispatcher` unless the `multithreading`
feature is on, so every row above rasterises on one core. `mui-vello`'s
`cpu-threads` feature turns it on (`cargo run -p mui-vello --release --features
cpu-threads --example bench`), and on this machine it moves both CPU columns —
strip generation is dispatched to the pool too, so `encode` drops as well as
`render`:

| backend | case | resolve | encode | render | total | fps |
|---|---|---:|---:|---:|---:|---:|
| vello_cpu | static | 1.306 | 2.906 | 0.266 | 4.478 | 223 |
| vello_cpu cached | static | 1.317 | 2.750 | 0.267 | 4.333 | 231 |
| vello_cpu cached | cold | 5.243 | 5.150 | 0.260 | 10.653 | 94 |

Against 5.725 / 11.756 single-threaded: a warm CPU frame goes 175 → 231 fps.
It stays off by default — a 120x60 snapshot pixmap loses more to thread
hand-off than it gains, and a plugin host may not want MUI spawning a pool.

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

**The renderer comparison is unchanged, and it is now the larger half of the
story.** Resolve is MUI's and is identical across all four rows — 1.27 ms warm,
5.1 ms cold. The split between classic and the sparse-strip backends is the
same architectural trade as before:

- classic encode 0.28 ms, render 4.31 ms
- hybrid encode 3.71 ms, render 0.65 ms

Classic's `Scene::fill` appends to an encoding buffer and the GPU flattens,
bins and tiles in compute. Hybrid flattens and builds sparse strips on the CPU
during the paint walk, then the GPU does one ordinary render pass. Totals are
still a wash — 5.62 ms hybrid against 5.86 ms classic — so the choice is still
made on what the numbers don't measure: classic needs compute shaders and MUI
has to render inside a plugin host's device, and `vello_cpu` shares hybrid's
pipeline so the no-GPU path is a reference rather than a fallback (5.70 ms with
no GPU, within noise of the GPU rows).

**The paint walk was the finding, and it has been cut.** The previous run of
this file read 2.21 ms of resolve and a 7.75 ms hybrid frame, and blamed the
line breaking. Instrumenting `resolve_scene_with` said otherwise: the first
solve was 0.27 ms, the hint pass and second solve 0.013 ms, and the paint walk
1.82 ms — 86% of resolve. `break_lines` over all 20 paragraphs costs 0.051 ms
a pass, so caching line breaks across frames was never worth 0.1 ms. Four
things in the walk were:

- **`ResolvedSurface::bounds` flattened every outline to a polyline.** 719
  surfaces, every frame, at tolerance 0.5, into two `Vec`s — for a field whose
  value is already in the `rect` beside it for all but a welded node, and which
  nothing outside a test read. Reading it off the rect: 2.12 → 1.83 ms.
- **Every text node rescanned the whole paint list to unpaint its own Fill.**
  A `retain` with a `String` compare per entry, once per text node, over a list
  that grows to 632 — an O(n²) hunt for an entry this node pushed thirty lines
  earlier. An `rposition` and a `remove`: 1.83 → 1.67 ms.
- **Glyph ink was translated into a fresh `Path` per line and never drawn.**
  `mui_vello::one` returns through `canvas.glyphs(..)` the moment `p.text` is
  set, so the outline only ever reached `bounding_box`, after `paint_cached`
  had converted and fingerprinted it. Leaving `Painted::path` empty when the
  glyphs are attached: 1.67 → 1.23 ms of resolve, and encode 4.30 → 3.59 ms.
  `PathCache` holds 571 entries now instead of 631, and conversion costs
  0.089 ms rather than 0.481.
- **Key strings were heap-copied four times per node per frame.** `Arc<str>`
  makes the other three a refcount bump.

Between them: **warm resolve 2.21 → 1.23 ms, a static hybrid frame 7.75 →
5.62 ms.** Cold resolve went 6.03 → 5.16 ms, which is the same walk saving
against unchanged shaping — cold is still dominated by shaping ~1400
characters with nothing cached.

**The path cache is now worth almost nothing.** Static encode drops 3.635 →
3.585 ms on `vello_cpu` and 3.712 → 3.685 on hybrid, because the work it
cached was mostly the glyph outlines that no longer exist. Converting the
remaining 571 paths costs 0.089 ms and the cache brings it to 0.023. The
`vello (classic)` encode column — 0.28 ms, because the GPU flattens — is the
same paint walk with that work removed, and it is the ceiling the cache is
chasing.

So MUI's own share of a static 5.62 ms frame is 1.27 ms of resolve plus
0.02 ms of cached conversion: 23% of the frame, and the **`mui (resolve only)`**
row puts the floor at 1.23 ms with nothing painted. What is left in the walk is
the outline work itself — `RoundedRect::path` per node, per shell, per stroke —
and it is shared geometry, not a redundant copy. The next real win is on
Vello's side of the seam: 3.6 ms of strip generation against classic's 0.28 ms
of buffer appends.

What is left of the two-pass wrap is one case: a paragraph squeezed by a flex
row (`shrink` sharing a width with siblings) does not know its final main size
until the flex pass runs, so `resolve_scene_with` still records a hint and
solves again for those nodes only. `wrap_hints` keys on the text node's own
frame, so a squeeze that lands on an intermediate container is invisible to it
until mui-layout clamps a container's children to its cross size. The upgrade
is the flex pass re-measuring its items at their final main size, in
`mui-layout`, and it is listed in ROADMAP.md.

Glyph runs still need nothing. `vello_cpu` and `vello_hybrid` both hold a
`GlyphPrepCache` in the `Resources` MUI already threads through every frame
(`vello_cpu-0.2.0/src/render.rs:66`, `vello_hybrid-0.2.0/src/resources.rs:20`),
keyed on font blob id, glyph id, size, hinting and subpixel bucket
(`glifo-0.3.0/src/atlas/key.rs:50`). 474 runs cost only the strip
generation, not re-shaping.

Render on `vello_hybrid` is the quiet column (0.63 to 0.66 ms across every row
in this run, where an earlier run of the same binary wandered to 1.6 ms); it is
GPU submit plus `poll(wait)` and it moves between runs for reasons unrelated to
the paint list.

### What would change the answer

A scene an order of magnitude denser, or a machine whose CPU is much weaker
relative to its GPU. Both push toward classic. Neither describes a plugin UI on
a host's device, which is what MUI is.
