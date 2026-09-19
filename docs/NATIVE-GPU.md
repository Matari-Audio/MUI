# Native GPU welding and retained Hybrid encoding

19 September 2026. Integration base: `a9cc11d232986fce8dfb1a12aa20afd7dcbb664e`.

**Status: implementation authored, not Rust-build-validated.** No native WGSL
execution, Vello capture, physical-device benchmark or DAW test ran in the
bundle-authoring environment. It lacked Rust tools, network downloads failed,
and browser navigation policy prevented a secure WebGPU test page. A native
conformance binary and a Naga validation test are supplied; their existence is
not a passing test result.

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
                                 |
                         one queue submission
```

`Ui::gpu_welding()` selects the analytic backend for new material welds.
`.gpu_weld(options)` and `.reference_weld(options)` are explicit per-node
choices. The legacy `.weld(fill)` operation is unchanged. No renderer is swapped
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
  texture domain, a 352-byte uniform ABI, allocation-free changed-lane iterator,
  CPU point containment using the same smooth-min policy.
- `mui-scene`: explicit execution backend; GPU lowering bypasses `WeldCache`
  and CPU rasterization; an external paint marker preserves z order, surrounding
  clips, group opacity and authored child identity. Morph/material setters leave
  layout and the ordinary paint list unchanged.
- `mui-input`: analytic containment override after normal broad rejection and
  exact ancestor clips. It reads the scene's current material rather than a
  stale copied hit mask. Original child plates remain separate interactive nodes;
  the group can own its connecting region. Opacity is not geometric hit policy.
- `mui-vello::effects::WeldTextures`: persistent texture/uniform/bind-group
  slots; changed parameter uploads; 64-pixel capacity buckets; budgets by logical
  texel bytes and visible count; unused-resource eviction; commit after queue
  submission and explicit abort. Low-level callers must not mutate an effect
  twice in one submission or call commit before submission.
- `HybridEffects`: a real Hybrid renderer and resources, not a parallel overlay
  renderer. An exactly unchanged paint list and transform reuse the already
  encoded Hybrid scene, skipping its CPU strip preparation. The material pass
  precedes sampling in the same encoder. Optional overlays invalidate retention.
- Optional four-slot asynchronous GPU timestamps. Unsupported devices report
  unsupported; a full telemetry ring drops a sample instead of blocking rendering.
- Feature-selected native gallery host plus `gpu_welding` windowed example.
  Resize preserves pipelines. Surface loss attempts surface recreation; full
  device loss still requires reconstructing the host/renderer and its resources.
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
Its effect texture contains premultiplied sRGB-encoded values. The separate
classic shader entry produces straight-alpha sRGB RGBA8 for classic texture
registration, including its required atlas-copy path. Do not exchange these two
contracts or assume an sRGB attachment requires no further changes.

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

## Hybrid/classic measurement

The separate `gpu_matrix` example measures an ISOLATED analytic-effect fixture
with the same background, clip, opacity and foreground in each backend. It is
not the old whole-editor fixture and not a DAW or presentation benchmark.
Only the selected backend creates renderer resources. Classic includes its
texture-registration/dirty-atlas-copy route. Both use the exact WGSL file, with
separate output-alpha entry points.

```sh
# Requires Pillow for strict paired-image checks.
python tools/native-gpu/run_matrix.py ./gpu-results --frames 600 --repetitions 3
```

This runs a device contract first, then static/morph/geometry/resize at
1x/1.5x/2x, alternates backend order, writes raw CSVs, adapter metadata and PNGs,
and rejects paired images beyond an explicit 3/255 channel threshold. A mismatch
stops the report; do not silently loosen the threshold to pick a winner. Shader
and edge equivalence must be reviewed before interpreting performance.

The `perf` Cargo profile inherits release with opt-level=3; the original
size-optimized release profile remains unchanged. Compare profiles using
`--profile release`, into a new output directory.

Reports use nearest-rank p50/p95/p99 and retain missing timestamp samples.
`cpu_submit_ms` excludes the bounded three-in-flight admission wait.
`gpu_queue_interval_ms` brackets queue commands; classic's multi-submit route
can include intervening idle gaps. Neither metric is presented latency/FPS.
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
