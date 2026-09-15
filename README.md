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
let root = col![
    row![title("Filter"), spacer(), toggle(&ui, "bypass", &mut bypass)]
        .center()
        .tip("Bypass the filter"),
    slider(&mut ui, "cutoff", "Cutoff", &mut cutoff, 0.0..=1.0),
]
.gap(M)
.pad(L)
.radius(20.0)
.fill(Surface)
.shadow(Shadow::soft(12.0));

let frame = ui
    .frame(root, Some(Size::new(280.0, 120.0)), Input::default(), 1.0 / 60.0)
    .unwrap();
assert!(frame.scene.paint.len() > 5);
// mui::vello::paint(&mut Gpu { scene, resources }, frame.scene, Affine::IDENTITY)?;
```

`Ui::frame` takes an `Input` — pointer, wheel, key presses and typed text —
advances gestures and springs, styles the tree by state (hover and press are
mixed into a named node's fill through a spring), resolves layout and
geometry, and rebuilds the hit regions from the paths it painted, so what
responds and what you see cannot drift apart. A `PointerInput` converts into
an `Input`, so a pointer-only host passes one unchanged. What comes back:
`frame.animating` says whether to schedule another frame, `frame.cursor` is
what the hovered surface asks for, and `frame.tip` is the tooltip that came
due (already floated into the scene, handed back for a host that would
rather open a native window).

## The DSL

```rust
use mui::prelude::*;

let control = |id: &str| leaf(28.0, 28.0).pill().fill(Primary).id(id).cursor(Cursor::Hand);

// A tab whose shell is a parallel inset of its own rounded outline.
let tab = col![control("plus"), control("phase"), control("warp")]
    .gap(10.0)
    .pad(22.0)
    .min_width(92.0)
    .center()
    .id("tab")
    .shell(12.0, Raised);

// The tab and the panel welded into one filleted shape.
let root = row![tab, leaf(520.0, 230.0).id("panel")]
    .start()
    .weld(Surface);

let scene = resolve_scene(&SceneSpec::new(root)).unwrap();
assert_eq!(scene.surface("tab").unwrap().frame.size.width, 92.0);
```

| you write | it means |
|---|---|
| `row![..]`, `col![..]`, `stack![..]`, `grid![3; ..]` | flex axis, stack, cells; each arg goes through `IntoEl`, so `&str` is a text run |
| `row([..])`, `column([..])`, `overlay([..])`, `grid(3, [..])` | the same four, taking an iterator |
| `leaf(w, h)`, `spacer()`, `text("..")` | a sized box, a `grow(1)` gap, a measured text run |
| `title("..")`, `label("..")`, `caption("..")` | text at 18, 13 and 11 px |
| `.gap(M)`, `.pad(S)`, `.pad(12.0)` | spacing tokens `Xs S M L Xl` from the theme, or pixels |
| `.grow(w)`, `.shrink(w)`, `.basis(px)`, `.expand()` | flexbox weights |
| `.width(Len::Pct(50.0))`, `.aspect(16.0 / 9.0)` | percentage and ratio sizes |
| `.w(clamp(64.0, 30.0, 220.0))` | CSS `clamp(min, pct%, max)`: fluid between two pixel stops |
| `SceneSpec::new(root).scale(2.0)`, `Ui::scale` | the host's device pixels per unit: every edge and baseline the walk paints lands on the device grid, so abutting fills have no seam |
| `.w(120)`, `.h(40)`, `.square(28)` | the same sizes taking a bare integer |
| `.align(..)`, `.justify(..)`, `.anchor(x, y)`, `.offset(dx, dy)` | cross axis, main axis, overlay placement, nudge |
| `.center()`, `.start()`, `.end()`, `.between()` | the four alignments worth a word |
| `.justify(Justify::SpaceAround)`, `.justify(Justify::SpaceEvenly)` | the other two CSS distributions |
| `.wrap()` | a row or column that breaks into lines instead of overflowing |
| `.span(2)`, `.order(-1)` | a grid cell two columns wide; placed before its declaration slot |
| `.min_col(120.0)` | `repeat(auto-fit, minmax(120px, 1fr))`: the grid drops columns until each clears 120 px |
| `.push(child)`, `.baseline()`, `.lines(2)` | append to a container, sit text children on one baseline, cap a wrapped label |
| `.fill(Primary)`, `.fill(Color::..)`, `.fill(Gradient::vertical(a, b))` | a palette role, a literal, a gradient |
| `.fill(Fill::Image(img, Fit::Cover))` | an RGBA buffer as a fill: `Cover`, `Contain` or `Fill` (`vello_cpu` paints the pixmap, `vello_hybrid` uploads it once into its atlas) |
| `.stroke(Ink)`, `.radius(8.0)`, `.pill()`, `.shadow(Shadow::soft(12.0))` | outline, corners, shadow |
| `.animate()`, `.transition(Spring::new(0.3, 1.0))` | this node's fill, stroke, radius, text size and shadow spring to their new values |
| `.shell(d, fill)` | a parallel inset of the outline before it, cumulative |
| `.weld(fill)` | paint the union of the children's frames as one filleted shape |
| `.scroll()`, `.clip()`, `.float()` | overflow the wheel slides, overflow cut off, a child painted over everything |
| `canvas(\|size\| vec![Draw::fill(path, Ink)])` | your own paths, in the node's own space |
| `.cursor(Cursor::Hand)`, `.tip("..")`, `.focusable()` | the pointer, a tooltip after half a second, Tab stops here |
| `.id("name")` | a gesture target and a lookup key; unnamed nodes are decoration |
| `Path::from_svg_data("M0 0 h10 a5 5 0 0 1 0 10 z")` | an icon's `d` attribute as a `Path`, arcs and all |
| `ui.tween(id, target)`, `ui.edit(id)` | a spring-smoothed number; `Begin`/`End` of a gesture |
| `frame.clipboard`, `frame.edits` | what a copy wants put on the clipboard, and every gesture edge this frame |

Alignment is inherited: a child without `.anchor` sits where its parent's
`align` and `justify` say, and `Stretch` is the default cross-axis value so
a row of controls fills its column unless told otherwise.

## Kurv on MUI

A plugin editor shell — header, a scrolling parameter list, a response
curve, a status bar — is forty-six lines, twenty-one of them the tree
itself, and not one coordinate:

```rust
use mui::prelude::*;

