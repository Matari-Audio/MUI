# MUI — intrinsic Rust UI / geometry foundation

**MUI** (Matari-UI) is the UI foundation used by [Matari Audio](https://github.com/Matari-Audio) plugins.
It solves the part that is genuinely hard: laying out a tree intrinsically, merging the resulting
frames into single Boolean surfaces, and deriving correctly nested children from the *final* merged
outline rather than from guessed radii.

The core is renderer-independent and does **not** use Taffy. `mui-layout` has no dependencies at all.
The whole workspace, including the egui adapter, compiles to `wasm32-unknown-unknown`.

> **Status: foundation, not a framework.** There is no input handling, no text shaping, no widgets,
> no retained state and no native renderer yet. See *Current intentional scope* below for the full
> list of what is deliberately absent. Everything that *is* here is tested and measured rather than
> asserted; run `tools/verify.sh` to reproduce.

## What is implemented

- `mui-layout`: small intrinsic row/column/overlay solver with hug-content sizing, padding, gaps, alignment, justification, min/max, weighted growth, validation and transactional state.
- `mui-geometry`: Boolean union/intersection/difference/XOR, adaptive convex/concave fillets, exact rounded-rectangle inset/outset, general parallel path offsets, holes, topology cleanup and validation.
- `mui-core`: layout + surface dependency resolver. A surface can be a layout frame, a Boolean merge, or a parallel inset/outset of another resolved surface.
- `mui-tessellate`: renderer-independent path -> triangle mesh adapter using Lyon.
- `mui-egui`: thin egui paint adapter plus a path/tessellation cache.
- `@matari/mui`: a build-time TypeScript authoring frontend. TypeScript generates typed Rust builder code; there is no JavaScript runtime in the plugin.
- `mui-demo`: end-to-end TypeScript-authored pill/tab scene compiled into Rust and resolved by the Rust core.

All reusable Rust library crates use `#![forbid(unsafe_code)]`.

## The important rounding distinction

There are two different semantics:

### Styling relationship

```rust
FrameRadius::ParentNormalized {
    parent: "card".into(),
    scale: 1.0,
}
```

This gives a child the same dimensionless visual roundness (`radius / short_side`). It does **not** promise a constant-width shell.

### Geometric relationship

```rust
SurfaceSpec::inset("pill-shell", "tab", Spacing::px(12.0))
```

This derives the child from the parent's final boundary. For a rounded rectangle it is analytic:

```text
child bounds  = parent bounds inset by d
child convex radius = parent radius - d
```

so the two arcs are concentric and the gap is exactly `d`. For a merged shape, MUI offsets the **final filleted path**, including concave shoulders. Concave offset radii naturally move in the opposite direction from convex ones.

## Rust authoring

```rust
use mui_core::{CornerProfile, SceneSpec, Spacing, SurfaceSpec, Theme};
use mui_layout::{Align, Node, Size};

let controls = Node::column("controls", [
    Node::leaf("plus",  Size::new(28.0, 28.0)),
    Node::leaf("pie-a", Size::new(28.0, 28.0)),
    Node::leaf("pie-b", Size::new(28.0, 28.0)),
])
.gap(10.0)
.align(Align::Center);

let tab = Node::column("tab-frame", [
    Node::column("pill-frame", [controls]).padding(10.0),
])
.padding(12.0)
.min_size(Size::new(92.0, 0.0));

let root = Node::column("root", [
    tab,
    Node::leaf("panel-frame", Size::new(520.0, 230.0)),
])
.align(Align::Start);

let scene = SceneSpec::new(root)
    .theme(Theme {
        corners: CornerProfile::new(28.0, 32.0),
        ..Theme::default()
    })
    .surface(SurfaceSpec::frame("panel", "panel-frame"))
    .surface(SurfaceSpec::frame("tab", "tab-frame"))
    .surface(SurfaceSpec::merge("outer", ["panel", "tab"]))
    .surface(SurfaceSpec::inset("pill-shell", "tab", Spacing::px(12.0)));

let resolved = mui_core::resolve_scene(&scene)?;
```

No pixel Y positions are needed for the controls. Their stack sizes the pill content, the pill sizes the tab, and the tab/parent geometry resolves afterward.

## TypeScript authoring, Rust runtime

`packages/mui-ts/examples/pill.ts` is real TypeScript:

```ts
export default defineScene({
  theme: { corners: { convex: 28, concave: 32 } },

  root: column("root", [
    column("tab-frame", [
      column("pill-frame", [
        column("controls", [
          leaf("plus", [28, 28]),
          leaf("pie-a", [28, 28]),
          leaf("pie-b", [28, 28]),
        ], { gap: 10, align: "center" }),
      ], { padding: 10 }),
    ], { padding: 12, min: [92, 0] }),

    leaf("panel-frame", [520, 230]),
  ], { align: "start" }),

  surfaces: [
    frameSurface("panel", "panel-frame"),
    frameSurface("tab", "tab-frame"),
    mergeSurface("outer", ["panel", "tab"]),
    insetSurface("pill-shell", "tab", px(12)),
  ],
});
```

The compiler emits `crates/mui-demo/src/generated.rs`. The final Rust binary contains normal Rust structures; it does not embed V8, QuickJS, Node or TypeScript.

```text
ui.ts
  -> tsc (build/dev only)
  -> @matari/mui compiler
  -> generated Rust builders / future compact IR
  -> MUI Rust core
```

## Scene geometry rules

`SurfaceSource::Frame` keeps two representations:

- a **sharp** rectangular Boolean basis;
- a rounded render path.

`SurfaceSource::Merge` unions the bases first, then applies the global convex/concave corner profile. This prevents old rounded corners from leaking into newly-created junctions.

`SurfaceSource::Inset` and `Outset` derive from the parent's **final rendered path**, because nesting is a geometric relationship rather than a new style pass.

Dependency resolution is recursive, order-independent and cycle-checked.

## Safety / plugin rules

- UI/layout/Boolean/tessellation work belongs on the UI thread, never the audio callback.
- A failed `SceneState::commit` leaves the previously-published scene and revision untouched.
- `PathMeshCache` avoids retessellation when neither the path nor tolerance changed.
- No global mutable UI state is required; state is per plugin instance.
- NaN/infinite/negative invalid inputs, dependency cycles, duplicate keys and budget violations are rejected explicitly.

## Current intentional scope

This is not a complete application framework yet. In particular:

- text shaping/wrapping is an external leaf-measurement concern;
- scroll/virtualization/grid are not implemented yet;
- input/focus/accessibility and plugin parameter gestures are future layers;
- the general parallel offset uses a bounded polygon approximation for circular paths; rounded rectangles use exact analytic offsets;
- wgpu rendering is not implemented here; egui is the current adapter.

The public authoring model is owned by MUI, so Taffy or another solver can still be added as an optional backend later without changing plugin code.

## Glyphs as geometry

`mui-text` turns a glyph into a `mui_geometry::Path` — the same type every
surface is made of — rather than into a texture:

```rust
let path = mui_text::glyph_path(font_bytes, 'a', 220.0, &[("wght", 700.0)], 0.05)?;
```

Variable-font axes are an argument, not a font variant, so the outline is
re-derived at whatever axis position is asked for. That is what makes a
Material Symbols icon animating from unfilled to filled (`FILL` 0 → 1) a
geometry change: no glyph atlas is being refilled with an entry per
intermediate value.

`mui_text::axes` reports what a face actually declares, with its real ranges,
so a control cannot leave the design space. A static font declares none, and
axis settings it lacks are ignored rather than rejected.

Curves are flattened on the way in, because `PathCommand` carries lines and
exact circular arcs only. That vocabulary is right for the corner system and
wrong for a typeface — no bezier in a font is a circular arc — so the
tolerance is a property of the call and the path should be re-derived if the
display scale changes.

Not yet: shaping, line breaking, bidi, fallback chains, colour glyphs, or
contour winding classification (which is what a glyph needs before it can
Boolean-merge with a surface instead of being drawn over one).

## Preview and live iteration

`mui-preview` is a native gallery binary. It resolves every scene in
`crates/mui-preview/src/scenes.rs` through the same `resolve_scene` +
`Tessellator` path the real runtime uses, then paints the triangles, so what
you see is the actual mesh rather than a mock.

```
cargo run -p mui-preview
```

For the edit-and-watch loop, [bacon](https://dystroy.org/bacon/) rebuilds and
relaunches the window on every save:

```
bacon
```

`bacon check` and `bacon test` are the cheaper jobs when you do not need the
window. Editing a scene and seeing the new window costs about half a second
of build time on a warm target directory. The preview is a dev host, so no
file watching, scripting, or reload machinery lives in the library crates.

The **Glyph axes** scene is the live version of the section above: point
`MUI_PREVIEW_FONT` at a variable font and its axes become sliders, with the
triangle count in the status bar moving as you drag one.

Adding a scene means adding one `impl PreviewScene` and one line in
`scenes::all()`. `every_scene_bakes` then covers it — a scene that fails to
resolve or tessellates to nothing fails the test suite.

`mui-preview` is excluded from the wasm gate: `eframe::run_native` is
native-only. The library crates still check clean on `wasm32-unknown-unknown`.

## Verify

With Rust/cargo, the WASM target, Node and TypeScript available:

```bash
./tools/verify.sh
```

The script checks formatting, native tests, clippy with warnings denied, full-workspace WASM compilation, TypeScript compilation, deterministic TS -> Rust generation, and the end-to-end demo.
