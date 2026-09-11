#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo fmt --all -- --check
cargo test --workspace --offline
cargo clippy --workspace --all-targets --offline -- -D warnings
# mui-preview is a native dev host (eframe::run_native does not exist on wasm).
# The wasm claim is about the library crates; drop the --exclude once the
# preview grows a WebRunner entry point and is served with trunk.
cargo check --workspace --exclude mui-preview --target wasm32-unknown-unknown --offline

pushd packages/mui-ts >/dev/null
npm ci --silent
rm -rf dist
npx --no-install tsc -p tsconfig.json
node dist/src/compiler.js dist/examples/pill.js /tmp/mui-generated.rs
popd >/dev/null
rustfmt --edition 2021 /tmp/mui-generated.rs

diff -u crates/mui-demo/src/generated.rs /tmp/mui-generated.rs
cargo run -p mui-demo --offline
