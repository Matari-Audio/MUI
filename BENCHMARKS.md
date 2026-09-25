# Benchmarks -- 2026-09-23

What a frame of MUI costs, before and after the overhaul (`d75b234` ->
`HEAD` of `t3code/53b2a817`, stages A to C: fonts as a type, the deleted
caches and shims, the scene and runtime split, `mui-widgets` folded into
`mui`). Every number is a median. No row is a sum of per-phase medians and
there is no FPS column (see `docs/rendering-investigation.md`).

## Since `780c3f4`: render-perf, scene-copies, interaction-access, daw-host

Base `780c3f4` against the merge of the four branches, run alternately
(base, head, base, head ...) 6 times each under `--profile perf`, same
machine. **Load average 58 to 68 on 16 threads** the whole time (other
agents building), so every absolute number here is 2 to 5 times the idle
figures below and only the before -> after column means anything; under 10%
is noise, and some rows moved more than that both ways between runs.

Each tree ran its own `bench.rs`. At `780c3f4` the bench built a new image
`Arc` every frame, which defeated every retained cache; head keeps one and adds
a `fresh image` row for that case. The `static` and `one knob turning` encode
rows below include that bench fix as well as the library change (the CPU
glyph atlas from a font's second frame).

| measurement (median ms) | 780c3f4 | head | change |
|---|---:|---:|---:|
| bench, mui (resolve only), static: total | 3.773 | 3.161 | -16% |
| bench, vello_cpu, cold: total | 26.115 | 20.586 | -21% |
| bench, vello_cpu, static: encode | 14.131 | 6.992 | -51% |
| bench, vello_cpu, static: total | 29.481 | 24.024 | -19% |
| bench, vello_cpu, one knob turning: encode | 12.898 | 7.582 | -41% |
| bench, vello_cpu, one knob turning: total | 24.419 | 21.284 | -13% |
| bench, vello_cpu, static: render | 0.833 | 1.162 | **+40%, slower** |
| bench, vello_hybrid, cold: total | 20.084 | 18.425 | -8% (noise) |
| bench, vello_hybrid, static: total | 15.131 | 12.941 | -14% |
| bench, vello_hybrid, one knob turning: total | 13.527 | 12.713 | -6% (noise) |
| bench, arc-to-cubic conversion per frame | 0.152 | 0.160 | same |
| bench, peak RSS | 96.1 MiB | 110.7 MiB | **+15%, larger** |
| stress 1280x800: warm resolve | 1.892 | 1.075 | -43% |
| stress 240x2400: warm resolve | 1.365 | 1.079 | -21% |
| stress 2000x300: warm resolve | 1.146 | 1.058 | -8% (noise) |
| stress 1280x800: cold resolve | 7.409 | 9.172 | +24% (noise: runs spread 5.2 to 10.4 at base, 5.6 to 18.9 at head) |
| stress 240x2400 / 2000x300: cold resolve | 5.346 / 5.827 | 4.197 / 4.482 | -21% / -23% |
| stress: bare layout solve (3 shapes) | 0.322 / 0.324 / 0.324 | 0.340 / 0.330 / 0.328 | same |
| stress: allocations per warm resolve | 5924, 3161 KiB | 5310, 2737 KiB | -10%, -13% (exact) |
| frame_cost: pill resolve (cold) | 0.163 | 0.156 | same |
| frame_cost: `Ui::frame`, sliders + knob | 0.059 | 0.029 | -51% (base spread 0.029 to 0.174) |
| frame_cost: mui paint walk | 0.014 | 0.015 | same |

**What did not improve.** The paint walk in `frame_cost` is unchanged: the
free `paint()` still converts every path every frame (only the retained
renderers keep conversions now), so the 0.001 ms of the old `PathCache` is
not back. Arc-to-cubic conversion in the bench is the same. `vello_cpu`
render reads 40% higher, since glyphs now come from atlas bitmaps that the
render stage composites; the encode saving is about twenty times that. Peak RSS
grew by the CPU glyph atlas pages. `vello_hybrid` encode is unchanged in
kind (no glyph atlas on the GPU canvas, see ROADMAP). The retained
renderers' skipped render pass (`hybrid retained static`, 1 -> 0 renders
over 55 frames) is behind the `gpu-effects` rows, which this run did not
take; the render-perf builder measured 3.253 -> 1.558 ms at load 27 to 38.

## Machine and conditions

| | |
|---|---|
| CPU | AMD Ryzen 7 7800X3D, 16 logical |
| GPU | AMD Radeon RX 6600 (RADV NAVI23), Vulkan, discrete |
| OS | Linux 7.3.0-rc3 (CachyOS), rustc 1.98.1 |
| Profile | `perf`: `release` with `opt-level = 3`, LTO, 1 codegen unit, `debug = 1` |

