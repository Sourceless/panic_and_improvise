
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use std::cmp::Reverse;
use std::collections::HashMap;
use std::collections::BinaryHeap;

use crate::contour::{clip_to_terrain_triangle, triangulate, Contour, Smoothing, OPEN};
use crate::field_material::{FieldExtension, FieldMaterial};
use crate::map::{fbm, grid_pos, TerrainMap, CELL, TILE_CELLS};
use crate::params::GenParams;
use crate::roads::RoadNetwork;
use crate::terrain::{TerrainRoot, TerrainTextures};
use crate::zones::{Zone, ZoneMap};

pub const FIELD_LIFT: f32 = 0.15;
const SHED_SPACING: f32 = 60.0;

pub fn spawn_fill(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    field_materials: &mut Assets<FieldMaterial>,
    textures: &TerrainTextures,
    map: &TerrainMap,
    zones: &ZoneMap,
    roads: &RoadNetwork,
    params: &GenParams,
) {
    let hedge_points = spawn_field_tiling(commands, meshes, materials, field_materials, textures, map, zones, roads, params);
    if !crate::world::skip("trees") {
        crate::vegetation::spawn_vegetation(commands, meshes, materials, map, zones, &hedge_points);
    }
    spawn_sheds(commands, meshes, materials, map, zones);
}

const SMOOTH_PASSES: u32 = 2;
const FARM_YARD_RADIUS: f32 = 42.0;

// Tiles the farmland zone (Arable and Pasture) into organic fields, each bordered by a
// hedge, stone wall or fence. Farmyards around each farm's buildings are left clear.
//
// Fields are grown, not cut: seeds are scattered across farmland, then every farmland cell
// is claimed by whichever seed reaches it most cheaply in a cost-weighted search (the same
// technique used for rivers and roads), where crossing a slope costs more than flowing
// along it. That makes a region's boundary hug the land's contours rather than cutting
// across them, and gives organic shapes instead of rectangles. A smoothing pass then rounds
// off the grid-stepping, and a connected-components pass splits any field that ended up
// split into disconnected pieces - most often by a road - into separate fields. Both the
// fill colour and the boundaries are then rendered at native grid resolution: boundaries
// via marching squares, which cuts every corner near 45 degrees instead of stair-stepping,
// and naturally leaves a gap wherever a road interrupts a run since there's no cell there to
// draw a wall through.
fn spawn_field_tiling(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    field_materials: &mut Assets<FieldMaterial>,
    textures: &TerrainTextures,
    map: &TerrainMap,
    zones: &ZoneMap,
    roads: &RoadNetwork,
    params: &GenParams,
) -> Vec<Vec2> {
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
    let keep_clear: Vec<bool> = (0..n * n)
        .map(|idx| {
            let (ix, iz) = (idx % n, idx / n);
            roads.kind_at(ix, iz).is_some() || farms.iter().any(|f| f.distance(grid_pos(ix, iz)) < FARM_YARD_RADIUS)
        })
        .collect();
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
        return Vec::new();
    }
    let owner = smooth_owners(map, &is_farmland, claim_regions(map, &is_farmland, &seeds, params), SMOOTH_PASSES);
    // A field can end up split into pieces that aren't actually touching - most often
    // because a road cuts through it, with the two halves otherwise reconnecting around the
    // road's ends. Giving every disconnected group its own fresh id makes a road-split field
    // render (and colour) as two separate fields instead of one that invisibly jumps the gap.
    let mut owner = split_disconnected_regions(map, &is_farmland, &owner);
    merge_tiny_fields(map, &mut owner);
    fill_small_holes(map, &mut owner, &keep_clear);
    let styles = build_field_styles(map, zones, &owner);

    let labels: Vec<u32> = owner.iter().map(|o| o.unwrap_or(OPEN)).collect();
    commands.insert_resource(ground_cover(map, zones, &labels, &styles, &keep_clear));
    let contour = Contour::build(n, &labels, map.seed, None, Smoothing::FIELD);
    if !crate::world::skip("fields") {
        spawn_field_colour(commands, meshes, field_materials, textures, map, &labels, &contour, &styles);
    }
    if !crate::world::skip("boundaries") {
        spawn_field_boundaries(commands, meshes, materials, textures, map, &contour);
    }
    hedge_tree_points(&contour)
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
            // A low-frequency noise field marks broad patches as "large field" zones; inside
            // one, most candidate seeds are thinned out so the few that remain claim a much
            // bigger area than usual, giving some size variety across the map instead of
            // every field being close to the same size.
            if params.large_field_fraction > 0.0 {
                let patch = fbm(base.x / 700.0, base.y / 700.0, map.seed ^ 0x7A12_0000, 3);
                if patch < params.large_field_fraction && field_hash(base, 42) > 0.18 {
                    continue;
                }
            }
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

