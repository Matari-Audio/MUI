# Shared Bézier component validation — 2026-09-13

MUI core now owns the normalized cubic model, validated import, evaluation, splitting and response sampling. LFO and compact unison use the same GPUI editor. Native paths and SVG export consume cubic control points.

- Core Bézier contract passed: x inversion, shape-preserving subdivision, edits/import validation, clamped/periodic evaluation, response sampling and capacity.
- Workspace verification passed, including Rust, WASM and TypeScript checks.
- Final integration library tests: 5 passed. Library clippy passed with warnings denied for local code; two existing upstream GPUI Linux dead-code warnings remain.
- Final headless input matrix: 7/7 passed. Both LFO and unison anchor/handle drags, owner gesture balance, recall, cancellation and disable checks run before the existing composition tests at 100/150/200% scale.

The final matrix was launched with DISPLAY, WAYLAND_DISPLAY and WAYLAND_SOCKET removed; clients only received the private headless Weston/Xwayland display. No desktop windows were opened for these final checks.

This is a reusable curve component, not a completed KURV DSP/preset migration or a new performance benchmark.
