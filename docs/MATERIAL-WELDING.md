# Material-aware welding: implementation and continuation plan

Base: `Matari-Audio/MUI@a9cc11d232986fce8dfb1a12aa20afd7dcbb664e`.
Status: Rust implementation and integration patches authored; not Rust-compiled or
applied to a full checkout in the authoring environment. The JavaScript reference,
Python tooling and offline browser checks were executed. This document separates
implemented source from proposed follow-on work.

## 1. Authoring contract

A weld owns a shared outline, not the identity or behavior of its children. Each
source keeps its layout slot, ID, text, descendants and semantic role. Only the
participating plate's fill and inside-border paint are consumed by this version.

```rust
use mui::prelude::*;

// Default: weld the body AND its border; no option boilerplate.
let group = weld![shape_a, shape_b];

// Explicit: weld the bodies, keep BOTH original borders (including seams).
let group = weld![Weld::shape(); shape_a, shape_b];

// Explicitly omit borders; this is different from keeping them independent.
let group = weld![Weld::all().border(WeldChannel::Omit); shape_a, shape_b];

// A layout container and a material-weld policy are orthogonal.
let group = row![shape_a, shape_b]
    .gap(10.)
    .weld(Weld::all().reach(24.).blend(72.).morph(progress));

// Same policies, explicit animation input.
let group = weld![Weld::default().morph(progress); shape_a, shape_b];
let group = weld![Weld::shape().morph(progress); shape_a, shape_b];
```

Examples above are alternatives; they do not reuse moved Rust values in one
function. `weld!` uses an ordinary stack. Use `row!`, `col!` or `grid!` with
`.weld(...)` to arrange the source plates instead.

| Policy | Body | Border |
|---|---|---|
| `Weld::all()` / omitted options | Spatially blend | Shared border, blended paint and width |
| `Weld::shape()` | Spatially blend | Keep original source borders |
| `Weld::borders()` | Keep original source fills | Shared blended border |
| `Channel::Keep` | Original paint and coverage | Original paint and coverage |
| `Channel::Omit` | Do not paint this channel | Do not paint this channel |

`Keep` does not mean invisible. `Omit` does not disable interaction. A border-only
weld can intentionally outline a connection whose body remains transparent.

New convenience DSL: `.stroke(paint).stroke_width(w)`, `.no_stroke()`, `.no_fill()`,
`.weld(Weld::shape())`, `.weld(Weld::borders())`, `.weld(w.morph(t))`,
`.weld(Weld::off())`, `.unwelded()`, `.weld(w.quality(q))`, and
`.outline(|size| path)`.

A shared vector outline is a different operation, `.union(fill)`: it unions
the children's outlines and leaves each child's paint alone. `.weld(Weld::off())`
removes the material weld only.
`no_fill()` and `no_stroke()` clear the current style; a later preset can restore
it. They are not hidden inheritance/reset sentinels.

### Three independent controls

- `reach`: geometric soft-union reach in logical units. For two facing sources,
  the full-progress contact threshold corresponds to this gap. It is not a strict
  universal gap cap for an arbitrary multi-source arrangement.
- `blend`: spatial material-ownership bandwidth. It can be wide without making
  the geometric bridge equally wide.
- `morph(progress)`: temporal amount in `0..=1`, eased internally by smoothstep.
  It can be driven by a spring, pointer distance, timeline or another parameter.
  Invalid/non-finite values fail rather than poisoning the retained cache.

At zero, selected channels reproduce the independent source compositing; at one,
blended channels use the shared geometry/material. Intermediate geometry changes
its field, so this is not merely opacity-crossfading two detached images.

## 2. Geometry and material algorithm

For source signed distances `d_i`, solve the compact simplex blend:

```
minimize sum(w_i * d_i) + k/2 * (sum(w_i*w_i) - 1)
subject to w_i >= 0 and sum(w_i) = 1
```

