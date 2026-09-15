# MUI good/bad pattern audit

> Historical audit snapshot, before remediation. **F01–F11 have now been corrected**; see [validated fixes and verification](FIXES.md) for current status. Source line numbers below refer to the original audit snapshot.

Reviewed 2026-09-13 against working-tree sources based on commit `ba0617e9cb52cd5f09ed5599de03ac84e1989f2c`. The same sixteen topic agents from the idiomatic-Rust research reviewed MUI, followed by a consolidation pass and targeted executable reproductions. Scope: all eleven workspace crate directories, the TypeScript authoring package, examples/tests, manifests, and the verification/docs boundary. Existing user edits were preserved.

**The architecture is generally sound: the actionable problems concentrate in TypeScript generation, interaction defaults/event transport, and a few validation/conversion contracts.** The review did not find a demonstrated local unsafe-memory or shared-state race defect. That is a review result, not proof of absence. Several initially reported “bad patterns” were downgraded after checking reachability, intentional scope, or measurement evidence.

## Prioritized findings

Priority here is the consolidated review judgment: **P1** = fix early because a normal supported authoring path fails; **P2** = concrete API/behavior defect with a particular trigger; **P3** = diagnostics/docs/robustness. Platform-dependent cases and optimization hypotheses are separated below. The individual topic chapters contain more detail and may discuss optional API alternatives; this table is the recommended fix order.

| ID | Priority | Finding and trigger | Evidence and verification | Smallest useful correction |
|---|---|---|---|---|
| F01 | P1 | A valid TS scene without `theme.palette` generates an incomplete Rust `Theme`. Even omitting the whole optional theme breaks the generated build. | [compiler.ts:91](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/packages/mui-ts/src/compiler.ts:91). Fresh TS output checked with Cargo: `missing field palette`. | Emit the neutral palette or a `Theme::DEFAULT` struct update. Add a minimal/default scene fixture. |
| F02 | P2 | JSON string escaping is reused as Rust string escaping. A valid control-character ID produces `\u0001`, `\b`, or `\f`, which Rust rejects. | [compiler.ts:10](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/packages/mui-ts/src/compiler.ts:10). A generated U+0001 ID fails Rust compilation with `incorrect unicode escape sequence`. | Serialize Rust literals correctly; reject unrepresentable JS surrogate values explicitly. Check ordinary quotes, slashes, newlines, and control characters. |
| F03 | P2 | Integer-valued numbers formatted in exponent notation acquire an invalid `.0` suffix: `1e21` becomes `1e+21.0`. | [compiler.ts:11](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/packages/mui-ts/src/compiler.ts:11). Isolated generated fixture fails with Rust E0610. | Append `.0` only to a plain integer representation; exponent notation is already a valid float representation. Retain finite-number validation. |
| F04 | P2 | Queued button edges keep only `bool`, while cursor events overwrite one latest position. Press at A, move to B, release, then redraw can replay both edges at B. | [main.rs:451](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:451), [main.rs:465](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:465), [main.rs:480](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:480). Static event-flow trace plus a runnable interaction replay model confirms wrong-target clicking. No live-window reproduction was performed. | Preserve each edge's pointer snapshot and chronological motion, or process a coherent input-event queue in order. Test position changes between queued edges. |
| F05 | P2 | `Interaction::default()` has threshold 0; `Interaction::new()` has threshold 4. The same small jitter is a drag under one constructor and a click under the other. | [mui-input/lib.rs:119](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-input/src/lib.rs:119), [lib.rs:135](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-input/src/lib.rs:135). Standalone public-API check reproduces the difference with a 1 px move. Built-in callers currently use `new()`. | Implement the intended threshold in `Default`; have `new()` delegate to it. Test equivalent gesture behavior. |
| F06 | P2 | An invalid drag threshold is accepted silently. `NaN` classifies a 70 px move as a click; a negative threshold suppresses a stationary click. | [mui-input/lib.rs:143](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-input/src/lib.rs:143). Both outcomes reproduced through the public API. | Validate negative/non-finite input at configuration entry. Decide explicitly whether infinity is an intentional “disable drag” policy rather than accepting accidental NaN semantics. |
| F07 | P2 | `Palette::valid` accepts negative `step` and `hover` despite their documented positive-direction contract. Scene validation accepts the theme. | [color.rs:378](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-core/src/color.rs:378), [color.rs:598](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-core/src/color.rs:598). A scene with both negative values resolves successfully; the formulas reverse depth/interaction direction. | Enforce the intended sign in the shared validator. Decide/document whether zero deliberately disables an effect. |
| F08 | P2 | Variant-inapplicable `SurfaceSpec` modifiers return success-shaped values unchanged. `.radius(...)` on an inset does nothing. | [scene.rs:98](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-core/src/scene.rs:98). Public-API equality assertion confirms the requested change is discarded. Existing generated callers use matching variants. | Document applicability immediately; provide a checked method or source-specific API if misuse must be rejected. Avoid an automatic broad builder redesign. |
| F09 | P2 | A readable but invalid `MUI_PREVIEW_FONT` becomes empty axes/overlay instead of an explanatory font error. | [scenes.rs:170](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/scenes.rs:170), [scenes.rs:237](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/scenes.rs:237). Direct configured-font and error-suppression path trace; not exercised in a live GUI. | Keep the actual font error and report it through the preview diagnostic channel; explicitly fall back if that is the desired policy. |
| F10 | P2 | `Polygon::rectangle` validates individual finite inputs but not derived endpoints: `1e308 + 1e308` overflows and the constructor returns `Ok` with infinite coordinates. | [boolean.rs:24](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-geometry/src/boolean.rs:24). Public-API assertion reproduces it. Normal boolean preparation subsequently rejects the value, limiting exposure. | Check the computed endpoints before returning success; retain downstream validation. |
| F11 | P2 | Very large finite text sizes pass f64 validation, overflow f32 font scaling, and return successful text runs with non-finite metrics or invalid glyph paths. | [mui-text/lib.rs:167](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-text/src/lib.rs:167), [lib.rs:215](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-text/src/lib.rs:215). Reproduced at `f32::MAX` and `f64::MAX` using local Adwaita Sans. Extreme-input robustness issue, not a demonstrated normal-size rendering bug. | Validate the supported size range at both entry points and check derived metrics/path finiteness; checking only the input's finiteness or `f32::MAX` is insufficient. |

