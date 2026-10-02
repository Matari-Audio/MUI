#!/usr/bin/env python3
"""Writes buffr.cut.json, buffr-music.wav and the atmosphere plates in
media/: the BUFFR trailer, 20 s.

BUFFR is a recorder, so the film plays it the music: the soundtrack goes
into its input (`--input`, its adapter's), the motif's notes into its MIDI,
and every frame of its UI is the real editor recording them. Music and
motion share one beat grid (buffr_music.py: 120 BPM, a bar is 2 s).

    python3 buffr.gen.py          # then: mui-cut check buffr.cut.json

It needs BUFFR beside this repository (`../drop-recorder`), on a branch
with the `cut` feature and `mui-cut-adapter.rs` (see the PR).
"""
import hashlib
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image

import buffr_music as music

HERE = Path(__file__).parent
# Its captures key on the source's args: the music's hash makes a new mix a
# new recording.
music.write(HERE / "buffr-music.wav")
MUSIC_ID = hashlib.sha1((HERE / "buffr-music.wav").read_bytes()).hexdigest()[:12]
FPS = float(sys.argv[sys.argv.index("--fps") + 1]) if "--fps" in sys.argv else 60.0
DUR = music.DUR
DROP, HIT, STOP = music.DROP, music.HIT, music.STOP
E = 1.0 / FPS  # a shot's last key: the frame before its cut

BRAND = {"red": "#ff1547", "yellow": "#ffea00", "green": "#05ff74", "cyan": "#00deff"}
RAINBOW = list(BRAND.values())


def key(t, v, interp="bezier", out=None, in_=None):
    k = {"t": round(t, 4), "v": v, "interp": interp}
    if out:
        k["out"] = out
    if in_:
        k["in"] = in_
    return k


def keys(*pairs, interp="bezier"):
    return [key(t, v, interp) for t, v in pairs]


def hold(*pairs):
    return keys(*pairs, interp="hold")


def track(points, cuts=()):
    """Keys from (t, v); the point before a cut holds, so the cut is clean."""
    out = []
    for i, (t, v) in enumerate(points):
        nxt = points[i + 1][0] if i + 1 < len(points) else None
        cut = nxt is not None and any(abs(nxt - c) < 1e-9 for c in cuts)
        out.append(key(t, v, "hold" if cut else "bezier"))
    return out


# ---------------------------------------------------------------- plates
def rgb(h):
    return np.array([int(h[i:i + 2], 16) for i in (1, 3, 5)], np.float32) / 255


