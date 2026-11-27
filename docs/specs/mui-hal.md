---
status: in-progress
project: mui
owner: derpcat
created: 2026-11-20
updated: 2026-11-27
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
  reaching callers. `Surface` is `Send + !Sync` so a window thread can hand it
  to a render worker while preventing concurrent cross-thread surface races.
- Backends:
  - `vulkan` (Linux): `ash`, Vulkan 1.3 loader and device required (dynamic
    rendering and timeline semaphores in core), SPIR-V from `mui-shaders`,
    presentation through `mui-present`'s `VkSwapchain`.
  - `metal` (macOS): `objc2-metal` + `objc2-quartz-core` (`CAMetalLayer`),
    precompiled `.metallib` (hash-verified from `mui-shaders` or native
    `xcrun metal` + `xcrun metallib`) with MSL 2.3 runtime fallback, reusable
    `StorageModeShared` staging belt, sealed `FreeThreaded` / `SendMetal`
    threading markers, and main-thread-enforced `CAMetalLayer` mutations via
    GCD (`dispatch_sync_f` / non-blocking `dispatch_async_f`). Selectable via
    `MUI_HAL_BACKEND=metal` / `MUI_HAL_BACKEND=native` (or standalone builds
    without the `wgpu` feature); interim `wgpu` fallback remains default while
    `wgpu` is enabled until macOS OS-CI parity passes.
  - `d3d11` (Windows): `windows` COM bindings (`ID3D11Device`,
    `ID3D11DeviceContext`, `ID3D11DeviceContext1`, `IDXGIFactory2`,
    `IDXGISwapChain1`, `IDCompositionDevice`), precompiled SM 5.0 DXBC from
    `mui-shaders` (native FXC or hash-verified `prebuilt/dxbc/` for Linux ->
    `x86_64-pc-windows-gnu` cross-builds) with runtime `D3DCompile` fallback,
    presentation through `mui-present`'s `DxgiSwapChainPresenter` (defaulting
    opaque surfaces to `HwndFlipSwapChain` via `CreateSwapChainForHwnd` and
    reserving `DirectCompositionPresenter` for `translucent == true`),
    lock-ordered (`Surface::state -> DeviceShared::state`) serialization of all
    DXGI swapchain ops with the immediate context, sealed `FreeThreaded` /
    `SendCom` markers, zero-initialized buffers with full-shadow
    `UpdateSubresource(..., None, ...)` constant-buffer uploads, and bounded 5 s
    timeout + `GetDeviceRemovedReason()` checks in `poll(true)`. Selectable via
    `MUI_HAL_BACKEND=d3d11` / `MUI_HAL_BACKEND=native` / `WGPU_BACKEND=d3d11`
    (or standalone builds without the `wgpu` feature); interim `wgpu` fallback
    remains default while `wgpu` is enabled until Windows OS-CI parity passes.
  - `wgpu` (opt-in feature): passthrough for web, media, and compute-bearing
    plugins, and interim default on macOS and Windows until OS-CI parity
    promotes the native backends.
