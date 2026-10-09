#!/usr/bin/env python3
"""Makes the footstep sounds (and the splash of a bullet into water): four takes each of a step on grass, dirt, gravel, tarmac, stone, wood
and water. They are synthesised (filtered noise and damped tones shaped like what the sound is: a
swish, a thud, a scatter of little clicks), not recorded, so there is nothing to credit.

Usage: python3 tools/make_footsteps.py
"""
import os

import numpy as np
from scipy import signal
from scipy.io import wavfile

SR = 48000
OUT = "assets/sounds/footsteps"


def t_axis(seconds):
    return np.arange(int(seconds * SR)) / SR


def noise(rng, n):
    return rng.standard_normal(n)


def band(x, lo, hi, order=2):
    sos = signal.butter(order, [lo, hi], "bandpass", fs=SR, output="sos")
    return signal.sosfilt(sos, x)


def low(x, hz, order=2):
    return signal.sosfilt(signal.butter(order, hz, "lowpass", fs=SR, output="sos"), x)


def high(x, hz, order=2):
    return signal.sosfilt(signal.butter(order, hz, "highpass", fs=SR, output="sos"), x)


def env(t, attack, decay):
    """Rises over `attack`, then dies away exponentially with time constant `decay`."""
    e = np.minimum(t / attack, 1.0) * np.exp(-np.maximum(t - attack, 0.0) / decay)
    return e


def tone(t, hz, decay, amp=1.0):
    return amp * np.sin(2 * np.pi * hz * t) * np.exp(-t / decay)


def place(base, sound, at):
    start = int(at * SR)
    end = min(len(base), start + len(sound))
    base[start:end] += sound[: end - start]


def finish(x, peak=0.8):
    x = x - np.mean(x)
    x = high(x, 25, 1)
    n = int(0.02 * SR)
    x[-n:] *= np.linspace(1, 0, n)
    return x / max(np.abs(x).max(), 1e-6) * peak


def thump(rng, t, cutoff, decay, amp=1.0):
    """A dull knock: noise with only its lows left, which dies away fast. Noise rather than a tone, so
    it doesn't ring on a note."""
    return amp * low(noise(rng, len(t)), cutoff, 3) * env(t, 0.003, decay)


def jitter(rng, value, spread=0.2):
    return value * (1.0 + rng.uniform(-spread, spread))


def grass(rng, k):
    t = t_axis(0.3)
    swish = low(noise(rng, len(t)), jitter(rng, 1500)) * env(t, 0.014, 0.075)
    crunch = band(noise(rng, len(t)), 3000, 7500) * env(t, 0.01, 0.03) * 0.2
    return finish(swish + crunch + thump(rng, t, jitter(rng, 220), 0.04, 2.0))


def dirt(rng, k):
    t = t_axis(0.28)
    body = low(noise(rng, len(t)), jitter(rng, 800)) * env(t, 0.008, 0.055)
    grit = np.zeros(len(t))
    for _ in range(12):
        at = rng.uniform(0.0, 0.12)
        n = int(0.012 * SR)
        click = band(noise(rng, n), 2000, 6000) * np.exp(-np.arange(n) / (0.002 * SR))
        place(grit, click * rng.uniform(0.1, 0.4), at)
    return finish(body + grit * 0.5 + thump(rng, t, jitter(rng, 260), 0.05, 2.2))


def gravel(rng, k):
    t = t_axis(0.34)
    x = np.zeros(len(t))
    for i in range(60):
        at = rng.uniform(0.0, 0.2) ** 1.3 * 0.9
        n = int(rng.uniform(0.006, 0.02) * SR)
        click = band(noise(rng, n), rng.uniform(1500, 3500), rng.uniform(4500, 9000)) * np.exp(-np.arange(n) / (0.004 * SR))
        place(x, click * rng.uniform(0.15, 1.0) * (1 - at * 2), at)
    return finish(x * 1.2 + thump(rng, t, jitter(rng, 240), 0.03, 1.0))


