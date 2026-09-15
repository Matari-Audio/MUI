# MUI state, gesture, cache, and lifecycle audit

Scope: `mui-input`, the preview event loop and GPU host, scene/layout/surface state, and the path cache. I queried Graft before opening source, then followed callers for `Interaction::update`, preview `tick`, `window_event`, `resize`, `draw`, `rebake`, `select`, and the three `commit` methods. No repository files were changed.

## Findings

### P2 — focus loss can leave pointer capture latched

Location: `crates/mui-preview/src/main.rs:443-504`, especially `443-455` and `468-486`; state consumed by `crates/mui-input/src/lib.rs:149-199`, especially `161-182`.

Concrete sequence:

1. A left press queues `true` in `App::buttons`; the next redraw sets `pointer.primary_down = true` and `Interaction::update` stores the hit target in `active` and sets `was_down`.
2. The window loses focus or the OS releases mouse capture while the button is held. The handler has `CursorLeft`, but only sets `pointer.pos = None`; it has no `Focused(false)`/cancel arm and does not clear `primary_down` or queued buttons.
3. Redraws continue to send `primary_down = true`. `Interaction::update` therefore never enters its release branch, so `active` remains held indefinitely. `tick` sees `self.input.held().is_some()` and keeps giving the gesture ownership of the stage.

Impact: after a focus loss during a drag, the preview can remain in a permanent drag until a later platform event happens to deliver a release. Sidebar input is blocked by the stale stage capture, and the next physical click may be treated as a continuation because the stored level is still down. The existing test at `crates/mui-input/src/tests.rs:247-260` intentionally preserves capture when the pointer leaves, but it does not cover cancellation without a release.

Minimal fix: handle `WindowEvent::Focused(false)` (and any platform suspension/capture-loss event) by setting `pointer.primary_down = false`, `pointer.pos = None`, and clearing `buttons`, then request one redraw so `Interaction::update` consumes a release. A small `Interaction::cancel()` that clears `active`, `press_pos`, `last_pos`, `dragging`, and `was_down` is clearer if the UI must cancel immediately rather than synthesize a release. Add a regression sequence for press → focus loss → redraw → ordinary click.

Confidence: high for the missing cancellation path; whether a particular backend separately emits `MouseInput::Released` after focus loss is platform-dependent, so this is P2 rather than an unconditional M1.

### P2 (conditional event timing) — queued button edges use the final pointer position

Location: `crates/mui-preview/src/main.rs:443-504`, especially `468-486`, and the consumer at `crates/mui-preview/src/main.rs:258-303`; hit/click state is computed by `crates/mui-input/src/lib.rs:149-199`.

Concrete sequence (the backend must coalesce these events before one redraw, which is allowed under `ControlFlow::Wait`):

1. The pointer is over sidebar button A. `MouseInput::Pressed` appends only `true` to `App::buttons`; it does not save the pointer position.
2. `CursorMoved` changes the single `App::pointer.pos` slot to the location of sidebar button B.
3. `MouseInput::Released` appends only `false`. One redraw takes `[true, false]` with `mem::take`, but both calls to `tick` read the same final `pointer.pos`, B.
4. The `true` replay therefore captures B, and the `false` replay can report B clicked. A press on A can select B. If B is outside the sidebar, the same stale sample can instead make A's press disappear or start a stage gesture at B.

This is stronger than a merely theoretical queue concern: the edge order is preserved but the coordinate that gives each edge meaning is not. The existing test at `crates/mui-preview/src/main.rs:695-716` queues `[true, false]` at one fixed position, so it cannot detect movement between the two edges. The exact occurrence depends on event-loop scheduling and backend redraw coalescing, hence P2 rather than an unconditional M1.

Minimal fix: queue a pointer sample with each button edge, such as `(primary_down, pos_at_event)`, and have the replay use that sample. For full drag fidelity, queue cursor samples as well and replay the event stream in arrival order; edge-only snapshots fix the cross-button click but cannot reconstruct intermediate drag motion. Add a regression with A press → move to B → B release before one redraw, and assert that A is not replaced by B.

Confidence: high for the code-level mismatch and the deterministic replay trace; backend/event-loop dependence controls whether users encounter it in a given run.

### P3 (conditional API/future scene) — failed rebakes discard the last valid preview

Location: `crates/mui-preview/src/main.rs:57-115` (`Baked::build`) and `222-225` (`rebake`), with callers at `227-234` (`select`) and `308-372` (`chrome_frame`).

`Baked::build` catches a scene-resolution error and returns `Baked { error: Some(...), ..Default::default() }`. `rebake` assigns that result directly to `self.baked` and increments `rebakes`. The same function is used after a selected scene changes and after a sidebar control reports `retune`.