- Shaders are named by `mui_shaders::ShaderId`, never by arbitrary source text.
  `mui-shaders/build.rs` computes pure-Rust FIPS 180-4 SHA-256 digests of
  lowered HLSL/MSL sources and compiled blobs; when `prebuilt/{dxbc,metallib}/`
  is committed (it is, from `Matari-Audio/MUI` run 45360025044), every host build
  verifies `manifest.sha256` (source hash, blob hash, `DXBC`/`MTLB` magic) and
  fails loudly on any drift, while `MUI_SHADERS_EMIT_PREBUILT=<dir>` skips that
  verification and emits fresh blobs + manifests on Windows/macOS CI, so a WGSL
  edit is regenerated rather than blocked by its own stale manifest.
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
| Apple QuartzCore `CAMetalLayer` | macOS Metal 2.3 / QuartzCore | https://developer.apple.com/documentation/quartzcore/cametallayer?language=objc | 2026-11-24 | `nextDrawable()` is thread-safe and blocks up to 1s returning `nil` when `allowsNextDrawableTimeout == true`; `CALayer` hierarchy and property mutations (`addSublayer`, `setFrame`, `removeFromSuperlayer`) execute on the AppKit main thread (verified via `MainThreadMarker` or dispatched via GCD main queue); `displaySyncEnabled` paces to display refresh; `maximumDrawableCount` defaults to 3 (valid 2..=3). |
| Apple Metal `MTLDevice`, `MTLLibrary`, `MTLRenderPipelineDescriptor`, `MTLCommandBuffer` | MSL 2.3 (`MTLLanguageVersion::Version2_3`) | https://developer.apple.com/documentation/metal/mtldevice?language=objc | 2026-11-24 | `newLibraryWithData:error:` loads precompiled `.metallib` via `dispatch_data_t`; `newLibraryWithSource:options:error:` compiles MSL 2.3 at runtime fallback; `MTLDevice`, `MTLCommandQueue`, `MTLBuffer`, `MTLTexture`, `MTLSamplerState`, `MTLLibrary`, and `MTLRenderPipelineState` are thread-safe (`FreeThreaded`); per-frame encoding runs inside `objc2::rc::autoreleasepool`. |
| Microsoft Direct3D 11 (`D3D11CreateDevice`, `ID3D11Device`, `ID3D11DeviceContext`, `ID3D11DeviceContext1::UpdateSubresource1`, `D3DCompile`) | D3D_FEATURE_LEVEL_11_0 / 11_1, HLSL SM 5.0 | https://learn.microsoft.com/en-us/windows/win32/api/d3d11_1/nf-d3d11_1-id3d11devicecontext1-updatesubresource1 | 2026-11-24 | `ID3D11Device` and immutable device children are free-threaded while `ID3D11DeviceContext` is NOT thread-safe and requires external synchronization; `UpdateSubresource1` supports partial constant-buffer updates with 16-byte-aligned byte `D3D11_BOX` when `D3D11_FEATURE_DATA_D3D11_OPTIONS::ConstantBufferPartialUpdate == TRUE` (D3D11.0 `UpdateSubresource` requires `pDstBox = NULL`); `VSSetConstantBuffers1`/`PSSetConstantBuffers1` bind dynamic offsets in 16-byte constant units (256-byte aligned = 16 constants); `GetDeviceRemovedReason` detects TDR/device-removed without blocking. |
| Microsoft DXGI 1.2 (`IDXGIFactory2::CreateSwapChainForHwnd`, `CreateSwapChainForComposition`, `IDXGISwapChain::ResizeBuffers`) | DXGI 1.2 (`dxgi1_2.h`) | https://learn.microsoft.com/en-us/windows/win32/api/dxgi1_2/nf-dxgi1_2-idxgifactory2-createswapchainforhwnd | 2026-11-24 | `CreateSwapChainForHwnd` with `DXGI_SWAP_EFFECT_FLIP_DISCARD` + `DXGI_ALPHA_MODE_IGNORE` is the simpler, lower-risk default for opaque embedded DAW child HWNDs (`MakeWindowAssociation(DXGI_MWA_NO_ALT_ENTER)`); `CreateSwapChainForComposition` with `DXGI_ALPHA_MODE_PREMULTIPLIED` is reserved for `translucent == true`; DXGI swapchain ops (`ResizeBuffers`, `GetBuffer`, `Present`) use the immediate context internally and MUST be serialized under `DeviceShared::state`; `DXGI_ERROR_DEVICE_REMOVED`/`RESET`/`INVALID_CALL` map to `PresentError::SurfaceLost`; every back-buffer `ID3D11Texture2D`/`RTV` reference MUST be released before `ResizeBuffers`. |
| Microsoft DirectComposition (`DCompositionCreateDevice`, `IDCompositionDevice`, `IDCompositionTarget`, `IDCompositionVisual`) | DirectComposition (`dcomp.h`) | https://learn.microsoft.com/en-us/windows/win32/api/dcomp/nf-dcomp-dcompositioncreatedevice | 2026-11-24 | Reserved for translucent surfaces (`CompositeAlphaMode::PreMultiplied` / `PostMultiplied`); `DCompositionCreateDevice(dxgiDevice, ...)` binds to the D3D11 device's `IDXGIDevice`, attaches via `CreateTargetForHwnd(hwnd, false)` -> `CreateVisual()` -> `SetContent(swapChain)` -> `SetRoot(visual)` -> `Commit()`. Teardown clears content/root and commits without blocking. |
| Khronos Vulkan 1.3 Specification (Dynamic Rendering & Synchronization2) | Vulkan 1.3 (`VK_API_VERSION_1_3`, SPIR-V 1.3) | https://registry.khronos.org/vulkan/specs/1.3/html/vkspec.html | 2026-11-24 | `VK_KHR_dynamic_rendering` (`vkCmdBeginRendering`/`vkCmdEndRendering`) and `VK_KHR_synchronization2` (`vkQueueSubmit2`/`vkCmdPipelineBarrier2`) plus timeline semaphores are core in Vulkan 1.3; validation layers verified with `MUI_HAL_VALIDATION=1`. |

