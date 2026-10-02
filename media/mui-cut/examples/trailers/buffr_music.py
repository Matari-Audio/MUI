"""The BUFFR trailer's music, synthesised here so it shares one beat grid
with the picture (buffr.gen.py imports it). 120 BPM, D minor, 20 s:

    bars 1-2   (0-4 s)   the idea: an electric piano plays the motif
    bar 3      (4-6 s)   it is gone: a tape stop, then nothing but air
    bar 4      (6-8 s)   the build: riser, snare roll, the motif filtered
    bars 5-8   (8-16 s)  the drop: kick, sub, pad, the motif as a pluck
    bars 9-10  (16-20 s) the hit, and the motif once more as it rings out

    python3 buffr_music.py   # writes buffr-music.wav (48 kHz, 16-bit)
"""
import wave
from pathlib import Path

import numpy as np

SR = 48000
BPM = 120.0
BEAT = 60.0 / BPM
BAR = 4 * BEAT
DUR = 20.0
N = int(DUR * SR)
DROP = 8.0
HIT = 16.0
STOP = 4.0  # the tape stop

rng = np.random.default_rng(7)


def hz(p):
    return 440.0 * 2 ** ((p - 69) / 12)


# ------------------------------------------------------------------ notes
# The motif: what "you played". MIDI pitches, (beat, length in beats, pitch).
MOTIF = [
    (1.0, 0.5, 69), (1.5, 0.5, 72), (2.0, 1.0, 74), (3.0, 0.5, 76), (3.5, 0.5, 74),
    (4.0, 0.75, 72), (4.75, 0.25, 69), (5.0, 1.0, 67), (6.0, 0.5, 69), (6.5, 0.5, 72),
    (7.0, 1.0, 74),
]
# Chords per bar of the drop (and the intro): Dm9, Bbmaj9, Fmaj7, C6/9.
CHORDS = [
    [50, 57, 60, 64, 65],
    [46, 53, 57, 60, 62],
    [53, 57, 60, 64, 67],
    [48, 55, 59, 62, 64],
]
ROOTS = [38, 34, 41, 36]


def intro_notes():
    """The motif on the electric piano, bars 1-2: (t, dur, pitch, vel)."""
    return [(b * BEAT, d * BEAT, p, 100 if b % 1 == 0 else 84) for b, d, p in MOTIF]


def arp_notes():
    """The drop's pluck: the motif re-cut as sixteenths over each chord,
    bars 5-8, plus the last bar's run up into the hit."""
    out = []
    shape = [0, 2, 3, 4, 3, 2, 4, 3]  # into the chord's top, turning
    for bar in range(4):
        chord = CHORDS[bar]
        for s in range(16):
            t = DROP + bar * BAR + s * BEAT / 4
            if bar == 3 and s >= 12:
                p = chord[s - 12 + 1] + 12  # the run into the hit
            else:
                p = chord[shape[s % 8]] + 12
            out.append((t, BEAT / 4 * 0.8, p, 112 if s % 4 == 0 else 78 + 6 * (s % 4)))
    return out


def outro_notes():
    """The motif's first phrase once more after the hit, slower."""
    return [(HIT + 0.5 + b * BEAT * 0.75, d * BEAT, p, 80) for b, d, p in MOTIF[:5]]


# ------------------------------------------------------------------ voices
def env(n, a, d, s, r, hold):
    """ADSR over n samples: attack, decay, sustain level, release; the
    note held for `hold` samples."""
    t = np.arange(n)
    e = np.minimum(t / max(a, 1), 1.0)
    e = np.where(t > a, s + (1 - s) * np.exp(-(t - a) / max(d, 1)), e)
    rel = np.clip((t - hold) / max(r, 1), 0, None)
    return e * np.exp(-5 * rel) * (t < hold + 5 * r)


def epiano(f, dur, vel):
    """A soft FM electric piano: a 1:1 pair whose brightness dies away,
    and a bell partial on the strike."""
    n = int((dur + 1.6) * SR)
    t = np.arange(n) / SR
    amp = vel / 127
    index = 1.6 * amp * np.exp(-t * 3.0)
    mod = np.sin(2 * np.pi * f * t) * index
    body = np.sin(2 * np.pi * f * t + mod)
    bell = 0.18 * np.sin(2 * np.pi * f * 14.0 * t) * np.exp(-t * 18)
    e = env(n, int(0.002 * SR), int(0.9 * SR), 0.0, int(0.25 * SR), int(dur * SR)) + np.exp(-t * 1.3) * 0.35
    e = np.minimum(e, 1.0) * np.where(t < dur + 0.05, 1.0, np.exp(-(t - dur - 0.05) * 6))
    return (body + bell) * e * amp


