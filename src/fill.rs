
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::map::{grid_pos, TerrainMap, CELL};
use crate::params::GenParams;
use crate::roads::RoadNetwork;
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
    roads: &RoadNetwork,
    params: &GenParams,
) {
    spawn_trees(commands, meshes, materials, map, zones);
    spawn_field_tiling(commands, meshes, materials, map, zones, roads, params);
    spawn_sheds(commands, meshes, materials, map, zones);
}

const SMOOTH_PASSES: u32 = 2;
const RENDER_STRIDE: usize = 3;
const FARM_YARD_RADIUS: f32 = 42.0;
const NONE_OWNER: u32 = u32::MAX;

struct Segment {
    centre: Vec2,
    half_len: f32,
    along_z: bool,
}

// Tiles the farmland zone (Arable and Pasture) into organic fields, each bordered by a
// hedge, stone wall or fence. Farmyards around each farm's buildings are left clear.
//
// Fields are grown, not cut: seeds are scattered across farmland, then every farmland cell
// is claimed by whichever seed reaches it most cheaply in a cost-weighted search (the same
// technique used for rivers and roads), where crossing a slope costs more than flowing
// along it. That makes a region's boundary hug the land's contours rather than cutting
// across them, and gives organic shapes instead of rectangles. A smoothing pass then rounds
// off the grid-stepping. Both the fill colour and the boundaries are then rendered from a
// coarser 30m sampling of the result, which bounds the geometry regardless of how many
// fields the growth produces.
fn spawn_field_tiling(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    map: &TerrainMap,
    zones: &ZoneMap,
    roads: &RoadNetwork,
    params: &GenParams,
) {
    let n = map.grid_size();
    let farms: Vec<Vec2> = map
        .pois
        .iter()
        .filter(|p| p.kind == crate::map::PoiKind::Farm)
        .map(|p| p.position)
        .collect();

    // Roads are excluded from the traversable farmland graph, the same way water is, so a
    // field can't grow across one. That splits the field in two either side of the road and
    // gives it a boundary there, instead of the road just being painted over one field.
    let is_farmland: Vec<bool> = (0..n * n)
        .map(|idx| {
            let (ix, iz) = (idx % n, idx / n);
            let zone = zones.zone_at(ix, iz);
            if zone != Zone::Arable && zone != Zone::Pasture {
                return false;
            }
            if roads.kind_at(ix, iz).is_some() {
                return false;
            }
            !farms.iter().any(|f| f.distance(grid_pos(ix, iz)) < FARM_YARD_RADIUS)
        })
        .collect();

    let seeds = scatter_seeds(map, &is_farmland, params);
    if seeds.is_empty() {
        return;
    }
    let owner = smooth_owners(map, &is_farmland, claim_regions(map, &is_farmland, &seeds, params), SMOOTH_PASSES);

    let field_colour: Vec<[f32; 3]> = seeds
        .iter()
        .map(|&seed_idx| {
            let (ix, iz) = (seed_idx % n, seed_idx / n);
            let p = grid_pos(ix, iz);
            let block = (
                ((p.x + crate::map::HALF_SIZE) / 150.0) as usize,
                ((p.y + crate::map::HALF_SIZE) / 150.0) as usize,
            );
            if zones.zone_at(ix, iz) == Zone::Arable {
                crop_colour(block.0, block.1)
            } else {
                pasture_colour(block.0, block.1)
            }
        })
        .collect();

    let cn = n.div_ceil(RENDER_STRIDE);
    let coarse_owner: Vec<Option<u32>> = (0..cn * cn)
        .map(|i| {
            let (cx, cz) = (i % cn, i / cn);
            let (fx, fz) = ((cx * RENDER_STRIDE).min(n - 1), (cz * RENDER_STRIDE).min(n - 1));
            owner[fz * n + fx]
        })
        .collect();

    spawn_field_colour(commands, meshes, materials, map, &coarse_owner, &field_colour, cn);
    spawn_field_boundaries(commands, meshes, materials, map, &coarse_owner, cn);
}

