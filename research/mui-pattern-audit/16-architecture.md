# MUI architecture and boundary audit

Audit snapshot: `ba0617e9cb52cd5f09ed5599de03ac84e1989f2c`. This is a read-only
architecture review. I used `graft map` first, then targeted `graft ask`,
`graft callers`, and `graft skeleton` queries before opening the cited source
spans. The graph is a navigation aid: its 37-file count includes research and
pattern-check material and is not the count of runtime source files.

## Inventory and data flow

The workspace has eleven Rust crates, one build-time TypeScript package, and a
shell verification gate. The crates and package contribute 36 `.rs`/`.ts`
files; repo-wide inventory adds `research/idiomatic-rust/pattern_checks.rs`,
for 37 such files (plus manifests, tests, examples, docs, lockfiles, and
configuration).

| Layer | Package/files | Boundary and dependencies |
|---|---|---|
| Intrinsic layout | `mui-layout` (`src/lib.rs`, `examples/pill.rs`) | Renderer-independent solver with no dependencies (`crates/mui-layout/Cargo.toml:7-9`). It turns a `Node` tree into frames and validates sizes. |
| Geometry | `mui-geometry` (`src/lib.rs`, `boolean.rs`, `fillet.rs`, `math.rs`, `nesting.rs`, `offset.rs`, `path.rs`, `tests.rs`, `examples/offset_demo.rs`) | Path/topology, Boolean operations and offsets; only `i_overlay` (`crates/mui-geometry/Cargo.toml:7-8`). |
| Scene core | `mui-core` (`src/lib.rs`, `scene.rs`, `theme.rs`, `color.rs`) | Owns `SceneSpec`, theme and the dependency resolver; depends on geometry and layout only (`crates/mui-core/Cargo.toml:7-12`). `resolve_scene` validates IDs, missing frames, cycles, depth and options (`crates/mui-core/src/scene.rs:245-305`). |
| Mesh adapter | `mui-tessellate/src/lib.rs` | Geometry path to Lyon `TriangleMesh`, with f32 finiteness checks and non-zero fill (`crates/mui-tessellate/src/lib.rs:48-94`). |
| Text leaf | `mui-text/src/lib.rs` | Font bytes and skrifa produce the same MUI `Path` space; validates size/tolerance (`crates/mui-text/src/lib.rs:167-213`). It does not read files. |
| Renderer seams | `mui-vello/src/lib.rs`, `mui-egui/src/lib.rs` | Vello converts a path to `kurbo::BezPath` (`crates/mui-vello/src/lib.rs:27-57`). Egui is a low-level geometry/mesh helper over `PlacedShape`, Lyon and egui (`crates/mui-egui/src/lib.rs:47-65`), rather than a `SceneSpec` consumer. |
| Interaction | `mui-input/src/lib.rs`, `src/tests.rs` | Hit registration uses `mui-vello::bez_path` and `vello_common` (`crates/mui-input/src/lib.rs:48-81`), then `Interaction` carries capture/click/drag state (`:112-199`). |
| Public facade | `mui/src/lib.rs` | Re-exports core, layout, geometry and tessellation; egui is opt-in (`crates/mui/src/lib.rs:4-17`, `crates/mui/Cargo.toml:7-16`). Native preview, input, text and Vello remain explicit dependencies. |
| Generated demo | `mui-demo/src/generated.rs`, `src/main.rs` | Generated Rust constructs `SceneSpec`; the binary resolves it and checks frame/mesh properties (`crates/mui-demo/src/main.rs:3-46`). Its direct dependencies are core, geometry and layout (`crates/mui-demo/Cargo.toml:7-10`). |
| Native preview | `mui-preview/src/{main,host,scenes,skin,ui}.rs`, integration tests | `scenes::all` supplies four `PreviewScene` objects (`crates/mui-preview/src/scenes.rs:15-45`). `Baked::build` resolves the scene, preserves declaration paint order, converts paths through Vello and registers the same paths with input (`crates/mui-preview/src/main.rs:52-115`). `host.rs` alone owns winit/wgpu (`crates/mui-preview/src/host.rs:27-107`). |
| TypeScript authoring | `packages/mui-ts/src/{index,compiler}.ts`, examples | `index.ts` is a typed scene DSL (`:1-104`). `compiler.ts` is a build-time serializer to Rust (`:81-104`); package scripts compile and generate the checked-in demo (`packages/mui-ts/package.json:6-11`). There is no JS runtime in the generated binary. |
| Build/docs | `tools/verify.sh`, `ARCHITECTURE.md`, `README.md`, `ROADMAP.md`, `BENCHMARKS.md` | The gate formats/tests/lints, checks library WASM excluding preview, compiles TS, regenerates Rust, diffs it, and runs the demo (`tools/verify.sh:6-24`). |

