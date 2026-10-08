# GPUI and MUI graphics comparison

Source audit on 2026-10-08, pinned to Zed
[`f1a10a5227a331e86bdb006301e1090a21d33e7a`](https://github.com/zed-industries/zed/tree/f1a10a5227a331e86bdb006301e1090a21d33e7a).
Two independent researchers examined device requirements and native lifecycle.
This establishes implementation differences, not comparative crash rates.

The [GPUI/Vello integration decision](gpui-vello-integration.md) supersedes a
native-callback-only integration: GPUI must replace UI ownership to earn the
breaking change. The [renderer comparison](plugin-renderer-choice.md) now favors
Vello GPU over classic compute Vello for the first material-preserving proof;
native GPUI and Skia raster remain concrete alternatives.

| Area | GPUI evidence | Implication for MUI |
| --- | --- | --- |
| Windows renderer | [D3D11 feature levels 11.1, 11.0 and 10.1, with capability checks](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_windows/src/directx_devices.rs#L103-L195); [raster shader model 4.1](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_windows/build.rs#L143-L174) | Classic Vello uses compute shaders. Working GPUI does not establish that an adapter can run MUI's renderer. Validate actual compute requirements and use the native baseview CPU fallback for returned failures. |
| Linux stack | [wgpu context](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_wgpu/src/wgpu_context.rs#L417-L480); [Blade replaced by wgpu](https://github.com/zed-industries/zed/pull/46758) | Current GPUI Linux uses wgpu, while Windows/macOS use native APIs. Backend names alone do not predict stability. |
| Adapter selection | [Linux override, compositor GPU, then ranked candidates](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_wgpu/src/wgpu_context.rs#L497-L689); [hybrid NVIDIA/Intel Wayland fix](https://github.com/zed-industries/zed/pull/50528) | MUI prefers a discrete adapter. Compositor matching deserves hardware investigation; it is not safe to infer the correct device from vendor alone. Record every candidate and the selected adapter first. |
| Resize and acquisition | [Wait for work, retire old targets, recreate after healthy acquisition, use acquired dimensions](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_wgpu/src/wgpu_renderer.rs#L1000-L1197) | MUI must commit its configured size only after successful target allocation/configuration. Present retries an unsuccessful resize instead of acquiring an inconsistent surface. Do not copy an indefinite wait into a DAW editor. |
| Device loss | [Coordinated renderer recovery](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_wgpu/src/wgpu_renderer.rs#L2203-L2279); [Intel atlas recovery fix](https://github.com/zed-industries/zed/pull/56035) | Recreate renderer resources and force fresh content together. Test device loss, generation changes, and drawing after recovery. GPUI has also had real recovery bugs. |
| Window teardown | [X11 renderer released before native window/connection](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_linux/src/linux/x11/window.rs#L883-L905) | Keep GPU/surface resources inside window lifetime. Test immediate close, close with queued work, reopen and multiple editors. |
| Graphics tests | [Adapter-required rendering/resize tests](https://github.com/zed-industries/zed/blob/f1a10a5227a331e86bdb006301e1090a21d33e7a/crates/gpui_wgpu/src/wgpu_renderer.rs#L2678-L2770) | Missing adapters must fail a presentation test. Software hosted tests and native VM probes must identify their actual coverage. |

GPUI is not evidence of a universal CPU safety net. Linux accepts CPU adapters
at startup but excludes them in recovery; Windows production device creation
does not explicitly request WARP; the Metal renderer needs a Metal device.
MUI's integrated baseview fallback uses `vello_cpu` and `softbuffer`, and
`MUI_RENDERER=cpu` bypasses GPU startup. The winit gallery remains a GPU host.
A native driver fault can terminate the process before returned-error fallback
runs. The child collector and interrupted-operation markers preserve different
parts of that evidence; neither supplies a native stack trace.

Free hosted Linux UI tests cover Vulkan and GL software implementations, all
gallery scenes, input, themes, native resizing, visible pixels and teardown.
Native DX12/Metal VM probes retain failures without promising physical GPU
coverage. Affected Intel and consumer NVIDIA machines, the actual DAW/plugin
build, and OS postmortem evidence are still needed to establish the reported
driver-dependent crash cause.
