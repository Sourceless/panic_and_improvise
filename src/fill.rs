
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::map::{TerrainMap, CELL};
use crate::terrain::TerrainRoot;
use crate::zones::{Zone, ZoneMap};

const TREE_SPACING: f32 = 16.0;
const FIELD_LIFT: f32 = 0.15;
const SHED_SPACING: f32 = 60.0;

pub fn spawn_fill(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    map: &TerrainMap,
    zones: &ZoneMap,
) {
    spawn_trees(commands, meshes, materials, map, zones);
    spawn_field_tiling(commands, meshes, materials, map, zones);
    spawn_sheds(commands, meshes, materials, map, zones);
}

const PLOT_SIZE: f32 = 100.0;
const PLOT_SUBDIVISIONS: usize = 4;
const PLOT_MAX_RELIEF: f32 = 14.0;
const FARM_YARD_RADIUS: f32 = 42.0;

// Tiles the whole farmland zone (Arable and Pasture) into square fields, each bordered
// by a hedge, stone wall or fence. Farmyards around each farm's buildings are left clear.
fn spawn_field_tiling(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    map: &TerrainMap,
    zones: &ZoneMap,
) {
    let farms: Vec<Vec2> = map
        .pois
        .iter()
        .filter(|p| p.kind == crate::map::PoiKind::Farm)
        .map(|p| p.position)
        .collect();

    let mut positions = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    let mut boundaries: [Vec<Vec2>; 3] = [Vec::new(), Vec::new(), Vec::new()];

    let half_map = crate::map::HALF_SIZE;
    let cols = (crate::map::MAP_SIZE / PLOT_SIZE) as i32;
    for pz in 0..cols {
        for px in 0..cols {
            let centre = Vec2::new(
                -half_map + (px as f32 + 0.5) * PLOT_SIZE,
                -half_map + (pz as f32 + 0.5) * PLOT_SIZE,
            );
            let (cx, cz) = nearest_cell(map, centre);
            let zone = zones.zone_at(cx, cz);
            if zone != Zone::Arable && zone != Zone::Pasture {
                continue;
            }
            if farms.iter().any(|f| f.distance(centre) < FARM_YARD_RADIUS) {
                continue;
            }
            if !plot_is_suitable(map, centre) {
                continue;
            }
            let block = (px.div_euclid(2), pz.div_euclid(2));
            let colour = if zone == Zone::Arable {
                crop_colour(block.0 as usize, block.1 as usize)
            } else {
                pasture_colour(block.0 as usize, block.1 as usize)
            };
            add_plot_grid(map, centre, colour, &mut positions, &mut colors, &mut indices);

            let kind = (hash01(px as usize, pz as usize, 21) * 3.0) as usize;
            boundaries[kind.min(2)].push(centre);
        }
    }

    if !positions.is_empty() {
        let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
            .with_inserted_indices(Indices::U32(indices));
        commands.spawn((
            TerrainRoot,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 1.0,
                ..default()
            })),
        ));
    }

    let specs = [
        (leaf_material([0.18, 0.38, 0.16]), 1.6, 1.9),
        (
            StandardMaterial {
                base_color: Color::srgb(0.66, 0.64, 0.6),
                ..default()
            },
            0.6,
            1.1,
        ),
        (
            StandardMaterial {
                base_color: Color::srgb(0.42, 0.30, 0.18),
                ..default()
            },
            0.18,
            1.2,
        ),
    ];
    for (centres, (material, thickness, height)) in boundaries.into_iter().zip(specs) {
        if centres.is_empty() {
            continue;
        }
        let mat = materials.add(material);
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut indices = Vec::new();
        let half = PLOT_SIZE * 0.5;
        for centre in centres {
            for (offset, half_extent) in [
                (Vec2::new(0.0, -half), Vec3::new(half, height / 2.0, thickness / 2.0)),
                (Vec2::new(0.0, half), Vec3::new(half, height / 2.0, thickness / 2.0)),
                (Vec2::new(-half, 0.0), Vec3::new(thickness / 2.0, height / 2.0, half)),
                (Vec2::new(half, 0.0), Vec3::new(thickness / 2.0, height / 2.0, half)),
            ] {
                let mid = centre + offset;
                let ground = map.height_at(mid);
                let base = Vec3::new(mid.x, ground + height / 2.0, mid.y);
                push_box(&mut positions, &mut normals, &mut indices, base, half_extent);
            }
        }
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_indices(Indices::U32(indices));
        commands.spawn((TerrainRoot, Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat)));
    }
}