F01–F03 share the same missing generator test coverage, but have different root causes. F04 is in event transport; F05/F06 belong in the shared interaction constructor/configuration path. Fixing individual controls would leave sibling callers exposed.

## Concrete examples

A default scene should be the least surprising supported case:

```ts
export default defineScene({
  root: leaf([1, 1]),
  surfaces: [],
});
```

The current serializer emits every required `Theme` field except `palette`. The checked-in pill fixture masks this because it supplies a palette. Test the simplest valid input as well as the showcase example. See [architecture](16-architecture.md) and the [codegen reproducer](repros/check_codegen.py).

The interaction default issue has an equally small trigger:

```rust
let normal = Interaction::new();     // threshold = 4
let derived = Interaction::default(); // threshold = 0
```

Both constructors are public. A downstream struct deriving `Default` can therefore get different gesture behavior from the preview's explicit constructor. The correction belongs in `Interaction`, not in downstream widgets. See [state and resources](10-state-resources.md).

## Good patterns worth preserving

| Existing pattern | Why it fits MUI | Evidence |
|---|---|---|
| Resolve completely before committing state | Invalid updates do not corrupt the previous layout/scene/surface snapshot. | [SceneState::commit](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-core/src/scene.rs:479), [LayoutState::commit](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-layout/src/lib.rs:699), [SurfaceState::commit](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-egui/src/lib.rs:114) |
| Public value specifications validated at resolver seams | Convenient builders coexist with bounded geometry, ID/cycle checks, and finite-value validation. Newtyping every coordinate would not replace those coupled checks. | [layout resolve](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-layout/src/lib.rs:664), [scene resolver](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-core/src/scene.rs:245) |
| One final path and consistent winding drive paint and hit testing | Rounded corners, holes, and paint order agree with interaction geometry. Existing tests compare hit/paint behavior. | [Hit::push](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-input/src/lib.rs:53), [Baked::build](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:73) |
| One event-loop owner for mutable UI/GPU state | Ordinary ownership and short mutable borrows avoid lock/atomic complexity. | [App](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/main.rs:157), [GPU host](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/host.rs:27) |
| Borrow inputs; own derived snapshots | Font bytes, paths, layout nodes, and scene specifications need not carry borrowed lifetimes into caches. | [ownership review](01-ownership.md) |
| `Box<dyn PreviewScene>` only at a real heterogeneous boundary | Four runtime-selectable scenes justify dynamic dispatch; no speculative renderer hierarchy is needed. | [scene interface](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-preview/src/scenes.rs:15), [trait review](06-traits.md) |
| Cache invalidation includes path and tolerance | Reuse does not silently ignore changed approximation settings, and failed preparation does not publish a broken cache. | [PathMeshCache::prepare](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/crates/mui-egui/src/lib.rs:254) |
| Small independent computational crates | Layout has no external dependency; core owns semantics; hosts own windows/devices; text accepts bytes instead of opening files. | [architecture inventory](16-architecture.md) |
| Deterministic output and focused invariant tests | Generated-source parity, topology/holes, transactional failures, and gesture behavior test meaningful contracts. | [verification gate](/mnt/Windows11/DEV_PROJECTS/Repos/MUI/tools/verify.sh:1), [testing review](15-testing-docs.md) |

