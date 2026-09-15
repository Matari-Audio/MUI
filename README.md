# MUI — a styled tree in, pixels and gestures out

**MUI** (Matari-UI) is the UI foundation for [Matari Audio](https://github.com/Matari-Audio)
plugins. You write a tree the way you would write CSS flexbox with tokens; MUI
lays it out intrinsically, turns every welded group into one filleted outline,
derives every shell as a true parallel inset of the outline before it, colours
every surface from a role palette, and hands a z-ordered paint list to Vello.
Nothing is placed absolutely: the only coordinates in the system are an
anchor and an offset, and a slider thumb sits where two flex weights put it.

Every library crate is `#![forbid(unsafe_code)]`, dependency-light, and
compiles to `wasm32-unknown-unknown`. The native preview host is the one
exception.

## One frame

```rust
use mui::prelude::*;

let mut ui = Ui::new(Theme::DEFAULT);
let mut cutoff = 0.5;
let mut bypass = false;

// Built every frame, like an immediate-mode tree. Widgets read last frame's
// gesture on their id, so state lives in your own variables.
let root = column([
    row([text("Filter").text_size(18.0), spacer(), toggle(&mut ui, "bypass", &mut bypass)])
        .align(Align::Center),
    slider(&mut ui, "cutoff", "Cutoff", &mut cutoff, 0.0..=1.0),
])
.gap(M)
.pad(L)
.radius(20.0)
.fill(Role::Surface)
.shadow(Shadow::soft(12.0));

let frame = ui
    .frame(root, Some(Size::new(280.0, 120.0)), PointerInput::default(), 1.0 / 60.0)
    .unwrap();
assert!(frame.scene.paint.len() > 5);
// mui::vello::paint(&mut vello_scene, frame.scene, Affine::IDENTITY)?;
```

`Ui::frame` advances gestures and springs, styles the tree by state (hover
and press are mixed into a named node's fill through a spring), resolves
layout and geometry, and rebuilds the hit regions from the paths it painted,
so what responds and what you see cannot drift apart. `frame.animating` says
whether to schedule another frame.

## The DSL

```rust
use mui::prelude::*;

let control = |id: &str| leaf(28.0, 28.0).pill().fill(Role::Primary).id(id);

// A tab whose shell is a parallel inset of its own rounded outline.
let tab = column([control("plus"), control("phase"), control("warp")])
    .gap(10.0)
    .pad(22.0)
    .min_width(92.0)
    .align(Align::Center)
    .id("tab")
    .shell(12.0, Role::Raised);

// The tab and the panel welded into one filleted shape.
let root = column([tab, leaf(520.0, 230.0).id("panel")])
    .align(Align::Start)
    .weld(Role::Surface);

let scene = resolve_scene(&SceneSpec::new(root)).unwrap();
assert_eq!(scene.surface("tab").unwrap().frame.size.width, 92.0);
```

| you write | it means |
|---|---|
| `row([..])`, `column([..])`, `overlay([..])`, `grid(3, [..])` | flex axis, stack, cells |
| `leaf(w, h)`, `spacer()`, `text("..")` | a sized box, a `grow(1)` gap, a measured text run |
| `.gap(M)`, `.pad(S)`, `.pad(12.0)` | spacing tokens `Xs S M L Xl` from the theme, or pixels |
| `.grow(w)`, `.shrink(w)`, `.basis(px)`, `.expand()` | flexbox weights |
| `.width(Len::Pct(50.0))`, `.aspect(16.0 / 9.0)` | percentage and ratio sizes |
| `.align(..)`, `.justify(..)`, `.anchor(x, y)`, `.offset(dx, dy)` | cross axis, main axis, overlay placement, nudge |
| `.fill(Role::Primary)`, `.fill(Color::..)`, `.fill(Gradient::vertical(a, b))` | a palette role, a literal, a gradient |
| `.stroke(Role::Ink)`, `.radius(8.0)`, `.pill()`, `.shadow(Shadow::soft(12.0))` | outline, corners, shadow |
| `.shell(d, fill)` | a parallel inset of the outline before it, cumulative |
| `.weld(fill)` | paint the union of the children's frames as one filleted shape |
| `.id("name")` | a gesture target and a lookup key; unnamed nodes are decoration |

Alignment is inherited: a child without `.anchor` sits where its parent's
`align` and `justify` say, and `Stretch` is the default cross-axis value so
a row of controls fills its column unless told otherwise.

## Colour

A theme carries a `Palette`: a `Mode`, seven `Pigment`s (hue and chroma, no
lightness), and two steps. Every surface, every ink, every state and the
other theme are derived. Ink is not a colour you pick: `Role::Ink` and
`Role::Dim` resolve against the fill they sit on, and `Palette::on` is
checked at 4.5:1.

```rust
use mui::prelude::*;

pub const SKIN: Theme = Theme {
    palette: Palette {
        neutral: Pigment::new(264.0, 0.015),
        primary: Pigment::new(242.0, 0.131),
        step: 0.045,
        hover: 0.11,
        ..Palette::NEUTRAL
    },
    corners: CornerProfile::new(28.0, 32.0),
    ..Theme::DEFAULT
};
let light = SKIN.palette.with_mode(Mode::Light);
assert!(light.valid());
```

`crates/mui-preview/src/skin.rs` is exactly this file. There is no
light-theme half because there is nothing in it a mode could contradict.

## Geometry rules

- A plain node's outline is its frame rounded by `Radius::{Theme, Px, Scale, Pill}`.
- A welded node unions the children's **sharp** frames first and fillets the
  result second, with the theme's convex and concave radii, so old rounded
  corners never leak into a new junction.
- A shell is an inset of the **final** outline before it: analytic for a
  rounded rectangle (`radius − d`, concentric arcs), a parallel offset of the
  filleted path for a weld. A shell that would collapse simply stops.
- Text is a glyph outline from `mui-text`, in the same space as every other
  path, so a variable-font axis change is a geometry change.
- Zero-area frames are invisible, not errors: a flex share may collapse.

## Crates

| crate | what it owns |
|---|---|
| `mui-layout` | the dependency-free flex solver: tokens, pct, aspect, grid, anchors, frames in tree order |
| `mui-geometry` | Booleans, fillets, exact rounded-rect insets, general parallel offsets |
| `mui-text` | glyph and string outlines from a (variable) font |
| `mui-core` | `El` + `Styled` DSL, roles and palette, the walk from tree to `ResolvedScene` paint list, `Spring` |
| `mui-input` | hit testing against real paths, press capture, hover, click, drag |
| `mui-vello` | `Canvas` over `vello_hybrid::Scene` and `vello_cpu::RenderContext`; `paint(canvas, scene, transform)` |
| `mui` | `Ui` runtime and widgets (`slider`, `knob`, `toggle`, `button`); the `prelude` |
| `mui-tessellate`, `mui-egui` | triangle meshes and the egui debug adapter |
| `mui-preview` | the winit + wgpu gallery, itself one `mui` tree |

## Preview

```bash
cargo run -p mui-preview
```

The whole window is one tree: the sidebar's scene list, toggles and sliders
are `mui` widgets, the specimen is an anchored child of the stage, and a drag
on the stage pans it through an offset. `bacon` rebuilds and relaunches on
save. Point `MUI_PREVIEW_FONT` at a variable font and the Glyph scene grows a
slider per axis.

```bash
cargo run -p mui-vello --example headless -- /tmp/pill.png
```

is the stack end to end with no window. With `--features cpu`, `mui-vello`
renders through `vello_cpu` and its snapshot test asserts actual pixels.

## Verify

```bash
./tools/verify.sh
```

Formatting, tests, clippy with warnings denied, and a wasm check of the
library crates. The TypeScript frontend under `packages/mui-ts` is frozen;
see `packages/mui-ts/FROZEN.md`.
