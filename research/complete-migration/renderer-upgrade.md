# Current renderer forward port

Classic Vello, encoding, and registry shaders move together from 0.10 to 0.11.
CPU/common move from 0.2 to 0.3, with glifo 0.4 and PNG glyph decoding enabled.
The exact upstream commit and complete source comparison are recorded in
`vendor/vello/PATCHES.md`. All retained binding, compute-pass, texture-pool and
hint-instance patches remain. The pipeline-cache namespace also advances to
`wgpu30-vello0.11`.

The CPU adapter adopts explicit pixel metadata and byte output while preserving
the former floor premultiplication. Glyph errors now reach `paint` through an
additive `Canvas::try_glyphs`; existing canvases keep their void `glyphs` method.
Root examples and media exports adopt the new straight-alpha byte output. The
optional media sparse renderer keeps its independently pinned git development
source and explicit adapter boundary.

The original synthetic sbix fixture tests non-Apple bitmap bearing placement,
CPU/GPU pixel parity and unchanged-frame retention. Restoring only the former
GPU `scene.rs` causes the authored-bearing assertion to fail. The 0.11 scene
passes on the Radeon RX 6600. The authored fixture's deterministic SHA-256 is
`d370689e2be0998bb13f0b0bff8a9d4fac8fa037007a368e554f5ecec006aeb1`.
Before/after PNG captures are review artifacts outside the repository; the
font, generator, provenance and behavioral test are committed.

Validation uses the owned target directory, two build jobs, no incremental
builds and zero dev/test debug information. On stable Rust 1.98.1:

- Root all-feature workspace tests and doctests: 1098 passed, 31
  measurement/native/discovery/illustrative-documentation ignores.
- `tools/verify.sh root-lint`: strict all-target Clippy, separate no-backend,
  CPU-only and GPU-only configurations, workspace WASM and Python guards pass.
- Media all-feature unit/integration tests: 273 passed, including native
  adapter build fixtures and GPU rendering checks.
- Media all-feature doctests pass on settled source. An earlier E0463 during
  concurrent source edits did not recur; rustc/rustdoc fingerprints match and
  neither has an environment flag override.
- `tools/verify.sh media-lint`: strict all-target Clippy, stage without default
  features, CUT WASM, formatting and the six-test pin guard pass.
- The four normally ignored GPU lifecycle tests pass when explicitly run
  serially on RX 6600 Vulkan: shared/rebuilt generations, gated late-result
  close, actual device destruction/recovery and surface reconciliation.
  Together they complete in 0.28 seconds. The bitmap fixture also passes in
  0.07 seconds and logs the actual adapter instead of silently claiming a GPU.

These checks establish API/build compatibility and rendering correctness. No
general CPU or GPU throughput improvement is claimed for the release upgrade.
