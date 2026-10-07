#!/usr/bin/env python3
"""Bakes foliage cluster textures from the CC0 ambientCG leaf atlases in assets/textures/pbr.

Each output is a card showing a dense cluster of real leaves (or conifer sprays), with an
alpha channel, so a tree can be built from a few hundred cards instead of tens of thousands
of single leaves. Deterministic: rerun to regenerate. Needs Pillow, numpy and scipy.

    python3 tools/bake_foliage.py
"""
import os
import random

import numpy as np
from PIL import Image, ImageEnhance
from scipy import ndimage

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "textures")
PBR = os.path.join(ROOT, "pbr")
OUT = os.path.join(ROOT, "veg")
os.makedirs(OUT, exist_ok=True)
SIZE = 512


def load_sprites(name, min_area=900):
    """Cuts the individual leaves out of an atlas by connected alpha components."""
    atlas = Image.open(os.path.join(PBR, name)).convert("RGBA")
    alpha = np.array(atlas)[:, :, 3]
    labels, count = ndimage.label(alpha > 40)
    sprites = []
    for i, sl in enumerate(ndimage.find_objects(labels), start=1):
        if sl is None:
            continue
        mask = labels[sl] == i
        if mask.sum() < min_area:
            continue
        crop = atlas.crop((sl[1].start, sl[0].start, sl[1].stop, sl[0].stop))
        # Keep only this component's pixels (neighbouring leaves can overlap the box).
        a = np.array(crop)
        a[:, :, 3] = np.where(mask, a[:, :, 3], 0)
        sprites.append(Image.fromarray(a))
    return sprites


def paste(canvas, sprite, centre, angle, scale, shade):
    s = sprite.resize((max(2, int(sprite.width * scale)), max(2, int(sprite.height * scale))), Image.LANCZOS)
    s = s.rotate(angle, expand=True, resample=Image.BICUBIC)
    s = ImageEnhance.Brightness(s.convert("RGBA")).enhance(shade)
    # ImageEnhance drops alpha handling on some versions; restore it explicitly.
    s.putalpha(sprite_alpha(s, sprite, scale, angle))
    x, y = int(centre[0] - s.width / 2), int(centre[1] - s.height / 2)
    layer = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    layer.paste(s, (x, y), s)
    canvas.alpha_composite(layer)


def sprite_alpha(enhanced, original, scale, angle):
    a = original.getchannel("A").resize(
        (max(2, int(original.width * scale)), max(2, int(original.height * scale))), Image.LANCZOS
    ).rotate(angle, expand=True, resample=Image.BICUBIC)
    return a


def leafy_cluster(atlas, seed, count, leaf_px, spread=(0.42, 0.40), outward=True):
    """A roundish clump of leaves; leaves near the bottom and centre are shaded darker."""
    rng = random.Random(seed)
    sprites = load_sprites(atlas)
    canvas = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    placed = []
    for _ in range(count):
        r = rng.random() ** 0.6
        th = rng.random() * 6.2832
        x = SIZE / 2 + np.cos(th) * r * spread[0] * SIZE
        y = SIZE / 2 + np.sin(th) * r * spread[1] * SIZE
        placed.append((y, x))
    # Paint back to front so lower leaves sit behind.
    for y, x in sorted(placed, key=lambda p: -p[0]):
        sprite = rng.choice(sprites)
        scale = leaf_px * rng.uniform(0.8, 1.2) / max(sprite.width, sprite.height)
        angle = rng.uniform(0, 360)
        # Darker toward the bottom and the middle (self-shadowing), lighter at the rim/top.
        depth = 1.0 - np.hypot(x - SIZE / 2, y - SIZE / 2) / (SIZE * 0.5)
        shade = 0.62 + 0.55 * (1.0 - y / SIZE) * 0.6 + 0.3 * (1 - max(0.0, depth)) + rng.uniform(-0.08, 0.08)
        paste(canvas, sprite, (x, y), angle, scale, float(np.clip(shade, 0.45, 1.2)))
    return canvas


