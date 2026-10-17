# Native editor workers, display pacing and automatic damage

Decision: 2026-10-17. This implements Windows **Tier 1: offscreen worker +
bitmap blit to the host-owned HWND**. It also shares worker isolation and damage
policy with MUI's native AppKit and X11 adapters. Changes stay in MUI's vendored
backend; the owner-approved `mui-host`/`mui-plugin` extraction is a later migration.

## Why this ownership model

The host UI thread keeps the Windows child HWND, input queue, native accessibility
provider, IME and window operations. One editor worker owns model polling, ordered
input dispatch, tree construction, layout, scene snapshots, CPU raster, GPU
encoding/submission and GPU readback. The worker receives no native handle.
A process-image GPU initializer compiles pipelines separately; CPU frames can
finish while it initializes, at every demanded display refresh, without throttling.

A worker-created child HWND was considered and rejected. Cross-thread parent/child
relationships implicitly attach input queues; synchronous messages can propagate
a stalled window thread into the DAW. Moving only a swapchain to a worker also
requires a native lifetime lease: the host can destroy the parent while a graphics
call stalls, and waiting for that lease in WM_DESTROY would recreate the freeze.
Tier 1 avoids both dependencies: render to persistent window-independent textures,
read back only completed damage on the worker, and give the native endpoint owned
bitmap patches. CPU fallback uses the same transport.

`BitmapPatch { rect, pixels }` and the native presenter form the presentation seam.
A future feature-gated DirectComposition transport can replace it without moving
model work or HWND ownership. That spike must first prove nonblocking parent
destruction and pass REAPER, Bitwig, Ableton and FL Studio; it is not enabled here.
GDI blits and compositor transactions remain native calls. Their wall time must
be measured: this removes native-thread shader/raster/GPU waits, not arbitrary
OS/driver latency inside a bitmap copy.

## Scheduling and ordering

* Native input and resize/DPI events enter one FIFO. Button, wheel, key, focus,
  IME, drop and resize edges are preserved in order. Native callbacks never lock
  the model, run raster/GPU work or join the editor worker.
* Tick requests coalesce. Once queued, native demand parks until the worker
  publishes its next deadline; a stalled worker cannot leave an expired demand
  causing a refresh-rate callback loop. Completed bitmaps use a one-slot mailbox.
* Windows explicit requests, animations and expired `At` deadlines all pass through
  the compositor pacer. `DwmFlush` runs only on that thread. Its fallback queries
  the window monitor's refresh rate; no 8/16 ms cap is imposed. AppKit retains its
  display link, and X11 retains demand scheduling against its queried display
  interval. There is no native render frame cap or CPU fallback throttle.
* Unchanged paint submits no raster, GPU output/readback or native bitmap update.
  Idle rendered frames are zero. The existing `View::changed` compatibility API
  still needs a quiet model heartbeat (250 ms by default, configurable for
  telemetry); this does **not** build or paint an unchanged tree. Eliminating even
  those model polls needs revision-driven notifications in the future `mui-host`
  API. GPU readiness/retry likewise has control polls rather than redraws.
* The native adapter applies cursor, IME, capture and accessibility output after
  the worker releases the model lock. Keyboard capture uses the last completed
  focus state; key hooks run on the worker. Synchronous host accelerator and
  drag/drop accept decisions therefore require the native replay/host matrix
  below, especially click-to-type, piano hooks and rejected drops.

Windows `frame_state.rs` has no Win32 dependencies and is included in Linux test
builds. Tests cover repeated dirty/past deadlines, compositor selection, pending
request coalescing and idle-without-a-deadline. `Driver::display_paced` bypasses its
old animation interval gate; native demand, rather than a fixed millisecond limit,
decides when to advance. The explicit min_interval API remains for other callers.

## Automatic damage: no application invalidation API

MUI's retained tree/layout and weld caches produce a `Painted` snapshot. The
shared `mui-vello/src/damage.rs` compares it to the previous snapshot by stable
node key/layer and exact content/style/path/text/transform equality. Exact
comparison avoids hash collisions; applications need neither hashes nor dirty
rectangle calls. A bounded lookahead recognizes inserted/deleted canvas entries
without damaging every following unchanged sibling.

Each changed element adds both its old and new footprint: path/stroke/text bounds,
transform, shadow reach, and antialiasing padding. Device bounds snap outward to
16-pixel tiles. Touching/overlapping rectangles merge; sparse damage stays separate.
At the 16-rectangle metadata budget, affected boxes coalesce into a conservative
union. There is no area threshold that turns a local change into a full repaint.
A moving/animated node therefore clears its old footprint and paints its new one
without requiring the application to mark it dirty.