fn push_box(positions: &mut Vec<[f32; 3]>, normals: &mut Vec<[f32; 3]>, indices: &mut Vec<u32>, centre: Vec3, half: Vec3) {
    let (hx, hy, hz) = (half.x, half.y, half.z);
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        ([1.0, 0.0, 0.0], [[hx, -hy, -hz], [hx, -hy, hz], [hx, hy, hz], [hx, hy, -hz]]),
        ([-1.0, 0.0, 0.0], [[-hx, -hy, hz], [-hx, -hy, -hz], [-hx, hy, -hz], [-hx, hy, hz]]),
        ([0.0, 1.0, 0.0], [[-hx, hy, -hz], [hx, hy, -hz], [hx, hy, hz], [-hx, hy, hz]]),
        ([0.0, -1.0, 0.0], [[-hx, -hy, hz], [hx, -hy, hz], [hx, -hy, -hz], [-hx, -hy, -hz]]),
        ([0.0, 0.0, 1.0], [[hx, -hy, hz], [-hx, -hy, hz], [-hx, hy, hz], [hx, hy, hz]]),
        ([0.0, 0.0, -1.0], [[-hx, -hy, -hz], [hx, -hy, -hz], [hx, hy, -hz], [-hx, hy, -hz]]),
    ];
    for (normal, corners) in faces {
        let base = positions.len() as u32;
        for corner in corners {
            positions.push([centre.x + corner[0], centre.y + corner[1], centre.z + corner[2]]);
            normals.push(normal);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn plot_is_suitable(map: &TerrainMap, centre: Vec2) -> bool {
    let half = PLOT_SIZE * 0.5;
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for k in 0..=PLOT_SUBDIVISIONS {
        for m in 0..=PLOT_SUBDIVISIONS {
            let p = centre
                + Vec2::new(
                    -half + k as f32 * PLOT_SIZE / PLOT_SUBDIVISIONS as f32,
                    -half + m as f32 * PLOT_SIZE / PLOT_SUBDIVISIONS as f32,
                );
            let (ix, iz) = nearest_cell(map, p);
            if map.water_level(ix, iz).is_some() {
                return false;
            }
            let h = map.height_at(p);
            lo = lo.min(h);
            hi = hi.max(h);
        }
    }
    hi - lo <= PLOT_MAX_RELIEF
}

fn add_plot_grid(
    map: &TerrainMap,
    centre: Vec2,
    colour: [f32; 3],
    positions: &mut Vec<[f32; 3]>,
    colors: &mut Vec<[f32; 4]>,
    indices: &mut Vec<u32>,
) {
    let half = PLOT_SIZE * 0.5;
    let step = PLOT_SIZE / PLOT_SUBDIVISIONS as f32;
    let c = [colour[0], colour[1], colour[2], 1.0];
    let n = PLOT_SUBDIVISIONS + 1;
    let base = positions.len() as u32;
    for m in 0..n {
        for k in 0..n {
            let p = centre + Vec2::new(-half + k as f32 * step, -half + m as f32 * step);
            positions.push([p.x, map.height_at(p) + FIELD_LIFT, p.y]);
            colors.push(c);
        }
    }
    for m in 0..PLOT_SUBDIVISIONS {
        for k in 0..PLOT_SUBDIVISIONS {
            let a = base + (m * n + k) as u32;
            let b = a + 1;
            let d = a + n as u32;
            let e = d + 1;
            indices.extend_from_slice(&[a, d, b, b, d, e]);
        }
    }
}

fn pasture_colour(bx: usize, bz: usize) -> [f32; 3] {
    let shades = [[0.52, 0.80, 0.40], [0.58, 0.84, 0.46], [0.47, 0.76, 0.38]];
    let pick = (hash01(bx, bz, 15) * shades.len() as f32) as usize;
    shades[pick.min(shades.len() - 1)]
}

fn spawn_trees(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    map: &TerrainMap,
    zones: &ZoneMap,
) {
    let broadleaf = meshes.add(Sphere::new(1.0));
    let conifer = meshes.add(Cone::new(1.0, 1.0));
    let broadleaf_mats: Vec<Handle<StandardMaterial>> = [
        [0.20, 0.44, 0.18],
        [0.26, 0.50, 0.20],
        [0.16, 0.36, 0.16],
    ]
    .iter()
    .map(|c| materials.add(leaf_material(*c)))
    .collect();
    let conifer_mats: Vec<Handle<StandardMaterial>> = [[0.10, 0.27, 0.22], [0.13, 0.32, 0.25]]
        .iter()
        .map(|c| materials.add(leaf_material(*c)))
        .collect();
    let orchard_mat = materials.add(leaf_material([0.46, 0.66, 0.26]));

    let steps = (crate::map::MAP_SIZE / TREE_SPACING) as usize;
    for iz in 0..steps {
        for ix in 0..steps {
            let jitter_x = hash01(ix, iz, 1) * 2.0 - 1.0;
            let jitter_z = hash01(ix, iz, 2) * 2.0 - 1.0;
            let p = Vec2::new(
                -crate::map::HALF_SIZE + (ix as f32 + 0.5) * TREE_SPACING + jitter_x * TREE_SPACING * 0.4,
                -crate::map::HALF_SIZE + (iz as f32 + 0.5) * TREE_SPACING + jitter_z * TREE_SPACING * 0.4,
            );
            let (vx, vz) = nearest_cell(map, p);
            let zone = zones.zone_at(vx, vz);
            let roll = hash01(ix, iz, 3);
            let ground = map.height_at(p);
            let size = 0.8 + hash01(ix, iz, 4) * 0.6;
            let pick = (hash01(ix, iz, 5) * 2.0) as usize;
            match zone {
                Zone::Woodland if roll < 0.55 => {
                    let r = 3.2 * size;
                    spawn_tree(commands, broadleaf.clone(), broadleaf_mats[pick % 3].clone(), p, ground, Vec3::new(r, r, r), ground + r + 1.5);
                }
                Zone::Conifer if roll < 0.65 => {
                    let h = 9.0 * size;
                    spawn_tree(commands, conifer.clone(), conifer_mats[pick % 2].clone(), p, ground, Vec3::new(2.6 * size, h, 2.6 * size), ground + h / 2.0);
                }
                Zone::Orchard if roll < 0.35 => {
                    let r = 2.2 * size;
                    spawn_tree(commands, broadleaf.clone(), orchard_mat.clone(), p, ground, Vec3::new(r, r, r), ground + r + 1.0);
                }
                _ => {}
            }
        }
    }
}

fn spawn_tree(
    commands: &mut Commands,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
    p: Vec2,
    _ground: f32,
    scale: Vec3,
    y: f32,
) {
    commands.spawn((
        TerrainRoot,
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_xyz(p.x, y, p.y).with_scale(scale),
    ));
}

fn leaf_material(c: [f32; 3]) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgb(c[0], c[1], c[2]),
        perceptual_roughness: 0.9,
        ..default()
    }
}