let mut ui = Ui::new(Theme::DEFAULT);
let (mut bypass, mut preset) = (false, "Init".to_owned());
let mut values = [0.4, 0.5, 0.8, 0.2, 0.6];
const NAMES: [&str; 5] = ["Drive", "Tilt", "Mix", "Air", "Floor"];

let params: Vec<El> = NAMES
    .iter()
    .zip(&mut values)
    .map(|(n, v)| slider(&mut ui, n, n, v, 0.0..=1.0))
    .collect();

let curve = canvas(|size| {
    let pts = (0..=48).map(|i| {
        let t = f64::from(i) / 48.0;
        Point::new(t * size.width, size.height * (1.0 - t * t))
    });
    vec![Draw::stroke(Path::polyline(pts, false), Primary, 2.0)]
});

let root = col![
    row![
        title("Kurv"),
        text_input(&mut ui, "preset", &mut preset).w(140),
        spacer(),
        toggle(&ui, "bypass", &mut bypass).tip("Bypass"),
    ]
    .gap(S)
    .center(),
    row![
        column(params).gap(S).scroll().w(200),
        curve.grow(1.0).fill(Raised).radius(12.0).cursor(Cursor::Crosshair),
    ]
    .gap(M)
    .grow(1.0),
    row![caption("48 kHz"), spacer(), caption("2 voices")].center(),
]
.gap(M)
.pad(L)
.fill(Surface);

let frame = ui
    .frame(root, Some(Size::new(560.0, 340.0)), Input::default(), 1.0 / 60.0)
    .unwrap();
assert!(frame.scene.surface("Drive").is_some());
```

## Motion

Nothing is keyframed. `.animate()` puts a spring on a node's own visual
channels -- fill (in Oklch, hue the short way round), stroke width, radius,
text size, shadow blur, shell depths -- and a new declared value **retargets**
the live spring instead of restarting it, so a colour changed mid-flight
keeps its velocity. `.transition(s)` is the same with your own spring;
`Spring::new(response_s, damping)` is the tuning you want (`1.0` is critical,
below that overshoots), `Spring::instant()` turns one off.

`ui.tween(id, target)` is the escape hatch for a number MUI cannot see --
a canvas sweep, a pan -- and `ui.tween_with` takes a spring. `frame.animating`
is true while any of them still moves, which is the only thing a host needs
to decide whether to schedule another frame.

Gestures have edges: `ui.edit(id)` reports `Edit::Begin` when a press
captures a target and `Edit::End` when it lets go (including a cancelled
one), which is exactly a plugin parameter's begin/end-edit bracket.
`frame.edits` is the whole list.

```rust
use mui::prelude::*;

