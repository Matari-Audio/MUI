#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo fmt --all -- --check
cargo test --workspace --offline
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo check --workspace --target wasm32-unknown-unknown --offline

pushd packages/mui-ts >/dev/null
rm -rf dist
tsc -p tsconfig.json
node dist/src/compiler.js dist/examples/pill.js /tmp/mui-generated.rs
popd >/dev/null
rustfmt --edition 2021 /tmp/mui-generated.rs

diff -u crates/mui-demo/src/generated.rs /tmp/mui-generated.rs
cargo run -p mui-demo --offline
