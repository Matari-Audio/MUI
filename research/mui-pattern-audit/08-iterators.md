# MUI iterator and `Option`/`Result` pattern audit

## Scope and method

This audit covers the indexed Rust crates under `crates/`, with emphasis on `for` loops, iterator adapters and consumers, `Option`/`Result` conversion, filtering, short-circuiting, ordering, and partial-error behavior. The repository has 37 indexed files. I queried graft before opening source, used exhaustive indexed searches for `collect`, `filter_map`, `for `, `unwrap`, `.ok()`, `and_then`, and `is_some_and`, then traced the callers of the suspected preview paths (`Baked::build`, `frame_overlay`, `Ui::run`, and `chrome_frame`). No MUI source was changed.

The codebase generally chooses loops where state, ordering, or fallible work is central, and uses adapters for pure transformations. The main observations are at the preview boundary, where intentionally best-effort rendering changes conversion failures into `Option::None` and omits the affected chrome or diagnostic overlay without reporting it.

No reachable iterator or `Option`/`Result` correctness defect was demonstrated from the current scenes, fonts, or resolver construction path. The findings below are maintenance and observability notes, with severity kept informational where the trigger is only hypothetical.

## Findings

### MUI-ITER-01 — Chrome paint has an unobservable conversion-failure path

**Severity:** Informational (observability gap; no reachable trigger demonstrated)  
**Confidence:** High  
**Evidence:** [`crates/mui-preview/src/main.rs:364`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:364), [`crates/mui-vello/src/lib.rs:27`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-vello/src/lib.rs:27)

`chrome_frame` turns every `(Path, ink)` from `ui.finish()` into a Bézier path with:

```rust
.filter_map(|(path, ink)| {
    Some((mui_vello::bez_path(path, ARC_TOLERANCE).ok()?, *ink))
})
```

`bez_path` validates the path and returns `Result`; `.ok()?` changes any validation or conversion error into `None`, and `filter_map` removes it. The caller has no error count or message for the missing paint. `Ui::register` separately attempts to add the same path to `Hit` and logs an “unclickable” message on failure at [`crates/mui-preview/src/ui.rs:292`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/ui.rs:292), so a malformed widget could end up both unpainted and unavailable to input while the sidebar keeps running.

The current paths are constructed from validated rounded rectangles or successful text transforms, and no reachable failing widget was demonstrated. This is therefore an optional observability improvement, not a confirmed rendering bug. If geometry construction becomes dynamic, preserve the `Result` at this boundary by collecting a named error into the existing preview error channel or logging the widget identity. The best-effort decision can remain unchanged.

### MUI-ITER-02 — Frame overlay errors are intentionally best-effort and unreported

**Severity:** Informational (diagnostic overlay only; no reachable trigger demonstrated)  
**Confidence:** High  
**Evidence:** [`crates/mui-preview/src/main.rs:130`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:130)

`frame_overlay` loops over every layout frame, but intentionally skips failures from `RoundedRect::new`, `bez_path`, `text_run`, `rigid_transform`, and the final `bez_path` through nested `if let Ok`, `.ok()`, and `if let Some`. A frame rectangle or label could therefore vanish while the resolved scene still paints. The current layout and embedded font provide no demonstrated failing input, and best-effort behavior is reasonable for an optional overlay.

Keep the loop: it preserves frame order and makes the best-effort nature clear. Add a small failure count or debug log only if overlay correctness matters during geometry debugging. Do not make an optional frame label fatal merely to expose a currently unreachable path.

### MUI-ITER-03 — Text rendering has an unreported fallback path

**Severity:** Informational (fallback behavior; no reachable trigger demonstrated)  
**Confidence:** High  
**Evidence:** [`crates/mui-preview/src/ui.rs:254`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/ui.rs:254), callers at [`crates/mui-preview/src/ui.rs:365`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/ui.rs:365) and [`crates/mui-preview/src/ui.rs:429`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/ui.rs:429)

`Ui::run` returns `Option<TextRun>` by calling `mui_text::text_run(...).ok()`. `text_row` still advances the cursor when `run` is `None`, and `button` still paints/registers the button row but skips its label. A future bad font, invalid text option, or geometry failure would be presented as an empty label. The fixed embedded font and validated sizes provide no demonstrated failing input in the current preview.

If text inputs or fonts become dynamic, give `run` an error-aware mode for development, or at least emit the text context and error before returning `None`. The current `Option` fallback is acceptable for the fixed-font preview.

### MUI-ITER-04 — Preview baking retains only the latest conversion error

**Severity:** Informational (diagnostic fidelity; no reachable failing path demonstrated)  
**Confidence:** High  
**Evidence:** [`crates/mui-preview/src/main.rs:73`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:73) and [`crates/mui-preview/src/main.rs:95`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:95)

`Baked::build` correctly preserves authored surface order by iterating `spec.surfaces` and chaining `source.overlay()`. For each path, however, a `bez_path` error only assigns `error = Some(...)`; the loop would continue, so `paths` and `hit` would contain only successful surfaces. If multiple paths failed, a later failure would overwrite the earlier message. This gives a useful partial-preview policy, but the single `Option<String>` would not describe how incomplete the result is or retain all failed IDs.

