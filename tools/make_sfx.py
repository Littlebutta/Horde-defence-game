"""Makes the generated sound effects in assets/sfx/gen/: every game event that had no recording of
its own (see /mnt/project-files/sound/missing-sounds.md). Everything is synthesised from noise,
sines and saws, so the clips can be remade or tweaked here and dropped back in.

    python tools/make_sfx.py

Needs numpy and scipy. The clips are mono 16-bit WAV at 22.05 kHz (plenty for effects, and half the size in the exe); the game sets each one's
volume (App::play_sfx), so here they're only levelled to about the same loudness. The *-loop clips are
seamless loops."""

import os
import wave

import numpy as np
from scipy.signal import butter, sosfilt

SR = 22050
OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "sfx", "gen")
rng = np.random.default_rng(7)


def t_(d):
    return np.arange(int(d * SR)) / SR


def noise(d):
    return rng.uniform(-1, 1, int(d * SR))


def filt(x, kind, f, order=2):
    if kind == "band":
        sos = butter(order, [f[0] / (SR / 2), f[1] / (SR / 2)], btype="band", output="sos")
    else:
        sos = butter(order, f / (SR / 2), btype=kind, output="sos")
    return sosfilt(sos, x)


def lp(x, f, o=2):
    return filt(x, "low", f, o)


def hp(x, f, o=2):
    return filt(x, "high", f, o)


def bp(x, a, b, o=2):
    return filt(x, "band", (a, b), o)


def decay(d, tau, attack=0.002):
    t = t_(d)
    return np.minimum(t / attack, 1.0) * np.exp(-t / tau)


def osc(freq, d, shape="sin", phase=0.0):
    """freq: a number or an array (a sweep) the length of the clip"""
    n = int(d * SR)
    f = np.broadcast_to(np.asarray(freq, dtype=float), (n,))
    ph = 2 * np.pi * np.cumsum(f) / SR + phase
    if shape == "sin":
        return np.sin(ph)
    if shape == "saw":
        return 2 * ((ph / (2 * np.pi)) % 1.0) - 1
    if shape == "sq":
        return np.sign(np.sin(ph))
    if shape == "tri":
        return 2 * np.abs(2 * ((ph / (2 * np.pi)) % 1.0) - 1) - 1
    raise ValueError(shape)


def glide(a, b, d, curve=1.0):
    k = np.linspace(0, 1, int(d * SR)) ** curve
    return a * (b / a) ** k


def ramp(d, a, b):
    return np.linspace(a, b, int(d * SR))


def pad(x, d):
    n = int(d * SR)
    return np.pad(x, (0, max(0, n - len(x))))[:n] if len(x) < n else x[:n]


def at(base, x, sec, g=1.0):
    i = int(sec * SR)
    end = min(len(base), i + len(x))
    base[i:end] += g * x[: end - i]
    return base


def fade_out(x, d=0.05):
    n = min(len(x), int(d * SR))
    x = x.copy()
    x[-n:] *= np.linspace(1, 0, n)
    return x


def loopable(x, xf=0.25):
    """a seamless loop: the clip's tail is crossfaded into its head"""
    n = int(xf * SR)
    body, tail = x[:-n].copy(), x[-n:]
    k = np.linspace(0, 1, n)
    body[:n] = body[:n] * np.sqrt(k) + tail * np.sqrt(1 - k)
    return body


def formants(src, fs, bw=1.0, gains=(1.0, 0.6, 0.25)):
    """a voice: the source through the vowel's formant bands"""
    out = np.zeros_like(src)
    for f, g in zip(fs, gains):
        out += g * bp(src, f / (1 + 0.12 * bw), f * (1 + 0.12 * bw), 2)
    return out


def save(name, x, peak=0.89):
    x = np.asarray(x, dtype=float)
    # every clip about as loud as the next (-16 dB RMS), short of clipping
    rms = np.sqrt(np.mean(x**2))
    if rms > 0:
        x = x * min(10 ** (-16 / 20) / rms, peak / np.max(np.abs(x)))
    if not name.endswith("-loop"):
        x = fade_out(x, 0.02)
    pcm = (np.clip(x, -1, 1) * 32767).astype("<i2")
    os.makedirs(OUT, exist_ok=True)
    with wave.open(os.path.join(OUT, name + ".wav"), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())
    print(f"{name:16s} {len(x) / SR:5.2f} s")


