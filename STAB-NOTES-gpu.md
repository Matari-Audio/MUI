# GPU stabilization notes

Branch: `stab/gpu`. Worktree: `/home/derpcat/.t3/worktrees/MUI/stab-gpu`.
Validation date: 2026-10-10. All Cargo invocations ran under
`flock /tmp/mui-cargo.lock`; no other stabilization branch was merged here.

## Implemented

- Native editors submit CPU pixels before starting GPU initialization. A successful
  image pin permits a named `mui-gpu-init` worker. A failed pin selects the explicit
  synchronous fallback **after CPU submission**, with a diagnostic breadcrumb.
- The loaded MUI image shares an editor-owned device, queue, Vello renderer, material
  pipelines and RGBA/BGRA presentation pipelines. The static registry is a `Weak`,
  not a permanent GPU owner. Different plugin images do not share Rust statics.
- Each window keeps its own surface, retained scene, targets, uniforms and effect
  budget. Workers never capture a native window, borrowed display or surface.
  Surface creation, configuration, replacement and destruction stay on the window
  thread. Renderer contention skips a frame rather than waiting for another editor.
- Device loss invalidates the shared generation. One rebuild is admitted at a time;
  observers adopt the rebuilt context. A window that cannot use the shared adapter
  tries private candidates without poisoning other windows. Exhausted surface
  capability rejections select CPU; temporary configuration failures can retry.
- Transient retries start at 250 ms, double, and cap at 8 s. Permanent failures stay
  disabled until reset/reopen. Skipped acquisition and surface replacement are paced.
  Successful presentation clears retry state; inactive CPU/GPU deadlines are cleared.
- Native Requests publish atomics before calling the optional `FrameRequester`.
  GPU completion needs 25 ms polling while pending, not worker-to-window callbacks.
  Standalone windows retain a named 250 ms model heartbeat: `View::changed()` is
  polling-based. Push-based model notification is the eventual replacement.
- A target above 1 MP uses a background-only startup frame instead of full CPU
  rasterization while GPU initialization is pending. Permanent CPU fallback returns
  to full quality within the 16 MP RGBA budget. Above that budget it remains an
  explicitly diagnosed background frame, and shrinking restores full CPU quality.
  Startup overlays wait for the full renderer; native presentation buffers still
  consume memory. This is not a total process-memory bound.
- Optional reporting pins the native image before starting its worker. Pin failure
  returns an actionable error and starts no worker. Final reporter release cancels
  and drops the thread handle; it never joins the GUI. A bounded in-flight delivery
  can finish after the last editor, without resuming into unmapped code.
- `mui-baseview` rejects `panic=abort` unless `allow-panic-abort` is enabled. The
  opt-out explicitly accepts host termination on panic.

## First-open trace

1. The baseview fork waits for real MapNotify/Expose, rather than treating a checked
   MapWindow request as proof of viewability. Dependency commit `0d765cf1` was
   cherry-picked here as `719e7df8`.
2. `crates/mui-baseview/src/lib.rs:414-618` reads the current native size/DPI, creates
   the CPU presenter, resolves the CPU scene and submits pixels. It starts `GpuInit`
   only after `present()` reports an actual submission; see `:710-715` and its gate
   regression test. Successful CPU submission is recorded in Requests presentation
   evidence and the `cpu_frame_presented` breadcrumb.
3. `crates/mui-vello/src/host/initialization.rs:8-178` finds or builds the shared
   context. `:244-393` prepares per-window resources, then invokes the caller's
   surface callback. No surface/native handle enters either worker stage.
4. The window thread drops the CPU presenter before installing the GPU surface,
   switches GPU welding availability, resolves the current scene and presents it.
   Polling pending work does not join an initializer. Closing drops its receiver
   and strong editor ownership.
5. `crates/mui-winit/src/gpu.rs:60-111` polls before scene resolution; direct callers
   also progress through `present()`. Installation requests a redraw so a completion
   between resolve and submit cannot leave a CPU-lowered material scene retained.
   The production preview calls `update()` before resolving and includes
   `next_wake()` in event-loop scheduling.

A submit return/breadcrumb is **not** a scanout timestamp. The native pixel fixtures
provide stronger visible-pixel evidence for the tested Linux paths. PIE test
executables cannot be dlopened by the fork's pin helper, so native initializer
runs here exercised the documented pin-false synchronous path. A separate blocked
worker test exercises cancellation/drop without a join; it is not plugin-loader
or native-driver proof for the pin-true path.

