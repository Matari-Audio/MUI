# Measured renderer results — 2026-10-08

All five engines passed four fixture cases each, on both hosted software Vulkan
and local RX 6600 hardware: 1,200 measured frames per dataset, plus warmups and
first frames. These are basic rectangle/polyline coverage and animation checks,
not complete visual parity or plugin lifecycle validation.

Local hardware: Ryzen 7 7800X3D, Radeon RX 6600, Vulkan/RADV Mesa
26.2.4-arch3.1. Executed CI-built release binaries from fdca0ecb sequentially;
no local Rust compilation, and other Rust jobs remained running. The background
load means these are preliminary hardware measurements, not isolated lab results.

Dynamic redraw medians in milliseconds, including scene encoding, completed
rasterization and CPU RGBA readback. Each row has 30 samples after five warmups.

| Engine | Controls 1× | Controls 2× | Vectors 1× | Vectors 2× |
|---|---:|---:|---:|---:|
| classic | 1.112 | 3.050 | 1.611 | 3.542 |
| vello-gpu | 1.032 | 2.462 | 2.824 | 5.298 |
| vello-cpu | 0.478 | 1.386 | 2.403 | 4.218 |
| skia-cpu | 0.900 | 1.970 | 8.442 | 13.484 |
| gpui | 1.142 | 4.499 | 3.307 | 7.506 |

At 2× vectors, classic Vello used 0.738 ms of process CPU per completed frame,
versus 3.475 for Vello GPU, 4.213 for Vello CPU and 5.207 for GPUI. CPU time
matters for audio plugins even where readback-inclusive wall times are close.
Readback is required by GPUI's public completion API and is included for every
engine. A normal presented plugin frame does not require CPU readback, so these
rows cannot determine a production GPU winner.

The software run also passed all engines. Its Vello CPU job used an EPYC 9V74;
the other engines used EPYC 7763 hosts. Those separate-job numbers are recorded
in the JSONL but must not be treated as a controlled ranking. CI now adds a
sequential same-host comparison job using all five already-built binaries.

MUI's existing full editor baseline used 673 surfaces, 612 paint operations and
474 glyph runs on software Vulkan. A moving knob took 3.678 ms with retained CPU
versus 6.311 ms with retained classic GPU. Static retained frames took roughly
0.73 ms with both, dominated by UI/layout rather than drawing. Its vector-heavy
editor took 26.714 ms retained CPU and 26.071 ms retained GPU; scene encoding and
preparation account for most of that cost. These timings exclude readback and
use different geometry from the shared fixture, so they are a separate baseline.

Recommendation: these results do not justify replacing Vello with GPUI's raster
backend for speed. Vello CPU is a strong low-memory fallback for ordinary
controls; classic Vello remains competitive for dense animated vectors and uses
less process CPU on this GPU. Vello GPU deserves a full-scene/material prototype,
not an unconditional replacement. GPUI's ownership/lifecycle ideas may still
reduce MUI complexity, but native embedding, device failure handling, text,
materials and DAW behavior need direct evidence before replacing the host layer.

The macOS Metal gallery now passes after fitting the tester into the desktop
work area. Windows WARP/DX12 still exits with 0xc0000005 after one surface
submission. That is a reproduced native fault, not proof of NVIDIA/Intel driver
failures or of a Vello-specific cause. Native stack capture is the next step.

Raw CI evidence: https://github.com/Matari-Audio/MUI/actions/runs/37780498626
Local raw samples and PNGs: /tmp/mui-renderer-local-rx6600-fdca0ecb
Compact complete summaries: renderer-measurements-2026-10-08.jsonl

Not measured: native GPUI D3D11/Metal, NVIDIA/Intel hardware, hosted plugin
lifecycle for candidate engines, full text/effects/material parity, audio xruns,
GPU timestamps or GPU memory. Startup is fresh-process, not guaranteed cold
driver-cache startup.