Unchanged overlapping backgrounds and siblings must be replayed **within** those
regions to reconstruct blending correctly. Blend/opacity changes include the
contained paint. Backdrop dependencies flow forward in paint order through intersecting old/new
effect footprints: content drawn above a backdrop cannot invalidate its prefix.
The retained GPU backdrop cache holds a complete prefix encoding, so a changed
prefix conservatively invalidates that encoding and repaints the target, matching
its existing contract even if the changed paint lies outside the blur footprint.
Hover/content above unchanged glass reuses its cached blur and stays local.
Analytic weld slots retain material textures/uniform lanes; changed or animated
materials add their old/new bounds to the same classifier. CPU reference welds
produce ordinary changed paint, so CPU and GPU share the same automatic policy.

CPU `Renderer::render_damage` records scene geometry at its original global
coordinates, allocates region-sized scratch targets, and crops the recorded strips
at the coarse/fine raster boundary. It replaces only damaged rows in a persistent
bitmap; the worker converts only those rows for native transport. Translating
curves before flattening into a small target was not bit-exact (one-bit coverage
changes on motion), so `vendor/vello_cpu` exposes the existing nonzero-origin
bucketer through `RasterizerSettings::viewport`. The source is the same 0.3.0
release with this small extension; no dependency versions change. Full renders
keep the upstream path, and the exact partial/full assertions remain unchanged. GPU retained
patch/atlas rasterization updates only affected scene texture regions; persistent
offscreen output uses scissor rectangles with `LoadOp::Load`, and readback copies
only dirty rectangles. Patch-atlas packing is an allocation heuristic; failing to
pack renders those same rectangles separately rather than rendering the window.

Full target invalidation occurs on first frame, resize, DPI/zoom changes, renderer
handover/recovery and global resource/atlas invalidation. Theme changes naturally
diff all affected paint; a changed full-window background legitimately covers the
window. Unknown/nonfinite paint bounds require conservative target invalidation
or an error. Native expose/WM_PAINT repairs retained bitmap regions without a new
scene build/raster. Buffer-age restoration may copy old retained pixels into a
fresh native transport buffer; that is distinct from repainting the scene.

Native Windows blits only changed rectangles. X11 uses softbuffer damage and
buffer-age repair. AppKit uses a retained CoreAnimation bitmap mosaic: new dirty
images replace/split overlapping cropped layers; unchanged layers retain their
CGImages. No AppKit view leaves its native thread. This avoids softbuffer's AppKit
full-image upload on every patch. Direct wgpu-surface clients still need to restore
a newly acquired swapchain image from retained pixels; wgpu does not expose
Present1 dirty rectangles. Their scene rasterization is incremental, but that
surface restoration is not a claim of platform dirty-present support. The separate
winit/Wayland floating runtime is not rewritten into the baseview worker here.

Set `MUI_DEBUG_DAMAGE=1` before launching the host to flash damage borders. Native
Windows/X11 restore clean retained pixels after the diagnostic flash without scene
rasterization. AppKit fades a compositor border layer. Diagnostics can issue that
extra repair; ordinary idle content does not present. Pure CPU-feature tests check
a single-widget color change stays in its padded tiles, motion covers old and new
bounds, and unchanged snapshots yield empty damage. A CPU equivalence test checks
partial pixels against a full render through motion/overlap, backdrops, shifted
inset shadows and full resize/scale invalidation. Inset shadows damage their
containing outline rather than just their offset inverse-shadow path.

## Lifetime, faults and compiler policy

Closing revokes host delivery, cancels publication and drops the native endpoint
without joining rendering. Worker cleanup releases model/UI memo state. Reopening
the same model waits for predecessor cleanup **on the new worker only**; a forever
wedged predecessor can delay that editor's rendering, but does not block the DAW.
Custom adapters must queue model work with `Requests::dispatch_render_work` and
marshal native/host work to a native/main-thread pump rather than model-locking
from close, idle or native-start hooks.

Only callers with a successfully pinned module may start detached native workers.
Admitted `GpuInit` sessions use a strong process-lifetime static reference, retaining
the device and compiled pipelines across editor closes and plugin instances.
Unpinned callers get private synchronous sessions. Pinning keeps code mapped for
late workers/device callbacks, so physical DLL unload is intentionally prevented;
no HWND, borrowed plugin instance or native surface is retained by the service.
A logical unload/reload loader test must verify instance destruction and safe late
completion; it must not expect pinned image memory to be reclaimed.

