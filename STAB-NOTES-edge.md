# Stability work package: edge

Branch: `stab/edge`. Base: `stab/gold`. No vendor files changed.

## Changes and bug mapping

- **Bug 2 (DAW crash):** release now unwinds, retaining size optimization,
  LTO, one codegen unit and stripping. The explicit plugin profile remains
  compatible. Unwind metadata costs some binary size; a default shipping
  command that kills the DAW is a worse trade. Size was not measured.
- **Bug 2:** `mui-truce/src/lib.rs` rejects `cfg(panic = "abort")`, unless the
  explicit `allow-panic-abort` feature is selected. This checks the actual
  final strategy, not our dependency profile. Docs explain final-workspace
  profiles and prohibit the opt-out for DAW plugins.
- **Bugs 1/2:** `MuiEditor::open` validates the author's tree before allocating
  a native child, catches construction panics, and detaches/cleans the model
  on failure. Teardown guards the bridge, UI and native window separately;
  failure in one cannot skip the next. Idle, state, size, scale and native
  close paths have guards. Inert size/capability getters contain no fallible
  code. The existing baseview native render/event guards remain in use.
  Truce's `Editor::open` returns `()`: MUI can leave no child, but cannot return
  a format-level attachment failure without a framework API change.
- **Bug 1:** removed `HOST_SCALE`; scale belongs to an editor. Open and late
  scale use the same platform policy. macOS always uses AppKit backing scale;
  Windows honors explicit host scale; X11 defaults to host scale 1. Accepted
  logical resizes survive open. A late scale also replays the latest logical
  size, rather than accepting stale/zero native bounds as its reference.
  Invalid/zero resizes do not replace the last committed size. Recreated
  editors need instance-scale replay from the framework, not another
  editor's scale. Initial authored size/design are nonzero.
- **Bug 2; opt-in infrastructure, NOT a default Linux fix:** `HostPump` queues
  begin/set/end/resize, coalesces sets within each gesture, checks the exact
  opening thread, releases its lock before callbacks, blocks recursive
  flushes, and ends delivered gestures after callback failure/overflow.
  Pending commands are bounded to 4096. Close queues Ends, then `MuiEditor`
  flushes outside the model lock; stale resize requests are canceled on close.
  Retired host/waker captures drop outside the queue lock and under guards.
  Drop revokes without host callbacks. Deferred resize does not claim host
  acceptance: the wrapper must reply through `Editor::set_size`, so a rejected
  asynchronous request cannot resize the child optimistically.
  `set_waker` is a clean-to-pending notification seam (also wakes an already
  pending queue on registration). It must schedule a later host callback,
  never flush or re-enter the editor inline. Register it for every open.
  `Editor::idle` flushes, and an independent `Arc<HostPump>` lets a framework
  flush outside its own editor lock. No queue lock is shared with audio.
- **Bug 2 (global capture lifetime):** added `Ui::release_thread_memos`.
  Memo trees are removed from TLS before their destructors run, preventing a
  RefCell panic when a captured destructor re-enters the table. The truce
  session releases captures on native cancellation and before handing the
  preflight UI to a native rendering thread. Other adapters must release on
  their rendering thread before moving a UI or unloading its image.
- **Bug 2:** invalid/unrepresentable headless global clock values cannot make
  `Duration::from_secs_f64` panic on each frame.
- Updated root README, the truce README and gain-plugin comments, with a
  shipping checklist and explicit remaining limitations.

**Coordinator constraint:** no truce patch/vendoring. Default synchronous
begin/set/end still executes the same host call sequences; deferred delivery
is OFF unless `.with_host_pump()` is selected by a framework with a real pump.
The original 14 tests still pass. `Bridge::context()` is an explicit escape
hatch: direct context mutations bypass the queue and are caller-owned.

## Upstream request for moose/truce

