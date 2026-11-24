#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# One gate. CI splits media-test into disjoint CUT and native package groups;
# local media-test/all still run the whole media workspace.
# `pr` is the merge gate (local and PR CI): fmt, then lint and test only the
# packages the branch changed and their dependents. The full gate runs on
# main after merge and nightly (.github/workflows/verify.yml).
section="${1:-all}"
case "$section" in
  pr | root-lint | root-test | media-lint | media-test | media-cut-test | media-native-test | all) ;;
  *) echo "usage: $0 [pr|root-lint|root-test|media-lint|media-test|media-cut-test|media-native-test|all]" >&2; exit 2 ;;
esac
on() { [ "$section" = all ] || [ "$section" = "$1" ]; }

if [ "$section" = pr ]; then
base="${VERIFY_BASE:-origin/main}"
cargo fmt --all -- --check
read -r -a scope <<< "$(python3 tools/ci/changed_packages.py "$base")"
if [ "${#scope[@]}" -gt 0 ]; then
cargo clippy "${scope[@]}" --all-features --all-targets --locked --offline -- -D warnings
cargo test "${scope[@]}" --all-features --locked --offline
fi
# A workflow that does not parse fails on every push without running a job.
if ! git diff --quiet "$base"...HEAD -- .github tools/ci; then
# actionlint resolves reusable `./.github/workflows/...` paths against the
# enclosing `.git` root; isolate `.github` when running inside a monorepo.
if [ "$(git rev-parse --show-toplevel 2>/dev/null || true)" != "$ROOT" ]; then
  al_tmp="$(mktemp -d)"
  mkdir -p "$al_tmp/.git"
  ln -s "$ROOT/.github" "$al_tmp/.github"
  al_bin="${ACTIONLINT:-actionlint}"
  # Resolve before leaving ROOT: a bare name via PATH (which may hold relative
  # entries), then any relative result against ROOT, the current directory.
  if [[ $al_bin != */* ]]; then al_bin="$(command -v "$al_bin" || printf %s "$al_bin")"; fi
  if [[ $al_bin == */* && $al_bin != /* ]]; then al_bin="$ROOT/$al_bin"; fi
  al_rc=0
  (cd "$al_tmp" && "$al_bin" -shellcheck= .github/workflows/*.yml) || al_rc=$?
  rm -rf "$al_tmp"
  if [ "$al_rc" -ne 0 ]; then exit "$al_rc"; fi
else
  "${ACTIONLINT:-actionlint}" -shellcheck= .github/workflows/*.yml
fi
python3 -m unittest discover -s tools/ci
fi
if ! git diff --quiet "$base"...HEAD -- media; then
cargo fmt --manifest-path media/Cargo.toml --all -- --check
cargo clippy --manifest-path media/Cargo.toml --workspace --all-features --all-targets --locked --offline -- -D warnings
fi
# The baseview fork is its own package, outside the root workspace's scope.
if ! git diff --quiet "$base"...HEAD -- vendor/moose-baseview; then
cargo clippy --manifest-path vendor/moose-baseview/Cargo.toml --all-features --all-targets --locked --offline -- -D warnings
fi
exit 0
fi

if on root-lint; then
python3 tools/test_verify.py
python3 -m unittest discover -s tools/ci
cargo fmt --all -- --check
fi
# Every workspace feature is additive (mui-vello's cpu/gpu-quad backends and
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
cargo clippy --manifest-path vendor/moose-baseview/Cargo.toml --all-features --all-targets --locked --offline -- -D warnings
# --all-features hides a cfg that only breaks with a feature off: mui-vello's
# backends each compile alone and with none.
cargo clippy -p mui-vello --no-default-features --all-targets --locked --offline -- -D warnings
cargo clippy -p mui-vello --no-default-features --features cpu --all-targets --locked --offline -- -D warnings
cargo clippy -p mui-vello --no-default-features --features gpu-quad --all-targets --locked --offline -- -D warnings
# mui-preview is a native dev host: it owns a moose-baseview window and a wgpu
# surface, neither of which this gate can build for wasm. The wasm claim is
# about the library crates; drop the --exclude once the preview grows a
# `spawn_app` entry point and is served from a canvas.
# mui-baseview is a native window (moose-baseview + a wgpu surface): no wasm either.
cargo check --workspace --all-features --exclude mui-preview --exclude mui-baseview --target wasm32-unknown-unknown --locked --offline
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