## Lower-priority work and explicit non-findings

**Diagnostics and docs.** Wrapper errors omit `Error::source()` even when they contain a lower-level error; implement causal chains without automatically exposing new dependency types. The README TypeScript example uses the stale `palette.accent` field; the WASM headline contradicts the native-preview exclusion; `ARCHITECTURE.md` still shows the older tessellation-first rendering path. Those are concrete documentation/diagnostic gaps. See [errors](03-errors.md), [tests/docs](15-testing-docs.md), and [architecture](16-architecture.md).

**Conditional event lifecycle risks.** Focus loss has no explicit capture-cancellation path. If a backend does not deliver a release, a held target can remain captured. Suspend/resume surface recreation also needs a target-specific check before claiming support on platforms that invalidate native surfaces. These are supported by static flow inspection, but no affected live backend was reproduced in this audit. See [state review](10-state-resources.md).

**Intentional policies, not established defects.** Best-effort frame overlays and partial preview errors need an explicit observability policy; they are not evidence that core transactions fail. Startup `expect`/`block_on` and indefinite GPU diagnostic waits are dev-host/tooling tradeoffs. Direct generator writes could be made atomic if interrupted-write recovery matters. These do not justify introducing a runtime/task framework. See [iterator review](08-iterators.md) and [async review](12-async.md).

**Representation limit.** The public f32 tessellation output rounds a width-2 rectangle near x=16,777,217 into width 4. This is reproducible loss of precision, but rounding is inherent in the advertised f32 output type. Document/enforce a useful coordinate/precision envelope or rebase large world coordinates when that use case matters; do not call every f64-to-f32 rounding a bug. See [numeric review](07-conversions.md).

**No demonstrated performance regression.** Frame allocations, linear hit testing, resolver clones, repeated flex scans, and geometry complexity are profiling candidates. One agent ran the existing release measurements and reported low costs for the checked scenes; that does not establish large-scene scaling or a full-frame budget. Do not add caches, spatial indexes, shared pointers, or parallelism solely from a static complexity observation. See [performance review](13-collections-performance.md).

