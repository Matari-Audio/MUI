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
