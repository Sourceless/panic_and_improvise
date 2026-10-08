#!/usr/bin/env python3
"""Builds the SMG shot sounds. Two sets are made so they can be compared by ear:

  assets/sounds/smg/        "clean": three real shots cut from a recording, barely touched
  assets/sounds/smg_synth/  "synth": three shots built from filtered noise, no recording at all

Source for the clean set: "Gunshot Sounds" by Vincent Sevedge on OpenGameArt (cz.wav, a CZ-52
pistol, four shots in one file; the pack's licence file says CC BY 3.0, credited in
assets/CREDITS.md). The synth set is original.

The clean set is deliberately minimal: cut each shot at its crack, remove the rumble below
40 Hz with a high-pass, shorten the recording's room echo with a gentle exponential decay, fade
out cleanly and normalise. An earlier version also mixed in a synthesised low "thump" (a falling
sine) and soft-clipped the result; a pure falling tone is what a laser zap sounds like, so both
were removed.

Usage: python3 tools/make_shot_sounds.py <path to cz.wav>
Audition in the game with FPS_SHOT_SOUNDS=smg | smg_synth | old
"""
import os
import sys
import wave

import numpy as np
from scipy import signal

SOURCE_SHOTS = [2.83, 4.05, 5.54]   # onset of shots 2-4 in cz.wav, seconds (shot 1 has a slow low boom)
SR = 48000


def load(path):
    w = wave.open(path)
    assert w.getsampwidth() == 2 and w.getnchannels() == 2
    sr = w.getframerate()
    data = np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).reshape(-1, 2).astype(np.float64) / 32768.0
    return sr, data


def save(path, seg, sr):
    pcm = (np.clip(seg, -1, 1) * 32767).astype(np.int16)
    with wave.open(path, "wb") as w:
        w.setnchannels(2)
        w.setsampwidth(2)
        w.setframerate(sr)
        w.writeframes(pcm.tobytes())


def find_crack(sr, data, onset):
    """The first sample of the muzzle crack: the first time the signal reaches 30% of the shot's
    own peak, searching around the rough onset (which can land on handling noise just before)."""
    m = np.abs(data).max(axis=1)
    lo, hi = int((onset - 0.03) * sr), int((onset + 0.15) * sr)
    window = m[lo:hi]
    return (lo + int(np.argmax(window > 0.3 * window.max()))) / sr


def clean(sr, data, onset):
    start = int((find_crack(sr, data, onset) - 0.003) * sr)
    seg = data[start:start + int(0.45 * sr)].copy()
    sos = signal.butter(2, 40, "highpass", fs=sr, output="sos")
    seg = signal.sosfilt(sos, seg, axis=0)
    t = np.arange(len(seg)) / sr
    # Shorten the room echo a little after the first 60 ms; the crack and body are untouched.
    seg *= np.where(t < 0.06, 1.0, np.exp(-(t - 0.06) * 7.0))[:, None]
    fade_in = int(0.0015 * sr)
    seg[:fade_in] *= np.linspace(0, 1, fade_in)[:, None]
    tail = int(0.15 * sr)
    seg[-tail:] *= (np.linspace(1, 0, tail) ** 2)[:, None]
    return seg * (0.85 / max(abs(seg).max(), 1e-6))


def band(noise, lo, hi, sr):
    sos = signal.butter(2, [lo, hi], "bandpass", fs=sr, output="sos")
    return signal.sosfilt(sos, noise)


def synth(seed):
    """A gunshot from noise alone: a sharp crack, a mid 'snap', a body, a low boom and a short
    reverberant tail, each noise filtered into its own band and shaped by its own decay."""
    n = int(0.45 * SR)
    t = np.arange(n) / SR
    out = np.zeros((n, 2))
    for ch in range(2):
        rng = np.random.default_rng(seed * 10 + ch)
        white = rng.standard_normal(n)
        crack = signal.sosfilt(signal.butter(2, 2500, "highpass", fs=SR, output="sos"), white) * np.exp(-t / 0.003)
        snap = band(rng.standard_normal(n), 700, 5000, SR) * np.exp(-t / 0.012)
        body = signal.sosfilt(signal.butter(2, 1100, "lowpass", fs=SR, output="sos"), rng.standard_normal(n)) * np.exp(-t / 0.035)
        boom = signal.sosfilt(signal.butter(2, 240, "lowpass", fs=SR, output="sos"), rng.standard_normal(n)) * np.exp(-t / 0.09)
        # A reverberant tail: noise convolved with a decaying noise burst, lowpassed.
        ir_t = np.arange(int(0.3 * SR)) / SR
        ir = rng.standard_normal(len(ir_t)) * np.exp(-ir_t / 0.07)
        tail = signal.fftconvolve(snap + 0.5 * crack, ir)[:n] * 0.012
        tail = signal.sosfilt(signal.butter(2, 4500, "lowpass", fs=SR, output="sos"), tail)
        out[:, ch] = 0.9 * crack + 1.0 * snap + 1.1 * body + 1.4 * boom + tail
    out *= 1.0 / max(abs(out).max(), 1e-6)
    out = np.tanh(out * 1.25) / np.tanh(1.25)
    fade = int(0.1 * SR)
    out[-fade:] *= (np.linspace(1, 0, fade) ** 2)[:, None]
    return out * 0.85


def main():
    sr, data = load(sys.argv[1])
    os.makedirs("assets/sounds/smg", exist_ok=True)
    os.makedirs("assets/sounds/smg_synth", exist_ok=True)
    for i, onset in enumerate(SOURCE_SHOTS, start=1):
        save(f"assets/sounds/smg/smg_shot_{i}.wav", clean(sr, data, onset), sr)
        save(f"assets/sounds/smg_synth/smg_shot_{i}.wav", synth(i), SR)
    print("wrote assets/sounds/smg and assets/sounds/smg_synth")


if __name__ == "__main__":
    main()