## Pinned Crate & SDK Ground Truth

| Dependency / Tool | Pinned Version | Lockfile / Header Source | Verified Symbols / Features |
|---|---|---|---|
| `ash` | `0.38.0+1.3.281` | `frameworks/mui/Cargo.lock` | Features `loaded`, `debug`; `Entry::load`, Vulkan 1.3 `cmd_begin_rendering`, `queue_submit2`, timeline semaphores. |
| `objc2` | `0.6.4` | `frameworks/mui/Cargo.lock` | Feature `std`; `rc::Retained`, `rc::autoreleasepool`, `runtime::ProtocolObject`, `MainThreadMarker`, `msg_send!`. |
| `objc2-foundation` | `0.3.2` | `frameworks/mui/Cargo.lock` | Features `NSArray`, `NSError`, `NSGeometry`, `NSObject`, `NSRange`, `NSString`. |
| `objc2-metal` | `0.3.2` | `frameworks/mui/Cargo.lock` | Features `MTLBlitCommandEncoder`, `MTLBuffer`, `MTLCommandBuffer`, `MTLCommandEncoder`, `MTLCommandQueue`, `MTLCounters`, `MTLDevice`, `MTLDrawable`, `MTLLibrary`, `MTLPixelFormat`, `MTLRenderCommandEncoder`, `MTLRenderPass`, `MTLRenderPipeline`, `MTLResource`, `MTLSampler`, `MTLTexture`, `MTLTypes`, `MTLVertexDescriptor`; verified `MTLCreateSystemDefaultDevice`, `MTLDevice::newLibraryWithSource_options_error`, `newLibraryWithData:error:`, `MTLRenderPipelineDescriptor`, `MTLCommandBuffer::waitUntilCompleted`. |
| `objc2-quartz-core` | `0.3.2` | `frameworks/mui/Cargo.lock` | Features `CALayer`, `CAMetalLayer`, `CATransaction`, `objc2-core-foundation`, `objc2-metal`; verified `CAMetalLayer::{new, setDevice, setPixelFormat, setFramebufferOnly, setDrawableSize, setMaximumDrawableCount, setDisplaySyncEnabled, setAllowsNextDrawableTimeout, nextDrawable}`, `CAMetalDrawable::texture`. |
| `windows` | `0.62.2` | `frameworks/mui/Cargo.lock` | Features `Win32_Foundation`, `Win32_Graphics_Direct3D`, `Win32_Graphics_Direct3D11`, `Win32_Graphics_Direct3D_Fxc`, `Win32_Graphics_DirectComposition`, `Win32_Graphics_Dxgi`, `Win32_Graphics_Dxgi_Common`; Microsoft-generated WinMD COM bindings (`D3D11CreateDevice`, `ID3D11Device`, `ID3D11DeviceContext`, `ID3D11DeviceContext1::{UpdateSubresource1, VSSetConstantBuffers1, PSSetConstantBuffers1}`, `CreateDXGIFactory1`, `IDXGIFactory2::{CreateSwapChainForHwnd, CreateSwapChainForComposition, MakeWindowAssociation}`, `IDXGISwapChain1`, `DCompositionCreateDevice`, `IDCompositionDevice`, `IDCompositionTarget`, `IDCompositionVisual`, `D3DCompile`). |
| `windows-sys` | `0.61.2` | `frameworks/mui/Cargo.lock` | Features `Win32_Foundation`, `Win32_Graphics_Gdi`, `Win32_UI_Shell`, `Win32_UI_WindowsAndMessaging` (retained by `mui-present/src/windows/dib.rs` for flat Win32 GDI/Shell C APIs). |