The intended runtime path is:

```text
TypeScript DSL --tsc/compiler--> generated Rust SceneSpec
  -> mui-layout frames
  -> mui-core surface dependency graph (frame/merge/inset/outset)
  -> ResolvedSurface.path
       -> mui-vello -> vello_hybrid/wgpu (preview and headless)
       -> mui-input (the same Vello path for hit testing)

Raw PlacedShape inputs -> mui-egui -> geometry/offset -> Lyon meshes -> egui
```

The preview keeps its chrome outside `SceneSpec` on purpose: `ui.rs` says the
sidebar is hand-built paths through `mui-input`, so a resolver regression does
not make the diagnostic UI unusable (`crates/mui-preview/src/ui.rs:1-10`).
Text and glyph paths join the Vello/mesh/input path as overlays. `mui-vello`
is the current path-to-pixels seam; `mui-tessellate` is still useful for the
debug egui adapter and alternate mesh consumers.

## Good patterns observed

1. **Core owns semantics; hosts own devices.** `mui-core` has no renderer or
   window dependency. The preview owns the event loop, surface, device and
   scene lifetime (`crates/mui-preview/src/host.rs:38-145`). This is a sound
   library/binary split: a plugin host can supply its own window/device while
   the core remains usable on WASM. `mui` also avoids pulling preview into the
   plugin-facing facade.

2. **The resolver is the correctness gate for generated data.** `SceneSpec`
   retains geometry and offset options (`crates/mui-core/src/scene.rs:112-131`),
   and the resolver rejects duplicates, missing references, cycles and excessive
   depth (`:245-305`). `SceneState::commit` is transactional (`:468-488`), so
   an invalid update cannot publish a half-resolved scene. This is a good
   boundary for a build-time frontend: the Rust runtime remains authoritative.

3. **One final path drives paint and hit testing.** Preview converts each
   resolved path once for drawing and pushes the source path into `Hit`
   (`crates/mui-preview/src/main.rs:90-105`). `Hit::at` uses the same non-zero
   winding convention as tessellation and Vello (`crates/mui-input/src/lib.rs:67-80`,
   `crates/mui-tessellate/src/lib.rs:75-88`). This avoids the common bounding-box
   or independently-guessed-radius bug at rounded and concave corners.

4. **State and cache ownership is explicit.** `mui-egui::SurfaceState` commits
   a completely prepared result and increments the revision only after success
   (`crates/mui-egui/src/lib.rs:100-129`); `PathMeshCache` avoids retessellation
   when path and tolerance are unchanged (`:241-273`). Preview rebakes on scene
   changes, not every frame (`crates/mui-preview/src/main.rs:52-55,222-225`).

5. **Generation is deterministic and reviewable.** The checked-in generated
   file is diffed against fresh output (`tools/verify.sh:15-24`) and the demo
   resolves the resulting scene (`crates/mui-demo/src/main.rs:39-46`). The
   generated output contains ordinary Rust structures and no scripting runtime.

