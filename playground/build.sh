#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -p mui-playground --release --target wasm32-unknown-unknown --locked
TARGET_DIR=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
wasm-bindgen "$TARGET_DIR/wasm32-unknown-unknown/release/mui_playground.wasm" --target web --out-dir playground/pkg
