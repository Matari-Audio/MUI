# MUI collections, allocation, and frame-path audit

## Scope and method

This is a static audit of the MUI collection and frame paths, with the graft graph queried before opening source. The main flow is `mui-preview::tick` (`crates/mui-preview/src/main.rs:258-303`) -> `chrome_frame` (`:308-372`) -> `Ui::finish`/hit registration -> `draw` (`:374-425`). A scene rebuild goes through `Baked::build` (`:73-115`) -> `resolve_scene` (`crates/mui-core/src/scene.rs:460-464`) -> layout `measure`/`distribute`/`arrange` (`crates/mui-layout/src/lib.rs:389-684`) and geometry boolean/offset/fillet paths. I also inspected the ignored `frame_cost` and `render_resize` integration tests.

`graft map` reports 37 indexed files, 731 symbols, and 1,174 edges. Exhaustive graph searches found 29 `.clone()` hits in 20 symbols, 40 `collect` hits in 32 symbols, 18 `BTreeMap` hits in 13 symbols, 25 `Vec::new` hits in 20 symbols, no `HashMap` hits, and no `par_iter` hits. No repository files were edited and no heavy benchmark was run. Findings marked **inferred** need a runtime profile; **measured coverage** means the existing benchmark measures the named path, not that I ran it here.

The good pattern throughout the code is to keep geometry immutable while it is being consumed, preserve deterministic ordering with `BTreeMap` where output order matters, and commit new layout/surface state only after the whole calculation succeeds (`mui-layout/src/lib.rs:699-713`, `mui-egui/src/lib.rs:114-128`). The cost is that several hot paths materialize ordered maps, clone full resolved values, and rebuild transient paths on every frame.

## Findings

### Profiling candidate (unmeasured) — the frame path repeatedly rebuilds UI text, paths, and hit geometry

**Evidence:** `tick` invokes `chrome_frame` on each frame (`crates/mui-preview/src/main.rs:258-263`). `chrome_frame` builds the sidebar and then converts every `Ui::finish()` path to Vello paths and collects a new `Vec` (`:361-365`). `Ui::run` makes a text run for each label (`crates/mui-preview/src/ui.rs:254-268`), while `place` transforms and pushes each path (`:271-276`). `register` calls `id.to_string()` and `Hit::push`, which converts the path again (`:292-303`; `crates/mui-input/src/lib.rs:53-61`). Text shaping itself creates a fresh `PathPen` command vector (`crates/mui-text/src/lib.rs:167-213`).

**Trigger and impact candidate:** every redraw with an unchanged sidebar; the static path grows with labels, controls, glyph count, and path segments. This creates short-lived `String`, `Vec`, path, and hit-target allocations and repeats Bezier conversion. The existing `frame_cost` test measures 18 text rows and 40 Bezier conversions separately (`crates/mui-preview/tests/frame_cost.rs:22-104`), so combined frame allocation and interaction cost is not measured. This is a **profiling candidate**, not a demonstrated regression; confidence is **inferred**.

**Minimal next step:** measure an unchanged frame, pointer-move frame, and forced-rebake frame with allocation counts before changing ownership or caching. If profiling demonstrates that this path matters, evaluate reuse or caching with invalidation tests; until then the existing straightforward rebuild is a valid correctness baseline.

### Profiling candidate (unmeasured) — hit testing is ordered but linear, and pointer movement clones IDs

`Hit::at` scans targets in reverse registration order, rejects by bounding box, then performs winding tests (`crates/mui-input/src/lib.rs:44-82`). The reverse scan is the correct topmost-paint behavior, and the nonzero winding rule is covered by `mui-input` tests. `Interaction::update` converts the current target to an owned `String` and clones it for `over`, `pressed`, and `active` transitions (`:149-199`).

For `T` targets and `S` path segments, a pointer move is O(T*S) in the worst case; target registration also converts each path. This could matter with a large generated UI or many geometry-heavy controls, especially when the pointer moves every frame, but no such regression is measured. Confidence is **inferred**. Profile target counts and hit-test time first. Any later broad-phase experiment must retain registration order for ties, and any ID representation change must preserve the current event and topmost-hit semantics.

