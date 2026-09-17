# Curve gesture history — 2026-09-13

- Core curve/history tests passed: bounded history, redo branching, no-op commits and external-state protection.
- Core WASM check and core/UI library clippy passed.
- Native headless input matrix: 7/7 passed. Both LFO and unison use actual Ctrl-Z/Ctrl-Shift-Z events; owner event balance, cancellation and recall checks remain in the matrix at 100%, 150% and 200% scale.
- Keyboard dispatch is performed outside a leased parent entity in the test harness, avoiding reentrant updates.

Desktop display variables were removed before starting the private headless compositor. No desktop test windows were opened.

Local curve history is optional and capped at 32 gestures. Whole-document KURV history integration remains separate; owners can disable local history.
