# MUI rendering investigation — 2026-09-20

## Decision

Aim for order-of-magnitude savings by avoiding repeated work and choosing the appropriate rendering pipeline for the workload. A global renderer substitution alone is not sufficient. Keep the production backend until visual parity, device support, and actual KURV workloads justify changing it.

Target: a four-core laptop, integrated graphics, 8 GB RAM, 1080p at 60 Hz, with audio running. This target has **not** been validated by the desktop measurements below.

## Repository and evidence state

The investigation starts from MUI `5942d19` (0.3.2), after the shape/layout integration and first optimization pass. KURV was integrated at `8aa6e8a`, using that MUI revision. MUI's relevant production path is Vello Hybrid; old GPUI experiments do not describe KURV's active renderer. Current KURV work must be audited separately before changing its pin.

A temporary research worktree and its raw logs disappeared during this investigation. This persistent branch restores the root-cause fix and the core benchmark improvements. Earlier retention/atlas/readout measurements are explicitly provisional below: they survive in investigation notes, but their original logs and patches are unavailable. New evidence in `rendering-evidence/` is reproducible from this branch. Do not mix those evidence levels.

## Where the work goes

KURV's host already skips settled idle frames. During animation or interaction it constructs the tree, resolves a complete MUI frame, rebuilds hit-testing/paint data, and re-encodes the scene. Processing queued events can produce multiple resolves before one presentation. Preserving event ordering and gesture edges is essential if these resolves are coalesced.

MUI's incremental layout still scans and validates the tree. Cached layout does not mean no tree construction, styling, text-key generation, scene traversal, or paint-list allocation. The existing `PathCache` remembers conversion into Bezier paths; Hybrid still processes those paths into sparse strips. This distinction matters for thousands of animated curves.

The existing `Ui::set_text` supports a reserved single-line readout without a full layout rebuild. Reuse this API for suitable meters/readouts before inventing a new update system. Structural changes, wrapping, accessibility, hit testing, and animated geometry still need their respective updates.

## Confirmed bug and shipped experiment

`Text::eq` compared the identity of the outer font-list allocation. Resolving the same scene builds another list containing the same font objects, so unchanged text was marked unequal. Retained renderers could re-encode static text and damage tracking could repaint unnecessarily.

The fix compares list length and each font object's identity. It retains identity-based font invalidation without comparing complete font files. The regression proves identical consecutive scenes compare equal and newly allocated font data remains unequal. It failed before the fix; all 123 mui-scene tests and doctests pass afterward. This is a correctness fix enabling retention, not a claim that KURV now renders ten times faster.

## Visage: what to learn

Visage's frame drawing checks whether that frame needs redrawing and updates its associated region. Its architecture makes a local visual change local work, rather than reconstructing the application paint description every time. MUI should adopt that property where its existing retained APIs allow it. [Frame implementation](https://github.com/VitalAudio/visage/blob/828037000d0893647ab29b66ae9c4a241c90f671/visage_ui/frame.cpp)