// After growth and smoothing, a field's cells may no longer all be connected to each other -
// most often because a road cuts through it and the flood fill reconnected around the road's
// ends. A simple flood fill over same-owner farmland cells gives every disconnected group its
// own fresh id, in scan order.
fn split_disconnected_regions(map: &TerrainMap, is_farmland: &[bool], owner: &[Option<u32>]) -> Vec<Option<u32>> {
    let n = map.grid_size();
    let mut relabeled: Vec<Option<u32>> = vec![None; n * n];
    let mut visited = vec![false; n * n];
    let mut next_id = 0u32;
    let mut stack = Vec::new();
    for start in 0..n * n {
        if !is_farmland[start] || visited[start] || owner[start].is_none() {
            continue;
        }
        let owner_id = owner[start];
        visited[start] = true;
        stack.push(start);
        while let Some(c) = stack.pop() {
            relabeled[c] = Some(next_id);
            // Orthogonal neighbours only: cells touching just at a corner aren't one field,
            // since marching squares would draw them as two separate outlines anyway.
            for nb in field_neighbours(n, c).filter(|&nb| nb % n == c % n || nb / n == c / n) {
                if !is_farmland[nb] || visited[nb] || owner[nb] != owner_id {
                    continue;
                }
                visited[nb] = true;
                stack.push(nb);
            }
        }
        next_id += 1;
    }
    relabeled
}

// What a field is used for. The ids are shared with assets/shaders/field.wgsl.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FieldKind {
    Ploughed = 0,
    Wheat = 1,
    Barley = 2,
    Rapeseed = 3,
    RowCrop = 4,
    Maize = 5,
    Stubble = 6,
    Legume = 7,
    Hay = 8,
    Pasture = 9,
    Rough = 10,
}

#[derive(Clone, Copy)]
struct FieldStyle {
    kind: FieldKind,
    // For crop kinds an absolute canopy colour (linear); for textured kinds a multiplier on
    // the texture.
    tint: [f32; 3],
    // Direction rows / stripes run along, in the world XZ plane.
    row_dir: Vec2,
}

