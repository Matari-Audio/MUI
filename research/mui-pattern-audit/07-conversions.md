# MUI numeric conversion and range audit

Audit date: 2026-09-13. Scope was the Rust geometry, tessellation, layout,
text, renderer and preview crates plus `packages/mui-ts`. I followed the graft
map first (37 files, 731 symbols, 1,174 edges), then searched every indexed
`as` cast and the finite, clamp, floor, ceil, round, `u16`, `usize`, and
`Number.isFinite` boundaries. Existing focused tests pass: 24 `mui-core`, 44
`mui-geometry`, 12 `mui-layout`, 2 `mui-tessellate`, 13 `mui-text`, and 4
`mui-egui` tests.

The four confirmed findings are below. Reproducers are in
`/tmp/mui-numeric-repro`; they do not modify the repository.

## Confirmed findings

### P2 (extreme input): rectangle accepts an overflowing endpoint

`crates/mui-geometry/src/boolean.rs:24-37` checks that `x`, `y`, `w`, and `h`
are finite and positive, then constructs `x + w` and `y + h` without checking
the derived coordinates. `Polygon::rectangle(1e308, 0., 1e308, 1.)` therefore
returns `Ok` with exterior points `[1e308, 0]`, `[inf, 0]`, `[inf, 1]`, and
`[1e308, 1]`. This is an actual malformed geometry value, not merely lost
precision. The downstream boolean boundary does catch it in
`crates/mui-geometry/src/boolean.rs:188-208` (`prepared_ring` rejects
non-finite transformed points), so the normal scene path fails with an error
rather than reaching the renderer. Direct users of the public `Polygon` fields
or `bounds` can still observe the invalid value, and `sharp_rect` at
`crates/mui-core/src/scene.rs:217-221` relies on this constructor.

Minimum fix: compute the four endpoints, require all of them to be finite,
and return `Error::CoordinateLimit` (or `NonFinite`) before constructing the
polygon. Add the overflowing finite-input case beside the existing non-finite
geometry test. Confidence: high. The offset path already follows this style,
including a coordinate limit and bounded fixed-scale conversion, at
`crates/mui-geometry/src/offset.rs:82-131`.

### P2 (extreme input): text sizes are checked as f64 but consumed as f32

`crates/mui-text/src/lib.rs:167-213` rejects only non-finite or non-positive
`size_px`, then casts it to f32 at line 181. `draw_glyph` repeats the cast at
`crates/mui-text/src/lib.rs:215-232` (cast at line 227). A finite `f64::MAX` is thus accepted;
with a local Adwaita font, `/tmp/mui-numeric-repro/text_extreme` reports
`text_run` as `Ok` with `ascent=inf`, `descent=inf`, and `line_height=NaN`.
`glyph_path` also returns `Ok`, but flattening its path returns
`Err(InvalidPath)`. At `f32::MAX`, the metrics are finite but the line height
is already `inf` and flattening still fails. This leaks invalid metrics through
an API that promised a successful `TextRun`, and can silently drop or poison
rendered labels when a caller uses those values for placement.

The repro uses `/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf`; on systems
without that path, substitute any parseable TTF to exercise the same boundary.

Minimum fix: use one checked conversion helper for both entry points and
reject sizes outside the supported text domain before constructing
`skrifa::Size`; also require returned metrics and the finished path to be
finite. Merely checking `f32::MAX` is insufficient because font metric
scaling can overflow below that bound. Confidence: high. Normal finite checks
and path validation are good patterns, but this boundary needs a range check
after the representation change.

### P3: f32 tessellation precision is undocumented at large coordinates

`crates/mui-tessellate/src/lib.rs:48-94` (cast at line 58) converts every flattened point to
`[f32; 2]`, and only then checks `is_finite()`. Finite values that are still
representable as f32 can lose many units. The safe reproducer builds a two-unit
rectangle at x coordinates 16,777,217 and 16,777,219. The returned positions
are `[16,777,216]` and `[16,777,220]`, changing the width from 2 to 4 while
returning `Ok`. `CoordinateOverflow` therefore detects infinity but not
precision loss. This rounding is inherent in the declared f32 output, so it
is a demonstrated precision/contract gap rather than automatically a defect.
It becomes a correctness bug if callers assume the input f64 geometry or
tessellation tolerance is a total world-space error bound. It is less exposed
through `mui-egui::prepare`, because the boolean path normally enforces the
default coordinate limit before tessellation, but the public
`Tessellator::tessellate` and direct text paths bypass that boundary.

Minimum fix: enforce an explicit renderer coordinate/precision limit before
the cast (the existing boolean and offset limits are possible policies), or
rebase coordinates around a local origin before converting. If world-space
f64 precision is required, the tessellation/output representation must remain
f64. Confidence: high for observed rounding, medium for treating it as a bug
without an explicit precision contract. `prepared_ring` at
`boolean.rs:188-208` is the good bounded-transform pattern this API currently
lacks.