def tarmac(rng, k):
    t = t_axis(0.25)
    heel = high(noise(rng, len(t)), jitter(rng, 1400)) * env(t, 0.001, 0.007)
    body = thump(rng, t, jitter(rng, 700), 0.022, 1.6) + band(noise(rng, len(t)), 300, 1200) * env(t, 0.002, 0.03) * 0.8
    toe = np.zeros(len(t))
    n = int(0.05 * SR)
    place(toe, high(noise(rng, n), 1800) * env(t_axis(0.05), 0.001, 0.006) * 0.45, 0.08 + 0.02 * rng.random())
    return finish(heel * 0.9 + body + toe)


def stone(rng, k):
    x = tarmac(rng, k)
    # A little room: a few quick, faint echoes.
    out = np.concatenate([x, np.zeros(int(0.12 * SR))])
    for delay, gain in [(0.011, 0.35), (0.023, 0.22), (0.047, 0.12)]:
        d = int(delay * SR)
        out[d : d + len(x)] += x * gain
    return finish(out)


def wood(rng, k):
    t = t_axis(0.3)
    body = band(noise(rng, len(t)), jitter(rng, 130), jitter(rng, 520)) * env(t, 0.002, 0.045)
    knock = band(noise(rng, len(t)), 600, 2200) * env(t, 0.001, 0.01)
    heel = high(noise(rng, len(t)), 2500) * env(t, 0.001, 0.004) * 0.3
    # The faintest hollow note under it, a different one each time.
    hollow = tone(t, rng.uniform(95, 150), 0.05, 0.25)
    return finish(body * 1.4 + knock * 0.7 + heel + hollow)


def water(rng, k):
    t = t_axis(0.45)
    splash = band(noise(rng, len(t)), 300, 3500) * env(t, 0.006, 0.09)
    out = splash.copy()
    for _ in range(5):
        at = rng.uniform(0.02, 0.25)
        d = int(0.05 * SR)
        tt = np.arange(d) / SR
        f0 = rng.uniform(300, 700)
        chirp = np.sin(2 * np.pi * (f0 * tt + 6000 * tt * tt)) * np.exp(-tt / 0.015)
        place(out, chirp * rng.uniform(0.15, 0.4), at)
    out += thump(rng, t, 200, 0.05, 1.5)
    return finish(out, 0.7)


def splash(rng, k):
    """A bullet into water: a sharp slap, a plume, a plop as the hole closes, and drops pattering down."""
    t = t_axis(0.8)
    slap = high(noise(rng, len(t)), 1200) * env(t, 0.001, 0.02)
    plume = band(noise(rng, len(t)), 250, 5000) * env(t, 0.004, 0.16)
    out = slap * 0.9 + plume * 0.9
    out += thump(rng, t, 300, 0.07, 1.6)
    # The plop: a bubble rising in pitch as the cavity collapses.
    at = rng.uniform(0.05, 0.1)
    d = int(0.09 * SR)
    tt = np.arange(d) / SR
    f0 = rng.uniform(250, 420)
    plop = np.sin(2 * np.pi * (f0 * tt + 3500 * tt * tt)) * np.exp(-tt / 0.03)
    place(out, plop * 0.5, at)
    # The patter of drops falling back.
    for _ in range(14):
        at = rng.uniform(0.12, 0.65)
        n = int(0.03 * SR)
        tt = np.arange(n) / SR
        drop = np.sin(2 * np.pi * (rng.uniform(900, 2200) * tt + 9000 * tt * tt)) * np.exp(-tt / 0.007)
        place(out, drop * rng.uniform(0.05, 0.25) * (1 - at), at)
    return finish(out, 0.85)


SURFACES = {"grass": grass, "dirt": dirt, "gravel": gravel, "tarmac": tarmac, "stone": stone, "wood": wood, "water": water}


def main():
    os.makedirs(OUT, exist_ok=True)
    for name, make in SURFACES.items():
        for k in range(1, 5):
            rng = np.random.default_rng(sum(map(ord, name)) * 100 + k)
            samples = make(rng, k)
            wavfile.write(f"{OUT}/{name}_{k}.wav", SR, (samples * 32767).astype(np.int16))
    # A bullet into water, with the other impacts.
    os.makedirs("assets/sounds/impact", exist_ok=True)
    for k in range(1, 4):
        rng = np.random.default_rng(9000 + k)
        wavfile.write(f"assets/sounds/impact/splash_{k}.wav", SR, (splash(rng, k) * 32767).astype(np.int16))
    print("wrote", len(SURFACES) * 4 + 3, "files")


main()