Concrete sequence: display a valid scene, then make a future `PreviewScene` or unchecked external edit produce an invalid layout, surface, font path, or overlay. `Baked::build` returns an error object with no scene, paths, or hit targets. `rebake` replaces the valid snapshot. `draw` then paints no specimen paths, and the empty `Hit` map makes the stage inert until a later successful rebuild. On selection, `selected` is changed before the failing build, so the sidebar describes the new scene while the previous valid frame has already been lost. The current built-in controls constrain their values to valid ranges, so this report did not reproduce the failure through a built-in control; it is a future-scene/API policy gap rather than an unconditional M1.

Impact: a transient invalid edit causes a blank, non-interactive stage instead of preserving the last known-good render and showing the error. The state packages deliberately use the safer transaction pattern: `SceneState::commit` at `crates/mui-core/src/scene.rs:479-488`, `LayoutState::commit` at `crates/mui-layout/src/lib.rs:699-713`, and `SurfaceState::commit` at `crates/mui-egui/src/lib.rs:114-129` all compute `next` before publishing it, and their tests assert the old revision and value survive failure.

Minimal fix: make `Baked::build` return `Result<Baked, BuildError>` (or return a separate status plus a last-good snapshot), and swap `self.baked` only after a complete successful build. For scene selection, publish `selected` together with the successful baked snapshot, or retain the old selection while showing the build error. Count a rebake only when a new snapshot is published. Preserve an error overlay/status separately so diagnostics do not require throwing away render and hit data.

Confidence: medium-high; the failure path and direct replacement are explicit, while the triggering invalid scene is currently conditional on a future scene or unchecked external input.

### M2 — public drag threshold accepts values that corrupt gesture semantics

Location: `crates/mui-input/src/lib.rs:143-146` and the comparison at `178-182`.

`Interaction::with_drag_threshold` stores any `f64`. A `NaN` threshold makes `distance > threshold` false forever, so a large move never becomes a drag; if the pointer returns to the original target, release can be reported as a click. A negative threshold makes even zero movement satisfy `0.0 > threshold`, so a press becomes a drag immediately and cannot click. Infinity disables dragging in the same way as `NaN` for practical purposes.

Impact: a caller reading a bad preference, touch configuration, or unit conversion gets silently incorrect pressed/dragged/clicked behavior. The current tests cover larger valid thresholds (`crates/mui-input/src/tests.rs:193-206` and the neighboring threshold test), but not non-finite or negative input.

Minimal fix: reject non-finite or negative thresholds at the API boundary. A backward-compatible builder can retain the default on invalid input only if that policy is documented; a `Result<Self, InputError>` is clearer for a public library. Add tests for `NaN`, negative, and infinite values. Do not clamp `NaN` implicitly, because hiding configuration errors makes gesture bugs difficult to diagnose.

Confidence: high; this follows directly from the public setter and comparison.

### P2 — `Interaction::default()` and `Interaction::new()` have different drag semantics

Location: `crates/mui-input/src/lib.rs:120-140`. `Interaction` derives `Default`, so its private `threshold: f64` is initialized to `0.0` (`120-132`); `Interaction::new()` then explicitly sets `threshold: DRAG_THRESHOLD` (`30`, `135-140`), where `DRAG_THRESHOLD` is `4.0`. The only indexed MUI callers use `Interaction::new()` (`crates/mui-preview/src/main.rs:196-220`, especially `204`, and `crates/mui-preview/src/ui.rs:103-113`, especially `105`), but the public type exposes both construction paths and no indexed caller uses `Interaction::default()`.

Concrete reproduction:

1. Create a hit target and press at `(50, 50)`.
2. Move to `(50.1, 50)` and release. With `Interaction::default()`, `distance > threshold` is `0.1 > 0.0`, so the target becomes dragged and the release is not a click. With `Interaction::new()`, `0.1 > 4.0` is false, so the same interaction is a click.
3. Real pointer devices routinely produce subpixel or one-pixel motion while a user intends a click. Downstream code that follows Rust convention and uses `Default::default()` therefore gets jitter-sensitive behavior that differs from the preview and all current tests, which use `new()`.

Impact: this is a public API contract mismatch, not invalid caller configuration. `Default` and `new` are normally interchangeable ways to obtain the standard value; here they silently select different gesture policies. A downstream widget can pass unit tests with `new()` and then reject ordinary clicks when assembled through `Default`.

Minimal fix: make one constructor authoritative. Remove the derived `Default`, implement it with `threshold: DRAG_THRESHOLD`, and have `new()` return `Self::default()` (or make `new()` the direct implementation and have `Default` call it). Add a test that runs the same sub-threshold press/move/release through both constructors and asserts identical `clicked`/`dragged` responses.

