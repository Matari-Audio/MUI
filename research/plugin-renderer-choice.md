# Renderer choice for MUI plugins

Source audit, 2026-10-08. This changes the provisional classic-Vello choice in
[the GPUI integration proposal](gpui-vello-integration.md). It does not establish
comparative crash rates or measured frame times.

## Recommendation

Evaluate **GPUI core with `vello_gpu`** first when MUI's current welded materials
must remain. It keeps one Rust/wgpu graphics implementation and can reuse our
WGSL material pass. Evaluate **GPUI's native renderer** first for editors whose
visual requirements fit its primitives. **Skia raster** is the serious third
candidate: a successful CPU-first proof could remove GPU lifecycle entirely.

Do not keep classic compute Vello as the default without a measured benefit.
Upstream now puts it under `research/`, describes it as experimental, and
identifies `vello_gpu` (formerly Vello Hybrid) as its developing production GPU
direction. Vello CPU is currently its most mature renderer.
[Pinned upstream guidance](https://github.com/linebender/vello/blob/96d4643cca622259acad44e9e715e3e3fac3c09f/README.md).

| Candidate | Benefit for our plugins | Cost / adoption blocker |
| --- | --- | --- |
| **Vello GPU** | CPU path preprocessing, GPU raster/compositing, no compute requirement; same-device texture paints and caller-owned encoder reuse MUI materials. | GPUI paths/brushes/atlas still need a backend seam. Unsupported mask layers, complex filter graphs and some non-isolated blends panic. API and resource ownership are evolving. |
| **GPUI native** | Draws its scene directly; Windows D3D11, macOS Metal, Linux wgpu. Lowest scene-conversion cost for ordinary controls. | Our materials need three shader implementations or baked substitutes. No general portable material hook; Windows renderer is currently crate-private. Stock desktop platforms are unsuitable unchanged inside a DAW. |
| **Skia raster / GPU** | One Canvas and SkSL material API across CPU/GPU; rich built-in filters, blending and clipping. CPU-first removes GPU driver entry from drawing. | Port WGSL to SkSL; C++/FFI and packaging. CPU runtime shaders currently require SkSL 100. Native contexts/presentation still belong to us if using GPU. |
| **Vello CPU** | Already integrated, Rust, GPU-independent; useful reference and fallback. | Existing software mode rebuilds CPU welding instead of sampling GPU materials; complete parity and audio CPU budget need measurement. |
| **Classic Vello** | Upstream reports stronger raw performance for dynamic vector-heavy scenes. | Experimental compute pipeline; those workloads are not a demonstrated match for our editors. |

Sources: [Vello GPU limits/performance](https://github.com/linebender/vello/blob/96d4643cca622259acad44e9e715e3e3fac3c09f/vello_gpu/src/lib.rs#L54),
[renderer/texture API](https://github.com/linebender/vello/blob/96d4643cca622259acad44e9e715e3e3fac3c09f/vello_gpu/src/render/wgpu/mod.rs#L82),
[GPUI Windows devices](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_windows/src/directx_devices.rs#L103),
[Windows renderer visibility](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_windows/src/directx_renderer.rs#L39),
[SkSL effects](https://docs.skia.org/docs/user/sksl/),
[CPU shader restriction](https://github.com/google/skia/blob/3d7350b46987b0343a946fd0f92559cc85fd3576/src/shaders/SkRuntimeShader.cpp#L80),
[rust-skia shipping/backends](https://github.com/rust-skia/rust-skia/blob/594bb85bad45777ff5f7e284ba54c51346fb9e2e/README.md).

## What actually improves device coverage

Removing compute is a concrete reduction in renderer requirements. It does not
make every Intel/NVIDIA model compatible, eliminate wgpu/driver failures, or
automatically reduce the device limits our host requests. Vello GPU's example
uses empty features and downlevel WebGL2 limits, increasing only texture/buffer
dimensions to the adapter's limits. Use a similarly explicit minimum contract.
[Example](https://github.com/linebender/vello/blob/96d4643cca622259acad44e9e715e3e3fac3c09f/vello_gpu/examples/wgpu_webgl/src/lib.rs#L62).

Vello GPU's wgpu path still uses DX12/Vulkan/Metal/GL, not native D3D11. Skia's
rust-skia `d3d` backend is D3D12; D3D11 requires GLES through ANGLE, adding a
context and deployment dependency. GPUI's D3D11 path potentially covers older
Windows devices, subject to its capability checks.
[rust-skia backend definitions](https://github.com/rust-skia/rust-skia/blob/594bb85bad45777ff5f7e284ba54c51346fb9e2e/skia-safe/Cargo.toml),
[Skia ANGLE](https://skia.org/docs/user/special/angle/).

Our weld pass already writes an RGBA8 render attachment through the caller's
encoder (`crates/mui-vello/src/effects/pool.rs:319`). Vello GPU can sample it
as a texture paint on the same device: no cross-API texture imports or readback.
Its alpha-mask tint also fits GPUI monochrome atlas sprites. Glyph baking can
still submit internally; one device/queue does not promise one submit.
[Tint contract](https://github.com/linebender/vello/blob/96d4643cca622259acad44e9e715e3e3fac3c09f/vello_common/src/paint.rs#L251).

FemtoVG lacks custom shaders; Blend2D's roadmap still includes missing filters
and nonrectangular clipping. Neither clearly reduces material ownership here.
[FemtoVG](https://github.com/femtovg/femtovg/blob/d70ffeb79658606fe220801115ccefff33897e86/README.md),
[Blend2D roadmap](https://blend2d.com/roadmap.html).

## Proof before replacement

Use one representative editor with actual materials, glyphs, images, clips,
blur/shadows, parameter gestures, meters and high-DPI resize. Verify every used
primitive and alpha/color-space contract before adopting a renderer; never
silently discard unsupported features. Compare pixel parity, cold startup,
idle wakeups, active frame time, memory and audio underruns with several editors.
Measure GPU completion separately from CPU submit time.

Start with Vello GPU for material reuse and use existing Vello CPU as the
reference. A small Skia raster proof determines whether avoiding GPU drawing
altogether is practical; only add its GPU path if CPU misses the budget. All
candidates still need two independently loaded plugins, simultaneous editors,
close/reopen and unload with pending callbacks. Keep the GPUI UI ownership and
deletion ledger the same whichever renderer wins; do not ship several renderer
architectures or preserve old host APIs as a compatibility layer.

Hosted evidence so far: Linux embedded pixels/accessibility/reopen and two-window
resize/close pass on GL and Vulkan software drivers. The gallery was blocked
by a missing runner dependency, now fixed. Windows WARP/DX12 faults after its
first submitted frame; Metal selected an adapter but the tester waited for an
OS-clamped size. Neither is affected-user Intel/NVIDIA proof. The revised test
fits monitor sizes and honors synchronous native resize results; WER dumps are
enabled for the hosted Windows child. Those revisions await hosted verification.

No renderer migration or hardware benchmark is implemented. Local Rust
validation remains blocked at the user's request while other jobs continue.