6. **The unit of abstraction is appropriately small in several seams.**
   `mui-layout` has no dependency, `mui-vello::bez_path` is one explicit
   conversion function, and `mui-text` accepts font bytes. These choices keep
   pure computation separate from native I/O and make the library crates usable
   on WASM.

## Findings and concrete bad patterns

### F-1 — High: the TS compiler can emit Rust that cannot compile

**Evidence.** `compiler.ts` only emits `palette` when `scene.theme.palette` is
present (`packages/mui-ts/src/compiler.ts:81-96`). `Theme` requires `palette`
(`crates/mui-core/src/theme.rs:115-121`), so a valid TypeScript scene with no
theme or no palette produces an incomplete Rust struct. The compiler's numeric
formatter appends `.0` to every `Number.isInteger` value (`compiler.ts:10-14`),
but JavaScript integer `1e21` stringifies as `1e+21`; the result `1e+21.0` is
invalid Rust. Finally, `q` delegates to `JSON.stringify` (`compiler.ts:10`),
whose `\u0001` escape is invalid in Rust, which requires `\u{0001}`.

These are reproduced in `/tmp/mui-pattern-audit/n-case.rs` and
`q-case.rs`. A temporary Cargo crate including each file reports, respectively,
`missing field palette`, `E0610` for `1e+21.0`, and `incorrect unicode escape
sequence` for `\u0001`. The existing happy-path pill example does not cover
these optional/default cases because it supplies a palette and ordinary decimal
numbers.

```rust
// Bad current output for a valid Scene with no palette:
let theme = mui_core::Theme {
    corners: mui_core::CornerProfile::new(18.0, 14.0),
    spacing: mui_core::SpacingScale { xs: 4.0, s: 8.0, m: 12.0, l: 18.0, xl: 28.0 },
    stroke_width: 1.5,
}; // `palette` is missing

// Minimal correct shape for the default case:
let theme = mui_core::Theme {
    corners: mui_core::CornerProfile::new(18.0, 14.0),
    spacing: mui_core::SpacingScale { xs: 4.0, s: 8.0, m: 12.0, l: 18.0, xl: 28.0 },
    palette: mui_core::Palette::NEUTRAL,
    stroke_width: 1.5,
};
```

**Impact.** The advertised `Scene` type accepts these values, but generation
fails at the Rust boundary. This makes the build-time authoring API partial and
turns ordinary IDs or numeric values into compiler errors far from the TS call.

**Minimal fix.** Always emit a palette (or use `..mui_core::Theme::DEFAULT`),
replace `n` with a Rust-safe float formatter that handles exponent notation,
and use a Rust string serializer that emits brace Unicode escapes or rejects
unsupported control characters with a TS error. Add generator fixtures for no
theme, no palette, `1e21`, and a control-character ID to the existing
generate/diff gate. Confidence: **high**, because all three failures reproduce
without network access.

### F-2 — Medium: WASM scope is contradictory in public documentation

`README.md:8-10` says “The whole workspace, including the egui adapter,
compiles” to WASM, while the actual gate explicitly excludes native
`mui-preview` (`tools/verify.sh:9-13`). `README.md:411-412` and
`ROADMAP.md:68-69` state the narrower, correct scope, but the verify section
still calls this “full-workspace WASM compilation” (`README.md:449-457`).

**Impact.** A reader can reasonably expect `cargo check --workspace` to include
the preview binary and mistake the intentional native-host boundary for a
regression. The code gate is coherent; the contract in the prose is not.

**Minimal fix.** Change the opening and verify prose to “all library crates and
the egui adapter, excluding the native preview host” and link to the exact
`--exclude mui-preview` command. Confidence: **high**; this is a direct docs vs
script mismatch.

### F-3 — Medium: `ARCHITECTURE.md` presents an obsolete render path

