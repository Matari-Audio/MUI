# Native GPU welding and the retained GPU renderer

19 September 2026. Integration base: `a9cc11d232986fce8dfb1a12aa20afd7dcbb664e`.

11 November 2026: the GPU path is the instanced SDF quad pass
(`mui_vello::quad`, feature `gpu-quad`) on wgpu 30, behind the one retained
renderer, `effects::GpuRenderer`: no compute shaders, no storage buffers.
Classic Vello (compute shaders) is deleted, with `effects::VelloRenderer` and
the `vello-classic` feature; the last head-to-head numbers are in
`BENCHMARKS.md`.

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
                        +-- ordinary paint ---------> quad pass instances
                        |
                        +-- ExternalWeld (local analytic spec)
                                 |
                       changed 16-byte uniform lanes
                                 |
                        native WGSL fragment pass
                                 |
                        persistent RGBA8 texture
                                 |
              sampled in place by the quad pass, in its z slot
```

Queue submissions per frame: `GpuRenderer::render` submits the material
passes that changed, the quad pass into its retained frame texture (only the
box of what changed, when entries changed in place) and the present pass. An
unchanged frame into the view presented last submits nothing; into a new
view (a swapchain) it is one present pass of the texture already holding
the frame. Nothing is read back to the CPU. A frame the pass refuses (see
`quad::Reason`: vector paints, colour or bitmap glyphs, rotated text, a path
clip inside a path clip, over-budget frames) is rasterised by `vello_cpu`,
uploaded into the same frame texture and presented the same way;
`GpuRenderer::fallback` counts it, and the next changed list is tried on the
GPU again. A refused frame that also holds weld material is an error, and the
host falls back to its CPU presenter. `render_with_overlay` (debug overlays)
draws its frame on the CPU too.

`Ui::gpu_welding()` selects the analytic backend for material welds; without
a `Ui`, `SceneSpec::weld_backend(WeldBackend::AnalyticGpu)` does. The backend
is the host's choice for the whole scene, not a per-node one. The vector `.union(fill)` outline is a separate operation. No renderer is swapped
on a frame-by-frame basis.

```rust,ignore
let mut ui = Ui::new(theme).gpu_welding();
let plates = row![a, b]
    .gap(8.)
    .weld(Weld::all().reach(24.).blend(72.).morph(progress))
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
  `external.rs` calls `cache.analytic` to share the analytic material. An external paint marker preserves z order, surrounding
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
- `GpuRenderer`: the quad pass on the host's device. An exactly unchanged
  paint list and transform submit nothing; entries changed in place redraw
  one device box (reported by `damage()`); a list whose length changed, or a
  new transform, redraws whole. Material passes are submitted before the quad
  pass samples them.
- `GpuTimer`: an optional, standalone four-slot asynchronous timestamp ring.
- The native gallery host (`mui-preview`) plus the `gpu_welding` windowed example.
  Resize preserves pipelines. Surface loss recreates the surface. Device loss
  rebuilds the device and the renderer (see below).

## Images and the per-renderer `Cache`

Converted images and fonts live in a `mui_vello::Cache` that belongs to the
renderer. There are no process-wide globals. `GpuRenderer` owns its cache;
direct `Cpu` callers pass one in. Images are keyed by `Weak<[u8]>` on the
app's pixel buffer, so the cache never keeps a buffer alive. On each image
lookup, entries whose buffer the app has dropped are swept. A host texture
registered with `GpuRenderer::set_texture` is sampled in place.

## Device loss (host contract)

A wgpu device loss kills everything created from that device: pipelines, weld
textures, atlas contents and ids, and the retained encoding. `mui-preview`'s
`host.rs` is the reference host. It does the following:

1. It registers `Device::set_device_lost_callback`. The callback only sets an
   `AtomicBool`, because wgpu may call it on any thread.
2. At the start of the next `present`, if the flag is set, it requests a new
   adapter and device for the same surface. It then reconfigures the surface
   and builds a new `GpuRenderer`, which brings a fresh
   `Cache`. That frame is skipped and a redraw is requested.
3. Nothing from the old device is carried over: no `Cache`, no `WeldTextures`
   and no texture ids. The scene is plain CPU data and needs no change.

Other hosts must follow the same steps. The path has not yet been exercised by
a real device loss, only by `device::tests::a_destroyed_device_is_replaced_and_renders_again`.
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
and geometric morph, all axis-aligned: the shader keeps a rotation term, but no
public constructor sets one, so every analytic source is unrotated.

General arbitrary contours, image brushes, nested welds, shadow/shell effect
stacks, arbitrary GPU visualization payloads and backdrop refraction are NOT
implemented by this GPU path. A weld cannot itself clip descendants to its
changing union; use an ordinary ancestor clipping viewport. Unsupported
combinations fail before the CPU fallback rather than pretending to be correct.
Border bands near a smooth join are approximate field offsets, not certified
Euclidean parallel offsets.

Whole-group translation can reuse local texture contents, but changes the
placement. Morph-only updates reuse layout and texture allocation, and redraw
only the weld's box. A normal `Ui::frame` STILL walks/resolves the immediate tree.
There is no automatic fine-grained reactive dependency graph, subtree layout
retention, arbitrary-path distance-tile cache or generic static-layer atlas here.
The retained cache stores an exact copy of one paint list; it is not a bounded
history of every scene ever shown.

