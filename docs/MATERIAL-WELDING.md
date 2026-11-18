# Material welding

A weld makes several plates read as one object: their outlines join with a
soft fillet and their fills and borders blend across the seam. Each child
keeps its layout slot, id, text, descendants and accessible role. Only the
plates' own fill and border paint are replaced. Every `rust` block here is
compiled and run by `cargo test -p mui --doc`.

## Weld, union or segmented

| you want | write |
|---|---|
| one outline, each child keeps its own paint | `.union(fill)` |
| controls butted into a strip with square inner seams | `.segmented()` |
| one outline and one blended material | `weld![a, b]` or `.weld(Weld::all())` |

`weld![..]` puts its children in a `stack`. To arrange the plates, put
`.weld(..)` on a `row!`, `col!` or `grid!` instead.

```rust
use mui::prelude::*;

let plate = |id: &str, role: Role| block(80.0, 56.0).radius(16).fill(role).stroke(Role::Ink).id(id);

// Default: blend fills and borders.
let a = weld![plate("a", Role::Primary), plate("b", Role::Secondary)];
// Blend fills, keep each plate's own border.
let b = weld![Weld::shape(); plate("c", Role::Primary), plate("d", Role::Secondary)];
// Laid out by a row, with the join tuned and half formed.
let c = row![plate("e", Role::Primary), plate("f", Role::Secondary)]
    .gap(20)
    .weld(Weld::all().reach(24.0).blend(72.0).morph(0.5))
    .id("join");

let scene = resolve(&SceneSpec::new(col![a, b, c].gap(M))).unwrap();
assert!(scene.surface("e").is_some(), "plates keep their ids");
```

## Policy

`Weld` has two channels, each `WeldChannel::Blend`, `Keep` or `Omit`.

| constructor | fill | border |
|---|---|---|
| `Weld::all()`, `Weld::default()` | blend | blend |
| `Weld::shape()` | blend | keep each plate's |
| `Weld::borders()` | keep each plate's | blend |
| `Weld::off()` | keep | keep (no weld; removes one set earlier) |

`Keep` paints the original. `Omit` paints nothing for that channel; the
plates still take input. Set channels directly with `.fill(..)` and
`.border(..)`.

Three numbers, all in logical units except `morph`:

- `reach(units)`: the widest gap two facing plates bridge at full progress.
  Default 16. `Weld::crisp()` or `.sharp()` sets it to 0: plates join only
  where they touch, with no blob before contact.
- `blend(units)`: how wide the material transition is. Default 32. It can
  be wider than `reach`.
- `morph(t)`: progress from 0 (separate plates) to 1 (fully welded), eased
  with smoothstep. Drive it from a spring, a parameter or pointer distance.
  Out-of-range or non-finite values are an error.

## What a weld rejects

A weld fails with `SceneError::UnsupportedWeld` instead of drawing something
else. These cases are rejected:

<!-- coderabbit-ai-generated:v1 -->

<!-- coderabbit-ai-generated-section:v1:9707a2f75d73ec3e1ed7217661ea1dea7201ade1055e1e37f0cd5ac368c1a9ca -->

- a shell, shadow or mask on a welded plate; put the effect on a wrapper
  marked `.unwelded()`, or on the group. Keep plates that should join the
  weld outside that wrapper: plates inside it are excluded from the weld;

<!-- /coderabbit-ai-generated:v1 -->
- a nested weld among the members; mark the inner group `.unwelded()`;
- a rotation or opacity layer on one member; apply it to the whole group;
- a `.union(..)`, custom `.outline(..)` or carve on the weld container;
  carve a plate instead.

Text, floats, sticky children, zero-area children and children marked
`.unwelded()` are not plates. An unpainted spacer is not a plate unless the
group supplies a fill.

## Backends

`SceneSpec::weld_backend` picks how a weld is drawn.

- `WeldBackend::Reference` (default): a bounded CPU bake into an image, with
  a contour traced from the same field for hit testing and clipping. It
  supports solid, gradient and image paints, any closed outline and up to
  64 plates. Resolves through a `Ui` or `Resolver` reuse bakes from a
  per-`Ui` `WeldCache` (`Ui::weld_cache_stats`, `Ui::clear_weld_cache`).
- `WeldBackend::AnalyticGpu`: an analytic shader in `GpuRenderer`, with no
  bake. It takes one to three plates that are plain rounded rectangles.
  The group itself must not `.clip()`; put a clipping container above it.

A `Ui` opts in with `Ui::default().gpu_welding()`. The native hosts
(`mui-baseview`, `mui-winit`) switch a `Ui` back to the reference backend
while they present on the CPU, so the same tree works on both. A tree the
GPU backend rejects makes `Ui::frame` return the error, so keep GPU welds
within its limits.

On the GPU backend, a resolved weld can be changed without a relayout:

```rust
use mui::prelude::*;

let mut ui = Ui::default().gpu_welding();
let tree = || {
    row![
        block(80.0, 56.0).radius(16).fill(Role::Primary).id("a"),
        block(80.0, 56.0).radius(16).fill(Role::Secondary).id("b"),
    ]
    .gap(20)
    .weld(Weld::all().reach(30.0))
    .id("join")
};
ui.frame(tree(), None, Input::default(), 1.0 / 60.0).unwrap();
ui.set_weld_morph("join", 0.25).unwrap();
ui.set_weld_material_blend("join", 48.0).unwrap();
ui.set_weld_solid_material("join", 0, Some(Color::oklch(0.7, 0.2, 30.0)), None, 2.0).unwrap();
```

These calls change only the live scene. The next `Ui::frame` resolves the
declared tree again, so keep the same values in your model and in the tree.
`cargo run -p mui-preview --example gpu_welding` is an interactive lab.

## Algorithm notes

Plates are signed-distance fields. The union and the material ownership use
one n-way compact smooth minimum, so the result does not depend on child
order (except for `Keep` channels and partial morphs, which composite in
painter order). Colours blend in premultiplied Oklab. Rounded rectangles use
their exact distance; other outlines are flattened. A blended border is a
field-based width near the join, not an exact parallel offset.
`mui_weld::Sample::scalar` exposes the same ownership weights for your own
numeric properties.
