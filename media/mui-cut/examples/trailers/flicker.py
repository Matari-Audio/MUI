"""Temporal flicker gate: no frame-to-frame luma jump well above the local
motion baseline, except on the cuts.

    python3 flicker.py OUT.mp4 [--cuts 120,240] [--fps 60]

Exits 1 and lists the spikes if any frame fails."""

import re
import statistics
import subprocess
import sys

video = sys.argv[1]
arg = lambda k, d: sys.argv[sys.argv.index(k) + 1] if k in sys.argv else d
cuts = {int(c) for c in arg("--cuts", "120,240").split(",") if c}
out = subprocess.run(
    ["ffmpeg", "-v", "error", "-i", video, "-vf",
     "format=yuv420p,tblend=all_mode=difference,signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=-",
     "-f", "null", "-"],
    capture_output=True, text=True, check=True,
).stdout
# d[i]: the change from frame i to frame i + 1.
d = [float(x) for x in re.findall(r"YAVG=([\d.]+)", out)]
bad = []
for i, x in enumerate(d):
    near = d[max(0, i - 6):i] + d[i + 1:i + 7]
    base = statistics.median(near) if near else 0.0
    # A jump well over the motion around it: a state or a shadow flipping.
    if x > 1.4 * base + 0.4 and i + 1 not in cuts:
        bad.append((i + 1, round(x, 2), round(base, 2)))
print(f"{video}: {len(d)} diffs, max {max(d):.2f}, {len(bad)} spikes")
for f, x, b in bad:
    print(f"  frame {f}: {x} against {b}")
sys.exit(1 if bad else 0)
