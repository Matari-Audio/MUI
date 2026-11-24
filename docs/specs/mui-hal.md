---
status: in-progress
project: mui
owner: derpcat
created: 2026-11-20
updated: 2026-11-24
proof_scope: external-api
---

# mui-hal: owned GPU HAL (Wave 3A)

Plan: Wave 3A of the MUI stack-ownership plan (`mui-hal` + `mui-present` + `mui-shaders`).

## Goal

Shipped plugin binaries link no `wgpu`. Native plugin windows render the
`mui-vello` quad pass and `effects::GpuRenderer` through `mui-hal`, a thin
render-only HAL shaped after the subset of `wgpu` that `mui-vello` uses.
Web/wasm, `mui-stage`/`mui-cut`, and compute-bearing plugins keep `wgpu`
through the HAL's opt-in `wgpu` backend until they migrate.

## Shape

- One concrete API, `wgpu`-subset names and descriptor shapes, so `mui-vello`
  ports by path (`wgpu::X` becomes `mui_hal::X`). Each handle is a cheap
  clone and dispatches over backends with an enum. No traits, no generics
  reaching callers.
- Backends:
  - `vulkan` (Linux): `ash`, Vulkan 1.3 loader and device required (dynamic
    rendering and timeline semaphores in core), SPIR-V from `mui-shaders`,
    presentation through `mui-present`'s `VkSwapchain`.
  - `metal` (macOS): `objc2-metal` + `objc2-quartz-core` (`CAMetalLayer`),
    MSL 2.3 from `mui-shaders`. Selectable via `MUI_HAL_BACKEND=metal` /
    `MUI_HAL_BACKEND=native` (or standalone builds without the `wgpu` feature);
    interim `wgpu` fallback remains default while `wgpu` is enabled until
    macOS OS-CI parity passes.
  - `d3d11` (Windows): `windows` COM bindings (`ID3D11Device`,
    `ID3D11DeviceContext`, `ID3D11DeviceContext1`, `IDXGIFactory2`,
    `IDXGISwapChain1`, `IDCompositionDevice`), precompiled SM 5.0 DXBC from
    `mui-shaders` with runtime `D3DCompile` fallback when cross-built without
    FXC, presentation through `mui-present`'s owned DirectComposition
    presenter (`dcomp::DirectCompositionPresenter`). Selectable via
    `MUI_HAL_BACKEND=d3d11` / `MUI_HAL_BACKEND=native` / `WGPU_BACKEND=d3d11`
    (or standalone builds without the `wgpu` feature); interim `wgpu` fallback
    remains default while `wgpu` is enabled until Windows OS-CI parity passes.
  - `wgpu` (opt-in feature): passthrough for web, media, and compute-bearing
    plugins, and interim default on macOS and Windows until OS-CI parity
    promotes the native backends.
- Shaders are named by `mui_shaders::ShaderId`, never by arbitrary source text.
- Render only: no compute, no storage buffers. Resources: buffers (vertex,
  uniform with dynamic offsets, copy, map-read, query-resolve), 2D and 2D
  array textures (`R8Unorm`, `Rgba8Unorm`, `Rgba8UnormSrgb`, `Bgra8Unorm`,
  `Bgra8UnormSrgb`, `Rgba16Float`, `Rgba32Float`), views, linear/nearest
  samplers, bind groups, pipelines (triangle list/strip, per-instance vertex
  buffer, one colour target, fixed blend), render passes (clear/load, viewport,
  scissor, timestamps), texture and buffer copies, queue writes, readback,
  pipeline cache, error scopes, and device-lost callback.

## Primary-Source & Version Proof

