#!/usr/bin/env python3
"""Cuts the sounds of a window breaking from CC0 recordings on OpenGameArt.

Sources (both CC0; see assets/CREDITS.md):
  bfh1_glass_breaking_01..06.wav, bfh1_glass_falling_01.wav
                       https://opengameart.org/content/75-cc0-breaking-falling-hit-sfx
  glass_breaking.wav   https://opengameart.org/content/glass-break  (Till Behrend)

The 75-sound pack is .ogg; convert those to .wav first, e.g.
  gst-launch-1.0 -q filesrc location=x.ogg ! decodebin ! audioconvert ! audioresample \
      ! audio/x-raw,rate=48000,channels=1,format=S16LE ! wavenc ! filesink location=x.wav

Outputs, all mono 48 kHz 16-bit, under assets/sounds/glass/:
  break_{1,2,3,4}.wav   a pane smashed by a body going through it: the crash and the fall of the pieces
  shot_{1,2,3}.wav      a pane hit by a bullet: shorter and sharper
  tinkle.wav            pieces still falling a moment later

Usage: python3 tools/make_glass_sounds.py "<directory of the .wav sources>"
"""
import sys

import numpy as np

from make_gun_sounds import OUT, RATE, cut, load, save


def main():
    folder = sys.argv[1].rstrip("/")
    save(f"{OUT}/glass/break_1.wav", cut(load(folder, "bfh1_glass_breaking_03"), 0.0, 0.78, fade_out=0.15))
    save(f"{OUT}/glass/break_2.wav", cut(load(folder, "bfh1_glass_breaking_04"), 0.0, 0.76, fade_out=0.15))
    save(f"{OUT}/glass/break_3.wav", cut(load(folder, "bfh1_glass_breaking_06"), 0.0, 0.92, fade_out=0.2))
    # The longest recording, only its crash and the first of the pieces falling.
    save(f"{OUT}/glass/break_4.wav", cut(load(folder, "glass_breaking"), 0.0, 1.3, fade_out=0.3))
    save(f"{OUT}/glass/shot_1.wav", cut(load(folder, "bfh1_glass_breaking_01"), 0.0, 0.5, fade_out=0.12))
    save(f"{OUT}/glass/shot_2.wav", cut(load(folder, "bfh1_glass_breaking_02"), 0.0, 0.39, fade_out=0.1))
    save(f"{OUT}/glass/shot_3.wav", cut(load(folder, "bfh1_glass_breaking_05"), 0.0, 0.44, fade_out=0.1))
    save(f"{OUT}/glass/tinkle.wav", cut(load(folder, "bfh1_glass_falling_01"), 0.0, 0.43, fade_out=0.1), peak=0.6)


if __name__ == "__main__":
    main()