The machine was shared while these ran: other builds and a DAW were running,
and the load average was 6 to 19. The after-numbers are medians over 3 to 6
runs of each command; the before column is a single run of `d75b234`, under
unrecorded load, so its deltas are indicative, not medians against medians.
Treat a difference under about 10% as noise. The GPU render column on `vello_hybrid` was the noisiest (0.56 to 1.6 ms
for the same binary across runs), because the GPU was shared too.

## Commands

```
cargo run -p mui-scene --profile perf --example stress
cargo run -p mui-vello --profile perf --features cpu,gpu-effects --example bench
cargo run -p mui-vello --profile perf --features cpu-threads,gpu-effects --example bench
cargo test -p mui-preview --profile perf --test frame_cost -- --nocapture
```

`bench` is a Kurv-sized editor: 40 knobs, 8 sliders, 200 labels, a clipped
60-row list, two 50-cubic curves, three floats, 20 wrapped paragraphs, 4 image
pills and 6 animated cards. At 1280x800 it resolves to 717 surfaces, 632 paint
ops and 472 glyph runs, the same scene before and after. Each row is the median
of 50 frames after 5 warm-ups. `total` is the median of the measured per-frame
total, and `p95` is its 95th percentile. Build is the tree, resolve is
`Ui::frame`, encode is `mui_vello::paint` onto the canvas, and render is
rasterise-and-wait. **cold** uses a fresh `Ui` every frame, **static** hands in
the same tree again, and **one knob turning** changes one value per frame.

`stress` resolves an 873-node tree at three window shapes and counts
allocations. `frame_cost` prints the mean of 50 iterations per line; the
figure below is the median of 8 runs of the test.

## Before -> after

Milliseconds. Before is `d75b234`, after is this commit.

| measurement | before | after | change |
|---|---:|---:|---:|
| bench, mui (resolve only), static: total | 2.156 | 1.418 | -34% |
| bench, vello_cpu, cold: total | 12.865 | 10.819 | -16% |
| bench, vello_cpu, static: total | 5.921 | 5.125 | -13% |
| bench, vello_cpu, one knob turning: total | 5.849 | 5.037 | -14% |
| bench, vello_hybrid, cold: total | 13.334 | 11.932 | -11% |
| bench, vello_hybrid, static: total | 6.262 | 5.938 | -5% (noise) |
| bench, vello_hybrid, one knob turning: total | 6.167 | 5.809 | -6% (noise) |
| bench, static resolve (`Ui::frame`), vello_cpu row | 2.175 | 1.347 | -38% |
| bench, cold resolve, vello_cpu row | 8.243 | 5.928 | -28% |
| bench, static encode, vello_cpu | 3.212 | 3.270 | +2% (noise) |
| bench, static encode, vello_cpu with the old `PathCache` | 2.984 | -- | cache deleted |
| bench, static render, vello_hybrid | 0.575 | 0.725 | +26%, GPU contention |
| bench, arc-to-cubic conversion per frame | 0.102 | 0.106 | same |
| bench, peak RSS | 164.0 MiB | 105 MiB | -36% |
| stress 1280x800: warm resolve | 0.975 | 0.870 | -11% |
| stress 240x2400: warm resolve | 1.566 | 0.678 | -57% |
| stress 2000x300: warm resolve | 1.525 | 0.645 | -58% |
| stress 1280x800: cold resolve | 4.851 | 4.424 | -9% |
| stress 240x2400: cold resolve | 4.473 | 2.318 | -48% |
| stress 2000x300: cold resolve | 3.433 | 2.061 | -40% |
| stress: bare layout solve (3 shapes) | 0.190 / 0.220 / 0.323 | 0.194 / 0.195 / 0.195 | same / -11% / -40% |
| stress: allocations per warm resolve | 14257 | 5924 | -58% |
| stress: bytes allocated per warm resolve | 4631 KiB | 3177 KiB | -31% |
| frame_cost: pill resolve (union + shell + text, cold) | 0.096 | 0.098 | same |
| frame_cost: `Ui::frame`, sliders + knob | 0.025 | 0.020 | -20% |
| frame_cost: mui paint walk | 0.001 | 0.011 | **+10 us, slower** |

