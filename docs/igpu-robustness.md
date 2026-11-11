# Integrated GPU robustness (implementation, not hardware qualification)

Rebase baseline: `fcada6a9` (hosted UI suite, PR #140). Original implementation
baseline: `9d1059cc` (`fix/gpu-crash-fallback`). The field report has no logs;
none of these mechanisms establishes the cause of that particular crash.
Coordinator owns local builds and tests. Implementation validation runs only on
public MUI hosted runners; real-device qualification remains separate.

**11 November 2026: the GPU renderer is now the quad pass** (`mui_vello::quad`
behind `effects::GpuRenderer`): render pipelines only, no compute shaders, no
storage buffers, no workgroup memory. What below was only true of classic
Vello, since deleted (checked compute shaders, workgroup zeroing, the tile-allocation patch,
the coarse shader's 8 storage buffers / 256 invocations / 16 KiB workgroup
storage) is gone with it. Adapter selection now asks WebGL2-class limits at
the adapter's own texture size and the texture usages the pass creates
(`host/selection.rs`); compute is no longer required. Everything else here
holds: DX12-first backend order, the WARP gate, the crash guard, the pipeline
cache, DX12 pipeline creation serialised per plugin image (now MUI's own
gate in `effects::gate`), device-loss rebuild, CPU first frame and retry
pacing.

## Before-change contracts

| Path / format | Thread and overlap | Owner: create → adopt/reject → retire/drop | Bounds / overload / recovery | Distinguishing proof |
|---|---|---|---|---|
| Native editor startup, CLAP/VST3/AU/standalone | GPU init worker; multiple editors may initialize concurrently | Session owns instance/device; each candidate owns pending trial and pipelines until accepted; rejected resources drop on initializer | Finite backend/adapter list; unsupported limits/formats reject before device request; next backend then CPU | Headless ranking/limit/candidate tests; real Windows Intel first-frame journal |
| MUI pipeline creation, every GPU consumer | Init workers; resize/presenter setup may overlap other editors | Shared device owns shader cache; renderer owns pipelines | DX12 compilation serialized across MUI pipelines | Naga downlevel validation of every shader; GNU release FXC and MSVC DXC runs |
| Surface configure/resize/present, native hosts | Window callback; rendering worker where supported; never audio callback | Host owns surface and matching config; replacement validates anew | Zero size skipped; caps select format/alpha/present; lost device rebuild or CPU, lost surface recreate | Surface-selection tests; native_editor smoke and forced loss/reopen |
| Hosted WARP admission | GPU initialization worker before device creation | Selection rejects CPU DX12 Microsoft adapters before requesting a device unless both test gates pass | Requires compiled `test-support` AND `MUI_TEST_ALLOW_WARP=1`; production cannot opt in | Pure admission policy test in `host::selection::tests`, plus hosted StaticDxc/FXC presentation |
| Offscreen readback, Windows native CPU presentation of GPU-rendered frames | Editor render worker | Offscreen owns COPY_DST/MAP_READ buffer; map view drops before unmap | Bounded bitmap dimensions and map deadline; errors return to existing CPU recovery | Intel/AMD shared-memory resize/damage/readback runs |

Audio execution paths, parameter IDs and session state are unchanged. Rendering
retains automatic damage and existing display pacing; no frame cap is added.

## Changes and evidence (reviewed 2026-10-17)

- **Windows DX12 first, all vendors.** Both lazy backend startup and the stable
  adapter rank prefer DX12 to Vulkan. Initializing Vulkan merely to discover
  the vendor can itself crash before selection; applying the native-API policy
  to all vendors avoids that risk for Intel and AMD APUs. This is a policy
  inference, not evidence that every DX12 driver is better. Intel Vulkan stack
  problems are documented by a wgpu maintainer in
  [wgpu discussion 1544](https://github.com/gfx-rs/wgpu/discussions/1544).
  Existing 8 MiB stacks remain. `WGPU_BACKEND` stays an allowlist, including
  explicit Vulkan-only selection. Non-Windows order is unchanged.
- **Validate before request_device.** A small host selection module checks
  the limits the quad pass needs (15 vertex attributes and inter-stage
  variables, a 240-byte instance stride, 6 KiB uniforms: all inside
  `Limits::downlevel_webgl2_defaults`, requested at the adapter's own
  texture size and alignment) and the texture usages it creates (RGBA8
  render/sample/copy, R8 atlas, RGBA32F path records, BGRA8
  render/readback), with `check_limits_with_fail_fn`. Unusable candidates go
  through the existing finite adapter/backend loop, then CPU. No optional
  timestamp, subgroup, float-format or MSAA feature is requested.
- **Shared/direct surface parity.** Both initialization paths use the same pure
  [capability](https://docs.rs/wgpu/30.0.1/wgpu/struct.SurfaceCapabilities.html)
  selection policy. Empty modes/usages or lack of non-sRGB UNORM rejects the
  candidate. Alpha/present selections use offered concrete values; mailbox or
  immediate precedes FIFO variants. Existing device-loss rebuild, surface-loss
  recreation and worker CPU recovery remain; capability failure now also logs
  the rejected shared adapter and reason.
- **GNU/FXC needs real fixes too.**
  [wgpu issue 10547](https://github.com/gfx-rs/wgpu/issues/10547) reports a
  non-atomic FXC blob reference-count race in wgpu 30.0.1. The locked local
  `wgpu-hal` cache indeed clones shader blobs under its cache lock and returns
  them to pipeline callers. All MUI DX12 pipeline construction shares one
  compilation gate per plugin image, on the dedicated 8 MiB worker (unless
  explicitly requested inline). This report is a lead plus source
  confirmation, not a measurement made here. GNU remains FXC unless existing dynamic-DXC
  discovery/override succeeds; MSVC retains static DXC. Moving the Windows
  release to MSVC + DXC remains a separate task.
- **Actionable journal.** `candidate_considered`, `candidate_rejected`,
  `surface_rejected`, `gpu_ready` record adapter name, hex vendor/device IDs,
  backend, device type, driver and driver_info, followed by a separate reason.
  This avoids the 2 KiB message bound truncating a failure behind a full limits
  dump. `backend_policy` records compiler, enabled APIs and instance flags.
  The existing log root remains `%LOCALAPPDATA%\MUI\session-*.log`.

## Cost and other iGPU checks

The quad pass's own costs are measured in `BENCHMARKS.md` on one discrete
GPU only. DX12's single compilation lane can increase cold-open time;
cached/steady rendering has no compilation lock. Automatic damage, idle
rendering and monitor pacing remain intact, with no frame cap.

Readback uses a separate COPY_DST/MAP_READ staging buffer, drops mapped views
before unmap, and has a bounded two-second wait plus a bitmap-size budget; it
does not request MAPPABLE_PRIMARY_BUFFERS or assume discrete VRAM. The inspected
production path requests only optional PIPELINE_CACHE when offered, with no
mandatory timestamp queries, MSAA textures or subgroup capabilities. The
texture usages the renderer creates are checked before creating the device. No additional shared-memory-specific bug was established.

## Checks and remaining qualification

Local implementation-agent checks: source review, standalone `rustfmt` and
`git diff --check` only. **No local Cargo, rustc, Clippy, builds or tests run.**
Hosted validation follows [the CI matrix](ci.md). The unfiltered
`cargo test --workspace --all-features` in Linux root-test and both
native-platform jobs executes `host::selection::tests` and
`host::tests::windows_tries_dx12_before_loading_vulkan_and_honors_backend_override`
on all three OSes. The simulated Windows ordering
test deliberately has no Windows cfg gate. WARP policy tests pass an explicit
boolean without changing the process environment; actual admission still
requires both the feature and environment gates.

The rebase preserves cached output/presented-view release before surface resize,
replacement and device reopening, plus material-only retained redraw.
`gpu_contract`, `gpu_pixels` and native lifecycle jobs exercise these. Pixel tolerances
remain those established by #140; shader safety changes do not justify widening
them without inspecting the rendered differences.
Cargo.lock and dependency versions are unchanged. Added headless tests cover
Intel/AMD/NVIDIA ranking on simulated Windows, explicit backend allowlists,
short WebGL2-class limits, offered surface modes and rejection; Naga
validates every shader the renderer builds for a GLES 3 device.

Coordinator commands and the final branch/commit are in
`/mnt/Windows11/DEV_WORKSPACE/BuildScratch/mui-igpu-rebase-handoff.md`.

Still required: real Windows 10 Intel Gen9/Gen11 and AMD APU GPU-rendered
first-frame completion, resize, animation, multiple editors, close/reopen,
device reset and forced surface loss; optimized GNU FXC and native MSVC DXC
artifacts; real-device Vulkan-only fallback; hardware Metal/Linux rendering
correctness of the quad pass; same-device comparison of GPU time,
cold-open and active-frame latency. Hosted software/VM GPU receipts establish
only the correctness and timing actually measured there.
A Linux cross-check cannot establish Windows driver behavior. Hosted artifact
hashes and receipts are recorded in the handoff; no real-device hardware
receipt exists from this implementation agent. Obtain the original
user's session log and adapter/driver identity before attributing the crash.