# ------------------------------------------------------------------------------ weapons and troops


def rifle():
    d = 0.22
    crack = hp(noise(d), 1800) * decay(d, 0.006)
    body = bp(noise(d), 350, 2200) * decay(d, 0.035)
    thump = osc(glide(160, 70, d), d) * decay(d, 0.03)
    tail = lp(noise(d), 900) * decay(d, 0.09) * 0.25
    return crack * 0.9 + body * 0.8 + thump * 0.6 + tail


def helium_loop():
    d = 3.25
    hiss = bp(noise(d), 2500, 9000, 3)
    cold = bp(noise(d), 600, 1800) * 0.3
    t = t_(d)
    wob = 0.75 + 0.25 * np.sin(2 * np.pi * t * 2 / 3.0)
    return loopable((hiss + cold) * wob)


def engine_loop():
    d = 2.25
    t = t_(d)
    # a diesel ticking over: firing pulses at 32 Hz, the tracks' squeal and clank on top
    fire = osc(32, d, "saw")
    fire = lp(np.maximum(fire, 0) ** 3, 260, 3)
    rumble = lp(noise(d), 120, 3) * 2.5
    grind = bp(noise(d), 700, 1600) * (0.5 + 0.5 * np.sin(2 * np.pi * 4 * t)) * 0.18
    clank = np.zeros(len(t))
    for k in range(9):
        at(clank, bp(noise(0.06), 1800, 3500) * decay(0.06, 0.012), k * d / 9 + rng.uniform(0, 0.05))
    return loopable(fire * 1.6 + rumble + grind + clank * 0.35)


def rotor_loop():
    d = 2.25
    t = t_(d)
    beats = np.zeros(len(t))
    blade = lp(noise(0.07), 380, 3) * decay(0.07, 0.022)
    for k in range(int(d * 13)):
        at(beats, blade, k / 13)
    whine = osc(1650, d) * 0.04 + osc(3300, d) * 0.015
    wash = lp(noise(d), 500) * 0.25
    return loopable(beats * 1.4 + whine + wash)


def rotor_pass(d, rate_a, rate_b, env):
    """rotor thumps whose rate goes from rate_a to rate_b, shaped by env"""
    out = np.zeros(int(d * SR))
    blade = lp(noise(0.08), 400, 3) * decay(0.08, 0.025)
    s = 0.0
    while s < d:
        f = s / d
        at(out, blade, s)
        s += 1.0 / (rate_a + (rate_b - rate_a) * f)
    whine = osc(glide(1200, 1900, d), d) * 0.05
    wash = lp(noise(d), 600) * 0.3
    return (out * 1.4 + whine + wash) * env


def gunship():
    d = 3.2
    env = np.interp(t_(d), [0, 1.2, 2.2, d], [0.05, 1.0, 0.8, 0.0])
    return rotor_pass(d, 12, 14, env)


def evac():
    d = 3.6
    env = np.interp(t_(d), [0, 0.6, 2.0, d], [0.4, 1.0, 0.7, 0.0])
    x = rotor_pass(d, 7, 16, env)
    # it lifts off: the wash fades higher as it climbs
    return x + hp(noise(d), 1500) * env * 0.05


def jet():
    d = 5.0
    t = t_(d)
    env = np.interp(t, [0, 2.2, 2.7, d], [0.0, 1.0, 0.85, 0.0]) ** 1.4
    low = lp(noise(d), 180, 3) * 3.0
    roar = bp(noise(d), 400, 2500) * 0.7
    # the hiss is strongest right overhead
    hissenv = np.interp(t, [0, 2.0, 2.5, 3.2, d], [0, 0.2, 1.0, 0.2, 0])
    hiss = hp(noise(d), 2500) * hissenv * 0.5
    whine = osc(np.interp(t, [0, 2.4, 2.8, d], [980, 900, 620, 560]), d) * 0.05
    return (low + roar) * env + hiss + whine * env


