# Public hosted UI qualification

MUI is public. All jobs use standard GitHub-hosted runners; none require a paid larger runner or a private plugin repository. `Verify` calls `ui.yml`, which calls the shared `native-ui.yml` recipe. Native setup, pinned Rust, Mesa dependencies and Swatinem caches live in `.github/actions/native-setup`. Third-party actions are pinned by full commit SHA. Superseded PR runs cancel automatically. Renderer, browser and drag workflows use path filters; the required Verify aggregate always runs.

## Matrix

| Job | Runner | Required evidence |
| --- | --- | --- |
| Root lint | ubuntu-24.04 | rustfmt; Clippy `-D warnings`, all targets/features and feature-isolation checks; WASM libraries; evidence collector checks |
| Root tests | ubuntu-24.04 | Entire workspace with all features; standalone moose-baseview and XIM regressions; portal teardown |
| Media shards | ubuntu-24.04 | Locked media resolution, lint/WASM, CUT tests, native media tests |
| Native platforms | windows-latest (MSVC), macos-latest | Native Clippy `-D warnings`, workspace tests and vendored moose-baseview tests; adapter/shader unit tests included automatically |
| Windows GNU | ubuntu-24.04 | Clippy all targets/features for x86_64-pc-windows-gnu; explicitly cross-compiled coverage |
| X11 display | ubuntu-24.04 | Lavapipe Vulkan / llvmpipe GL under Xvfb; gallery/input, embedded editor, damage, device loss, CPU fallback, multiwindow teardown |
| Wayland | ubuntu-24.04 | Weston headless compositor; standalone and embedded resize/close/reopen; distinct from XWayland |
| Native drag | ubuntu-24.04, windows-latest, macos-latest | Gesture ownership, X11 ignored wire regression, Win32 capture, AppKit coordinates |
| Native GPU qualification | ubuntu-24.04 Vulkan, windows-latest DX12 with StaticDxc and Fxc, macos-latest Metal | Actual GPU presentation; native editor resize/close/reopen; gallery; effects integration tests; `gpu_contract`; every Vello AA pipeline and MUI RGBA/BGRA pipelines; GPU/CPU pixel oracle |
| Embedded CPU | windows-2025, macos-15 | Explicit CPU plus automatic fallback and native close/reopen |
| Renderer measurements | ubuntu-24.04; native GPUI bridge also windows-2025 and macos-15 | Isolated engines and same-host comparison, production editor workloads, native image bridge/input/teardown |

No frame cap is introduced. Production remains damage-driven and display-paced. Timeouts bound test processes, not rendering cadence.

## Local reproduction

Use the pinned `rust-toolchain.toml`, native platform toolchain and dependencies from the setup action. Run commands from the repository root. The coordinator owns local builds in shared work sessions.

* Gate: `cargo fetch --locked`, fetch the standalone vendor lockfiles, then `tools/verify.sh root-lint` and `tools/verify.sh root-test`. Media: `cargo fetch --manifest-path media/Cargo.toml --locked`, then `tools/verify.sh media-lint`, `media-cut-test`, or `media-native-test`.
* Native lint/tests: `cargo clippy --workspace --all-features --all-targets --locked -- -D warnings`, `cargo test --workspace --all-features --locked`, `cargo test --manifest-path vendor/moose-baseview/Cargo.toml --lib --locked`.
* GNU: install `gcc-mingw-w64-x86-64`, `g++-mingw-w64-x86-64`, and `rustup target add x86_64-pc-windows-gnu`; run the native Clippy command with `--target x86_64-pc-windows-gnu`.
* GPU/native: build the examples and gallery using the exact commands in `native-ui.yml`, set `WGPU_BACKEND=vulkan`, `metal`, or `dx12`, and run `python3 tools/ci/native_suite.py`. Linux wraps this in `xvfb-run -a dbus-run-session -- ...`; choose the lavapipe ICD with `VK_ICD_FILENAMES`. The display job in `ui.yml` contains the complete X11 and Weston commands.
* Drag: `cargo test --manifest-path vendor/moose-baseview/Cargo.toml --lib --locked drag::tests`; run X11 `platform::x11::drag_n_drop::tests -- --ignored --test-threads=1` under Xvfb. Windows also runs `capture_tests`; macOS runs `--test mac_drag_geometry`.
* Renderer and native GPUI: reproduce the build and invocation commands in `renderer-bench.yml`; its independent workspaces have their own lockfiles and cache mappings. These measurements compare candidates on one runner, not across dissimilar machines.

## WARP is a test opt-in

Production rejects Microsoft's CPU DX12 adapter because driver shader compilation can fault outside Rust's panic boundary. Hosted correctness jobs must **both** compile `mui-vello/test-support` and set `MUI_TEST_ALLOW_WARP=1`. An environment variable alone cannot enable WARP in a normal build. The admission test checks the default rejection separately from the opt-in policy without mutating global environment.

On x86_64 MSVC, set `WGPU_DX12_COMPILER=StaticDxc` or `Fxc`. Each compiler gets its own required native job. `gpu_pixels` constructs all vendored Vello AA pipelines (including MSAA8/16) and both MUI output formats under a validation error scope. Missing adapters, validation errors, native child failures, and pixel mismatches fail the job. Software adapters qualify correctness only.

Windows Vulkan via Mesa or SwiftShader is supplementary future coverage: WARP exercises the native Windows production DX12 path; Linux lavapipe already exercises software Vulkan. Do not imply Windows Vulkan qualification from those rows.

## Pixel and retained-scene evidence

`gpu_pixels` compares the identical resolved paint through independent CPU and GPU rasterizers at 1x, 1.5x and 2x. Per-backend channel maximum/mean limits are explicit in the example (Vulkan/lavapipe, DX12/WARP, Metal). Every pixel participates. `gpu-*.png`, `cpu-*.png` and amplified opaque `diff-*.png` are uploaded even on failure; console logs report measured errors and limits. `gpu_contract` independently checks clipping, paint order, stable frames, changed morph pixels, a 16-byte uniform write, no allocation, and zero scene re-encoding.

`CI source maintenance` resolves dependent workspace lockfiles on a hosted runner and returns a reviewable patch; it does not push changes. Locked gates never silently refresh dependency graphs. Review source/version/checksum deltas before applying that patch.

## Scope

Hosted software/virtual GPUs do not qualify Intel/AMD integrated GPUs, discrete AMD/NVIDIA devices, mixed-DPI multi-monitor systems, real drag-and-drop between applications, or DAW embedding/format behavior. Metal VM receipts identify the adapter actually exposed. Hardware, real DAWs (CLAP/VST3/AU where supported), driver loss and installed release artifacts remain release qualification work.
