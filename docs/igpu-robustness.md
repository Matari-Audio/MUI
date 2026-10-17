# Integrated GPU robustness (implementation, not hardware qualification)

Baseline: `9d1059cc` (`fix/gpu-crash-fallback`). The field report has no logs;
none of these mechanisms establishes the cause of that particular crash.
Coordinator owns all builds, tests and real-device qualification.

## Before-change contracts

| Path / format | Thread and overlap | Owner: create → adopt/reject → retire/drop | Bounds / overload / recovery | Distinguishing proof |
|---|---|---|---|---|
| Native editor startup, CLAP/VST3/AU/standalone | GPU init worker; multiple editors may initialize concurrently | Session owns instance/device; each candidate owns pending trial and pipelines until accepted; rejected resources drop on initializer | Finite backend/adapter list; unsupported limits/formats reject before device request; next backend then CPU | Headless ranking/limit/candidate tests; real Windows Intel first-frame journal |
| Vello and MUI pipeline creation, every GPU consumer | Init workers; resize/presenter setup may overlap other editors | Shared device owns shader cache; renderer owns pipelines | DX12 compilation serialized across MUI/Vello pipelines; checked shaders and workgroup zeroing | Headless shader-source regression; GNU release FXC and MSVC DXC runs |
| Surface configure/resize/present, native hosts | Window callback; rendering worker where supported; never audio callback | Host owns surface and matching config; replacement validates anew | Zero size skipped; caps select format/alpha/present; lost device rebuild or CPU, lost surface recreate | Surface-selection tests; native_editor smoke and forced loss/reopen |
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
- **Checked shaders on every GPU.** Vello 0.11's unchecked creation and disabled
  zero-init are real in the baseline. The safe wgpu 30.0.1 API enables
  [runtime checks](https://docs.rs/wgpu/30.0.1/wgpu/struct.ShaderRuntimeChecks.html)
  for bounds, loops and integer division. Compute pipeline options explicitly
  [zero workgroup memory](https://docs.rs/wgpu/30.0.1/wgpu/struct.PipelineCompilationOptions.html).
  Choosing safety globally covers AMD APUs and misclassified adapters and avoids
  creating an unchecked configuration to maintain. This reverses the optimization
  in [Vello PR 1093](https://github.com/linebender/vello/pull/1093).
- **Validate before request_device.** A small host selection module validates
  Vello area-AA metadata requirements, compute support and internal RGBA/BGRA
  texture usages. The current coarse shader needs 8 storage buffers, 256
  invocations and 16 KiB workgroup storage. wgpu buffer-size defaults remain
  minimums; supported headroom is requested only after all limits pass
  `check_limits_with_fail_fn`. Unusable candidates go through the existing
  finite adapter/backend loop, then CPU. No optional timestamp, subgroup,
  float-format or MSAA feature is requested; MUI compiles area AA only.
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
  them to pipeline callers. All MUI/Vello DX12 pipeline construction now shares
  one compilation gate per plugin image, and Vello compiles on one DX12 lane.
  [Vello issue 1991](https://github.com/linebender/vello/issues/1991) additionally
  reports an optimized FXC tile-allocation hang. The locked registry shader
  contains both its storage broadcast and a missing workgroup-memory barrier.
  A version-checked generated-source patch adds workgroup ordering on every API
  and uses the already-declared shared scalar on DX12; Metal/Vulkan keep their
  historic storage broadcast workaround. Runtime checks alone do not resolve
  synchronization races. These reports are leads plus source confirmation,
  not measurements made here. GNU remains FXC unless existing dynamic-DXC
  discovery/override succeeds; MSVC retains static DXC. Moving the Windows
  release to MSVC + DXC remains a separate task.
- **Actionable journal.** `candidate_considered`, `candidate_rejected`,
  `surface_rejected`, `gpu_ready` record adapter name, hex vendor/device IDs,
  backend, device type, driver and driver_info, followed by a separate reason.
  This avoids the 2 KiB message bound truncating a failure behind a full limits
  dump. `backend_policy` records compiler, enabled APIs and instance flags.
  The existing log root remains `%LOCALAPPDATA%\MUI\session-*.log`.

## Cost and other iGPU checks

Bounds checks can add comparisons/control flow per indexed access; loop checks
add counters to unbounded loops. Zeroing adds work proportional to live workgroup
storage, approximately 4,096 u32 stores per coarse workgroup (16 KiB), plus
compiler-dependent initialization barriers. Tile allocation adds workgroup
ordering; its shared-scalar broadcast is DX12-only. This is a source-level cost
estimate, **not a measured percentage or guaranteed generated-instruction count**;
the driver may optimize redundant work. Measure baseline/candidate GPU time,
first-open time and active-frame latency on the same Intel/AMD scene. DX12's
single compilation lane can increase cold-open time; cached/steady rendering
has no compilation lock. Automatic damage, idle rendering and monitor pacing
remain intact, with no frame cap.

Readback uses a separate COPY_DST/MAP_READ staging buffer, drops mapped views
before unmap, and has a bounded two-second wait plus a bitmap-size budget; it
does not request MAPPABLE_PRIMARY_BUFFERS or assume discrete VRAM. The inspected
production path requests only optional PIPELINE_CACHE when offered, with no
mandatory timestamp queries, MSAA textures or subgroup capabilities. RGBA
compute targets and BGRA readback/presentation usage are now checked before
creating the device. No additional shared-memory-specific bug was established.

## Checks and remaining qualification

Implementation-agent checks: source review, standalone `rustfmt` and
`git diff --check` only. **No cargo, rustc, clippy, builds or tests were run.**
Cargo.lock and dependency versions are unchanged. Added headless tests cover
Intel/AMD/NVIDIA ranking on simulated Windows, explicit backend allowlists,
unsupported storage/workgroup limits, offered surface modes and rejection,
and the actual locked tile shader patch. The explicit-initializer test also
parses and validates all 22 shipped area/MSAA shader variants with Naga.

Coordinator commands and the final branch/commit are in
`/mnt/Windows11/DEV_WORKSPACE/BuildScratch/mui-igpu-handoff.md`.

Still required: real Windows 10 Intel Gen9/Gen11 and AMD APU GPU-rendered
first-frame completion, resize, animation, multiple editors, close/reopen,
device reset and forced surface loss; optimized GNU FXC and native MSVC DXC
artifacts; Vulkan-only fallback; Metal/Linux rendering correctness after the
additional barrier; comparison against CPU pixels and prior frame timing.
A Linux cross-check cannot establish Windows driver behavior. No artifact hash
or hardware receipt exists from this implementation agent. Obtain the original
user's session log and adapter/driver identity before attributing the crash.