| Standard / Vendor API | Pinned Version / Spec Rev | Primary-Source URL | Verified Date | Key Invariants / Deprecations Checked |
|---|---|---|---|---|
| Apple QuartzCore `CAMetalLayer` | macOS Metal 2.3 / QuartzCore | https://developer.apple.com/documentation/quartzcore/cametallayer?language=objc | 2026-11-24 | `nextDrawable()` blocks up to 1s and returns `nil` when `allowsNextDrawableTimeout == true` (default); `presentsWithTransaction == true` requires `MTLCommandBuffer::waitUntilScheduled()` before calling `MTLDrawable::present()`; `displaySyncEnabled` paces to display refresh; `maximumDrawableCount` defaults to 3 (valid 2..=3). |
| Apple Metal `MTLDevice`, `MTLLibrary`, `MTLRenderPipelineDescriptor`, `MTLComputePipelineState`, `MTLCommandBuffer` | MSL 2.3 (`MTLLanguageVersion::Version2_3`) | https://developer.apple.com/documentation/metal/mtldevice?language=objc | 2026-11-24 | `newLibraryWithSource:options:error:` compiles MSL 2.3 at runtime; `MTLRenderPipelineDescriptor` configures vertex/fragment `MTLFunction`, `MTLVertexDescriptor`, and `colorAttachments[0]` pixel format and blend state; `MTLComputePipelineState` represents compute kernels (out of scope for render-only HAL); `waitUntilScheduled()` blocks until GPU scheduling without waiting for full execution; wrap per-frame encoding in `objc2::rc::autoreleasepool`. |
| Microsoft Direct3D 11 (`D3D11CreateDevice`, `ID3D11Device`, `ID3D11DeviceContext`, `ID3D11DeviceContext1`, `D3DCompile`) | D3D_FEATURE_LEVEL_11_0 / HLSL SM 5.0 | https://learn.microsoft.com/en-us/windows/win32/api/d3d11/nf-d3d11-d3d11createdevice | 2026-11-24 | `D3D11CreateDevice` creates `ID3D11Device` + immediate `ID3D11DeviceContext`; `D3D11_USAGE_STAGING` with `CPUAccessFlags = D3D11_CPU_ACCESS_READ` is required for GPU-to-CPU readback via `CopyResource`/`CopySubresourceRegion` + `Map`/`Unmap`; `ID3D11DeviceContext1::VSSetConstantBuffers1`/`PSSetConstantBuffers1` bind dynamic constant-buffer offsets in 16-byte constant units (256-byte aligned = 16 constants); `D3DCompile` compiles HLSL SM 5.0 (`vs_5_0`/`ps_5_0`) when precompiled DXBC is absent. |
| Microsoft DXGI 1.2 (`IDXGIFactory2::CreateSwapChainForComposition`, `IDXGISwapChain::ResizeBuffers`) | DXGI 1.2 (`dxgi1_2.h`) | https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgifactory2-createswapchainforcomposition | 2026-11-24 | Accepts `ID3D11Device` (as `IUnknown`); requires `DXGI_SWAP_EFFECT_FLIP_DISCARD` or `DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL`, `BufferCount >= 2`, `Scaling = DXGI_SCALING_STRETCH`, `SampleDesc = { Count: 1, Quality: 0 }`; every outstanding `ID3D11RenderTargetView` and back-buffer `ID3D11Texture2D` reference MUST be released before `ResizeBuffers`. |
| Microsoft DirectComposition (`DCompositionCreateDevice`, `IDCompositionDevice`, `IDCompositionTarget`, `IDCompositionVisual`) | DirectComposition (`dcomp.h`) | https://learn.microsoft.com/en-us/windows/win32/api/dcomp/nf-dcomp-dcompositioncreatedevice | 2026-11-24 | `DCompositionCreateDevice(dxgiDevice, ...)` binds to the D3D11 device's `IDXGIDevice`; attach to `HWND` via `CreateTargetForHwnd(hwnd, false)`, create root visual via `CreateVisual()`, bind swapchain via `SetContent(swapChain)`, attach via `SetRoot(visual)`, and publish atomically via `Commit()`. Teardown clears root/content and commits without blocking. |
| Khronos Vulkan 1.3 Specification (Dynamic Rendering & Synchronization2) | Vulkan 1.3 (`VK_API_VERSION_1_3`, SPIR-V 1.3) | https://registry.khronos.org/vulkan/specs/1.3/html/vkspec.html | 2026-11-24 | `VK_KHR_dynamic_rendering` (`vkCmdBeginRendering`/`vkCmdEndRendering`) and `VK_KHR_synchronization2` (`vkQueueSubmit2`/`vkCmdPipelineBarrier2`) plus timeline semaphores are core in Vulkan 1.3; validation layers verified with `MUI_HAL_VALIDATION=1`. |

## Pinned Crate & SDK Ground Truth

