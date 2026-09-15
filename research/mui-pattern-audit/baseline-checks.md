# Audit baseline checks

All completed successfully before targeted finding verification:

- `cargo test --workspace --locked` (including doctests; 3 ignored doctests).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- `cargo fmt --all -- --check`.
- `cargo check --workspace --exclude mui-preview --target wasm32-unknown-unknown --offline --locked`.
- `packages/mui-ts/node_modules/.bin/tsc -p tsconfig.json --noEmit` from the TS package.
- TypeScript emitted into `/tmp`, example generated into `/tmp`, formatted with Rust 2021, and compared equal to `crates/mui-demo/src/generated.rs`.
- `cargo run -p mui-demo --offline --locked`.

These gates do not exercise a live GUI or prove the behavior of all malformed inputs.

Additional checks completed:

- `cargo test -p mui --features egui --locked --offline` (topic reviewer).
- `cargo clippy -p mui --features egui --all-targets --offline --locked -- -D warnings`.
- `cargo check -p mui --features egui --target wasm32-unknown-unknown --offline --locked`.
- Standalone public-API reproductions: rectangle endpoint overflow, interaction constructor difference, invalid drag thresholds, negative palette acceptance, no-op surface modifier, and final-position event replay model.
- Text non-finite metrics/path check with local Adwaita Sans; f32 tessellation precision demonstration.
- Fresh TS generation: control scene compiles; missing palette, control-character escape, and exponent-suffix cases fail as expected.
- SHA-256 comparison of 53 tracked application/config files matched the pre-review snapshot. Existing user modifications remain unchanged.

The expected-failure reproductions intentionally confirm current defects. They are not ordinary success-path regression tests until their assertions are inverted after a fix.