## Backend Rules

### Shared Native Infrastructure (`mui-hal/src/shared.rs`)
- `BackendError`, `ErrorSink`, `DeviceLost`, `BufferMapState`, `WorkWaiter`,
  `lock`, and portable resource/copy/write validation helpers live in
  `shared.rs` and are unit-tested on every host so Vulkan, Metal, and D3D11
  enforce identical validation contracts without duplicated boilerplate.

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
- Uses `objc2-metal` + `objc2-quartz-core` (`CAMetalLayer`) with precompiled
  `.metallib` from `mui-shaders` (loaded via `dispatch_data_create` +
  `newLibraryWithData:error:`) and MSL 2.3 (`MTLLanguageVersion::Version2_3`)
  runtime fallback when no `.metallib` is embedded.
- Sealed `FreeThreaded` and `SendMetal` marker traits restrict `SendObj` /
  `SendOnlyObj` to verified thread-safe Metal protocol objects and
  externally-synchronized objects (no blanket `unsafe impl<T>`).
- Queue writes (`write_buffer`, `write_texture`) sub-allocate from a reusable
  1 MiB `StorageModeShared` staging belt instead of allocating a fresh
  `MTLBuffer` per write. Only standard 1 MiB chunks are recycled (at most 8, so
  retained staging is bounded at 8 MiB); oversized one-off chunks are released
  on completion, and chunks whose staged writes were all pruned (retired
  drawables, empty submits) return to the pool immediately.
- Uniform buffers bind at `[[buffer(msl_slot)]]` (`0..15`), textures at
  `[[texture(msl_slot)]]`, samplers at `[[sampler(msl_slot)]]`, and vertex
  buffers at `VERTEX_BUFFER_BASE + slot` (`16 + slot`) to guarantee zero index
  collision with uniform buffers within Metal's 31-slot vertex buffer table.
- `CAMetalLayer` creation requires `MainThreadMarker`; `configure` dispatches
  layer hierarchy/property mutations synchronously onto the AppKit main thread
  (outside `Surface::state`) and `release`/`Drop` detaches via non-blocking
  `dispatch_async_f` on `_dispatch_main_q`. Presentation always calls
  `command_buffer.presentDrawable(&drawable)` followed by `commit()`.
- Command recording runs inside `objc2::rc::autoreleasepool`, retains all
  referenced resources until `MTLCommandBuffer` completion, and routes
  compilation/creation/submission errors into per-thread `ErrorSink` without
  reachable `.expect(...)` panics.

### D3D11 + DXGI / DirectComposition (Windows)
- `mui-present::windows::DxgiSwapChainPresenter` defaults opaque surfaces
  (`CompositeAlphaMode::Opaque` / `Auto`) to `HwndFlipSwapChain`
  (`IDXGIFactory2::CreateSwapChainForHwnd`, `DXGI_SWAP_EFFECT_FLIP_DISCARD`,
  `DXGI_ALPHA_MODE_IGNORE`, `DXGI_MWA_NO_ALT_ENTER`) and reserves
  `DirectCompositionPresenter` (`CreateSwapChainForComposition`,
  `DXGI_ALPHA_MODE_PREMULTIPLIED`) for `translucent == true`
  (`CompositeAlphaMode::PreMultiplied` / `PostMultiplied`).
- **Strict Lock Ordering (`Surface::state -> DeviceShared::state`):** Every
  DXGI swapchain operation (`new`, `resize`, `back_buffer`, `present`, and
  teardown) acquires `DeviceShared::state` (and unbinds OM render targets +
  flushes before `ResizeBuffers`, `Present`, and teardown) so DXGI's internal
  use of the immediate `ID3D11DeviceContext` can never race with `Queue::submit`.
