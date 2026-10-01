#!/usr/bin/env python3
"""Writes kurv.cut.json: the KURV trailer. The arp's notes and KURV's param
curves are generated here so the music and the motion share one beat grid.

    python3 kurv.gen.py [--fps N]   # then: mui-cut check kurv.cut.json
"""
import os
import json
import sys
from pathlib import Path

BPM = 120.0
BEAT = 60.0 / BPM  # 0.5 s
S16 = BEAT / 4  # 0.125 s
DUR = 10.0
FPS = float(sys.argv[sys.argv.index("--fps") + 1]) if "--fps" in sys.argv else 60.0


def key(t, v, interp="bezier"):
    return {"t": round(t, 4), "v": v, "interp": interp}


def keys(*pairs, interp="bezier"):
    return [key(t, v, interp) for t, v in pairs]


def hold(*pairs):
    return keys(*pairs, interp="hold")


# ---------------------------------------------------------------- music
# F minor, 120 BPM (a beat is 0.5 s, a bar 2 s). Bar 1 a low pulse, bar 2 the
# arp enters, bars 3-4 rise while a hand opens the oscillator, 8.0 s the hit
# and its tail.
DB = [49, 53, 56, 61, 65, 68]  # Db  F  Ab ...
BBM = [46, 49, 53, 58, 61, 65]  # Bb  Db F  ...
C = [48, 52, 55, 60, 64, 67]  # C   E  G  (the V)
PATTERN = [0, 2, 4, 3, 1, 3, 4, 5]  # into the chord, up and turning
HIT = [29, 41, 48, 53, 56, 60, 65, 68, 72]  # F minor, F1 to C5
HIT_T = 8.0

notes = []


def note(t, dur, pitch, vel):
    notes.append({"t": round(t, 4), "dur": round(dur, 4), "pitch": pitch, "vel": vel})


