---
status: in-progress
project: mui
owner: mui-waves-2-3-present-hal
created: 2026-11-23
updated: 2026-11-23
---

# MUI graphics and presentation ownership

## Goal

Complete the owned presentation, shader, and GPU renderer work described in
Waves 2A, 2B (shaders), and 3A of the Matari stack ownership plan. MUI owns the
native buffers and swapchains used for presentation. Quad, blur, analytic weld,
and present shaders validate and translate during the build. The final native
UI renderer uses D3D11, Metal, and Vulkan directly.

## Invariants and scope

- Retain the audio-thread-safe polling setter and wake an idle editor when
  polling changes. Preserve resize scale rollback and X11 event draining.
- Native initialization, rendering, and presentation must preserve CPU
  fallback. Teardown must not wait on a stopped compositor or GPU.
- Use separate Wayland child surfaces for the software and Vulkan roles.
- Preserve retained damage, color and alpha behavior, and the CPU reference
  parity tests. Keep native code out of the library WASM build.
- Keep runtime shader translation out of the final dependency graph. The native
  quad HAL only needs raster passes. Media compute, ray queries, and browser
  rendering need an explicit migration that preserves their existing behavior.
- Linux binary hygiene, Moose and plugin changes, and monorepo tooling changes
  are out of scope for this branch.

## PR #130 audit acceptance

- The Wave 2 GPU renderer remains wgpu. Vulkan uses a fresh Wayland child
  surface per promotion. CPU and GPU children preserve the parent window's
  frame scheduling through promotion, fallback, and resize.
- Closing retires worker surfaces without waiting on the GUI thread. Platform
  and graphics handles remain alive until their last presenter releases them.
- Vulkan reconfiguration owns the replacement and its views even on failure.
  X11 only writes idle SHM slots and tiles core requests in both dimensions.
  IOSurface only writes idle or newly allocated buffers. GDI flushes before
  direct pixel writes.
- The published state includes the selected GPU backend and typed CPU fallback
  reason. The vendor test runs with its own lockfile.
- Native shader compilers emit SM 5.0 DXBC on Windows and verify MSL on macOS.
  Run the full gate and exercise the native Linux display paths with Weston
  and Xvfb.

Wave 3A and consumer migrations are follow-up work.

## Shader and presentation contracts

`mui-shaders::ShaderId::artifact()` exposes SPIR-V 1.3, MSL 2.3, and HLSL
Shader Model 5.0 with entry-point names and binding reflection. Vulkan uses the
WGSL group and binding. Metal and D3D11 use dense indices within each resource
class. The vertex shader normalizes SPIR-V clip space to the native viewport.

The presentation boundary uses physical framebuffer dimensions and retained
bitmap patches. Native surfaces own acquire, resize, present, and teardown.
The render loop retains its CPU fallback and diagnostics on adapter or
presentation failure.

## Verification

| Command (from the MUI workspace) | Criterion |
| --- | --- |
| `build-slot cargo nextest run --locked -p mui-present -p mui-baseview -p mui-vello` | Rebased presentation, wakeup, and rendering tests pass |
| `build-slot cargo nextest run --locked -p mui-shaders` | Artifact, reflection, layout, and available native compiler checks pass |
| `build-slot cargo nextest run --locked -p mui-hal -p mui-vello --all-features` | HAL and renderer tests pass, with GPU/CPU parity on a suitable adapter |
| `build-slot cargo check --locked -p mui-hal -p mui-baseview --target x86_64-pc-windows-msvc` | D3D11 and DirectComposition compile |
| `build-slot cargo check --locked -p mui-hal -p mui-baseview --target aarch64-apple-darwin` | Metal and AppKit compile |
| `build-slot tools/verify.sh` | Native, media, and WASM gates pass |
| `python3 -m unittest discover -s tools/stack-check` | Dependency guard tests pass |

Both MUI workspaces' lockfiles must contain zero retired presentation or GPU
packages after Wave 3A. Naga must be reachable only as a build dependency of
`mui-shaders`.

## Delivery

1. Rebase and verify owned presentation (`mui-present`, native host adapters).
2. Integrate and verify build-time artifact generation (`mui-shaders`, shader
   consumers).
3. Add and verify the native GPU HAL, migrate host and media callers, and
   remove retired dependencies from both locks.

These crates ship in the MUI mirror. The changes do not alter project file
formats or preset data.
