#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -p mui-playground --release --target wasm32-unknown-unknown --locked
wasm-bindgen --target web --out-dir playground/pkg target/wasm32-unknown-unknown/release/mui_playground.wasm