# Bar 1: the pulse, 8ths, F2 on the beat and F3 off it.
for i in range(16 // 2):
    note(i * BEAT / 2, 0.22, 41 if i % 2 == 0 else 53, 100 if i % 2 == 0 else 64)
# Bars 2-4: 16ths over a bass on every beat; bar 4 climbs an octave.
for bar, (chord, bass) in enumerate([(DB, 37), (BBM, 34), (C, 36)], start=1):
    for s in range(16):
        t = bar * 2.0 + s * S16
        pitch = chord[PATTERN[s % 8]]
        if bar == 3:
            pitch += 12 * (s // 8)  # the second half an octave up
        if bar == 3 and s >= 12:
            pitch = chord[s - 12 + 2] + 12  # a run into the hit
        note(t, S16 * 0.85, pitch, 110 if s % 4 == 0 else 80 + 4 * (s % 4))
        if s % 4 == 0:
            note(t, BEAT * 0.8, bass, 110)
for p in HIT:
    note(HIT_T, DUR - HIT_T - 0.06, p, 124)

# KURV's host parameters that its DSP reads: the rest of its patch is
# played by hand below.
params = {
    "Output": hold((0, -12.0)),
    "Velocity Amount": hold((0, 0.55)),
}


def plugin_params():
    return [{"id": name, "field": "value", "value": v} for name, v in params.items()]


# The mix: a swell into bar 1, then the hit's tail decays to nothing.
VOLUME = [key(0.0, 0.0), key(1.9, 1.0), key(HIT_T, 1.0), key(9.94, 0.0, "hold")]
VOLUME[2]["out"] = [0.25, -0.55]  # falls away fast, then long

# ---------------------------------------------------------------- the hand
# Gestures on KURV's own UI (UI pixels, seconds). Its oscillator is not a
# host parameter, so a hand plays it: the same keyed pointer drag moves the
# control on screen and the sound. Up is more; WAVE is ~125 px for its whole
# range (sine 0 .. triangle .. saw .. pulse), VOICES ~120 px for 1..64.
WAVE = (411, 363)
VOICES = (682, 363)
GESTURES = [
    ("drag", 0.12, 0.24, *WAVE, 0, 220),  # before the picture: down to a sine
    ("drag", 2.0, 4.7, *WAVE, 0, -74),  # the build opens it towards saw
    ("drag", 5.0, 5.35, *VOICES, 0, -11),  # the explode: one voice to seven
    ("drag", 6.0, 7.9, *WAVE, 0, -12),  # the V leans harder
    ("drag", 8.5, 9.8, *WAVE, 0, 70),  # the tail closes again
]
def pointer(gestures):
    xs, ys, down = [(0, -1.0)], [(0, -1.0)], [(0, 0)]
    for g in gestures:
        if g[0] == "click":
            _, t, x, y = g
            xs.append((t, float(x)))
            ys.append((t, float(y)))
            down += [(t + 0.05, 1), (t + 0.15, 0)]
            end = t + 0.2
        else:  # ("drag", t0, t1, x, y, dx, dy): press, move with an ease, let go
            _, t0, t1, x, y, dx, dy = g
            xs += [(t0 - 0.1, float(x)), (t0, float(x)), (t1, float(x + dx))]
            ys += [(t0 - 0.1, float(y)), (t0, float(y)), (t1, float(y + dy))]
            down += [(t0, 1), (t1 + 0.05, 0)]
            end = t1 + 0.1
        xs.append((end, -1.0))
        ys.append((end, -1.0))
    ease = lambda k: [key(t, v, "hold" if v < 0 or nxt < 0 else "bezier") for (t, v), (_, nxt) in zip(k, k[1:] + [(0, -1)])]
    return {"pointer_x": ease(xs), "pointer_y": ease(ys), "pointer_down": hold(*down)}



# ---------------------------------------------------------------- picture
# KURV floats above a dark, reflective floor. UI pixels (u, v) map to the
# project as X = KX + (u - 640) * KS, Y = KY + (v - 400) * KS.
KX, KY, KS = 960.0, 430.0, 1.0
FLOOR = 1000.0


def ui(u, v):
    return KX + (u - 640) * KS, KY + (v - 400) * KS


# The explode: slightly apart in depth from the start, open on the 5.0 s
# build (panels first, then their controls), back together on the hit.
OPEN = 0.55
REST = 0.04  # never quite shut: coplanar parts would z-fight
EXPLODE = [key(0, 0.08), key(4.9, 0.08), key(6.1, OPEN)]
# The arp moves it: a small lift on every beat of bars 3-4, from the notes.
for n in notes:
    if 6.1 < n["t"] < HIT_T - 0.4 and n["vel"] >= 110 and n["pitch"] > 40:
        EXPLODE += [key(n["t"], OPEN), key(n["t"] + 0.06, OPEN + 0.04), key(n["t"] + 0.45, OPEN)]
EXPLODE += [key(HIT_T, OPEN), key(HIT_T + 0.75, REST)]
EXPLODE[-2]["out"] = [0.1, -0.7 * (OPEN - REST)]  # snaps shut on the hit, then settles

# Controls the hand touches, lit while touched.
P_WAVE = "osc/0/osc/0/panel/wave/osc/0/wave"
P_VOICES = "osc/0/osc/0/panel/unison/osc/0/voices"
P_EDITOR = "osc/0/osc/0/panel/wave/osc/0/wave-editor"
P_UNISON = "osc/0/osc/0/panel/unison/osc/0/unison"


def lit(t0, t1):
    # Out by the release: a control drops its pressed fill as it lets go,
    # and a plate still lit behind it would flash through.
    return keys((t0 - 0.15, 0.0), (t0, 1.0), (t1 - 0.12, 1.0), (t1, 0.0))


kurv_parts = {
    "masthead": {"opacity": 0.0},  # its light-grey plate reads as a slab in 3D
    P_WAVE: {"highlight": lit(2.0, 4.7)},
    P_VOICES: {"highlight": lit(5.0, 5.35)},
}

source = {"plugin": "../../../../../KURV", "features": ["process-lab"]}
PRESET = os.environ.get("KURV_PRESET", "Arp/Pulse Step")
common = {
    "kind": "plugin",
    "source": source,
    "preset": f"../../../../../KURV/assets/presets/{PRESET}.kurvy",
    "notes": notes,
    "params": plugin_params(),
    "explode_levels": 4,
    "explode_stagger": 0.12,
    **pointer(GESTURES),
    "x": KX,
    "y": KY,
    "scale": KS,
    "extrude": 6.0,
}


def track(points, cuts=()):
    """Keys from (t, v) points; a point just before a cut holds, so the
    cut is a clean jump on its frame. Every move is a smooth bezier ease."""
    out = []
    for i, (t, v) in enumerate(points):
        nxt = points[i + 1][0] if i + 1 < len(points) else None
        cut = nxt is not None and any(abs(nxt - c) < 1e-9 for c in cuts)
        out.append(key(t, v, "hold" if cut else "bezier"))
    return out


CUTS = (2.0, 4.0)
E = 1.0 / FPS  # a shot's last key: the frame before its cut


def camera():
    # (t, target u, v, z, distance, rx, ry, fov, aperture) per waypoint.
    shots = [
        (0.0, 360, 292, -45, 1060, -4, -40, 24, 18),  # 1: macro across the wave
        (2.0 - E, 520, 300, -45, 900, -6, -28, 24, 18),
        (2.0, 440, 332, -50, 1000, -3, 24, 26, 13),  # 2: the hand on WAVE
        (4.0 - E, 452, 336, -50, 900, -4, 15, 26, 13),
        (4.0, 640, 330, -40, 1380, -9, -28, 28, 8),  # 3: the oscillator, 3/4, the sky behind
        (5.0, 655, 332, -60, 1300, -10, -22, 28, 7),
        (6.2, 600, 330, -200, 1500, -12, -38, 30, 6),  # 4: the explode, orbiting, glass
        (8.0, 610, 340, -180, 1400, -6, 34, 30, 5),
        (9.3, 640, 500, 0, 1800, -5, -12, 30, 3),  # 5: the hero
        (10.0, 640, 502, 0, 1770, -5, -13, 30, 3),
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


def lights():
    on = lambda v: keys((0, 0.0), (1.5, v), (9.4, v), (10.0, 0.0))
    return [
        {
            # A soft key from the front left, raking across the panel as it
            # comes up: the sky's light on the face the camera sees.
            "id": "key",
            "kind": "light",
            "fill": "#f4f1ea",
            "rx": 38.0,
            "ry": keys((0, -78.0), (2.0, -40.0)),
            "intensity": on(2.2),
            "softness": 3.0,
        },
        {
            # The sun itself, low and to the left, out of the frame: warm
            # rims (it travels from the sky's sun, 22 degrees down, from 70
            # left of straight ahead).
            "id": "sun",
            "kind": "light",
            "fill": "#ffe2b8",
            "rx": 22.0,
            "ry": 110.0,
            "intensity": on(1.6),
            "softness": 2.0,
        },
        {
            "id": "fill",
            "kind": "light",
            "type": "ambient",
            "fill": "#b4c6dc",
            "intensity": on(0.3),
        },
    ]


# What is not a part (the panel's flat card) falls away while it is apart.
BACKDROP = keys((0, 1.0), (5.2, 1.0), (6.0, 0.0), (HIT_T, 0.0), (HIT_T + 0.6, 1.0))


def kurv():
    parts = swept_parts()
    for pid, p in kurv_parts.items():
        parts[pid] = {**parts.get(pid, {}), **p}
    return {
        "id": "kurv",
        "name": "KURV",
        **common,
        "volume": VOLUME,
        "explode": EXPLODE,
        "backdrop": BACKDROP,
        "parts": parts,
        "material": GLASS,
    }


# ---------------------------------------------------------------- glass
# On the V's downbeat (6.0 s) KURV turns to glass, rippling out from the
# oscillator the hand plays to its racks in under half a second; the explode opens through it
# and the hit (8.0 s) snaps it shut, all glass. Its dark UI turns clear
# (`print`), its light marks stay as ink; bevelled rims bend the clouds.
GLASS_AT = 6.0
SWEEP = 0.4  # the middle to the racks
MIDDLE = 0.47
TURN = 0.3  # one panel, opaque to clear
# KURV's panels and their controls (two levels of parts; a part's glass
# keys its children too) and where their middles sit across the UI, 0..1.
# The racks' ids have dots, which part paths cannot: they turn with the
# layer, last.
PANELS = {
    "osc/0": 0.47,
    "warp/0": 0.47,
    "group/0": 0.47,
    "osc/0/osc/0/input-tab": 0.21,
    "osc/0/osc/0/title/well": 0.36,
    "osc/0/osc/0/title/above": 0.25,
    "osc/0/osc/0/title/material": 0.25,
    "osc/0/osc/0/title/below": 0.25,
    "osc/0/osc/0/panel/wave": 0.38,
    "osc/0/osc/0/panel/unison": 0.6,
    "osc/0/osc/0/output-tab": 0.74,
    "warp/0/warp/0/rail": 0.26,
    "warp/0/warp/0/response": 0.5,
    "warp/0/warp/0/type/prev": 0.3,
    "warp/0/warp/0/type": 0.33,
    "warp/0/warp/0/type/next": 0.36,
    "warp/0/warp/0/CUTOFF": 0.42,
    "warp/0/warp/0/RESONANCE": 0.5,
    "warp/0/warp/0/DB/OCT": 0.59,
    "warp/0/warp/0/MORPH": 0.67,
    "group/0/group/0/power": 0.21,
    "group/0/group/0/title": 0.26,
    "group/0/group/0/envelope": 0.4,
    "group/0/group/0/gain": 0.52,
    "group/0/group/0/pan": 0.56,
    "group/0/group/0/pitch": 0.6,
    "group/0/group/0/routing": 0.68,
    "group/0/group/0/remove": 0.73,
    "group/0/group/0/collapse": 0.73,
}


def turn(t0):
    """Opaque to glass from t0, over TURN seconds."""
    return {
        "transmission": keys((t0, 0.0), (t0 + TURN, 1.0)),
        "roughness": keys((t0, 0.42), (t0 + TURN, 0.04)),
        "bevel": keys((t0, 0.0), (t0 + TURN, 9.0)),
    }


GLASS = {
    # The backdrop and the racks: the end of the ripple.
    **turn(GLASS_AT + SWEEP),
    "print": 1.0,
    "ior": 1.5,
    "thickness": 14.0,
    "dispersion": 0.55,
    "tint": "#f4f8ff",
}


def swept_parts():
    out = {}
    for pid, u in PANELS.items():
        out[pid] = {"material": turn(GLASS_AT + SWEEP * min(abs(u - MIDDLE) / 0.4, 1.0))}
    return out


def wordmark():
    return {
        "id": "wordmark",
        "kind": "text",
        "text": "KURV",
        "x": KX,
        "y": keys((8.7, FLOOR - 92.0), (9.4, FLOOR - 100.0)),
        "z": -120.0,
        "font_size": 64.0,
        "weight": 500.0,
        "tracking": 30.0,
        "fill": "#cdd2da",
        "opacity": keys((0, 0.0), (8.7, 0.0), (9.4, 1.0)),
    }


project = {
    "size": [1920, 1080],
    "fps": FPS,
    "scenes": [
        {
            "name": "kurv",
            "duration": DUR,
            "background": "#9fb6cf",
            "mode": "3d",
            "layers": [camera(), *lights(), kurv(), wordmark()],
            # Late afternoon above the clouds: the sun low on the left, out
            # of the frame, warming the clouds; they drift right.
            "sky": {
                "elevation": 22.0,
                "azimuth": -70.0,
                "cover": 0.44,
                "wind": 0.05,
                "zenith": "#1d4c96",
                "horizon": "#a8c1dd",
                "sun": "#ffe7c6",
                "intensity": keys((0, 0.0), (0.9, 1.0), (9.4, 1.0), (10.0, 0.0)),
            },
            "ao": {"strength": 1.0, "radius": 50.0},
            "effects": [
                {"type": "grain", "amount": 0.035, "size": 1.2},
                {"type": "crt", "curvature": 0.0, "scanlines": 0.0, "vignette": 0.45},
            ],
        }
    ],
}

out = Path(__file__).with_name("kurv.cut.json")
out.write_text(json.dumps(project, indent=2) + "\n")
print(f"wrote {out} ({len(notes)} notes)")