Visage also tracks damaged regions in layers, batches compatible shapes, caches glyph/path data in atlases, and uses specialized primitive/graph shaders. Those are complementary techniques. Layer textures have memory and composition costs; graph shaders have pixel/overdraw costs. Copying every layer or shader would not automatically improve MUI. [Layer implementation](https://github.com/VitalAudio/visage/blob/828037000d0893647ab29b66ae9c4a241c90f671/visage_graphics/layer.cpp), [graph shader](https://github.com/VitalAudio/visage/blob/828037000d0893647ab29b66ae9c4a241c90f671/visage_graphics/shaders/fs_graph_line.sc)

No equivalent Visage application was benchmarked. These are architectural observations, not evidence that Visage is faster by a particular factor.

## Vello alternatives

| Path | Where substantial work happens | Candidate use | Main question |
|---|---|---|---|
| Pinned Hybrid | CPU path processing, GPU raster/composition | Broad GPU support, retained controls | Can unchanged strips/regions survive frames? |
| Classic compute Vello | More vector processing on GPU compute | Dense changing vector geometry | Device limits, memory, effects and text parity |
| Vello CPU | CPU processing and rasterization | Fallback, small damaged areas | Can updates remain bounded on weak CPUs? |
| Current upstream Vello GPU | Successor naming/evolution of the Hybrid architecture | Future dependency upgrade | Measure the actual new revision before attributing gains |

The upstream architecture separates CPU sparse-strip preparation from GPU rendering; the compute renderer is a different tradeoff. Upstream naming/version changes do not establish a performance gain in the pinned release. [Vello architecture](https://github.com/linebender/vello/blob/60618fc3ff3343aeabbceb25180be6c6319154e3/ARCHITECTURE.md)

Qt Quick likewise emphasizes retaining geometry, batching, and texture atlases. Transform updates can avoid rebuilding geometry; clipping and alpha ordering constrain batching. This reinforces the case for minimizing changed data before optimizing draw submission. [Qt renderer documentation](https://doc.qt.io/qt-6/qtquick-visualcanvas-scenegraph-renderer.html)

## Benchmark corrections

The original benchmark excluded tree construction, summed independent phase medians, printed a reciprocal latency as FPS, supplied different image content to classic, and recreated classic font identity each frame.

The restored benchmark includes tree construction, records actual per-frame total latency and p95, removes the FPS claim, uses identical image-free scenes when classic is enabled, and preserves classic's font handle. The classic adapter is still a benchmark adapter, not a complete production backend; the fixture uses a single static font.

`MUI_BENCH_SCENE=vectors` selects 96 curves, 50 cubic segments each (4,800 cubics). Its moving case changes every curve each frame. Default dimensions are 1280×800, with five warm-up frames and 50 measured frames per case. A cold case resets UI state, not the GPU device. GPU completion is waited for: results are headless completion latency, not presentation FPS or input latency. GPU timestamp measurements and visual parity are additional gates.

## Fresh reproduced results

Development desktop: Ryzen 7 7800X3D, Radeon RX 6600, RADV Vulkan, Linux 7.3.0-rc3. Other desktop applications were running. Three sequential vector runs, each with 50 measured frames after warm-up:

| Backend, all 4,800 cubics changing | Median of run medians (ms) | Individual run medians (ms) |
|---|---:|---|
| vello_cpu cached | 16.439 | 16.439, 16.362, 16.450 |
| vello_hybrid cached | 18.749 | 18.749, 18.749, 18.750 |
| vello (classic) | 1.030 | 1.030, 1.040, 1.009 |

Compute is about **18× faster** than cached Hybrid in this synthetic vector workload. Hybrid spends about 16 ms preparing/encoding paths; caching Bezier conversion alone does not remove that work. This supports investigating a compute or specialized graph path. Pixel-level parity is still outstanding; neither this timing nor the earlier 15× observation proves equal visual quality or end-to-end KURV improvement.

The default text-heavy fixture tells a different story: static cached Hybrid is 6.251 ms and classic 6.001 ms in one fresh run. Switching backends alone does not deliver a comparable improvement there. These are actual total-frame medians, not sums of phase medians.

The recovered benchmark passes strict Clippy. Raw vector, text, regression, and Clippy logs accompany this report. The completed rebuild required moving old task-owned binaries off the full shared build disk; no other project's data was deleted.

## Earlier provisional observations

These numbers were observed before loss of the temporary directory. They are hypotheses to reproduce, **not retained benchmark evidence or deployment results**:

- Dense animated vectors: Hybrid about 18.8 ms versus compute about 1.2 ms, suggesting roughly 15× potential for that workload.
- Fixing text identity allowed a tiled static scene to fall from about 29.2 ms to 2.9 ms. That measures repair of broken retention, not the production host.
- Reserved readout updates took about 0.0105 ms versus 0.799 ms for rebuilding the test UI, about 76× less CPU work. Rendering was excluded.
- Experimental glyph atlas use cut Hybrid encoding roughly in half, but total frame improvement was nearer 1.3× and cold cases regressed. The upstream API labels it experimental; it is not enabled by this branch.
- Tiled retention helped static/local changes but worsened all-changing vector scenes. A full-repaint fallback is necessary before using tiled rendering generally.

## Implementation order and acceptance gates

1. Land the text retention correctness fix and keep the corrected benchmark as the baseline.
2. Instrument actual KURV build/resolve/encode/GPU/present phases, event counts, changed area, allocations, uploads, and audio underruns. Record p50/p95/p99 and memory across long sessions.
3. Route reserved readouts through existing local updates. Separate structural/layout changes from paint-only and animation changes. Coalesce presentation work without dropping press/release, text input, automation, or accessibility changes.
4. Retain static backgrounds, labels, axes and control geometry. Update only changed regions; use full redraw when damage or layer overhead makes it cheaper. Budget retained GPU memory rather than allocating a texture per widget.
5. Benchmark specialized oscilloscope/spectrum rendering with persistent geometry or sample textures. Reduce display data to pixel resolution while preserving peaks; never alter DSP data to simplify a drawing.
6. Choose compute versus sparse-strip rendering from equal-content, equal-quality experiments. Check clipping, blending, welding, gradients, images, fallback fonts, scaling, resize, device loss and software fallback before migrating KURV.

Acceptance workloads: idle editor, one knob, many meters, dense spectra/scopes, scrolling, popup, preset change, resize, and multiple windows at scales 1/1.5/2. Run them on the agreed weak laptop with active audio, not only the development desktop. Suggested budgets of 1 ms CPU p95 and 4 ms GPU p95 are goals, not observed results; the 60 Hz frame deadline is 16.67 ms.

Amdahl's law matters: making 80% of a frame ten times faster improves the total only 3.57×. Tenfold overall improvement needs removing most of the frame's work. The promising strategy is local updates plus retained visuals plus a suitable dense-vector path.

## Reproduce

```sh
cargo test -p mui-scene --locked --offline
cargo run -p mui-vello --example bench --features cpu,bench-classic --profile perf --locked --offline
MUI_BENCH_SCENE=vectors cargo run -p mui-vello --example bench --features cpu,bench-classic --profile perf --locked --offline
```

## Implemented follow-up: adaptive tiled redraws

`TiledEffects` now renders once when most tiles are dirty and an extra full-window target fits the existing tile budget. Guarded copies refresh the persistent tiles; local changes retain the old path. This follows the investigation's full-redraw fallback recommendation. See [repeated measurements and validation](rendering-evidence/tile-adaptive-notes.md). It remains opt-in and does not change KURV's host backend.