## Native display and macOS layer ownership

- A shared instance intentionally has no borrowed display. Winit must use the safe
  `create_surface(window.clone())` display-and-window target, not
  `from_window_without_display`. Native testing caught the latter's missing display
  handle; both standalone windows now reach GPU presentation on X11.
- The macOS fork owns flipped-parent top anchoring and installs the root
  CAMetalLayer before insertion. The old per-tick `reanchor_to_superview_top` call
  and helper were removed; they could undo correct native anchoring.
- Softbuffer 0.4.8 `src/backends/cg.rs:170-173` adds a CPU CALayer sublayer, but
  `:115-125` removes only KVO observers on Drop. Merely dropping its surface would
  leave a CPU image covering the Metal root. `software/macos.rs` snapshots children
  around construction, retains only newly added layers, then removes them before
  softbuffer observer teardown. It never removes the root or existing siblings.
  Its non-Send marker keeps cleanup on the owning native thread. This code is
  cross-checked, **not** Apple runtime-verified.

## Windows / cache policy

`host.rs:59-83` uses opt-in `MUI_GPU_DEBUG=1`, the configured `WGPU_BACKEND`, static
DXC, `DxgiFromHwnd` presentation and `Dx12UseFrameLatencyWaitableObject::DontWait`.
An available explicit opaque alpha mode is preferred. HWND flip-model presentation
avoids relying on a transparent child-window composition path; no DX12 runtime
claim is made from the cross-check.

Workspace wgpu enables `static-dxc`. Cargo.lock pins
`mach-dxcompiler-rs 0.1.6+2026.09.16-48d5a66.1` and `lhash 1.1.0`. The DXC build
fetches a checksummed, target-specific native archive. It adds build/download and
binary-size cost. A cold/offline builder must have that archive available. No final
plugin DLL was linked or measured here, so release size, CRT/link compatibility and
Windows redistribution packaging remain native release validation tasks.

`host/pipeline_cache.rs:34-119` uses the adapter/device cache key, a versioned filename,
a checksummed envelope and a 64 MiB payload limit. Missing, corrupt or incompatible
cache data is ignored; wgpu uses `fallback: true`. Writes use unique temporary files
and rename. Cache support is optional; unsupported backends still work. Corruption
and truncation policy tests pass. Cross-process/cold-vs-warm performance and Windows
cache replacement were not measured. The shared worker path uses PRIMARY backends;
legacy synchronous Host APIs retain their native GL path. No GL-only shared-context
runtime claim is made.

## Tests and checks

Logs are retained under `/tmp/mui-audit/`; they are environment-local evidence, not
repository artifacts. Native failures are listed rather than reported as passes.

| Command / case | Result | Log |
|---|---|---|
| `cargo test -p mui-vello -p mui-baseview -p mui-winit` | Pass: baseview 18; vello 60 library, 3 capture, 19 effects; winit 14. Native ignored tests excluded. | `gpu-tests-default-final.log` |
| Vello library with `gpu-effects,software-window,reporting` | Pass: 66; 3 native ignored. Includes reporter pin refusal/lifetime and blocked initializer close tests. | `gpu-tests-reporting-final.log` |
| Three owned crates plus preview, clippy all-targets with reporting | Pass; no owned-code warnings. Existing xim lifetime warnings remain. | `gpu-clippy.log` |
| Three owned crates, Windows MSVC all-targets check | Pass. Static DXC dependency resolves/builds for check. No DLL link/run. | `gpu-check-windows.log` |
| Three owned crates, aarch64 macOS all-targets check | Pass, including the CPU layer helper. | `gpu-check-macos-default.log` |
| Three owned crates, macOS all-targets clippy | Pass after Linux-only fixture helper cfg fix. | `gpu-clippy-macos.log` |
| macOS check with reporting enabled | Blocked by ring's C build: GNU cc does not accept Darwin `-arch`, deployment-target and `-gfull` flags. Default-feature cross-check is unaffected. | `gpu-check-macos.log` |
| Vello ignored native device/generation/replacement tests, Vulkan | Pass: 3/3 on the RX 6600 host. | `gpu-native-vello-final.log` |
| Baseview full ignored suite, `DISPLAY=:0`, Vulkan | 6/7 pass. GPU recovery, GPU surface reopen, CPU fallback and destroyed-parent cases pass. A11y move/reparent fixture fails under the real WM. | `gpu-native-display-final.log` |
| Baseview full ignored suite, isolated `xvfb-run -a`, Vulkan | 5/7 pass. GPU-present/recovery fixtures cannot establish a Vulkan-presentable adapter: Xvfb has no DRI3, and this host has only RADV ICDs, not lavapipe. CPU/teardown/a11y cases pass. | `gpu-native-xvfb-final.log` |
| Winit two-window GPU smoke, `:0` with Wayland env unset | One run passed; the final rerun times out on WM-constrained resize. Both windows present GPU frames. Do not treat the entire real-WM lifecycle fixture as reliably passing. | `gpu-native-winit-x11-display.log`, `gpu-native-winit-x11-final.log` |
| Winit two-window forced-CPU smoke, isolated Xvfb with Wayland env unset | Pass: 1/1; both windows present, resize, present again and close. | `gpu-native-winit-xvfb-cpu-final.log` |

