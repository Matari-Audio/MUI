# MUI — intrinsic Rust UI / geometry foundation

**MUI** (Matari-UI) is the UI foundation used by [Matari Audio](https://github.com/Matari-Audio) plugins.
It solves the part that is genuinely hard: laying out a tree intrinsically, merging the resulting
frames into single Boolean surfaces, and deriving correctly nested children from the *final* merged
outline rather than from guessed radii.

The core is renderer-independent and does **not** use Taffy. `mui-layout` has no dependencies at all.
The whole workspace, including the egui adapter, compiles to `wasm32-unknown-unknown`.

> **Status: foundation, not a framework.** There is no text shaping, no widget library and no
> retained state. Pointer hit testing, glyph outlines and a Vello renderer are here; everything
> built on top of them is not. See *Current intentional scope* below for what is
> deliberately absent, and `ROADMAP.md` for the ticked/unticked inventory of the
> whole workspace. Everything that *is* here is tested and measured rather than
> asserted; run `tools/verify.sh` to reproduce.

## What is implemented

- `mui-layout`: small intrinsic row/column/overlay solver with hug-content sizing, padding, gaps, alignment, justification, min/max, weighted growth, validation and transactional state.
- `mui-geometry`: Boolean union/intersection/difference/XOR, adaptive convex/concave fillets, exact rounded-rectangle inset/outset, general parallel path offsets, holes, topology cleanup and validation.
- `mui-core`: layout + surface dependency resolver, plus the theme and its Oklch palette. A surface can be a layout frame, a Boolean merge, or a parallel inset/outset of another resolved surface.
- `mui-tessellate`: renderer-independent path -> triangle mesh adapter using Lyon.
- `mui-text`: glyph and string outlines from a variable font, as paths in the same space as every other surface. Nothing reorders or substitutes; there is no atlas.
- `mui-input`: pointer hit testing over paths, with press capture, hover, click and drag.
- `mui-vello`: path -> `vello_hybrid` scene, on wgpu.
- `mui-egui`: thin egui paint adapter plus a path/tessellation cache.
- `@matari/mui`: a build-time TypeScript authoring frontend. TypeScript generates typed Rust builder code; there is no JavaScript runtime in the plugin.
- `mui-demo`: end-to-end TypeScript-authored pill/tab scene compiled into Rust and resolved by the Rust core.
- `mui-preview`: a windowed gallery that resolves a scene live and drives it with the widgets in `mui-input`.

All reusable Rust library crates use `#![forbid(unsafe_code)]`.

## The important rounding distinction

There are two different semantics:

### Styling relationship

```rust,ignore
Radius::ParentNormalized {
    parent: "card".into(),
    scale: 1.0,
}
```

This gives a child the same dimensionless visual roundness (`radius / short_side`). It does **not** promise a constant-width shell.

### Geometric relationship

```rust,ignore
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
use mui_layout::{column, leaf, Align};

let controls = column([
    leaf(28.0, 28.0).id("plus"),
    leaf(28.0, 28.0).id("pie-a"),
    leaf(28.0, 28.0).id("pie-b"),
])
.gap(10.0)
.align(Align::Center);

// Only a node you look up, or take a surface from, needs a name. The
// wrapper below is structural, so it stays anonymous.
let tab = column([column([controls]).padding(10.0)])
    .id("tab")
    .padding(12.0)
    .min_width(92.0);

let root = column([tab, leaf(520.0, 230.0).id("panel")])
    .id("root")
    .align(Align::Start);

let scene = SceneSpec::new(root)
    .theme(Theme {
        corners: CornerProfile::new(28.0, 32.0),
        ..Theme::default()
    })
    .surface(SurfaceSpec::frame("panel"))
    .surface(SurfaceSpec::frame("tab"))
    .surface(SurfaceSpec::merge("outer", ["panel", "tab"]))
    .surface(SurfaceSpec::inset("pill-shell", "tab", Spacing::px(12.0)));

let resolved = mui_core::resolve_scene(&scene).expect("the scene resolves");
```

No pixel Y positions are needed for the controls. Their column sizes the pill content, the pill sizes the tab, and the tab/parent geometry resolves afterward.

## TypeScript authoring, Rust runtime

`packages/mui-ts/examples/pill.ts` is real TypeScript:

```ts
export default defineScene({
  theme: {
    corners: { convex: 28, concave: 32 },
    palette: { accent: [0.752, 0.131, 242], step: 0.045, hover: 0.11 },
  },

  root: column([
    column([
      column([
        column([
          leaf([28, 28], { id: "plus" }),
          leaf([28, 28], { id: "pie-a" }),
          leaf([28, 28], { id: "pie-b" }),
        ], { gap: 10, align: "center" }),
      ], { padding: 10 }),
    ], { id: "tab", padding: 12, min: [92, 0] }),

    leaf([520, 230], { id: "panel" }),
  ], { id: "root", align: "start" }),

  surfaces: [
    frameSurface("panel"),
    frameSurface("tab"),
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

## Colour

A theme carries a `Palette`: a `Mode`, seven `Pigment`s, and two steps.
Everything else -- every surface, every ink, every state, and the other theme --
is derived.

A `Pigment` is a hue and a chroma with no lightness, because lightness is not a
role's identity. "Our blue" stays our blue on a white ground; what changes is
where it sits between the ground and the ink, and that is what `Mode` decides.
So a light theme is one field, not a second table to keep in step with the first.

`Palette::NEUTRAL` is the default and is deliberately tasteless: greys for the
brand roles, plus the three hues that "success", "warning" and "danger" already
mean everywhere. Which colour is *yours* is the one decision a layout library
has no business making. An application declares what differs and inherits the
rest.

```rust
use mui_core::{Mode, Palette, Pigment};

// The crate ships greys and three status hues; the brand is your decision.
let skin = Palette {
    neutral: Pigment::new(264.0, 0.015),  // a faintly cool grey for everything
    primary: Pigment::new(242.0, 0.131),
    ..Palette::NEUTRAL
};

skin.background();        // the window
skin.surface();           // a panel lifted off it
skin.raised();            // a control that looks pressable
skin.field();             // a well: a text input, a list row, a slider track
skin.layer(4);            // anything further, in the same perceptual step

skin.primary();           // the brand roles, placed at this mode's accent lightness
skin.secondary();
skin.tertiary();
skin.success();
skin.warning();
skin.danger();

skin.hover(skin.primary());     // what the pointer does to it
skin.pressed(skin.primary());
skin.disabled(skin.primary());
skin.on(skin.raised());   // ink that reads on that fill, guaranteed 4.5:1
skin.dim(skin.raised());  // a secondary label, dimmed only as far as 4.5:1 allows

skin.with_mode(Mode::Light);    // the whole theme switch
skin.primary().to_srgb();       // gamut-mapped, ready to paint
```

### Where a theme lives

One file, one `const`. `Theme::DEFAULT` exists so it can be a `const` -- state
what differs, spread the rest, and the fields you did not mention keep tracking
the crate instead of drifting from it silently.

```rust
// src/skin.rs -- the only place in the application where a colour is chosen.
use mui_core::{CornerProfile, Mode, Palette, Pigment, Theme};

pub const SKIN: Theme = Theme {
    palette: Palette {
        neutral: Pigment::new(264.0, 0.015),  // a faintly cool grey
        primary: Pigment::new(242.0, 0.131),
        secondary: Pigment::new(310.0, 0.120),
        tertiary: Pigment::new(190.0, 0.110),
        step: 0.045,   // one layer's depth
        hover: 0.11,   // what the pointer adds
        ..Palette::NEUTRAL
    },
    corners: CornerProfile::new(28.0, 32.0),
    ..Theme::DEFAULT
};

pub fn skin(light: bool) -> Palette {
    SKIN.palette.with_mode(if light { Mode::Light } else { Mode::Dark })
}
```

There is no light-theme half to that file, because there is nothing in it a
mode could contradict. `crates/mui-preview/src/skin.rs` is exactly this file,
and the gallery's "light theme" checkbox is the only other thing involved.

If you author scenes in TypeScript, the same file is an object and the compiler
emits the `const` for you:

```ts
// examples/skin.ts
import type { Theme } from "@matari/mui";

export const skin: Theme = {
  corners: { convex: 28, concave: 32 },
  palette: { primary: [242, 0.131], step: 0.045, hover: 0.11 },
};
```

Which of the two you author in is a build decision, not a design one -- the
shape is deliberately the same.

### What the neutrals are allowed to do

`layer(n)` is `ground + n * step`, walking toward the ink for positive levels
and away for negative ones. Both ends are stated, not left to chance:

| | dark | light |
|---|---|---|
| `ground` -- `layer(0)` | 0.22 | 0.93 |
| `limit` -- as far as a surface may go | 0.50 | 0.60 |
| `ink` -- full-strength type | 0.93 | 0.18 |
| `accent` -- where a saturated role sits | 0.75 | 0.55 |

`Mode::limit` is where full-strength `ink()` stops clearing 4.5:1, so **every
colour `layer(n)` can return takes full-strength ink at AA**, for any `n`.
Levels past either end saturate; a test sweeps to level 1000 and back. The
ground is deliberately not at black or white either, because `field()` sits on
the far side of it and needs somewhere to be.

The four names are fixed levels: `field()` is -1, `background()` 0, `surface()`
1, `raised()` 2.

One step is a **depth cue, not an accessibility boundary** -- adjacent layers
land near 1.15:1. The palette says so rather than implying otherwise:

```rust
use mui_core::Palette;
let skin = Palette::NEUTRAL;

skin.separation(0, 2);                     // what "raised" is actually worth
skin.levels_for(Palette::UI_NONTEXT);      // where 1.4.11's 3:1 really is
```

On a light ground that is level 7. On a dark ground it is `None`: the whole
band from black to `limit` tops out near 2.3:1 against the ground, so a dark
theme cannot make a control visible by fill alone -- which is why dark
interfaces draw borders. Better to return `None` than a level that lies.

Colours are Oklch because every derivation above is a move in lightness or
chroma, and only a perceptual space makes the same move look the same on every
hue: nudging sRGB channels lightens a yellow and barely touches a blue. A light
theme is `step` and `hover` negated and two colours swapped -- not a second
table of literals to keep in step with the first. `on` picks whichever of ink
and surface sits further from the background in perceptual lightness, so the
flip stays legible.

Legibility is checked, not assumed. `Color::contrast` is the WCAG 2.1 ratio,
and `readable_on(bg, ratio)` returns the least-changed version of a colour that
clears it: hue and chroma held, lightness pushed away from `bg` until the ratio
is met, saturating at black or white if even that falls short. `on` and `dim`
both go through it at `Palette::AA_TEXT`, so a label cannot be illegible by
construction -- including on the accent, where the ink flips dark on its own.

WCAG 2.1 is what an audit measures against, so it is what ships. It is also
known to over-credit dark-on-dark, which APCA (the WCAG 3 draft) exists to fix;
that is a roadmap item, not a claim made here.

`to_srgb` gamut-maps by CSS Color 4 13.2: hold lightness and hue, bisect chroma
down until the clipped result is within a just-noticeable difference. The
`color` crate deliberately ships only per-component clipping, which its own docs
call perceptually poor -- lifting the accent by 0.15 L and clipping swings its
hue 27.5 degrees toward cyan, where bisection holds it to 7.3.

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
- NaN/infinite/negative invalid inputs, dependency cycles, duplicate node ids and budget violations are rejected explicitly.

## Current intentional scope

This is not a complete application framework yet. In particular:

- text shaping/wrapping is an external leaf-measurement concern;
- scroll/virtualization/grid are not implemented yet;
- accessibility and plugin parameter gestures are future layers; pointer input and focus exist only as `mui-preview`'s own widgets, not as a library;
- the general parallel offset uses a bounded polygon approximation for circular paths; rounded rectangles use exact analytic offsets;
- `mui-vello` renders on wgpu through `vello_hybrid`; `mui-egui` remains only as a debug adapter.

The public authoring model is owned by MUI, so Taffy or another solver can still be added as an optional backend later without changing plugin code.

## Glyphs as geometry

`mui-text` turns a glyph into a `mui_geometry::Path` — the same type every
surface is made of — rather than into a texture:

```rust,ignore
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

```bash
cargo run -p mui-preview
```

For the edit-and-watch loop, [bacon](https://dystroy.org/bacon/) rebuilds and
relaunches the window on every save:

```bash
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

```bash
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
