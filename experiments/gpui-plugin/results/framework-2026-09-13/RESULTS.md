# Truce framework foundation — 2026-09-13

The shared `mui-truce` adapter uses Truce parameter metadata and atomics, balanced automation gestures, and Truce persistence for per-instance module, routing, and theme state. The embedded GPUI probe uses the adapter and persists its editable name.

Validation:

- Workspace `tools/verify.sh` passed (Rust tests/clippy, WASM, TypeScript and generated fixture checks).
- Final adapter contract test and clippy passed.
- Final native input matrix: 7/7 passed, covering composition at 100%, 150%, and 200% scale and the embedded editor. Includes host parameter updates without UI edits, cancellation, instance isolation, and document recall while open and after reopening.
- Final CLAP validator: 37 successful, 7 skipped, 0 failed. Local Truce 6.3.0 wrapper patch sends the parameter-value rescan after state/preset load.

Native checks used the RX 6600 selection environment and a private headless Weston/Xwayland session. These are correctness checks, not a new rendering performance benchmark.

Remaining: migrate the legacy KURV composition to this shared document and parameter controller. No DAW integration or DSP work was performed. Truce custom PersistField rejection preserves the previous document but cannot make parameter-plus-document loading an atomic transaction; see the adapter README.