| Dependency / Tool | Pinned Version | Lockfile / Header Source | Verified Symbols / Features |
|---|---|---|---|
| `ash` | `0.38.0+1.3.281` | `frameworks/mui/Cargo.lock` | Features `loaded`, `debug`; `Entry::load`, Vulkan 1.3 `cmd_begin_rendering`, `queue_submit2`, timeline semaphores. |
| `objc2` | `0.6.4` | `frameworks/mui/Cargo.lock` | Feature `std`; `rc::Retained`, `rc::autoreleasepool`, `runtime::ProtocolObject`, `MainThreadMarker`. |
| `objc2-foundation` | `0.3.2` | `frameworks/mui/Cargo.lock` | Features `NSArray`, `NSError`, `NSGeometry`, `NSObject`, `NSRange`, `NSString`. |
| `objc2-metal` | `0.3.2` | `frameworks/mui/Cargo.lock` | Features `MTLBlitCommandEncoder`, `MTLBuffer`, `MTLCommandBuffer`, `MTLCommandEncoder`, `MTLCommandQueue`, `MTLCounters`, `MTLDevice`, `MTLDrawable`, `MTLLibrary`, `MTLPixelFormat`, `MTLRenderCommandEncoder`, `MTLRenderPass`, `MTLRenderPipeline`, `MTLResource`, `MTLSampler`, `MTLTexture`, `MTLTypes`, `MTLVertexDescriptor`; verified `MTLCreateSystemDefaultDevice`, `MTLDevice::newLibraryWithSource_options_error`, `MTLRenderPipelineDescriptor`, `MTLCommandBuffer::waitUntilScheduled`, `MTLCommandBuffer::waitUntilCompleted`. |
| `objc2-quartz-core` | `0.3.2` | `frameworks/mui/Cargo.lock` | Features `CALayer`, `CAMetalLayer`, `CATransaction`, `objc2-core-foundation`, `objc2-metal`; verified `CAMetalLayer::{new, setDevice, setPixelFormat, setFramebufferOnly, setDrawableSize, setMaximumDrawableCount, setPresentsWithTransaction, setDisplaySyncEnabled, setAllowsNextDrawableTimeout, nextDrawable}`, `CAMetalDrawable::texture`. |
| `windows` | `0.62.2` | `frameworks/mui/Cargo.lock` | Features `Win32_Foundation`, `Win32_Graphics_Direct3D`, `Win32_Graphics_Direct3D11`, `Win32_Graphics_Direct3D_Fxc`, `Win32_Graphics_DirectComposition`, `Win32_Graphics_Dxgi`, `Win32_Graphics_Dxgi_Common`; Microsoft-generated WinMD COM bindings (`D3D11CreateDevice`, `ID3D11Device`, `ID3D11DeviceContext`, `ID3D11DeviceContext1`, `CreateDXGIFactory1`, `IDXGIFactory2::CreateSwapChainForComposition`, `IDXGISwapChain1`, `DCompositionCreateDevice`, `IDCompositionDevice`, `IDCompositionTarget`, `IDCompositionVisual`, `D3DCompile`). Note: `windows-sys 0.61.2` intentionally omits COM interfaces (removed upstream in `windows-sys 0.59+`), so COM backends use `windows 0.62.2` (already pinned in the workspace lockfile) for ABI-exact vtables and RAII `IUnknown::Release`. |
| `windows-sys` | `0.61.2` | `frameworks/mui/Cargo.lock` | Features `Win32_Foundation`, `Win32_Graphics_Gdi`, `Win32_UI_Shell`, `Win32_UI_WindowsAndMessaging` (retained by `mui-present/src/windows/dib.rs` for flat Win32 GDI/Shell C APIs). |

## Backend Rules

### Vulkan (Linux)
- Resources are reference-counted. A submission keeps every resource it
  recorded alive until its timeline value completes, so dropping a handle
  never frees memory the GPU still reads.
- Images live in `GENERAL` layout, swapchain images excepted. A global memory
  barrier separates passes and copies.
- Queue writes go through a staging belt and are recorded ahead of the next
  submission, matching `wgpu`'s ordering.
- Error scopes catch HAL errors per thread like `wgpu`; Vulkan API validation
  runs with `MUI_HAL_VALIDATION=1`.

### Metal (macOS)
- Uses `objc2-metal` + `objc2-quartz-core` (`CAMetalLayer`) with `mui-shaders`
  MSL 2.3 sources (`MTLLanguageVersion::Version2_3`).