- Sealed `FreeThreaded` and `SendCom` marker traits restrict `ComSend` /
  `ComSendOnly` to exact verified free-threaded D3D11/DXGI interfaces and
  mutex-guarded immediate contexts (no blanket `unsafe impl<T>`).
- `poll(true)` checks `GetDeviceRemovedReason()` inside the wait loop and
  enforces a bounded 5 s deadline with yield-then-sleep backoff; on timeout or
  device removal it publishes the lost flag *under* `DeviceShared::state`
  before releasing the `completed` it force-advanced (so no thread can treat
  unfinished GPU work as complete on a live device), then fires the device-lost
  callback outside the mutex, fails pending map callbacks cleanly, and breaks
  out without deadlocking.
- Swapchain teardown (device switch, translucency toggle, surface lost,
  `release`) runs under the old device's context lock in the order unbind ->
  release presenter -> `Flush`, so DXGI's deferred destruction completes before
  any device creates the next flip-model swapchain on the same `HWND`.
- `DXGI_ERROR_DEVICE_REMOVED`, `DXGI_ERROR_DEVICE_RESET`, and
  `DXGI_ERROR_INVALID_CALL` on `ResizeBuffers`, `GetBuffer`, and `Present`
  preserve `PresentError::SurfaceLost`.
- Every native buffer is created from zeros (D3D11 leaves `CreateBuffer`
  without initial data undefined; WebGPU buffers start zeroed). Uniform
  buffer writes update the CPU shadow and re-upload the whole shadow with
  `UpdateSubresource(..., None, ...)`, valid on every D3D11.0+ driver and
  versioned by the runtime against queued draws. Partial
  `UpdateSubresource1` boxes are deliberately not used: their
  constant-buffer semantics hinge on `ConstantBufferPartialUpdate` plus
  `NO_OVERWRITE`/`DISCARD` copy flags, which would race in-flight draws, and
  uniform buffers are at most 64 KiB.
- Shaders use precompiled SM 5.0 DXBC from `mui-shaders` (native FXC or
  hash-verified `prebuilt/dxbc/` on Linux -> `x86_64-pc-windows-gnu` cross-builds),
  falling back to runtime `D3DCompile` (`vs_5_0` / `ps_5_0`) when absent.
- All command-execution paths return structured `Error` instead of panicking on
  `.expect(...)`.

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
- [x] `mui-present` provides owned Windows `HwndFlipSwapChain` (opaque default)
  and `DirectCompositionPresenter` (translucent) via `DxgiSwapChainPresenter`,
  `mui-hal` implements sound native Metal (`objc2-metal` + main-thread-safe
  `CAMetalLayer` + staging belt) and D3D11 (lock-ordered swapchain/context
  synchronization, sealed `FreeThreaded` traits, bounded `poll(true)` deadline,
  full-shadow CB uploads), `mui-shaders` commits hash-checked prebuilt
  DXBC/`.metallib` blobs verified on every host, portable tests pass on Linux, and
  `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`, and `x86_64-pc-windows-gnu`
  all cross-compile cleanly with and without `--features mui-hal/wgpu`.