The high-level renderer supports non-sRGB RGBA8/BGRA8 UNORM output; its frame
is premultiplied sRGB-encoded RGBA8, presented premultiplied or, for a
`PostMultiplied` surface, straight.
Its effect texture contains premultiplied sRGB-encoded values, written by the
single `fs_hybrid` entry point. Do not assume an sRGB attachment works
without further changes.

The native window host is winit, NOT a completed CLAP/VST3/AU parent-window host.
No downstream plugin source or real-time audio transport was changed. Parley
restoration from the initial audit remains separate unfinished work.

## Run the native checks

First apply the cumulative bundle to a clean checkout of the pinned revision,
then format once. Formatting is not evidence of type correctness.

```sh
cargo fmt --all
rustup target add wasm32-unknown-unknown
cargo fetch --locked
bash tools/native-gpu/verify.sh
cargo run --locked --profile perf -p mui-preview --example gpu_welding
```

`gpu_welding`: Space toggles animation, arrows change morph, 1/2/3 change the
welding channel policy, R resets, mouse movement repositions the second source.
The title reports resolves, upload bytes, effect draws and scene encodes, not
unvalidated FPS. The example intentionally isolates morph-only updates from full
input/geometry rebuilds.

`gpu_contract` is a real device test, not an ignored unit test. It checks static
pixel identity and zero extra effect work; a 16-byte morph update that redraws one
damage box (one quad frame, no path or glyph data rebuilt, no allocation); no layout or
paint-list change; visible pixel change; later floating-content order; a clipping
sample; and native validation errors at 1x/1.5x/2x. Failure to obtain an adapter
fails the command. Software adapters may be used for correctness, but are named.
Its current pixel assertions are targeted, not exhaustive CPU/GPU parity.

## Measurement

The whole-editor comparison is the `bench` example
(`cargo run -p mui-vello --profile perf --features gpu-quad --example bench`,
`vello_cpu` and `GpuRenderer` rows) and `quad_bench` (`--features gpu-quad`):
the bare quad pass and the wired `GpuRenderer` on the same paint lists. See
`BENCHMARKS.md` for numbers; they are CPU-side and GPU-span timings on one
RX 6600 (RADV/Vulkan), not presented latency, and power, thermals, real
audio underruns and integrated GPUs remain unmeasured.

## Upstream contracts checked

- https://github.com/gfx-rs/wgpu/tree/v30.0.1/wgpu/src/api

`vello_cpu`/`vello_common` 0.3 and glifo 0.4 rasterise glyph masks and the
CPU frames.
CPU PNG glyph decoding is enabled for bitmap-font parity; fallible glyph draws
propagate through the additive `Canvas::try_glyphs` path.
Cargo --locked remains authoritative; do not invent or
refresh registry checksums.


## Custom host startup and transparency

A custom native host first opens `mui_vello::software::Window::new_startup`
and presents a completed CPU scene. Only after that successful present create
`GpuInit::new_with_transparency(pin_current_image_for_detached_work(), want)`.
The opaque compatibility constructor remains `GpuInit::new(pinned)`.

Keep the initializer while the window is open, including CPU recovery. Poll
with the current physical size and a surface factory that owns that window's
handle/display. `None` means the job is pending, already installed, minimized,
or backing off; `Some(Err(error))` records a failure and its retry class;
`Some(Ok(host))` promotes the window and requires a full redraw plus recreation
of any device-bound texture producers. `deadline()` describes retry backoff;
`pending()` allows the native scheduler to poll active initialization.

The surface factory runs on the owning GUI thread only after the shared
device and per-window resources are ready. Drop the CPU presenter immediately
before creating the GPU surface (especially before attaching CAMetalLayer).
On an error after this handover, recreate the CPU presenter. Finish CPU startup
quality when a permanent GPU failure selects software-only operation.

Translucent Windows sessions select `DxgiFromVisual` before instance creation;
HWND opaque sessions keep their ordinary swapchain. Their devices, weld
pipeline and pipeline cache are shared within each compatible presentation
mode; each window builds its own quad and present pipelines. Other platforms
share one context across alpha modes and select alpha per surface. Actual
capability remains available through `Host::translucent()`, including opaque
fallback where alpha is unavailable. Native OS blur remains the host's policy.

On device loss, drop the Host before destroying its native handle, invalidate
all device-bound image bindings, call `init.recover(error.failure_class())`,
reopen CPU presentation and redraw. Shared sessions adopt a new generation;
`Host::generation()` plus device ownership identifies each promoted producer.
Do not spin on permanent failures. A caller whose image pin failed receives a
private synchronous session and cannot inherit another editor's detached-work
permission. No close joins a detached initializer; its code must remain pinned.

Custom hosts must keep native ownership alive through surface creation and
configuration in `poll`: pinning the containing image does not preserve a
native drawable. Baseview's guarded callback lease and handler-before-native
teardown provide this boundary for its own windows. Keep native close separate
from detached initialization: workers own only GPU resources and result senders.
The optional `test-support` feature exposes a hidden `InitGate` that holds actual
per-window result publication; integration fixtures can close the editor, release
the late result and verify it was dropped without ever creating a surface.