let mut ui = Ui::new(Theme::DEFAULT);
let mut gain = 0.5;
let sweep = ui.tween("sweep", gain);
let root = col![
    leaf(60.0, 60.0).fill(Primary).animate().id("lamp"),
    knob(&mut ui, "gain", "Gain", &mut gain, 0.0..=1.0, 72.0),
];
let frame = ui.frame(root, Some(Size::new(200.0, 200.0)), Input::default(), 1.0 / 60.0).unwrap();
assert!(sweep <= gain && frame.edits.is_empty());
```

## Images and icons

`Image::rgba(w, h, bytes)` takes straight (non-premultiplied) RGBA8 -- what a
decoder hands back -- and `Fill::Image(img, fit)` paints it into whatever
outline the node already has, cropped (`Cover`), letterboxed (`Contain`) or
stretched (`Fill`). Decoding is the host's job: no library crate takes an
image dependency. `Path::from_svg_data` turns an icon's `d` attribute into a
`Path` (arcs included), so a symbol is geometry like everything else.

`vello_cpu` paints the pixmap itself. `vello_hybrid` wants an atlas id and
panics on a pixmap, so `mui_vello::Gpu` carries an optional `Atlas` -- the
renderer, device, queue and a host-owned `ImageIds` -- and uploads each image
buffer once through `Renderer::upload_image`, then paints by id. A `Gpu` built
without an `Atlas` flattens an image fill to the mid grey `Paint::solid`
already uses for contrast rather than crashing, and so does an image no
atlas tile could hold. Both caches key on the buffer's `Arc` and sweep the
entries the app has dropped on the next upload, so a panel handing over a
fresh frame buffer every frame does not grow either one.

## Accessibility

`mui-access` turns a `ResolvedScene` into an `accesskit::TreeUpdate`:
`tree_update(&scene, &access, focus)`, where `Access` maps an id to a
`Semantics { role, label }`. Roles are not inferred -- a surface nobody
described reports as a labelled group -- so a host registers the widgets it
cares about and hands the update to its platform adapter.

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
assert!(light.is_valid());
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
| `mui-core` | `El` + `Styled` DSL and the `row!`/`col!`/`stack!`/`grid!` sugar, roles and palette, `canvas` draws, clip and float layers, the walk from tree to `ResolvedScene` paint list, the frame-to-frame `TextCache`, `Spring` |
| `mui-input` | `Input` (pointer, wheel, keys, text), hit testing against real paths and their clips, press capture, hover, click, drag and drop |
| `mui-vello` | the `Canvas` trait and its `Gpu` / `Cpu` wrappers over `vello_hybrid` and `vello_cpu`: fills, strokes, image fills (`Cpu` paints the pixmap, `Gpu` uploads once through its `Atlas`, see Images), clip push/pop, and hinted glyph runs (Vello hints and caches the outlines per font blob); `paint(canvas, scene, transform)`, and `paint_cached` with a `PathCache` that keeps a still frame's arc-to-cubic conversions |
| `mui-access` | a `ResolvedScene` plus a `Semantics` map as an `accesskit::TreeUpdate` |
| `mui-truce` | the non-real-time document and parameter contract a Truce plugin shares with its editor |
| `mui` | `Ui` runtime, focus and wheel scrolling, tooltips, transitions, tweens, gesture edits, and widgets (`slider`, `knob`, `toggle`, `button`, `text_input`); the `prelude` |
| `mui-tessellate`, `mui-egui` | triangle meshes and the egui debug adapter |
| `mui-preview` | the winit + wgpu gallery, itself one `mui` tree |

## Preview

```bash
cargo run -p mui-preview
```

The whole window is one tree: the sidebar's scene list, toggles and sliders
are `mui` widgets, the specimen is an anchored child of the stage, and a drag
on the stage pans it through an offset. Text reaches the pixels as a hinted glyph
run, not a filled outline: the host hands `paint` a `mui_vello::Gpu`.
`bacon` rebuilds and relaunches on save. Point `MUI_PREVIEW_FONT` at a
variable font and the Glyph scene grows a slider per axis; point
`MUI_PREVIEW_THEME` at a `key = value` file and the palette reloads while the
window is open. F12 outlines every surface and names the one under the
pointer, and the title bar reads the resolve and paint cost of the last
thirty frames.

winit's wheel, keys and modifiers ride into `Input` with the last pointer
sample of each batch, so `Ui::scroll`, `Ui::focus` and `text_input` work in
the window: the Scroll, Text, Tooltip, Canvas, Drag, Image, Wrap, Motion,
Grid and Select scenes are there to prove it -- one per thing the library
claims to do. `Frame.cursor` is applied with `window.set_cursor`; `Frame.tip` is
not, because the tooltip is already floated into the scene. Printable
characters go to `Input.text` only -- `Key::Char` is emitted just for
ctrl/cmd shortcuts, or `text_input` would insert every character twice.
Copy, cut and paste leave and re-enter through `Frame::clipboard` and
`Input::clipboard`; the preview loops them back to itself, so it is its own
clipboard and never touches the OS one. IME is still missing.

```bash
cargo run -p mui-vello --example headless -- /tmp/pill.png
```

is the stack end to end with no window, and

```bash
cargo run -p mui --features cpu --example snapshot -- /tmp/widgets.png
```

runs two `Ui::frame`s of the widget card and rasterises them on the CPU:
no GPU, no window. With `--features cpu`, `mui-vello` renders through
`vello_cpu` and its snapshot test asserts actual pixels. `--features
cpu-threads` rasterises on a rayon pool instead of one core; it is off by
default because a snapshot-sized pixmap loses more to thread hand-off than
it gains (BENCHMARKS.md measures both).

## Verify

```bash
./tools/verify.sh
```

Formatting, tests, clippy with warnings denied, and a wasm check of the
library crates. `BENCHMARKS.md` is the frame budget of a Kurv-sized scene
across `vello_hybrid`, `vello_cpu` and classic `vello`, reproduced by
`cargo run -p mui-vello --release --features cpu --example bench`. The
TypeScript frontend under `packages/mui-ts` is frozen; see
`packages/mui-ts/FROZEN.md`.
