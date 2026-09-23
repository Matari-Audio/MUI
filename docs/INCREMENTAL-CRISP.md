# Incremental work and metric-correct crisp welding

Status: native source authored; numerical JavaScript and offscreen GLSL reference
executed. Rust/WGSL/Vello implementation not compiled or executed here.

## The two separate problems

The former shader combines signed-distance-like fields and interprets the result
as a distance. This is not Gaussian splatting. Smooth minimum changes both the
contour and the field's metric. For a local implicit field F, a field interval w
corresponds to approximately w / |gradient F| physical distance. At a join whose
combined normal magnitude is 0.5, a nominal 3-unit band can locally span about 6.
Changing only antialiasing does not fix this. Dividing by the gradient is a local
approximation and becomes unstable when source normals cancel.

The new crisp route instead computes the exposed boundary, then the distance to
that boundary. Geometry merging and material blending are independent operations.

## Crisp contour algorithm

Each rounded rectangle yields line and circle-arc primitives transformed by a
rigid rotation/translation. Pairwise line/line, line/circle and circle/circle
intersections split those primitives. Midpoint coverage classification retains
exposed pieces; an outward probe handles coincident edges and exact contact.
The output is capped at 128 pieces. Budget/invalid geometry errors are explicit.

A nearest-point search projects onto each surviving line/arc. The minimum
Euclidean distance uses the sign of the source union. It therefore measures the
exterior union boundary, not an internal source edge or an approximate soft field.

At the nearest boundary point q, the material weights a_i are nonnegative and
normalize to 1. Border width is sum(a_i * width_i), so widths 2 and 4 produce a
requested width in [2,4]. Colour is blended independently with the existing
premultiplied material policy. Coverage uses the real boundary distance and a
one-device-pixel antialiasing transition. In the deep interior, a conservative
hard-min bound skips the exact nearest-boundary search; coverage there is saturated.

The 6144-byte boundary block is 128 records of three vec4 values. It changes only
when geometry changes. The 352-byte material/transform parameter block remains;
its policy.z stores the piece count. Native pools update only changed 16-byte
parameter lanes and retain boundary contents across morph/material updates.
Buffer submission/abort semantics remain transactional: encoding is not completion.

### What the width promise does and does not mean

Widths are logical UI units; the device-scale transform maps them to physical
pixels. The guarantee is the blended target width and the normal-distance band
where a boundary footpoint is well-defined. It is not a claim that every diagonal
cross-section, acute corner bounding box or medial-axis region is 2–4 pixels wide.
An acute corner may have a long miter-like footprint despite a correct normal
width. Very narrow bodies saturate before a full-width interior band can fit.

At intermediate morph values independent borders still contribute while their
seams fade. Their overlap is not the final fused border's width guarantee.
The fully fused state has one union border, not two alpha-stacked outlines.

Crisp mode does not bridge a gap before contact. Its geometric reach is zero and
its contour is the Boolean union. Materials can still blend smoothly over it.
Organic mode remains for proximity bridges and blob-like morphing. A future
sharp precontact connector needs explicitly constructed bridge boundaries and
join-radius/miter-limit rules; this bundle does not pretend to implement that.

Arbitrary path redistancing, arbitrary affine scaling, true Bezier intersection,
nested material welds and general variable-width stroking are outside this exact
analytic path. Some earlier arbitrary-contour CPU APIs remain approximate.

## Incremental layout without a new DSL

`LayoutCache` retains measured subtrees and arrangement outputs. Cache misses
execute the original `measure`/`arrange` code. The public uncached API remains
available as the comparison oracle and compatibility path.

A prepass validates every node and ID and creates a complete shallow layout
projection. Destructuring Node without `..` makes future Node fields require a
projection update at compile time. Child revision sequences carry dependencies.
Named nodes use stable IDs; unnamed nodes use structural slots. Transient current
node addresses only connect the current solve to its revisions and are not
retained as pointers into old trees.

Measurement cache keys include the exact payload fields consumed by measurement,
node revision, both offered extents, wrapping room and nearest container sizes.
For text: text, text size, weight, line cap and reserve string. Font and
spacing/limits changes invalidate context. Generic callers must include every
external measurement dependency in their key or explicitly clear the cache.

Arrangement cache keys include the measured ticket, origin, offered size and
scroll viewport. An intrinsic-width change can invalidate its parent and move
siblings: this is required behavior, not an optimization failure. Different flex
remeasurement constraints receive different cache keys. Pins use a conservative
full arrangement pass because their anchor dependencies are external to the
local subtree key. The existing pinned-anchor ordering limitation is not fixed.

Both caches bound entry count and stored snapshot records. This does not mean
zero allocation or byte-perfect memory accounting. Current full-tree declaration
validation, payload key construction, snapshot rebinding and output copying are
still O(N). The scene paint walk also remains. This is not a fully tracked signal
runtime and it has not been benchmarked against the uncached solver.

`Ui::layout_stats()` reports validated/measured/arranged/rebound/copied nodes and
cache hits, plus the pin fallback. Native tests compare warm, changed, resized,
reordered, removed and duplicate-ID cases with the original solver. These tests
are authored, not executed in this environment.

## Damage and persistent tiles

Damage detection compares the old/new paint records plus live external materials.
A change marks the union of old and new bounds. Removed effects invalidate their
old locations. State-stack, text, shadow or otherwise unknown extents conservatively
invalidate the full viewport. Cached state commits only after submissions succeed.

The optional `TiledEffects` backend owns persistent 256px GPU tiles with a 2px
sample guard. It rerasterizes dirty tiles only, replaying all intersecting paint
in original order with relevant clip/blend commands. It does not draw only the
changed shape over stale translucent pixels. External effects are prepared once,
not once per tile. Each tile is submitted before Vello's internal buffers are
reused, without a blocking completion wait. A separate lightweight tile scene
covers the acquired presentation target; swapchain persistence is never assumed.

Limits: default host tile budget 64MiB and at most 1024 tiles. Unknown bounds
retain commands during replay. Text/shadow changes currently invalidate globally.
Whole-scene rendering remains the default. Tiling is opt-in with MUI_TILED=1 and
may lose under full damage, extra tile submissions, or broad animation. No speed
claim is made until native parity and end-to-end measurements pass. Inspector
imperative overlays are intentionally unsupported on this optional path.

The native conformance source compares whole-scene vs tiled pixels at 1x, 1.5x
and 2x; checks identical-frame zero dirty tiles and bounded local invalidation.
That gate has not run, so it does not establish tile/alpha/guard correctness yet.

## Connection to Svelte and Visage

Svelte's push-pull reactivity marks derived values dirty on dependency changes and
re-evaluates on read; unchanged derived outputs can skip downstream work. Visage
advertises partial rendering of dirty regions and automatic shape batching.
These address different levels: avoiding computation versus avoiding rasterization.
MUI needs both, and this implementation keeps those mechanisms separate.

This bundle does not import either framework or copy their implementation.
Sources consulted (primary documentation, accessed 2026-09-19):

- Svelte `$derived`: https://svelte.dev/docs/svelte/$derived
- Visage README: https://github.com/VitalAudio/visage
- hg_sdf distance-bound/gradient guidance: https://mercury.sexy/hg_sdf/

## Acceptance gates still required

Run rustfmt, workspace tests, Clippy, WASM, Naga ABI validation, native whole-vs-tile
pixel contracts, then real KURV scenes with CPU/GPU p95/p99, memory/upload/submission
counters and audio load. Include integrated GPUs, fractional scaling and mobile
where applicable. No reciprocal-CPU-time FPS, no software-GPU performance ranking,
and no publication until native conformance passes.