def paint():
    """The atmosphere, painted once: a haze wall with BUFFR's colours
    fanning up through it (behind everything, in 3D), light shafts over
    the shot, and two sheets of dust that drift through them."""
    rng = np.random.default_rng(7)
    w, h = 1920, 1080
    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    u, v = x / w - 0.5, y / h

    # Haze: a dark studio, its floor glow warm, the brand's colours as
    # broad beams fanning up from below the horizon.
    img = np.zeros((h, w, 3), np.float32)
    img += rgb("#0c0a12") * (0.6 + 0.4 * v[..., None])
    for i, c in enumerate(RAINBOW):
        angle = (i - 1.5) * 0.17
        d = u - angle * (1.05 - v)  # beams lean out as they rise
        beam = np.exp(-(d / (0.05 + 0.10 * (1 - v))) ** 2) * (0.25 + 0.75 * v) ** 1.6
        img += rgb(c) * beam[..., None] * 0.30
    glow = np.exp(-((u / 0.42) ** 2) - (((v - 1.0) / 0.35) ** 2))
    img += rgb("#ffb27a") * glow[..., None] * 0.10
    lo = np.kron(rng.random((h // 60 + 1, w // 60 + 1)), np.ones((60, 60)))[:h, :w]
    img *= (0.88 + 0.24 * lo[..., None]).astype(np.float32)
    save(img, None, "haze.png")

    # Shafts: soft diagonal light from the top corners, brand-tinted, alpha
    # only where the light is.
    a = np.zeros((h, w), np.float32)
    col = np.zeros((h, w, 3), np.float32)
    for i, (x0, c, k) in enumerate([(0.12, "cyan", 0.55), (0.30, "red", -0.35), (0.78, "yellow", -0.55), (0.92, "green", 0.4)]):
        d = (x / w - x0) - k * (y / h)
        s = np.exp(-(d / 0.045) ** 2) * np.clip(1.1 - y / h, 0, 1) ** 2
        a += s * 0.55
        col += rgb(BRAND[c]) * s[..., None]
    col = col / np.maximum(a[..., None] / 0.55, 1e-4)
    save(np.clip(col * 0.6 + 0.4, 0, 1), np.clip(a, 0, 1), "shafts.png")

    # Dust: soft motes of every size, a few out of focus (big and faint).
    for name, n, big in [("dust-near.png", 70, 1), ("dust-far.png", 260, 0)]:
        a = np.zeros((h, w), np.float32)
        for _ in range(n):
            cx, cy = rng.random() * w, rng.random() * h
            r = (rng.random() ** 3) * (14 if big else 3.2) + (2.5 if big else 0.8)
            peak = (0.18 if big else 0.75) * (0.4 + 0.6 * rng.random())
            x0, x1 = int(max(cx - 3 * r, 0)), int(min(cx + 3 * r, w))
            y0, y1 = int(max(cy - 3 * r, 0)), int(min(cy + 3 * r, h))
            yy, xx = np.mgrid[y0:y1, x0:x1]
            q = ((xx - cx) ** 2 + (yy - cy) ** 2) / r ** 2
            disc = np.clip(1.6 - q, 0, 1) if big else np.exp(-q * 2)  # bokeh: flat discs
            a[y0:y1, x0:x1] = np.maximum(a[y0:y1, x0:x1], disc * peak)
        save(np.ones((h, w, 3), np.float32) * rgb("#fff3e6"), a, name)


def save(color, alpha, name):
    out = HERE / "media" / name
    data = np.clip(color, 0, 1) * 255
    if alpha is not None:
        data = np.dstack([data, np.clip(alpha, 0, 1) * 255])
    Image.fromarray(data.round().astype(np.uint8)).save(out, optimize=True)


# ---------------------------------------------------------------- BUFFR
# The editor laid out at 1280x720 UI px; UI (u, v) to project px:
VIEW = (1280, 720)
BX, BY, BS = 960.0, 520.0, 1.2


def ui(u, v):
    return BX + (u - VIEW[0] / 2) * BS, BY + (v - VIEW[1] / 2) * BS


FLOOR = BY + VIEW[1] / 2 * BS + 90  # a black mirror under it

# What it records: the music, and the motif (then the drop's pluck) as MIDI.
NOTES = [
    {"t": round(t, 4), "dur": round(d, 4), "pitch": p, "vel": v}
    for t, d, p, v in music.intro_notes() + music.arp_notes() + music.outro_notes()
]

SELECT = [
    "hdr.logo", "hdr.status", "tb.left", "tb.center", "tb.right",
    "tl.audio", "tl.audio.plot", "tl.audio.scale", "tl.midi", "tl.midi.plot",
]

# The exploded view: the UI comes apart in depth, a stack of plates, the
# header nearest. `DEPTH` is how far apart (0 shut, 1 open); each part's
# z is its share of it (negative is towards the camera).
DEPTH = [
    key(0, 0.0), key(STOP, 0.0, out=[0.1, 0.0]),
    key(5.3, 0.85, in_=[-0.5, 0.0]), key(7.35, 1.0, out=[0.25, 0.0]),
    key(DROP, 0.0, in_=[-0.06, 0.4]),
    key(12.0, 0.0), key(12.7, 0.8, in_=[-0.3, 0.0]), key(13.6, 0.9), key(14.0, 0.0, in_=[-0.1, 0.3]),
    key(HIT, 0.0), key(HIT + 0.1, 0.35), key(HIT + 0.9, 0.0),
]
STACK = {
    "hdr.logo": -420, "hdr.status": -380, "tb.left": -300, "tb.center": -280, "tb.right": -300,
    "tl.audio": -160, "tl.audio/tl.audio.plot": -80, "tl.audio/tl.audio.scale": -40,
    "tl.midi": 40, "tl.midi/tl.midi.plot": -80,  # a child's z is on its parent's
}


def depth(z):
    """`DEPTH` scaled by `z`, its handles' value offsets too."""
    def scaled(k):
        k = dict(k, v=round(k["v"] * z, 3))
        for h in ("in", "out"):
            if h in k:
                k[h] = [k[h][0], round(k[h][1] * z, 3)]
        return k
    return [scaled(k) for k in DEPTH]


# While it is apart each plate wears a thin lit edge, so the stack reads.
EDGE = keys((0, 0.0), (STOP, 0.0), (5.0, 0.35), (7.6, 0.35), (DROP, 0.0),
            (12.0, 0.0), (12.5, 0.3), (13.8, 0.3), (14.05, 0.0))
BACKDROP = keys((0, 1.0), (STOP, 1.0), (5.0, 0.4), (7.6, 0.4), (DROP, 1.0),
                (12.0, 1.0), (12.5, 0.45), (13.8, 0.45), (14.05, 1.0))

# A hand drags a selection across the waveform in the last bar of the drop.
SEL = ((420, 250), (900, 250), 14.15, 14.75)

# The hit turns it to glass, printed (its dark clear, its light as ink),
# so the haze and the brand's colours come through it; then it goes.
GLASS = keys((0, 0.0), (HIT - 0.05, 0.0), (HIT + 0.5, 0.92))


def pointer():
    (x0, y), (x1, _), t0, t1 = SEL
    return {
        "pointer_x": [key(0, -1.0, "hold"), key(t0 - 0.1, float(x0)), key(t0, float(x0)), key(t1, float(x1), "hold"), key(t1 + 0.2, -1.0, "hold")],
        "pointer_y": [key(0, -1.0, "hold"), key(t0 - 0.1, float(y), "hold"), key(t1 + 0.2, -1.0, "hold")],
        "pointer_down": hold((0, 0), (t0, 1), (t1 + 0.05, 0)),
    }


def buffr():
    return {
        "id": "buffr",
        "name": "BUFFR",
        "kind": "plugin",
        "source": {"plugin": "../../../../../drop-recorder", "features": ["cut"],
                   "args": ["--input", "buffr-music.wav", "--music", MUSIC_ID]},
        "notes": NOTES,
        "volume": 0.0,  # its output is its input: the audio layer plays it
        "view_width": float(VIEW[0]),
        "view_height": float(VIEW[1]),
        "select": SELECT,
        "explode_levels": 2,
        "explode": depth(0.12),
        "backdrop": BACKDROP,
        "parts": {p: {"z": depth(z), "highlight": EDGE} for p, z in STACK.items()},
        **pointer(),
        "opacity": keys((0, 1.0), (HIT + 0.55, 1.0), (HIT + 1.3, 0.0)),
        "x": BX,
        "y": BY,
        "scale": BS,
        "extrude": 8.0,
        "edge": "#4a4f5c",
        "material": {"roughness": 0.3, "bevel": 4.0, "transmission": GLASS,
                     "print": 1.0, "ior": 1.5, "dispersion": 1.2, "thickness": 8.0},
    }


# ---------------------------------------------------------------- camera
CUTS = (DROP, 10.0, 12.0, 14.0, HIT)


def camera():
    # (t, target u, v, z, distance, rx, ry, fov, aperture); target z
    # follows the stack's middle while it is apart.
    shots = [
        # 1: low and close over the lanes as the idea is played, drifting
        # with the playhead as the notes land
        (0.0, 150, 520, 0, 980, 9, 28, 26, 22),
        (STOP - 0.3, 440, 505, 0, 860, 6, 15, 26, 20),
        # 2: the tape stop: it comes apart; the camera swings round to see
        # the plates in depth
        (STOP + 0.25, 560, 420, -60, 1500, 4, 6, 28, 12),
        (6.0, 640, 380, -180, 1750, -2, 34, 28, 10),
        # 3: the build: round the other way, accelerating into the drop
        (7.3, 640, 380, -200, 1650, -6, -30, 28, 10),
        (DROP - E, 640, 380, -60, 1900, -6, -20, 28, 8),
        # 4: the drop: the hero, welded
        (DROP, 640, 400, 0, 2000, -8, 18, 30, 5),
        (10.0 - E, 640, 400, 0, 1900, -6, 11, 30, 5),
        # 5: along the waveform it is recording
        (10.0, 520, 250, 0, 860, -4, -24, 26, 16),
        (12.0 - E, 820, 250, 0, 820, -3, -18, 26, 16),
        # 6: in depth: the plates apart, from the side
        (12.0, 640, 380, -140, 1900, -10, 38, 30, 9),
        (14.0 - E, 640, 380, -160, 1800, -7, 28, 30, 9),
        # 7: the hand selects the moment
        (14.0, 650, 280, 0, 900, -3, 6, 26, 12),
        (HIT - E, 700, 280, 0, 840, -3, 2, 26, 12),
        # 8: the hit: glass, a sweep, then it goes back into the dark
        (HIT, 640, 360, 0, 1750, -12, -26, 30, 6),
        (17.4, 640, 380, 0, 2500, -8, -8, 30, 4),
        (DUR, 640, 380, 0, 2700, -8, -4, 30, 3),
    ]
    col = lambda i: [(w[0], w[i]) for w in shots]
    xy = [(w[0], ui(w[1], w[2])) for w in shots]
    return {
        "id": "cam",
        "kind": "camera",
        "x": track([(t, round(p[0], 2)) for t, p in xy], CUTS),
        "y": track([(t, round(p[1], 2)) for t, p in xy], CUTS),
        "z": track(col(3), CUTS),
        "distance": track(col(4), CUTS),
        "rx": track(col(5), CUTS),
        "ry": track(col(6), CUTS),
        "fov": track(col(7), CUTS),
        "aperture": track(col(8), CUTS),
    }


# ---------------------------------------------------------------- light
def lights():
    """A warm key and a cool fill that never go out (the tape stop takes
    them down, not off), and BUFFR's four colours as pools of light round
    the stage that swell on the drop and the hit."""
    def pulse(base, low, peak):
        return [key(0, base * 0.7), key(1.0, base), key(STOP, base, out=[0.12, 0]),
                key(STOP + 0.5, low), key(6.5, low), key(7.9, base * 1.2),
                key(DROP, peak), key(DROP + 0.5, base), key(HIT, base),
                key(HIT + 0.06, peak), key(HIT + 0.9, base), key(19.2, base), key(DUR, base * 0.3)]

    def pool(id_, color, x, y, z):
        return {"id": id_, "kind": "light", "type": "point", "fill": BRAND[color],
                "x": x, "y": y, "z": z, "range": 1900.0, "softness": 3.0,
                "intensity": pulse(0.45, 0.35, 1.3)}

    return [
        {"id": "key", "kind": "light", "fill": "#fff1e2", "rx": 18.0,
         "ry": keys((0, -40.0), (STOP, -30.0), (DROP, -24.0), (DUR, -18.0)),
         "intensity": pulse(1.5, 0.45, 2.6), "softness": 3.0},
        {"id": "fill", "kind": "light", "type": "ambient", "fill": "#8fa0c4",
         "intensity": pulse(0.3, 0.2, 0.45)},
        pool("pool-red", "red", 120.0, 120.0, -500.0),
        pool("pool-yellow", "yellow", 960.0, -260.0, -200.0),
        pool("pool-green", "green", 260.0, 1180.0, -400.0),
        pool("pool-cyan", "cyan", 1800.0, 860.0, -500.0),
    ]


def atmosphere():
    """The haze wall far behind; over the shot, the shafts breathing with
    the music and two sheets of dust drifting at different speeds."""
    beat = [key(0, 0.18), key(STOP, 0.22), key(STOP + 0.4, 0.42), key(7.8, 0.5),
            key(DROP, 0.75), key(DROP + 0.6, 0.32), key(HIT, 0.32), key(HIT + 0.1, 0.7),
            key(HIT + 1.2, 0.2), key(DUR, 0.0)]
    return [
        {"id": "haze", "kind": "image", "width": 1920.0, "height": 1080.0, "path": "media/haze.png",
         "x": 960.0, "y": 420.0, "z": 2400.0, "scale": 3.4, "cast_shadows": False,
         "material": {"roughness": 1.0}},
        {"id": "shafts", "kind": "image", "width": 1920.0, "height": 1080.0, "path": "media/shafts.png", "overlay": True,
         "x": [key(0, 900.0), key(DUR, 1020.0)], "y": 540.0, "scale": 1.12, "opacity": beat},
        {"id": "dust-far", "kind": "image", "width": 1920.0, "height": 1080.0, "path": "media/dust-far.png", "overlay": True,
         "x": [key(0, 990.0, "linear"), key(DUR, 900.0, "linear")],
         "y": [key(0, 560.0, "linear"), key(DUR, 500.0, "linear")], "scale": 1.15,
         "opacity": keys((0, 0.0), (0.8, 0.5), (HIT + 1.5, 0.5), (DUR, 0.0))},
        {"id": "dust-near", "kind": "image", "width": 1920.0, "height": 1080.0, "path": "media/dust-near.png", "overlay": True,
         "x": [key(0, 1080.0, "linear"), key(DUR, 820.0, "linear")],
         "y": [key(0, 600.0, "linear"), key(DUR, 470.0, "linear")], "scale": 1.3,
         "opacity": keys((0, 0.0), (0.8, 0.6), (HIT + 1.5, 0.6), (DUR, 0.0)),
         "effects": [{"type": "blur", "radius": 3.0}]},
    ]


# ---------------------------------------------------------------- words
FONT = "Barlow"
FONT_REG = "BarlowRegular"
LX, LY = 150.0, 968.0  # the lower third: left, on a soft dark band


def caption(id_, text, t0, t1, size=50.0):
    """A lower-third line: BUFFR's four colours draw in as a rule, the
    words rise in crisp behind it, and both go together."""
    show = [key(0, 0.0, "hold"), key(t0, 0.0), key(t0 + 0.3, 1.0), key(t1 - 0.22, 1.0), key(t1, 0.0, "hold")]
    out = [{
        "id": id_, "kind": "text", "text": text, "font": FONT, "align": "left", "overlay": True,
        "x": LX, "y": [key(t0, LY + 12), key(t0 + 0.45, LY)],
        "font_size": size, "fill": "#f7f6f2", "tracking": 0.4, "opacity": show,
        "effects": [{"type": "blur", "radius": [key(t0, 6.0), key(t0 + 0.22, 0.0), key(t1 - 0.2, 0.0), key(t1, 4.0)]}],
    }]
    for i, c in enumerate(RAINBOW):
        t = t0 + 0.04 * i
        out.append({
            "id": f"{id_}-rule{i}", "kind": "rect", "overlay": True, "fill": c,
            "x": LX + 14 + i * 32, "y": LY - size * 0.95, "height": 4.0,
            "width": [key(t, 0.0), key(t + 0.3, 28.0)], "opacity": show,
        })
    return out


def words():
    lines = [
        ("you-played", "You played it.", 0.5, 2.0),
        ("best-idea", "The best idea of the night.", 2.1, STOP),
        ("gone", "Gone?", 4.55, 6.3),
        ("has-it", "BUFFR already has it.", DROP + 0.2, 10.0),
        ("always", "Always recording. Hours of it.", 10.1, 12.0),
        ("both", "Audio and MIDI, side by side.", 12.1, 14.0),
        ("drag", "Drag the moment anywhere.", 14.2, HIT),
    ]
    band = {"id": "band", "kind": "rect", "overlay": True, "fill": "#000000",
            "x": 960.0, "y": 1090.0, "width": 2600.0, "height": 330.0,
            "opacity": keys((0, 0.0), (0.4, 0.62), (HIT, 0.62), (HIT + 0.5, 0.0)),
            "effects": [{"type": "blur", "radius": 90.0}]}
    return [band] + [layer for line in lines for layer in caption(*line)]


def end_card():
    """The logo comes forward out of the dark, then the line and the url."""
    t0 = HIT + 0.9
    fade = lambda t: [key(0, 0.0, "hold"), key(t, 0.0), key(t + 0.5, 1.0), key(19.55, 1.0), key(DUR, 0.0)]
    return [
        {"id": "logo", "kind": "svg", "path": "media/buffr.svg", "overlay": True,
         "x": 960.0, "y": [key(t0, 468.0), key(t0 + 1.0, 452.0)],
         "scale": [key(t0, 0.112), key(DUR, 0.122)], "opacity": fade(t0),
         "effects": [{"type": "blur", "radius": [key(t0, 12.0), key(t0 + 0.45, 0.0)]}]},
        {"id": "never-lose", "kind": "text", "text": "Never lose an idea.", "font": FONT_REG,
         "overlay": True, "x": 960.0, "y": [key(t0 + 0.5, 652.0), key(t0 + 1.0, 642.0)],
         "font_size": 60.0, "fill": "#f7f6f2", "tracking": 0.4, "opacity": fade(t0 + 0.5)},
        {"id": "url", "kind": "text", "text": "matari-audio.com/buffr", "font": FONT_REG,
         "overlay": True, "x": 960.0, "y": 714.0, "font_size": 30.0, "fill": "#9a9a95",
         "tracking": 1.0, "opacity": fade(t0 + 1.0)},
    ]


# ---------------------------------------------------------------- project
def project():
    return {
        "size": [1920, 1080],
        "fps": FPS,
        "sources": [
            {"id": FONT, "kind": "font", "path": "media/Barlow-SemiBold.ttf"},
            {"id": FONT_REG, "kind": "font", "path": "media/Barlow-Regular.ttf"},
        ],
        "render": {"glass": "trace", "crf": 14},
        "scenes": [{
            "name": "buffr",
            "duration": DUR,
            "background": "#07060b",
            "mode": "3d",
            "layers": [
                camera(), *lights(),
                {"id": "music", "kind": "audio", "path": "buffr-music.wav"},
                *atmosphere()[:1], buffr(), *atmosphere()[1:], *words(), *end_card(),
            ],
            "environment": {"intensity": 0.35},
            "ground": {"y": FLOOR, "color": "#0a0910", "radius": 3000.0, "reflect": 0.5, "contact": 0.7},
            "fog": {"color": "#0d0a14", "near": 2200.0, "far": 7500.0},
            "ao": {"strength": 0.8, "radius": 40.0},
            "bloom": {"strength": 0.75, "threshold": 0.78},
            "effects": [
                {"type": "levels", "black": 0.015, "gamma": 0.96, "saturation": 1.12,
                 "tint": "#ffd9b8", "tint_amount": 0.06},
                {"type": "grain", "amount": 0.035, "size": 1.2},
                {"type": "crt", "curvature": 0.0, "scanlines": 0.0, "vignette": 0.55},
            ],
        }],
    }


if __name__ == "__main__":
    paint()
    out = HERE / "buffr.cut.json"
    out.write_text(json.dumps(project(), indent=2) + "\n")
    print(f"wrote {out}, buffr-music.wav and media/ ({len(NOTES)} notes)")
