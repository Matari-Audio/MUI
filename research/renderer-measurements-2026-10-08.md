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
in the JSONL but must not be treated as a controlled ranking. The subsequent sequential same-host comparison passed all 20 cases on one EPYC
7763 VM and one llvmpipe/Mesa 25.2.8 Vulkan adapter, with no compilation during
measurement. Dynamic redraw medians, in the same column order as above:

| Engine | Controls 1× | Controls 2× | Vectors 1× | Vectors 2× |
|---|---:|---:|---:|---:|
| classic | 9.558 | 28.529 | 17.386 | 39.352 |
| vello-gpu | 3.035 | 10.682 | 11.324 | 26.731 |
| vello-cpu | 0.853 | 2.739 | 4.267 | 7.595 |
| skia-cpu | 1.539 | 4.765 | 14.024 | 24.366 |
| gpui | 15.559 | 56.898 | 39.084 | 114.583 |

This software-adapter result favors CPU rasterization for these workloads;
it is not a hardware GPU or D3D11/Metal ranking. All 40 timing distributions,
startup/first-frame/RSS and environment metadata are preserved as `ci-same-host`
in the JSONL. The runner CPU and CI merge revision are recorded, rather than
inferred from a separately built engine job.
The earlier 010c5cd1 benchmark attempt failed to compile because a local
`classic` module shadowed the dependency in the shared path helper. The helper
now imports kurbo directly; that harness bug was not a renderer runtime failure.

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

The macOS Metal and forced CPU galleries passed 750 frames each on fc666b46;
standalone multiwindow resize/teardown also passed. Embedded child presentation
and resize worked but final native event-loop teardown timed out. The native
fixture deliberately fails that path; passing standalone work does not hide it.

Windows WARP/DX12 previously exited with 0xc0000005 after one submission.
Normal and CDB reproductions locate the fault in
`d3d10warp!ComputeShaderTransformer::ConstructLoadStoreSets+0x319` on a shader
compiler worker. The shared host now rejects the measured Microsoft CPU/DX12
adapter before compute submission and uses software presentation. This does not
explain NVIDIA/Intel user crashes. Exact versions, dump evidence and attribution
limits are in [platform-validation-2026-10-08.md](platform-validation-2026-10-08.md).

Raw CI evidence: https://github.com/Matari-Audio/MUI/actions/runs/37780498626
Controlled same-host evidence: https://github.com/Matari-Audio/MUI/actions/runs/37784385465
Local raw samples and PNGs: /tmp/mui-renderer-local-rx6600-fdca0ecb
Compact complete summaries: renderer-measurements-2026-10-08.jsonl

Not measured: native GPUI D3D11/Metal, NVIDIA/Intel hardware, hosted plugin
lifecycle for candidate engines, full text/effects/material parity, audio xruns,
GPU timestamps or GPU memory. Startup is fresh-process, not guaranteed cold
driver-cache startup.
