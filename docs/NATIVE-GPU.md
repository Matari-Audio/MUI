# Native GPU welding and retained Hybrid encoding

19 September 2026. Integration base: `a9cc11d232986fce8dfb1a12aa20afd7dcbb664e`.

**Status: builds, passes its tests, and runs on a physical device.** The
workspace builds and its test suite passes (including the Naga validation test
and the `effects_smoke` device test). The `bench` example has been run on an
AMD RX 6600 (RADV/Vulkan). No DAW or plugin-host test has been run.

This is cumulative with the earlier audit-hardening and CPU material-welding
patches. The CPU baker remains an explicit reference/snapshot path. Native
animation must not silently switch back to that per-pixel CPU implementation.

## Native data flow

```
El tree -> layout -> paint-ordered scene
                        |                   
                        +-- ordinary paint ---------> retained Vello scene
                        |
                        +-- ExternalWeld (local analytic spec)
                                 |
                       changed 16-byte uniform lanes
                                 |
                        native WGSL fragment pass
                                 |
                        persistent RGBA8 texture
                                 |
                   Hybrid external texture sample in its z slot
```

Queue submissions per frame, as the code does them today:

- `HybridEffects`: one encoder holds the material passes and the Vello render.
  It is submitted once per frame.
- `Gpu::image`: an image upload or atlas free submits its own small encoder at
  the moment the image is painted. A frame that uploads new images therefore
  submits more than once.
- `TiledEffects`: one encoder for the material passes, one per dirty tile, and
  one for the final blit to the target. That is N+2 submissions for N dirty
  tiles, plus any image uploads.

`Ui::gpu_welding()` selects the analytic backend for new material welds.
`.gpu_weld(options)` and `.reference_weld(options)` are explicit per-node
choices. The vector `.union(fill)` outline is a separate operation. No renderer is swapped
on a frame-by-frame basis.

```rust,ignore
let mut ui = Ui::new(theme).gpu_welding();
let plates = row![a, b]
    .gap(8.)
    .weld_with(Weld::all().reach(24.).blend(72.).morph(progress))
    .id("joined-plates");
// Initial resolve with the real host's size/scale/input...

// A later morph-only tick does not build or resolve another El tree:
ui.set_weld_morph("joined-plates", progress)?;
renderer.render(ui.scene().unwrap(), transform, target_view)?;
```

The public `.set_weld_solid_material` and `.set_weld_material_blend` likewise
update only material data. Keep the application declaration synchronized: a
later complete resolve intentionally reapplies the authored tree. These setters
do not recompute child-label contrast or arbitrary dependent application state;
refresh those dependent values explicitly when needed.

## What is implemented

- `mui-weld::analytic`: validated local analytic sources, stable maximum-reach
  texture domain, a 336-byte uniform ABI (21 `vec4<f32>`), allocation-free changed-lane iterator,
  CPU point containment using the same smooth-min policy.
- `mui-scene`: explicit execution backend. GPU lowering skips CPU
  rasterization, but it still goes through `WeldCache`: `finish_gpu` in
  `external.rs` calls `cache.get_analytic` to reuse the analytic material. An external paint marker preserves z order, surrounding
  clips, group opacity and authored child identity. Morph/material setters leave
  layout and the ordinary paint list unchanged.
- `mui-input`: analytic containment override after normal broad rejection and
  exact ancestor clips. It reads the scene's current material rather than a
  stale copied hit mask. Original child plates remain separate interactive nodes;
  the group can own its connecting region. Opacity is not geometric hit policy.
- `mui-vello::effects::WeldTextures`: persistent texture/uniform/bind-group
  slots; changed parameter uploads; 64-pixel capacity buckets; budgets by logical
  texel bytes and visible count. Textures stay resident after their weld leaves
  the viewport. When a new frame goes over budget, `begin` evicts idle slots,
  least recently used first. A weld that scrolls back into view therefore
  re-renders nothing. Commit happens after queue submission, and aborts are
  explicit. Low-level callers must not mutate an effect
  twice in one submission or call commit before submission.
- `HybridEffects`: a real Hybrid renderer and resources, not a parallel overlay
  renderer. An exactly unchanged paint list and transform reuse the already
  encoded Hybrid scene, skipping its CPU strip preparation. The material pass
  precedes sampling in the same encoder. Optional overlays invalidate retention.
  Retention helps only when the paint list really is identical. In the `bench`
  editor something changes every frame, so it re-encodes all 55 of 55 frames.
