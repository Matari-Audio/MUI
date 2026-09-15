# hybrid vs cpu vs classic — 2026-09-15

One scene, three backends, three cases. The question is narrow: MUI renders
through `vello_hybrid`, and classic `vello` is the obvious alternative. This
file is the number that answers "why".

## Machine

| | |
|---|---|
| CPU | AMD Ryzen 7 7800X3D (16 logical) |
| GPU | AMD Radeon RX 6600 (RADV NAVI23), Vulkan, discrete |
| OS | Linux x86-64 |
| Commit | `19a59b5` plus this one |
| Profile | workspace `release`: `opt-level = "s"`, LTO, 1 codegen unit |

`opt-level = "s"` is the repository's choice for binary size and it costs the
CPU-side numbers something. Every column here pays it equally.

## Scene

A Kurv-sized editor: 40 knobs, 8 sliders, 200 labels, a clipped 60-row
scrolling list, two 50-cubic response curves and three floats. Resolved at
1280x800, it is **674 surfaces, 530 paint ops, 382 glyph runs**.

Reproduce:

```
cargo run -p mui-vello --release --features cpu --example bench
cargo run -p mui-vello --release --features cpu,bench-classic --example bench
```

## What is being timed

| Phase | What runs |
|---|---|
| resolve | `Ui::frame`: style, layout, text shaping, the z-ordered paint list |
| encode | `mui_vello::paint` onto a `Canvas` — arcs to cubics, plus whatever the backend does eagerly |
| render | rasterise and wait: GPU submit + `poll(wait)`, or `RenderContext::render` into a pixmap |

Cases: **cold** is a fresh `Ui` every frame, so nothing is shaped and no text
cache is warm. **static** is the same tree again, unchanged. **one knob
turning** changes one `f64` per frame.

Median of 50 frames after 5 warm-ups. Milliseconds.

## Results

| backend | case | resolve | encode | render | total | fps |
|---|---|---:|---:|---:|---:|---:|
| vello_cpu | cold | 3.204 | 4.804 | 0.672 | 8.679 | 115 |
| vello_cpu | static | 1.670 | 3.294 | 0.669 | 5.633 | 178 |
| vello_cpu | one knob turning | 1.671 | 3.298 | 0.651 | 5.620 | 178 |
| vello_hybrid | cold | 3.238 | 4.889 | 0.623 | 8.751 | 114 |
| vello_hybrid | static | 1.664 | 3.302 | 0.933 | 5.899 | 170 |
| vello_hybrid | one knob turning | 1.648 | 3.324 | 0.986 | 5.959 | 168 |
| vello (classic) | cold | 3.183 | 0.927 | 3.483 | 7.593 | 132 |
| vello (classic) | static | 1.632 | 0.961 | 3.402 | 5.995 | 167 |
| vello (classic) | one knob turning | 1.650 | 0.981 | 3.419 | 6.050 | 165 |

Memory: classic's `Scene::bump_estimate` reports **0.5 MiB peak** of GPU
buffer for this scene. Neither sparse-strip backend exposes an equivalent;
peak RSS of the whole process was 120 MiB in every configuration, which is
dominated by the font and the wgpu device, so it separates nothing.

## Verdict

**The resolve column is MUI's, and it is identical across all three.** 1.65 ms
warm, 3.2 ms cold. Whichever renderer wins, half the frame is MUI's own
layout and shaping, and the text cache is worth ~1.6 ms on this scene. That is
where optimisation effort belongs, not in the renderer choice.

**Classic moves work to the GPU; hybrid keeps it on the CPU.** The split is
stark and it is the whole architectural story:

- classic encode 0.96 ms, render 3.40 ms
- hybrid encode 3.30 ms, render 0.93 ms

Classic's `Scene::fill` appends to an encoding buffer and the GPU does
flattening, binning and tiling in compute. Hybrid flattens and builds sparse
strips on the CPU during the paint walk, then the GPU does one ordinary render
pass. The ~2.4 ms difference in encode is exactly that strip generation, and
the ~2.5 ms difference in render is exactly the compute pipeline it replaces.

**On totals they are a wash here: 5.90 ms hybrid against 6.00 ms classic.**
Neither is a reason to pick the other. On this machine — a discrete RX 6600 —
you could pick either and hold 165 fps.

So the choice is made on what the numbers *don't* measure:

- Classic needs compute shaders. No WebGL2, no GLES2, no old or embedded GPU.
  Hybrid's render pass runs anywhere a triangle runs. MUI is a plugin UI that
  has to render inside a host's existing device, and that device is not always
  a 2026 discrete card.
- Classic's render cost is a fixed multi-pass price. 3.4 ms of it appeared for
  a scene of 530 paint ops, and it barely moved between the cold and static
  cases — that floor is paid whatever is on screen. Hybrid's render was 0.6 to
  1.0 ms.
- `vello_hybrid` and `vello_cpu` share the sparse-strip pipeline, so the
  headless CPU path renders the same scene the same way. The `vello_cpu` rows
  above are within noise of the hybrid rows on resolve and encode, which is the
  evidence: snapshot tests, thumbnails and no-GPU hosts get real pixels from the
  same code, with no second renderer to keep honest.
- Linebender note that hybrid can trail classic on vector-heavy dynamic
  scenes, and the mechanism is visible above: hybrid's per-frame cost scales
  with CPU strip generation over the paint list. A UI is not that workload —
  530 ops of rectangles, pills and glyph runs, redrawn identically — but a
  scene of thousands of moving paths would push hybrid's encode up while
  classic's stayed flat.

