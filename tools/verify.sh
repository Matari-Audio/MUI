#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo fmt --all -- --check
# Every workspace feature is additive (mui-vello's cpu/gpu-effects backends and
# their facade forwards), so one all-features surface covers test, lint and wasm.
cargo test --workspace --all-features --locked --offline
cargo clippy --workspace --all-features --all-targets --locked --offline -- -D warnings
# --all-features hides a cfg that only breaks with a feature off: mui-vello's
# backends each compile alone and with none.
cargo clippy -p mui-vello --no-default-features --all-targets --locked --offline -- -D warnings
cargo clippy -p mui-vello --no-default-features --features cpu --all-targets --locked --offline -- -D warnings
cargo clippy -p mui-vello --no-default-features --features gpu-effects --all-targets --locked --offline -- -D warnings
# mui-preview is a native dev host: it owns a winit event loop and a wgpu
# surface, neither of which this gate can build for wasm. The wasm claim is
# about the library crates; drop the --exclude once the preview grows a
# `spawn_app` entry point and is served from a canvas.
# mui-gain-plugin is a CLAP/VST3 cdylib: truce-vst3 compiles a C++ shim and
# truce-clap wants a native parent window, so it has no wasm build at all.
# mui-truce itself stays in: its window/GPU half is cfg'd out on wasm32.
cargo check --workspace --all-features --exclude mui-preview --exclude mui-gain-plugin --target wasm32-unknown-unknown --locked --offline

# ---------------------------------------------------------------------------
# media/: its own workspace (mui-stage, mui-reel, mui-motion-bridge), kept out
# of the root so plugin builds never compile it. Same gate, its own lockfile.
# mui-stage's default `backends` feature is the only feature; --all-features
# keeps it on. No wasm check: these crates own a native device or ffmpeg.
# ponytail: no --locked here. media/Cargo.lock also pins the root crates'
# dependencies, so --locked would fail on every root dependency change; the
# gate refreshes it instead and git status shows the diff.
# ---------------------------------------------------------------------------
cargo fmt --manifest-path media/Cargo.toml -p mui-stage -p mui-reel -p mui-motion-bridge -- --check
cargo test --manifest-path media/Cargo.toml --workspace --all-features --offline
cargo clippy --manifest-path media/Cargo.toml --workspace --all-features --all-targets --offline -- -D warnings
cargo check --manifest-path media/Cargo.toml -p mui-stage --no-default-features --offline
