# Asset credits

Textures in `textures/pbr/` are from [ambientCG](https://ambientcg.com), released under
CC0 1.0 (public domain). They are 1K colour maps, converted to JPG; the foliage atlases
(`leaf_*.png`, `blades.png`) have their opacity map merged into the alpha channel.

| File | ambientCG asset |
| --- | --- |
| dirt.jpg | Ground106 |
| stone.jpg | Rock062 |
| soil_plough.jpg | Ground048 |
| soil_loam.jpg | Ground103 |
| meadow.jpg | Grass004 |
| pasture.jpg | Grass007 |
| forest_conifer.jpg | Ground077 |
| forest_broadleaf.jpg | ScatteredLeaves009 |
| gravel.jpg | Gravel041 |
| asphalt.jpg | Asphalt004 |
| sand.jpg | Ground054 |
| bark_oak.jpg | Bark001 |
| bark_birch.jpg | Bark004 |
| bark_conifer.jpg | Bark012 |
| leaf_beech.png | LeafSet024 |
| leaf_willow.png | LeafSet022 |
| leaf_maple.png | LeafSet027 |
| leaf_conifer.png | LeafSet019 |
| leaf_lime.png | LeafSet023 |
| blades.png | Foliage001 |

## Normal maps

Every ground layer and bark texture has a matching `<name>_n.jpg`: the OpenGL-convention
normal map (the `NormalGL` map) from the same ambientCG asset as its colour map above.

## Derived textures

`textures/veg/` holds foliage cluster cards and a hedge tile baked from the leaf atlases
above by `tools/bake_foliage.py` (so they are CC0 derivatives of the ambientCG sets
LeafSet019/022/023/024/027). Rerun the script to regenerate them.

## Sounds

`sounds/smg/smg_shot_*.wav` are cut from [The Free Firearm Sound Library](https://opengameart.org/content/the-free-firearm-sound-library):
real close-miked recordings of a Carl Gustav M45 ("Swedish K") 9 mm submachine gun. The library
is released as **CC0** ("no rights reserved") by its creators. `tools/make_shot_sounds.py` cuts
three single shots, resamples them to 48 kHz and applies a 30 Hz high-pass and a fade; nothing
else is done to them.

`sounds/gunshots/*.wav` are an earlier CC0 pack from OpenGameArt ("Gunshots") and are no longer
used by the gun, except as the `FPS_SHOT_SOUNDS=old` comparison.

The reload, empty-click and impact sounds are cut from other CC0 recordings on OpenGameArt by
`tools/make_gun_sounds.py` (trimmed, fades, a high-pass, and a gentle low-pass on the clicks and two
of the thumps; the reload is slowed slightly in the game):

| Files | Source |
| --- | --- |
| `sounds/gun/reload_*.wav` | [Handgun Reload Sound Effect](https://opengameart.org/content/handgun-reload-sound-effect) |
| `sounds/gun/dry_click_*.wav` | [Gun Reload Sound Effects](https://opengameart.org/content/gun-reload-sound-effects) (clipload1, clipload2) |
| `sounds/impact/dirt_1,2.wav`, `target_1.wav`, `stone_*.wav` | [75 CC0 breaking / falling / hit sfx](https://opengameart.org/content/75-cc0-breaking-falling-hit-sfx) |
| `sounds/impact/dirt_3.wav`, `target_2.wav` | [Thwack Sounds](https://opengameart.org/content/thwack-sounds) |
| `sounds/impact/metal_*.wav` | [Metal Impact Sounds](https://opengameart.org/content/metal-impact-sounds) (clink2, clink3) |

All of these are listed on OpenGameArt as CC0; the Thwack Sounds archive includes a CC0 licence file.

## Reference photographs

The gun model was drawn from public-domain US Navy museum photographs of a Sterling L2A3
(Wikimedia Commons, "Submachine Gun, 9mm, L2A3, Sterling, British, S-N UF57A5347
(NHHC 2002-11-2)"). The photographs are measurements only; none of their pixels are in the game.

The magazine's size and position were measured from photographs of Sterling L2A2/L2A3 guns in
the Royal Armouries' collection (collections.armouries.net, object numbers PR.1434 and the
Sterling L2A1/L2A2 entries), viewed for reference only. They are the Royal Armouries' copyright
and are not included in this repository or in the game.

`sounds/footsteps/*.wav` and `sounds/impact/splash_*.wav` are cut by `tools/cut_footsteps.py` from three CC0 packs on OpenGameArt:
[Different steps on wood, stone, leaves, gravel and mud](https://opengameart.org/content/different-steps-on-wood-stone-leaves-gravel-and-mud) (TinyWorlds),
[Fantozzi's Footsteps (Grass/Sand & Stone)](https://opengameart.org/content/fantozzis-footsteps-grasssand-stone) (Fantozzi, via qubodup) and
[Water Splash and sand footsteps](https://opengameart.org/content/water-splash-and-sand-footsteps) (Peludo).
