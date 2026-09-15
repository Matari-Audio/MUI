#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo fmt --all -- --check
# Keep the full native workspace gate locked and feature-complete.
cargo test --workspace --all-features --locked --offline
cargo clippy --workspace --all-features --all-targets --locked --offline -- -D warnings
# Only renderer-independent crates are promised to compile for wasm. Native
# preview/GPUI/Vello hosts own windowing and GPU backends.
cargo check -p mui-geometry -p mui-layout -p mui-tessellate -p mui-core \
  -p mui-text -p mui --target wasm32-unknown-unknown --locked --offline

# packages/mui-ts is frozen (see packages/mui-ts/FROZEN.md): the Rust DSL is
# the source of truth and the TS compiler is no longer part of the gate.
cargo check -p mui-demo --examples --locked --offline
