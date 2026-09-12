#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
python3 tools/prepare.py
cargo build --release --locked
mkdir -p output
./target/release/mui-render-lab geometry 1 30 output
for scale in 1 1.5 2; do
  for backend in gpui gpui-tight vello-area vello-msaa8 vello; do
    ./target/release/mui-render-lab "$backend" "$scale" 60 output
  done
done
./target/release/mui-render-lab embedding 1 30 output
for scale in 1 2; do
 for backend in vello-area-present vello-present; do
  ./target/release/mui-render-lab "$backend" "$scale" 60 output
 done
done
for backend in gpui gpui-tight vello-area-present vello-present; do
 ./target/release/mui-render-lab "$backend" 1 60 output/stress 8
done
for run in 2 3; do
 for backend in gpui vello-area-present vello-present; do
  ./target/release/mui-render-lab "$backend" 1 60 "output/repeat-$run"
 done
done
./target/release/mui-render-lab glass output/gpui-1.png output/glass-gpui.png
./target/release/mui-render-lab glass output/vello-1.png output/glass-vello.png
python3 tools/quality.py output