- `TiledEffects`: damage-tracked tiles. Welds whose bounds miss the viewport
  are culled before `begin`, so they are never admitted or rendered. An idle
  frame plans and commits its damage without allocating
  (`tests/idle_alloc.rs`).
- `GpuTimer`: an optional, standalone four-slot asynchronous timestamp ring,
  used by `gpu_matrix`. `HybridEffects` no longer carries a profiler.
- Feature-selected native gallery host plus `gpu_welding` windowed example.
  Resize preserves pipelines. Surface loss recreates the surface. Device loss
  rebuilds the device and the renderer (see below).

## Images and the per-renderer `Cache`

Decoded pixmaps, atlas ids and fonts live in a `mui_vello::Cache` that belongs
to the renderer. There are no process-wide globals. `HybridEffects` and
`TiledEffects` own their cache; direct `Gpu`/`Cpu` callers pass one in.
Images are keyed by `Weak<[u8]>` on the app's pixel buffer, so the cache never
keeps a buffer alive. On each image lookup, entries whose buffer the app has
dropped are swept: their pixmap is freed and their atlas slot is released.
Fonts are kept for the renderer's lifetime.

When the image atlas is full, the image draws as its solid stand-in colour
instead of panicking. The cache tracks atlas occupancy with a mirror
`vello_common` allocator. This mirror is exact only with the default
`vello_hybrid::Renderer::new` atlas settings and with the glyph atlas off.

## Device loss (host contract)

A wgpu device loss kills everything created from that device: pipelines, weld
textures, atlas contents and ids, and the retained encoding. `mui-preview`'s
`host_gpu.rs` is the reference host. It does the following:

1. It registers `Device::set_device_lost_callback`. The callback only sets an
   `AtomicBool`, because wgpu may call it on any thread.
2. At the start of the next `present`, if the flag is set, it requests a new
   adapter and device for the same surface. It then reconfigures the surface
   and builds a new `HybridEffects` or `TiledEffects`, which brings a fresh
   `Cache`. That frame is skipped and a redraw is requested.
3. Nothing from the old device is carried over: no `Cache`, no `WeldTextures`
   and no texture ids. The scene is plain CPU data and needs no change.

Other hosts must follow the same steps. The path has not yet been exercised by
a real device loss. The plain `host.rs` gallery host (Hybrid without effects) does not recover yet.
- Deadline-based tooltip/caret wakeups, true elapsed wall-clock time, one
  catch-up frame for an immediate-mode caret edge, and hidden-window gating.
- Allocation-free waveform extrema and spectrum peak reducers over existing
  UI-side snapshots. They are not audio-thread transport or an audio resampler.

The default effect budget is 128 surfaces, 64 MiB logical texture storage and
4M pixels per effect; node quality budgets can be stricter. These are admission
limits, not a physical VRAM measurement: driver overhead and resources awaiting
GPU completion are additional. Large allocations fail explicitly rather than
silently lowering quality.

## Exact scope and remaining work

The native shader supports one to three rounded rectangles, solid or two-stop
linear fills, solid borders, different border widths, Blend/Keep/Omit channels
and geometric morph. The low-level analytic source supports rigid rotation; the
current in-tree source adapter lowers ordinary axis-aligned rounded plates.

General arbitrary contours, image brushes, nested welds, shadow/shell effect
stacks, arbitrary GPU visualization payloads and backdrop refraction are NOT
implemented by this GPU path. A weld cannot itself clip descendants to its
changing union; use an ordinary ancestor clipping viewport. Unsupported
combinations fail before the CPU fallback rather than pretending to be correct.
Border bands near a smooth join are approximate field offsets, not certified
Euclidean parallel offsets.

Whole-group translation can reuse local texture contents, but changes the
placement encoding. Morph-only updates reuse layout, texture allocation and
Hybrid encoding. A normal `Ui::frame` STILL walks/resolves the immediate tree.
There is no automatic fine-grained reactive dependency graph, subtree layout
retention, arbitrary-path distance-tile cache or generic static-layer atlas here.
The retained cache stores an exact copy of one paint list; it is not a bounded
history of every scene ever shown.

