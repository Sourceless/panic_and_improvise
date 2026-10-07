
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::contour::{clip_to_terrain_triangle, triangulate, Contour, OPEN};
use crate::map::{fbm, grid_pos, TerrainMap, CELL};
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
        return;
    }
    let owner = smooth_owners(map, &is_farmland, claim_regions(map, &is_farmland, &seeds, params), SMOOTH_PASSES);
    // A field can end up split into pieces that aren't actually touching - most often
    // because a road cuts through it, with the two halves otherwise reconnecting around the
    // road's ends. Giving every disconnected group its own fresh id makes a road-split field
    // render (and colour) as two separate fields instead of one that invisibly jumps the gap.
    let mut owner = split_disconnected_regions(map, &is_farmland, &owner);
    merge_tiny_fields(map, &mut owner);
    fill_small_holes(map, &mut owner, &keep_clear);
    let field_colour = build_field_colours(map, zones, &owner);

    let labels: Vec<u32> = owner.iter().map(|o| o.unwrap_or(OPEN)).collect();
    let contour = Contour::build(n, &labels, map.seed, None);
    spawn_field_colour(commands, meshes, materials, map, &labels, &contour, &field_colour);
    spawn_field_boundaries(commands, meshes, materials, map, &contour);
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

// Picks each field's colour from the first cell found for its id. Ids come from
// split_disconnected_regions rather than directly from the seed list, so two pieces of a
// road-split field get different ids and so different colours.
fn build_field_colours(map: &TerrainMap, zones: &ZoneMap, owner: &[Option<u32>]) -> Vec<[f32; 3]> {
    let n = map.grid_size();
    let field_count = owner.iter().filter_map(|&o| o).max().map_or(0, |m| m + 1) as usize;
    let mut colours: Vec<Option<[f32; 3]>> = vec![None; field_count];
    for (idx, &o) in owner.iter().enumerate() {
        let Some(id) = o else { continue };
        let slot = &mut colours[id as usize];
        if slot.is_some() {
            continue;
        }
        let (ix, iz) = (idx % n, idx / n);
        let p = grid_pos(ix, iz);
        let block = (
            ((p.x + crate::map::HALF_SIZE) / 150.0) as usize,
            ((p.y + crate::map::HALF_SIZE) / 150.0) as usize,
        );
        *slot = Some(if zones.zone_at(ix, iz) == Zone::Arable {
            crop_colour(block.0, block.1)
        } else {
            pasture_colour(block.0, block.1)
        });
    }
    colours.into_iter().map(|c| c.unwrap_or([1.0, 0.0, 1.0])).collect()
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
    labels: &[u32],
    contour: &Contour,
    field_colour: &[[f32; 3]],
) {
    // Each cell is clipped to the same smoothed contour the boundary walls follow
    // (see Contour::cell_regions), so the colour edge meets the wall exactly and a field's outline
    // against open ground is as smooth as one between two fields. Each clipped polygon is
    // then split along the terrain's own diagonal and heighted per triangle, so it stays
    // coplanar with the real terrain.
    let n = map.grid_size();
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for iz in 0..n - 1 {
        for ix in 0..n - 1 {
            let get = |x: usize, z: usize| labels[z * n + x];
            let cell = [get(ix, iz), get(ix + 1, iz), get(ix + 1, iz + 1), get(ix, iz + 1)];
            if cell.iter().all(|&o| o == OPEN) {
                continue;
            }
            for (id, poly) in contour.cell_regions(ix, iz, cell) {
                let c = field_colour[id as usize];
                let colour = [c[0], c[1], c[2], 1.0];
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
                                    let base = positions.len() as u32;
                                    for p in t {
                                        let w = origin + p * CELL;
                                        positions.push([w.x, height(p), w.y]);
                                        colors.push(colour);
                                    }
                                    indices.extend_from_slice(&[base, base + 1, base + 2]);
                                }
                            }
                        }
                    }
                }
            }
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

fn spawn_field_boundaries(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    map: &TerrainMap,
    contour: &Contour,
) {
    let segs = &contour.segs;

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
    let mut buckets: [Vec<&crate::contour::Seg>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for seg in segs {
        buckets[pair_hash(seg.pair.0, seg.pair.1) as usize % 3].push(seg);
    }
    for (bucket, (material, thickness, height)) in buckets.into_iter().zip(specs) {
        if bucket.is_empty() {
            continue;
        }
        let mat = materials.add(material);
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut indices = Vec::new();
        for seg in bucket {
            // Each segment is already short (at most one grid cell across), and pins to the
            // true ground height at both of its own endpoints - the same trick road_mesh
            // uses for its ribbons - so it follows the terrain tightly without needing any
            // further subdivision.
            let ground_a = map.height_at(seg.a);
            let ground_b = map.height_at(seg.b);
            push_wall_segment(&mut positions, &mut normals, &mut indices, seg.a, ground_a, seg.b, ground_b, thickness, height);
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

// A short prism between two points, each pinned to its own ground height, rather than a
// box translated to one flat height - so a chain of these follows the ground rising and
// falling along its length exactly at each sample point, the same technique road_mesh uses
// for its ribbons, extended with a top, two sides and end caps for real thickness/height.
fn push_wall_segment(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    indices: &mut Vec<u32>,
    a: Vec2,
    ground_a: f32,
    b: Vec2,
    ground_b: f32,
    thickness: f32,
    height: f32,
) {
    let dir = (b - a).normalize_or_zero();
    let perp = Vec2::new(-dir.y, dir.x) * (thickness * 0.5);
    let dir3 = Vec3::new(dir.x, 0.0, dir.y);
    let perp3 = Vec3::new(perp.x, 0.0, perp.y).normalize_or_zero();

    let corner = |p: Vec2, sign: f32, y: f32| {
        let c = p + perp * sign;
        Vec3::new(c.x, y, c.y)
    };
    let a_left_bot = corner(a, -1.0, ground_a);
    let a_left_top = corner(a, -1.0, ground_a + height);
    let a_right_bot = corner(a, 1.0, ground_a);
    let a_right_top = corner(a, 1.0, ground_a + height);
    let b_left_bot = corner(b, -1.0, ground_b);
    let b_left_top = corner(b, -1.0, ground_b + height);
    let b_right_bot = corner(b, 1.0, ground_b);
    let b_right_top = corner(b, 1.0, ground_b + height);

    let mut quad = |corners: [Vec3; 4], normal: Vec3| {
        let base = positions.len() as u32;
        for c in corners {
            positions.push([c.x, c.y, c.z]);
            normals.push([normal.x, normal.y, normal.z]);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    };

    // The underside sits exactly on the ground and is never seen, so it's skipped.
    quad([a_left_top, a_right_top, b_right_top, b_left_top], Vec3::Y);
    quad([a_left_bot, a_left_top, b_left_top, b_left_bot], -perp3);
    quad([a_right_top, a_right_bot, b_right_bot, b_right_top], perp3);
    quad([a_left_top, a_left_bot, a_right_bot, a_right_top], -dir3);
    quad([b_left_bot, b_left_top, b_right_top, b_right_bot], dir3);
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
        let segs = Contour::build(map.grid_size(), &labels, map.seed, None).segs;
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