def launch():
    d = 5.0
    t = t_(d)
    ignite = osc(glide(90, 35, 0.8), 0.8) * decay(0.8, 0.25)
    roar_n = noise(d)
    crackle = (rng.uniform(0, 1, len(t)) > 0.9965) * rng.uniform(-1, 1, len(t))
    near = lp(roar_n, 2200, 2) + lp(crackle, 3000) * 4
    far = lp(roar_n, 500, 2) * 1.6
    k = np.clip(t / d, 0, 1)
    env = np.interp(t, [0, 0.3, 1.5, d], [0, 1, 0.9, 0]) ** 1.2
    out = (near * (1 - k) + far * k) * env
    return at(out, ignite, 0.0, 2.0)


def radio():
    d = 1.0
    out = np.zeros(int(d * SR))
    sq = bp(noise(0.09), 400, 3200) * decay(0.09, 0.05)
    at(out, sq, 0.0, 0.7)
    for i, (f, s) in enumerate([(1250, 0.14), (1660, 0.30)]):
        b = np.clip(osc(f, 0.11) * 2.5, -1, 1)
        b = bp(b, 300, 3400) * np.minimum(1, decay(0.11, 0.06) * 3)
        at(out, b, s, 0.6)
    at(out, sq * 0.6, 0.48, 0.6)
    # the line's background hiss
    out += bp(noise(d), 500, 3000) * 0.04 * np.interp(t_(d), [0, 0.05, 0.55, 0.6], [0, 1, 1, 0])
    return out


# ------------------------------------------------------------------------------------- the hordes


def voice(d, f0, f1, vowel, rough=0.0, vib=5.0):
    t = t_(d)
    f = glide(f0, f1, d) * (1 + 0.025 * np.sin(2 * np.pi * vib * t + rng.uniform(0, 6)))
    f *= 1 + 0.01 * lp(noise(d), 8) * 40
    src = osc(f, d, "saw")
    if rough:
        # a growl: the voice beating at half its pitch
        src *= 1 - rough * (0.5 + 0.5 * osc(f / 2, d))
    breath = bp(noise(d), 300, 3000) * 0.25
    return formants(src + breath, vowel)


def zombie_groan():
    d = 2.6
    out = np.zeros(int(d * SR))
    for k in range(4):
        dd = rng.uniform(1.6, 2.3)
        f0 = rng.uniform(85, 130)
        v = voice(dd, f0, f0 * 0.72, [rng.uniform(450, 600), rng.uniform(800, 1000), 2600], rough=0.3, vib=3.5)
        env = np.interp(t_(dd), [0, dd * 0.35, dd], [0, 1, 0]) ** 1.3
        at(out, v * env, rng.uniform(0, d - dd))
    return out


def zombie_rush():
    d = 3.2
    out = np.zeros(int(d * SR))
    for k in range(10):
        dd = rng.uniform(1.4, 2.6)
        f0 = rng.uniform(95, 190)
        v = voice(dd, f0 * 1.15, f0 * 0.8, [rng.uniform(600, 750), rng.uniform(1000, 1250), 2700], rough=0.55, vib=6)
        env = np.interp(t_(dd), [0, 0.25, dd], [0, 1, 0])
        at(out, v * env, rng.uniform(0, d - dd), rng.uniform(0.6, 1.0))
    # feet
    out += lp(noise(d), 160, 3) * np.interp(t_(d), [0, 0.5, 2.6, d], [0, 1.2, 1.0, 0])
    return out


def chitter(d=0.9, rate=(28, 45)):
    out = np.zeros(int(d * SR))
    s = 0.0
    while s < d - 0.02:
        f = rng.uniform(2600, 5200)
        click = bp(noise(0.012), f * 0.8, f * 1.2) * decay(0.012, 0.003)
        at(out, click, s, rng.uniform(0.4, 1.0))
        s += 1.0 / rng.uniform(*rate)
    trill = osc(3800 + 900 * osc(37, d), d) * 0.12
    env = np.interp(t_(d), [0, 0.05, d * 0.7, d], [0, 1, 0.8, 0])
    return (out + trill) * env


def bug_drop():
    d = 1.4
    out = np.zeros(int(d * SR))
    thud = osc(glide(75, 38, 0.5), 0.5) * decay(0.5, 0.12) + lp(noise(0.5), 300, 3) * decay(0.5, 0.06) * 0.8
    at(out, thud, 0.0, 1.0)
    at(out, chitter(1.0), 0.25, 0.45)
    return out


