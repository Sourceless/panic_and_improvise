#!/usr/bin/env python3
"""Makes the footstep sounds: four takes each of a step on grass, dirt, gravel, tarmac, stone, wood
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


def grass(rng, k):
    t = t_axis(0.3)
    swish = low(noise(rng, len(t)), 1500 + 200 * k) * env(t, 0.012, 0.07)
    crunch = band(noise(rng, len(t)), 3000, 7000) * env(t, 0.01, 0.03) * 0.25
    thump = tone(t, 85 + 6 * k, 0.04, 0.8)
    return finish(swish + crunch + thump)


def dirt(rng, k):
    t = t_axis(0.28)
    body = low(noise(rng, len(t)), 700 + 100 * k) * env(t, 0.008, 0.06)
    grit = np.zeros(len(t))
    for _ in range(10):
        at = rng.uniform(0.0, 0.12)
        click = band(noise(rng, int(0.012 * SR)), 2000, 6000) * np.exp(-np.arange(int(0.012 * SR)) / (0.002 * SR))
        place(grit, click * rng.uniform(0.1, 0.4), at)
    thump = tone(t, 105 + 8 * k, 0.045, 1.1)
    return finish(body + grit * 0.5 + thump)


def gravel(rng, k):
    t = t_axis(0.34)
    x = np.zeros(len(t))
    for i in range(46):
        at = rng.uniform(0.0, 0.2) ** 1.3 * 0.9
        n = int(rng.uniform(0.006, 0.02) * SR)
        click = band(noise(rng, n), rng.uniform(1500, 3500), rng.uniform(4500, 9000)) * np.exp(-np.arange(n) / (0.004 * SR))
        place(x, click * rng.uniform(0.15, 1.0) * (1 - at * 2), at)
    thump = tone(t, 95 + 5 * k, 0.03, 0.4)
    return finish(x * 1.2 + thump)


def tarmac(rng, k):
    t = t_axis(0.25)
    heel = high(noise(rng, len(t)), 1400) * env(t, 0.001, 0.008)
    body = tone(t, 150 + 12 * k, 0.03, 0.9) + tone(t, 310 + 20 * k, 0.015, 0.25)
    toe = np.zeros(len(t))
    place(toe, (high(noise(rng, int(0.05 * SR)), 1800) * env(t_axis(0.05), 0.001, 0.006)) * 0.5, 0.085 + 0.01 * rng.random())
    return finish(heel * 0.9 + body + toe)


def stone(rng, k):
    x = tarmac(rng, k)
    # A little room: two quick, faint echoes.
    out = np.concatenate([x, np.zeros(int(0.12 * SR))])
    for delay, gain in [(0.011, 0.35), (0.023, 0.22), (0.047, 0.12)]:
        d = int(delay * SR)
        out[d : d + len(x)] += x * gain
    return finish(out)


def wood(rng, k):
    t = t_axis(0.3)
    hollow = tone(t, 135 + 9 * k, 0.07, 1.0) + tone(t, 225 + 14 * k, 0.05, 0.6) + tone(t, 370 + 20 * k, 0.03, 0.3)
    knock = band(noise(rng, len(t)), 700, 2500) * env(t, 0.001, 0.012)
    heel = high(noise(rng, len(t)), 2500) * env(t, 0.001, 0.004) * 0.3
    return finish(hollow * 0.8 + knock * 0.8 + heel)


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
    out += tone(t, 70, 0.05, 0.5)
    return finish(out, 0.7)


SURFACES = {"grass": grass, "dirt": dirt, "gravel": gravel, "tarmac": tarmac, "stone": stone, "wood": wood, "water": water}


def main():
    os.makedirs(OUT, exist_ok=True)
    for name, make in SURFACES.items():
        for k in range(1, 5):
            rng = np.random.default_rng(sum(map(ord, name)) * 100 + k)
            samples = make(rng, k)
            wavfile.write(f"{OUT}/{name}_{k}.wav", SR, (samples * 32767).astype(np.int16))
    print("wrote", len(SURFACES) * 4, "files to", OUT)


main()