### Profiling candidate (unmeasured) — `Resolver` clones cached resolved surfaces and merged geometry

`Resolver` stores specifications, state, and output in ordered `BTreeMap`s (`crates/mui-core/src/scene.rs:237-274`). `resolve` first clones every surface ID into a `Vec` (`:269-275`). A cache hit returns `v.clone()` (`:276-280`); new results are inserted with another `resolved.clone()` before returning (`:455-456`). Merged surfaces collect child placed shapes and run a union (`:337-346`). `BTreeMap` is a sound choice when deterministic surface order and stable diagnostics matter, but it is O(log N) per lookup and keeps key/value allocations.

For large scenes or frequent resolution, cloning a resolved surface also clones its path/shape collections. The possible impact is proportional to surface count and vertex count, and merged geometry may dominate the map overhead, but the repository supplies no clone-byte or resolver timing measurement. Confidence is **inferred**. Instrument clone volume and resolution time before selecting an ownership or cache redesign. Preserve deterministic iteration explicitly; replacing `BTreeMap` with a hash map solely for an assumed lookup win would change a deliberate ordering property without evidence.

### Profiling candidate (unmeasured) — flex distribution performs repeated scans and temporary collections

`distribute` collects a base vector and clones it into `allocated`, then on every freeze iteration collects the active indices and sums them (`crates/mui-layout/src/lib.rs:498-538`, especially `:499-525`). With `k` flexible children this is O(k²) in the number of children and allocates at least one temporary vector per iteration. `arrange` then clones each node ID while inserting frames in a `BTreeMap` (`:540-660`).

The trigger would be a wide row/column with many min/max constrained children; a normal small row may not matter. The loop has a possible O(k²) shape and temporary vectors, but no layout profile demonstrates that it is material. Confidence is **inferred**. Benchmark wide rows with alternating constraints before changing the algorithm. If a measured case warrants an experiment, preserve min/max freeze semantics and deterministic frame output, then compare a one-pass calculation against the existing implementation.

### Profiling candidate (unmeasured) — boolean/fillet geometry has superlinear work and repeated point copies

`prepare_all` prepares each shape and incrementally unions it into the accumulated output (`crates/mui-geometry/src/boolean.rs:220-249`). As the accumulated geometry grows, later overlays repeatedly process and allocate the whole result. `Topology::polygons` loops over every component, scans all rings to find its exterior, then scans all rings again for holes and clones their points (`:145-166`), giving O(C*R) lookup work plus output copies. `fillet` checks every vertex against every ring vertex/segment while computing clearance (`crates/mui-geometry/src/fillet.rs:80-173`), which is O(V²) for a complex topology.

The trigger would be many overlapping shapes, large union outputs, or many close contours; the small demo geometry is unlikely to expose it. The loops have potentially superlinear work and repeated point copies, but no geometry profile demonstrates a frame stall. Confidence is **inferred**. Measure rebake/offset time and peak allocations on representative shape counts before choosing a batch overlay, spatial index, or component index. Preserve winding, hole ownership, vertex limits, and stable output order; any optimization would need area/hole/fillet regression tests because shortcuts can alter topology.

### Unconfirmed invariant concern — raw component numbers can contain gaps after a degenerate exterior

`topology` assigns `component` from `raw.into_iter().enumerate()` (`crates/mui-geometry/src/boolean.rs:250-283`). If `clean_ring` rejects the first ring of a raw polygon as `DegenerateRing`, the whole polygon is skipped (`:255-259`), but later raw polygons keep their original component number. `components()` counts only surviving exterior rings (`:127-132`), while `polygons()` assumes component IDs are dense and loops `0..self.components()` (`:145-166`). Thus, if raw component 0 is degenerate and raw component 1 is valid, `components()` is 1 but `polygons()` searches only for component 0 and can return no polygon. This is a static invariant concern; it is **unconfirmed** and has no demonstrated production failure.