The active-set solution is `w_i = max((lambda - d_i)/k, 0)`. An active-set scan
on distances shifted by their minimum avoids exponentials and overflow-prone
softmax expressions. Two sources recover the usual quadratic smooth minimum.
Unlike a sequential pairwise fold, the n-way result is independent of source order
apart from floating-point roundoff. At `k=0`, use the minimum and equal ownership
for exact ties. With `Keep` or partial morphing, original painter order remains
meaningful for the original component compositing.

Geometry and materials use the same source distances/operator, with separately
controlled bandwidths. Border colour and width share material ownership weights.
`Sample::scalar(values)` exposes these weights to other finite, numeric,
application-owned properties without inventing interpolation for enum values,
fonts, blend modes, or arbitrary shader programs.

Rounded rectangles use their analytic signed distance. Other closed outlines are
flattened to oriented contours and evaluated by segment distance and nonzero
winding. Holes need opposite winding. This is not a general self-intersecting SVG
normalizer. Geometry flattening and contour extraction are approximations.

Fill/border colours blend in premultiplied Oklab; ordinary layer composition and
image filtering use premultiplied linear RGB. Source gradient/image coordinates
stay attached to the source's own bounds. Final storage is straight sRGB RGBA8,
not HDR. The Oklab conversion follows Björn Ottosson's published equations:
https://bottosson.github.io/posts/oklab/ . Premultiplied interpolation background:
https://www.w3.org/TR/css-color-4/#interpolation-alpha .

The CPU reference supports solids, linear/radial/conic gradients, and images with
Fill/Cover/Contain fitting. The accompanying JavaScript demo validates solids and
gradients, not the Rust image path.

### Morph continuity correction

A reference test exposed an alpha pop at the first nonzero morph frame on two
overlapping antialiased edges. Using `1 - old_coverage` to fill a new bridge also
changed ordinary overlapping-edge alpha. The implementation now distinguishes
new geometric coverage from the hard union's existing coverage, and bypasses the
temporal material mix only for genuinely newly covered regions. Both languages
contain the same correction; only the JavaScript regression has run here.

## 3. Runtime integration and costs

The new dependency-free `mui-weld` crate owns geometry/material sampling, bounded
CPU baking, companion contours and a local cache. `mui-scene` converts existing
resolved outlines/paints to it. `mui-vello` needs no new shader or public paint
variant: the baked image uses its existing image path.

A persistent, per-Ui `WeldCache` reuses identical requests. It is not global. Default
retention is 32 MiB and at most 32 entries, with exact request equality and LRU
replacement. Accounting conservatively includes retained source and result data;
it is not a measurement of allocator RSS or GPU memory. Oversize bakes may be used
without being retained. `Ui::weld_cache_stats()` and `clear_weld_cache()` expose
this behavior. A `Resolver` keeps persistent text/weld caches; the one-shot
`resolve(&spec)` uses a temporary welding cache.

Default raster limits are 1,048,576 pixels, 100,000,000 estimated work units,
1–64 sources and no image axis over 4096 pixels. The host's device scale wins over
per-widget resolution and never rescales layout lengths. The local raster origin
is snapped to the device grid to prevent fractional-parent-origin sampling drift.

A stable, padded raster domain avoids texture extent jitter through the morph.
Marching triangles extract directed closed contours from the same sampled
geometric field, including holes. The companion outline controls the group hit
shape and clipping; the painted RGBA asset controls its appearance. They are not
a proof of exact pixel-identical coverage at every point: both sampling and
contour interpolation have finite resolution.

This CPU implementation is a correctness/reference route, not a performance claim
for many animated 4K weld groups. Even though it reuses the Vello image machinery,
large updates still require CPU baking and GPU texture upload; device-specific
atlas limits and image allocation failure need host testing. A smooth union is
not re-distanced, so a varying border near the join is a field-based thickness,
not a guaranteed exact Euclidean parallel offset.

## 4. Unsupported combinations fail explicitly

