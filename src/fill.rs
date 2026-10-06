use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::map::{grid_pos, TerrainMap, CELL};
use crate::terrain::TerrainRoot;
use crate::zones::{Zone, ZoneMap};

const TREE_SPACING: f32 = 16.0;
const FIELD_BLOCK_CELLS: usize = 5;
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
    spawn_fields(commands, meshes, materials, map, zones);
    spawn_sheds(commands, meshes, materials, map, zones);
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

// Crop fields: every arable cell is coloured by the crop of its 50m block, drawn as a
// quad that follows the terrain.
fn spawn_fields(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    map: &TerrainMap,
    zones: &ZoneMap,
) {
    let n = map.grid_size();
    let half = CELL * 0.5;
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    for iz in 0..n - 1 {
        for ix in 0..n - 1 {
            if zones.zone_at(ix, iz) != Zone::Arable {
                continue;
            }
            let crop = crop_colour(ix / FIELD_BLOCK_CELLS, iz / FIELD_BLOCK_CELLS);
            let c = [crop[0], crop[1], crop[2], 1.0];
            let p = grid_pos(ix, iz);
            let base = positions.len() as u32;
            for (dx, dz, cx, cz) in [
                (-half, -half, ix, iz),
                (half, -half, ix + 1, iz),
                (half, half, ix + 1, iz + 1),
                (-half, half, ix, iz + 1),
            ] {
                positions.push([p.x + dx, map.vertex_height(cx, cz) + FIELD_LIFT, p.y + dz]);
                colors.push(c);
            }
            indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }
    if positions.is_empty() {
        return;
    }
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
