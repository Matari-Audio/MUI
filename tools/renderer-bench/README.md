# Renderer measurements

Run `.github/workflows/renderer-bench.yml` on free Ubuntu runners. No production
backend dependencies change. Each renderer/workload/scale starts in a fresh
process; the artifact preserves the exact source SHA, dependency lockfile,
adapter inventory, binary, first image, raw samples and native child status.

The common fixture has 40 control panels and either 8 or 96 polylines of 128
points, at 1280×800 and 2560×1600. Engines render identical coordinates/colors
against opaque black. First-frame checks require a magenta sentinel, black
corner, opaque pixels and visible waveforms. Animated frames must change pixels.
Five warmups precede 30 samples each for static and dynamic full redraws.

Timing includes scene encoding, completed rasterization and CPU RGBA readback
for **every** engine, including CPUs. It excludes fixture generation, PNG
encoding and validation. Readback is required because GPUI's public headless
completion API includes it. These numbers do not isolate GPU execution time.
Process CPU time includes worker and software driver threads; peak RSS includes
the process and its driver allocations, not total GPU memory. The 100 ms idle
sample measures headless workers, not a host/window event loop.

GPUI uses its wgpu headless renderer. It does not measure its native Windows
D3D11 or macOS Metal renderer. Lavapipe results represent software Vulkan,
not consumer NVIDIA/Intel performance. Text, gradients, images, clipping,
filters, custom materials, embedded-window lifecycle, DAW audio interference
and visual parity across those features remain separate proof requirements.

The second job reuses MUI's existing `mui-vello/examples/bench.rs` full editor
benchmark, including text, images, layout, scene resolution, retained idle
frames and vector-heavy updates. Its completed-render timings **exclude
readback** and are a pipeline baseline, not directly comparable to the fixture
rows. Its `cold` case recreates UI state, not the GPU device; startup is printed
separately. No renderer migration should be decided from software-only rows.

To reproduce on real hardware after a CI build, use the artifact binary and
run `mui-renderer-bench classic controls 1 results`. Repeat for `vello-gpu`,
`vello-cpu`, `skia-cpu`, and the separately built `gpui` binary. Preserve the
actual selected adapter from stderr and record concurrent machine load.