I attempted a safe public-API repro in `/tmp/mui-topology-repro`: a degenerate `Polygon` followed by a valid rectangle passed to `union`. The public call returns `Err(DegenerateRing)` during input preparation, so it does not reach `topology` with a skipped raw component. The concern therefore remains an unconfirmed backend/invariant case, not a confirmed public bug. If a private unit fixture or backend-generated result can produce that raw shape, assert components, holes, and reconstructed area before considering a dense-ID change. Do not silently remove the existing degenerate-ring policy: skipping an invalid raw polygon is reasonable; only the mismatch between raw index and dense count is in question.

### Profiling candidate (unmeasured) — the egui cache avoids retessellation but still copies mesh data at paint time

`PathMeshCache` compares the cached path and tolerance and retessellates only on a miss (`crates/mui-egui/src/lib.rs:241-273`); its tests cover unchanged paths and tolerance invalidation (`:280-297`). That is a good correctness and performance pattern. However, `paint` collects translated vertices, clones indices, and collects contour points on every call (`:134-162`, especially `:135-157`). For a large surface or many repaint frames, tessellation is avoided but memory bandwidth and allocations remain.

The possible cost grows with mesh size and repaint frequency, but no paint allocation or bandwidth profile is available. Confidence is **inferred**. Keep the tested cache invalidation rules and measure paint time/allocations before changing buffer ownership. Any future reuse must preserve path, tolerance, scale, and transform invalidation so it cannot render stale geometry.

### Profiling candidate (unmeasured) — flatten and transform paths grow vectors without reuse

`Path::flatten` creates `contours` and a new `active` vector per `MoveTo`, then pushes arc samples without a capacity estimate (`crates/mui-geometry/src/path.rs:96-165`). `rigid_transform` collects transformed commands and validates again (`:168-192`). These checks are valuable for finite coordinates and `max_points`, but repeated flatten/offset operations allocate proportional to path commands and arc samples.

This may matter for high-resolution arcs or repeated offsetting, but no path allocation profile is available. Confidence is **inferred**. Measure flatten/transform time and allocations first; then compare capacity planning or reuse experiments while keeping the existing limit checks and validation. Avoid sharing mutable scratch buffers across concurrent operations without ownership discipline.

## Benchmark and parallelism assessment

`crates/mui-preview/tests/frame_cost.rs:5-20` is an ignored release measurement with 20 timing repeats. `what_a_frame_costs` covers text, 40 Bezier conversions, one large glyph, and a Vello fill/stroke (`:22-104`). `scene_resolution_costs` warms 20 calls and reports median/p95 for 200 `black_box(resolve_scene(...))` samples (`:106-134`); it clones a baseline scene to create an offset variant, so that clone is benchmark setup rather than necessarily frame work. Neither test measures `tick` + hit registration + draw as one unit, allocation counts, layout-only cost, or cache hit/miss ratios. `render_resize.rs` is an ignored GPU correctness/readback test (`:1-151`), not a performance benchmark.

A small end-to-end benchmark should record frame median/p95 for unchanged state, hover movement, and a forced rebake, while separately reporting layout, resolver, geometry, path conversion, and draw submission. Allocation counts or a heap profiler are needed to distinguish CPU from allocator pressure. Existing median/p95 and warmup structure is a good baseline; the benchmark should also use `black_box` around outputs and avoid measuring setup.

There is no `par_iter`/Rayon path in the indexed MUI code. Do not add parallelism as a first response: ordered maps, mutable caches, and incremental union/fillet state make naive parallelization risky, and small UI workloads may lose to scheduling overhead. If profiling proves geometry dominates, parallelize independent shape preparation or text shaping into bounded tasks, combine results in declaration order, and benchmark the crossover on representative scene sizes. Determinism, cache ownership, and error propagation are correctness requirements.

## Coverage and confidence

The audit covered the preview frame loop/UI, input hit testing, core resolver, layout measure/distribute/arrange, geometry topology/boolean/fillet/path, text shaping, egui mesh cache/paint, Vello path conversion, and the two preview integration benchmarks. Static risks are clearly labeled as inferred or conditional; no runtime timings or allocation profile are claimed. The highest-value follow-up is an end-to-end frame benchmark plus a topology regression test for skipped degenerate components.

Graft accounting for this audit: approximately 608,643 source tokens saved across the graph map, targeted asks/skeletons, exhaustive searches, and the public-API repro audit (the count is the sum reported by graft; it is not a runtime performance result).