The diagram ends `Path -> mui-tessellate -> generic TriangleMesh -> egui /
future wgpu / Skia` (`ARCHITECTURE.md:21-27`). The current runtime instead
uses `mui-vello::bez_path` directly in preview (`crates/mui-preview/src/main.rs:90-105`)
and the headless example; README explicitly says Vello is the path-to-pixels
seam and tessellation is now for debug display (`README.md:414-430`). Egui's
actual API accepts raw `PlacedShape`, unions/fillets/insets with default geometry
options, then tessellates (`crates/mui-egui/src/lib.rs:47-65`).

**Impact.** A contributor following the diagram may treat the debug adapter as
the canonical renderer path, miss Vello's authoritative conversion, or assume
an egui adapter consumes the already-resolved `SceneSpec` path. That can produce
different options and different geometry.

**Minimal fix.** Draw two explicit branches: `ResolvedSurface.path ->
mui-vello -> vello_hybrid/wgpu` and `PlacedShape -> mui-egui ->
mui-tessellate/egui`. Label egui as a low-level/debug adapter, matching
`README.md:338`. Confidence: **high**; the code and current README agree with
each other and disagree with the diagram.

### F-4 — Medium: `mui-input` is coupled to the Vello/wgpu dependency tree

The intended input abstraction is path-based, but its manifest depends directly
on `mui-vello` and `vello_common` (`crates/mui-input/Cargo.toml:7-12`).
`Hit::push` therefore converts every path using the Vello seam
(`crates/mui-input/src/lib.rs:48-59`). `cargo tree -p mui-input --edges normal
--offline` shows this pulls `mui-vello -> vello_hybrid -> wgpu` into the hit
testing crate, even though input itself owns no window or GPU. The WASM check
still passes (`cargo check -p mui-input --target wasm32-unknown-unknown
--offline`), so this is dependency coupling and build weight, not a reported
WASM failure.

**Impact.** A host selecting a different renderer cannot use the input crate
without carrying the Vello conversion path, and dependency/build costs spread
from a logic crate into a renderer-specific graph. The current preview goal of
pixel-identical hit testing makes this coupling understandable, but it is a
real boundary that should remain explicit.

**Minimal fix.** Document the Vello-aligned contract in the input crate and
keep the coupling until a second renderer is real; then reassess the conversion
seam based on that concrete consumer. Avoid introducing a generic renderer trait
speculatively. Confidence: **high** for the dependency fact, **medium** for
severity because the current product deliberately aligns input with Vello.

### F-5 — Low/Medium: the egui helper can diverge from a resolved SceneSpec

`SceneSpec` carries caller-supplied `geometry_options` and `offset_options`
(`crates/mui-core/src/scene.rs:112-131`), and the core resolver uses them for
merge and general offsets (`:337-413`). `mui-egui::prepare`, however, accepts
raw `PlacedShape` and hardcodes `GeometryOptions::default()` before filleting
and insetting (`crates/mui-egui/src/lib.rs:47-65`). It cannot consume a
`ResolvedSurface` or preserve a scene's custom options. The README already calls
egui a debug adapter, so this is a contract footgun rather than proof of a
runtime defect.

**Impact.** A caller may expect the egui path to render the same result as a
core-resolved scene, while custom budgets/tolerances and parent-derived final
paths are not represented by this API.

**Minimal fix.** Keep `prepare` explicitly documented as a low-level helper for
raw shapes, and state that `ResolvedSurface.path` is the authoritative result
for scene rendering. Add a path-only paint/cache entry point only when a second
consumer demonstrates the need; no broad abstraction is justified by the
current tree. Confidence: **high** for the API difference, **medium** for user
impact because the debug-only intent is documented.

### F-6 — Low: cross-reference diagnostics arrive at the Rust boundary

The TS model accepts arbitrary surface IDs and references (`packages/mui-ts/src/index.ts:57-66`)
and the serializer emits them without checking duplicates or references
(`packages/mui-ts/src/compiler.ts:57-64,81-96`). Rust correctly catches empty or
duplicate IDs, missing surfaces/layout frames and cycles (`crates/mui-core/src/scene.rs:245-305`).

