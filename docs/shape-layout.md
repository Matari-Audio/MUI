# Shape layout

## Crate ownership

- `mui-geometry`: `WidthProfile`, `BorderAlign`, `border_geometry`, `boolean_paths`,
  and `ShapeSplit`. These operate on paths and logical distances, with no scene,
  paint, widget, or layout dependencies. Existing `inset_path` supplies padding.
- `mui-layout`: intrinsic/weighted rectangular frame allocation and validated
  publication of contour-derived frames.
- `mui-scene`: `.inside(...)` / `.bend(...)`, theme/style resolution, geometry
  caches, and publishing matching paint, clip, hit and accessibility surfaces.
- `mui`: spring channels for padding, matching gaps, bend and border widths.

## Compact DSL


`ShapeLayout` is part of the MUI prelude. Containers
partition their **final contour**, including custom paths, vector unions and holes.
No new layout language or renderer is required:

```rust
use mui::prelude::*;

let panel = row![
    stack![].flex(1.).fill(Role::Primary),
    col![
        stack![].flex(1.).fill(Role::Secondary),
        stack![].flex(1.).fill(Role::Dim),
    ].flex(1.).inside(2.),
]
.inside(2.)
.radius(24.)
.w(320.).h(200.);
```

- `.inside(2.)`: inset the available contour by 2px, then fill each child's
  layout share with the inherited shape. Also sets the total sibling gap to 2px.
- `.gap(6.)` after `.inside(...)`: override the gap without changing padding.
- `.flex(1.)` / `.flex(2.)`: existing weighted shares. Fixed lengths, rows,
  columns and stacks retain their normal layout meanings. A zero-size child
  stays empty; an empty flexible container is `stack![].flex(1.)`.
- `.bend(0.2)`: bow a two-child row/column divider; negative bends the other way.
  The fraction is of the partition axis's available extent, limited to ±0.45.
  Gap is measured normal to the curve, not as a horizontal/vertical shift.
- `.outline(...)`, `.union(...)`, `.cut(...)`, and nesting remain the shape
  vocabulary. Unions follow the authored source placement: gaps between
  source shapes remain gaps, rather than being silently bridged.

`BorderAlign::{Inside, Center, Outside}` controls ordinary borders through
`.border_align(...)`, and ramps through `BorderRamp::horizontal(...).align(...)`.
Inside is the existing default. `.inside(...)` reserves only the border's
inward share, **then offsets the resulting inner boundary by padding**. A
20→1px inward border therefore produces a correspondingly sloped content edge.
Named ramp anchors and material tabs/dividers resolve against the authored
layout before shape fitting, avoiding geometry/layout feedback.

Named nodes with `.animate_with(Spring::DEFAULT)` spring pixel padding, its
matching gap, bend and ramp widths together. Independently specified gaps keep
their own declared value. Spacing tokens resolve from the theme; they are not
pixel spring channels. Changed outlines and host sizes are re-evaluated normally.

Children's fill, own canvas/text clip, descendant clips, surface paths and named
layout frames use the derived geometry. Ordinary text and controls retain their
proportions; this is contour fitting, not text flowing around holes or glyph
warping. Existing floating overlays still escape parent clips. Outward child
borders reserve space inside their allocated regions too, preserving sibling gaps.
An outward region ramp must use its own frame as its anchor.
Material/raster welding is rejected for `.inside(...)`; use a vector `.union(...)`.

Offsets may split or erase narrow regions. Empty children are omitted from the
scene and hit testing rather than being stretched across a gap. Geometry uses
the existing bounded offset/boolean engine and caches with input validation;
there is no bitmap mask or new dependency.

Run the checks and generate a visual example from actual resolved contours:

```sh
cargo test -p mui-scene -p mui-layout -p mui
cargo run -p mui-scene --example shape_layout > shape-layout.svg
```

## Geometry-only use

```rust
use mui_geometry::*;
fn demo(outline: &Path) -> Result<(), mui_geometry::Error> {
let offsets = OffsetOptions::default();
let geometry = GeometryOptions::default();
let border = border_geometry(
    outline,
    WidthProfile::horizontal(20., 1., 0., 200.),
    BorderAlign::Inside,
    offsets,
    geometry,
)?;
let content = inset_path(&border.interior, 2., offsets)?.path;
let [left, right] = ShapeSplit::new(SplitAxis::X, 0.5)
    .gap(2.)
    .bend(0.2)
    .regions(&content, offsets, geometry)?;
Ok(())
}
```

Pass either returned path through the same operations to nest. Coordinates and
widths are logical units. Offsets preserve holes and can split or erase narrow
regions. `boolean_paths` handles normalized filled sets; `union_contours` is for
solid overlapping sweep pieces, which must not be interpreted as even-odd holes.
Run the independent core tests with `cargo test -p mui-geometry`.

## Joined border labels and inset materials

Use `.surface_layout(padding)` when intrinsic layout footprints describe several
materials inside one container, rather than partitioning every immediate child
with `.inside(...)`:

```rust
use mui::prelude::*;
let owner = row![
    col![
        stack!["remove"].grow(1.).id("above"),
        stack![body("OSC")].h(60.).fill(Role::Primary).join_border("body"),
        stack!["power"].grow(1.).id("below"),
    ].w(48.),
    stack!["wave"].grow(1.).fill(Role::Field).inset_surface(),
    stack!["unison"].grow(1.).fill(Role::Field).inset_surface(),
].gap(8.).pad(8.).w(400.).h(240.).id("body")
 .radius((20., 14.)).surface_layout(8.)
 .border_ramp(BorderRamp::horizontal((Role::Primary, 4.), (Role::Dim, 1.5)));
resolve(&SceneSpec::new(owner)).unwrap();
```

- The owner supplies convex/concave radii and the actual border contour.
- `.inset_surface()` uses the node's frame. `.inset_surface_of([ids...])`
  unions named footprints, so two header extensions and a neighboring panel can
  be one surface. Put that material on a background sibling if the layout also
  contains controls in its holes; their rectangular hit ancestry stays intact.
- `.join_border(body_id)` joins the node's footprint to the nearest left/right
  edge of the named body. Its two roots and exposed corners follow the owner's
  policy; the owner's **inside** border ramp paints it. The node's fill is used
  only as the contrast background for its contents, not a second rectangle.
- Authored panel footprints receive the owner’s inset rounding policy, then
  intersect the padded interior left by the actual border and joins. Inherited
  boundary curves are clipped directly, without filleting their flattened arcs. Changing only the
  owner updates all marked material outlines. No panel radius arithmetic or
  label overlap offsets are needed. A joined label also identifies its content
  body: panels inside that body reserve the owner's maximum inward border width
  and padding, so external ports and footer tabs cannot pull their edges outward.
- This does not reflow children or infer groups from matching colors. Marked
  surfaces publish their derived paint/clip/hit outline; layout frames remain
  the authored footprints. Floating descendants and nested owners are separate
  scopes. Normal controls are untouched.
- The bounded cache keys geometry, member frames, border widths, padding,
  corners, scale, and geometry options. Paint colors remain live independently.

For a group sharing one outer border, put the owner on the group and leave the
individual bodies as layout containers. Each title still names its own body.