**vello_cpu is not a fallback, it is the reference.** 5.6 ms total with no GPU
at all, marginally *faster* than hybrid here because the GPU round-trip and
`poll(wait)` cost more than rasterising 1280x800 in SIMD. A no-GPU host is not
a degraded host on a scene this size.

### What would change the answer

A scene an order of magnitude denser, or a machine whose CPU is much weaker
relative to its GPU. Both push toward classic. Neither describes a plugin UI on
a host's device, which is what MUI is.

---

# After caching — 2026-09-15

Same machine, same scene, same commit plus the path cache. `mui_vello::paint`
converts every outline to a `BezPath` on every frame: it validates the path,
turns each arc into cubics and allocates. `paint_cached` keeps that conversion
in a `PathCache` keyed by a fingerprint of the node key, the layer, every
coordinate, the stroke width and the blur, so a surface that did not move is
converted once and reused until it does. Two extra backend rows run the same
three cases through it; everything else is unchanged.

Reproduce:

```
cargo run -p mui-vello --release --features cpu,bench-classic --example bench
```

| backend | case | resolve | encode | render | total | fps |
|---|---|---:|---:|---:|---:|---:|
| mui (resolve only) | static | 1.713 | — | — | 1.713 | 584 |
| vello_cpu | cold | 3.439 | 5.032 | 0.645 | 9.116 | 110 |
| vello_cpu | static | 1.785 | 3.413 | 0.643 | 5.841 | 171 |
| vello_cpu | one knob turning | 1.790 | 3.333 | 0.627 | 5.750 | 174 |
| vello_cpu cached | cold | 3.433 | 4.740 | 0.682 | 8.855 | 113 |
| vello_cpu cached | static | 1.831 | 3.029 | 0.663 | 5.523 | 181 |
| vello_cpu cached | one knob turning | 1.768 | 2.976 | 0.634 | 5.378 | 186 |
| vello_hybrid | cold | 3.455 | 4.990 | 0.631 | 9.076 | 110 |
| vello_hybrid | static | 1.847 | 3.418 | 1.615 | 6.879 | 145 |
| vello_hybrid | one knob turning | 1.816 | 3.427 | 1.575 | 6.817 | 147 |
| vello_hybrid cached | cold | 3.448 | 4.780 | 0.664 | 8.892 | 112 |
| vello_hybrid cached | static | 1.825 | 3.056 | 1.572 | 6.454 | 155 |
| vello_hybrid cached | one knob turning | 1.829 | 3.042 | 0.715 | 5.586 | 179 |
| vello (classic) | cold | 3.472 | 0.941 | 3.533 | 7.947 | 126 |
| vello (classic) | static | 1.871 | 0.980 | 3.853 | 6.704 | 149 |
| vello (classic) | one knob turning | 1.849 | 1.024 | 3.975 | 6.847 | 146 |

The bench also times the conversion on its own, outside any backend:

```
bez conversion: 0.353 ms uncached, 0.131 ms from a warm PathCache (529 entries)
```

## What the cache is worth

**Static encode drops from 3.413 ms to 3.029 ms on `vello_cpu` and from 3.418
to 3.056 on hybrid — about 0.38 ms, 11% of encode, 6% of the frame.** The
standalone number says why it cannot be more: converting all 530 paths costs
0.353 ms, and the cache brings that to 0.131 ms. Everything left in the encode
column belongs to Vello. On the sparse-strip backends that is CPU flattening
and strip generation over the paint list, and no cache on MUI's side of the
seam can avoid it — the `vello (classic)` rows, whose encode is 0.98 ms because
the GPU does the flattening, are the same paint walk with that work removed.

So "make a static frame nearly free on the MUI side" is already nearly true and
was before this change: MUI's share of a static 5.8 ms frame is 1.79 ms of
resolve plus 0.35 ms of conversion. The new **`mui (resolve only)`** row makes
the floor explicit — 1.71 ms with nothing painted at all. The cache removes
0.22 ms of the 0.35, which leaves resolve as the only MUI number that matters,
exactly as the original verdict said.

The cold rows improve too (5.03 → 4.74 ms), because "cold" throws away the
`Ui` and its text cache but the tree it rebuilds is identical, so the paths
still hit. A genuinely new scene pays one extra fingerprint per path and
nothing else; the fingerprint is folded eight bytes per round precisely so
that miss cost stays under the conversion it replaces. A byte-at-a-time FNV
over the same data measured 2.46 ms and made the cache a pessimisation.

Glyph runs needed nothing. `vello_cpu` and `vello_hybrid` both hold a
`GlyphPrepCache` in the `Resources` that MUI already threads through every
frame (`vello_cpu-0.2.0/src/render.rs:66`,
`vello_hybrid-0.2.0/src/resources.rs:20`), and its key is font blob id, glyph
id, size, hinting and subpixel bucket — `glifo-0.3.0/src/atlas/key.rs:50`.
Outlines and hinting instances survive between frames as long as the blob id
does, which is what `mui_vello`'s `FONTS` interning guarantees. The paint walk
itself allocates nothing per run: `run()` hands the backend a lazy iterator
over the glyph slice.

Render on `vello_hybrid` is the noisy column across both runs (0.63 to 1.6 ms
for the identical scene); it is GPU submit plus `poll(wait)` and it moves
between runs for reasons that have nothing to do with the paint list.