- Uniform buffers bind at `[[buffer(msl_slot)]]` (`0..15`), textures at
  `[[texture(msl_slot)]]`, samplers at `[[sampler(msl_slot)]]`, and vertex
  buffers at `VERTEX_BUFFER_BASE + slot` (`16 + slot`) to guarantee zero index
  collision with uniform buffers within Metal's 31-slot vertex buffer table.
- `CAMetalLayer` configures `pixelFormat`, `drawableSize`, `framebufferOnly`,
  `maximumDrawableCount` (clamped `2..=3`), `displaySyncEnabled` from
  `PresentMode`, `allowsNextDrawableTimeout = true` (returning
  `CurrentSurfaceTexture::Timeout` cleanly when `nextDrawable()` returns `nil`),
  and `opaque` from `CompositeAlphaMode`. When `presentsWithTransaction` is
  enabled, presentation calls `commit()` + `waitUntilScheduled()` before
  `drawable.present()`; otherwise `command_buffer.presentDrawable(&drawable)`
  before `commit()`.
- Command recording runs inside `objc2::rc::autoreleasepool`, retains all
  referenced resources until `MTLCommandBuffer` completion, and routes
  compilation/creation/submission errors into per-thread `ErrorSink`.

### D3D11 + DirectComposition (Windows)
- `mui-present::windows::dcomp::DirectCompositionPresenter` owns the
  `IDCompositionDevice`, `IDCompositionTarget`, `IDCompositionVisual`, and
  `IDXGISwapChain1` created via `IDXGIFactory2::CreateSwapChainForComposition`
  with `DXGI_SWAP_EFFECT_FLIP_DISCARD`, `BufferCount = 2` (or configured
  latency + 1, minimum 2), `DXGI_SCALING_STRETCH`, and `DXGI_ALPHA_MODE`
  matching `CompositeAlphaMode`.
- Before calling `ResizeBuffers`, the surface releases all cached back-buffer
  `ID3D11Texture2D` and `ID3D11RenderTargetView` references and flushes the
  device context's OM target binding (`OMSetRenderTargets(None, None)`).
- Shaders use precompiled SM 5.0 DXBC from `mui-shaders` when present (Windows
  native builds with FXC), falling back to runtime `D3DCompile` (`vs_5_0` /
  `ps_5_0`) on cross-compiled builds without embedded DXBC.
- Input layouts bind `@location(N)` attributes to HLSL semantic `"LOC"` +
  `SemanticIndex = N` matching naga's SM 5.0 output.
- Uniform buffers use `ID3D11DeviceContext1::VSSetConstantBuffers1` /
  `PSSetConstantBuffers1` when dynamic offsets are non-zero (16-byte constant
  granularity; 256-byte aligned HAL dynamic offsets map cleanly to multiples of
  16 constants), with a staging-buffer fallback if `ConstantBufferOffsetting`
  is unsupported.
- GPU-to-CPU readback copies into `D3D11_USAGE_STAGING` (`D3D11_CPU_ACCESS_READ`)
  and maps via `ID3D11DeviceContext::Map`/`Unmap`.
- All COM pointers are owned by `windows` RAII wrappers (`Drop` calls
  `IUnknown::Release()`); `DirectCompositionPresenter::drop` clears visual
  content and root target and commits without blocking.

## Invariants & Non-Goals

### Hard Invariants
- **Audio RT-Safety:** `mui-hal` and `mui-present` never run on the audio
  processing thread; GUI/render presentation never blocks audio callbacks.
- **Lockfile & Feature Hygiene:** Never add Linux target-specific features to
  `mui-hal`'s optional `wgpu` dependency. Keep `wgpu` fallback selectable via
  `MUI_HAL_BACKEND=wgpu` (and default on macOS/Windows while `wgpu` feature is
  enabled) until OS-CI parity passes.
- **Mirror Boundaries:** `frameworks/mui` exports publicly to
  `Matari-Audio/MUI`. Zero local filesystem paths, secrets, or private monorepo
  links may appear in `frameworks/mui`.
- **Error-Scope Semantics:** Native backend object creation, shader
  compilation, buffer mapping, and submission errors report into per-thread
  `ErrorSink` (or the uncaptured handler) without panicking on driver failure.