Surface and non-validation Render faults are transient and retry with backoff;
device loss allows the shared context generation to rebuild. Validation faults
remain permanent for that initializer. There is no process-wide software-only flag
for ordinary mid-session presentation/render faults. CPU fallback rasterization is
always on the editor worker, including startup and GPU recovery.

Windows adapter ranking prefers Vulkan over DX12 within a device class. Explicit
`WGPU_DX12_COMPILER` wins; supported x64 MSVC uses StaticDxc. GNU/other Windows
configurations discover DynamicDxc through `MUI_DXCOMPILER_PATH` (DLL or directory),
then beside the **plugin module**, and otherwise use FXC. This supports GNU today
and the owner's later MSVC-primary migration. No version or target policy is changed. The CPU dependency now uses a local
0.3.0 source copy for the exact strip-crop extension; transitive versions and Git
pins are unchanged.

## Convergence: exactly what moose must change later

Do not copy these patches back to moose. The owner chose MUI as the sole runtime
and adapter owner, with a thin moose shim. No moose files are edited here.

1. During migration, point moose's canonical `baseview` dependency at MUI's tested
   vendored revision (currently package `moose-baseview` 0.4.1-moose.1), replacing
   its `crates/moose-baseview` 0.3.4-moose.1 path dependency. Remove that workspace
   member/copy and the root `[patch."https://github.com/Matari-Audio/moose"]`
   moose-baseview override. That override cannot unify MUI's existing path vendor.
   Pin `mui`/`mui-baseview` together to the tested MUI revision in moose-mui.
2. Move MUI's vendored backend and `mui-baseview` into MUI's future `crates/mui-host`,
   with a compatibility re-export during adoption. Change moose's window imports
   to this one runtime. Delete moose-gui, moose-gpu, moose-cpu, moose-gui-types,
   moose-gui-utils and moose-font as their consumers move to MUI; route standalone
   through the same MUI runtime. Port then remove moose-gui's old WindowHandler
   resize/frame policies and moose-egui's separate editor renderer.
3. Port moose-mui `src/editor.rs` open/close/state_changed/Drop to the Requests
   native-start/render-lifecycle/render-work hooks: host/native callbacks must
   not acquire Shared while a worker can hold it. Bind a host pump before worker
   startup; revoke it before close. Keep plugin instance references out of native
   renderer callbacks. Adopt MUI's future `mui-plugin` PluginHost/ParamHost/HostPump
   and Bridge lifecycle so moose-mui becomes a compatibility shim.
4. **Actual host-main-thread pumping remains wrapper work.** This branch drains
   mui-truce's pump on the native window thread. Windows/AppKit match the normal
   host UI thread; X11 baseview owns a separate native thread, so this alone does
   not establish CLAP/VST3 main-thread affinity. Implement `Editor::idle` with a
   MainThread token and make wrappers actually invoke it: CLAP request_callback /
   on_main_thread (timer/posix-fd support as needed, debug thread-check), VST3's
   existing IRunLoop/shim timer, AU main run loop, and standalone event loop.
   Add bounded `PluginContext::request_main_thread(FnOnce)` delivery; drain
   begin/set/end, resize and dirty notifications there. Never dispatch them from
   the editor worker or treat X11's native thread as the DAW main thread.
5. Gate convergence with one runtime package in `cargo tree -d`, ordered gesture
   and resize tests with thread assertions, and the CLAP/VST3 native matrix on
   all three operating systems. Remove compatibility crates only after consumers
   adopt the shared implementation. No release/version/pin changes happen here.

## Builder commands and release gates

The implementation agent has not compiled or executed tests/probes. The
orchestrator must run these **serially under its existing builder lock**, in this
worktree; do not override CARGO_TARGET_DIR or RUSTC_WRAPPER:

```sh
cargo fmt --all -- --check
cargo test --manifest-path vendor/moose-baseview/Cargo.toml --lib windows_frame_state_tests
cargo test -p mui --lib host::tests
cargo test -p mui-vello --no-default-features --features cpu --lib
cargo test -p mui-vello --features cpu,gpu-effects --lib
cargo test -p mui-baseview -p mui-truce --lib
cargo clippy -p mui-baseview -p mui-truce --all-targets -- -D warnings
cargo check --target x86_64-pc-windows-gnu -p mui-baseview -p mui-truce --all-targets
cargo clippy --target x86_64-pc-windows-gnu -p mui-baseview -p mui-truce --all-targets -- -D warnings
cargo build --target x86_64-pc-windows-gnu -p mui-baseview --examples
xvfb-run -a cargo run -p mui-baseview --example worker_close_probe
xvfb-run -a cargo run -p mui-baseview --example native_editor
MUI_RENDERER=cpu MUI_EXPECT_SOFTWARE=1 xvfb-run -a cargo run -p mui-baseview --example native_editor
cargo run --profile perf -p mui-vello --features cpu,gpu-effects --example bench
```