**Rejected/limited hypotheses.** Empty GPU surface formats were not promoted to a defect because successful adapter selection requests compatibility with that surface. A suspected component-number gap after dropping a degenerate polygon was not reproduced through public `union`, which rejected the attempted invalid input. Raw geometry DTOs and repeated logical hit IDs are not automatically invalid designs. Text shaping/bidi/grapheme support is a documented product limit, not a newly introduced regression. See [unsafe/FFI](14-unsafe-ffi.md), [performance](13-collections-performance.md), [domain types](05-domain-types.md), and [strings/text](02-strings.md).

**Verification policy.** No remote CI configuration or declared MSRV was found. The local gate omits the facade's optional egui feature and Cargo `--locked`; those are coverage/reproducibility decisions, not current compilation failures. The optional feature was explicitly tested and checked during this audit and passed. Start with generator edge cases and the interaction regressions before expanding generic test infrastructure.

## Verification and reproductions

The existing workspace gate equivalents passed: formatting; all enabled Rust tests and doctests; workspace Clippy with warnings denied; library WASM compilation excluding the native preview; strict TypeScript checking; fresh generated-example parity; and the demo run. The optional facade `egui` feature also passed tests, Clippy, and WASM checking. The harness lists 148 ordinary test cases (3 ignored) and 8 doctests (3 ignored): 150 enabled cases, with the hardware/measurement tests and incomplete snippets explicitly excluded.

The audit added standalone research reproductions. The remediation pass converted these into checks of corrected behavior:

```sh
cargo run --manifest-path research/mui-pattern-audit/repros/Cargo.toml --offline
python research/mui-pattern-audit/repros/check_codegen.py
```

The Rust executable checks the corrected public API contracts. The Python script freshly compiles TypeScript into a temporary directory and checks that the control scene and all three formerly failing generator cases compile. It requires the existing TS dependencies and cached Cargo dependencies for offline checking. The additional `text_extreme` binary checks F11 with a caller-supplied font; `tessellation_precision` demonstrates the f32 coordinate limitation:

```sh
cargo run --manifest-path research/mui-pattern-audit/repros/Cargo.toml --offline --bin text_extreme -- /path/to/font.ttf
cargo run --manifest-path research/mui-pattern-audit/repros/Cargo.toml --offline --bin tessellation_precision
```

These ran successfully during review (text used `/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf`; font bytes are not bundled). **The regression executables now assert corrected behavior; the tessellation precision example remains an intentional representation-limit demonstration.** They do not prove the live event loop or GPU/backend behavior.

A full third-party dependency audit, fuzz campaign, Miri/sanitizer run, accessibility/localization implementation review, and manual platform matrix were not performed. The ignored GPU resize test was not run. Existing performance measurements were run by one reviewer, but no new performance guarantee is claimed. The source snapshot/check details are recorded in [baseline-checks.md](baseline-checks.md). Application fixes and their verification are recorded in [FIXES.md](FIXES.md).

## The sixteen topic reports

1. [Ownership, borrowing, lifetimes](01-ownership.md)
2. [Strings, IDs, paths, and text](02-strings.md)
3. [Errors and diagnostic boundaries](03-errors.md)
4. [Panics and validation](04-panic-validation.md)
5. [Domain types and invariants](05-domain-types.md)
6. [Traits, generics, and dispatch](06-traits.md)
7. [Numeric conversions and precision](07-conversions.md)
8. [Iterators and fallible pipelines](08-iterators.md)
9. [Public API design](09-api-design.md)
10. [State, input, caches, and lifecycle](10-state-resources.md)
11. [Shared state and thread ownership](11-shared-state.md)
12. [Async, event loops, and tooling](12-async.md)
13. [Collections, costs, and profiling candidates](13-collections-performance.md)
14. [Unsafe/FFI and GPU boundaries](14-unsafe-ffi.md)
15. [Tests, documentation, and build policy](15-testing-docs.md)
16. [Architecture and coverage inventory](16-architecture.md)
