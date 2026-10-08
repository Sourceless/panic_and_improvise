#!/usr/bin/env python3
"""Cuts the reload, dry-fire and impact sounds from CC0 recordings on OpenGameArt.

Sources (all CC0; see assets/CREDITS.md):
  reload.wav                  https://opengameart.org/content/handgun-reload-sound-effect
                              magazine dropped (0.11 s), new magazine in (0.60 s), slide racked (1.05 s)
  clipload1.wav, clipload2.wav  https://opengameart.org/content/gun-reload-sound-effects
  bfh1_wood_hit_02, bfh1_hit_02, bfh1_wood_hit_01
                              https://opengameart.org/content/75-cc0-breaking-falling-hit-sfx
  thwack-02.wav, thwack-03.wav  https://opengameart.org/content/thwack-sounds

The 75-sound pack is .ogg; convert those to .wav first, e.g.
  gst-launch-1.0 -q filesrc location=x.ogg ! decodebin ! audioconvert ! audioresample \
      ! audio/x-raw,rate=48000,format=S16LE ! wavenc ! filesink location=x.wav

Outputs, all mono 48 kHz 16-bit, under assets/sounds/:
  gun/reload_swap.wav      magazine out and in (the part of the recording before the slide is racked)
  gun/reload_charge.wav    the whole recording, with the slide racked: for a gun whose bolt was forward
  gun/dry_click_{1,2}.wav  the trigger pulled with nothing to fire
  impact/dirt_{1,2,3}.wav  a bullet hitting soil: dull thumps
  impact/target_{1,2}.wav  a bullet hitting the target dummy: sharper knocks

Usage: python3 tools/make_gun_sounds.py "<directory of the .wav sources>"
"""
import os
import sys
import wave

import numpy as np
from scipy import signal
from scipy.io import wavfile

OUT = "assets/sounds"
RATE = 48000


def load(folder, name):
    sr, a = wavfile.read(f"{folder}/{name}.wav")
    a = a.astype(np.float64) / 32768.0
    if a.ndim > 1:
        a = a.mean(axis=1)
    if sr != RATE:
        a = signal.resample_poly(a, RATE, sr)
    return a


def cut(a, start, end, fade_in=0.002, fade_out=0.02, highpass=30, lowpass=None):
    seg = a[int(start * RATE):int(end * RATE)].copy()
    if highpass:
        seg = signal.sosfilt(signal.butter(2, highpass, "highpass", fs=RATE, output="sos"), seg)
    if lowpass:
        seg = signal.sosfilt(signal.butter(4, lowpass, "lowpass", fs=RATE, output="sos"), seg)
    n = max(1, int(fade_in * RATE))
    seg[:n] *= np.linspace(0, 1, n)
    n = max(1, int(fade_out * RATE))
    seg[-n:] *= np.linspace(1, 0, n) ** 2
    return seg


def save(path, seg, peak=0.9):
    seg = seg / (abs(seg).max() + 1e-9) * peak
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes((np.clip(seg, -1, 1) * 32767).astype(np.int16).tobytes())
    print(f"{path}  {len(seg) / RATE:.2f} s")


def main():
    folder = sys.argv[1].rstrip("/")
    reload = load(folder, "reload")
    # The slide is racked at about 1.05 s; the swap ends just before it.
    save(f"{OUT}/gun/reload_swap.wav", cut(reload, 0.0, 0.98, fade_out=0.04))
    save(f"{OUT}/gun/reload_charge.wav", cut(reload, 0.0, len(reload) / RATE, fade_out=0.05))

    # Clicks: the click of a magazine seating, trimmed to just the click.
    # Softened: a gentle low-pass takes the sharp edge off, and they are kept well below full scale
    # (an empty click should never startle).
    save(f"{OUT}/gun/dry_click_1.wav", cut(load(folder, "clipload1"), 0.095, 0.2, fade_out=0.03, lowpass=4500), peak=0.55)
    save(f"{OUT}/gun/dry_click_2.wav", cut(load(folder, "clipload2"), 0.095, 0.2, fade_out=0.03, lowpass=4500), peak=0.55)

    # Soil takes a bullet with a dull thump: the quietest low-passed knocks.
    save(f"{OUT}/impact/dirt_1.wav", cut(load(folder, "bfh1_wood_hit_02"), 0.0, 0.28, fade_out=0.06))
    save(f"{OUT}/impact/dirt_2.wav", cut(load(folder, "bfh1_hit_02"), 0.0, 0.40, fade_out=0.1, lowpass=1800))
    save(f"{OUT}/impact/dirt_3.wav", cut(load(folder, "thwack-03"), 0.0, 0.16, fade_out=0.05, lowpass=1500))
    # The target dummy is harder, so sharper.
    save(f"{OUT}/impact/target_1.wav", cut(load(folder, "bfh1_wood_hit_01"), 0.0, 0.25, fade_out=0.05))
    save(f"{OUT}/impact/target_2.wav", cut(load(folder, "thwack-02"), 0.0, 0.21, fade_out=0.05))


if __name__ == "__main__":
    main()
