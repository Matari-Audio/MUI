#!/usr/bin/env bash
# Native validation: fail rather than silently skipping absent GPU/toolchain.
set -euo pipefail
cd "$(dirname "$0")/../.."
node --test tools/incremental-sharp/boundary.test.mjs
rustc --version --verbose
cargo fmt --all -- --check
cargo test --workspace --all-features --locked
cargo test -p mui-vello --features gpu-effects --locked effects::tests::naga_validates_the_shader_and_the_uniform_abi -- --exact
cargo clippy --workspace --all-features --all-targets --locked -- -D warnings
cargo check --workspace --all-features --exclude mui-preview --target wasm32-unknown-unknown --locked
cargo run --locked --profile perf -p mui-vello --features gpu-effects --example gpu_contract -- "${1:-native-gpu-contract}"
