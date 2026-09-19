#!/usr/bin/env python3
"""Summarize raw gpu_matrix CSVs; never convert submission time into FPS.

Quantiles use nearest rank. Missing GPU samples stay missing. Outputs identify
an isolated headless fixture, not a presented editor or real-time audio test.
"""
from __future__ import annotations
import argparse
import csv
import json
import math
from pathlib import Path

COUNTERS = ('uniform_bytes', 'effect_draws', 'effect_pixels', 'texture_allocations',
            'scene_encodes', 'target_allocations', 'resident_effect_bytes')

def quantile(values: list[float], probability: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    return ordered[max(0, math.ceil(probability * len(ordered)) - 1)]

def timings(values: list[float], total: int) -> dict:
    return {'samples': len(values), 'missing': total - len(values),
            'p50_ms': quantile(values, .50), 'p95_ms': quantile(values, .95),
            'p99_ms': quantile(values, .99), 'max_ms': max(values, default=None)}

def summarize(path: Path) -> dict:
    cpu: list[float] = []
    gpu: list[float] = []
    frames: set[int] = set()
    totals = dict.fromkeys(COUNTERS, 0)
    peak_resident = 0
    with path.open(newline='', encoding='utf8') as stream:
        reader = csv.DictReader(stream)
        required = {'frame', 'cpu_submit_ms', 'gpu_queue_interval_ms', *COUNTERS}
        if not required.issubset(reader.fieldnames or ()):
            raise ValueError(f'{path}: missing CSV columns')
        for row in reader:
            frame = int(row['frame'])
            if frame < 0 or frame in frames:
                raise ValueError(f'{path}: duplicate/negative frame {frame}')
            frames.add(frame)
            for name, output in [('cpu_submit_ms', cpu), ('gpu_queue_interval_ms', gpu)]:
                raw = row[name]
                if not raw and name == 'gpu_queue_interval_ms':
                    continue
                value = float(raw)
                if not math.isfinite(value) or value < 0:
                    raise ValueError(f'{path}: invalid {name}')
                output.append(value)
            for name in COUNTERS:
                count = int(row[name])
                if count < 0:
                    raise ValueError(f'{path}: negative {name}')
                totals[name] += count
                if name == 'resident_effect_bytes':
                    peak_resident = max(peak_resident, count)
    if not frames:
        raise ValueError(f'{path}: no samples')
    del totals['resident_effect_bytes']
    return {'file': str(path), 'scope': 'isolated headless effect + composition; not presented FPS',
            'cpu_prepare_submit': timings(cpu, len(frames)),
            'gpu_queue_interval': timings(gpu, len(frames)),
            'totals': totals, 'peak_logical_effect_bytes': peak_resident}

def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('csv', nargs='+', type=Path)
    parser.add_argument('--out', type=Path)
    args = parser.parse_args()
    result = json.dumps([summarize(p) for p in args.csv], indent=2, allow_nan=False) + '\n'
    if args.out:
        args.out.write_text(result, encoding='utf8')
    else:
        print(result, end='')

if __name__ == '__main__':
    main()
