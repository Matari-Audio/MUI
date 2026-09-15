# MUI API-design audit

Scope: public Rust APIs in `mui-core`, `mui-layout`, `mui-geometry`, `mui-input`, `mui-egui`, `mui-text`, and the `mui` facade, plus the public TypeScript surface in `packages/mui-ts`. The workspace is version `0.3.0` (`Cargo.toml:1-20`), so recommendations labelled *pre-1.0* are API-hardening suggestions; the unchecked-value findings are current correctness defects. I followed Graft ranked spans and callers before reading implementation details. Coverage: 37 indexed files, 731 symbols, 1,174 edges; broad callers include preview, egui, vello, tessellate, demo, examples, and tests.

## Findings

### 1. P2 Medium — variant-inapplicable fluent setters silently do nothing (current bug)

`SurfaceSpec::radius` only mutates `SurfaceSource::Frame`, while `SurfaceSpec::corners` only mutates `SurfaceSource::Merge` (`crates/mui-core/src/scene.rs:98-109`). The constructors make that distinction explicit: `frame` stores a radius (`scene.rs:56-67`), `merge` stores corners (`scene.rs:68-79`), and `inset`/`outset` have neither (`scene.rs:80-97`). Therefore this compiles and returns a seemingly configured value, but has no effect:

```rust
let s = SurfaceSpec::inset("card", "root", Spacing::px(4.0))
    .radius(Radius::Absolute(4.0)); // silently unchanged
let s2 = SurfaceSpec::frame("card").corners(CornerRule::GlobalScaled(0.8)); // unchanged
```

**Trigger:** calling either setter on the wrong `SurfaceSource` variant. **Impact:** visual output uses the default behavior while the caller believes a style was applied; no `Result`, panic, or diagnostic identifies the mistake. **Minimal fix:** before 1.0, make these operations return `Result<Self, SceneError>` (or expose variant-typed builders); a debug assertion alone would leave release behavior incorrect. **Confidence:** high. **Coverage:** Graft’s `grep` shows radius use on frame data (`scene.rs:535`) and corners use on merge data; `resolve_scene` callers span demo (`crates/mui-demo/src/main.rs:39-46`), preview, vello, egui, and tests. Existing callers use matching variants, but there is no invalid-variant test, so this public API hole remains reachable.

### 2. P2 Medium — `Palette::valid` contradicts its positive-value contract (current bug)

`Palette` documents `step` and `hover` as “always positive” (`crates/mui-core/src/color.rs:354-383`), but `Palette::valid` checks only finiteness (`color.rs:598-612`). `layer` then multiplies depth by `step` (`color.rs:448-458`), so a negative step reverses the intended light/dark progression; a negative hover reverses hover contrast. `Theme::valid` delegates to this check (`crates/mui-core/src/theme.rs:152-158`), and `Resolver::new` rejects only invalid themes (`scene.rs:245-248`).

**Trigger:** `Theme { palette: Palette { step: -0.045, ..Palette::NEUTRAL }, ..Theme::default() }`. **Impact:** the theme is accepted and produces semantically inverted layers. **Minimal fix:** require finite and strictly positive `step` and `hover` in `Palette::valid` (the docs say positive), with regression tests for negative and zero values. Current tests cover NaN pigments and infinite step (`color.rs:975-988`), not negative values. **Confidence:** high. **Coverage:** all scene resolution paths call the same theme validation; Graft callers include demo, preview, vello, egui, and core tests.

### 3. P2 Medium — drag-threshold builder accepts values that destroy interaction semantics (current bug)

`Interaction::with_drag_threshold` stores any `f64` (`crates/mui-input/src/lib.rs:134-146`), while update marks a drag when `distance > threshold` (`lib.rs:176-186`). A negative threshold makes even zero movement a drag; `NaN` makes the comparison false forever. **Trigger:** `.with_drag_threshold(-1.0)` or `.with_drag_threshold(f64::NAN)`. **Impact:** click/drag classification silently becomes unusable. **Minimal fix:** return a validation error from a fallible builder (or add `try_with_drag_threshold`) and require finite, non-negative input. **Confidence:** high. **Coverage:** Graft finds normal uses in interaction tests (`crates/mui-input/src/tests.rs:192-206`, `270-281`) and update callers in preview UI (`crates/mui-preview/src/ui.rs:155`, `main.rs:281`), but no invalid-value test.

### 4. Medium — TypeScript exposes only part of the Rust layout builder (pre-1.0 API gap)

