# Subsystem audit — 2026-09-13

Baseline: `ba0617e`. Linux x86-64, Ryzen 7 7800X3D, 16 logical CPUs.
Release uses the repository's size optimization (`opt-level = "s"`), LTO,
and one codegen unit. These are local microbenchmarks, not end-to-end FPS.

## Current state

Intrinsic flex layout, Boolean surfaces, fillets, parallel offsets, glyph
outlines, pointer interaction, Vello rendering, and TypeScript-to-Rust
authoring are implemented. Recent commits added derived light/dark palettes,
contrast checks, bounded neutral layers, and one-file constant themes.

## Measurements

Final run after compilation completed, tests run serially:

| Work | Time |
|---|---:|
| Resolve canonical pill scene | median 0.008 ms; p95 0.010 ms |
| Resolve scene plus inset of merged outline | median 0.093 ms; p95 0.107 ms |
| Rebuild 18 sidebar labels at 13 px | mean 0.314 ms |
| One 13 px label | mean 0.027 ms |
| Convert 40 rounded paths to Beziers | mean 0.020 ms |
| Extract one 280 px glyph | mean 0.002 ms |
| Vello CPU fill/stroke preparation, 1600×1000 scene | mean 0.008 ms |

Scene measurements use 20 warmups and 200 samples with `black_box`.
The existing text/render-preparation benchmark averages 20 iterations;
it has no percentile statistics and uses Hack Regular. These small workloads
do not establish scaling, GPU execution time, presentation latency, or memory use.
Label rebuilding is the largest measured CPU item; merged offsets cost about
12 times the basic scene resolve on this specimen.

## Quality and correctness

- Verification passed: formatting, workspace tests/doc tests, Clippy with
  warnings denied, library WASM check, TypeScript compilation, deterministic
  generated Rust, and the demo. The extended benchmark also passed Clippy.
- Geometry: 44 tests passed, including randomized shapes/insets, hole topology,
  tangency, collapse/split behavior, budgets, and invalid inputs. The existing
  general-offset distance test bounds error below 0.08 units for an 8-unit
  inset at 0.02 flattening tolerance; this is a fixture bound, not a universal
  error measurement.
- Typography: 13 tests passed. Baseline metrics, counters, overlapping glyphs,
  advance positioning, missing glyphs, and tolerance-dependent detail are covered.
  Text remains advance-only and unhinted: no kerning/shaping, bidi, fallback
  chain, or first-class measured layout leaf. The curve subdivision cap of 64
  means arbitrarily small requested tolerances are not guaranteed.
- Anti-aliasing: headless rendering succeeded and the 640×360 image was visually
  inspected. Curved edges and the glyph counter look intact. The transparent
  render contains 956 partially covered pixels across 223 intermediate alpha
  levels, confirming fractional coverage. This is a smoke check, not comparison
  with a ground-truth raster or a small-text/high-DPI quality benchmark.
- GPU resize: the explicit readback regression passed. The test does not report
  adapter identity, so it does not establish hardware-versus-software execution.
- Palette: existing contrast tests passed. Their guarantees concern generated
  colors, not a complete UI accessibility audit. Adjacent neutral fills are
  intentionally subtle; the documented dark palette cannot provide a 3:1
  boundary against its ground through neutral fill alone.
- Interaction and layout tests passed, including painted-path/hit agreement.
  Keyboard traversal, IME, and accessibility integration remain incomplete.

## Reproduce

```sh
./tools/verify.sh
cargo test -p mui-preview --release --test frame_cost --offline -- --ignored --nocapture --test-threads=1
cargo test -p mui-preview --test render_resize --offline -- --ignored --nocapture
cargo run -p mui-vello --example headless --offline -- /tmp/mui-audit.png
```

Session logs: `/tmp/mui-audit-verify.log`, `/tmp/mui-audit-bench-final.log`,
`/tmp/mui-audit-gpu.log`. Image: `/tmp/mui-audit.png`.

The next useful measurements are sustained frame latency with the actual GPU
identified, layout/geometry scaling, and small text at multiple display scales.
The current results do not justify a broad performance rewrite.
