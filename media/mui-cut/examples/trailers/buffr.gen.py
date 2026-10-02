#!/usr/bin/env python3
"""Writes buffr.cut.json and buffr-music.wav: the BUFFR trailer, 20 s.

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
import os
import sys
from pathlib import Path

import buffr_music as music

HERE = Path(__file__).parent
# Its captures key on the source's args: the music's hash makes a new mix a
# new recording.
music.write(HERE / "buffr-music.wav")
MUSIC_ID = hashlib.sha1((HERE / "buffr-music.wav").read_bytes()).hexdigest()[:12]
FPS = float(sys.argv[sys.argv.index("--fps") + 1]) if "--fps" in sys.argv else 60.0
DUR = music.DUR
BEAT = music.BEAT
DROP, HIT, STOP = music.DROP, music.HIT, music.STOP
E = 1.0 / FPS  # a shot's last key: the frame before its cut


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


# ---------------------------------------------------------------- BUFFR
# The editor laid out at 1280x720 UI px; UI (u, v) to project px:
VIEW = (1280, 720)
BX, BY, BS = 960.0, 520.0, 1.2


def ui(u, v):
    return BX + (u - VIEW[0] / 2) * BS, BY + (v - VIEW[1] / 2) * BS


FLOOR = BY + VIEW[1] / 2 * BS + 70  # a black mirror under it

# What it records: the music, and the motif (then the drop's pluck) as MIDI.
NOTES = [
    {"t": round(t, 4), "dur": round(d, 4), "pitch": p, "vel": v}
    for t, d, p, v in music.intro_notes() + music.arp_notes() + music.outro_notes()
]

# Parts: the header's logo and status, the toolbar's three groups, the two
# lanes and what is in them.
SELECT = [
    "hdr.logo", "hdr.status", "tb.left", "tb.center", "tb.right",
    "tl.audio", "tl.audio.plot", "tl.audio.scale", "tl.midi", "tl.midi.plot",
]

# Explode: shut (never quite: coplanar parts z-fight) while it records;
# the tape stop throws it apart into the dark; the build pulls it back,
# accelerating, and the drop welds it on the beat. 12 s opens it in depth.
REST = 0.03
APART = 1.0
EXPLODE = [
    key(0, REST), key(STOP, REST, out=[0.12, 0.25]),
    key(6.4, APART * 0.82), key(7.55, APART, in_=[-0.4, -0.02], out=[0.2, 0.0]),
    key(DROP, REST, in_=[-0.08, 0.35]),
    key(12.0, REST), key(12.6, 0.8), key(13.6, 0.86), key(14.1, REST),
    key(HIT, REST), key(HIT + 0.12, 0.09), key(HIT + 0.9, REST),
]
BACKDROP = keys((0, 1.0), (STOP + 0.15, 1.0), (STOP + 0.7, 0.0), (7.7, 0.0), (DROP, 1.0),
                (12.05, 1.0), (12.5, 0.15), (13.7, 0.15), (14.1, 1.0))

# A hand: drags a selection across the waveform in the last bar of the drop.
SEL = ((420, 250), (900, 250), 14.15, 14.75)


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
        "explode_stagger": 0.1,
        "explode": EXPLODE,
        "backdrop": BACKDROP,
        **pointer(),
        # It steps back into the dark for the end card.
        "opacity": keys((0, 1.0), (HIT + 0.25, 1.0), (HIT + 0.95, 0.0)),
        "x": BX,
        "y": BY,
        "scale": BS,
        "extrude": 5.0,
        "material": {"roughness": 0.38, "bevel": 3.0},
    }


# ---------------------------------------------------------------- camera
CUTS = (DROP, 10.0, 12.0, 14.0, HIT)


def camera():
    # (t, target u, v, z, distance, rx, ry, fov, aperture)
    shots = [
        # 1: low and close over the lanes as the idea is played, tracking
        # the playhead as the notes land
        (0.0, 120, 480, 0, 1000, 8, 30, 26, 22),
        (STOP - 0.25, 430, 470, 0, 880, 5, 16, 26, 20),
        # 2: the tape stop: the camera falls back as the light goes out
        (STOP + 0.9, 640, 360, -60, 2400, 4, 8, 30, 8),
        (6.0, 640, 360, -80, 2600, 0, -8, 30, 8),
        # 3: the build: an orbit through the parts, accelerating
        (7.55, 640, 360, -40, 2250, -4, -34, 30, 8),
        (DROP - E, 640, 360, 0, 2100, -5, -28, 30, 6),
        # 4: the drop: the hero, welded
        (DROP, 640, 380, 0, 2350, -8, 20, 30, 4),
        (10.0 - E, 640, 380, 0, 2250, -7, 13, 30, 4),
        # 5: along the waveform it is recording
        (10.0, 520, 250, 0, 820, -4, -30, 26, 16),
        (12.0 - E, 840, 250, 0, 780, -3, -24, 26, 16),
        # 6: in depth: the lanes apart, from the side
        (12.0, 640, 400, -80, 2500, -10, 44, 30, 7),
        (14.0 - E, 640, 400, -80, 2400, -8, 32, 30, 7),
        # 7: the hand selects the moment
        (14.0, 650, 290, 0, 900, -3, 6, 26, 12),
        (HIT - E, 700, 290, 0, 840, -3, 2, 26, 12),
        # 8: the hit, then it goes back into the dark for the end card
        (HIT, 640, 360, 0, 2000, -14, -30, 30, 4),
        (17.4, 640, 380, 0, 2900, -8, -6, 30, 3),
        (DUR, 640, 380, 0, 3100, -8, -4, 30, 3),
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
BRAND = {"red": "#ff1547", "yellow": "#ffea00", "green": "#05ff74", "cyan": "#00deff"}


def lights():
    # The key goes out with the tape stop and comes back on the drop.
    key_on = [key(0, 0.0), key(0.8, 1.9), key(STOP, 1.9, out=[0.15, 0]), key(STOP + 0.5, 0.12),
              key(6.0, 0.12), key(7.6, 0.9), key(DROP, 3.6, "bezier"), key(DROP + 0.35, 2.0), key(HIT, 2.0),
              key(HIT + 0.08, 3.4), key(HIT + 0.8, 1.8), key(19.4, 1.6), key(DUR, 0.0)]
    # Rims from behind in BUFFR's colours: they sweep in on the build.
    def rim(id_, color, ry, rx, peak, at):
        return {
            "id": id_, "kind": "light", "type": "spot", "fill": color,
            "rx": rx, "ry": ry, "cone": 34.0, "feather": 0.7, "softness": 2.0,
            "intensity": [key(0, 0.0), key(at, 0.0), key(at + 0.25, peak * 0.6), key(DROP, peak * 1.4),
                          key(DROP + 0.4, peak), key(HIT, peak), key(HIT + 0.1, peak * 1.6),
                          key(HIT + 1.0, peak * 0.8), key(DUR, 0.0)],
        }
    return [
        {"id": "key", "kind": "light", "fill": "#f2f0ea", "rx": 14.0,
         "ry": keys((0, -70.0), (STOP, -48.0), (DROP, -36.0), (DUR, -30.0)),
         "intensity": key_on, "softness": 3.0},
        rim("rim-red", BRAND["red"], 150.0, 18.0, 1.6, 6.0),
        rim("rim-cyan", BRAND["cyan"], -150.0, 14.0, 1.6, 6.5),
        rim("rim-yellow", BRAND["yellow"], 175.0, 42.0, 0.9, 7.0),
        {"id": "fill", "kind": "light", "type": "ambient", "fill": "#9fb0c8",
         "intensity": keys((0, 0.0), (0.8, 0.22), (STOP, 0.22), (STOP + 0.5, 0.05), (DROP, 0.25), (19.4, 0.22), (DUR, 0.0))},
    ]


# ---------------------------------------------------------------- words
FONT = "Barlow"
FONT_REG = "BarlowRegular"


def caption(id_, text, t0, t1, y=960.0, size=58.0, font=FONT, x=960.0, fill="#f4f4f2", z=None):
    """A line that focuses in (blur to sharp, a small rise) and out."""
    rise = 14.0
    layer = {
        "id": id_, "kind": "text", "text": text, "font": font, "overlay": True,
        "x": x, "y": [key(t0, y + rise), key(t0 + 0.55, y)],
        "font_size": size, "fill": fill, "tracking": 0.5,
        "opacity": [key(0, 0.0, "hold"), key(t0, 0.0), key(t0 + 0.4, 1.0), key(t1 - 0.3, 1.0), key(t1, 0.0, "hold")],
        "effects": [{"type": "blur", "radius": [key(t0, 14.0), key(t0 + 0.5, 0.0), key(t1 - 0.3, 0.0), key(t1, 10.0)]}],
    }
    return layer


def words():
    return [
        caption("you-played", "You played it.", 0.55, 2.0),
        caption("best-idea", "The best idea of the night.", 2.05, STOP),
        caption("gone", "Gone?", 4.7, 6.1, y=540.0, size=96.0, font=FONT_REG),
        caption("has-it", "BUFFR already has it.", DROP + 0.25, 10.0),
        caption("always", "Always recording. Hours of it.", 10.1, 12.0),
        caption("both", "Audio and MIDI, side by side.", 12.1, 14.0),
        caption("drag", "Drag the moment anywhere.", 14.2, HIT),
    ]


def end_card():
    """The logo comes forward out of the dark, then the line and the url."""
    t0 = HIT + 0.8
    return [
        {"id": "logo", "kind": "svg", "path": "media/buffr.svg", "overlay": True,
         "x": 960.0, "y": [key(t0, 470.0), key(t0 + 0.9, 455.0)],
         "scale": [key(t0, 0.11), key(t0 + 1.2, 0.12)],
         "opacity": [key(0, 0.0, "hold"), key(t0, 0.0), key(t0 + 0.5, 1.0), key(19.5, 1.0), key(DUR, 0.0)],
         "effects": [{"type": "blur", "radius": [key(t0, 16.0), key(t0 + 0.6, 0.0)]}]},
        caption("never-lose", "Never lose an idea.", HIT + 1.4, DUR, y=640.0, size=60.0, font=FONT_REG),
        caption("url", "matari-audio.com/buffr", HIT + 2.0, DUR, y=712.0, size=30.0, font=FONT_REG, fill="#9a9a95"),
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
            "background": "#040406",
            "mode": "3d",
            "layers": [
                camera(), *lights(),
                {"id": "music", "kind": "audio", "path": "buffr-music.wav"},
                buffr(), *words(), *end_card(),
            ],
            "ground": {"y": FLOOR, "color": "#060608", "radius": 2400.0, "reflect": 0.35, "contact": 0.6},
            "fog": {"color": "#040406", "near": 1400.0, "far": 5200.0},
            "ao": {"strength": 0.8, "radius": 40.0},
            "bloom": {"strength": 0.5, "threshold": 0.9},
            "effects": [
                {"type": "grain", "amount": 0.03, "size": 1.2},
                {"type": "crt", "curvature": 0.0, "scanlines": 0.0, "vignette": 0.5},
            ],
        }],
    }


if __name__ == "__main__":
    out = HERE / "buffr.cut.json"
    out.write_text(json.dumps(project(), indent=2) + "\n")
    print(f"wrote {out} and buffr-music.wav ({len(NOTES)} notes)")