Confidence: high; the mismatch is explicit in the definition, and the existing indexed callers confirm normal MUI paths use `new()` while leaving the public `Default` path untested.

### P3 (platform coverage limitation) — GPU state is retained across suspend/resume

Location: `crates/mui-preview/src/main.rs:429-440`; GPU construction and resource ownership are in `crates/mui-preview/src/host.rs:27-108`.

`resumed` creates a `Gpu` only when `self.gpu.is_none()`. There is no `suspended` handler that drops the surface/device or resets pointer state. On platforms where suspension invalidates the native window/surface, resume sees `gpu.is_some()` and returns, so the stale surface is never recreated. Desktop backends may never exercise this lifecycle, but `winit` exposes it for platforms that do.

Minimal fix if a supported mobile/suspend backend needs this lifecycle: on suspension, drop `Gpu`, clear pending button transitions and pointer capture, and let the next `resumed` create a fresh surface. Keep scene/baked data in `App` so device recreation does not force a geometry rebuild. Android/mobile support is not established by the current target set, so this remains a coverage follow-up rather than a demonstrated MUI defect.

Confidence: low-medium; impact depends on the target backend and whether it preserves the surface through suspension, and no desktop reproduction was found.

### M3 — suboptimal surfaces are presented without reconfiguration

Location: `crates/mui-preview/src/host.rs:150-209`, `Acquired::Suboptimal(f)`.

The `Suboptimal` texture is treated like `Success` and presented. In the current `wgpu` API this is allowed, but the API recommends configuring again when the texture no longer matches the surface properties. DPI/backend changes can therefore leave the preview running on a degraded configuration until a later `Outdated` result.

Minimal fix: after presenting a suboptimal frame, reconfigure with the current `config` or mark a `needs_reconfigure` flag and do it before the next acquisition. Keep the existing `Outdated` path, since that one must reconfigure before retrying.

Confidence: medium-low; this is a maintenance/performance risk rather than a demonstrated rendering failure in the checked tests.

## Good patterns already present

- `Interaction` has explicit edge-triggered `pressed`, `released`, and `clicked` slots, a consuming capture state, and a separate drag threshold. `Hit::push` converts geometry once and keeps hit targets in paint order (`crates/mui-input/src/lib.rs:44-81`). The preview preserves button-edge ordering with `std::mem::take` instead of collapsing a press/release batch (`crates/mui-preview/src/main.rs:468-486`), and the corresponding test is at `695-716`. That queue does not preserve a pointer sample per edge, which is the boundary of the P2 batching finding above.
- Hit testing uses the same paths and order as rendering: `Baked::build` builds paths and `Hit` from the authored surface sequence (`crates/mui-preview/src/main.rs:73-115`). This avoids the common bug where a separately recomputed hit shape disagrees with what the user sees.
- Scene, layout, surface, and path-mesh updates are transactional. `PathMeshCache::prepare` (`crates/mui-egui/src/lib.rs:241-273`) keys reuse on exact path plus tolerance and advances its revision only after tessellation succeeds. Tests cover unchanged-path reuse and failure preservation.
- Resize is bounded before conversion to Vello’s `u16` dimensions (`crates/mui-preview/src/host.rs:23-25`), reconfigures only when the size changes, and resets the Vello scene so its clip matches the new surface (`126-137`). `Outdated` and `Timeout` request another redraw, which matters under `ControlFlow::Wait`.
- GPU resources are owned by `Gpu` and therefore released by normal Rust drop when the event loop exits. `begin` resets the scene before handing out `&mut Scene`, preventing stale commands from surviving into the next frame (`host.rs:141-144`). Close and Escape both exit through the event loop; no manual resource shutdown is needed for the normal desktop path.

## Validation and coverage

Passing checks:

- `cargo test -p mui-input`: 15 passed.
- `cargo test -p mui-core`: 24 unit tests and 1 doc test passed.
- `cargo test -p mui-layout`: 12 passed.
- `cargo test -p mui-egui`: 4 passed.
- `cargo test -p mui-preview`: 25 passed; frame-cost and GPU resize tests were present but ignored as measurement/hardware tests.

The tests cover capture while the pointer leaves, click-versus-drag threshold behavior, hit/paint agreement, sidebar ownership, fixed-position event batching, idle no-rebake behavior, successful axis rebakes, transactional scene/layout/surface commits, and cache reuse. They do not cover focus loss without a release, movement between queued button edges, failed preview rebake rollback, invalid threshold values, or suspend/resume.

Graft tally for this audit: 40 indexed map/ask/skeleton/caller queries, approximately 1,045,058 tokens saved according to Graft’s per-call estimates.
