"""GPU regression check: analytic sRGB/Oklab ramps plus thin-circle diagnostics.

Run after MUI_LAB_COMPONENTS=1 captures. Requires numpy and Pillow.
Cairo does not implement Oklab interpolation, so use independent matrix math here.
"""
import json
import sys
from pathlib import Path
import numpy as np
from PIL import Image


def linear(rgb):
    return np.where(rgb <= 0.04045, rgb / 12.92, ((rgb + 0.055) / 1.055) ** 2.4)


def srgb(rgb):
    return np.where(rgb <= 0.0031308, rgb * 12.92, 1.055 * np.maximum(rgb, 0) ** (1 / 2.4) - 0.055)


M1 = np.array([[0.4122214708, 0.5363325363, 0.0514459929],
               [0.2119034982, 0.6806995451, 0.1073969566],
               [0.0883024619, 0.2817188376, 0.6299787005]])
M2 = np.array([[0.2104542553, 0.7936177850, -0.0040720468],
               [1.9779984951, -2.4285922050, 0.4505937099],
               [0.0259040371, 0.7827717662, -0.8086757660]])


def ramp(t, oklab):
    a, b = np.array([64, 128, 192]) / 255, np.array([192, 64, 128]) / 255
    if oklab:
        a, b = (np.cbrt(linear(c) @ M1.T) @ M2.T for c in [a, b])
    mixed = a + t[:, None] * (b - a)
    if oklab:
        mixed = srgb((mixed @ np.linalg.inv(M2).T) ** 3 @ np.linalg.inv(M1).T)
    return mixed * 255


def check(out):
    results = {}
    files = sorted(out.glob('gpui*.png'))
    assert files, 'No GPUI captures found'
    failures = []
    for file in files:
        a = np.asarray(Image.open(file).convert('RGB'), dtype=float)
        scale = a.shape[1] / 768
        gradients, circles = {}, {}
        for oklab in [False, True]:
            for native in [False, True]:
                x0 = (392 if native else 16) * scale
                xs = np.arange(int(np.ceil(x0 + 3)), int(np.floor(x0 + 350 * scale - 3)))
                y = int((240 if oklab else 490) * scale)
                error = np.abs(a[y, xs] - ramp((xs + 0.5 - x0) / (350 * scale), oklab))
                key = f'{"oklab" if oklab else "srgb"}-{"quad" if native else "path"}'
                gradients[key] = {'mae': float(error.mean()), 'max': float(error.max())}
                if error.max() > 2:
                    failures.append(f'{file.name} {key}: max error {error.max():.3f} > 2/255')
        for i in range(4):
            for row, width in enumerate([0.25, 0.5, 1, 1.5]):
                for native in [False, True]:
                    theta = np.linspace(0, 2 * np.pi, 4096, endpoint=False)
                    x = (45 + i * 180.25 + (65 if native else 0) + 20 * np.cos(theta)) * scale - 0.5
                    y = (280 + row * 52 + i * 0.25 + 20 * np.sin(theta)) * scale - 0.5
                    xi, yi = np.floor(x).astype(int), np.floor(y).astype(int)
                    fx, fy = x - xi, y - yi
                    l = (a[:, :, 0] - 20) / 210
                    coverage = (l[yi, xi] * (1-fx) * (1-fy) + l[yi, xi+1] * fx * (1-fy)
                                + l[yi+1, xi] * (1-fx) * fy + l[yi+1, xi+1] * fx * fy)
                    circles[f'{i}-{width}-{"quad" if native else "path"}'] = {
                        'min_coverage': float(coverage.min()),
                        'weak_fraction': float((coverage < 0.025).mean()),
                    }
        results[file.name] = {'gradients': gradients, 'circles': circles}
    (out / 'gradient-check.json').write_text(json.dumps(results, indent=2) + '\n')
    assert not failures, '\n'.join(failures)
    print(f'{len(files)} GPU captures passed sRGB/Oklab path/quad gradient checks (max 2/255).')
    return results


if __name__ == '__main__':
    check(Path(sys.argv[1]))