def hive():
    d = 2.8
    out = np.zeros(int(d * SR))
    for k in range(28):
        bd = rng.uniform(0.04, 0.12)
        f = rng.uniform(140, 480)
        b = osc(glide(f, f * 2.4, bd, 0.5), bd) * decay(bd, bd * 0.35, 0.004)
        at(out, b, rng.uniform(0, d - bd), rng.uniform(0.3, 0.8))
    groan = osc(glide(55, 42, d), d) * np.interp(t_(d), [0, 0.6, d], [0, 0.5, 0])
    squelch = lp(noise(d), 500, 3) * np.interp(t_(d), [0, 0.1, 0.5, d], [0, 1.5, 0.4, 0])
    return lp(out, 1800) + groan + squelch


def orc_portal():
    d = 3.0
    t = t_(d)
    sweep = osc(glide(260, 55, d, 0.6) * (1 + 0.08 * osc(7, d)), d, "saw")
    sweep = lp(sweep, 900) * np.interp(t, [0, 0.4, d], [0, 1, 0])
    whoosh = bp(noise(d), 200, 1200) * np.interp(t, [0, 1.6, 2.0, d], [0, 1.0, 0.6, 0]) ** 2 * 1.4
    shimmer = osc(glide(1800, 900, d), d) * osc(11, d) * 0.08 * np.interp(t, [0, 0.5, d], [0, 1, 0])
    boom = osc(glide(70, 30, 1.0), 1.0) * decay(1.0, 0.3)
    out = sweep + whoosh + shimmer
    return at(out, boom, 1.7, 1.2)


def orc_warcry():
    d = 2.0
    out = np.zeros(int(d * SR))
    for k in range(5):
        dd = rng.uniform(1.1, 1.5)
        f0 = rng.uniform(105, 150)
        f = np.interp(t_(dd), [0, 0.25, dd], [f0 * 0.85, f0 * 1.2, f0 * 0.9])
        # a shout that rises and falls, growling at half its pitch
        src =osc(f, dd, "saw") * (1 - 0.7 * (0.5 + 0.5 * osc(f / 2, dd)))
        v = formants(src + bp(noise(dd), 400, 3000) * 0.35, [720, 1150, 2500])
        env = np.interp(t_(dd), [0, 0.12, dd * 0.7, dd], [0, 1, 0.8, 0])
        at(out, v * env, rng.uniform(0, 0.4), rng.uniform(0.7, 1.0))
    return out


def death():
    d = 0.4
    f0 = rng.uniform(150, 190)
    v = formants(osc(glide(f0, f0 * 0.6, d), d, "saw"), [550, 950, 2500]) * decay(d, 0.09, 0.01)
    thud = lp(noise(d), 250, 3) * decay(d, 0.05) * 2
    return v + thud


def metal(d, parts, taus):
    out = np.zeros(int(d * SR))
    for f, tau in zip(parts, taus):
        out += osc(f * rng.uniform(0.98, 1.02), d) * decay(d, tau)
    return out


def tank_lost():
    d = 2.0
    boom = osc(glide(80, 30, d), d) * decay(d, 0.3) * 1.4 + lp(noise(d), 700, 3) * decay(d, 0.25) * 1.2
    clang = metal(d, [410, 1130, 1870, 2650, 3420], [0.5, 0.35, 0.25, 0.2, 0.12]) * 0.35
    debris = np.zeros(int(d * SR))
    for k in range(14):
        at(debris, bp(noise(0.05), 1500, 4500) * decay(0.05, 0.01), rng.uniform(0.2, 1.6), rng.uniform(0.2, 0.6))
    return boom + clang + debris


# ---------------------------------------------------------------------------------- base


def place():
    d = 0.45
    out = np.zeros(int(d * SR))
    for s, f in [(0.0, 110), (0.11, 90)]:
        thud = osc(glide(f, f * 0.6, 0.2), 0.2) * decay(0.2, 0.05) + lp(noise(0.2), 600) * decay(0.2, 0.02) * 0.6
        at(out, thud, s)
    at(out, metal(0.25, [2300, 3700], [0.05, 0.03]) * 0.25, 0.11)
    return out


