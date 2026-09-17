# First KURV migration brick — 2026-09-13

- Probe library tests: 6 passed, 0 failed, including finite/order/endpoint curve checks.
- Probe library clippy with `-D warnings`: passed. Two existing upstream GPUI Linux dead-code warnings remain.
- Input matrix: 7/7 passed on the RX 6600 selection environment in private Weston/Xwayland. Composition at widths 1280 and 1480, scales 1, 1.5 and 2; embedded probe at 1x.
- Each composition case first drags the new LFO curve using GPUI pointer events, verifies balanced owner events, external recall without feedback, cancellation and disabled edits, then runs the existing oscillator/modulation regression sequence.

The curve model currently uses straight segments. Musical LFO modes, KURV spline evaluation, persistence and DSP bindings remain pending. These correctness results do not establish rendering performance or full KURV migration.