**Impact.** A TS author gets a generated-Rust compiler/runtime error instead of
an error at the authoring call. This does not weaken runtime correctness because
the Rust resolver is the final gate, but it weakens iteration and CI diagnostics.

**Minimal fix.** Add a TS validation pass only if better authoring diagnostics
are a goal; otherwise document that Rust resolution is authoritative and keep
the single validation implementation. Confidence: **high** for the boundary,
**low** for severity.

## Coverage and unreviewed surfaces

All eleven workspace crate directories were inventoried from the root manifest:
`mui-core`, `mui-demo`, `mui-egui`, `mui-geometry`, `mui-input`, `mui-layout`,
`mui-preview`, `mui-tessellate`, `mui-text`, `mui-vello`, and `mui`. The
TypeScript package (`packages/mui-ts`) and `tools/verify.sh` were also inventoried.

The following were read at exact line ranges because they define the boundaries
or findings: all workspace/package manifests; `ARCHITECTURE.md`, `README.md`,
`ROADMAP.md`, `BENCHMARKS.md`, `tools/verify.sh`; TS `compiler.ts`, `index.ts`,
and both examples; core `lib.rs`, `scene.rs`, `theme.rs`; demo generated/main;
egui; input; layout API; preview `main.rs`, `host.rs`, `scenes.rs`, and the UI
module header; tessellate; text's run/metrics path; Vello; and the facade.
`graft skeleton` additionally surfaced every definition in the 34 indexed
runtime Rust/TS files with definitions, including geometry modules, tests,
examples, preview integration tests, color, skin and the remaining UI/text
functions.

Detailed semantic review remains unverified for the full bodies of
`mui-geometry/src/{boolean,fillet,math,nesting,offset,path,tests}.rs`,
`mui-geometry/examples/offset_demo.rs`, `mui-input/src/tests.rs`,
`mui-layout/examples/pill.rs`, `mui-preview/src/ui.rs` beyond its boundary and
the targeted paths, `mui-preview/tests/{frame_cost,render_resize}.rs`,
`mui-text` tests, `mui-vello/examples/headless.rs`, and `mui-core/src/color.rs`.
They are surfaced in the graph but should not be treated as line-by-line
audited here. `Cargo.lock`, `package-lock.json`, `bacon.toml`, `opencode.json`,
and `.gitignore` were inventoried but not dependency-audited. Research reports
and generated build artifacts are outside runtime coverage.

The most consequential unreviewed follow-up is the test assertion that every
preview scene bakes and that hit geometry agrees with paint (the README describes
it at `README.md:405-409`): the graph shows those tests, but this audit did not
execute the native preview integration suite. The TS generator's negative cases
were exercised independently because the normal pill fixture does not cover
them.

The caller graph confirms `resolve_scene` is consumed by the demo, preview,
headless Vello example, transactional state and scene tests
(`crates/mui-core/src/scene.rs:460-488`, `crates/mui-demo/src/main.rs:3-46`,
`crates/mui-preview/src/main.rs:73-115`,
`crates/mui-vello/examples/headless.rs:62-102`). `generated_scene` has only the
demo and preview cost-test call paths (`crates/mui-demo/src/main.rs:3-46`,
`crates/mui-preview/tests/frame_cost.rs:108-134`), which is why generator
coverage depends heavily on the shell diff gate.

## Verification and graft tally

- `cargo tree -p mui-input --edges normal --offline`: confirms the input to
  Vello/wgpu dependency path.
- `cargo check -p mui-input --target wasm32-unknown-unknown --offline`: passes.
- Temporary Cargo checks including `/tmp/mui-pattern-audit/n-case.rs` and
  `q-case.rs`: fail with the generator errors described in F-1.
- No repository source files were edited; only this report and temporary files
  under `/tmp/mui-pattern-audit` were created.

Graft savings this turn: **≈391,704 tokens** across the initial map, targeted
asks, caller queries, skeleton queries, and two full source skeleton passes.
The map's indexed-file count was not used as the source coverage count.
