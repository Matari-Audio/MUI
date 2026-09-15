# MUI error propagation and boundary audit

## Scope and method

This audit covers the Rust crates in `/mnt/Windows11/DEV_PROJECTS/Repos/MUI`, with emphasis on public `Result`/`Error` APIs, propagation across crate boundaries, context preservation, suppression, and application versus library policy. I started with the repository's Graft index, then opened only the exact source spans returned by Graft and traced callers for the error-producing functions. The report is about behavior that can affect users or callers; it does not treat every `unwrap_or` or every test panic as a defect.

Severity is relative to this workspace: **P1** means a likely correctness or data-loss failure in a normal supported path, **P2** means a user-visible or integration-significant diagnostic/correctness problem, and **P3** means a lower-impact diagnostic gap or a policy improvement. Confidence describes how directly the source and caller trace establish the finding.

## Prioritized findings and observations

### Policy observation — Preview permits partial output and keeps only the last path error

**Evidence:** `crates/mui-preview/src/main.rs:73-115`, with rebakes at `crates/mui-preview/src/main.rs:222-225`, status rendering at `crates/mui-preview/src/main.rs:346-361`, and drawing at `crates/mui-preview/src/main.rs:374-425`.

`Baked::build` resolves the scene once, then iterates surfaces. A `mui_vello::bez_path` failure records `error = Some(...)` and skips that surface. The code also has a `Hit::push` failure branch, but `Hit::push` calls the same `mui_vello::bez_path(path, mui_vello::ARC_TOLERANCE)` conversion shown at `crates/mui-input/src/lib.rs:53-61`; the outer build call uses the same path and tolerance. With the current deterministic implementation, a successful outer conversion should make that inner failure unreachable. It should not be reported as a currently reachable draw/hit mismatch.

Multiple path-conversion failures still replace the previous `Option<String>`, so only the last one is shown. `App::rebake` also unconditionally replaces the current `Baked`; that may be the intended preview policy because the builder keeps all successfully converted surfaces and reports a degraded result, but it is not evidence of a P2 correctness bug by itself.

**Trigger:** multiple path-conversion failures, if such inputs are supplied. **Impact:** diagnostics are incomplete; the preview does not tell the user about every omitted surface. This is a policy/observability observation, not a confirmed draw/hit inconsistency. **Optional fix if full diagnostics are desired:** collect a structured list of surface IDs and causes instead of one string. Retaining the last good bake would be a separate product decision, since the current code deliberately shows the newly built partial result.

**Confidence:** high for last-error behavior; high that the alleged hit mismatch is not reachable with the current identical converter calls. No test currently exercises multiple conversion failures.

### P3 — Public wrapper errors lose causal chains

**Evidence:** `crates/mui-core/src/scene.rs:178-200`, `crates/mui-egui/src/lib.rs:11-27`, `crates/mui-tessellate/src/lib.rs:14-32`, and `crates/mui-text/src/lib.rs:20-47`.

The crate-level error enums correctly make lower-level failures part of the public type, but their `std::error::Error` implementations are empty. `SceneError::Display` is `scene: {self:?}`, and the wrappers generally print the child error without implementing `source()`. Consequently, a generic reporter or an `anyhow`/`eyre` caller can display the top-level message but cannot walk from `SceneError::Geometry` to `mui_geometry::Error`, from `mui_egui::Error` to tessellation, or from text errors to their nested geometry cause. This is a diagnostic/interoperability gap, not a demonstrated computation failure. `mui-tessellate::Error::Tessellation(String)` and text's `Font(String)`/`Draw(String)` also erase concrete external error types, but changing those public representations is optional and has API-compatibility costs.

**Trigger:** a library consumer reports an error through a source-chain-aware logger, or needs to distinguish a nested geometry/font/backend cause. **Impact:** diagnostics lose causal context and machine-readable classification at crate boundaries; callers must match the outer enum or rely on display strings. Leaf `mui_geometry::Error` and `mui_layout::Error` values have no nested cause and are not defects merely because their `Error` implementations are empty.

**Minimum fix:** implement `Error::source()` for nested variants and add focused tests that assert the source chain. Improving `Display` is useful but separate. Preserving lower-level errors instead of converting to `String` is optional and should be evaluated as an API change, not assumed to be a required fix.

**Confidence:** high for missing source chains; not assessed as a defect for the `String` representations because the right public API is a design choice.

### P2 — Preview swallows invalid-font and glyph-overlay failures

**Evidence:** `crates/mui-preview/src/scenes.rs:170-196` and `crates/mui-preview/src/scenes.rs:237-263`.

`GlyphAxes::new` reads the configured font and, after a successful read, calls `mui_text::axes(&font).unwrap_or_default()`. An invalid font therefore looks like a font with no axes. The overlay path uses `let Ok(path) = mui_text::glyph_path(...) else { return Vec::new(); }`, turns flattening failure into an empty point list, and turns rigid-transform failure into an empty overlay. These failures never reach `Baked::error` or the status UI.

**Trigger:** `MUI_PREVIEW_FONT` names a readable but invalid/unsupported font, or a requested glyph/path cannot be converted. **Impact:** the preview can show a blank glyph or missing axis controls without explaining whether the glyph is absent, the font is invalid, or geometry conversion failed. The fallback for an unreadable configured font is better: `scenes.rs:174-182` includes the path and OS error before using bundled Hack. The missing-codepoint `.notdef` behavior in `mui-text::text_run` is documented and intentional, so it should not be reported as this same issue.

