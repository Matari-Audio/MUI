#!/usr/bin/env python3
"""Compare paired native screenshots before using their timing results.

Requires Pillow. This is a strict diagnostic, not an automatic golden-image
approver. Backend AA/colour differences that exceed the explicit threshold fail.
"""
from __future__ import annotations
import argparse
import json
from pathlib import Path
from PIL import Image

def compare(a: Path, b: Path, threshold: int = 3) -> dict:
    if not 0 <= threshold <= 255:
        raise ValueError('channel threshold must be in 0..255')
    with Image.open(a) as left, Image.open(b) as right:
        if left.size != right.size:
            raise ValueError(f'image sizes differ: {left.size}, {right.size}')
        size = left.size
        x, y = left.convert('RGBA').tobytes(), right.convert('RGBA').tobytes()
    max_error = 0
    total = 0
    bad_pixels = 0
    for offset in range(0, len(x), 4):
        errors = [abs(x[offset + j] - y[offset + j]) for j in range(4)]
        worst = max(errors)
        max_error = max(max_error, worst)
        total += sum(errors)
        bad_pixels += int(worst > threshold)
    pixels = size[0] * size[1]
    return {'left': str(a), 'right': str(b), 'width': size[0], 'height': size[1],
            'max_channel_error_255': max_error, 'mean_channel_error_255': total / len(x),
            'threshold_255': threshold, 'pixels_exceeding_threshold': bad_pixels,
            'fraction_exceeding_threshold': bad_pixels / pixels, 'passed': bad_pixels == 0}

def main() -> None:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('left', type=Path)
    p.add_argument('right', type=Path)
    p.add_argument('--max-channel-error', type=int, default=3)
    p.add_argument('--out', type=Path)
    a = p.parse_args()
    result = compare(a.left, a.right, a.max_channel_error)
    text = json.dumps(result, indent=2) + '\n'
    if a.out:
        a.out.write_text(text, encoding='utf8')
    print(text, end='')
    if not result['passed']:
        raise SystemExit(1)

if __name__ == '__main__':
    main()