Run the same final benchmark on baseline `b8799856add3ee77714bb785d29d042ad78d56dc`
in the orchestrator's clean baseline worktree on the same adapter/scale. If one
is not already available, use worktrunk (never copy a repo/target directory):

```sh
wt switch -c validate/mui-editor-baseline --base b8799856add3ee77714bb785d29d042ad78d56dc --no-cd -x bash -- -c 'cargo run --profile perf -p mui-vello --features cpu,gpu-effects --example bench'
```

Require all checks to pass, exact CPU partial/full pixel equivalence, bounded
single-widget damage, and zero raster/submits/blits for an unchanged snapshot.
Investigate repeatable >5% renderer-cost regression on unchanged paths. The micro-
benchmark does not measure native readback or host responsiveness.

Windows cross commands do not execute Windows binaries on Linux. On a **real
Windows desktop** with MSVC available, also run:

```powershell
cargo test -p mui-vello --features cpu,gpu-effects --lib
cargo test -p mui-baseview -p mui-truce --lib
cargo clippy -p mui-baseview -p mui-truce --all-targets -- -D warnings
cargo run -p mui-baseview --example worker_close_probe
cargo run -p mui-baseview --example native_editor
$env:MUI_RENDERER='cpu'; $env:MUI_EXPECT_SOFTWARE='1'
cargo run -p mui-baseview --example native_editor
```

Run the GNU-built `worker_close_probe.exe` and `native_editor.exe` on real Windows
as well when qualifying GNU; Wine is not evidence for these thread/window rules.
On macOS run the native unit/lint commands and the same two cargo-run probes under
a real AppKit desktop. Linux/Xvfb cannot validate AppKit ABI or CoreAnimation.
The close probe must exit 0 after four responsive parent callbacks while the
worker's build is blocked, and close in <100 ms on Windows / <300 ms otherwise.
Its independent watchdog exits 2 on a stalled native loop; baseline Windows should
fail this probe. The editor probe must resize, close and reopen successfully in
both GPU-default and forced-CPU modes.

Before release, qualify REAPER, Bitwig, Ableton and FL Studio in both VST3 and CLAP:
60/144/240 Hz active drags follow display demand, idle produces no rendered frames,
damage flashes stay local, host transport and other windows stay responsive during
an injected slow build/GPU completion, and close/parent destruction never waits for
that work. Measure max native callback time (target <1 ms; injected-stall gate
<5 ms), readback+blit time, frame p50/p99/p99.9 and damage ratio at 1280x800 and 4K.
Also test keyboard shortcuts/piano hooks, click-to-type, IME, focus/capture loss,
a11y, accepted/rejected drops, multi-instance DPI changes, device loss/recovery,
50 close/reopen cycles, and logical unload/reload with late worker completion.

Risks needing real-platform evidence: GPU readback bandwidth, OS bitmap-copy
latency, asynchronous key/drop capture policy, dependency expansion for complex
backdrops/shadows/clips, AppKit retina orientation/cropping and mosaic retention,
and wrapper host-main-thread affinity on X11. The no-wait close design can retain
a wedged worker/model until process exit. These are release gates, not test claims.

## Release note

Native baseview editors render on editor workers with display-paced demand and
automatic shared CPU/GPU damage. Windows uses offscreen targets plus dirty bitmap
blits; X11 uses damage-aware buffers and AppKit retains cropped bitmap layers.
GPU sessions survive reopen, transient runtime faults recover, Vulkan is preferred
on Windows, and DXC discovery supports GNU alongside MSVC. No application region
invalidation is required. DirectComposition and the mui-host/mui-plugin extraction
remain separate migrations.

## Research references

* [Microsoft: cross-thread window relationships attach input queues](https://devblogs.microsoft.com/oldnewthing/20130619-00/?p=4043).
* [Microsoft: SendMessage blocks until the receiver processes it](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendmessage).
* [Microsoft: DestroyWindow destroys child windows](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-destroywindow).
* [wgpu DX12 compiler policy](https://wgpu.rs/doc/src/wgpu_types/backend.rs.html).
* [softbuffer damage presentation contract](https://docs.rs/softbuffer/0.4.8/softbuffer/struct.Buffer.html#method.present_with_damage).
