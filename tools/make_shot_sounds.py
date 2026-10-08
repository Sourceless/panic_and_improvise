#!/usr/bin/env python3
"""Cuts the gun's shot sounds from The Free Firearm Sound Library (CC0).

Source: "The Free Firearm Sound Library" (Kickstarter-funded, released as CC0 "no rights
reserved"; https://opengameart.org/content/the-free-firearm-sound-library), the Carl Gustav M45
("Swedish K") 9 mm submachine gun, close-miked ("near distance") recordings, 96 kHz / 24-bit.
  G_31P.wav  three single shots

The three single shots are cut, at their muzzle blast, resampled to 48 kHz, 16-bit stereo, with only a
30 Hz high-pass to remove DC and rumble and a fade over the end of the tail. Nothing else is done
to them: no added layers, no distortion. They keep their natural level differences.

Usage: python3 tools/make_shot_sounds.py "<dir containing G_31P.wav>"
"""
import sys
import wave

import numpy as np
from scipy import signal
from scipy.io import wavfile

OUT = "assets/sounds/smg"
# (file, onset of the shot in seconds, roughly). The recording's burst files aren't used: in a
# burst each shot's tail contains the next shot, so a single cut from one sounds like a double hit.
SHOTS = [("G_31P.wav", 0.31), ("G_31P.wav", 3.5), ("G_31P.wav", 6.726)]
LENGTH = 0.7   # seconds kept after each shot
FADE = 0.25    # of which the last this long fade out


def load(path):
    sr, a = wavfile.read(path)
    scale = {np.dtype("int32"): 2.0 ** 31, np.dtype("int16"): 2.0 ** 15}[a.dtype]
    return sr, a.astype(np.float64) / scale


def find_crack(sr, data, onset):
    """The first sample of the muzzle blast: the first time the signal reaches 25% of the
    shot's own peak, searching just around the rough onset."""
    m = np.abs(data).max(axis=1)
    lo, hi = int((onset - 0.03) * sr), int((onset + 0.06) * sr)
    window = m[lo:hi]
    return (lo + int(np.argmax(window > 0.25 * window.max()))) / sr


def cut(sr, data, onset):
    start = int((find_crack(sr, data, onset) - 0.003) * sr)
    seg = data[start:start + int(LENGTH * sr)].copy()
    seg = signal.resample_poly(seg, 1, 2, axis=0)             # 96 kHz -> 48 kHz
    sos = signal.butter(2, 30, "highpass", fs=sr // 2, output="sos")
    seg = signal.sosfilt(sos, seg, axis=0)
    fade_in = int(0.001 * 48000)
    seg[:fade_in] *= np.linspace(0, 1, fade_in)[:, None]
    n = int(FADE * 48000)
    seg[-n:] *= (np.linspace(1, 0, n) ** 2)[:, None]
    return seg


def main():
    import os
    folder = sys.argv[1].rstrip("/")
    os.makedirs(OUT, exist_ok=True)
    cache = {}
    cuts = []
    for name, onset in SHOTS:
        if name not in cache:
            cache[name] = load(f"{folder}/{name}")
        sr, data = cache[name]
        cuts.append(cut(sr, data, onset))
    # One common gain for the set (so the shots keep their natural differences), leaving a
    # little headroom under the loudest.
    gain = 0.9 / max(abs(c).max() for c in cuts)
    for i, seg in enumerate(cuts, start=1):
        pcm = (np.clip(seg * gain, -1, 1) * 32767).astype(np.int16)
        with wave.open(f"{OUT}/smg_shot_{i}.wav", "wb") as w:
            w.setnchannels(2)
            w.setsampwidth(2)
            w.setframerate(48000)
            w.writeframes(pcm.tobytes())
        print(f"smg_shot_{i}.wav  {len(seg) / 48000:.2f} s")


if __name__ == "__main__":
    main()
