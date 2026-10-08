#!/usr/bin/env python3
"""Draws a settlement layout (the JSON written by the `dump_a_settlement` diagnostic test) top-down.

  SETTLEMENT_DUMP=town.json cargo test --lib dump_a_settlement -- --ignored
  python3 tools/draw_settlement.py town.json town.png [pixels per metre]

Roads are drawn at their true width (main roads dark, lanes mid grey, farm tracks tan, footpaths thin and
brown); buildings as their footprints with a tick on the front; churchyards as circles; the settlement's
radius, and the fully flattened core (0.6 of it), as rings.
"""
import json
import math
import sys

from PIL import Image, ImageDraw

KIND = {"Major": (70, 70, 74), "Lane": (118, 118, 122), "Minor": (176, 150, 104), "Path": (150, 110, 70)}
BUILD = {"Cottage": (232, 214, 170), "House": (214, 170, 120), "Terrace": (200, 120, 100), "Church": (150, 150, 200),
         "Pub": (220, 170, 60), "Shop": (90, 170, 210), "School": (110, 200, 120), "Hall": (190, 130, 190),
         "PetrolStation": (230, 80, 80), "Farmhouse": (232, 214, 170), "Barn": (170, 130, 90), "Silo": (200, 200, 205),
         "Mill": (160, 160, 160)}


def main():
    data = json.load(open(sys.argv[1]))
    out = sys.argv[2]
    cx, cz = data["centre"]
    radius = data["radius"]
    scale = float(sys.argv[3]) if len(sys.argv) > 3 else max(2.0, min(10.0, 1100 / (radius * 2.6)))
    size = int(radius * 2.7 * scale)
    img = Image.new("RGB", (size, size), (104, 138, 74))
    draw = ImageDraw.Draw(img)

    def px(x, z):
        return ((x - cx) * scale + size / 2, (z - cz) * scale + size / 2)

    def ring(r, colour, width=1):
        x0, y0 = px(cx - r, cz - r)
        x1, y1 = px(cx + r, cz + r)
        draw.ellipse([x0, y0, x1, y1], outline=colour, width=width)

    ring(radius, (240, 240, 120), 2)
    ring(radius * 0.6, (200, 230, 120), 1)
    for y in data["yards"]:
        r = y[2]
        x0, y0 = px(y[0] - r, y[1] - r)
        x1, y1 = px(y[0] + r, y[1] + r)
        draw.ellipse([x0, y0, x1, y1], fill=(120, 160, 90), outline=(90, 130, 70))
    for road in sorted(data["roads"], key=lambda r: {"Minor": 0, "Path": 1, "Lane": 2, "Major": 3}.get(r["kind"], 0)):
        pts = [px(*p) for p in road["points"]]
        if len(pts) >= 2:
            draw.line(pts, fill=KIND.get(road["kind"], (0, 0, 0)), width=max(1, int(road["hw"] * 2 * scale)), joint="curve")
    for b in data["buildings"]:
        yaw = b["yaw"]
        c, s = math.cos(yaw), math.sin(yaw)
        w, d = b["w"] / 2, b["d"] / 2
        corners = []
        for lx, lz in [(-w, -d), (w, -d), (w, d), (-w, d)]:
            corners.append(px(b["c"][0] + lx * c + lz * s, b["c"][1] - lx * s + lz * c))
        draw.polygon(corners, fill=BUILD.get(b["kind"], (255, 0, 255)), outline=(40, 30, 20))
        # The front (local -Z) is marked with a short thick line.
        fx, fz = -s, -c
        a = px(b["c"][0] + fx * d, b["c"][1] + fz * d)
        e = px(b["c"][0] + fx * (d + 1.4), b["c"][1] + fz * (d + 1.4))
        draw.line([a, e], fill=(30, 30, 30), width=max(1, int(scale * 0.5)))
    lx, lz = px(*data["landmark"])
    draw.polygon([(lx, lz - 6), (lx + 5, lz + 4), (lx - 5, lz + 4)], fill=(255, 255, 255), outline=(0, 0, 0))
    x, z = px(cx, cz)
    draw.line([(x - 6, z), (x + 6, z)], fill=(255, 0, 0), width=2)
    draw.line([(x, z - 6), (x, z + 6)], fill=(255, 0, 0), width=2)
    img.save(out)
    print(f"{out}: {len(data['buildings'])} buildings, {len(data['roads'])} roads, {scale:.1f} px/m")


main()