**What got slower.** The paint walk in `frame_cost` went from 1 to 11
microseconds, because stage B deleted `PathCache`. The walk now converts every
arc to cubics every frame. In the full bench the conversion alone measures
0.106 ms a frame, against 0.026 ms from the old warm cache. In the baseline run
the `vello_cpu cached` static row beat the uncached one by 0.23 ms of encode
(2.984 against 3.212). Since these numbers the walk refills one reused
`BezPath` instead of allocating one per entry; that is not re-measured here.
The `vello_hybrid` render column
reads higher, but the same binary spread from 0.56 to 1.6 ms across runs, so
that change is load on a shared GPU, not a regression.

**What got faster.** Resolve, warm and cold, and the allocation count behind
it: 14257 -> 5924 allocations per warm resolve. This file does not attribute
the win to individual stage A/B commits. Stage C (the scene and runtime split, the widget fold,
`preset`/`base` by move) did not move these numbers. The C1 builder measured
stress warm resolve before and after its refactor (0.894 -> 0.878 ms at
1280x800) and got identical allocation counts, and it claimed no speedup.

## Current numbers

### bench, `--features cpu` (median of 6 runs)

| backend | case | build | resolve | encode | render | total | p95 |
|---|---|---:|---:|---:|---:|---:|---:|
| mui (resolve only) | static | 0.070 | 1.335 | -- | -- | 1.418 | 2.146 |
| vello_cpu | cold | 0.088 | 5.928 | 4.378 | 0.365 | 10.819 | 12.619 |
| vello_cpu | static | 0.075 | 1.347 | 3.270 | 0.361 | 5.125 | 5.909 |
| vello_cpu | one knob turning | 0.071 | 1.220 | 3.289 | 0.350 | 5.037 | 6.188 |
| vello_hybrid | cold | 0.097 | 6.080 | 4.488 | 0.762 | 11.932 | 13.560 |
| vello_hybrid | static | 0.082 | 1.433 | 3.371 | 0.725 | 5.938 | 6.704 |
| vello_hybrid | one knob turning | 0.077 | 1.315 | 3.359 | 0.649 | 5.809 | 6.627 |

### bench, `--features cpu,bench-classic` (median of 4 runs)

| backend | case | resolve | encode | render | total | p95 |
|---|---|---:|---:|---:|---:|---:|
| vello_hybrid | static | 1.409 | 3.252 | 1.332 | 6.261 | 6.878 |
| vello (classic) | cold | 6.016 | 0.253 | 2.908 | 9.525 | 11.826 |
| vello (classic) | static | 1.552 | 0.262 | 4.055 | 5.957 | 6.626 |
| vello (classic) | one knob turning | 1.377 | 0.260 | 3.516 | 5.722 | 6.595 |

The trade is the same as it has always been. Classic's encode is buffer
appends, and the GPU flattens and tiles in compute. Hybrid builds its sparse
strips on the CPU during the walk and then renders one ordinary pass. Totals
are a wash (5.9 to 6.3 ms either way under this load), so the choice is made on
what the table does not show: classic needs compute shaders, and MUI has to
render inside a plugin host's device. `vello_cpu` shares hybrid's pipeline, so
the no-GPU path is a real renderer and not a fallback. Classic reported a
2.5 MiB peak GPU buffer estimate.

### bench, `--features cpu-threads` (median of 3 runs)

| backend | case | resolve | encode | render | total | p95 |
|---|---|---:|---:|---:|---:|---:|
| vello_cpu | cold | 5.944 | 4.085 | 0.201 | 10.421 | 11.987 |
| vello_cpu | static | 1.372 | 2.910 | 0.200 | 4.675 | 5.891 |
| vello_cpu | one knob turning | 1.250 | 2.943 | 0.185 | 4.517 | 5.722 |

With the rayon pool, strip generation and rasterisation both leave the main
thread: static `vello_cpu` goes from 5.125 to 4.675 ms. The pool stays off by
default. A snapshot-sized pixmap loses more to thread hand-off than it gains,
and a plugin host may not want MUI spawning threads.

### stress (median of 6 runs)

| window | cold | warm resolve | bare solve | allocations / warm resolve |
|---|---:|---:|---:|---:|
| 1280x800 | 4.424 | 0.870 | 0.194 | 5924, 3177 KiB |
| 240x2400 | 2.318 | 0.678 | 0.195 | 5932, 3178 KiB |
| 2000x300 | 2.061 | 0.645 | 0.195 | 5924, 3177 KiB |

A bare solve is 2907 allocations and 329 KiB, the same as before.

## Where the frame goes

In a static `vello_cpu` frame (5.1 ms), MUI's own resolve is 1.35 ms and the
paint walk plus Vello's strip generation (encode) is 3.3 ms. MUI's
arc-to-cubic conversion is 0.106 ms of that encode, and most of the rest is
Vello. The next
real win is on Vello's side of that seam, not in the walk.
