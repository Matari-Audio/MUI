#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo fmt --all -- --check
cargo test --workspace --offline
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo check --workspace --target wasm32-unknown-unknown --offline

GENERATED="$(mktemp --suffix=.rs)"
trap 'rm -f "$GENERATED"' EXIT
pushd packages/mui-ts >/dev/null
rm -rf dist
npm run build
npm test
node dist/src/compiler.js dist/examples/pill.js "$GENERATED"
popd >/dev/null
rustfmt --edition 2021 "$GENERATED"

diff -u crates/mui-demo/src/generated.rs "$GENERATED"
node packages/mui-ts/dist/src/compiler.js packages/mui-ts/dist/examples/compiler-contract.js "$GENERATED"
rustfmt --edition 2021 "$GENERATED"
diff -u crates/mui-demo/src/compiler_contract.rs "$GENERATED"
cargo run -p mui-demo --offline
cargo run -p mui-demo --example chrome_tabs --offline > "$GENERATED"
diff -u docs/chrome-tabs.svg "$GENERATED"

node packages/mui-ts/dist/src/compiler.js packages/mui-ts/dist/examples/items.js "$GENERATED"
rustfmt --edition 2021 "$GENERATED"
diff -u crates/mui-demo/src/generated_items.rs "$GENERATED"
cargo run -p mui-demo --example items --offline > "$GENERATED"
diff -u docs/items.svg "$GENERATED"

cargo run -p mui-demo --example items --offline -- --hover > "$GENERATED"
diff -u docs/items-hover.svg "$GENERATED"