def conifer_card(atlas, seed, sprays):
    """A fir branch: sprays fanned out either side of a central stem, tips pointing out."""
    rng = random.Random(seed)
    sprites = load_sprites(atlas, min_area=1500)
    canvas = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    placed = []
    for i in range(sprays):
        t = i / (sprays - 1)
        y = SIZE * (0.08 + 0.84 * t)
        side = 1 if i % 2 == 0 else -1
        reach = SIZE * (0.12 + 0.34 * np.sin(np.pi * (0.12 + 0.88 * (1 - t))))
        placed.append((y, SIZE / 2 + side * reach * 0.55, 90 - side * rng.uniform(10, 40) + (0 if side > 0 else 180)))
    for y, x, angle in sorted(placed, key=lambda p: p[0]):
        sprite = rng.choice(sprites)
        scale = SIZE * rng.uniform(0.34, 0.5) / max(sprite.width, sprite.height)
        shade = 0.6 + 0.5 * (1 - y / SIZE) + rng.uniform(-0.06, 0.06)
        # Sprays in the atlas point right; rotate so they sweep out from the stem.
        paste(canvas, sprite, (x, y), angle - 90 if x > SIZE / 2 else angle - 90, scale, float(np.clip(shade, 0.45, 1.15)))
    return canvas


def grass_tuft(atlas, seed, blades):
    """A tuft of grass: blades fanning up and out from a point at the bottom centre of the card."""
    rng = random.Random(seed)
    sprites = load_sprites(atlas, min_area=250)
    canvas = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    for _ in range(blades):
        sprite = rng.choice(sprites)
        length = rng.uniform(0.55, 0.97) * SIZE
        scale = length / max(sprite.width, sprite.height)
        s = sprite.resize((max(2, int(sprite.width * scale)), max(2, int(sprite.height * scale))), Image.LANCZOS)
        # Blades in the atlas can lie either way up; stand them with their longer side vertical.
        if s.width > s.height:
            s = s.rotate(90, expand=True)
        root = (SIZE / 2 + rng.uniform(-28, 28), SIZE - 6)
        layer = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
        layer.paste(s, (int(root[0] - s.width / 2), int(root[1] - s.height)), s)
        angle = rng.uniform(-34, 34) * (0.4 + abs(root[0] - SIZE / 2) / 28.0)
        layer = layer.rotate(angle, center=root, resample=Image.BICUBIC)
        shade = rng.uniform(0.75, 1.1)
        layer = ImageEnhance.Brightness(layer.convert("RGBA")).enhance(shade)
        layer.putalpha(layer.getchannel("A"))
        canvas.alpha_composite(layer)
    return canvas


def opaque_hedge(atlas, seed):
    """A seamless-ish dense leaf texture for the faces of hedges (no transparency)."""
    base = leafy_cluster(atlas, seed, 520, 70, spread=(0.62, 0.62))
    bg = Image.new("RGBA", (SIZE, SIZE), (22, 40, 16, 255))
    bg.alpha_composite(base)
    # Wrap-around pass so the tile repeats without a visible seam.
    out = bg.copy()
    for dx, dy in ((SIZE, 0), (-SIZE, 0), (0, SIZE), (0, -SIZE)):
        out.alpha_composite(base.transform(base.size, Image.AFFINE, (1, 0, -dx, 0, 1, -dy)))
    return out.convert("RGB")


def main():
    save = lambda im, n: im.save(os.path.join(OUT, n), optimize=True)
    save(leafy_cluster("leaf_beech.png", 1, 160, 120), "cluster_beech.png")
    save(leafy_cluster("leaf_maple.png", 2, 110, 150), "cluster_maple.png")
    save(leafy_cluster("leaf_willow.png", 3, 170, 125), "cluster_willow.png")
    save(leafy_cluster("leaf_lime.png", 4, 130, 135), "cluster_lime.png")
    save(conifer_card("leaf_conifer.png", 5, 14), "cluster_conifer.png")
    save(grass_tuft("blades.png", 7, 16), "grass_tuft.png")
    opaque_hedge("leaf_beech.png", 6).save(os.path.join(OUT, "hedge.jpg"), quality=88)
    for f in sorted(os.listdir(OUT)):
        print(f, os.path.getsize(os.path.join(OUT, f)) // 1024, "KB")


if __name__ == "__main__":
    main()