For a real isolated X11 winit run, unset `WAYLAND_DISPLAY` and `WAYLAND_SOCKET`;
changing DISPLAY alone can leave winit on the existing Wayland compositor. The
native example labels required CPU startup submissions `Frame::Startup`, and waits
for GPU submissions before advancing the GPU-evidence resize/close assertions.
Forced CPU runs still assert CPU presentation throughout.

### Device-loss/resize fixture diagnosis

The original destroyed-device check assumed an immediate lost callback. The fixture
now polls for that callback with a bounded deadline. A GPU-to-CPU transition also
queued softbuffer writes on baseview's borrowed X connection, then blocked in
GetImage before the callback returned. Baseview flushes **after** on_frame; the
readback therefore saw black despite a successful CPU submission. The fixture now
returns after CPU present and reads back on the following callback. It also verifies
the acknowledged native extent: this XWayland WM can constrain requested 200x160 to
200x161. It checks opaque green bottom-edge pixels, Expose re-presentation and the
rebuilt GPU generation, twice through close/reopen. These checks pass on `:0`.

The remaining :0 a11y move/reparent assertion is the X11 worker's isolated-display
case. The winit real-WM smoke can ignore the requested resize when tiled, so its
watchdog can fail despite both successful GPU handovers. Neither failure is hidden
by relaxing renderer evidence or changing production WM behavior.

## Verified / NOT verified

**Verified:** policy/classification/backoff, CPU-before-init gate, atomics-before-wake,
lock-free idle seam, cache corruption rejection, no-join pending-work close, shared
context identity/generation adoption and final owned-context release, native Linux
GPU recovery/CPU pixel visibility, native CPU resize/reopen, renderer smoke tests,
owned-code clippy, and default-feature Rust cross-checks for Windows/macOS.

**NOT verified:** native Windows or Apple rendering/DAW behavior; root Metal/CPU layer
handover at runtime; production pin-true plugin unload with a wedged native driver;
all GPU/driver allocations disappearing immediately at editor close; cold/warm
cache speedup or final static-DXC binary/link size; uniform surface compatibility
across hosts; a fully passing real-WM native winit lifecycle suite; Vulkan GPU
presentation under this Xvfb installation. No browser build was executed here;
reporting's baseview dependency is optional, native-target-only, and excluded from
WASM source paths.

## Coordinator integration

- Merge this branch into the already integrated platform branch; do not merge that
  branch here. Dependency picks: `79a65405` (API), `5be2ca6d` (X11 requester), and
  `719e7df8` (real MapNotify).
- Wire the GPU-INIT-DEMAND marker to `gpu_init.pending()` / `deadline()`: 25 ms while
  pending, capped retry deadline otherwise, plus the 250 ms model/a11y heartbeat.
  `gpu_retry_at` carries adapter acquisition backoff. Common frame_requester platform
  dispatch belongs to the already integrated platform work, not this branch.
- Forward `mui-truce/allow-panic-abort` to `mui-baseview/allow-panic-abort`. No truce
  vendoring or deferred-queue default behavior was changed here. Requests::on_idle is
  only an optional seam; HostPump remains externally flushed and opt-in.
- A small standalone image-pin crate would avoid the renderer's optional reporting
  dependency on the whole native window crate. No such restructuring was done here.