def sell():
    d = 0.6
    out = np.zeros(int(d * SR))
    for k, f in enumerate([1500, 1150, 820]):
        at(out, metal(0.3, [f, f * 2.7], [0.08, 0.04]), k * 0.09, 0.6)
    at(out, lp(noise(0.25), 1200) * decay(0.25, 0.06) * 0.6, 0.0)
    return out


def wrecked():
    d = 1.4
    boom = osc(glide(90, 40, d), d) * decay(d, 0.18) + lp(noise(d), 900, 3) * decay(d, 0.2) * 1.3
    crumble = np.zeros(int(d * SR))
    for k in range(40):
        g = bp(noise(0.03), 300, 2500) * decay(0.03, 0.008)
        at(crumble, g, rng.uniform(0.05, 1.2) ** 1.5, rng.uniform(0.2, 0.7))
    clang = metal(d, [620, 1480, 2200], [0.3, 0.2, 0.12]) * 0.2
    return boom + crumble * 0.8 + clang


def collapse():
    d = 3.8
    t = t_(d)
    rumble = lp(noise(d), 140, 3) * np.interp(t, [0, 0.4, 1.4, d], [0, 1, 0.8, 0]) * 3.5
    crash = lp(noise(d), 1400, 2) * np.interp(t, [0, 0.15, 0.9, 2.5, d], [0, 0.4, 1.0, 0.2, 0])
    debris = np.zeros(len(t))
    for k in range(90):
        at(debris, bp(noise(0.04), 400, 3500) * decay(0.04, 0.01), rng.uniform(0.2, 3.2), rng.uniform(0.15, 0.6))
    return rumble + crash + debris * 0.6


def hq_fallen():
    d = 4.0
    t = t_(d)
    boom = osc(glide(70, 25, d), d) * decay(d, 0.6) * 1.5 + lp(noise(d), 500, 3) * decay(d, 0.5) * 1.5
    doom = lp(osc(glide(220, 55, d, 0.7), d, "saw") + osc(glide(262, 65, d, 0.7), d, "saw") * 0.6, 700)
    doom *= np.interp(t, [0, 0.3, 2.5, d], [0, 0.6, 0.4, 0])
    return boom + doom


def low_ammo():
    d = 0.75
    out = np.zeros(int(d * SR))
    for k, f in enumerate([880, 660, 880, 660]):
        b = lp(osc(f, 0.13, "sq"), 3000) * np.interp(t_(0.13), [0, 0.005, 0.11, 0.13], [0, 1, 1, 0])
        at(out, b, k * 0.17, 0.5)
    return out


def error():
    d = 0.28
    b = osc(110, d, "sq") + osc(117, d, "sq")
    return lp(b, 1800) * np.interp(t_(d), [0, 0.005, 0.24, d], [0, 1, 1, 0])


def factory_loop():
    d = 2.1
    t = t_(d)
    hum = osc(60, d) + 0.5 * osc(120, d) + 0.25 * osc(180, d)
    # a press stamping twice a second and a belt whirring
    press = np.zeros(len(t))
    for k in range(5):
        hit = lp(noise(0.12), 500, 3) * decay(0.12, 0.03) * 2.0 + metal(0.12, [1800], [0.03]) * 0.3
        at(press, hit, k * 0.5)
    belt = bp(noise(d), 900, 2400) * (0.6 + 0.4 * np.sin(2 * np.pi * 8 * t)) * 0.12
    return loopable(hum * 0.35 + press + belt, 0.1)


# --------------------------------------------------------------------------------- civilians


def bell(f, d, g=1.0):
    return g * (osc(f, d) * decay(d, d * 0.35) + 0.4 * osc(f * 2.01, d) * decay(d, d * 0.2) + 0.2 * osc(f * 3.03, d) * decay(d, d * 0.12))


def rescue():
    d = 0.7
    out = np.zeros(int(d * SR))
    at(out, bell(1318.5, 0.5), 0.0)
    at(out, bell(1975.5, 0.55), 0.09)
    return out