def additive(f, n, cutoff, harmonics=None, odd=False, detune=0.0, phase=0.0):
    """A band-limited saw (or square, `odd`) whose harmonics roll off above
    `cutoff` (Hz, a scalar or one per sample): a 2-pole lowpass, drawn
    harmonic by harmonic."""
    t = np.arange(n) / SR
    fr = f * (1 + detune)
    top = int(min(harmonics or 64, (SR / 2 - 200) / fr))
    out = np.zeros(n)
    for k in range(1, top + 1):
        if odd and k % 2 == 0:
            continue
        lp = 1.0 / (1.0 + (k * fr / cutoff) ** 4)
        out += np.sin(2 * np.pi * k * fr * t + phase * k) / k * lp
    return out


def pluck(f, dur, vel):
    n = int((dur + 0.5) * SR)
    t = np.arange(n) / SR
    cut = 300 + 5200 * (vel / 127) * np.exp(-t * 14)
    x = additive(f, n, cut, 48) * 0.7 + additive(f, n, cut, 48, detune=0.004, phase=1.3) * 0.5
    return x * env(n, int(0.001 * SR), int(0.18 * SR), 0.25, int(0.08 * SR), int(dur * SR)) * vel / 127


def pad(freqs, start, length, cutoff, out):
    """A wide supersaw chord into the stereo bus `out`."""
    n = int((length + 0.8) * SR)
    t = np.arange(n) / SR
    e = np.minimum(t / 0.06, 1.0) * np.where(t < length, 1.0, np.exp(-(t - length) * 5))
    i0 = int(start * SR)
    m = min(n, N - i0)
    for f in freqs:
        for side, det in ((0, -0.006), (1, 0.006), (0, 0.0013), (1, -0.0017)):
            v = additive(f, n, cutoff, 40, detune=det, phase=rng.uniform(0, 6.28))
            out[side, i0 : i0 + m] += (v * e)[:m] * 0.055


def kick(n=int(0.45 * SR)):
    t = np.arange(n) / SR
    f = 45 + 90 * np.exp(-t * 32)
    ph = 2 * np.pi * np.cumsum(f) / SR
    body = np.sin(ph) * np.exp(-t * 6.5)
    click = rng.standard_normal(n) * np.exp(-t * 400) * 0.25
    return np.tanh((body + click) * 1.6)


def noise_band(n, lo, hi, decay):
    """White noise kept between lo and hi Hz, decaying."""
    x = rng.standard_normal(n)
    X = np.fft.rfft(x)
    fr = np.fft.rfftfreq(n, 1 / SR)
    X *= (fr > lo) & (fr < hi)
    x = np.fft.irfft(X, n)
    t = np.arange(n) / SR
    return x / (np.abs(x).max() + 1e-9) * np.exp(-t * decay)


def clap():
    n = int(0.35 * SR)
    t = np.arange(n) / SR
    x = noise_band(n, 900, 7000, 18)
    # three quick hits before the tail, as hands do
    x *= 1 + 0.8 * ((t % 0.011) < 0.004) * (t < 0.03)
    body = np.sin(2 * np.pi * 190 * t) * np.exp(-t * 40) * 0.4
    return x * 0.8 + body


def hat(open_=False):
    n = int((0.25 if open_ else 0.06) * SR)
    return noise_band(n, 7000, 18000, 9 if open_ else 70) * 0.5


def place(bus, x, t, gain=1.0, pan=0.0):
    i0 = int(round(t * SR))
    if i0 >= N:
        return
    m = min(len(x), N - i0)
    l, r = np.cos((pan + 1) * np.pi / 4), np.sin((pan + 1) * np.pi / 4)
    bus[0, i0 : i0 + m] += x[:m] * gain * l * 1.414
    bus[1, i0 : i0 + m] += x[:m] * gain * r * 1.414


def reverb(x, seconds=2.6, damp=3.0, pre=0.02):
    """A stereo hall: decaying noise, darker as it dies, convolved by FFT."""
    n = int(seconds * SR)
    t = np.arange(n) / SR
    out = np.zeros_like(x)
    for c in range(2):
        ir = rng.standard_normal(n) * np.exp(-t * damp)
        # darken the tail: blend towards a smoothed copy over time
        smooth = np.convolve(ir, np.ones(24) / 24, mode="same")
        mix = np.clip(t / seconds * 1.5, 0, 1)
        ir = ir * (1 - mix) + smooth * mix
        ir[: int(pre * SR)] = 0
        ir /= np.sqrt((ir**2).sum())
        L = len(x[c]) + n
        size = 1 << (L - 1).bit_length()
        y = np.fft.irfft(np.fft.rfft(x[c], size) * np.fft.rfft(ir, size), size)[: len(x[c])]
        out[c] = y
    return out