The partial behavior is defensible for a gallery preview, and no failing path was demonstrated from the current scene set. If operators later need to diagnose malformed scenes, preserve all failures or include a count and IDs. If a resolved scene must become atomic, return a failed bake and keep the previous `Baked` value instead of publishing a partial one. The test [`crates/mui-preview/src/main.rs:574`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:574) proves authored order, but it does not assert that every declared surface is present after baking.

### MUI-ITER-05 — Defensive `filter_map` relies on a resolver invariant

**Severity:** Informational (optional invariant assertion; unreachable through the normal resolver)  
**Confidence:** High behavior, medium reachability  
**Evidence:** [`crates/mui-preview/src/main.rs:91`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:91), [`crates/mui-core/src/scene.rs:269`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-core/src/scene.rs:269), [`crates/mui-core/src/scene.rs:290`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-core/src/scene.rs:290)

The preview pipeline uses `spec.surfaces.iter().filter_map(|s| scene.surface(&s.id)...)`. `Resolver::resolve` first collects every declared ID and calls `one` for each, while `one` returns `MissingSurface` when its lookup fails, so a successful `resolve_scene` should contain every declared surface. If that invariant changed or a future `ResolvedScene` were assembled another way, this `filter_map` would silently remove the missing surface and let the preview report a smaller scene.

Keep the declaration-order iteration, which is required for correct back-to-front paint. A debug assertion or count/identity test could document the resolver invariant, but no production change is required while `resolve_scene` remains the only construction path.

## Good patterns and semantic coverage

- **Stateful validation uses loops instead of forced chains.** `Path::flatten` walks commands in order and returns immediately on malformed state, non-finite points, or budget overflow ([`crates/mui-geometry/src/path.rs:96`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-geometry/src/path.rs:96)). This keeps the active contour state and first failing command visible.
- **Fallible geometry accumulation propagates errors.** `prepare_all` uses `prepared_shape(s, o)?` inside a loop and checks the output budget after each union ([`crates/mui-geometry/src/boolean.rs:220`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-geometry/src/boolean.rs:220)). Its partial backend value is local, so an error cannot publish a half-built result.
- **Pure transformations preserve order and explicit checks.** `prepared_ring` maps points, then uses `any` to reject non-finite or over-limit coordinates before cleaning and validating the ring ([`crates/mui-geometry/src/boolean.rs:188`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-geometry/src/boolean.rs:188)). `offset_path` uses `flatten().any(...)` for a short-circuit coordinate limit and uses `map`/`sum` only for derived counts ([`crates/mui-geometry/src/offset.rs:82`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-geometry/src/offset.rs:82)).
- **The resolver avoids borrow-conflict workarounds without losing order.** It collects declared IDs, then iterates and calls `self.one(&id, 0)?` ([`crates/mui-core/src/scene.rs:269`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-core/src/scene.rs:269)). This is a good use of a small intermediate collection: resolution mutates `self`, and each dependency error remains visible.
- **Optional input is modeled as optional input.** `Interaction::update` uses `input.pos.and_then(|p| hit.at(p)).map(str::to_owned)` for the zero-or-one hit, then uses an explicit state-machine match for press/release/capture semantics ([`crates/mui-input/src/lib.rs:156`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-input/src/lib.rs:156)). It does not turn a missing hit into a fake ID or silently discard an error, because there is no error at this boundary.
- **Ordering has a regression test.** `paint_order_is_authored_order` compares baked IDs with the authored spec and proves they differ from `BTreeMap` order ([`crates/mui-preview/src/main.rs:574`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:574)). This justifies the explicit `spec.surfaces` loop and `.chain(source.overlay())` in the bake pipeline.
- **Transactional publication exists above iterator code.** `SceneState::commit` resolves a complete next scene before replacing `current` ([`crates/mui-core/src/scene.rs:468`](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-core/src/scene.rs:468)). `SurfaceState` and `LayoutState` have analogous tests that failed commits retain the previous state. This is the right pattern when a fallible traversal must not publish partial state.

## Coverage and graft tally

The indexed searches reported 40 `collect` hits across 32 symbols and 15 files; 3 `filter_map` hits across 3 symbols; 238 `for ` hits across 128 symbols and 29 files; 214 `unwrap` hits across 97 symbols and 15 files; 121 `.ok()` hits across 70 symbols and 28 files; 4 `and_then` hits across 3 symbols; and 11 `is_some_and` hits across 8 symbols. Most `unwrap` hits are tests or intentional defaults; the preview observability notes above are the relevant `.ok()`/`filter_map` boundaries.

The audit used graft map, targeted source asks, exhaustive literal searches, and caller traces before exact-span source reads. Graft reported approximately **1,007,534 tokens saved this turn**. The index and lexical searches provide strong coverage of the 37 indexed files; generated/unindexed files or runtime behavior outside the source paths were not tested.
