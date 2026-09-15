# Async, threads, event loop, and tooling lifecycle audit

Scope: native Rust crates, the `mui-preview` window host, GPU readback examples/tests, and the TypeScript build compiler. The repository has no browser runtime, worker, Tokio/async runtime, or application thread pool. Rust library code is synchronous; async is confined to wgpu setup/readback and the build-time Node compiler. The preview owns all mutable UI/GPU state on the winit event-loop thread.

## Findings

### P2 — GPU initialization blocks the winit event-loop callback

**Location:** [`main.rs:429-440`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:429), [`host.rs:46-68`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/host.rs:46)

**Trigger:** `ApplicationHandler::resumed` creates the window and then calls `pollster::block_on(Gpu::new(...))`. `Gpu::new` awaits both `request_adapter` and `request_device` before returning.

**Impact:** The event-loop thread cannot dispatch input, close, resize, or redraw events while a driver/backend performs adapter or device initialization. Slow driver startup is visible as a frozen window; a backend that does not complete leaves the app unable to process a close request. The same barrier can recur if the application ever drops `gpu` and is resumed again.

**Minimal fix:** Represent GPU startup as an explicit `Initializing` state and drive completion without blocking the event-loop callback, either through a backend-safe initialization worker or incremental future polling that requests a redraw when setup finishes. Keep event-loop-owned window/surface operations on the event-loop thread and render only after a `Ready` state. If this dev-only binary intentionally accepts a startup barrier, document it as such and at least report adapter/device errors instead of panicking.

**Confidence:** High. `pollster::block_on` is a synchronous wait in the only event-loop callback that creates the GPU; no other worker or cancellation path exists.

### P2 — TypeScript generation truncates the Rust output before the async write completes

**Location:** [`compiler.ts:100-104`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/packages/mui-ts/src/compiler.ts:100), [`package.json:6-8`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/packages/mui-ts/package.json:6)

**Trigger:** The build tool dynamically imports the scene and calls `await writeFile(output, compile(scene), "utf8")` directly on the requested Rust output. There is no temporary sibling, atomic rename, lock, or recovery file. The normal `generate:example` script writes directly to `../../crates/mui-demo/src/generated.rs`.

**Impact:** Interrupting Node during the write, or running two generation commands concurrently for the same output, can leave `generated.rs` empty or partially written. The next Cargo build then fails on generated source, and the previous known-good output is lost. This is a tooling lifecycle failure rather than an async race inside the Rust runtime.

**Minimal fix:** Render the complete string first, write it to a uniquely named temporary file in the output directory, optionally flush it, then rename it over the destination. Rename only after the write succeeds; clean up the temporary file on error. If parallel generation is a supported workflow, use a lock or unique output paths as well.

**Confidence:** High. The output path is user-controlled and the code has a single direct `writeFile` with no atomic commit protocol.

### P3 — GPU readback examples and the ignored resize test have no cancellation or deadline

**Location:** [`headless.rs:186-203`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-vello/examples/headless.rs:186), [`render_resize.rs:103-116`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/tests/render_resize.rs:103)

**Trigger:** Both paths submit a readback, call `map_async`, then wait with `device.poll(wgpu::PollType::wait_indefinitely())`. The callback uses `expect`, so the operation has no returned error state or deadline.

**Impact:** A wedged or lost GPU can leave the headless CLI or an explicitly requested ignored GPU test waiting indefinitely. There is no caller-visible cancellation path, and the output file is not reached until the wait returns. This does not affect the normal preview loop, and the test is intentionally ignored on CI without a GPU, so the scope is tooling and diagnostics.

**Minimal fix:** Return `Result<Vec<u8>, Error>` from readback, poll with a bounded deadline (or an externally enforced timeout), propagate the map error, and unmap/drop buffers on every exit path. Keep the ignored test’s timeout separate from the production renderer so a diagnostic cannot hang a test process forever.

**Confidence:** Medium-high. The indefinite wait is explicit; the device-failure-to-never-complete behavior depends on the backend, so the report does not claim a deterministic hang on every device.

## Good patterns already present

- `ControlFlow::Wait` is paired with `request_redraw` only after state-changing events at [`main.rs:443-503`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:443). `RedrawRequested` returns without requesting another frame, avoiding the classic busy redraw loop.
- Mutable UI/GPU state has one owner (`App` and its `Gpu`), so there is no lock ordering, cross-thread `Send` boundary, or detached task to shut down. `Arc<Window>` is ownership for the wgpu surface lifetime, not a shared mutable state escape hatch.
- Expensive scene work is cached in `Baked`; [`Baked::build`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:73) is reached at startup, scene selection, explicit rebuild, or a changed control, not on idle redraw. [`an_idle_frame_never_rebakes`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:752) and [`a_dragged_axis_rebakes`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:766) cover those ownership/update rules.
- Resize input rejects zero dimensions and clamps to the `u16` scene limit before configuring the surface at [`host.rs:23-25`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/host.rs:23). `present` handles outdated and timed-out surfaces by requesting another redraw and avoids redraw spinning while occluded at [`host.rs:150-178`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/host.rs:150).
- Surface ownership and frame presentation are synchronous and ordered: `begin` resets the scene, `present` submits the encoder, notifies the compositor, and presents the acquired frame. There is no future or thread that can outlive the scene buffer.
- The build compiler validates numeric inputs before code generation (`compiler.ts:11-14`) and awaits module import and file completion, so normal successful invocations do not race ahead of their work. The missing piece is atomic publication of the completed output.

## Coverage and verification

Graft reports 37 indexed files, 731 symbols, and 1,174 edges. I queried the graph with `map`, targeted `ask --source` for preview/event-loop/GPU/tooling flows, exhaustive `grep` for `thread`, `async`, `request_redraw`, `ControlFlow`, `rebake`, `Gpu::`, `request_adapter`, and `writeFile`, skeletons for each async-bearing file, and caller searches for the event-loop and rebake paths. Raw `rg` then checked unindexed package metadata and confirmed there are no `std::thread`, Tokio, channel, mutex, `Promise`, timer, or `AbortController` implementations beyond the exact paths reported above. The browser category is therefore absent rather than unreviewed.

Validation completed:

- `cargo test -p mui-preview --release`: 25 passed; the GPU resize test remains ignored because it requires a GPU.
- `cargo test -p mui-preview --release --test frame_cost -- --ignored --nocapture`: release measurements were low for the checked scenes (merged resolve p95 0.127 ms; sidebar text 0.312 ms), so no current frame-budget defect was promoted from the synchronous rebake path.
- `npm run build` and `npm run generate:example`: both exited successfully. The generated Rust file was restored after the generation check, so this audit made no application edits.

Graft reported approximately 420,018 tokens saved this turn.
