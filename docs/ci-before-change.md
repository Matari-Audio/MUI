# Hosted UI suite contracts

The coordinator is the sole local builder. This branch compiles, formats and tests only on the public Matari-Audio/MUI GitHub-hosted runners.

| Path | Thread and overlap | Ownership | Bounds and recovery | Distinguishing proof |
| --- | --- | --- | --- | --- |
| Retained material update | Render worker; queue submissions ordered | Renderer owns scene encoding, effect textures and target until worker teardown | Existing effect budget; only replay complete encodings with unchanged mappings, transforms and placements; backdrop dependencies invalidate | `gpu_contract`: morph uploads 16 bytes, encodes zero scenes, changes pixels; effects damage comparisons |
| WARP admission | GPU initialization worker before device creation | Shared session owns adapter/device; production still rejects WARP | Explicit `test-support` feature AND `MUI_TEST_ALLOW_WARP=1`; ordinary builds cannot opt in | Admission unit tests and hosted DX12 native presentation with static DXC and FXC |
| Native lifecycle tests | OS UI thread plus render workers; close joins before reopen | Each test owns native window and compositor session | X11 tests under Xvfb; Wayland test under Weston; child deadlines retain failure artifacts | Native editor, focus/dropdown/drag regressions and lifecycle receipts |
| Declaration-pool capture test | Test thread; callbacks destruct reentrantly on same thread | Arc captures drop before pool borrow; test counter atomic | Existing entry/capacity/byte limits unchanged | Clippy on all OSes and drop-count/reentrant ownership tests |
| CI lockfile refresh | Hosted maintenance job only | Resolved lockfiles returned as artifacts for review | Pinned Rust and unchanged manifests; locked gates reject drift | Lockfile diff review followed by `cargo fetch --locked` in each workspace |