**Minimum fix:** preserve the `axes` error as a diagnostic (while retaining a deliberate fallback if desired), and return structured overlay errors to the existing bake/status channel with the glyph or surface identifier. If a blank overlay is an intentional best-effort mode, expose that state explicitly. **Confidence:** high for invalid-font diagnostic loss; medium-high for the exact user-visible frequency because normal bundled-font paths are valid.

### Policy observation — Presentation-only paths use best-effort omission

**Evidence:** `crates/mui-preview/src/main.rs:130-155`, `crates/mui-preview/src/main.rs:357-361`, and `crates/mui-preview/src/ui.rs:254-303`.

`frame_overlay` ignores rounded-rectangle and text conversion failures with `if let Ok(...)`, `.ok()`, and conditional omission. `chrome_paint` uses `filter_map` with `mui_vello::bez_path(...).ok()?`, so a failed conversion simply removes a chrome path. `Ui::run` returns `mui_text::text_run(...).ok()`, making a failed label disappear; `register` logs hit errors with `eprintln!` but does not propagate them to the status path. These are explicit best-effort presentation choices. The current source does not establish a reachable failure for the bundled static font and resolved scene inputs, so this is not listed as a confirmed bug.

If future inputs make these failures reachable, the result will be a missing label/frame/control without status context. An optional improvement would be to collect contextual diagnostics into the existing preview status model or log the affected identifier consistently.

**Confidence:** high that errors are suppressed; low that the current supported inputs trigger them.

### Open question — Text metrics have an untested zero-width fallback

**Evidence:** `crates/mui-text/src/lib.rs:195-200`.

`text_run` maps an unmapped character to `GlyphId::NOTDEF`, which is documented behavior, but then uses `advance_width(...).unwrap_or(0.)`. The repository does not establish whether `skrifa` can return `None` for the glyphs reached here, so this is an observation requiring dependency-contract verification, not a confirmed bug. The `charmap.map(...).unwrap_or(NOTDEF)` branch should remain intentional and is not itself a finding.

**Next check:** verify the `skrifa` contract; if `None` is a valid state, document and test the fallback, and if it indicates malformed data, decide whether a typed error is appropriate. **Confidence:** unresolved.

### P3 — Example CLI fails fast on user-controlled output errors

**Evidence:** `crates/mui-vello/examples/headless.rs:62-102`.

The headless example uses `expect`/panic for scene resolution, path conversion, and PNG/file writes. Examples may reasonably fail fast for a fixed demonstration, so this is not a library defect. The output path is user-controlled, however, and a `main() -> Result<(), Box<dyn Error>>` with context would give a clearer command-line failure. **Confidence:** high for the behavior, low as a product defect; treat as a boundary caveat rather than a priority fix.

## Good patterns to preserve

The core libraries have several strong patterns that should anchor any fixes:

- Typed `Result` APIs propagate with `?`: layout resolution at `crates/mui-layout/src/lib.rs:664-684`, scene resolution at `crates/mui-core/src/scene.rs:460-464`, egui preparation at `crates/mui-egui/src/lib.rs:47-65`, tessellation at `crates/mui-tessellate/src/lib.rs:48-94`, and hit registration at `crates/mui-input/src/lib.rs:53-61`. These boundaries preserve failure until a caller can choose policy.
- State commits are transactional. `mui-core::SceneState::commit` computes `next` before changing current state (`crates/mui-core/src/scene.rs:470-488`); layout and egui use the same shape at `crates/mui-layout/src/lib.rs:699-713` and `crates/mui-egui/src/lib.rs:35-65`. Tests confirm a failed commit preserves the prior valid state (`scene.rs:583-591`, layout `lib.rs:784-794`, egui `lib.rs:202-222`). If preview eventually adopts an all-or-nothing rebake policy, this last-good-state pattern is a suitable precedent.
- `Baked::build` does surface resolution failure at least visibly (`crates/mui-preview/src/main.rs:75-84`), and the configured-font read fallback includes path and OS-error context (`crates/mui-preview/src/scenes.rs:174-182`). These are useful precedents for adding context to the currently suppressed branches.
- `mui-text` documents the `.notdef` fallback for an absent codepoint (`crates/mui-text/src/lib.rs:157-166`), showing the useful distinction between an intentional semantic fallback and an accidentally discarded error.

## Coverage and limits

`cargo test --workspace` passed all enabled workspace tests, including the core, geometry, input, layout, preview, tessellation, text, and vello suites and doctests. Three tests are ignored (two measurement tests and one GPU/render-resize test), and three doctests are ignored. This validates the normal geometry and scene paths but does not cover the findings above.

Specific gaps are:

- no preview test for multiple `bez_path` failures, error overwriting, or the chosen partial-rebuild/rebake policy; `Hit::push` currently repeats the same converter and tolerance as the preceding build call, so a failure after a successful conversion is not established as reachable;
- no invalid-font `MUI_PREVIEW_FONT` test and no overlay test asserting that `glyph_path`, flatten, or transform errors become diagnostics;
- no tests for `Error::source()` across `SceneError`, egui, tessellation, or text wrappers;
- no test for the `advance_width(...).unwrap_or(0.)` branch or a dependency-contract assertion about whether that `None` case is reachable;
- no tests documenting the best-effort frame/chrome/widget presentation omission policy.

The audit did not change application or library code. Graft reported approximately **967,974 tokens saved** across this audit turn (map, ranked source queries, exhaustive searches, and caller traces); the total is the sum of the savings printed by each Graft invocation.
