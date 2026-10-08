#!/usr/bin/env python3
"""Builds the SMG shot sounds in assets/sounds/smg/ from a real recording.

Source: "Gunshot Sounds" by Vincent Sevedge on OpenGameArt (cz.wav, a CZ-52 pistol, four shots
in one file; the pack's licence file says CC BY 3.0 - credited in assets/CREDITS.md).

Each output is one shot cut from the recording and made punchier and shorter, as a submachine
gun's rapid fire needs (the next shot arrives 0.12 s later, so the tail is kept to ~0.45 s):
  * a steep high-pass removes the rumble and handling noise before the shot
  * a synthesised low thump (a fast downward sine sweep) is mixed in for body
  * soft clipping glues the crack and the thump together and adds bite
  * a short fade-out ends the tail cleanly
Usage: python3 tools/make_shot_sounds.py <path to cz.wav>
"""
import sys
import wave

import numpy as np

SOURCE_SHOTS = [2.83, 4.05, 5.54]   # onset of shots 2-4 in cz.wav, seconds (shot 1 has a slow low boom and is left out)
OUT = "assets/sounds/smg"


def load(path):
    w = wave.open(path)
    assert w.getsampwidth() == 2 and w.getnchannels() == 2
    sr = w.getframerate()
    data = np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).reshape(-1, 2).astype(np.float64) / 32768.0
    return sr, data


def highpass(x, sr, cutoff):
    # Simple one-pole high-pass applied twice (12 dB/oct).
    a = np.exp(-2 * np.pi * cutoff / sr)
    for _ in range(2):
        y = np.zeros_like(x)
        prev_x = prev_y = np.zeros(x.shape[1])
        for i in range(len(x)):
            prev_y = a * (prev_y + x[i] - prev_x)
            prev_x = x[i]
            y[i] = prev_y
        x = y
    return x


def thump(sr, length, seed):
    t = np.arange(int(length * sr)) / sr
    freq = 55 + 95 * np.exp(-t * 38)             # sweeps down from ~150 Hz to 55 Hz
    phase = 2 * np.pi * np.cumsum(freq) / sr
    env = np.exp(-t * 26)
    return np.sin(phase) * env


def find_crack(sr, data, onset):
    """The first sample of the muzzle crack: the first time the signal reaches 30% of the shot's
    own peak, searching around the rough onset (which can land on handling noise just before)."""
    m = np.abs(data).max(axis=1)
    lo, hi = int((onset - 0.03) * sr), int((onset + 0.15) * sr)
    window = m[lo:hi]
    first = int(np.argmax(window > 0.3 * window.max()))
    return (lo + first) / sr


def process(sr, data, onset, index):
    start = int((find_crack(sr, data, onset) - 0.004) * sr)
    n = int(0.40 * sr)
    seg = data[start:start + n].copy()
    seg = highpass(seg, sr, 70)
    # The recording has a room echo; a submachine gun's fire needs a tight report, so the tail
    # is pulled down after the first 40 ms (an exponential decay), keeping the crack intact.
    t = np.arange(len(seg)) / sr
    seg *= np.where(t < 0.04, 1.0, np.exp(-(t - 0.04) * 16))[:, None]
    # Bring each shot to a common level before shaping.
    seg /= max(abs(seg).max(), 1e-6)
    body = thump(sr, 0.18, index)[:, None] * 0.55
    seg[:len(body)] += body
    # Soft clip (tanh) for glue and bite, then level.
    seg = np.tanh(seg * 1.6) / np.tanh(1.6)
    # Short fade in (no click) and a smooth fade out over the tail.
    fade_in = int(0.002 * sr)
    seg[:fade_in] *= np.linspace(0, 1, fade_in)[:, None]
    tail = int(0.12 * sr)
    seg[-tail:] *= (np.linspace(1, 0, tail) ** 2)[:, None]
    seg *= 0.9 / max(abs(seg).max(), 1e-6)
    return seg


def main():
    import os
    sr, data = load(sys.argv[1])
    os.makedirs(OUT, exist_ok=True)
    for i, onset in enumerate(SOURCE_SHOTS, start=1):
        seg = process(sr, data, onset, i)
        pcm = (np.clip(seg, -1, 1) * 32767).astype(np.int16)
        with wave.open(f"{OUT}/smg_shot_{i}.wav", "wb") as w:
            w.setnchannels(2)
            w.setsampwidth(2)
            w.setframerate(sr)
            w.writeframes(pcm.tobytes())
        print(f"smg_shot_{i}.wav  {len(seg) / sr:.2f} s  peak {abs(seg).max():.2f}")


if __name__ == "__main__":
    main()
