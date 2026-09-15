#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo fmt --all -- --check
# The only optional workspace feature is the additive mui facade `egui` feature;
# keep tests, lint, and the wasm check on the same all-features surface.
cargo test --workspace --all-features --locked --offline
cargo clippy --workspace --all-features --all-targets --locked --offline -- -D warnings
# mui-preview is a native dev host: it owns a winit event loop and a wgpu
# surface, neither of which this gate can build for wasm. The wasm claim is
# about the library crates; drop the --exclude once the preview grows a
# `spawn_app` entry point and is served from a canvas.
cargo check --workspace --all-features --exclude mui-preview --target wasm32-unknown-unknown --locked --offline

# packages/mui-ts is frozen (see packages/mui-ts/FROZEN.md): the Rust DSL is
# the source of truth and the TS compiler is no longer part of the gate.
