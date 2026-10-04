#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# One gate, four sections CI runs as parallel jobs: `./tools/verify.sh
# [root-lint|root-test|media-lint|media-test|all]`, all by default.
section="${1:-all}"
case "$section" in
  root-lint | root-test | media-lint | media-test | all) ;;
  *) echo "usage: $0 [root-lint|root-test|media-lint|media-test|all]" >&2; exit 2 ;;
esac
on() { [ "$section" = all ] || [ "$section" = "$1" ]; }

if on root-lint; then
cargo fmt --all -- --check
fi
# Every workspace feature is additive (mui-vello's cpu/gpu-effects backends and
# their facade forwards), so one all-features surface covers test, lint and wasm.
if on root-test; then
cargo test --workspace --all-features --locked --offline
fi
if on root-lint; then
cargo clippy --workspace --all-features --all-targets --locked --offline -- -D warnings
# --all-features hides a cfg that only breaks with a feature off: mui-vello's
# backends each compile alone and with none.
cargo clippy -p mui-vello --no-default-features --all-targets --locked --offline -- -D warnings
cargo clippy -p mui-vello --no-default-features --features cpu --all-targets --locked --offline -- -D warnings
cargo clippy -p mui-vello --no-default-features --features gpu-effects --all-targets --locked --offline -- -D warnings
# mui-preview is a native dev host: it owns a winit event loop and a wgpu
# surface, neither of which this gate can build for wasm. The wasm claim is
# about the library crates; drop the --exclude once the preview grows a
# `spawn_app` entry point and is served from a canvas.
# mui-gain-plugin is a CLAP/VST3 cdylib: truce-vst3 compiles a C++ shim and
# truce-clap wants a native parent window, so it has no wasm build at all.
# mui-baseview is a native window (baseview + a wgpu surface): no wasm either.
# mui-truce itself stays in: its window/GPU half is cfg'd out on wasm32.
cargo check --workspace --all-features --exclude mui-preview --exclude mui-gain-plugin --exclude mui-baseview --exclude mui-winit --target wasm32-unknown-unknown --locked --offline
fi

# ---------------------------------------------------------------------------
# media/: its own workspace (mui-stage, mui-reel, mui-motion-bridge, mui-cut), kept out
# of the root so plugin builds never compile it. Same gate, its own lockfile.
# mui-stage's default `backends` feature is the only feature; --all-features
# keeps it on. Only mui-cut's library is checked for wasm (its web editor);
# the rest own a native device or ffmpeg.
# ponytail: no --locked here. media/Cargo.lock also pins the root crates'
# dependencies, so --locked would fail on every root dependency change; the
# gate refreshes it instead and git status shows the diff.
# ---------------------------------------------------------------------------
if on media-lint; then
cargo fmt --manifest-path media/Cargo.toml -p mui-stage -p mui-reel -p mui-motion-bridge -p mui-cut -- --check
fi
if on media-test; then
cargo test --manifest-path media/Cargo.toml --workspace --all-features --offline
fi
if on media-lint; then
cargo clippy --manifest-path media/Cargo.toml --workspace --all-features --all-targets --offline -- -D warnings
cargo check --manifest-path media/Cargo.toml -p mui-stage --no-default-features --offline
cargo clippy --manifest-path media/Cargo.toml -p mui-cut --lib --target wasm32-unknown-unknown --offline -- -D warnings
# The plugin rev-pin guard (media/tools/mui-sync).
python3 -m unittest discover -s media/tools/mui-sync
fi
