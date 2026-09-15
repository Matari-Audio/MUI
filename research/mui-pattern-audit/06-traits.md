# MUI traits, generics, dynamic dispatch, and scene contracts

## Audit result

The indexed Rust workspace has one custom trait declaration: `PreviewScene` in `crates/mui-preview/src/scenes.rs:15-36`. The five `dyn` matches are the intended gallery storage and handoff: `Vec<Box<dyn PreviewScene>>` in `scenes.rs:38-45` and `main.rs:157-159`, plus borrowed `&dyn PreviewScene` at `main.rs:73` and `main.rs:522`. The gallery contains four heterogeneous scene implementations, selected at runtime, so this is a correct use of dynamic dispatch. There is no renderer trait hierarchy or generic renderer abstraction to simplify: `mui-vello` exposes one validated seam, `bez_path(&Path, tolerance)` at `crates/mui-vello/src/lib.rs:27-57`.

I found one concrete medium-severity scene API bug and one informational contract observation. No high-severity misuse of `dyn`, over-constrained generic bounds, or object-safety failure was found.

## P2 — variant-specific `SurfaceSpec` options silently do nothing

`crates/mui-core/src/scene.rs:98-109` implements `SurfaceSpec::radius` by mutating only `SurfaceSource::Frame` and `SurfaceSpec::corners` by mutating only `SurfaceSource::Merge`. Calling either method on another source returns the unchanged `SurfaceSpec` without an error:

```rust
// Compiles, but the requested radius is discarded.
let s = SurfaceSpec::inset("inner", "card", Spacing::px(8.0))
    .radius(Radius::Absolute(4.0));
```

The impact is a wrong rendered scene with no diagnostic: a caller can believe a style relationship was applied while `resolve_scene` uses the original inset unchanged. The trigger is any hand-authored Rust call that chains `.radius(...)` on an inset/outset/merge or `.corners(...)` on a frame/inset/outset. The generated TypeScript model already prevents this category at its input boundary: `packages/mui-ts/src/index.ts:57-60` is a discriminated union where only `frame` carries `radius` and only `merge` carries `corners`; `packages/mui-ts/src/compiler.ts:57-64` emits the matching Rust call.

Minimum compatible fix: document the variant preconditions on both methods and add tests that lock in the intended behavior. If callers need a checked failure, add opt-in checked methods or a source-specific builder; changing the existing fluent methods to return a configuration `Result` would be a multi-file, compatibility-affecting redesign and should be weighed against the crate’s current pre-1.0 status. A stronger API is source-specific typed builders (`FrameSpec::radius`, `MergeSpec::corners`), which makes misuse a compile error. Confidence: high (0.97) for the silent behavior; medium (0.90) that it is an actionable bug rather than an intentional no-op convention.

## Informational — stage target IDs are an implicit shared namespace

`PreviewScene::overlay` returns `Vec<(String, Path)>` at `crates/mui-preview/src/scenes.rs:21-25`. `Baked::build` appends those overlays to resolved surface paths at `crates/mui-preview/src/main.rs:90-105`, then registers each string with `Hit::push` at `crates/mui-preview/src/main.rs:95-100`. `Hit::push` accepts every ID without checking duplicates (`crates/mui-input/src/lib.rs:49-60`), while `Interaction` stores only an ID string and compares it in `update`/`get` (`crates/mui-input/src/lib.rs:156-219`).

This may be intentional: repeated IDs can make several painted paths one logical target. The `PreviewScene` contract does not state whether overlay IDs may collide with surface IDs, however. If a custom scene gives two independent overlays the same ID, the topmost path wins in `Hit::at`; both controls then appear as the same hovered/held/dragged response. If an overlay reuses a surface ID accidentally, the stage has no way to distinguish the targets. The built-in scenes currently avoid the collision (`GlyphAxes` uses a glyph label while its surface is `card`), so there is no built-in repro and this remains a contract observation rather than a confirmed defect.

Minimum fix: document that IDs are the stage’s logical target keys, that repeated IDs intentionally group paths, and that independent targets should use unique IDs across both `SceneSpec::surfaces` and `overlay()`. Add focused tests only if the grouping/uniqueness semantics become part of the intended contract. Making `Hit::push` reject duplicates would be a deliberate semantic change because grouping may be useful. Confidence: high (0.95) for the observed aliasing behavior, low (0.50) that it warrants a code change today.

## Good patterns and why they fit

`PreviewScene` is object-safe by construction: all methods have ordinary receivers, no generic methods, no `Self` in inputs/outputs, and no associated types. `all()` uses `Box<dyn PreviewScene>` only where heterogeneous ownership is required; `Baked::build` borrows `&dyn PreviewScene`, avoiding an extra allocation at each rebuild. `App::new` and `rebake` rebuild the selected scene through that single boundary (`crates/mui-preview/src/main.rs:195-224`), and `chrome_frame` mutably dispatches `controls` only for the selected scene (`main.rs:330-370`). The runtime selection and four different concrete scene types justify the vtable and allocation.

The core APIs use narrow generic capability contracts. `Node::id` takes `impl Into<String>` (`crates/mui-layout/src/lib.rs:163-168`), and `row`, `column`, and `overlay` take `impl IntoIterator<Item = Node>` (`lib.rs:247-268`), so callers can pass arrays, vectors, or other iterables without exposing storage choices. `SurfaceSpec::merge` uses the same conversion pattern (`crates/mui-core/src/scene.rs:68-78`) because it must own IDs eventually. These bounds add real input flexibility and no speculative trait abstraction.

`ResolvedScene::surfaces` returns `impl Iterator<Item = (&str, &ResolvedSurface)>` (`crates/mui-core/src/scene.rs:168-174`), hiding the `BTreeMap` representation while exposing the needed capability. The renderer seam is similarly small: `mui-vello::bez_path` validates a `Path` before converting it, so backend code does not need to know geometry internals. `resolve_scene` keeps the layout and surface composition in `mui-core` (`crates/mui-core/src/scene.rs:460-464`) and leaves Vello-specific work to the renderer crate.

The trait’s `&'static str` metadata and concrete `&mut crate::ui::Ui<'_>` control surface are not current downstream API defects. `mui-preview` is `publish = false` and is a binary with a private `mod scenes`, so `PreviewScene` is an internal extension point. If the gallery becomes a library or plugin API, the first changes should be returning `&str` for state-dependent/localized labels and splitting scene geometry from UI controls; keeping a backend-specific UI type in a public plugin trait would otherwise freeze that boundary.

## Verification and coverage

`cargo test -p mui-preview` passed all 25 active tests. Two frame-cost tests are intentionally ignored as measurements, and one GPU resize test is ignored without a GPU. Existing tests cover every built-in scene baking (`crates/mui-preview/src/main.rs:543-550`), paint order, hit/paint agreement, and rebake behavior, but they do not instantiate an external/custom `PreviewScene`, exercise duplicate overlay IDs, or compile-check wrong `SurfaceSpec` modifiers. Those are the smallest useful additions if either finding is fixed.

The graft index reported 37 files, 731 symbols, and 1,174 edges. This audit used exhaustive trait and `dyn` searches plus targeted source/caller tracing for `PreviewScene`, `Baked::build`, `Hit::push`, `SurfaceSpec` modifiers, scene resolution, generic layout constructors, and the TypeScript scene union. Graft reported approximately 407,815 tokens saved this turn.
