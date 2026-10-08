# Platform validation — 2026-10-08

## Reproduced Windows crash

Verify [run 37784386607](https://github.com/Matari-Audio/MUI/actions/runs/37784386607), Windows native DX12 probe, reproduced an access violation both normally and under CDB. The `mui-ui-native-windows-2025-dx12` artifact contains the original result, second-chance exception, all-thread stacks, module inventory, minidump, executable and PDB. The artifact expires after seven days; the relevant stack is retained below.

Adapter: Microsoft Basic Render Driver, DX12, `DeviceType::Cpu`, vendor `0x1414`, device `0x008c`, driver `10.0.26100.33438`, Windows 2025 runner. MUI recorded one surface submission and one retained frame before the process faulted. Submission does not prove completed GPU execution.

```text
ExceptionCode: c0000005 (access violation, write)
Faulting driver worker:
  d3d10warp!ComputeShaderTransformer::ConstructLoadStoreSets+0x319
  d3d10warp!PixelJitOptimizer::TransformComputeShader+0x16b
  d3d10warp!PixelJitOptimizer::Run+0x104
  d3d10warp!PixelJITProcessor::Initialize+0x51
  d3d10warp!Task_CompileShader+0x23
  d3d10warp!Task::ExecuteTask+0x148
  d3d10warp!ThreadPool::WorkCallBack+0xb8

Concurrent main thread:
  d3d10warp!JITRenderContext::CompileComputePipeline+0x61
  d3d10warp!UMContext::DispatchMain+0x196
  d3d10warp!UMCommandQueue::ExecuteCommandLists+0x68
  D3D12Core!CCommandQueue<0>::ExecuteCommandListsImpl+0x4e8
  wgpu_hal::dx12::submit                    wgpu-hal-30.0.1/src/dx12/mod.rs:1799
  vello::wgpu_engine::WgpuEngine::run_recording  vendor/vello/src/wgpu_engine.rs:802
  vello::Renderer::render_to_texture        vendor/vello/src/lib.rs:493
  mui_vello::effects::retained::render      crates/mui-vello/src/effects/retained.rs:315
  mui_vello::host::Host::present_inner      crates/mui-vello/src/host.rs:940
  mui_winit::gpu::Gpu::present              crates/mui-winit/src/gpu.rs:80
  mui_preview::App::draw                   crates/mui-preview/src/main.rs:626
```

This identifies the fault in WARP's compute shader JIT. It does not identify the particular shader instruction that triggers it, prove a WGPU/Vello defect, or explain consumer NVIDIA/Intel failures. Rust error scopes and `catch_unwind` cannot recover from this native worker fault. The host now rejects Microsoft CPU adapters on DX12 before device/pipeline creation, retaining the rejection in adapter diagnostics and trying remaining candidates. Baseview editors then use the existing Vello CPU presenter. Hardware adapters and Mesa software Vulkan remain eligible. The guard covers `Host`; direct device/renderer construction bypasses it.

## Fixed host failures

- macOS baseview `Window::is_open()` returned the closed flag without negating it. The portable editor smoke now checks the live window state.
- Baseview retained a partially configured GPU after terminal resize errors and retried forever. It now drops that surface and transitions to CPU, matching presentation-error behavior. A real native regression establishes GPU presentation, destroys the device, resizes and requires CPU presentation plus visible green pixels at the recovered extent.

## Coverage submitted for validation

| Target | Required checks | Limits |
| --- | --- | --- |
| Linux X11 | Vulkan/GL gallery pixels, input, accessibility, embedded GPU/CPU, failed resize recovery, expose/reopen and multiwindow/device-loss tests | Hosted Mesa software drivers; RX 6600 measurements are separate |
| Linux Wayland | Headless Weston plus standalone two-window presentation, resize and teardown | Baseview plugin embedding remains X11; this does not implement Wayland plugin embedding |
| Windows | Native workspace lint/tests; embedded explicit CPU, unavailable-backend fallback, actual WARP rejection and CPU presentation, resize/close/reopen | Original DX12 GPU probe remains informational and fails when no eligible adapter exists |
| macOS | Native workspace lint/tests and accessibility library isolation; embedded CPU/fallback lifecycle | Metal gallery, GPU embedded and multiwindow probes remain informational |
| Browser WASM | Chromium, Firefox and WebKit worker execution, nonuniform canvas pixels, scene edits, error recovery, viewport resize; screenshots and traces retained | Playground uses Vello CPU/WASM and a fixed 640×480 render target; viewport resize tests display resizing, not GPU/WebGPU or renderer extent changes |

Windows/macOS workspace checks are unconditional on updates. Embedded CPU lifecycle checks are required separately from GPU probes: a failed or absent GPU must not count as a successful rendering test. All jobs use standard hosted runners. The browser harness follows the [official Playwright CI procedure](https://playwright.dev/python/docs/ci), with a pinned test dependency outside the Rust production workspace.

The new checks are not yet a pass claim. Local Rust builds/tests/clippy are blocked by the user's instruction to leave other Rust jobs running. The previous revision passed Linux Vulkan/GL UI, macOS Metal gallery, root lint and root tests. Hosted validation of these new changes is required.

Remaining work: standalone winit/preview has no CPU fallback and therefore exits cleanly on a WARP-only machine after rejection; direct renderer clients need an explicit adapter policy. Consumer NVIDIA/Intel drivers, real DAW attachment/unload, native GPU material/text parity, browser WebGPU, and embedded Wayland need separate evidence. Reporting service/plugin wiring is still incomplete and is not deployed.