Evidence below is from fetched **truce 6.3.0**, under
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`. These are not MUI
vendor files, and were not edited. Context7 was consulted; the pinned registry
source is authoritative for the shipped version.

1. `truce-core-6.3.0/src/editor.rs:96-110` documents `Editor::idle` on the host
   UI thread. Exhaustive searches of CLAP/VST3 Rust and the VST3 shim find no
   invocation of it. `PluginContext`/`EditorBridge` (`editor.rs:292-337`,
   `411-535`) provide no `request_main_thread` delivery facility. Send+Sync
   on `EditorBridge` is not permission to call every underlying host API
   from an arbitrary thread.
2. `truce-vst3-6.3.0/src/lib.rs:2275-2287` synchronously forwards begin/set/end
   into the shim. `shim/vst3_shim.cpp:2997-3021` immediately invokes
   `IComponentHandler::beginEdit/performEdit/endEdit`, and uses the unsynchronized
   image-global ctx map at `:2990-2995`. On Linux MUI drives bindings from the
   X11 window thread, **outside VST3's host-GUI-thread contract**. Resizing is
   equally synchronous (`lib.rs:2289-2306`, shim `:3071-3085`). Default mode
   still carries this risk, deliberately, to avoid stranding existing edits.
3. CLAP parameter writes (`truce-clap-6.3.0/src/lib.rs:3493-3534`) already use
   the wrapper's GUI event queue plus `request_flush`/`request_callback`.
   Those notifications are thread-safe under CLAP. GUI resize is not:
   `:3535-3546` directly calls the host GUI resize path. The main-thread
   callback at `:784-809` handles rescan/latency but does not pump the editor.
4. **Needed CLAP pump:** deliver `Editor::idle` (or a new `Editor::pump`)
   through host timer-support / posix-fd-support on Linux, or through
   `request_callback -> on_main_thread`. Export a thread-safe wake seam.
   **Needed Linux VST3 pump:** `IRunLoop::registerTimer` or
   `registerEventHandler`, obtained from the host frame/connection context.
   The current shim already registers a restart timer (`shim:1956-1985`);
   extend framework-owned delivery rather than adding a permanent MUI thread.
5. Flush outside both framework editor-cell guards and the MUI model lock.
   `PluginContext::request_resize` documents wrapper-cell re-entry concerns
   (`truce-core editor.rs:516-528`). Merely calling idle under that cell is
   not enough. A retained `HostPump` handle provides a lock-independent seam.
   Provide authoritative asynchronous resize replies through `Editor::set_size`
   when a queued request is accepted. Queue insertion is not host acceptance.
   Coalesce wakes, preserve gesture Ends, revoke/unregister callbacks before
   editor/plugin destruction, and drain/cancel with no work after unload.
6. **CLAP scale replay:** `gui_set_scale` stores instance scale and forwards
   only to an existing editor (`lib.rs:3333-3352`). `gui_create:3288-3307` and
   `gui_set_parent_inner:3443-3596` do not replay it. Replay the instance scale
   when a new editor is created/attached. VST3 already replays before/after
   open (`lib.rs:2243-2260`, `:2362-2369`). Do not restore a process-global MUI
   fallback; that contaminates independent plugin instances.
7. Truce already firewalls CLAP/VST3 GUI calls: CLAP create/set-scale/set-parent
   (`:3288`, `:3333`, `:3432`) and VST3 open/close/scale/resize
   (`lib.rs:2234`, `:2374`, `:2012`, `:2121`) route through
   `run_extern_callback_with`. MUI guards add local failure cleanup; they do
   not replace those ABI guards. A future fallible open should let wrappers
   return failed attachment, rather than reporting success with no child.

## Global-state inventory

Searched every Rust static/TLS/OnceLock/LazyLock/lazy_static/AtomicPtr and OS
class/atom registration in `crates/` and `vendor/`, including unindexed vendor
files. No AtomicPtr or lazy_static macro was found. Instance fields and
immutable statics are distinguished from process/linked-image services.

A = two same-plugin editors; B = two independently linked plugin binaries /
MUI versions; C = close/reopen; D = unload. Unless noted, Rust statics below
are **per linked image**, not a shared cross-version ABI. This is a source
review, not an OS-loader runtime proof.

| State / evidence | A / B / C / D assessment |
| --- | --- |
| Former truce `HOST_SCALE` (`platform.rs`, removed) | A incorrect, now isolated per editor; B independent images; C same-editor scale retained, new-editor replay upstream; D no resource/callback. |
| `mui/src/ui/memo.rs:250-253` TREES/NEXT_UI | A IDs separate trees; B image-local table; C rendering-thread release added; D plugin-bearing captures now release outside TLS borrow. Arbitrary UI moves still require explicit release; std TLS destructor registration itself needs loader qualification. |
| `mui/src/host/headless.rs:20-21,37` CLAIMED/OFFERED/TIME | A deliberately one offline capture slot, not a multi-editor service; B image-local; C claim is sticky by API, `take` transfers the offered view; D an offline adapter must take/drop views before unload. Never claim in an ordinary DAW editor. No permanent thread/OS hook. Invalid clock fixed. |
| `mui-geometry/src/bezier.rs:16` ARCS; `boolean.rs:271` PASSES | A/B thread/image-local; C bounded plain numeric arc data / diagnostic counter; D no OS/user callback ownership, but heap-bearing TLS destructor loader behavior remains platform-dependent. |
| `mui-text/src/font.rs:175` NEXT_ID; `:158` shaper OnceLock; `mui-input/src/lib.rs:62` converted path OnceLock | A font IDs unique, shaper/path caches are instance-owned; B image-local identities; C instance Arcs release normally; D plain font/path data, no registered callbacks. |
| `mui-weld/src/raster.rs:154` THREADS | A/B/C cached parallelism count only; D no retained worker. Actual raster workers are scoped/joined (`:184-197`), not permanent global threads. |
| `mui-scene/src/scene/mod.rs:77` EMPTY; `mui-layout/src/node.rs:223` NONE; `mui-style/src/style.rs:509-510` shadow arrays; `mui-symbols/src/lib.rs:4321` TABLE | A/B/C immutable value defaults/tables; D no OS resource or callback. EMPTY is a heap-backed empty Path, not a service. |
| `mui-playground/src/lib.rs:12,16` font LazyLocks | A/B/C shared immutable bundled fonts; D no OS registration. Browser playground, not native plugin setup. |
| `mui-vello/src/lib.rs:439` SEEN | A thread-local color conversion memo; B image-local; C fixed 64 plain value slots; D no heap/OS/plugin callback. |
| `mui-vello/src/diagnostics.rs:51` JOURNAL | A synchronized per-image journal; B independent logs; C persistent bounded history; D PathBuf/String/history, not a persistent FD or service thread. |
| `mui-vello/src/diagnostics/reporting.rs:24` ACTIVE | A weak registry for an owned worker; B image-local; C new generation after final Reporter; D last owned worker joins, never intentionally detaches. GUI join latency remains a finding below. |
| `vendor/vello/src/recording.rs:18` ID_COUNTER; `lib.rs:652` HAS_WARNED | A unique recording IDs / one-time warning; B image-local; C monotonic identities / warning flag retained; D plain atomics, no OS/user callback. |
| `mui-baseview/src/a11y/x11.rs:71` XLIB_XCB | A/B/C immutable dynamic-library API cache; D retained libX11-xcb loader reference, no callback into the plugin. Unload may retain a library reference, not a MUI callback service. |
| `vendor/moose-baseview/src/pin.rs:11` PINNED | A/C idempotent; B pin resolved by address in each image; D deliberate NODELETE / DLL pin prevents unload for detached code. It is a leak by policy, not successful unload. Failed pin keeps synchronous join. |
| `vendor/moose-baseview/src/platform/win/hook.rs:26` HOOK_STATE | A same host GUI thread works by count, different GUI threads reuse the wrong HHOOK; B separate hooks/images; C relies on final unhook; D callback-in-image hazard if unhook fails or teardown is missed. |
| `vendor/moose-baseview/src/wrappers/appkit/view/implementation.rs:15` VIEW_CLASSES | A cached class per Rust V; B UUID names separate images; C class remains registered for AccessKit superclass lifetime; D registered IMP/dealloc pointers survive, requiring image lifetime protection. |
| `vendor/moose-baseview/src/wrappers/win32/window/window_class.rs:17-34,63-66` | A/B UUID class names and image HINSTANCE; C Arc owner unregisters; D ignored unregister failure can retain a WndProc registration. |
| `vendor/moose-baseview/src/wrappers/xlib/error_handler.rs:14,70-86` CURRENT_X11_ERROR / XSetErrorHandler | A TLS error data separate, but global handler save/set/restore races; B separate Rust locks cannot serialize the process-wide Xlib setter; C may restore a stale plugin callback; D unmapped callback hazard. |
| `vendor/moose-baseview/src/platform/x11/window_thread.rs:28` sizing_strategy OnceLock | A/B per handle; C new window initializes fresh; D no global OS callback ownership. |
| `vendor/accesskit_unix/src/context.rs:27` WORKER | A shared owned worker generation; B image-local; C weak registry resets on final adapter; D join normally waits for cancellation. Self-drop skips join and unbounded normal join require qualification below. |
| `vendor/accesskit_unix/src/atspi/{bus.rs:33,interfaces/cache.rs:78,interfaces/accessible.rs:149}` desktop OnceLocks | A shared through worker/Bus Arcs, not statics; B per image/connection; C owned-generation lifetime; D worker cancellation releases connections and tasks. |
| `vendor/accesskit_macos/src/image.rs:13-14`; `subclass.rs:26`; `class_macro.rs:50-64` | A class/ivar/subclass caches reused; B image-address namespace plus reload generation prevents collision, anchor has no Rust callbacks; C Rust-bearing classes persist; D namespaces do not solve IMP/ivar dealloc lifetime after image unload. |
| X atoms: `vendor/xim-rs/src/lib.rs:58`, `xlib.rs:159-160,477`, `x11rb.rs:184-185,430,728`; baseview a11y `_NET_FRAME_EXTENTS` `:114` | A per X connection/client; B no shared atom-ID cache; C new client/connection interns its own atoms; D numeric IDs belong to server, not plugin callback pointers. No global atom cache found. |
| `vendor/moose-baseview/src/wrappers/win32/h_instance.rs:21` __ImageBase | A/B linker-owned image base, used to obtain this image's HINSTANCE; C no registration of its own; D valid only while the image remains mapped. |
| Test/example-only globals | `mui/tests/frame_alloc.rs:9-35`; mui/frame_bench `:18,35,53`; layout/cache_bench `:11,28`; scene/stress `:7-27`; scene/walk test font `:1925`; vello/effects_smoke font `:371`; vello/bench flag `:31`; accesskit image_probe `:21`; xim benches `:4-5`; accesskit_winit example GC `examples/util/fill.rs:26-32`. Not linked into shipping plugin paths. The example GC intentionally uses ManuallyDrop and is not a plugin pattern. |

## For other workers

- **GPU worker:** consider the same `cfg(panic = "abort")` guard in
  mui-baseview, for non-truce adapters. If it has an `allow-panic-abort`
  feature, forward mui-truce's opt-out to it at integration; this worktree
  only has mui-truce's guard. `reporting.rs:216-225` joins without a bound;
  deadline-based HTTP is not proof that final Reporter drop is <50 ms.
  Keep the worker alive until owned shutdown completes; do not simply detach
  unpinned code. `Ui::release_thread_memos` can also be used by other hosts.
- **Windows worker:** hook.rs:84-105 uses one HHOOK despite thread-local hook
  registration; key the registry by GUI thread. `window_class.rs:63-66`
  ignores failed unregistration; verify all HWND destruction and callback
  revocation before unload, or retain/pin the image. Vendor WndProc/hook
  guards remain that worker's responsibility.
- **macOS worker:** VIEW_CLASSES now deliberately survives all editor closes
  (the older audit's per-view class disposal description is stale). AccessKit
  namespaces prevent collisions, not stale callback pointers. Qualify image
  pinning for registered IMP/deallocs and retained host views. Clearing TLS
  captures cannot unregister std's TLS destructor function pointer; test
  unload/reload on a long-lived host GUI thread, with ARCS/TREES initialized.
- **X11 worker:** process-wide XSetErrorHandler save/set/restore is unsafe
  across independent plugin images. A per-image Mutex is insufficient for B;
  prefer checked XCB operations that do not swap the host's Xlib handler.
  This worktree's callback remains unguarded at error_handler.rs:42-59.
  Window RPC request/recv and failed-pin join paths must not be claimed bounded
  merely because the normal MUI close asks for 250 ms.
- **Accessibility owner:** AccessKit Unix WORKER shutdown normally owns and
  joins its thread (`context.rs:98-119`), but final drop on the worker itself
  skips the join. The same image must remain mapped until that stop path
  really exits. Poisoned registry locks and spawn/runtime expects can panic
  (`:52-88`); MUI's native guards catch synchronous ones only, not worker
  panics. Test two independent plugin versions and dlclose during AT-SPI
  activity. Vendor unchanged here.

## Verification

Passed (every cargo invocation serialized with `/tmp/mui-cargo.lock`):

```sh
flock /tmp/mui-cargo.lock cargo test -p mui-truce
flock /tmp/mui-cargo.lock cargo test -p mui --lib
flock /tmp/mui-cargo.lock cargo test -p mui --lib lifetime_tests
flock /tmp/mui-cargo.lock cargo check -p mui-gain-plugin
flock /tmp/mui-cargo.lock cargo check -p mui-gain-plugin --release
flock /tmp/mui-cargo.lock cargo check -p mui-truce --target x86_64-pc-windows-msvc
flock /tmp/mui-cargo.lock cargo check -p mui-truce --target aarch64-apple-darwin
flock /tmp/mui-cargo.lock cargo check -p mui-truce --features allow-panic-abort --config 'profile.dev.panic="abort"'
flock /tmp/mui-cargo.lock cargo clippy -p mui-truce -p mui-gain-plugin --all-targets
```

The scale isolation test was run red before the fix: a second editor inherited
Some(1.5) rather than None. The forced abort command below fails with the
intended `mui-truce requires panic = "unwind"` compile error (exit 101):

```sh
flock /tmp/mui-cargo.lock cargo check -p mui-truce --config 'profile.dev.panic="abort"'
```

Results: 33 mui-truce unit tests pass (four doctests remain ignored),
145 mui unit tests pass, and the focused memo destructor/re-entry test passes.
Logs: `/tmp/mui-audit/edge-final-{test,clippy}-3.log`,
`edge-{windows,macos}-final.log`, `edge-{mui-tests,memo-test}.log`,
`edge-abort-{guard,optout}.log`, and `edge-release-check.log`.

Native Linux package check and clippy pass with four pre-existing hidden-
lifetime warnings in vendor/xim-rs. No new warnings remain. `mui-truce` tests
include four existing ignored documentation examples.

The combined gain-plugin Windows/macOS cross-checks were attempted but stop
in truce-vst3's C++ build scripts: missing MSVC `lib.exe`; GNU c++ rejects
Apple `-arch` / `-mmacosx-version-min` and there is no Apple SDK. No workaround
was installed. A stale shared-target macOS mui rmeta initially lacked the new
method; serialized `cargo clean -p mui --target aarch64-apple-darwin` removed
only that package's cross artifacts, then the scoped cross-check passed.
Graft was rebuilt; its worktree cache is ignored. `git diff --check` passes.

## NOT verified

- Native DAW first-open, hidden/stale/zero parent bounds, visibility/expose,
  actual scale/resize application, GPU/CPU pixels and first-frame latency.
  Permutation tests cover retained geometry and native-create arguments,
  not a real native request consumer.
- Actual native window destruction after a driver/OS panic. Panic tests use
  author closures and a synthetic headless idle handler; no live window/GPU.
- Linux VST3 host-thread correctness in default mode: intentionally NOT fixed
  until moose supplies the pump. CLAP resize has the same main-thread gap.
- Plugin packaging via cargo-truce, validators, audio underruns/locks inside
  arbitrary user Params implementations, or any downstream moose binary.
  Truce's standard parameters/meters use atomic/lock-free storage; MUI adds
  no audio-side mutex and cannot make custom parameter code lock-free.
- Two independent plugin binaries/versions, retained NSView/Win32 hook paths,
  AT-SPI self-drop, loader TLS destructors and unload/reload on native OSes.
- Gain-plugin cross compilation beyond the unavailable native C++ toolchains.
- Release binary size impact or performance cost of author preflight.
