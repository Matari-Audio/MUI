# MUI — intrinsic Rust UI / geometry foundation

**MUI** (Matari-UI) is the UI foundation used by [Matari Audio](https://github.com/Matari-Audio) plugins.
It solves the part that is genuinely hard: laying out a tree intrinsically, merging the resulting
frames into single Boolean surfaces, and deriving correctly nested children from the *final* merged
outline rather than from guessed radii.

The core is renderer-independent and does **not** use Taffy. `mui-layout` has no dependencies at all.
The whole workspace, including the egui adapter, compiles to `wasm32-unknown-unknown`.

> **Status: foundation, not a framework.** There is no text shaping, no widget library and no
> retained state. Pointer hit testing, glyph outlines and a Vello renderer are here; everything
> built on top of them is not. See *Current intentional scope* below for the full
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
- `mui-vello` renders on wgpu through `vello_hybrid`; `mui-egui` remains only as a debug adapter.

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

`mui-preview` is a native gallery binary: winit for the window, wgpu for the
device, `vello_hybrid` for pixels. It resolves every scene in
`crates/mui-preview/src/scenes.rs` through the same `resolve_scene` +
`mui_vello::bez_path` path the real runtime uses, so what you see is the actual
outline rather than a mock.

Its sidebar is built out of MUI itself — hand-built `mui_geometry::Path`
widgets routed by `mui-input`, deliberately *not* a `SceneSpec`, so a resolver
regression cannot take the chrome down with it. Hover lightens a control, press
takes the accent, and dragging the stage moves the specimen.

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
`MUI_PREVIEW_FONT` at a variable font and its axes become sliders in the
sidebar. The segment count moves as you drag one, which is the proof the
outline was re-solved rather than re-scaled.

Adding a scene means adding one `impl PreviewScene` and one line in
`scenes::all()`. `every_scene_bakes` then covers it — a scene that fails to
resolve or converts to nothing fails the test suite. `hit_is_what_was_painted`
then covers the other half: a 64x64 grid per scene, asserting the topmost
painted path and the hit geometry agree at every point.

`mui-preview` is excluded from the wasm gate: it owns a winit event loop and a
wgpu surface. The library crates still check clean on `wasm32-unknown-unknown`.

## Rendering

MUI decides *what the shape is*; Vello decides *what it looks like*. `mui-vello`
is the whole of the seam: one function that turns a resolved `mui_geometry::Path`
into a `kurbo::BezPath`, arcs kept as arcs and handed over as cubics rather than
flattened to a polyline.

```
cargo run -p mui-vello --example headless -- /tmp/pill.png
```

That example is the stack end to end with no window and no tessellator:
intrinsic layout, boolean union, fillets, then analytic antialiasing from
`vello_hybrid`. `vello_hybrid` is a CPU-preprocess / GPU-raster renderer on
wgpu 29 — the version KURV already ships — with a WebGL2 backend, so it needs no
compute shaders and the wasm claim survives. `mui-tessellate` stays for debug
display; it is no longer on the path to pixels.

## Interaction

`mui-input` is the other half of dropping egui: hit testing and pointer
gestures. It hit-tests the *same* `BezPath` the renderer fills, through the same
`mui_vello::bez_path`, so what responds and what you can see cannot drift apart —
the corner of a rounded shape is correctly outside its own bounding-box corner.
A press captures its target until release wherever the pointer then goes, which
is the single most common thing a hand-rolled UI gets wrong.

`mui-preview` above is the working demonstration: every widget in its sidebar
is a path and a `Hit` entry, and nothing else.

Not yet: gradients, strokes, clips and blend modes are all things `vello_hybrid`
supports and MUI does not surface. Scroll and text editing are not in
`mui-input` at all; the gallery's own click-to-focus is a dozen lines in the
binary, which is where it belongs until a second caller wants it.

## Verify

With Rust/cargo, the WASM target, Node and TypeScript available:

```bash
./tools/verify.sh
```

The script checks formatting, native tests, clippy with warnings denied, full-workspace WASM compilation, TypeScript compilation, deterministic TS -> Rust generation, and the end-to-end demo.