def tape_stop(bus, at, length=0.7):
    """Everything from `at` slows to a halt over `length` seconds."""
    i0, n = int(at * SR), int(length * SR)
    seg = bus[:, i0 : i0 + 2 * n].copy()
    u = np.arange(n) / n
    rate = (1 - u) ** 1.6
    pos = np.cumsum(rate)
    for c in range(2):
        bus[c, i0 : i0 + n] = np.interp(pos, np.arange(seg.shape[1]), seg[c]) * (1 - u**3)
    bus[:, i0 + n :] *= 0  # what came after is gone
    return bus


def sidechain(t_kicks, depth=0.75, release=0.22):
    g = np.ones(N)
    t = np.arange(N) / SR
    for tk in t_kicks:
        i0 = int(tk * SR)
        m = min(int(0.5 * SR), N - i0)
        tt = t[i0 : i0 + m] - tk
        g[i0 : i0 + m] = np.minimum(g[i0 : i0 + m], 1 - depth * np.exp(-tt / release * 2.2))
    return g


# ------------------------------------------------------------------ the mix
def render():
    dry = np.zeros((2, N))  # drums, sub: little reverb
    wet = np.zeros((2, N))  # sent to the hall
    keys = np.zeros((2, N))  # the intro, tape-stopped

    # Bars 1-2: the idea, over a quiet Dm9 then Bb pad.
    for t, d, p, v in intro_notes():
        x = epiano(hz(p), d, v)
        place(keys, x, t, 0.5, pan=-0.15 + 0.3 * ((p % 5) / 4))
    pad([hz(p) for p in CHORDS[0]], 0.0, 2.0, 900, keys)
    pad([hz(p) for p in CHORDS[1]], 2.0, 2.2, 900, keys)
    keys[:, : int(0.6 * SR)] *= np.linspace(0, 1, int(0.6 * SR)) ** 2
    keys = keys + reverb(keys) * 0.45
    keys = tape_stop(keys, STOP)

    # Bar 3: air. A reversed swell of the last chord into the build.
    swell = np.zeros((2, N))
    pad([hz(p) for p in CHORDS[1]], 5.0, 1.0, 1400, swell)
    swell = reverb(swell, 2.0)
    swell[:, : int(5.0 * SR)] = 0
    rev = swell[:, int(5.0 * SR) : int(7.0 * SR)][:, ::-1].copy()
    air = np.zeros((2, N))
    air[:, int(5.0 * SR) : int(7.0 * SR)] = rev * 2.0

    # Bar 4: the build. Noise riser, snare roll, the motif filtered open.
    t = np.arange(N) / SR
    rise = np.zeros(N)
    i0, i1 = int(6.0 * SR), int((DROP - 0.25) * SR)
    seg = np.arange(i1 - i0) / (i1 - i0)
    for k, (lo, hi) in enumerate([(200, 2000), (600, 6000), (2000, 16000)]):
        band = noise_band(i1 - i0, lo, hi, 0)
        rise[i0:i1] += band * seg ** (2.2 - k * 0.5) * 0.3
    ph = 2 * np.pi * np.cumsum(np.where((t >= 6.0) & (t < DROP), 220 * 2 ** (2 * np.clip((t - 6.0) / 1.75, 0, 1)), 0)) / SR
    rise += np.sin(ph) * np.clip((t - 6.0) / 1.75, 0, 1) ** 2 * 0.06 * (t < DROP - 0.25)
    wet[0] += rise
    wet[1] += np.roll(rise, 90)
    # a snare roll: eighths, sixteenths, thirty-seconds, swelling
    roll = [6.0 + i * 0.25 for i in range(4)] + [7.0 + i * 0.125 for i in range(4)] + [7.5 + i * 0.0625 for i in range(4)]
    for i, tr in enumerate(roll):
        place(dry, clap(), tr, 0.1 + 0.4 * (i / len(roll)) ** 1.5)
    # the motif, played back fast and opening up: it is coming back
    for i, (tn, d, p, v) in enumerate(intro_notes()[:7]):
        tt = 6.0 + i * BEAT / 2
        x = pluck(hz(p), BEAT / 4, 60 + 9 * i)
        place(wet, x, tt, 0.18 + 0.05 * i, pan=0.3 if i % 2 else -0.3)

    # Bars 5-8: the drop.
    kicks = [DROP + i * BEAT for i in range(16)]
    for tk in kicks:
        place(dry, kick(), tk, 0.9)
    for b in range(16):
        if b % 2 == 1:
            place(dry, clap(), DROP + b * BEAT, 0.55)
            place(wet, clap(), DROP + b * BEAT, 0.2)
        place(dry, hat(open_=True), DROP + b * BEAT + BEAT / 2, 0.22, pan=0.25)
        for s in (1, 3):
            place(dry, hat(), DROP + b * BEAT + s * BEAT / 4, 0.12, pan=-0.3)
    # the last beat before the hit: a 16th clap fill
    for i in range(4):
        place(dry, clap(), HIT - BEAT + i * BEAT / 4, 0.25 + 0.08 * i)
    duck = sidechain(kicks)
    padbus = np.zeros((2, N))
    for bar in range(4):
        pad([hz(p) for p in CHORDS[bar]], DROP + bar * BAR, BAR, 2600 + 400 * bar, padbus)
    for t0, d, p, v in arp_notes():
        place(wet, pluck(hz(p), d, v), t0, 0.32, pan=0.35 * np.sin(t0 * 5.1))
    # Sub: root on every beat, the octave on the offs.
    sub = np.zeros(N)
    for bar in range(4):
        for b in range(8):
            t0 = DROP + bar * BAR + b * BEAT / 2
            f = hz(ROOTS[bar] + (12 if b % 2 else 0))
            n = int(BEAT / 2 * SR * 0.95)
            tt = np.arange(n) / SR
            x = np.tanh(1.5 * np.sin(2 * np.pi * f * tt)) * np.minimum(tt / 0.004, 1) * np.exp(-tt * 2.5)
            i0 = int(t0 * SR)
            sub[i0 : i0 + n] += x * 0.42
    dry += sub * duck
    wet += padbus * duck * 1.1

    # Bar 9: the hit. A boom, a wide Dm(add9), a crash; then the motif again.
    n = int(3.5 * SR)
    tt = np.arange(n) / SR
    boom = np.sin(2 * np.pi * np.cumsum(38 + 60 * np.exp(-tt * 9)) / SR) * np.exp(-tt * 1.4)
    place(dry, np.tanh(boom * 1.8), HIT, 0.85)
    place(dry, kick(), HIT, 0.8)
    place(wet, noise_band(int(2.5 * SR), 3000, 16000, 2.2), HIT, 0.22)
    hitpad = np.zeros((2, N))
    pad([hz(p) for p in [38, 50, 57, 62, 65, 69, 76]], HIT, 1.6, 3200, hitpad)
    wet += hitpad * 1.2
    # reversed crash into the drop and the hit
    for at in (DROP, HIT):
        cr = noise_band(int(1.2 * SR), 2500, 15000, 2.5)[::-1].copy()
        place(wet, cr * np.linspace(0, 1, len(cr)) ** 2, at - len(cr) / SR, 0.25)
    for t0, d, p, v in outro_notes():
        place(wet, epiano(hz(p), d, v), t0, 0.45)

    hall = reverb(wet, 3.2, 2.2)
    mix = keys * 0.55 + air * 0.5 + dry + wet * 0.85 + hall * 0.55
    mix[:, int(4.65 * SR) : int(5.0 * SR)] *= 0.0  # the breath before the swell
    # A short gap before the drop: the breath.
    g0, g1 = int((DROP - 0.25) * SR), int(DROP * SR)
    mix[:, g0:g1] *= np.linspace(1, 0, g1 - g0) ** 0.5 * 0 + 0.08
    # The end: let the hall ring and fade by 19.8 s.
    fade = np.clip((19.8 - t) / 2.2, 0, 1) ** 1.5
    mix *= np.where(t > 17.6, fade, 1.0)
    # Master: gentle glue and a ceiling.
    mix = np.tanh(mix * 1.15) / np.tanh(1.15)
    mix *= 0.89 / (np.abs(mix).max() + 1e-9)
    return mix


def write(path):
    mix = render()
    dither = (rng.random(mix.shape) - rng.random(mix.shape)) / 32768
    pcm = np.clip((mix + dither) * 32767, -32768, 32767).astype("<i2")
    with wave.open(str(path), "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.T.copy().tobytes())


if __name__ == "__main__":
    out = Path(__file__).with_name("buffr-music.wav")
    write(out)
    print(f"wrote {out}")
