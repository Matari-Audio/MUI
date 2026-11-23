# mui-hal: owned GPU HAL (Wave 3A)

Status: in progress. Plan: `docs/STACK-OWNERSHIP-PLAN.md` Wave 3A.

## Goal

Shipped plugin binaries link no wgpu. Native plugin windows render the
`mui-vello` quad pass and `effects::GpuRenderer` through `mui-hal`, a thin
render-only HAL shaped after the subset of wgpu that `mui-vello` uses.
Web/wasm, `mui-stage`/`mui-cut` and BUFFR keep wgpu through the HAL's `wgpu`
backend until they migrate.

## Shape

- One concrete API, wgpu-subset names and descriptor shapes, so `mui-vello`
  ports by path (`wgpu::X` becomes `mui_hal::X`). Each handle is a cheap
  clone and dispatches over backends with an enum. No traits, no generics
  reaching callers.
- Backends:
  - `vulkan` (Linux first, Windows next): `ash`, Vulkan 1.3 loader and
    device required (dynamic rendering and timeline
    semaphores in core), SPIR-V from `mui-shaders`, presentation through
    `mui-present`'s `VkSwapchain`.
  - `metal` (macOS, `objc2-metal`, MSL from `mui-shaders`): planned.
  - `d3d11` (Windows, `windows-sys`, DXBC from `mui-shaders`): planned.
  - `wgpu` (opt-in feature): passthrough for web, media and BUFFR. It also
    covers macOS and Windows plugin builds until their native backends land.
- Shaders are named by `mui_shaders::ShaderId`, never by source text.
- Render only: no compute, no storage buffers. Resources: buffers (vertex,
  uniform with dynamic offsets, copy, map-read, query-resolve), 2D and 2D
  array textures (`R8Unorm`, `Rgba8Unorm`, `Bgra8Unorm`, `Rgba32Float`),
  views, linear samplers, bind groups, pipelines (triangle list/strip,
  per-instance vertex buffer, one colour target, fixed blend), render passes
  (clear/load, viewport, scissor, timestamps), texture and buffer copies,
  queue writes, readback, pipeline cache, device-lost callback.

## Vulkan backend rules

- Resources are reference-counted. A submission keeps every resource it
  recorded alive until its timeline value completes, so dropping a handle
  never frees memory the GPU still reads.
- Images live in `GENERAL` layout, swapchain images excepted. A global memory
  barrier separates passes and copies.
  `ponytail:` coarse barriers and one `VkDeviceMemory` per resource. Upgrade
  to tracked layouts and sub-allocation if profiling shows either costs.
- Queue writes go through a staging belt and are recorded ahead of the next
  submission, matching wgpu's ordering.
- Error scopes catch the errors the HAL itself raises (failed creation,
  surface configuration), per thread like wgpu's. Vulkan API validation is
  the layers' job, run in tests with `MUI_HAL_VALIDATION=1`.

## Acceptance

1. `mui-hal` Vulkan renders `present`, `quad`, `blur` and `weld` offscreen.
   The readback is byte-identical to its own wgpu backend on RADV and
   lavapipe.
2. `mui-vello` builds on `mui-hal`. The existing GPU/CPU pixel-parity tests
   pass on the Vulkan backend.
3. `mui-baseview` native windows present through `mui-hal` on Linux.
   `cargo tree -e normal -p <plugin> --target x86_64-unknown-linux-gnu` shows
   no `wgpu` for every shipped plugin except BUFFR, which keeps wgpu
   (`wgpu-only`) for its compute work.
4. Metal and D3D11 backends pass the same parity suite on mirror CI and the
   owner's hardware. After that, the `wgpu` feature is off for every native
   plugin target.

## Non-goals

- Compute, storage buffers, MSAA, mipmaps, depth/stencil.
- Migrating BUFFR's `wgsl-fft` or `mui-cut`'s 3D path (they keep `wgpu`).
- Runtime validation equal to wgpu's.