### Non-Goals
- Compute shaders, storage buffers, MSAA, mipmaps, depth/stencil.
- Migrating `wgpu`-only compute pipelines or 3D media pipelines (they keep
  `wgpu`).
- Runtime validation equal to `wgpu`'s full WebGPU validator.
- Deleting the interim `wgpu` fallback on macOS/Windows prior to passing real
  macOS and Windows OS-CI parity.

## Acceptance Criteria

- [x] `mui-hal` Vulkan renders `present`, `quad`, `blur`, and `weld` offscreen
  byte-identical to its `wgpu` backend on RADV and lavapipe.
- [x] `mui-vello` builds on `mui-hal` and passes GPU/CPU pixel-parity tests on
  the Vulkan backend.
- [x] `mui-baseview` native windows present through `mui-hal` on Linux with no
  `wgpu` linked for non-compute plugins.
- [x] `mui-present` provides owned Windows DirectComposition GPU presenter
  (`dcomp::DirectCompositionPresenter`), `mui-hal` implements native Metal
  (`objc2-metal` + `CAMetalLayer`) and D3D11 (`windows` COM + DirectComposition)
  backends, portable layout/binding/descriptor unit tests pass on Linux, and
  both `aarch64-apple-darwin` and `x86_64-pc-windows-msvc` cross-compile cleanly
  with and without `--features wgpu`.
- [ ] Metal and D3D11 backends pass the offscreen + window parity suite on
  real macOS and Windows OS-CI runners and hardware; only after that flips to
  `PASS` may the interim `wgpu` default on macOS/Windows be switched off.

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py frameworks/mui/docs/specs/mui-hal.md` | Exit 0, zero proof errors | PASS (2026-11-24, linux-x86_64) |
| Formatting check | Compile/ABI | linux-x86_64 | `cargo fmt --manifest-path frameworks/mui/Cargo.toml --all -- --check` | Exit 0, zero diffs | PASS (2026-11-24, linux-x86_64) |
| Workspace clippy gate | Compile/ABI | linux-x86_64 | `build-slot cargo clippy --manifest-path frameworks/mui/Cargo.toml --workspace --all-targets -- -D warnings` | Exit 0, zero warnings | PASS (2026-11-24, linux-x86_64) |
| Native + wgpu test suite | Validator/Runtime | linux-x86_64 | `build-slot cargo nextest run --manifest-path frameworks/mui/Cargo.toml -p mui-hal -p mui-present -p mui-shaders -p mui-vello -p mui-baseview` (and `--features mui-hal/wgpu`) | Exit 0, zero failures, zero Vulkan validation errors (275/275 native + 275/275 wgpu + 30/30 `MUI_HAL_VALIDATION=1`) | PASS (2026-11-24, linux-x86_64) |
| macOS Metal cross-compile (native + wgpu) | Compile/ABI | aarch64-apple-darwin | `build-slot cargo check --manifest-path frameworks/mui/Cargo.toml --target aarch64-apple-darwin -p mui-hal -p mui-present -p mui-vello -p mui-baseview` (default & `--features mui-hal/wgpu`, plus `clippy -p mui-hal -p mui-present -- -D warnings`) | Exit 0, zero errors/warnings | PASS (2026-11-24, linux-x86_64 -> aarch64-apple-darwin) |
| Windows D3D11 + DirectComposition cross-compile (native + wgpu) | Compile/ABI | x86_64-pc-windows-msvc | `build-slot cargo check --manifest-path frameworks/mui/Cargo.toml --target x86_64-pc-windows-msvc -p mui-hal -p mui-present -p mui-vello -p mui-baseview` (default & `--features mui-hal/wgpu`, plus `clippy -p mui-hal -p mui-present -- -D warnings`) | Exit 0, zero errors/warnings | PASS (2026-11-24, linux-x86_64 -> x86_64-pc-windows-msvc) |
| Lockfile offline integrity | Compile/ABI | linux-x86_64 | `cargo metadata --locked --offline --manifest-path frameworks/mui/Cargo.toml && cargo metadata --locked --offline --manifest-path frameworks/mui/media/Cargo.toml` | Exit 0 on both lockfiles (`sync-stack.py --check --projects frameworks/mui` PASS) | PASS (2026-11-24, linux-x86_64) |
| macOS Metal GPU & `CAMetalLayer` runtime parity | Hardware/OS-CI | macos-aarch64 | `MUI_HAL_BACKEND=metal cargo test --manifest-path frameworks/mui/Cargo.toml -p mui-hal -p mui-vello` | Byte-identical readback vs `wgpu` Metal and live `CAMetalLayer` presentation | PENDING_OS_CI (macos) |
| Windows D3D11 & DirectComposition runtime parity | Hardware/OS-CI | windows-x86_64 | `MUI_HAL_BACKEND=d3d11 cargo test --manifest-path frameworks/mui/Cargo.toml -p mui-hal -p mui-present -p mui-vello` | Byte-identical readback vs `wgpu` DX12 and live DirectComposition swapchain presentation | PENDING_OS_CI (windows) |

## Unverified Boundaries & Promotion Gate

When authoring on a Linux x86_64 host, the following cannot execute locally:

- **macOS Metal GPU & `CAMetalLayer` execution:** Cross-compilation against
  `aarch64-apple-darwin` proves Rust/Objective-C ABI typing via `objc2-metal`
  and `objc2-quartz-core`, and portable slot/descriptor tests execute on Linux,
  but actual Metal shader compilation, command execution, readback parity, and
  `CAMetalLayer` presentation require a macOS runner or hardware.
- **Windows D3D11 & DirectComposition execution:** Cross-compilation against
  `x86_64-pc-windows-msvc` proves COM interface/struct typing via `windows`,
  and portable slot/descriptor/input-layout tests execute on Linux, but actual
  `D3D11CreateDevice`, `D3DCompile`/FXC bytecode creation, `ID3D11DeviceContext`
  rendering, `DCompositionCreateDevice`, and `IDXGISwapChain1` resize/present
  require a Windows runner or hardware.

**Hard Promotion Rules:**
1. This spec **MUST NOT** move to `status: implemented` while any required row
   in `## Verification Matrix` is `PENDING`, `PENDING_OS_CI`, or `BLOCKED`, or
   while any unchecked `- [ ]` acceptance criterion remains.
