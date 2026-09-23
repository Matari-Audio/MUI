#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo fmt --all -- --check
# Every workspace feature is additive (mui-vello's cpu/gpu-effects backends and
# their facade forwards), so one all-features surface covers test, lint and wasm.
cargo test --workspace --all-features --locked --offline
cargo clippy --workspace --all-features --all-targets --locked --offline -- -D warnings
# mui-preview is a native dev host: it owns a winit event loop and a wgpu
# surface, neither of which this gate can build for wasm. The wasm claim is
# about the library crates; drop the --exclude once the preview grows a
# `spawn_app` entry point and is served from a canvas.
cargo check --workspace --all-features --exclude mui-preview --target wasm32-unknown-unknown --locked --offline
