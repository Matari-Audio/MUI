# Driver scheduling forward port (2026-10-12)

Baseline: MUI `38fc732c9dc43b753e808a6727eef568f13754f0`.

The three new behavioral regressions were executed before the implementation:
all failed, while the 20 existing driver tests passed. The implemented policies
consume rejected layouts, space only automatic animation/live frames by 7 ms,
and defer only automatic work during 35 ms of continuous resize. Dirty redraws,
ordered input (including IME), resize frames and due repaint deadlines remain
immediate. Each actual size/scale change extends the quiet deadline. A final
reconciliation runs when resizing settles, including after an intervening input
frame. A refusal clears that final obligation too, avoiding unchanged retries.
`resized_at` accepts the host event clock; existing `resized` uses `Instant::now`.
`next_wake` reflects the paced automatic work and final resize deadline.

## Focused profile

`cargo run -p mui --example resize_schedule --locked --offline` uses a real
64-channel MUI tree (label, knob and live meter per channel), 200 native resize
samples at 60 Hz, 200 model changes halfway between samples and 20 ordered IME
inputs. It warms layout/text/springs for 120 ticks before profiling. Scene size
is 800x800; dependencies use the repository's optimized dev policy. This is a
headless driver trace, with no GPU rendering or native presentation timing.

For the baseline run, only a `resized_at` compatibility entrypoint forwarding
to the unchanged old `resized` method was added, so the exact trace can be
replayed. No scheduler optimization was present in the baseline. Current MUI
source was restored immediately after that run.

| Measure | Baseline | Updated |
| --- | ---: | ---: |
| Immediate resize frames | 200 | 200 |
| Intermediate live-only frames | 200 | 0 |
| Immediate ordered input frames | 20 | 20 |
| Final reconciliation | yes | yes |
| View build / resolve samples | 421 | 221 |
| Driver-loop elapsed time | 405.769 ms | 190.699 ms |
| Build p50 | 110.392 us | 107.752 us |
| Build p95 | 161.013 us | 166.623 us |

The savings come from 200 avoided builds, not a claim of faster individual
layout: this fixture's driver elapsed time fell about 53%. This justifies the
quiet-window policy for this focused workload; it is not a DAW CPU/FPS claim.
The additional regression cases cover initial refusal without a scene, a
renewed resize deadline, due repaint bypass, input during quiet, and recovery
from layout refusal after a real size change.
