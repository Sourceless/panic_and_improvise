#!/usr/bin/env python3
"""Cuts the sounds of shells and clips being loaded from CC0 recordings on OpenGameArt.

Sources (all CC0; see assets/CREDITS.md):
  4 Shell Reload.mp3   in shotgunsounds.zip, https://opengameart.org/content/shotgun-reload-sound-effects (zer0_sol)
                       four shells pushed into a shotgun's tube, a second or so apart
  assaultriflereload1_0.wav  https://opengameart.org/content/gun-reload-sounds (SpringySpringo)
                       a rifle magazine going in (the last half second of the recording)

Convert the .mp3 to .wav first, e.g.
  gst-launch-1.0 -q filesrc location=x.mp3 ! decodebin ! audioconvert ! audioresample \
      ! audio/x-raw,rate=48000,channels=1,format=S16LE ! wavenc ! filesink location=x.wav

Outputs, all mono 48 kHz 16-bit, under assets/sounds/gun/:
  shell_{1,2,3,4}.wav   one shell going into a tube, the gate of a breech or the cylinder of a revolver
  clip_in.wav           a clip going into a rifle

Usage: PYTHONPATH=tools python3 tools/make_reload_sounds.py "<directory of the .wav sources>"
"""
import sys

from make_gun_sounds import OUT, cut, load, save


def main():
    folder = sys.argv[1].rstrip("/")
    four = load(folder, "four")
    # Each shell is a cluster of a few knocks; they begin about a second apart.
    for i, (a, b) in enumerate([(0.0, 0.85), (1.15, 2.05), (2.2, 3.1), (3.2, 4.2)], start=1):
        save(f"{OUT}/gun/shell_{i}.wav", cut(four, a, b, fade_out=0.08), peak=0.8)
    save(f"{OUT}/gun/clip_in.wav", cut(load(folder, "rifle_reload"), 0.9, 1.56, fade_out=0.1), peak=0.85)


if __name__ == "__main__":
    main()