### M: TypeScript code generation emits invalid Rust for large integer exponents

`packages/mui-ts/src/compiler.ts:11-14` formats every finite integer as
`${v}.0`. JavaScript uses exponent notation for integer numbers at and above
`1e21`, so `n(1e21)` produces `1e+21.0`; Rust reports type error `E0610:
{float} is a primitive type and therefore doesn't have fields` while compiling
the generated literal.
`n(1e308)` has the same failure. This is a valid finite TypeScript `number`
that fails before MUI's runtime validation. The small-value boundary is fine:
`n(1e-7)` emits `1e-7`, `n(1e-6)` emits `0.000001`, and both compile as Rust
f64 literals. A large ordinary decimal such as `1e10` emits
`10000000000.0` and compiles; with the default layout extent it is then
correctly rejected by `resolve` (`Limits::default` is 1e6 at
`crates/mui-layout/src/lib.rs:325-337`, and `Size::valid` is checked at
`664-684`). Thus huge dimensions below the exponent-format threshold are a
normal resolver rejection, while `1e21` is a code-generation type error.

Minimum fix: when an integer string contains an exponent, insert `.0` into
the mantissa before `e` (or use a Rust-aware number formatter), and add tests
for `1e-7`, `1e-6`, `1e20`, `1e21`, and `1e308`. Keep the existing
`Number.isFinite` rejection. Confidence: high. The exact output and compiler
error are preserved in `/tmp/mui-numeric-repro/n_case.mjs`, `n_case.out`, and
`n_case.rs`.

## Secondary workload observation

`crates/mui-vello/src/lib.rs:27-57` validates only source path command count and
then sends finite arcs directly to `kurbo::Arc::append_iter`. A three-command
path with a finite 1e30 radius and 0.1 tolerance passes validation and expands
to 37,378 Bezier elements (`cargo run --bin vello_arc`). This is workload
amplification from a single public path, rather than an unsafe cast, and is
within the existing 250,000 source-command/point baseline; the repro does not
prove unacceptable performance. The normal boolean coordinate limit keeps
scene geometry much smaller. A renderer coordinate/radius bound or
generated-element budget would make the policy explicit. Confidence: medium
for the observation, low for calling it a defect without a workload contract.

With custom `Limits { extent: f64::MAX }`, two `leaf(f64::MAX, 1.)` values
overflow their measured sum and `resolve` returns `Err(BudgetExceeded)` from
`measure` (`crates/mui-layout/src/lib.rs:389-489`) rather than a clearly named
numeric error. The default extent correctly returns `Err(InvalidValue)`, so
this is a low-severity diagnostic/API issue, not an exploitable allocation.
Checked sums or a hard extent cap would make the custom-limit behavior clear.

## Safe bounded patterns and coverage notes

The preview host clamps window dimensions to `u16::MAX` before both Vello
casts (`crates/mui-preview/src/host.rs:23-25,46-108,126-137`), with a test
covering 70,000 pixels. Layout child counts use `saturating_sub(1) as f64`
and guard the `SpaceBetween` division for one child (`lib.rs:540-660`). The
text subdivision cast is bounded by a final 64-step cap (`lib.rs:263-268`),
and SVG arc subdivision is bounded by `Arc::validate`'s sweep limit before
the cast (`crates/mui-geometry/src/path.rs:20-37,229-265`). Mesh indices are
u32 by construction and consumed as u32 by egui (`crates/mui-egui/src/lib.rs:134-162`);
the `u32 as usize` uses are test area calculations, with tessellation limited
to 250,000 points.

Rigid path transforms validate input and transformed output at
`crates/mui-geometry/src/path.rs:168-192`. Theme, palette, spacing, and layout
validation reject non-finite values at `crates/mui-core/src/theme.rs:152-158`,
`crates/mui-core/src/scene.rs:245-268`, and
`crates/mui-layout/src/lib.rs:359-379`. Offset conversion bounds distance,
point budget, coordinate magnitude, and fixed scale. These checks explain why
the P2 cases are extreme-input robustness gaps and the f32 case is a
documented-precision/contract gap, not memory-unsafe behavior in the covered
normal pipeline.

Graft tally: `graft map` reported 37 files, 731 symbols, and 1,174 edges;
all relevant source spans above came from graft asks/greps before opening.
Across this audit turn, graft reported approximately 1,601,352 saved tokens.

## Persistent reproduction entry points

The original investigation used `/tmp/mui-numeric-repro`. The saved audit includes equivalent checks under [repros](repros/):

- Default executable: rectangle overflow and the consolidated interaction/configuration checks.
- `--bin text_extreme -- /path/to/font.ttf`: font-size overflow; tested with local Adwaita Sans, not a bundled font.
- `--bin tessellation_precision`: the f32 coordinate limitation.
- [check_codegen.py](repros/check_codegen.py): fresh generation and compilation checks, including the exponent type error.

See the [consolidated report](README.md) for complete commands and validation limits.
