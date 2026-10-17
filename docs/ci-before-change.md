# Hosted UI suite contracts

The coordinator is the sole local builder. This branch compiles, formats and tests only on the public Matari-Audio/MUI GitHub-hosted runners.

| Path | Thread and overlap | Ownership | Bounds and recovery | Distinguishing proof |
| --- | --- | --- | --- | --- |
| Retained material update | Render worker; queue submissions ordered | Renderer owns scene encoding, effect textures and target until worker teardown | Existing effect budget; only replay complete encodings with unchanged mappings, transforms and placements; backdrop dependencies invalidate | `gpu_contract`: morph uploads 16 bytes, encodes zero scenes, changes pixels; effects damage comparisons |
| WARP admission | GPU initialization worker before device creation | Shared session owns adapter/device; production still rejects WARP | Explicit `test-support` feature AND `MUI_TEST_ALLOW_WARP=1`; ordinary builds cannot opt in | Admission unit tests and hosted DX12 native presentation with static DXC and FXC |
| Native lifecycle tests | OS UI thread plus render workers; close joins before reopen | Each test owns native window and compositor session | X11 tests under Xvfb; Wayland test under Weston; child deadlines retain failure artifacts | Native editor, focus/dropdown/drag regressions and lifecycle receipts |
| Declaration-pool capture test | Test thread; callbacks destruct reentrantly on same thread | Arc captures drop before pool borrow; test counter atomic | Existing entry/capacity/byte limits unchanged | Clippy on all OSes and drop-count/reentrant ownership tests |
| CI lockfile refresh | Hosted maintenance job only | Resolved lockfiles returned as artifacts for review | Pinned Rust and unchanged manifests; locked gates reject drift | Lockfile diff review followed by `cargo fetch --locked` in each workspace |
| Test-only native event queue | Fixture producer → native pump → existing render-worker event queue | Requests owns at most 32 pending events; native pump takes and forwards; close revokes worker delivery | Overflow returns an error; compiled only with mui-baseview/test-support | Native editor scripts actual capture/focus callbacks, first row click and control drag before each close/reopen |
| CUT CPU bridge | Media render workers and CLI workers | CUT and mui-vello share the same vendored RenderContext/Resources types | Native and WASM dependency edges use one path/source; independent git sparse renderer stays separate | Locked media lint, CPU renders and CUT tests reject duplicate incompatible CPU types |
| CUT cached base scene | Media render worker | One boxed base scene retained until its key changes or the renderer drops | Single cache entry; allocation only on cache miss | Media Clippy and rendering tests |
