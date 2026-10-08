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

`sounds/smg/smg_shot_*.wav` are derived from the CZ-52 recording in the "Gunshot Sounds" pack by
Vincent Sevedge on [OpenGameArt](https://opengameart.org/content/gunshot-sounds). The OpenGameArt
page lists the pack as CC0, but the licence file inside the download says **Creative Commons
Attribution 3.0** ("Copyright (c) 2009 Vincent Sevedge"), so it is credited here to be safe.
`tools/make_shot_sounds.py` cuts three individual shots out of the recording, removes the
rumble below 40 Hz and shortens the room echo, and nothing more. The same script also builds
`sounds/smg_synth/` entirely from filtered noise (original work, no licence issues), which can be
tried with `FPS_SHOT_SOUNDS=smg_synth`.

`sounds/gunshots/*.wav` are an earlier CC0 pack from OpenGameArt ("Gunshots") and are no longer
used by the gun.

## Reference photographs

The gun model was drawn from public-domain US Navy museum photographs of a Sterling L2A3
(Wikimedia Commons, "Submachine Gun, 9mm, L2A3, Sterling, British, S-N UF57A5347
(NHHC 2002-11-2)"). The photographs are measurements only; none of their pixels are in the game.

The magazine's size and position were measured from photographs of Sterling L2A2/L2A3 guns in
the Royal Armouries' collection (collections.armouries.net, object numbers PR.1434 and the
Sterling L2A1/L2A2 entries), viewed for reference only. They are the Royal Armouries' copyright
and are not included in this repository or in the game.
