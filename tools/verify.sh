#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# One gate. CI splits media-test into disjoint CUT and native package groups;
# local media-test/all still run the whole media workspace.
section="${1:-all}"
case "$section" in
  root-lint | root-test | media-lint | media-test | media-cut-test | media-native-test | all) ;;
  *) echo "usage: $0 [root-lint|root-test|media-lint|media-test|media-cut-test|media-native-test|all]" >&2; exit 2 ;;
esac
on() { [ "$section" = all ] || [ "$section" = "$1" ]; }

if on root-lint; then
python3 tools/test_verify.py
python3 -m unittest discover -s tools/ci
cargo fmt --all -- --check
fi
# Every workspace feature is additive (mui-vello's cpu/gpu-effects backends and
# their facade forwards), so one all-features surface covers test, lint and wasm.
if on root-test; then
cargo test --workspace --all-features --locked --offline
# Standalone patched native packages are excluded from the root workspace.
cargo test --manifest-path vendor/moose-baseview/Cargo.toml --lib --locked --offline
if [ "$(uname -s)" = Linux ]; then
cargo test --manifest-path vendor/xim-rs/Cargo.toml -p zed-xim --lib --features x11rb-client,x11rb-xcb --locked --offline
fi
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
# Commit both lockfiles when root dependencies change. CI must reject a stale
# media lock rather than silently resolve a different graph while checking it.
# ---------------------------------------------------------------------------
if on media-lint; then
cargo fmt --manifest-path media/Cargo.toml -p mui-stage -p mui-stage-rt -p mui-reel -p mui-motion-bridge -p mui-cut -- --check
fi
if on media-test || on media-cut-test || on media-native-test; then
case "$section" in
  media-cut-test) media_packages=(-p mui-cut) ;;
  media-native-test) media_packages=(-p mui-stage -p mui-stage-rt -p mui-reel -p mui-motion-bridge) ;;
  *) media_packages=(--workspace) ;;
esac
cargo test --manifest-path media/Cargo.toml "${media_packages[@]}" --all-features --locked --offline
fi
if on media-lint; then
cargo clippy --manifest-path media/Cargo.toml --workspace --all-features --all-targets --locked --offline -- -D warnings
cargo check --manifest-path media/Cargo.toml -p mui-stage --no-default-features --locked --offline
cargo clippy --manifest-path media/Cargo.toml -p mui-cut --lib --target wasm32-unknown-unknown --locked --offline -- -D warnings
# The plugin rev-pin guard (media/tools/mui-sync).
python3 -m unittest discover -s media/tools/mui-sync
fi
