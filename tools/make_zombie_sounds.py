#!/usr/bin/env python3
"""Cuts the zombie groans, the player's hurt cries and the rifle bolt from CC0 recordings.

Nothing here is synthesised: every output is a cut of a real recording, trimmed, faded and levelled.

Sources (all CC0; see assets/CREDITS.md):
  moans.wav            https://opengameart.org/content/zombie-moans  (Darsycho; darsycho__zombie-moans.ogg)
  zombienoise1/2/3.wav, fastzombie1.wav
                       https://opengameart.org/content/zombie-noises-and-moans  (ianzazz; zombienoises.zip)
  hurt_01..06.wav      https://opengameart.org/content/hurt-sound-effects  (EZduzziteh; hurt_01_0.mp3 and hurt_02..06.mp3)
  bolt_mosin.wav       https://freesound.org/people/Zott820/sounds/370345/  (Zott820, "Mosin Nagant Bolt Action Cycle")

The .ogg and .mp3 sources are converted to mono 48 kHz .wav first, e.g.
  gst-launch-1.0 -q filesrc location=x.ogg ! decodebin ! audioconvert ! audioresample \
      ! audio/x-raw,rate=48000,channels=1,format=S16LE ! wavenc ! filesink location=x.wav
(for the Freesound ones the high-quality preview, as the originals need an account).

Outputs, all mono 48 kHz 16-bit, under assets/sounds/:
  zombie/groan_{1..5}.wav    a groan or moan, for an ordinary zombie and (lowered) a big one
  zombie/growl.wav           a short snarl, for a fast one
  player/hurt_{1..6}.wav     a cry of pain when the player is hit
  gun/bolt_cycle_{1,2,3}.wav a bolt-action rifle's bolt lifted, drawn back, pushed home and turned down

Usage: python3 tools/make_zombie_sounds.py "<directory of the .wav sources>"
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


def cut(a, start, end, peak=0.85, fade_in=0.005, fade_out=0.05, highpass=40):
    """The part of `a` from `start` to `end` seconds, high-passed, faded at both ends and levelled to `peak`."""
    seg = a[int(start * RATE):int(end * RATE)].copy()
    if highpass:
        seg = signal.sosfilt(signal.butter(2, highpass, "highpass", fs=RATE, output="sos"), seg)
    n = max(1, int(fade_in * RATE))
    seg[:n] *= np.linspace(0, 1, n)
    n = max(1, int(fade_out * RATE))
    seg[-n:] *= np.linspace(1, 0, n) ** 2
    top = np.abs(seg).max()
    return seg * (peak / top) if top > 0 else seg


def save(path, a):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    pcm = (np.clip(a, -1, 1) * 32767).astype(np.int16)
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(pcm.tobytes())
    print(f"{path}: {len(a) / RATE:.2f} s")


def main(folder):
    # Groans: three from the long recording of several, two from the other pack (which is recorded quietly).
    moans = load(folder, "moans")
    save(f"{OUT}/zombie/groan_1.wav", cut(moans, 0.22, 2.96, fade_out=0.25))
    save(f"{OUT}/zombie/groan_2.wav", cut(moans, 3.66, 6.26, fade_out=0.25))
    save(f"{OUT}/zombie/groan_3.wav", cut(moans, 6.62, 8.90, fade_out=0.25))
    for i, name in ((4, "zombienoise1"), (5, "zombienoise2")):
        a = load(folder, name)
        save(f"{OUT}/zombie/groan_{i}.wav", cut(a, 0.0, len(a) / RATE, peak=0.8, fade_out=0.15))
    fast = load(folder, "fastzombie1")
    save(f"{OUT}/zombie/growl.wav", cut(fast, 0.0, len(fast) / RATE, peak=0.85, fade_out=0.04))
    # Hurt cries: some were recorded far quieter than others, so all are brought to a level.
    for i, name in enumerate(["hurt_01_0", "hurt_02", "hurt_03", "hurt_04", "hurt_05", "hurt_06"], start=1):
        a = load(folder, name)
        save(f"{OUT}/player/hurt_{i}.wav", cut(a, 0.0, len(a) / RATE, peak=0.85, fade_out=0.03))
    # The bolt: three whole cycles of the heavier rifle's, which suit an old military bolt-action.
    mosin = load(folder, "bolt_mosin")
    for i, (start, end) in enumerate([(0.10, 1.34), (1.82, 3.02), (5.76, 6.96)], start=1):
        save(f"{OUT}/gun/bolt_cycle_{i}.wav", cut(mosin, start, end, peak=0.8, fade_out=0.08))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