fn scatter_seeds(map: &TerrainMap, is_farmland: &[bool], params: &GenParams) -> Vec<usize> {
    let n = map.grid_size();
    let half_map = crate::map::HALF_SIZE;
    let spacing = params.field_spacing;
    let cols = (crate::map::MAP_SIZE / spacing).ceil() as i32;
    let mut seeds = Vec::new();
    for sz in 0..cols {
        for sx in 0..cols {
            let base = Vec2::new(
                -half_map + (sx as f32 + 0.5) * spacing,
                -half_map + (sz as f32 + 0.5) * spacing,
            );
            let jitter = Vec2::new(
                (field_hash(base, 40) - 0.5) * spacing * 0.7,
                (field_hash(base, 41) - 0.5) * spacing * 0.7,
            );
            let (ix, iz) = nearest_cell(map, base + jitter);
            let idx = iz * n + ix;
            if is_farmland[idx] {
                seeds.push(idx);
            }
        }
    }
    seeds
}

// A weighted multi-source search: every farmland cell is claimed by whichever seed reaches
// it most cheaply, where the cost of a step rises with the slope it crosses. This is the
// same shape of search used for river and road routing.
fn claim_regions(map: &TerrainMap, is_farmland: &[bool], seeds: &[usize], params: &GenParams) -> Vec<Option<u32>> {
    let n = map.grid_size();
    let mut cost = vec![f32::MAX; n * n];
    let mut owner: Vec<Option<u32>> = vec![None; n * n];
    let mut heap = BinaryHeap::new();
    for (id, &s) in seeds.iter().enumerate() {
        cost[s] = 0.0;
        owner[s] = Some(id as u32);
        heap.push(Reverse((0u32, id as u32, s as u32)));
    }
    while let Some(Reverse((_, id, idx))) = heap.pop() {
        let c = idx as usize;
        if owner[c] != Some(id) {
            continue;
        }
        for nb in field_neighbours(n, c) {
            if !is_farmland[nb] {
                continue;
            }
            let diagonal = (nb % n != c % n) && (nb / n != c / n);
            let step = if diagonal { CELL * std::f32::consts::SQRT_2 } else { CELL };
            let climb = (map.vertex_height(nb % n, nb / n) - map.vertex_height(c % n, c / n)).abs() / CELL;
            if climb > params.field_max_slope {
                continue;
            }
            let next_cost = cost[c] + step * (1.0 + params.field_contour_weight * climb);
            if next_cost < cost[nb] {
                cost[nb] = next_cost;
                owner[nb] = Some(id);
                heap.push(Reverse((next_cost.to_bits(), id, nb as u32)));
            }
        }
    }
    owner
}

// Reassigns each farmland cell to the most common owner among itself and its neighbours,
// which rounds off the single-cell jaggedness the weighted search leaves behind.
fn smooth_owners(map: &TerrainMap, is_farmland: &[bool], mut owner: Vec<Option<u32>>, passes: u32) -> Vec<Option<u32>> {
    let n = map.grid_size();
    for _ in 0..passes {
        let mut next = owner.clone();
        for idx in 0..n * n {
            if !is_farmland[idx] {
                continue;
            }
            let mut counts: Vec<(u32, u32)> = Vec::with_capacity(9);
            for sample in std::iter::once(idx).chain(field_neighbours(n, idx)) {
                let Some(o) = owner[sample] else { continue };
                match counts.iter_mut().find(|(id, _)| *id == o) {
                    Some(entry) => entry.1 += 1,
                    None => counts.push((o, 1)),
                }
            }
            if let Some(&(best, _)) = counts.iter().max_by_key(|(_, c)| *c) {
                next[idx] = Some(best);
            }
        }
        owner = next;
    }
    owner
}

fn field_neighbours(n: usize, idx: usize) -> impl Iterator<Item = usize> {
    let (ix, iz) = ((idx % n) as isize, (idx / n) as isize);
    (-1..=1isize)
        .flat_map(move |dz| (-1..=1isize).map(move |dx| (dx, dz)))
        .filter(|&(dx, dz)| (dx, dz) != (0, 0))
        .filter_map(move |(dx, dz)| {
            let (x, z) = (ix + dx, iz + dz);
            let inside = (0..n as isize).contains(&x) && (0..n as isize).contains(&z);
            inside.then(|| z as usize * n + x as usize)
        })
}