`packages/mui-ts/src/index.ts:5-15` exposes `LayoutProps` for id, gap, padding, min/max, grow, align, and justify. Rust `Node` also supports `basis`, `shrink`, and `align_self` (`crates/mui-layout/src/lib.rs:214-234`), but `packages/mui-ts/src/compiler.ts:22-39` never reads or emits them. This is a real public-surface mismatch, though not a correctness bug if TypeScript is intentionally a curated subset. **Trigger:** a TS consumer needs those existing Rust controls. **Impact:** the consumer must drop to generated Rust or cannot express the layout. **Minimal fix:** add optional TS properties, compiler mapping, and tests, or document the supported subset clearly. **Severity:** medium/low before 1.0. **Confidence:** high; current `examples/pill.ts:14-21` exercises only the supported subset.

### 5. Low — semantic validation is deferred past the TypeScript call (pre-1.0 ergonomics)

The TS constructors and `defineScene` are intentionally thin (`packages/mui-ts/src/index.ts:22-25, 63-66, 104`; `compiler.ts:81-98`). The compiler rejects only non-finite numbers (`compiler.ts:10-14`); empty ids, negative sizes/gaps, and invalid radii are emitted and rejected later by Rust (`mui-layout/src/lib.rs:359-379`; `mui-core/src/scene.rs:245-260`; `theme.rs:106-112`). **Trigger:** `leaf([10, 10], { id: "" })` or `radius.parentNormalized("x", -1)`. **Impact:** errors appear after code generation, with less local/domain-specific feedback. **Minimal fix:** validate ids and domain ranges in the TS compiler while retaining Rust validation as the authority. **Severity:** low/medium; this is an ergonomic improvement, not a current safety hole because Rust still rejects invalid scenes. **Confidence:** high.

### 6. Low — public enums create a future semver decision (pre-1.0)

Public enums include `Align`/`Justify` (`crates/mui-layout/src/lib.rs:104-120`), `BooleanOp` (`crates/mui-geometry/src/boolean.rs:171-177`), and `Radius`/`CornerRule`/`SurfaceSource` (`crates/mui-core/src/scene.rs:14-48`). Downstream exhaustive matches will break when a variant is added. No current bug is shown, but the 0.3 API should decide openness before stabilization: introduce `#[non_exhaustive]` on enums intended to grow, or document them as closed and accept a semver-major change for new variants. Adding the attribute later also breaks existing exhaustive matches/construction, so this is a design choice to make deliberately, not a drop-in patch. **Confidence:** high.

## Good patterns already present

- `Node` keeps fields private and exposes consuming, chainable builders (`crates/mui-layout/src/lib.rs:132-145, 165-242`), then validates once at resolution (`lib.rs:359-379, 664-684`). This keeps construction ergonomic and preserves a single invariant boundary. A typical good use is `leaf(120.0, 24.0).id("title").min_width(100.0).grow(1.0)` inside `SceneSpec::new(...)`, followed by `resolve_scene`; callers receive an error instead of a partially committed layout.

```rust
let spec = SceneSpec::new(leaf(120.0, 24.0).id("title").grow(1.0))
    .surface(SurfaceSpec::frame("title"));
let resolved = resolve_scene(&spec)?; // one checked resolution boundary
```

The corresponding bad pattern is the inapplicable fluent setter shown above: it has the same pleasant syntax but drops the requested value silently.
- `SceneSpec` composes builders (`crates/mui-core/src/scene.rs:113-145`), and `SceneState::commit` is transactional (`scene.rs:468-488`): failed resolution leaves prior state intact, as tests verify (`scene.rs:583-591`, layout tests `784-794`).
- Geometry options are explicit typed structs with `Default` and validation at use (`crates/mui-geometry/src/boolean.rs:64-92`, `offset.rs:14-78`, `fillet.rs:6-43`). `Align`, `Justify`, `Radius`, and `CornerRule` encode choices as enums rather than string flags. Constructors such as `Size::new`, `Insets::all/symmetric`, and `SurfaceSpec::frame/merge/inset/outset` use clear names and consuming builders.
- TS uses discriminated unions for `Node` and `Surface` (`packages/mui-ts/src/index.ts:17-19, 57-60`), helper constructors, and strict compiler settings (`packages/mui-ts/tsconfig.json:2-13`). The compiler’s `n()` guard prevents NaN/Infinity from crossing the generation boundary, while Rust remains the final semantic validator.
- The facade explicitly re-exports stable modules and gates optional egui support (`crates/mui/src/lib.rs:1-17`; `crates/mui/Cargo.toml` feature `egui`). This is a good feature boundary. The TS package is currently private (`packages/mui-ts/package.json:1-8`), so it does not yet make a published npm semver promise.

## Graft coverage

Queries used included `map`, ranked `ask`/`skeleton` for Rust and TS API surfaces, exhaustive `grep` for public structs/enums/functions and relevant setters/validation fields, and `callers` for `leaf`, `defineScene`, `resolve_scene`, `update`, `set_skin`, `commit`, and `valid`. Graft reported approximately **634,739 tokens saved** this turn. The findings above are grounded in those exact spans and caller sets; no source files were edited.
