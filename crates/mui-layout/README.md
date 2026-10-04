# MUI layout

Renderer-independent intrinsic layout with persistent revision/measurement
caches. Existing equal-column grids and `min_col` grids retain their sizing
and balanced wrapping behavior.

Explicit grids support pixel, percentage, fractional, auto, min-content, max-content,
bounded content (`MinMax`) and explicit-minimum fractional (`MinFr`) tracks
on both axes. For example:

```rust
use mui_layout::{GridTrack as T, Node};
let grid: Node = Node::grid(2, [
    Node::stack([]).id("sidebar").grid_row_span(2),
    Node::stack([]).id("header").grid_at(1, 0),
    Node::stack([]).id("body"),
])
.grid_tracks([T::Px(160.), T::MinFr { min: 0., fr: 1. }])
.grid_rows([T::Px(40.), T::Fr(1.)])
.gap(8.)
.size(640., 400.);
```

`grid_at` uses zero-based coordinates. Explicit placements reserve cells
before automatic placements; automatic items then flow in sparse row-major
order, skipping column and row spans. Explicit items may overlap. Extra rows
use `Auto`; positions outside declared columns return an error. `min_col`
currently cannot be combined with explicit column tracks. Track counts,
positions, row spans and occupied cells are bounded by the layout node budget.

Flex distribution freezes constrained items and redistributes the remaining
space. Shrink weights use unclamped flex bases; explicit basis takes precedence
over a percentage main size. Factors whose sum is below one consume only that
fraction of the free space. Wrapped rows remeasure fluid children at their
line's final shares. Min/max constraints settle before aspect-ratio derivation.

Run the independent expected-rectangle and warm-cache equivalence fixtures:

```sh
cargo run -p mui-layout --example conformance
cargo test -p mui-layout
```

This remains an intrinsic UI solver rather than complete CSS/Taffy semantics.
Grid named lines/areas, dense or column-first auto-flow, implicit column
expansion, auto-repeat, generalized CSS `minmax()` expressions, subgrid and
baseline track alignment are not implemented. Spanning content is distributed
among eligible content tracks; it does not implement every CSS intrinsic-track
phase. Min-content comes from the supplied measurement floor; the callback
has no separate max-content/min-content request mode. Existing grid cells
remain constrained to their tracks, including authored oversized fixed cells.
