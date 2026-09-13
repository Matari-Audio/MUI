# Baselines, viewports and transformed picking

`Align::Baseline` aligns first horizontal text baselines in flex rows (including wrapped
lines), grids and overlays. Containers propagate their descendant baseline through padding. Leaves
without font metrics retain Taffy's bottom-edge fallback. This does not add RTL or vertical
writing modes.

`Ui::resolve_with_baseline` accepts `Measurement { size, baseline }`; the baseline is a
logical-pixel offset from the **content** top edge. Layout adds padding. Use the same font
and line height when painting. Fixed-size leaves are measured for their baseline too.
The size-only measurement APIs remain available, and layouts without baseline alignment
do not perform the extra baseline measurement. `mui-text::TextSystem` supplies Parley's
actual first-line baseline automatically.

```rust
container([item("label").text("Gain"), item("value").text("80%")])
    .align(Align::Baseline)
```

## Overflow and scrolling

`Node::overflow` and `Item::overflow` apply to both axes:

- `Overflow::Fit` is the existing default: overflowing children produce a layout error.
- `Overflow::Clip` allows overflow and clips descendants to the content box.
- `Overflow::Scroll` additionally exposes positive, clamped offsets in `ViewState`.

Nested viewport content does not enlarge its parent's scroll range beyond the viewport's
own layout frame. Scroll alignment falls back to the start when oversized centered or
end-aligned content would otherwise be unreachable. Text defaults to shrinking; use
`.shrink(0.)` when a text item should retain its height inside a scroller. Explicit shrink
settings now survive conversion to measured text.

```rust
let ui = container([
    item("content").text("A long document").width(240.).height(600.).shrink(0.),
])
.id("viewport")
.layout(Flow::Column)
.width(240.).height(180.)
.overflow(Overflow::Scroll)
.build()?;

let mut state = ViewState::default();
state.set_scroll("viewport", Point::new(0., 80.))?;
let view = state.resolve(&ui, &scene)?; // scene resolved with your text measurement
```

## One snapshot for paint and input

`ViewState` keeps scroll offsets and affine transforms outside layout. A transform is
relative to an item's top-left layout origin and propagates to its descendants; scrolling
moves only descendants. Singular/nonfinite transforms and invalid offsets fail explicitly.
State keys are fully scoped item IDs. Remove state for deleted items with `remove(id)`.

Resolve a fresh `View` after changing layout or state. For each surface returned by
`Ui::outlines`, look up `view.item(&surface.id)` and:

1. Apply its `transform` to the original scene-space path and text coordinates.
2. Intersect every ancestor `clips` entry. Each clip has its own frame and transform.
3. Apply the same clips to text, geometry and embedded child content.

A rotated clip is a transformed rectangle, **not** its axis-aligned bounding box. Rectangular
content-box clips are currently supported; arbitrary clip paths are not. A backend must
implement this contract explicitly: the existing GPUI probe still uses GPUI's own scrolling
and is not yet wired to this new core view API.

`view.tap_at(screen_point)` and `view.hover_at(screen_point)` inverse-transform the pointer,
apply all clips and test the nonzero-filled path within the original layout frame. Painted
noninteractive items block older siblings, decorative children bubble to interactive
ancestors, and disabled items block activation. Text/stroke-only items use their surface
region for occlusion, not per-glyph or stroke-only coverage. Extended bridges remain outside
the action region. The old `Ui::tap_at` / `Ui::hover_at` methods retain rectangle-only
compatibility behavior; use the `View` methods for this contract.

`state.scroll_at(&view, screen_point, delta)` routes a wheel delta to the deepest hit item's
scroll ancestors. Unconsumed motion passes to the parent; the result is the remaining
screen-space delta. Positive offsets move content up/left. Re-resolve before the next event.

Transforms do not change layout or enlarge scroll ranges. Boolean-merged members must share
transforms and ancestor clips; divergent member transforms are rejected because the merge
is one painted surface. Path picking currently flattens on demand to a screen-space error
bound; the 65,536-point budget fails explicitly rather than claiming a hit on incomplete
geometry.

## Runnable evidence

```sh
cargo test --offline -p mui-layout -p mui-core -p mui-text
cargo run --offline -p mui-demo --example layout_view > layout-view.svg
```

The [rendered example](layout-view.svg) uses the same view transforms and clip stacks for
paths and text, and asserts the yellow points hit their transformed targets. It shows two
inner scroll offsets. Labels use DejaVu Sans; real baseline equality is also tested against
Parley's glyph metrics with the bundled font. Tests cover wrapped/nested baseline rows,
nested viewport limits, wheel chaining through affine transforms, clipping, disabled and
occluding siblings, invalid transforms and explicit text shrink behavior.
