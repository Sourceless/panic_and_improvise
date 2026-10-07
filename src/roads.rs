use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::map::{grid_pos, PoiKind, TerrainMap, CELL, HALF_SIZE};

const ROAD_LIFT: f32 = 0.25;

// Major roads (between villages and the mill) tolerate more climbing and wider water
// crossings than minor roads (farm tracks), which stick closer to flat, dry ground.
const MAJOR_SLOPE_PENALTY: f32 = 4.0;
const MAJOR_WATER_COST: f32 = 3.0;
const MINOR_SLOPE_PENALTY: f32 = 9.0;
const MINOR_WATER_COST: f32 = 6.0;
const EXISTING_ROAD_DISCOUNT: f32 = 0.12;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoadKind {
    Major,
    Minor,
}

#[derive(Resource)]
pub struct RoadNetwork {
    cells: Vec<Option<RoadKind>>,
    verts: usize,
}

impl RoadNetwork {
    pub fn generate(map: &TerrainMap) -> Self {
        let n = map.grid_size();
        let mut cells: Vec<Option<RoadKind>> = vec![None; n * n];

        let hubs: Vec<usize> = map
            .pois
            .iter()
            .filter(|p| matches!(p.kind, PoiKind::Village | PoiKind::Mill))
            .map(|p| nearest_cell(map, p.position))
            .collect();
        let farms: Vec<usize> = map
            .pois
            .iter()
            .filter(|p| p.kind == PoiKind::Farm)
            .map(|p| nearest_cell(map, p.position))
            .collect();

        if let Some((&first, rest)) = hubs.split_first() {
            grow_network(map, vec![first], rest.to_vec(), RoadKind::Major, &mut cells);
        }

        let seeds: Vec<usize> = hubs
            .iter()
            .copied()
            .chain((0..n * n).filter(|&i| cells[i].is_some()))
            .collect();
        grow_network(map, seeds, farms, RoadKind::Minor, &mut cells);

        RoadNetwork { cells, verts: n }
    }

    pub fn kind_at(&self, ix: usize, iz: usize) -> Option<RoadKind> {
        self.cells[iz * self.verts + ix]
    }

    pub fn length(&self, kind: RoadKind) -> f32 {
        self.cells.iter().filter(|c| **c == Some(kind)).count() as f32 * CELL
    }
}

// Grows the network by repeatedly finding, with a single Dijkstra search seeded from
// everything already connected, the cheapest path to the nearest still-unconnected
// target. The search stops as soon as it reaches any target, so each step costs roughly
// the distance to the nearest unconnected settlement rather than the whole grid.
fn grow_network(map: &TerrainMap, seeds: Vec<usize>, targets: Vec<usize>, kind: RoadKind, cells: &mut [Option<RoadKind>]) {
    let mut connected = seeds;
    let mut remaining: HashSet<usize> = targets.into_iter().collect();

    while !remaining.is_empty() {
        let Some((reached, previous)) = dijkstra_to_any(map, &connected, &remaining, kind, cells) else {
            break;
        };
        let mut current = reached;
        loop {
            if cells[current].is_none() {
                cells[current] = Some(kind);
            }
            match previous[current] {
                Some(p) => current = p,
                None => break,
            }
        }
        remaining.remove(&reached);
        connected.push(reached);
    }
}

fn dijkstra_to_any(
    map: &TerrainMap,
    sources: &[usize],
    targets: &HashSet<usize>,
    kind: RoadKind,
    cells: &[Option<RoadKind>],
) -> Option<(usize, Vec<Option<usize>>)> {
    let n = map.grid_size();
    let count = n * n;
    let mut cost = vec![f32::MAX; count];
    let mut previous: Vec<Option<usize>> = vec![None; count];
    let mut heap = BinaryHeap::new();
    for &s in sources {
        if cost[s] > 0.0 {
            cost[s] = 0.0;
            heap.push(Reverse((0u32, s as u32)));
        }
    }

    while let Some(Reverse((_, current))) = heap.pop() {
        let c = current as usize;
        if targets.contains(&c) {
            return Some((c, previous));
        }
        for neighbour in neighbours(n, c) {
            let step_cost = step_cost(map, cells, c, neighbour, kind);
            let next = cost[c] + step_cost;
            if next < cost[neighbour] {
                cost[neighbour] = next;
                previous[neighbour] = Some(c);
                heap.push(Reverse((next.to_bits(), neighbour as u32)));
            }
        }
    }
    None
}

fn step_cost(map: &TerrainMap, cells: &[Option<RoadKind>], from: usize, to: usize, kind: RoadKind) -> f32 {
    let n = map.grid_size();
    let diagonal = (to % n != from % n) && (to / n != from / n);
    let step = if diagonal { CELL * std::f32::consts::SQRT_2 } else { CELL };
    let (fx, fz) = (from % n, from / n);
    let (tx, tz) = (to % n, to / n);
    let climb = (map.vertex_height(tx, tz) - map.vertex_height(fx, fz)).abs() / CELL;
    let (slope_penalty, water_cost) = match kind {
        RoadKind::Major => (MAJOR_SLOPE_PENALTY, MAJOR_WATER_COST),
        RoadKind::Minor => (MINOR_SLOPE_PENALTY, MINOR_WATER_COST),
    };
    let mut c = step * (1.0 + slope_penalty * climb);
    if map.water_level(tx, tz).is_some() {
        c += step * water_cost;
    }
    if cells[to].is_some() {
        c *= EXISTING_ROAD_DISCOUNT;
    }
    c
}

fn neighbours(n: usize, idx: usize) -> impl Iterator<Item = usize> {
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

fn nearest_cell(map: &TerrainMap, p: Vec2) -> usize {
    let n = map.grid_size();
    let ix = ((p.x + HALF_SIZE) / CELL).round().clamp(0.0, (n - 1) as f32) as usize;
    let iz = ((p.y + HALF_SIZE) / CELL).round().clamp(0.0, (n - 1) as f32) as usize;
    iz * n + ix
}

// A quad per road cell, lifted above the terrain (and above any water, for bridges).
pub fn road_mesh(map: &TerrainMap, roads: &RoadNetwork, kind: RoadKind) -> Mesh {
    let n = map.grid_size();
    // Full cell width so adjacent road cells tile with no gaps, matching how water and
    // fields are rendered elsewhere on the map.
    let half = CELL * 0.5;
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for iz in 0..n {
        for ix in 0..n {
            if roads.kind_at(ix, iz) != Some(kind) {
                continue;
            }
            let p = grid_pos(ix, iz);
            let ground = map.vertex_height(ix, iz).max(map.water_level(ix, iz).unwrap_or(f32::MIN));
            let y = ground + ROAD_LIFT;
            let base = positions.len() as u32;
            for (dx, dz) in [(-half, -half), (half, -half), (half, half), (-half, half)] {
                positions.push([p.x + dx, y, p.y + dz]);
            }
            indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32(indices))
}