This version does not silently flatten the following into something unrelated:
shadow/shell/mask stacks on welded plates; nested material-weld participants;
nontrivial member compositing layers; a custom or union outline on the
material-weld container; and carving the material-weld container itself.

A nested group can be excluded from its parent's weld. Place independent effects
on an excluded wrapper/descendant. A custom outline is a `cut`/`keep` base like
any other: it is normalised NonZero first, so a hole wound against its outer
contour stays a hole. Custom-path shadows are likewise rejected until a path
filter renderer can honor them. A vector `.union(fill)` is unaffected by all of this.

Direct text, floats, sticky children, excluded children and zero-area children
are not weld plates. An unpainted spacer is excluded unless the parent explicitly
supplies plate paint. Child text, descendant controls, IDs, semantic roles, clips
and ordinary layout are not welded together. The parent's own fill/border, when
specified, explicitly override the participating plate materials.

Adding fields to the public `Element` struct affects downstream exhaustive struct
literals; builder calls and literals using `..Default::default()` remain the
intended migration route. Full downstream compatibility has not been compiled.

## 5. Broader DSL continuation plan — not claimed implemented

| Priority | Work | Acceptance gate |
|---|---|---|
| 0 | Compile this patch with the prior hardening changes | Core tests, integration tests, docs, Clippy, formatting, locked feature/WASM checks |
| 0 | Parley-based multi-font shaped runs | Shared cluster/caret map through measurement, painting, bidi editing and fallback; Hebrew/Arabic/combining/emoji cases |
| 0 | Actual native plugin host | CLAP/VST3 child window, focus/IME, resize/DPI, two instances, close/reopen and balanced automation on Linux/Windows/macOS |
| 1 | Geometry/backend separation for faster weld output | Adaptive vector contour and GPU field backend matching the CPU reference; no regressions in holes or morph endpoints |
| 1 | Exact variable-width border mode | Re-distance or solve true offset geometry; explicit tolerance and scale tests rather than field-width claims |
| 1 | Typed effect-channel policy | Shadow/inner-shadow/shell correspondence, `Keep`/`Blend`/`Omit` per named slot; reject incompatible categorical operations |
| 1 | Nested and pair-scoped welding | Explicit participant scope, stable model identities, exclusions and deterministic n-way semantics |
| 1 | Tri-state style overrides | `Inherit` / `Set` / `Clear` survive preset merging; do not conflate omitted with explicit none |
| 1 | Role-based typography and control parts | Theme text scale and per-slot control styling without copying widget implementations |
| 1 | Unified interaction authoring | Semantic activation, keyboard parameter edits, focus scopes, modals and disabled gestures tested across input sources |
| 2 | Stroke options and transforms | Dash/cap/join vocabulary, affine local paint/hit transforms, hit slop independent of visible edge width |
| 2 | Host GPU composition slot | Typed capability boundary for meters/refraction/custom passes, with CPU fallback or explicit unsupported response |
| 2 | Repaint deadlines and dirty scopes | Timers and live values without mandatory full-rate frames; measure real KURV before a retained-layout rewrite |

Do not collapse model identities into rack indices or blend semantic meaning with
paint. The prior `Id::entity(u64)` work remains the recommended stable identity
route. Do not add another generic component runtime simply to implement these
capabilities.

## 6. Applying, verifying and publishing

The bundle root's README explains the exact-base applicator and optional personal
fork helper. Organization writes returned HTTP 403; `DerpcatMusic/MUI` was not
found, and the active connector has no fork/create-repository action. No remote
fork, branch, commit or PR was created by this work.

The helper defaults to a dry run. With explicit `--execute`, it verifies the
account/fork, creates a NEW checkout and applies both patch generations. With
`--publish`, it refuses an existing remote branch and pushes only after successful
local validation. `--pr` opens a draft in the personal fork, not in the organization.
It does not force-push, reset local work or merge anything. The live fork/build
path remains untested; mocked orchestration tests are not a substitute.