The high-level renderer currently supports non-sRGB RGBA8/BGRA8 UNORM output.
Its effect texture contains premultiplied sRGB-encoded values, written by the
single `fs_hybrid` entry point. The classic straight-alpha entry is gone. Do not
assume an sRGB attachment works without further changes.

The native window host is winit, NOT a completed CLAP/VST3/AU parent-window host.
No KURV or BUFFR product source or real-time audio transport was changed. Parley
restoration from the initial audit remains separate unfinished work.

## Run the native checks

First apply the cumulative bundle to a clean checkout of the pinned revision,
then format once. Formatting is not evidence of type correctness.

```sh
cargo fmt --all
rustup target add wasm32-unknown-unknown
cargo fetch --locked
bash tools/native-gpu/verify.sh
cargo run --locked --profile perf -p mui-preview --features gpu-effects --example gpu_welding
```

`gpu_welding`: Space toggles animation, arrows change morph, 1/2/3 change the
welding channel policy, R resets, mouse movement repositions the second source.
The title reports resolves, upload bytes, effect draws and Hybrid encodes, not
unvalidated FPS. The example intentionally isolates morph-only updates from full
input/geometry rebuilds.

`gpu_contract` is a real device test, not an ignored unit test. It checks static
pixel identity and zero extra effect work; a 16-byte morph update; no layout or
paint-list change; visible pixel change; later floating-content order; a clipping
sample; and native validation errors at 1x/1.5x/2x. Failure to obtain an adapter
fails the command. Software adapters may be used for correctness, but are named.
Its current pixel assertions are targeted, not exhaustive CPU/GPU parity.

## Hybrid measurement

The separate `gpu_matrix` example measures an ISOLATED analytic-effect fixture
on the Hybrid path: background, clip, opacity and foreground around one weld.
It is not the whole-editor fixture, and it is not a DAW or presentation
benchmark. The classic backend has been removed.

```sh
python tools/native-gpu/run_matrix.py ./gpu-results --frames 600 --repetitions 3
```

This runs a device contract first, then static/morph/geometry/resize at
1x/1.5x/2x, and writes raw CSVs, adapter metadata and PNGs.

The whole-editor comparison is the `bench` example:
`cargo run -p mui-vello --profile perf --features cpu,gpu-effects --example bench`.
It includes `hybrid retained` (HybridEffects) and `tiles` (TiledEffects) rows.
On an RX 6600, median frame times were:

| case | HybridEffects | TiledEffects, per tile | TiledEffects, adaptive |
|---|---|---|---|
| static | 5.9 ms | 4.0 ms | 4.3 ms |
| one knob turning | 5.8 ms | 4.2 ms | 4.5 ms |
| cold | 11.4 ms | 16.0 ms | 12.0 ms |

On warm frames with partial damage, tiles save about 1.6 to 1.9 ms.

The `perf` Cargo profile inherits release with opt-level=3; the original
size-optimized release profile remains unchanged. Compare profiles using
`--profile release`, into a new output directory.

Reports use nearest-rank p50/p95/p99 and retain missing timestamp samples.
`cpu_submit_ms` excludes the bounded three-in-flight admission wait.
`gpu_queue_interval_ms` brackets queue commands. Neither metric is presented latency/FPS.
Readback and final draining occur only in diagnostic binaries, outside timed
samples, not in production rendering. Window scheduling, input-to-display,
power/thermals, real audio underruns and multiple plugin instances remain
unmeasured. The timer ring may drop samples; reports show the missing count.

## Upstream contracts checked

- https://docs.rs/vello_hybrid/0.2.0/vello_hybrid/struct.Scene.html
- https://docs.rs/vello_hybrid/0.2.0/vello_hybrid/struct.TextureBindings.html
- https://docs.rs/vello/0.10.0/vello/struct.Renderer.html
- https://docs.rs/vello/0.10.0/vello/struct.Scene.html
- https://github.com/gfx-rs/wgpu/tree/v29.0.3/wgpu/src/api

Published crate names remain `vello_hybrid = 0.2`, `vello = 0.10`, `wgpu = 29`.
The lockfile patch reuses the existing Naga29 pin and changes only workspace
edges. Cargo --locked remains authoritative; the applicator does not invent or
refresh registry checksums.