2. Neither `metal` nor `d3d11` may become the default backend when the `wgpu`
   feature is enabled, nor may `wgpu` be removed from macOS/Windows plugin
   builds, until its `PENDING_OS_CI` row becomes `PASS` on real OS CI or
   hardware.

## Task Decomposition

| # | Subtask | Owned Paths | Depends On | Verification |
|---|---|---|---|---|
| 1 | Upgrade `mui-hal.md` spec to 4-Tier Fact- & Docs-Proof Gate | `frameworks/mui/docs/specs/mui-hal.md` | — | `check-spec-proof.py` exits 0 |
| 2 | Implement `DirectCompositionPresenter` in `mui-present` + `GraphicsApi::D3D11` | `frameworks/mui/crates/mui-present/**` | 1 | `cargo check --target x86_64-pc-windows-msvc -p mui-present` + Linux unit tests |
| 3 | Implement native Metal backend (`objc2-metal` + `CAMetalLayer`) + portable tests | `frameworks/mui/crates/mui-hal/src/metal/**` | 1 | `cargo check --target aarch64-apple-darwin -p mui-hal` + Linux unit tests |
| 4 | Implement native D3D11 backend (`windows` COM + DirectComposition) + portable tests | `frameworks/mui/crates/mui-hal/src/d3d11/**` | 2 | `cargo check --target x86_64-pc-windows-msvc -p mui-hal` + Linux unit tests |
| 5 | Wire `mui-hal` dispatch, runtime `MUI_HAL_BACKEND` selection, lockfiles, and full cross-target verification | `frameworks/mui/crates/mui-hal/src/lib.rs`, `frameworks/mui/Cargo.lock`, `frameworks/mui/media/Cargo.lock` | 2, 3, 4 | fmt + clippy + nextest + macOS/Windows cross-check + locked metadata |

## Rollout & Affected Dependents

- **Affected Projects:** `frameworks/mui` (direct); `frameworks/mui/media` and
  all MUI-consuming plugins (transitive lockfile refresh when manifests add
  target-specific deps).
- **Mirror Impact:** Copybara PR to `Matari-Audio/MUI` (`frameworks/mui/**`).
- **Versioning & Changelog:** No version bump required until OS-CI promotion
  removes the interim `wgpu` fallback on macOS/Windows.
