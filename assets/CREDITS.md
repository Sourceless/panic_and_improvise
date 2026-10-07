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
