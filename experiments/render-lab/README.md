# MUI / GPUI / Vello render lab

This is an **experimental rendering comparison**, isolated from the MUI workspace. It does
not switch MUI's backend or claim a complete GPUI/Vello plugin framework integration.

- GPUI / gpui_wgpu: Zed revision `7960b2a7c9568e90fbe0727332149e5b2a5fd57a`.
- Vello: `0.10.0`; shared wgpu dependency `29.0.4`.
- Rust toolchain: repository pin `1.98.1`; optimized release builds.
- Dependencies locked in this directory's Cargo.lock; upstream source downloaded separately.
- No physical GPU was available in the recorded environment. Captures and shaders executed on
  Mesa llvmpipe software Vulkan, LLVM 20.1.2, Mesa 25.2.8, `LP_NUM_THREADS=4`.

## Run

Linux with an X11 display (or Xvfb), Vulkan driver, Rust, Python, Pillow, numpy and CairoSVG:

```sh
cd experiments/render-lab
./tools/run.sh
```

`prepare.py` fetches the pinned Zed archive into ignored `experiments/upstream`, preserving
upstream licensing. It adds a diagnostic GPUI surface readback hook. GPUI's drawing code,
shader code, blend state and sample-count selection are unchanged. The only surface change
is adding COPY_SRC usage. Captures execute outside timing samples. The script and its exact
instrumentation are checked in for review.

The `image` dependency uses one release codegen unit to work around recurring zero-length
archive-object failures with this environment's compiler/filesystem. Image decoding is not
inside rendering timing samples.

Native GPU runs must record the actual adapter and driver; never interpret the software
numbers as native GPU predictions. X11 and Vulkan are explicit requirements of this prototype;
it does not yet contain macOS/Windows/browser runners. A current compositor or DAW can
introduce different presentation and scheduling behavior.

Individual commands:

```sh
./target/release/mui-render-lab geometry 1 30 output
./target/release/mui-render-lab gpui 1 60 output
./target/release/mui-render-lab gpui-tight 1 60 output
./target/release/mui-render-lab vello-area 1 60 output
./target/release/mui-render-lab vello-msaa8 1 60 output
./target/release/mui-render-lab vello 1 60 output
./target/release/mui-render-lab vello-area-present 1 60 output
./target/release/mui-render-lab vello-present 1 60 output
./target/release/mui-render-lab gpui 1 60 output/stress 8
./target/release/mui-render-lab embedding 1 30 output
./target/release/mui-render-lab glass output/gpui-1.png output/glass-gpui.png
python3 tools/quality.py output
python3 tools/publish_results.py
```

Arguments: backend, scale (0.5–4), sample count, output directory, optional tile side (1–8).
Use separate output directories for different tile counts. `vello` is MSAA16. Each backend
constructs its native scene from the SAME serialized cubic paths and colors. Shared gradients
explicitly request sRGB interpolation. Native GPUI quads handle the rectangular test patches;
arbitrary shapes use GPUI PathBuilder with explicit nonzero fill. Standard path tolerance is
0.1 logical pixels; `gpui-tight` uses 0.01 / scale. Vello receives the cubics directly. MUI arcs
are approximated at 0.001 logical units before either backend receives them.

## Measurements and limits

- **Geometry:** 1,200 seeded width/height/gap/radius cases; actual MUI build/resolve, finite
  path validation, flattening, extension target coincidence and preserved logical tap bounds.
  This is a focused property sweep, not exhaustive Boolean-geometry fuzzing.
- **Quality:** exact solid/alpha interiors, analytic sRGB ramp, holes, concavity, merged shape,
  clipping, rotations and 0.25/0.5/1/1.5 logical-pixel strokes at 1x/1.5x/2x.
- **AA oracle:** independent Cairo at 4x resolution, box downsampling. Shared cubic geometry
  means this checks rasterization, not the mathematical correctness of MUI Boolean operations.
  Cairo is a reference, not perfect ground truth. Reports separate regions so gradient errors
  cannot masquerade as general AA errors. Quarter-pixel continuity samples 4,096 points on
  the true circle centerline with bilinear image sampling; weak means normalized coverage
  below 0.025. That is a documented diagnostic threshold, not an accessibility standard.
- **Performance:** scene encoding is timed separately. Drawing uses a prebuilt scene, five
  warmup frames then 60 serialized draw+device-wait samples. GPUI includes surface acquisition
  and presentation. Vello `*-present` includes acquisition, drawing, texture blit, presentation
  and device wait; other Vello modes are offscreen and must not be ranked directly against
  GPUI's presentation measurements. These are wall-clock service times, not GPU timestamps,
  interactive latency or application FPS. CPU contention/driver caches are uncontrolled;
  first-draw values are not guaranteed shader-cache-cold measurements.
- **Stress:** 8x8 tiles = 2,560 marks at the same output resolution, emphasizing many small
  paths and subpixel strokes. Not a realistic whole-plugin workload by itself.
- **Determinism:** exact RGBA comparison across three process runs for GPUI, Vello area-present
  and Vello MSAA16-present at 1x.
- **Embedding:** two winit X11 children of a synthetic host; query actual parent XIDs, share
  GPUI GPU context, close one, keep drawing sibling, resize/draw three times. This tests
  gpui_wgpu + raw handles. It does NOT test GPUI App's runtime inside a DAW, automation,
  focus/IME, host event-loop ownership, CLAP/VST3 lifecycle or non-X11 platforms.
- **Glass:** same WGSL blur/refraction/mask/highlight pass on each backend's captured image.
  PNG upload and readback excluded from shader timing. This is a postprocess experiment,
  not Apple Liquid Glass, a live backdrop graph or a GPUI custom-element hook.
- **Not measured:** native GPU speed, peak GPU memory, power use, full UI-tree invalidation,
  text rasterization/IME, plugin audio-thread isolation, browser support, real DAW stability.
  The combined benchmark binary size does not establish either backend's production size.

See [recorded findings](../../docs/render-lab/RESULTS.md) and the checked-in metrics/captures.
