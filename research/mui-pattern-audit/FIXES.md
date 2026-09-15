# Validated MUI audit fixes

Remediation of the 2026-09-13 audit, using the same sixteen topic agents. Changes are in the working tree; no commit was created. The original topic reports describe the pre-fix snapshot.

## Confirmed defects corrected

| Findings | Correction | Regression coverage |
|---|---|---|
| F01–F03: TypeScript generation | Always emit a palette; emit Rust string escapes, preserve valid Unicode, reject lone UTF-16 surrogates; retain valid exponent literals and reject non-finite numbers. Publish through a sibling temporary file and rename. | Five Node tests, Rust parsing, actual Cargo compilation of formerly failing generated scenes, unchanged checked-in example parity. |
| F04: pointer transport | Replay chronological pointer samples, including motion, rather than applying every button edge at the final pointer position. Cancel capture and pending samples on focus loss. | Actual headless App tests: same-batch tap, A-to-B release, away-and-return drag, cancellation and recovery. Chrome cancellation preserves presentation state. |
| F05–F06: interaction configuration | `Default` and `new()` use the same 4 px threshold; reject negative and non-finite thresholds. `cancel()` preserves configuration while clearing gesture state. | Constructor, invalid/zero threshold, capture and edge reset tests. |
| F07: palette direction | Require finite, strictly positive `step` and `hover`, matching the existing Rust/TS contract and shipped values. | Zero/negative/non-finite rejection and dark/light visual-direction checks. |
| F08: ineffective modifiers | Reject `.radius()` on non-frame surfaces and `.corners()` on non-merge surfaces; document applicability and panic contracts. | Matching-variant behavior and misuse tests. |
| F09: invalid configured font | Parse readable font bytes before installing them; retain the parser diagnostic and fall back to bundled Hack. | Malformed readable bytes tested without changing process environment. |
| F10: rectangle overflow | Validate computed endpoints and reject positive extents that round away at large coordinates. | Both overflow axes and valid/invalid ULP boundary tests. |
| F11: text conversion | Check the f64-to-f32 size conversion and derived metrics/path validity before returning successful output. | Ordinary sizes, extreme sizes, underflow, valid paths and nested error coverage. |
| Error diagnostics | Expose existing nested causes through `Error::source()` in core scene, text, tessellation and egui wrappers. | Direct and multilevel causal-chain tests; public enum shapes unchanged. |
| Documentation and gate | Correct stale TS example, library WASM scope and current Vello architecture. Add Cargo `--locked`, additive optional-feature coverage and generator tests to the gate. | Workspace gate, documentation tests, generated-source parity. |

Compatibility: previously ignored invalid surface modifiers and invalid drag thresholds now panic as documented programmer-configuration errors. A zero drag threshold remains valid. Zero palette step/hover is rejected because the existing palette contract requires positive values. Valid callers retain their fluent APIs.

## Deliberately unchanged after validation

- No evidence justified adding caches, spatial indexes, worker tasks, locks, newtypes throughout the API, or new dependencies. Large-scene/input-backlog performance remains a profiling question.
- Safe GPU/window ownership and rendering an acquired suboptimal frame follow the dependency contracts. No local unsafe-memory or shared-state race defect was demonstrated.
- Android suspension would require dropping/recreating the native surface if mobile preview support is introduced; the current desktop preview was not expanded to mobile.
- f32 tessellation precision, simple text shaping, optional preview overlays, synchronous GPU startup, and manually invoked GPU readback waits remain documented scope or representation limits.
- No remote CI, MSRV policy, fuzzing infrastructure, or renderer abstraction was invented.

## Verification

`bash tools/verify.sh` passed after integration: **170 Rust tests/doctests passed, 6 intentionally ignored; 5 Node tests passed**. Formatting, Clippy across all targets/features with warnings denied, locked offline library WASM checking, generated Rust parity, and the end-to-end demo all passed. `git diff --check` passed and the Graft index was refreshed. SHA-256 checks confirmed all six pre-existing user files remained unchanged. The research repro executables now assert corrected public API behavior; the tessellation precision executable still demonstrates an intentional representation limit.

The standalone public-API regression executable and `text_extreme` check passed (the latter used `/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf`). `python3 research/mui-pattern-audit/repros/check_codegen.py` passed all four actual Cargo compilation cases: the control and F01–F03. Standalone repro formatting passed; the unchanged tessellation example reproduced its documented precision limit.

No live window/backend matrix, ignored GPU resize test, fuzz campaign, or third-party dependency audit was run. These changes do not claim a new performance guarantee. Existing user edits were preserved.
