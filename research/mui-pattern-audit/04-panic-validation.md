# Panic, unwrap, indexing, and validation audit

## Scope and conclusion

This is a read-only audit of the Rust crates and the `packages/mui-ts` trust
boundary. I searched the indexed repository for `unwrap`, `expect`,
`panic!`, `assert!`, representative indexing forms, and integer-cast
indexing, then inspected the production spans and their callers. The key
distinction is between a panic that encodes a locally proved invariant and one
that accepts a failure from a window/GPU, user, or generated-input boundary.

No confirmed runtime panic defect was found. There is one defensive observation
in the preview binary; the other production-looking indexes are backed by
fixed-size arrays, an immediately preceding length check, synchronized vectors,
or a fixed scene list. Panics and unwraps in tests are fixture/setup assertions;
they do not create a library failure mode. The preview host intentionally uses
process-level `expect` calls for fatal startup conditions, while the reusable
geometry, text, layout, input, and rendering APIs return `Result` and validate
at their boundaries.

## Findings

### Defensive observation: capability-list indexing in preview initialization

* **Location:** `crates/mui-preview/src/host.rs:77`
* **Trigger:** `let preferred = caps.formats[0];` after
  `surface.get_capabilities(&adapter)`.
* **Impact:** A malformed or non-conforming capability response would panic
  during preview initialization. Under the wgpu 29 contract,
  [an empty format list means the surface is incompatible](https://docs.rs/wgpu/29.0.4/wgpu/struct.SurfaceCapabilities.html),
  and this code requests an adapter with `compatible_surface: Some(surface)`;
  after successful selection, a conforming backend therefore supplies a
  presentable format. This is an upstream-guaranteed invariant, not a
  reachable defect in the current flow.
* **Minimal fix:** Optional defensive hardening is
  `caps.formats.first().copied().ok_or(GpuInitError::NoSurfaceFormat)?`, with
  the same error path used for `get_default_config(...).expect("surface config")`
  at `host.rs:84-86`. No fix is required for correctness under the documented
  wgpu contract.
* **Confidence:** High that the local index relies on the documented upstream
  invariant; high that this is hardening only, with no confirmed trigger after
  successful compatible adapter selection.

### Informational: expected startup failures abort the preview process

* **Locations:** `crates/mui-preview/src/host.rs:53-68,84-86` and
  `crates/mui-preview/src/main.rs:436,508-510`.
* **Trigger:** `create_surface(...).expect("surface")`, adapter/device
  requests with `expect`, surface configuration with `expect`, window creation
  with `expect`, and top-level event-loop `expect` calls.
* **Impact:** A missing GPU, unsupported backend, invalid surface handle, or
  event-loop startup problem produces a panic and process exit. This is a
  preview executable, not a public library API, and startup failure is an
  intentional fatal host policy. It is not a core correctness defect.
* **Minimal fix:** None required for the current dev-host policy. A future
  user-facing host could return a typed initialization error and display it,
  but that would be a UX choice rather than a panic-safety repair.
* **Confidence:** High that the calls can panic on ordinary environmental
  failures; high that this is intentional application policy in this binary.

No other confirmed runtime panic defect was found.

## Correct patterns found

Geometry validates untrusted shape data before indexing. `Path::validate` and
`Path::flatten` reject non-finite coordinates, invalid command state, invalid
arc data, tolerance/budget violations, and excessive output. Polygon/boolean
operations normalize rings with `clean_ring` and reject non-simple or too-small
rings before backend conversion. `math.rs:218` uses `p.last().unwrap()` only
inside `if p.len() > 1`; the preceding condition proves both `p[0]` and the
last element exist. The modulo indexes in `clean_ring` and `fillet` follow
minimum-length checks, and fixed four-element arrays in `RoundedRect::path`
are indexed by a `0..4` loop.

The rendering and input seams preserve this design. `mui_vello::bez_path`
validates before converting to renderer commands and returns a `Result`; the
`mui_input::Hit::push` registration path propagates that error, so malformed
geometry fails at registration rather than during pointer dispatch. The
tessellator checks finite points and uses `[1..]` only after obtaining a first
point. `mui_egui`'s `mesh.as_ref().expect("cache populated")` is a local
invariant: the branch either installs `Some(next)` immediately or takes the
same-revision path only when the cache is already present.

Scene and layout validation also happens at the public boundary. Layout checks
IDs, finite/nonnegative spacing and grow/shrink values, min/max consistency,
and padding before resolution. `Resolver::new` and `Resolver::one` validate
surface IDs, duplicates, cycles, missing frames, depth, bounds, and radii;
`SceneState::commit` keeps validation transactional. The direct indexes in
`mui-layout/src/lib.rs:505-535,652-655` walk vectors created with matching
lengths in the same loop. The preview indexes at `main.rs:199,223,352,358-359`
use the fixed list from `scenes::all()` or the selected index guarded by
`select`; they are internal invariants rather than user-controlled indices.

The TypeScript compiler is a separate trust boundary.
`packages/mui-ts/src/compiler.ts:12` rejects non-finite numbers before
emitting Rust, while Rust still performs the semantic checks at
scene/layout/geometry resolution. This is a good layered pattern: build-time
code catches representational failures, and the Rust API does not trust
generated input merely because it came from a typed tuple.

## Bad patterns and when to change them

When an external boundary does not already prove a non-empty result, the
pattern to avoid is:

```rust
let caps = surface.get_capabilities(&adapter);
let format = caps.formats[0]; // conditional foreign invariant, unchecked
```

Prefer:

```rust
let format = caps
    .formats
    .first()
    .copied()
    .ok_or(GpuInitError::NoSurfaceFormat)?;
```

Similarly, use `?` or an explicit error conversion for GPU/device/surface
creation when the caller can report failure. Use `expect` only when the
condition is truly impossible after code-local validation, or when a deliberate
top-level application policy treats startup failure as fatal, and make its
message state the invariant. The existing `"checked just above"` cache expect
is an example of the first local use; the preview startup expects are an
example of the second application-policy use.

An `unwrap` in a test fixture is appropriate when fixture construction failing
means the test cannot be meaningfully executed. It is inappropriate in a
library path that receives a font, path, scene, surface, or generated numeric
value. `unwrap_or`/`unwrap_or_default` in preview overlays and missing text
metrics are deliberate UX fallbacks, but should remain limited to optional
preview behavior; core operations correctly return errors instead.

## Coverage and verification

The indexed repository contained 37 files. Exhaustive graft searches reported:

* `unwrap`: 214 hits in 97 symbols across 15 files.
* `expect`: 53 hits in 25 symbols across 14 files.
* `panic!`: 5 hits in 5 symbols across 5 files.
* `assert!`: 175 hits in 102 symbols across 15 files; the inspected asserts
  were in test modules.
* A broad `[0]` probe reported 300 hits in 126 symbols across 14 files (the
  displayed result was truncated), and an `as usize]` probe found 9 hits in 3
  symbols, all in tests. A supplementary raw regex scan covered direct runtime
  indexing and the TypeScript package.

The `panic!` production-looking exception is
`crates/mui-vello/examples/headless.rs:62-102`, where a per-surface rendering
error is labeled and escalated with `panic!`; it is an example executable, not
library code. Test-only `unwrap`/`expect` calls in geometry, text, input,
layout, tessellation, preview, and vello are setup or assertion code.

`cargo test --workspace` passed. Clippy with
`-W clippy::indexing_slicing -W clippy::unwrap_used -W clippy::expect_used`
also exited successfully with warnings only. The warnings correspond to the
test fixtures, the preview startup policy, and the checked/fixed-shape indexing
described above; no warning exposed a second confirmed defect. GPU/performance
tests remain ignored because they require a GPU or measurement environment.

Graft was used before source inspection, as required by the repository
instructions. Across the searches and targeted questions it saved
approximately **255,704 tokens**. The tally is included so a future audit can
compare coverage rather than treating this as a grep-only sample.