fn crop_colour(bx: usize, bz: usize) -> [f32; 3] {
    let crops = [
        [0.86, 0.72, 0.34],
        [0.80, 0.76, 0.46],
        [0.93, 0.86, 0.26],
        [0.56, 0.46, 0.30],
    ];
    let pick = (hash01(bx, bz, 7) * crops.len() as f32) as usize;
    crops[pick.min(crops.len() - 1)]
}

fn spawn_sheds(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    map: &TerrainMap,
    zones: &ZoneMap,
) {
    let shed = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let wall = materials.add(StandardMaterial {
        base_color: Color::srgb(0.62, 0.62, 0.64),
        ..default()
    });
    let steps = (crate::map::MAP_SIZE / SHED_SPACING) as usize;
    for iz in 0..steps {
        for ix in 0..steps {
            let p = Vec2::new(
                -crate::map::HALF_SIZE + (ix as f32 + 0.5) * SHED_SPACING,
                -crate::map::HALF_SIZE + (iz as f32 + 0.5) * SHED_SPACING,
            );
            let (vx, vz) = nearest_cell(map, p);
            if zones.zone_at(vx, vz) != Zone::Industrial || hash01(ix, iz, 9) > 0.3 {
                continue;
            }
            let size = Vec3::new(
                22.0 + hash01(ix, iz, 10) * 16.0,
                7.0 + hash01(ix, iz, 11) * 4.0,
                14.0 + hash01(ix, iz, 12) * 12.0,
            );
            let ground = map.height_at(p);
            commands.spawn((
                TerrainRoot,
                Mesh3d(shed.clone()),
                MeshMaterial3d(wall.clone()),
                Transform::from_xyz(p.x, ground + size.y / 2.0, p.y).with_scale(size),
            ));
        }
    }
}

fn nearest_cell(map: &TerrainMap, p: Vec2) -> (usize, usize) {
    let n = map.grid_size();
    let half = crate::map::HALF_SIZE;
    let ix = ((p.x + half) / CELL).round().clamp(0.0, (n - 1) as f32) as usize;
    let iz = ((p.y + half) / CELL).round().clamp(0.0, (n - 1) as f32) as usize;
    (ix, iz)
}

fn hash01(ix: usize, iz: usize, salt: u64) -> f32 {
    let mut h = (ix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (iz as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ salt.wrapping_mul(0x1656_67B1_9E37_79F9);
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    (h >> 40) as f32 / (1u64 << 24) as f32
}
