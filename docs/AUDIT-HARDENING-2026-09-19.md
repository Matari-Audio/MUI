# Audit hardening — source patch, not yet compile-validated

Base: `a9cc11d232986fce8dfb1a12aa20afd7dcbb664e`.

## Implemented in this patch

- Move the canonical MUI-to-Kurbo conversion into `mui-geometry`, retaining
  `mui_vello::bez_path` and `ARC_TOLERANCE` as compatibility re-exports.
  `mui-input` no longer depends on `mui-vello` or `vello_common` directly.
- Carry exact ancestor clip paths into hit testing. Rounded corners and holes
  are tested, and a floated node escapes the same clips that its paint escapes.
- Carry authored semantic parentage into AccessKit. Overlapping siblings are
  no longer nested merely because one rectangle contains the other.
- Carry current text independently of an explicit accessibility label;
  `set_text` updates it after shaping succeeds without moving its layout frame.
- Add identity-based Focus / Activate / SetValue semantic requests. The preview
  handles numeric SetValue and no longer fakes a pointer click at a node centre.
  Disabled controls advertise no mutating actions. Custom controls must consume
  the matching action through the runtime's get/drag contract.
- Preserve pending gesture End on repeated cancellation; close() drains pending
  gesture edges without requiring another successful frame. Removing/disabling
  a captured control reconciles capture against the current resolved tree.
- Reject non-finite/negative frame deltas before changing runtime state. Expose
  edits_for() because an atomic semantic action has Begin and End in one frame.
- Bound the full text-cache variant count, including animated font sizes and
  weights. Own the font allocation and invalidate on tolerance changes. This is
  still a flush-at-4096-entries cache, not an LRU or a byte-budgeted cache.
- Sweep unused motion entries after a successful frame, and remove selection /
  scroll state when its owner disappears. Applications requiring state to survive
  temporary unmounts should store that state in their model; this patch does not
  introduce a retained-offscreen policy.
- Add Id::entity(u64), so permanent model identities are not truncated on wasm32.
  Keep stable identity in .id() and display naming in .label(). This helper does
  not mint IDs, prevent reuse, or automatically repair position-based keys.
- Make arrows, deletion and selection respect extended grapheme boundaries while
  preserving the existing scalar-index Host API; avoid duplicate insertion when
  a host supplies both printable Key and committed text.
- Add regression tests plus a Linux/Windows/macOS compilation/test matrix and
  a separate WASM library check. These jobs do not test a real DAW or live GPU.

## Explicitly not completed

**Parley restoration is still required.** The legacy advance-only text_run is
not replaced by these editing fixes. Do not relabel this patch as complete text
shaping, bidi editing, font fallback, complex-script support, or Parley integration.
The renderer's single-font `(glyph_id, x)` representation must become multi-run,
font-index-aware `(glyph_id, x, y)` data with cluster/caret mapping. Keep one owned
FontContext/LayoutContext per UI or coarse text-service lifetime. Make measurement,
painting, hit/caret mapping and line breaking consume the same shaped layout.

**The native CLAP/VST3 host is not implemented.** It needs real parent-window
embedding, per-platform threading/lifetimes, resize negotiation, focus, IME,
clipboard and GPU loss handling, tested in target DAWs. A winit demo or a trait
that mentions a parent handle is not that implementation.

**The custom GPU compositor is not implemented.** Define its contracts against a
real host/device/target lifecycle, including clipping, z order, texture lifetime,
colour space and CPU fallback. Do not claim an opaque callback placeholder is an
integrated GPU surface or live glass/refraction.

Other remaining concerns: a byte budget / eviction policy for cached text and the
process-global font cache; measured font-width reservations; nested-pin dependency
ordering; keyboard activation/value stepping for all controls; full event ordering;
shaped horizontal text-input hit testing; and actual fuzzing/property campaigns.

## Validation status

Rust compilation, rustfmt, Clippy, the Rust tests, GPU rendering, cross-platform
hosting and benchmarks were NOT executed when this patch was authored. The build
container had no Rust toolchain and could not access GitHub/crates directly. GitHub
branch creation was rejected with HTTP 403, so no remote CI was triggered either.
Only the Python patch applicator/package validation was executed locally.

Run `cargo fmt --all`, then `cargo test --workspace --all-features --locked`,
`cargo clippy --workspace --all-features --all-targets --locked -- -D warnings`,
and `bash tools/verify.sh` before treating this as a candidate for merging.
Review benchmark changes from exact clipping and scene text retention, rather than
assuming the previous audit's timings still apply.
