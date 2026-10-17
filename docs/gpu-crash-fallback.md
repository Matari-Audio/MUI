# GPU startup crash fallback

Native driver faults (including access violations and stack overflow) cannot be
caught by Rust unwinding. A first encounter can still terminate the DAW. MUI now
leaves durable evidence before native GPU startup and uses it on the next launch
to select another backend/adapter, or retain the CPU presenter if none remain.
This is a recovery boundary, not a diagnosis of the reported Intel crash.

## Trial lifetime and selection

`<diagnostics directory>/gpu-trials/` contains one `.pending` file per attempt.
The directory is `%LOCALAPPDATA%\MUI` on Windows,
`~/Library/Logs/MUI` on macOS, and `$XDG_STATE_HOME/MUI` (or
`~/.local/state/MUI`) on Linux. `MUI_DIAGNOSTICS_DIR` overrides the root.
The filename includes a deterministic identity hash, PID, timestamp and collision
suffix; the content holds the complete identity, so hash collisions cannot
disable another GPU.

MUI creates and syncs the file before touching the native API. The owning attempt
retains an exclusive OS file lock. Startup takes a short registry lock while
scanning/creating files; no registry lock crosses a GPU call. A locked marker is
a live attempt and is ignored, even in another plugin image or another DAW.
An unlocked marker is an unfinished attempt from a dead owner. This uses kernel
liveness rather than testing PIDs, avoiding PID reuse and same-process instance
races. Readers and close/unlink transactions use the same registry lock.
Unsupported locking or filesystem errors reject GPU initialization safely.

Native APIs are created separately, preserving `WGPU_BACKEND` as an allowlist:
Metal, DX12/Vulkan (DX12 first on Windows), browser WebGPU, GL, and the
explicit test/noop backend. See [integrated GPU robustness](igpu-robustness.md).
Compute-only shared sessions exclude GL/noop.
Before instance creation/enumeration there is no adapter identity available, so
that short phase uses a **backend-only** key. Once enumeration and adapter-info
queries finish, that marker is retired. Each adapter trial uses backend, vendor
ID, device ID, `driver` and `driver_info` (wgpu's driver/version strings), before
capability queries, device requests and renderer/pipeline creation.

The shared context carries this trial into editor resource creation. A per-editor
trial takes over before creating its renderer; concurrent editors have separate
markers. Successful offscreen render **and completed readback**, including device
error checks, clear that editor's marker. Surface presenters clear theirs in the
submitted-work completion callback after presenting. Success never removes
another attempt's marker or an existing denial. Rust errors/unwinds and normal
renderer destruction remove their own pending marker.

A different reported driver/version creates a new identity and automatically
allows retry. Old denials remain relevant if that driver is installed again.
An enumeration/loader crash cannot identify a driver version: its backend-wide
denial needs the override or removal of that backend marker after a driver
update. Likewise, MUI cannot distinguish driver updates if the backend reports
identical version strings. A process exiting before an initialized shared context
is adopted or a first trial completes is conservatively treated as unfinished;
a marker alone is not proof of a driver defect.

Set `MUI_GPU_IGNORE_DENYLIST=1` before launching the DAW to ignore stale trials.
The override preserves the evidence and still records new attempts. To reset a
denial permanently, close DAWs using MUI and remove the corresponding stale
`.pending` file. `MUI_RENDERER=cpu` remains the unconditional GPU bypass. Already
created, externally supplied `wgpu::Instance`s cannot have their creation guarded
by MUI; their per-backend enumeration and adapter initialization are guarded.
Browser persistence remains owned by the browser, without native file sentinels.

## Panic recovery, threads and diagnostics

The native editor render worker catches GPU render/readback panics locally.
Both returned errors and panics drop its GPU, mark GPU welding unavailable,
request a fresh frame and invoke the existing initialization backoff. The worker
continues receiving input and painting with the CPU renderer. GPU destruction is
also protected from a second Rust unwind. Per-editor offscreen initialization has
its own unwind boundary. No process-wide panic/signal/exception hook is installed.
Switching presenters invalidates the old CPU bitmap once, since it predates the
GPU frames currently on screen; subsequent CPU frames use normal local damage.

`mui-editor-render`, `mui-gpu-init` and Vello shader compilation workers reserve
explicit 8 MiB stacks on every native OS. The editor worker owns rendering,
readback, panic recovery and CPU fallback. Initialization workers own adapter,
device and shared pipeline creation. Host callbacks and the audio callback gain
no GPU work. Existing plugin image pinning and process-lifetime sessions remain.
Backend/candidate iteration is finite; filename collision retries stop after 16
collisions and fall back to CPU. Trial records are retained until explicitly
cleared or their own attempt succeeds; there is no per-frame journal/file work
after a renderer's first completed GPU frame.

The flushed journal phases are `enumerate`, `request_device`, `create_renderer`
and `first_render`, each with backend and available adapter/driver identity.
`gpu_denylist` records skips and overrides; `gpu_sentinel` reports storage failures;
`worker_gpu_fault` records caught render panics and ordinary GPU errors.
The existing unbuffered journal writes are flushed before each native phase;
trial contents use `sync_all`. This protects process termination, not a guarantee
against power loss or filesystem corruption.

## Orchestrator validation

No compilation or test execution was performed by the implementation agent.
Run serially from the worktree:

```sh
cargo fmt --all -- --check
cargo test -p mui-vello --features cpu,gpu-effects --lib host::crash_guard::tests
cargo test -p mui-baseview --lib native_worker::gpu_recovery_tests
cargo test -p mui-vello --features cpu,gpu-effects --lib host::
cargo test -p mui-baseview -p mui-truce --lib
cargo clippy -p mui-vello --features cpu,gpu-effects --all-targets -- -D warnings
cargo clippy -p mui-baseview -p mui-truce --all-targets -- -D warnings
cargo clippy --target x86_64-pc-windows-gnu -p mui-baseview -p mui-truce --all-targets -- -D warnings
```

Required: stale trials skip only their backend/adapter; live locked trials are
ignored; driver changes allow retry; all denied backends leave CPU; the override
works without deleting denials; an injected render panic falls back to CPU and
the same worker processes subsequent frames. Existing host/session tests and
warnings gates must pass.

Real Windows validation still needs the reported Windows 10 Intel adapter, its
driver version, the artifact hash, DAW and journal. Confirm that the first native
fault leaves a matching marker, the next DAW launch tries DX12 after a Vulkan
fault (or CPU if both failed), multiple DAWs do not mistake live trials for
crashes, and healthy discrete GPUs retain normal rendering. Repeat startup,
close/reopen and forced CPU on Windows, macOS and Linux. Cross-checking Windows
tests on Linux cannot execute their Win32 paths; run those binaries on Windows.