- [ ] Metal and D3D11 backends pass the offscreen + window parity suite on
  real macOS and Windows OS-CI runners (`native-hal.yml` on `Matari-Audio/MUI`)
  and hardware; only after that flips to `PASS` may the interim `wgpu` default
  on macOS/Windows be switched off.

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py frameworks/mui/docs/specs/mui-hal.md` | Exit 0, zero proof errors | PASS (2026-11-24, linux-x86_64) |
| Formatting check | Compile/ABI | linux-x86_64 | `cargo fmt --manifest-path frameworks/mui/Cargo.toml --all -- --check` | Exit 0, zero diffs | PASS (2026-11-24, linux-x86_64) |
| Workspace clippy gate | Compile/ABI | linux-x86_64 | `build-slot cargo clippy --manifest-path frameworks/mui/Cargo.toml --workspace --all-targets -- -D warnings` | Exit 0, zero warnings | PASS (2026-11-24, linux-x86_64) |
| Native + wgpu test suite | Validator/Runtime | linux-x86_64 | `build-slot cargo nextest run --manifest-path frameworks/mui/Cargo.toml -p mui-hal -p mui-present -p mui-shaders -p mui-vello -p mui-baseview` (and `--features mui-hal/wgpu`) | Exit 0, zero failures | PASS (2026-11-24, linux-x86_64) |
| macOS Metal cross-compile (native + wgpu) | Compile/ABI | aarch64-apple-darwin | `build-slot cargo check --manifest-path frameworks/mui/Cargo.toml --target aarch64-apple-darwin -p mui-hal -p mui-present -p mui-shaders -p mui-vello -p mui-baseview` (default & `--features mui-hal/wgpu`) | Exit 0, zero errors/warnings | PASS (2026-11-24, linux-x86_64 -> aarch64-apple-darwin) |
| Windows MSVC cross-compile (native + wgpu) | Compile/ABI | x86_64-pc-windows-msvc | `build-slot cargo check --manifest-path frameworks/mui/Cargo.toml --target x86_64-pc-windows-msvc -p mui-hal -p mui-present -p mui-shaders -p mui-vello -p mui-baseview` (default & `--features mui-hal/wgpu`) | Exit 0, zero errors/warnings | PASS (2026-11-24, linux-x86_64 -> x86_64-pc-windows-msvc) |
| Windows GNU shipping cross-compile (native + wgpu) | Compile/ABI | x86_64-pc-windows-gnu | `build-slot cargo check --manifest-path frameworks/mui/Cargo.toml --target x86_64-pc-windows-gnu -p mui-hal -p mui-present -p mui-shaders -p mui-vello -p mui-baseview` (default & `--features mui-hal/wgpu`) | Exit 0, zero errors/warnings | PASS (2026-11-24, linux-x86_64 -> x86_64-pc-windows-gnu) |
| Stack & lockfile sync check | Compile/ABI | linux-x86_64 | `python3 tools/sync-stack.py --check --skip-lock-refresh` | Exit 0, zero drift | PASS (2026-11-24, linux-x86_64) |
| Prebuilt shader blobs verified on Linux | Validator/Runtime | linux-x86_64 | `build-slot cargo test --manifest-path frameworks/mui/Cargo.toml -p mui-shaders` | Build verifies every committed DXBC/`.metallib` source hash, blob hash and magic | PASS (2026-11-24, linux-x86_64) |
| macOS Metal GPU & `CAMetalLayer` runtime parity | Hardware/OS-CI | macos-15 | `MUI_HAL_BACKEND=native MUI_HAL_REQUIRE_NATIVE=1 cargo test -p mui-hal -p mui-present -p mui-vello --features mui-vello/gpu-quad --locked -- --nocapture` (`native-hal.yml`) | Fail-closed native Metal device; offscreen readback exact on `mui-hal` tests and within the quad tests' tolerances vs `vello_cpu`; `native_surface` presents to a live `CAMetalLayer` | PASS (2026-11-26, macos-15, https://github.com/Matari-Audio/MUI/actions/runs/45910963619/job/140218981119) |
| Windows D3D11 & DXGI/DComp runtime parity | Hardware/OS-CI | windows-latest | `MUI_HAL_BACKEND=native MUI_HAL_REQUIRE_NATIVE=1 cargo test -p mui-hal -p mui-present -p mui-vello --features mui-vello/gpu-quad --locked -- --nocapture` (`native-hal.yml`) | Fail-closed native D3D11 device; offscreen readback exact on `mui-hal` tests and within the quad tests' tolerances vs `vello_cpu`; `native_surface` presents through `HwndFlipSwapChain` and `DirectCompositionPresenter` on a live `HWND` | PASS (2026-11-26, windows-latest, https://github.com/Matari-Audio/MUI/actions/runs/45910963619/job/140218980731) |
| Physical multi-vendor GPU & DAW host qualification (Wave 5) | Hardware/OS-CI | macOS + Windows hardware | Real Apple Silicon / Intel / AMD / NVIDIA hardware + embedded DAW hosts (Logic Pro, Ableton Live, Reaper, FL Studio) | Native Metal/D3D11 pass visual & lifecycle parity before flipping default off `wgpu` | PENDING_OS_CI (requires physical hardware + DAW hosts in Wave 5) |

## Unverified Boundaries & Promotion Gate

`native-hal.yml` on `Matari-Audio/MUI` (run `45910963619`) verified both
native backends end-to-end on GitHub-hosted `macos-15` (Metal + `CAMetalLayer`)
and `windows-latest` (D3D11 + WARP + `HwndFlipSwapChain` /
`DirectCompositionPresenter`) runners. The following boundaries remain outside
automated Linux/CI execution until Wave 5 hardware qualification:

- **Physical multi-vendor GPU & DAW host verification (macOS & Windows):**
  CI proves Metal on Apple's `macos-15` runner and D3D11 on Windows WARP, plus
  `native_surface` windowless `NSView` / popup `HWND` presentation. Real
  discrete/integrated GPUs (Apple Silicon, Intel/AMD/NVIDIA) and embedded DAW
  child windows (Logic Pro, Ableton Live, Reaper, FL Studio) still require
  hardware qualification before deleting the interim `wgpu` fallback.

**Hard Promotion Rules:**
1. This spec **MUST NOT** move to `status: implemented` while any required row
   in `## Verification Matrix` is `PENDING`, `PENDING_OS_CI`, or `BLOCKED`, or
   while any unchecked `- [ ]` acceptance criterion remains.
