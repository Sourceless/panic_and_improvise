"""Draws horizontal slices of a building model dumped by the `dump_a_building` test.

    python3 -I tools/draw_building.py building.json out.png [y ...]

Each slice is the building as it is cut at that height above the floor: solid things that reach it are
drawn in their own colour, with the floor and anything lower paler beneath.
"""
import json
import math
import sys

from PIL import Image, ImageDraw

SCALE = 40  # pixels per metre


def corners(item):
    cx, _, cz = item["centre"]
    sx, _, sz = item["size"]
    yaw = item["yaw"]
    c, s = math.cos(yaw), math.sin(yaw)
    pts = []
    for dx, dz in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
        x, z = dx * sx / 2, dz * sz / 2
        pts.append((cx + x * c + z * s, cz - x * s + z * c))
    return pts


def main():
    data = json.load(open(sys.argv[1]))
    out = sys.argv[2]
    ys = [float(v) for v in sys.argv[3:]] or [1.0]
    items = [i for i in data["items"] if i["kind"] == "block"]
    xs = [p[0] for i in items for p in corners(i)]
    zs = [p[1] for i in items for p in corners(i)]
    x0, x1, z0, z1 = min(xs) - 1, max(xs) + 1, min(zs) - 1, max(zs) + 1
    w, h = int((x1 - x0) * SCALE), int((z1 - z0) * SCALE)
    panels = []
    for y in ys:
        img = Image.new("RGB", (w, h), (30, 30, 34))
        d = ImageDraw.Draw(img)
        # Things below the cut, then the things the cut goes through.
        for i in sorted(items, key=lambda i: i["centre"][1] + i["size"][1] / 2):
            bottom, top = i["centre"][1] - i["size"][1] / 2, i["centre"][1] + i["size"][1] / 2
            if bottom > y or top < y - 0.9 and top > y:
                continue
            cut = bottom <= y <= top
            if not cut and top < y - 1.2:
                continue
            col = tuple(int(255 * min(1, max(0, v))) for v in i["colour"])
            if not cut:
                col = tuple(int(c * 0.45 + 20) for c in col)
            pts = [((px - x0) * SCALE, (pz - z0) * SCALE) for px, pz in corners(i)]
            d.polygon(pts, fill=col, outline=(0, 0, 0) if cut else None)
        d.text((6, 4), f"{data['building']}  slice at y={y}", fill=(255, 255, 255))
        # Front is at the top (-z).
        d.text((w // 2 - 20, 16), "FRONT", fill=(255, 220, 120))
        panels.append(img)
    sheet = Image.new("RGB", (sum(p.width for p in panels) + 8 * (len(panels) - 1), panels[0].height), (0, 0, 0))
    x = 0
    for p in panels:
        sheet.paste(p, (x, 0))
        x += p.width + 8
    sheet.save(out)
    print(out, sheet.size)


main()