fn srgb(c: [f32; 3]) -> [f32; 3] {
    c.map(|v| v.powf(2.2))
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn weighted_pick<T: Copy>(options: &[(T, f32)], roll: f32) -> T {
    let total: f32 = options.iter().map(|o| o.1).sum();
    let mut acc = 0.0;
    for &(item, w) in options {
        acc += w / total;
        if roll < acc {
            return item;
        }
    }
    options[options.len() - 1].0
}

// Gives each field a kind, tint and row direction. A field's identity comes from the first
// cell found for its id; ids come from split_disconnected_regions rather than directly from
// the seed list, so two pieces of a road-split field are separate fields with their own look.
// Rows follow the contours on sloped ground (as real farmers plough) and are random on the
// flat.
fn build_field_styles(map: &TerrainMap, zones: &ZoneMap, owner: &[Option<u32>]) -> Vec<FieldStyle> {
    let n = map.grid_size();
    let field_count = owner.iter().filter_map(|&o| o).max().map_or(0, |m| m + 1) as usize;
    let mut sums = vec![(Vec2::ZERO, 0u32, Zone::Arable); field_count];
    for (idx, &o) in owner.iter().enumerate() {
        let Some(id) = o else { continue };
        let (ix, iz) = (idx % n, idx / n);
        let e = &mut sums[id as usize];
        if e.1 == 0 {
            e.2 = zones.zone_at(ix, iz);
        }
        e.0 += grid_pos(ix, iz);
        e.1 += 1;
    }
    sums.iter()
        .enumerate()
        .map(|(id, &(sum, count, zone))| {
            if count == 0 {
                return FieldStyle { kind: FieldKind::Pasture, tint: [1.0; 3], row_dir: Vec2::X };
            }
            let centre = sum / count as f32;
            let h = |salt: u64| {
                hash01((centre.x as i32).rem_euclid(9973) as usize, (centre.y as i32).rem_euclid(9973) as usize, 100 + salt)
            };
            let kind = if zone == Zone::Arable {
                weighted_pick(
                    &[
                        (FieldKind::Ploughed, 0.12),
                        (FieldKind::Wheat, 0.26),
                        (FieldKind::Barley, 0.16),
                        (FieldKind::Rapeseed, 0.10),
                        (FieldKind::RowCrop, 0.10),
                        (FieldKind::Maize, 0.06),
                        (FieldKind::Stubble, 0.12),
                        (FieldKind::Legume, 0.08),
                    ],
                    h(1),
                )
            } else {
                weighted_pick(&[(FieldKind::Hay, 0.30), (FieldKind::Pasture, 0.45), (FieldKind::Rough, 0.25)], h(1))
            };
            let ripeness = h(2);
            let vary = 0.9 + 0.2 * h(3);
            let tint = match kind {
                FieldKind::Ploughed => [vary; 3],
                FieldKind::Wheat => lerp3(srgb([0.40, 0.52, 0.20]), srgb([0.78, 0.64, 0.28]), ripeness),
                FieldKind::Barley => lerp3(srgb([0.46, 0.56, 0.26]), srgb([0.80, 0.72, 0.36]), ripeness),
                FieldKind::Rapeseed => {
                    if ripeness < 0.8 { srgb([0.93, 0.80, 0.10]) } else { srgb([0.42, 0.52, 0.18]) }
                }
                FieldKind::RowCrop => srgb([0.26, 0.42, 0.14]),
                FieldKind::Maize => srgb([0.28, 0.40, 0.12]),
                FieldKind::Stubble => srgb([0.74, 0.66, 0.40]),
                FieldKind::Legume => srgb([0.44, 0.58, 0.20]),
                FieldKind::Hay => [vary * 1.05, vary * 1.1, vary * 0.85],
                FieldKind::Pasture => [vary, vary * 1.05, vary * 0.9],
                FieldKind::Rough => [vary * 1.1, vary * 1.0, vary * 0.75],
            };
            let step = 25.0;
            let grad = Vec2::new(
                map.height_at(centre + Vec2::X * step) - map.height_at(centre - Vec2::X * step),
                map.height_at(centre + Vec2::Y * step) - map.height_at(centre - Vec2::Y * step),
            ) / (2.0 * step);
            let angle = if grad.length() > 0.04 && h(4) < 0.85 {
                // Along the contour: perpendicular to the downhill direction.
                Vec2::new(-grad.y, grad.x).to_angle() + (h(5) - 0.5) * 0.25
            } else {
                h(5) * std::f32::consts::PI
            };
            let _ = id;
            FieldStyle { kind, tint, row_dir: Vec2::from_angle(angle) }
        })
        .collect()
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
    materials: &mut Assets<FieldMaterial>,
    textures: &TerrainTextures,
    map: &TerrainMap,
    labels: &[u32],
    contour: &Contour,
    styles: &[FieldStyle],
) {
    // Each cell is clipped to the same smoothed contour the boundary walls follow
    // (see Contour::cell_regions), so the colour edge meets the wall exactly and a field's outline
    // against open ground is as smooth as one between two fields. Each clipped polygon is
    // then split along the terrain's own diagonal and heighted per triangle, so it stays
    // coplanar with the real terrain.
    let n = map.grid_size();
    // One mesh per tile, so frustum culling can skip fields that aren't in view.
    let mut tiles: HashMap<(usize, usize), FillBuf> = HashMap::new();
    for iz in 0..n - 1 {
        for ix in 0..n - 1 {
            let get = |x: usize, z: usize| labels[z * n + x];
            let cell = [get(ix, iz), get(ix + 1, iz), get(ix + 1, iz + 1), get(ix, iz + 1)];
            if cell.iter().all(|&o| o == OPEN) {
                continue;
            }
            let buf = tiles.entry((ix / TILE_CELLS, iz / TILE_CELLS)).or_default();
            for (id, poly) in contour.cell_regions(ix, iz, cell) {
                let style = styles[id as usize];
                let colour = [style.tint[0], style.tint[1], style.tint[2], style.kind as u32 as f32 / 16.0];
                let (along, across) = (style.row_dir, Vec2::new(-style.row_dir.y, style.row_dir.x));
                for tri in triangulate(&poly) {
                    // Relaxed vertices can sit a few metres outside this cell, so each
                    // triangle is clipped against every terrain triangle it overlaps and
                    // heighted from that triangle's own plane - which is what keeps it
                    // flush with the ground rather than dipping under it on a slope.
                    let lo = tri.iter().fold(Vec2::MAX, |m, p| m.min(*p)).floor();
                    let hi = tri.iter().fold(Vec2::MIN, |m, p| m.max(*p)).floor();
                    for dz in lo.y as i32..=hi.y as i32 {
                        for dx in lo.x as i32..=hi.x as i32 {
                            let (tx, tz) = (ix as i32 + dx, iz as i32 + dz);
                            if tx < 0 || tz < 0 || tx as usize >= n - 1 || tz as usize >= n - 1 {
                                continue;
                            }
                            let (tx, tz) = (tx as usize, tz as usize);
                            let shift = Vec2::new(dx as f32, dz as f32);
                            let local: Vec<Vec2> = tri.iter().map(|p| *p - shift).collect();
                            let h_tl = map.vertex_height(tx, tz) + FIELD_LIFT;
                            let h_tr = map.vertex_height(tx + 1, tz) + FIELD_LIFT;
                            let h_br = map.vertex_height(tx + 1, tz + 1) + FIELD_LIFT;
                            let h_bl = map.vertex_height(tx, tz + 1) + FIELD_LIFT;
                            let origin = grid_pos(tx, tz);
                            for lower in [true, false] {
                                let clipped = clip_to_terrain_triangle(&local, lower);
                                if clipped.len() < 3 {
                                    continue;
                                }
                                // terrain_mesh splits each cell along the TR-BL diagonal.
                                let height = |p: Vec2| {
                                    if lower {
                                        h_tl + p.x * (h_tr - h_tl) + p.y * (h_bl - h_tl)
                                    } else {
                                        h_br + (1.0 - p.x) * (h_bl - h_br) + (1.0 - p.y) * (h_tr - h_br)
                                    }
                                };
                                for k in 1..clipped.len() - 1 {
                                    let mut t = [clipped[0], clipped[k], clipped[k + 1]];
                                    // Winding must face up; (u, v) maps to (x, z).
                                    let area = (t[1].y - t[0].y) * (t[2].x - t[0].x)
                                        - (t[1].x - t[0].x) * (t[2].y - t[0].y);
                                    if area.abs() < 1e-7 {
                                        continue;
                                    }
                                    if area < 0.0 {
                                        t.swap(1, 2);
                                    }
                                    let base = buf.positions.len() as u32;
                                    for p in t {
                                        let w = origin + p * CELL;
                                        buf.positions.push([w.x, height(p), w.y]);
                                        buf.colors.push(colour);
                                        buf.uvs.push([w.dot(across), w.dot(along)]);
                                    }
                                    buf.indices.extend_from_slice(&[base, base + 1, base + 2]);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if tiles.is_empty() {
        return;
    }
    let material = materials.add(FieldMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 1.0,
            ..default()
        },
        extension: FieldExtension {
            diffuse: textures.ground.diffuse.clone(),
            normal: textures.ground.normal.clone(),
        },
    });
    for buf in tiles.into_values() {
        let normals = vec![[0.0, 1.0, 0.0]; buf.positions.len()];
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, buf.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, buf.colors)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, buf.uvs)
            .with_inserted_indices(Indices::U32(buf.indices));
        commands.spawn((TerrainRoot, bevy::light::NotShadowCaster, Mesh3d(meshes.add(mesh)), MeshMaterial3d(material.clone())));
    }
}

#[derive(Default)]
struct FillBuf {
    positions: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

// Small pockets of open ground wholly enclosed by a single field (a few cells of odd zone
// inside a field) read as noisy specks with their own wall loop, so they're filled in. Road
// and farmyard cells are never filled, since those are deliberately left clear.
const MAX_HOLE_CELLS: usize = 80;

fn fill_small_holes(map: &TerrainMap, owner: &mut [Option<u32>], keep_clear: &[bool]) {
    let n = map.grid_size();
    let mut seen = vec![false; n * n];
    for start in 0..n * n {
        if seen[start] || owner[start].is_some() || keep_clear[start] {
            continue;
        }
        let mut comp = vec![start];
        seen[start] = true;
        let mut i = 0;
        let mut surround: Option<u32> = None;
        let mut fillable = true;
        while i < comp.len() {
            let c = comp[i];
            i += 1;
            let (ix, iz) = (c % n, c / n);
            if ix == 0 || iz == 0 || ix == n - 1 || iz == n - 1 {
                fillable = false;
            }
            for nb in field_neighbours(n, c) {
                match owner[nb] {
                    Some(o) => match surround {
                        None => surround = Some(o),
                        Some(s) if s != o => fillable = false,
                        _ => {}
                    },
                    None if keep_clear[nb] => fillable = false,
                    None if !seen[nb] => {
                        seen[nb] = true;
                        comp.push(nb);
                    }
                    None => {}
                }
            }
            if comp.len() > MAX_HOLE_CELLS {
                fillable = false;
            }
        }
        if let (true, Some(id)) = (fillable, surround) {
            for c in comp {
                owner[c] = Some(id);
            }
        }
    }
}

// Fields below this many cells (~2500 m2) are slivers left over from growth, smoothing and
// road/river splitting; they get absorbed into whichever neighbouring field they share the
// most border with.
const MIN_FIELD_CELLS: usize = 25;

fn merge_tiny_fields(map: &TerrainMap, owner: &mut [Option<u32>]) {
    let n = map.grid_size();
    let count = owner.iter().filter_map(|&o| o).max().map_or(0, |m| m + 1) as usize;
    let mut sizes = vec![0usize; count];
    for &o in owner.iter().flatten() {
        sizes[o as usize] += 1;
    }
    let mut order: Vec<usize> = (0..count).filter(|&id| sizes[id] > 0 && sizes[id] < MIN_FIELD_CELLS).collect();
    order.sort_by_key(|&id| sizes[id]);
    for id in order {
        let id = id as u32;
        let mut border: Vec<(u32, u32)> = Vec::new();
        for idx in (0..n * n).filter(|&i| owner[i] == Some(id)) {
            for nb in field_neighbours(n, idx).filter(|&nb| nb % n == idx % n || nb / n == idx / n) {
                if let Some(o) = owner[nb].filter(|&o| o != id) {
                    match border.iter_mut().find(|(b, _)| *b == o) {
                        Some(e) => e.1 += 1,
                        None => border.push((o, 1)),
                    }
                }
            }
        }
        let Some(&(target, _)) = border.iter().max_by_key(|(_, c)| *c) else {
            // Nothing to merge into: an orphan sliver, better dropped than drawn as a speck.
            for o in owner.iter_mut().filter(|o| **o == Some(id)) {
                *o = None;
            }
            sizes[id as usize] = 0;
            continue;
        };
        let moved = sizes[id as usize];
        for o in owner.iter_mut().filter(|o| **o == Some(id)) {
            *o = Some(target);
        }
        sizes[target as usize] += moved;
        sizes[id as usize] = 0;
    }
}

// What grass grows where, for the near-camera grass: grazed and mown fields get their own
// kinds, and open ground that isn't a field (verges, orchards, moor) gets a sparse cover.
// Roads, settlements, woods and farmyards stay bare.
fn ground_cover(map: &TerrainMap, zones: &ZoneMap, labels: &[u32], styles: &[FieldStyle], keep_clear: &[bool]) -> crate::grass::GroundCover {
    use crate::grass::Cover;
    let n = map.grid_size();
    let kinds = (0..n * n)
        .map(|idx| {
            if keep_clear[idx] || map.water_level(idx % n, idx / n).is_some() {
                return Cover::None as u8;
            }
            if labels[idx] != OPEN {
                let cover = match styles[labels[idx] as usize].kind {
                    FieldKind::Hay => Cover::Meadow,
                    FieldKind::Pasture => Cover::Pasture,
                    FieldKind::Rough => Cover::Rough,
                    _ => Cover::None,
                };
                return cover as u8;
            }
            let cover = match zones.zone_at(idx % n, idx / n) {
                Zone::Pasture | Zone::Arable | Zone::Orchard | Zone::Open | Zone::Moorland | Zone::Wetland => Cover::Verge,
                Zone::Woodland | Zone::Conifer => Cover::Understory,
                _ => Cover::None,
            };
            cover as u8
        })
        .collect();
    crate::grass::GroundCover::new(n, kinds)
}

// Spots along hedgerows (the first of the three boundary kinds) where an occasional full-size
// tree stands, roughly one per 40 m of hedge.
fn hedge_tree_points(contour: &Contour) -> Vec<Vec2> {
    contour
        .segs
        .iter()
        .filter(|s| pair_hash(s.pair.0, s.pair.1) % 3 == 0)
        .filter(|s| {
            let m = (s.a + s.b) * 0.5;
            field_hash(m, 60) < 0.16
        })
        .map(|s| (s.a + s.b) * 0.5)
        .collect()
}

// Boundary geometry accumulates into one of these per kind, with UVs for the wall textures.
#[derive(Default)]
struct WallBuf {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl WallBuf {
    fn quad(&mut self, corners: [Vec3; 4], normal: Vec3, uvs: [[f32; 2]; 4]) {
        let base = self.positions.len() as u32;
        for (c, uv) in corners.iter().zip(uvs) {
            self.positions.push(c.to_array());
            self.normals.push(normal.to_array());
            self.uvs.push(uv);
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn into_mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
            .with_inserted_indices(Indices::U32(self.indices))
    }
}

fn spawn_field_boundaries(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    textures: &TerrainTextures,
    map: &TerrainMap,
    contour: &Contour,
) {
    // Each segment is already short (at most one grid cell across), and pins to the true
    // ground height at both of its own endpoints - the same trick road_mesh uses for its
    // ribbons - so it follows the terrain tightly without needing any further subdivision.
    // Split by tile, so frustum culling (including for shadows) can skip distant boundaries.
    let mut tiles: HashMap<(usize, usize, u32), WallBuf> = HashMap::new();
    for seg in &contour.segs {
        let (ga, gb) = (map.height_at(seg.a), map.height_at(seg.b));
        let kind = pair_hash(seg.pair.0, seg.pair.1) % 3;
        let mid = (seg.a + seg.b) * 0.5;
        let tile = (
            (((mid.x + crate::map::HALF_SIZE) / CELL).max(0.0) as usize) / TILE_CELLS,
            (((mid.y + crate::map::HALF_SIZE) / CELL).max(0.0) as usize) / TILE_CELLS,
            kind,
        );
        let buf = tiles.entry(tile).or_default();
        match kind {
            0 => push_hedge_segment(buf, map, seg.a, ga, seg.b, gb),
            1 => push_box_segment(buf, seg.a, ga, seg.b, gb, 0.6, 0.0, 1.1, 1.6),
            _ => push_fence_segment(buf, seg.a, ga, seg.b, gb),
        }
    }
    let textured = |tex: &Handle<Image>, tint: Color, roughness: f32| StandardMaterial {
        base_color_texture: Some(tex.clone()),
        base_color: tint,
        perceptual_roughness: roughness,
        ..default()
    };
    let materials_by_kind = [
        materials.add(textured(&textures.hedge, Color::srgb(0.55, 0.62, 0.45), 0.95)),
        materials.add(textured(&textures.wall_stone, Color::srgb(0.95, 0.93, 0.88), 0.95)),
        materials.add(textured(&textures.wood, Color::srgb(0.85, 0.78, 0.7), 0.9)),
    ];
    for ((_, _, kind), buf) in tiles {
        if buf.positions.is_empty() {
            continue;
        }
        commands.spawn((
            TerrainRoot,
            Mesh3d(meshes.add(buf.into_mesh())),
            MeshMaterial3d(materials_by_kind[kind as usize].clone()),
        ));
    }
}

fn pair_hash(a: u32, b: u32) -> u32 {
    let (lo, hi) = (a.min(b), a.max(b));
    (hash01(lo as usize, hi as usize, 50) * 997.0) as u32
}

fn field_hash(p: Vec2, salt: u64) -> f32 {
    hash01((p.x + crate::map::HALF_SIZE).max(0.0) as usize, (p.y + crate::map::HALF_SIZE).max(0.0) as usize, salt)
}

// A short box between two points, each end pinned to its own ground height, rather than a
// box translated to one flat height - so a chain of these follows the ground rising and
// falling along its length exactly at each sample point. `y0..y1` is the vertical extent
// above the ground, and `tile` the metres per texture repeat.
fn push_box_segment(buf: &mut WallBuf, a: Vec2, ga: f32, b: Vec2, gb: f32, thickness: f32, y0: f32, y1: f32, tile: f32) {
    let dir = (b - a).normalize_or_zero();
    let perp = Vec2::new(-dir.y, dir.x) * (thickness * 0.5);
    let dir3 = Vec3::new(dir.x, 0.0, dir.y);
    let perp3 = Vec3::new(perp.x, 0.0, perp.y).normalize_or_zero();
    let corner = |p: Vec2, sign: f32, y: f32| {
        let c = p + perp * sign;
        Vec3::new(c.x, y, c.y)
    };
    let (u0, u1) = (a.dot(dir) / tile, b.dot(dir) / tile);
    let (v0, v1) = (y0 / tile, y1 / tile);
    let (al_b, al_t) = (corner(a, -1.0, ga + y0), corner(a, -1.0, ga + y1));
    let (ar_b, ar_t) = (corner(a, 1.0, ga + y0), corner(a, 1.0, ga + y1));
    let (bl_b, bl_t) = (corner(b, -1.0, gb + y0), corner(b, -1.0, gb + y1));
    let (br_b, br_t) = (corner(b, 1.0, gb + y0), corner(b, 1.0, gb + y1));
    // The underside is never seen, so it's skipped.
    buf.quad([al_t, ar_t, br_t, bl_t], Vec3::Y, [[u0, 0.0], [u0, thickness / tile], [u1, thickness / tile], [u1, 0.0]]);
    buf.quad([al_b, al_t, bl_t, bl_b], -perp3, [[u0, v0], [u0, v1], [u1, v1], [u1, v0]]);
    buf.quad([ar_t, ar_b, br_b, br_t], perp3, [[u0, v1], [u0, v0], [u1, v0], [u1, v1]]);
    buf.quad([al_t, al_b, ar_b, ar_t], -dir3, [[0.0, v1], [0.0, v0], [thickness / tile, v0], [thickness / tile, v1]]);
    buf.quad([bl_b, bl_t, br_t, br_b], dir3, [[0.0, v0], [0.0, v1], [thickness / tile, v1], [thickness / tile, v0]]);
}

// Two thin rails along the segment, and a post at its start (the next segment's start is
// this one's end, so a post every segment length, about 7 m).
fn push_fence_segment(buf: &mut WallBuf, a: Vec2, ga: f32, b: Vec2, gb: f32) {
    for (y0, y1) in [(0.32, 0.42), (0.8, 0.9)] {
        push_box_segment(buf, a, ga, b, gb, 0.06, y0, y1, 1.0);
    }
    let dir = (b - a).normalize_or_zero();
    for t in [0.0, 0.5] {
        let p = a.lerp(b, t);
        let g = ga + (gb - ga) * t;
        push_box_segment(buf, p - dir * 0.07, g, p + dir * 0.07, g, 0.14, -0.1, 1.15, 1.0);
    }
}

// A hedge: a rounded, slightly irregular profile extruded between two points, textured with
// real leaves (the texture repeats every 2 m).
fn push_hedge_segment(buf: &mut WallBuf, map: &TerrainMap, a: Vec2, ga: f32, b: Vec2, gb: f32) {
    // (distance from the centreline, height) of the cross-section, left foot to right foot.
    const PROFILE: [(f32, f32); 9] = [
        (-0.6, 0.0), (-0.88, 0.55), (-0.78, 1.3), (-0.42, 1.78), (0.0, 1.92),
        (0.42, 1.78), (0.78, 1.3), (0.88, 0.55), (0.6, 0.0),
    ];
    let dir = (b - a).normalize_or_zero();
    let perp = Vec2::new(-dir.y, dir.x);
    // Smooth per-vertex normals from the profile's neighbouring edges.
    let normal_at = |i: usize| {
        let prev = PROFILE[i.saturating_sub(1)];
        let next = PROFILE[(i + 1).min(PROFILE.len() - 1)];
        let t = Vec2::new(next.0 - prev.0, next.1 - prev.1).normalize_or_zero();
        let n2 = Vec2::new(-t.y, t.x); // outward for a profile listed left to right
        Vec3::new(perp.x * n2.x, n2.y, perp.y * n2.x).normalize_or_zero()
    };
    // The same position always gets the same height wobble, so neighbouring segments agree.
    let wobble = |p: Vec2| 0.82 + 0.4 * fbm(p.x / 7.0, p.y / 7.0, 0x4ED6E, 2);
    let _ = map;
    let (wa, wb) = (wobble(a), wobble(b));
    let (u0, u1) = (a.dot(dir) / 2.0, b.dot(dir) / 2.0);
    let mut arc = 0.0;
    let mut rings: Vec<([Vec3; 2], Vec3, f32)> = Vec::new();
    for (i, &(x, y)) in PROFILE.iter().enumerate() {
        if i > 0 {
            let p = PROFILE[i - 1];
            arc += ((x - p.0).powi(2) + (y - p.1).powi(2)).sqrt();
        }
        let pa = a + perp * x;
        let pb = b + perp * x;
        rings.push(([Vec3::new(pa.x, ga + y * wa, pa.y), Vec3::new(pb.x, gb + y * wb, pb.y)], normal_at(i), arc / 2.0));
    }
    let base = buf.positions.len() as u32;
    for (ends, n, v) in &rings {
        for (k, end) in ends.iter().enumerate() {
            buf.positions.push(end.to_array());
            buf.normals.push(n.to_array());
            buf.uvs.push([if k == 0 { u0 } else { u1 }, *v]);
        }
    }
    for i in 0..PROFILE.len() as u32 - 1 {
        let (a0, b0, a1, b1) = (base + i * 2, base + i * 2 + 1, base + i * 2 + 2, base + i * 2 + 3);
        buf.indices.extend_from_slice(&[a0, a1, b0, b0, a1, b1]);
    }
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

    fn farmland(map: &TerrainMap, zones: &ZoneMap) -> Vec<bool> {
        let n = map.grid_size();
        let farms: Vec<Vec2> = map
            .pois
            .iter()
            .filter(|p| p.kind == crate::map::PoiKind::Farm)
            .map(|p| p.position)
            .collect();
        (0..n * n)
            .map(|idx| {
                let (ix, iz) = (idx % n, idx / n);
                let zone = zones.zone_at(ix, iz);
                if zone != Zone::Arable && zone != Zone::Pasture {
                    return false;
                }
                !farms.iter().any(|f| f.distance(grid_pos(ix, iz)) < FARM_YARD_RADIUS)
            })
            .collect()
    }

    #[test]
    fn claims_most_of_the_farmland() {
        let (map, zones, params) = generate();
        let is_farmland = farmland(&map, &zones);
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
        let is_farmland = farmland(&map, &zones);
        let seeds = scatter_seeds(&map, &is_farmland, &params);
        let owner = smooth_owners(&map, &is_farmland, claim_regions(&map, &is_farmland, &seeds, &params), SMOOTH_PASSES);
        let owner = split_disconnected_regions(&map, &is_farmland, &owner);
        let labels: Vec<u32> = owner.iter().map(|o| o.unwrap_or(OPEN)).collect();
        let segs = Contour::build(map.grid_size(), &labels, map.seed, None, Smoothing::FIELD).segs;
        // Marching squares at native (10m) resolution emits roughly one segment per grid
        // cell along a field's perimeter, several times more than the old coarse (30m),
        // merged-run tracing - bounded generously above the measured order of magnitude.
        assert!(segs.len() < 200_000, "{} boundary segments, too many", segs.len());
    }

    #[test]
    fn disconnected_regions_get_separate_ids() {
        let (map, zones, params) = generate();
        let is_farmland = farmland(&map, &zones);
        let seeds = scatter_seeds(&map, &is_farmland, &params);
        let owner = smooth_owners(&map, &is_farmland, claim_regions(&map, &is_farmland, &seeds, &params), SMOOTH_PASSES);
        let owner = split_disconnected_regions(&map, &is_farmland, &owner);
        // If every id is already a single connected blob, splitting again is a no-op:
        // re-splitting an already-fully-split map can't find more regions than it already
        // has. An increase here would mean some id still spanned disconnected cells.
        let resplit = split_disconnected_regions(&map, &is_farmland, &owner);
        let count = |o: &[Option<u32>]| o.iter().filter_map(|&x| x).max().map_or(0, |m| m + 1);
        assert_eq!(
            count(&owner),
            count(&resplit),
            "re-splitting found more regions than the first split produced"
        );
    }

    // Diagnostic, not an assertion: prints a spot where a road runs across farmland on both
    // sides, as a MAP_VIEWER_TARGET to visually check the road gap and field-split together.
    //   cargo test --lib find_road_through_field_hotspot -- --ignored --nocapture
    #[test]
    #[ignore]
    fn find_road_through_field_hotspot() {
        let (map, zones, params) = generate();
        let n = map.grid_size();
        let roads = crate::roads::RoadNetwork::generate(&map, &params);
        let is_field = |x: usize, z: usize| matches!(zones.zone_at(x, z), Zone::Arable | Zone::Pasture) && roads.kind_at(x, z).is_none();
        let mut best: Option<Vec2> = None;
        'search: for iz in 6..n - 6 {
            for ix in 6..n - 6 {
                if roads.kind_at(ix, iz).is_none() {
                    continue;
                }
                if is_field(ix - 5, iz) && is_field(ix + 5, iz) {
                    best = Some(grid_pos(ix, iz));
                    break 'search;
                }
                if is_field(ix, iz - 5) && is_field(ix, iz + 5) {
                    best = Some(grid_pos(ix, iz));
                    break 'search;
                }
            }
        }
        let p = best.expect("no road-through-field spot found");
        println!("ROAD-THROUGH-FIELD target={:.0},{:.0}", p.x, p.y);
    }

    // Diagnostic, not an assertion: prints the owned field cell with the largest height
    // range among its immediate neighbours, as a MAP_VIEWER_TARGET to check the field
    // colour overlay against a steep hillside.
    //   cargo test --lib find_field_clip_hotspot -- --ignored --nocapture
    #[test]
    #[ignore]
    fn find_field_clip_hotspot() {
        let (map, zones, params) = generate();
        let is_farmland = farmland(&map, &zones);
        let seeds = scatter_seeds(&map, &is_farmland, &params);
        let owner = smooth_owners(&map, &is_farmland, claim_regions(&map, &is_farmland, &seeds, &params), SMOOTH_PASSES);
        let owner = split_disconnected_regions(&map, &is_farmland, &owner);
        let n = map.grid_size();

        let mut best: Option<(f32, Vec2)> = None;
        for iz in 3..n - 3 {
            for ix in 3..n - 3 {
                if owner[iz * n + ix].is_none() {
                    continue;
                }
                let p = grid_pos(ix, iz);
                let mut lo = f32::MAX;
                let mut hi = f32::MIN;
                for (dx, dz) in [(-15.0, -15.0), (15.0, -15.0), (15.0, 15.0), (-15.0, 15.0), (0.0, 0.0)] {
                    let h = map.height_at(p + Vec2::new(dx, dz));
                    lo = lo.min(h);
                    hi = hi.max(h);
                }
                let range = hi - lo;
                if best.map_or(true, |(b, _)| range > b) {
                    best = Some((range, p));
                }
            }
        }
        let (range, p) = best.expect("no owned field cells found");
        println!("FIELD HOTSPOT target={:.0},{:.0} height_range_over_30m={:.1}", p.x, p.y, range);
    }
}
