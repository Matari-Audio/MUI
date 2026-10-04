# Profiling a window

Profiling is disabled by default. `driver.enable_profiling(ProfileConfig::default())`
preallocates 512 recent samples per phase. Capacity is clamped to 1–4096;
recording never takes a lock, spawns a worker, or writes a file. Keep all calls on
the window's UI thread. No profiling API belongs in an audio callback.

Driver samples cover view construction, `Ui::frame` resolution (input dispatch,
layout, scene and cache work combined), and the complete `advance` call including
skips. Queue latency is **oldest input in a batch to dispatch**, with timestamps
from the real monotonic clock; synthetic animation timestamps are not latency
measurements. Inert discarded hovers have counters but no dispatch latency.
Counters distinguish idle/inert/throttled/minimized skips, explicit redraw/model
changes/resizes, coalescing, failed layouts, and the existing measured layout and
weld cache work.

A native adapter can record observable intervals without acquiring a clock when
disabled:

```rust,ignore
let start = driver.profiler().map(|_| std::time::Instant::now());
native_present();
if let (Some(profile), Some(start)) = (driver.profiler_mut(), start) {
    profile.record_since(mui::profiling::Phase::PresentCall, start);
}
```

`PresentCall` means the CPU present function returned. It does not mean GPU work
finished or pixels reached the display. `InputToPresentCall` links the oldest
input in the dispatched batch to this return, and is emitted only if a present
call is recorded. The backend must call `discard_pending_presentation()` for
`Frame::Current` (no visual change), and keep the marker for `Frame::Skipped`
(deferred presentation). Deferred frames preserve the earliest pending input
across later input batches; window close clears both queued and presentation
markers. This is an observed CPU batch-to-present metric, not a claim that every
input changed a pixel. Encode/submit/backend-draw/native-wake intervals require actual
backend hooks. Nested phases overlap; do not add their times together.

Call `profiler.percentiles(phase)` to get p50/p95/p99 over retained samples plus
retained/all-time sample counts. An empty phase has count zero: zero percentiles
are not measurements. `driver.take_profiler()` moves a completed session out of
the callback; write `profile.write_csv(file)` after the callback/lock has returned.
Each Driver owns one window's samples. Sequence, monotonic elapsed timestamp and
duration (nanoseconds) accompany every exported sample. Export can block on I/O;
it is an explicit developer operation, never automatic frame or audio work.

## Repeatable CPU workloads

```sh
cargo run -p mui --example profile_scenarios --release -- --frames 600 --output /tmp/mui-profile
```

The harness prints its workload contract and tail latencies and exports one CSV
per scenario. It uses 800×800 physical pixels at scale 1, Hack font and synthetic
60 Hz UI steps, with 120 settling ticks before collection. Scenarios are a
knob/meter with four input samples per tick and 30-tick gesture cycles; an eager
1000-row scroll list with 24-pixel rows; 64 vector paths of 128 points; settled
idle; width resizing from 700 to 799; and recreation of the Ui/Driver with cold
retained caches. Reopen explicitly aggregates profiling sessions across new
windows. This eager-list case is a baseline, not a virtual-list benchmark.

Use the same sizes, node/path counts, font, inputs, settling ticks and backend
when porting a workload to GPUI or a native MUI runner. Compare release builds on
the same hardware, recording OS, renderer, display refresh and power state.
These headless CPU numbers cannot establish GPU or input-to-photon parity.

The bounded foreground-recording approach follows the useful pattern in GPUI's
Apache-2.0 profiler (`crates/gpui/src/profiler/journal.rs`, Zed revision
`a84689073d296dfd39987bc7dd478e43ef76d83a`); this implementation copies no GPUI code.