fn spawn_field_colour(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    map: &TerrainMap,
    coarse_owner: &[Option<u32>],
    field_colour: &[[f32; 3]],
    cn: usize,
) {
    let n = map.grid_size();
    let half = (RENDER_STRIDE as f32 * CELL) * 0.5;
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    for cz in 0..cn {
        for cx in 0..cn {
            let Some(id) = coarse_owner[cz * cn + cx] else { continue };
            let (fx, fz) = ((cx * RENDER_STRIDE).min(n - 1), (cz * RENDER_STRIDE).min(n - 1));
            let p = grid_pos(fx, fz);
            let c = field_colour[id as usize];
            let colour = [c[0], c[1], c[2], 1.0];
            let base = positions.len() as u32;
            for (dx, dz) in [(-half, -half), (half, -half), (half, half), (-half, half)] {
                let corner = p + Vec2::new(dx, dz);
                positions.push([corner.x, map.height_at(corner) + FIELD_LIFT, corner.y]);
                colors.push(colour);
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

// Traces the boundary of the coarse ownership grid (between two different fields, or a
// field and open ground) and merges consecutive same-kind cell-edges into single, longer
// segments, rather than placing one box per cell-edge.
fn spawn_field_boundaries(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    map: &TerrainMap,
    coarse_owner: &[Option<u32>],
    cn: usize,
) {
    let n = map.grid_size();
    let tile = RENDER_STRIDE as f32 * CELL;
    let get = |cx: usize, cz: usize| coarse_owner[cz * cn + cx].unwrap_or(NONE_OWNER);
    let coarse_pos = |c: usize| {
        let f = (c * RENDER_STRIDE).min(n - 1);
        grid_pos(f, 0).x
    };

    let mut buckets: [Vec<Segment>; 3] = [Vec::new(), Vec::new(), Vec::new()];

    for cx in 0..cn.saturating_sub(1) {
        let mut run: Option<(usize, u32)> = None;
        for cz in 0..=cn {
            let kind = (cz < cn && get(cx, cz) != get(cx + 1, cz)).then(|| pair_hash(get(cx, cz), get(cx + 1, cz)));
            match (run, kind) {
                (Some((_, k)), Some(k2)) if k == k2 => {}
                _ => {
                    if let Some((start, k)) = run {
                        let x = coarse_pos(cx) + tile * 0.5;
                        let z0 = coarse_pos(start) - tile * 0.5;
                        let z1 = coarse_pos(cz - 1) + tile * 0.5;
                        buckets[(k % 3) as usize].push(Segment {
                            centre: Vec2::new(x, (z0 + z1) * 0.5),
                            half_len: (z1 - z0) * 0.5,
                            along_z: true,
                        });
                    }
                    run = kind.map(|k| (cz, k));
                }
            }
        }
    }
    for cz in 0..cn.saturating_sub(1) {
        let mut run: Option<(usize, u32)> = None;
        for cx in 0..=cn {
            let kind = (cx < cn && get(cx, cz) != get(cx, cz + 1)).then(|| pair_hash(get(cx, cz), get(cx, cz + 1)));
            match (run, kind) {
                (Some((_, k)), Some(k2)) if k == k2 => {}
                _ => {
                    if let Some((start, k)) = run {
                        let z = coarse_pos(cz) + tile * 0.5;
                        let x0 = coarse_pos(start) - tile * 0.5;
                        let x1 = coarse_pos(cx - 1) + tile * 0.5;
                        buckets[(k % 3) as usize].push(Segment {
                            centre: Vec2::new((x0 + x1) * 0.5, z),
                            half_len: (x1 - x0) * 0.5,
                            along_z: false,
                        });
                    }
                    run = kind.map(|k| (cx, k));
                }
            }
        }
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
    for (segments, (material, thickness, height)) in buckets.into_iter().zip(specs) {
        if segments.is_empty() {
            continue;
        }
        let mat = materials.add(material);
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut indices = Vec::new();
        for seg in segments {
            let ground = map.height_at(seg.centre);
            let centre = Vec3::new(seg.centre.x, ground + height / 2.0, seg.centre.y);
            let half_extent = if seg.along_z {
                Vec3::new(thickness / 2.0, height / 2.0, seg.half_len)
            } else {
                Vec3::new(seg.half_len, height / 2.0, thickness / 2.0)
            };
            push_box(&mut positions, &mut normals, &mut indices, centre, half_extent);
        }
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_indices(Indices::U32(indices));
        commands.spawn((TerrainRoot, Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat)));
    }
}

fn pair_hash(a: u32, b: u32) -> u32 {
    let (lo, hi) = (a.min(b), a.max(b));
    (hash01(lo as usize, hi as usize, 50) * 997.0) as u32
}

fn field_hash(p: Vec2, salt: u64) -> f32 {
    hash01((p.x + crate::map::HALF_SIZE).max(0.0) as usize, (p.y + crate::map::HALF_SIZE).max(0.0) as usize, salt)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::TerrainMap;
    use crate::zones::ZoneMap;

    fn generate() -> (TerrainMap, ZoneMap, crate::params::GenParams) {
        let params = crate::params::GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let zones = ZoneMap::generate(&map, &params);
        (map, zones, params)
    }

    #[test]
    fn claims_most_of_the_farmland() {
        let (map, zones, params) = generate();
        let n = map.grid_size();
        let farms: Vec<Vec2> = map
            .pois
            .iter()
            .filter(|p| p.kind == crate::map::PoiKind::Farm)
            .map(|p| p.position)
            .collect();
        let is_farmland: Vec<bool> = (0..n * n)
            .map(|idx| {
                let (ix, iz) = (idx % n, idx / n);
                let zone = zones.zone_at(ix, iz);
                if zone != Zone::Arable && zone != Zone::Pasture {
                    return false;
                }
                !farms.iter().any(|f| f.distance(grid_pos(ix, iz)) < FARM_YARD_RADIUS)
            })
            .collect();
        let farmland_count = is_farmland.iter().filter(|b| **b).count();
        let seeds = scatter_seeds(&map, &is_farmland, &params);
        assert!(seeds.len() > 500, "only {} field seeds, too sparse", seeds.len());
        let owner = claim_regions(&map, &is_farmland, &seeds, &params);
        let claimed = owner.iter().filter(|o| o.is_some()).count();
        assert!(
            claimed as f32 > farmland_count as f32 * 0.9,
            "only claimed {claimed} of {farmland_count} farmland cells"
        );
    }

    #[test]
    fn boundary_segment_count_stays_bounded() {
        let (map, zones, params) = generate();
        let n = map.grid_size();
        let farms: Vec<Vec2> = map
            .pois
            .iter()
            .filter(|p| p.kind == crate::map::PoiKind::Farm)
            .map(|p| p.position)
            .collect();
        let is_farmland: Vec<bool> = (0..n * n)
            .map(|idx| {
                let (ix, iz) = (idx % n, idx / n);
                let zone = zones.zone_at(ix, iz);
                if zone != Zone::Arable && zone != Zone::Pasture {
                    return false;
                }
                !farms.iter().any(|f| f.distance(grid_pos(ix, iz)) < FARM_YARD_RADIUS)
            })
            .collect();
        let seeds = scatter_seeds(&map, &is_farmland, &params);
        let owner = smooth_owners(&map, &is_farmland, claim_regions(&map, &is_farmland, &seeds, &params), SMOOTH_PASSES);
        let cn = n.div_ceil(RENDER_STRIDE);
        let coarse_owner: Vec<Option<u32>> = (0..cn * cn)
            .map(|i| {
                let (cx, cz) = (i % cn, i / cn);
                let (fx, fz) = ((cx * RENDER_STRIDE).min(n - 1), (cz * RENDER_STRIDE).min(n - 1));
                owner[fz * n + fx]
            })
            .collect();
        let mut transitions = 0;
        for cz in 0..cn {
            for cx in 0..cn {
                if cx + 1 < cn && coarse_owner[cz * cn + cx] != coarse_owner[cz * cn + cx + 1] {
                    transitions += 1;
                }
                if cz + 1 < cn && coarse_owner[cz * cn + cx] != coarse_owner[(cz + 1) * cn + cx] {
                    transitions += 1;
                }
            }
        }
        // Each transition becomes at most one segment before merging; merging only reduces
        // this. The old rectangle grid produced on the order of a few thousand segments.
        assert!(transitions < 40000, "{transitions} raw boundary transitions, too many");
    }
}

