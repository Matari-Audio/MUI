# RX 6600: shared GPUI gradient correction and component AA fixtures

The pinned wgpu gradient transfer was incorrect. The shared shader correction
reduces the standard 1× grayscale ramp's mean error from **50.705 to 0.247 / 255**.
It is applied by the common upstream preparation script, so the rebuilt GPUI
plugin and render lab use the same fix. It does not fix general-path AA.

## Root cause and scope

GPUI's `Rgba → Hsla` conversion preserves sRGB channel values. The wgpu shader's
`hsla_to_rgba` returns those encoded channels, despite its misleading “linear”
comment. The selected surface uses `Bgra8Unorm`/`Rgba8Unorm`, not an sRGB attachment.
The old gradient preparation incorrectly treated the stops as linear, encoded
them, then decoded the interpolated result. Solid colors used a different path.

The guarded patch (`experiments/render-lab/tools/fix_gpui_gradients.py`, removed in `92033ae`)
keeps sRGB stops/output encoded; Oklab interpolation explicitly decodes before
conversion to Oklab and encodes after conversion back. This agrees with the
pinned Metal backend's gradient convention. Solid colors, blend state, sampling,
geometry and theme tokens are not adjusted to compensate for the defect.

## Quality validation

AMD Radeon RX 6600, RADV NAVI23, Mesa 26.2.2, Vulkan, release render lab. Every
capture's report records its adapter. A dedicated 1920×1200 rootful Xwayland
surface prevents the desktop compositor resizing the benchmark window.

| Standard sRGB ramp | Before | After |
| --- | ---: | ---: |
| 1× mean channel error / 255 | 50.705 | 0.247 |
| 1.5× mean channel error / 255 | not repeated | 0.258 |
| 2× mean channel error / 255 | not repeated | 0.253 |

The GPU regression check (`experiments/render-lab/tools/check_gradients.py`, removed in `92033ae`)
validates colored sRGB and Oklab ramps on both native quads and paths against
independent analytic interpolation. All six GPUI captures (default/tight
at 1×/1.5×/2×) pass a maximum error of 2/255. It also records circle coverage.
The old capture fails the same colored sRGB calculation at 10.55/255 maximum
error, confirming the check detects the defect. The existing solid/alpha patches
remain exact in the recorded checks.

The component fixture imports the actual `PieContainer` implementation, including
its union/fillet and padded well. It places eight dots across three growing lanes,
four translations (0/.25/.5/.75 logical px), the default ADSR curve snapshot with
a translucent vertical gradient, and equivalent native/path circles at four
stroke widths. Theme colors are representative diagnostic values.

The early component baseline has two differences: it predates the extra Oklab
ramps and uses slightly different attack/decay values. Do not use its whole-image
error or timing as an isolated shader comparison. The **standard fixture is
identical before/after**, and the after-component checks use its own exact fixture.
Cairo image metrics exclude Oklab, which its SVG renderer does not implement;
the analytic check covers those ramps separately.

[Before standard](../rx6600-2026-09-13/gpui-1.png) ·
[After standard](after/standard/gpui-1.png) ·
[After components](after/components/gpui-1.png) ·
[Component metrics](after/components/gradient-check.json)

## AA remains a demonstrated limitation

The standard 1× quarter-pixel circle retains **24.22%** weak centerline samples
under the existing 2.5% coverage diagnostic. Tight tolerance yields **25.39%**.
This is unchanged by the color correction. At 1.5× those figures are 1.27% and
0.78%; at 2× they are zero in this particular fixture.

The new translated 20px-radius circles also show gaps with native rounded-quad
borders: the worst 1× quarter-pixel case has 13.18% weak samples, versus 12.35%
for the path equivalent. Both native and path 1px/1.5px borders have zero weak
centerline samples in that matrix. Native primitives are useful, but **native
alone does not guarantee continuous subpixel strokes**. These diagnostic counts
are not an overall perceptual-quality score.

Keep actual merged contours and fractional translations as the next AA acceptance
fixture. Do not globally tighten tessellation, widen every border, or switch the
whole framework based on these results. Any AA-path experiment must compare edge
coverage and actual editor costs together.

## Performance and integration checks

Sixty samples per fixture/backend/scale, five warmup frames, capture excluded.
Patched GPUI component medians were **0.447 / 0.456 / 0.529 ms** at 1×/1.5×/2×.
These are CPU acquire/draw/present/device-wait measurements, not GPU timestamps,
application FPS or proof of a speed improvement. Vello runs here are offscreen
quality references and must not be ranked against these presentation timings.

- Shared pie-container contract test and render-lab Clippy with `-D warnings` pass.
- Guarded patch preparation is idempotent; the plugin example rebuild passes.
- Native interaction checks pass at 1× on the lab display and 1.5× on the full
  desktop, covering focus/actions, gestures, drag/drop, cancellation, routes and power.
- At 1.5× on the smaller lab display, the interaction driver failed at source
  re-arming. Full-desktop success does not erase that restricted-viewport failure;
  the interaction harness/layout needs a separate follow-up. The rendering captures
  at 1.5× pass their independent gradient checks.
- No new DAW, DSP, typography or end-to-end latency claim is made here.

See [validation metadata](validation.json), and before/after JSON reports beside
these captures. The earlier hardware comparison is historical evidence from
before this gradient patch; its gradient defect is now corrected in this checkout.
