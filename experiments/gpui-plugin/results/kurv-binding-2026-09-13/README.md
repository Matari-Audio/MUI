# KURV binding checkpoint — 2026-09-13

- Real KURV `mui-editor` test: native GPUI window paints; host level changes reach the shared card; resize bounds and close pass. Includes native control-scale round trips.
- Framework native input matrix: 7/7 (composition at 100/150/200%, two widths, plus embedded probe).
- `mui-truce` contract, WASM check, GPUI all-target clippy and explicit CLAP shared-library build pass.
- All native windows ran on private headless Weston/Xwayland, with the RX 6600 selected.

This is not full KURV migration acceptance, DAW certification, or a frame-rate benchmark. Remaining work is tracked in `docs/KURV-MIGRATION.md`.
