#!/usr/bin/env bash
# Build the web editor's engine: mui-cut's library for wasm32, bound for the
# browser. Needs wasm-bindgen-cli 0.2.128 (`cargo install wasm-bindgen-cli
# --version 0.2.128`). Output: media/mui-cut/web/pkg/, served by `mui-cut serve`.
# SIMD128 (every current browser has it): vello_cpu, the no-GPU viewport,
# picks its SIMD kernels at compile time on wasm. MUI_CUT_SIMD=0 builds
# without, for comparison.
set -euo pipefail
cd "$(dirname "$0")/../.."
if [ "${MUI_CUT_SIMD:-1}" != 0 ]; then
  export CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="-C target-feature=+simd128"
fi
cargo build --manifest-path Cargo.toml -p mui-cut --lib --release --target wasm32-unknown-unknown
TARGET_DIR=$(cargo metadata --manifest-path Cargo.toml --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
wasm-bindgen "$TARGET_DIR/wasm32-unknown-unknown/release/mui_cut.wasm" --target web --out-dir mui-cut/web/pkg