def brass(f, d):
    t = t_(d)
    v = osc(f * (1 + 0.004 * np.sin(2 * np.pi * 5 * t)), d, "saw") + 0.5 * osc(f * 1.003, d, "saw")
    cut = np.interp(t, [0, 0.06, d], [400, 3000, 1200])
    # a brighter attack: two filtered copies blended
    v = lp(v, 2600) * np.clip(cut / 3000, 0, 1) + lp(v, 600) * (1 - np.clip(cut / 3000, 0, 1))
    return v * np.interp(t, [0, 0.03, d * 0.7, d], [0, 1, 0.8, 0])


def win():
    d = 2.8
    out = np.zeros(int(d * SR))
    for k, f in enumerate([261.6, 329.6, 392.0]):
        at(out, brass(f, 0.3), k * 0.16, 0.7)
    for f in [261.6, 329.6, 392.0, 523.3]:
        at(out, brass(f, 2.2), 0.5, 0.45)
    at(out, bell(1046.5, 1.2, 0.5), 0.5)
    return out


def game_over():
    d = 3.4
    out = np.zeros(int(d * SR))
    for k, f in enumerate([220.0, 174.6, 146.8]):
        at(out, brass(f, 1.1 if k < 2 else 2.6), k * 0.55, 0.7)
    at(out, lp(osc(73.4, 2.6, "saw"), 300) * np.interp(t_(2.6), [0, 0.3, 2.6], [0, 1, 0]), 1.1, 0.8)
    return out


def research_start():
    d = 0.35
    b = osc(glide(600, 1250, d, 0.6), d, "tri") * np.interp(t_(d), [0, 0.01, 0.25, d], [0, 1, 0.7, 0])
    return lp(b, 5000)


def research_done():
    d = 1.1
    out = np.zeros(int(d * SR))
    for k, f in enumerate([783.99, 1046.5, 1318.5]):
        at(out, bell(f, 0.7), k * 0.11)
    return out


def tech_unlock():
    d = 1.6
    out = np.zeros(int(d * SR))
    for k, f in enumerate([1046.5, 1318.5, 1568.0, 2093.0, 2637.0]):
        at(out, bell(f, 0.8, 0.7), k * 0.06)
    sweep = bp(noise(d), 2000, 8000) * np.interp(t_(d), [0, 0.3, d], [0, 0.25, 0])
    low = brass(523.3, 1.1)
    at(out, low, 0.25, 0.25)
    return out + sweep


def inbound():
    d = 2.0
    out = np.zeros(int(d * SR))
    for s in [0.0, 0.55]:
        ping = osc(1050, 0.9) * decay(0.9, 0.18, 0.003)
        at(out, ping, s, 0.6)
        at(out, ping * 0.35, s + 0.18, 0.6)
    drone = lp(osc(110, d, "saw") + osc(116.5, d, "saw"), 500) * np.interp(t_(d), [0, 0.2, 1.6, d], [0, 0.6, 0.5, 0])
    return out + drone * 0.5


def join():
    d = 0.45
    out = np.zeros(int(d * SR))
    at(out, bell(880, 0.3), 0.0)
    at(out, bell(1318.5, 0.35), 0.1)
    return out


def leave():
    d = 0.45
    out = np.zeros(int(d * SR))
    at(out, bell(1318.5, 0.3), 0.0)
    at(out, bell(880, 0.35), 0.1)
    return out


CLIPS = {
    "rifle": rifle,
    "helium-loop": helium_loop,
    "engine-loop": engine_loop,
    "rotor-loop": rotor_loop,
    "factory-loop": factory_loop,
    "gunship": gunship,
    "jet": jet,
    "launch": launch,
    "radio": radio,
    "zombie-groan": zombie_groan,
    "zombie-rush": zombie_rush,
    "bug-drop": bug_drop,
    "hive": hive,
    "orc-portal": orc_portal,
    "orc-warcry": orc_warcry,
    "death": death,
    "tank-lost": tank_lost,
    "place": place,
    "sell": sell,
    "wrecked": wrecked,
    "collapse": collapse,
    "hq-fallen": hq_fallen,
    "low-ammo": low_ammo,
    "error": error,
    "rescue": rescue,
    "evac": evac,
    "win": win,
    "game-over": game_over,
    "research-start": research_start,
    "research-done": research_done,
    "tech-unlock": tech_unlock,
    "inbound": inbound,
    "join": join,
    "leave": leave,
}

if __name__ == "__main__":
    for name, make in CLIPS.items():
        save(name, make())