2. Neither `metal` nor `d3d11` may become the default backend when the `wgpu`
   feature is enabled, nor may `wgpu` be removed from macOS/Windows plugin
   builds, until its `Hardware/OS-CI` parity row is `PASS` and follow-up
   hardware qualification flips the default in its own dedicated PR.

## Task Decomposition

| # | Subtask | Owned Paths | Depends On | Verification |
|---|---|---|---|---|
| 1 | Upgrade `mui-hal.md` spec to 4-Tier Fact- & Docs-Proof Gate | `frameworks/mui/docs/specs/mui-hal.md` | — | `check-spec-proof.py` exits 0 |
| 2 | Implement `HwndFlipSwapChain` + `DirectCompositionPresenter` + `DxgiSwapChainPresenter` in `mui-present` | `frameworks/mui/crates/mui-present/**` | 1 | Cross-check MSVC + GNU + Linux unit tests |
| 3 | Implement sound native Metal backend (`objc2-metal`, sealed `FreeThreaded`, main-thread `CAMetalLayer`, staging belt) | `frameworks/mui/crates/mui-hal/src/metal/**` | 1 | `cargo check --target aarch64-apple-darwin -p mui-hal` + Linux unit tests |
| 4 | Implement sound native D3D11 backend (lock-ordered DXGI/context sync, sealed `FreeThreaded`, bounded `poll(true)`, zero-initialized full-shadow CB uploads) | `frameworks/mui/crates/mui-hal/src/d3d11/**` | 2 | Cross-check MSVC + GNU + Linux unit tests |
| 5 | Deduplicate shared HAL primitives (`shared.rs`), add hash-checked prebuilt DXBC/`.metallib` support (`mui-shaders/build.rs`), add `native-hal.yml` CI workflow, and verify all 4 targets | `frameworks/mui/crates/mui-hal/src/shared.rs`, `frameworks/mui/crates/mui-hal/src/lib.rs`, `frameworks/mui/crates/mui-shaders/**`, `frameworks/mui/.github/workflows/native-hal.yml` | 2, 3, 4 | fmt + clippy + nextest + 3 cross-targets + `sync-stack.py` + `check-spec-proof.py` |

## Rollout & Affected Dependents

- **Affected Projects:** `frameworks/mui` (direct); `frameworks/mui/media` and
  all MUI-consuming plugins (no lockfile changes required in this audit pass).
- **Mirror Impact:** Copybara PR to `Matari-Audio/MUI` (`frameworks/mui/**`),
  automatically triggering `native-hal.yml` on `macos-15` and `windows-latest`.
- **Versioning & Changelog:** No version bump required until OS-CI promotion
  removes the interim `wgpu` fallback on macOS/Windows.
