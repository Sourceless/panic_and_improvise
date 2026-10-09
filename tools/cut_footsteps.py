#!/usr/bin/env python3
"""Cuts the footstep sounds (and the splash of a bullet into water) from CC0 recordings on OpenGameArt:

  tiny/   "Different steps on wood, stone, leaves, gravel and mud" by TinyWorlds
          https://opengameart.org/content/different-steps-on-wood-stone-leaves-gravel-and-mud
          ([kdd]DifferentSteps_0.zip unpacked: wood01-03, leaves01-02, mud02, stone01, gravel .ogg)
  fant/   "Fantozzi's Footsteps (Grass/Sand & Stone)" by Fantozzi
          https://opengameart.org/content/fantozzis-footsteps-grasssand-stone
          (Fantozzi-footsteps.7z unpacked: Fantozzi-Sand/Stone L/R 1-3 .ogg under ogg/)
  peludo/ "Water Splash and sand footsteps" by Peludo
          https://opengameart.org/content/water-splash-and-sand-footsteps
          (splash1_0.wav, splash2_0.wav)

Each is trimmed to its step, resampled to 48 kHz mono 16-bit, given a short fade out and levelled to
the same peak. (gst-launch-1.0 is used to decode the .ogg files.)

Usage: python3 tools/cut_footsteps.py <directory holding tiny/, fant/ and peludo/>
"""
import os
import subprocess
import sys
import tempfile

import numpy as np
from scipy import signal
from scipy.io import wavfile

SR = 48000
STEPS = "assets/sounds/footsteps"
IMPACT = "assets/sounds/impact"


def decode(path):
    """Any sound file as mono floats at 48 kHz."""
    with tempfile.NamedTemporaryFile(suffix=".wav") as out:
        subprocess.run(
            ["gst-launch-1.0", "-q", "filesrc", f"location={path}", "!", "decodebin", "!", "audioconvert", "!", "audioresample", "!",
             f"audio/x-raw,rate={SR},channels=1,format=S16LE", "!", "wavenc", "!", "filesink", f"location={out.name}"],
            check=True,
        )
        sr, a = wavfile.read(out.name)
    return a.astype(np.float64) / 32768.0


def cut(x, start=None, length=0.5, peak=0.8, speed=1.0):
    """The step in `x`: from just before its first strike (or `start` seconds) for `length` seconds."""
    if start is None:
        loud = np.where(np.abs(x) > 0.1 * np.abs(x).max())[0]
        start = max(loud[0] / SR - 0.006, 0.0)
    x = x[int(start * SR):]
    if speed != 1.0:
        x = signal.resample(x, int(len(x) / speed))
    x = x[: int(length * SR)]
    n = int(min(0.06, len(x) / SR * 0.4) * SR)
    x = x.copy()
    x[-n:] *= np.linspace(1, 0, n) ** 2
    x = x - np.mean(x)
    return x / np.abs(x).max() * peak


def write(path, x):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    wavfile.write(path, SR, (x * 32767).astype(np.int16))


def main():
    base = sys.argv[1].rstrip("/")
    tiny = lambda name: decode(f"{base}/tiny/{name}.ogg")
    fant = lambda name: decode(f"{base}/fant/Fantozzi-footsteps/ogg/Fantozzi-{name}.ogg")
    sets = {
        "grass": [cut(fant(n)) for n in ["SandL1", "SandR1", "SandL2", "SandR2"]],
        "dirt": [cut(tiny(n)) for n in ["leaves01", "leaves02", "mud02"]],
        "gravel": [cut(tiny("gravel"), speed=s) for s in [1.0, 0.92, 1.08]],
        "tarmac": [cut(fant(n)) for n in ["StoneL1", "StoneR1", "StoneL2", "StoneR2"]],
        "stone": [cut(tiny("stone01"))] + [cut(fant(n)) for n in ["StoneL3", "StoneR3"]],
        "wood": [cut(tiny(n)) for n in ["wood01", "wood02", "wood03"]],
    }
    # A step into water: the start of each splash, short.
    splash = [decode(f"{base}/peludo/splash1_0.wav"), decode(f"{base}/peludo/splash2_0.wav")]
    sets["water"] = [cut(s, length=0.55) for s in splash]
    for name, takes in sets.items():
        for k, x in enumerate(takes, 1):
            write(f"{STEPS}/{name}_{k}.wav", x)
    # A bullet into water: the whole splash and its tail.
    for k, s in enumerate(splash, 1):
        write(f"{IMPACT}/splash_{k}.wav", cut(s, length=1.2, peak=0.85))
    print({name: len(takes) for name, takes in sets.items()}, "and 2 splashes")


main()
