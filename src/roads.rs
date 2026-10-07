use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::map::{grid_pos, PoiKind, TerrainMap, CELL, HALF_SIZE};
use crate::params::GenParams;

const ROAD_LIFT: f32 = 0.25;
const MAJOR_HALF_WIDTH: f32 = 4.0;
const MINOR_HALF_WIDTH: f32 = 2.2;

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
    edges: Vec<(usize, usize, RoadKind)>,
    verts: usize,
}

impl RoadNetwork {
    pub fn generate(map: &TerrainMap, params: &GenParams) -> Self {
        let n = map.grid_size();
        let mut cells: Vec<Option<RoadKind>> = vec![None; n * n];
        let mut edges: Vec<(usize, usize, RoadKind)> = Vec::new();

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
            grow_network(map, vec![first], rest.to_vec(), RoadKind::Major, params, &mut cells, &mut edges);
        }

        let seeds: Vec<usize> = hubs
            .iter()
            .copied()
            .chain((0..n * n).filter(|&i| cells[i].is_some()))
            .collect();
        grow_network(map, seeds, farms, RoadKind::Minor, params, &mut cells, &mut edges);

        RoadNetwork { cells, edges, verts: n }
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
fn grow_network(
    map: &TerrainMap,
    seeds: Vec<usize>,
    targets: Vec<usize>,
    kind: RoadKind,
    params: &GenParams,
    cells: &mut [Option<RoadKind>],
    edges: &mut Vec<(usize, usize, RoadKind)>,
) {
    let mut connected = seeds;
    let mut remaining: HashSet<usize> = targets.into_iter().collect();

    while !remaining.is_empty() {
        let Some((reached, previous)) = dijkstra_to_any(map, &connected, &remaining, kind, params, cells) else {
            break;
        };
        // Walk back from the newly reached settlement toward the network, recording each
        // step as an edge. Stop as soon as a cell that was already part of the network is
        // hit, since the rest of the path back to its root was already recorded earlier -
        // without this, every later connection would re-walk and re-record the whole shared
        // corridor back to the root, multiplying the ribbon geometry.
        let mut current = reached;
        loop {
            if cells[current].is_some() {
                break;
            }
            cells[current] = Some(kind);
            match previous[current] {
                Some(p) => {
                    edges.push((current, p, kind));
                    current = p;
                }
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
    params: &GenParams,
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
            let step_cost = step_cost(map, cells, c, neighbour, kind, params);
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

fn step_cost(map: &TerrainMap, cells: &[Option<RoadKind>], from: usize, to: usize, kind: RoadKind, params: &GenParams) -> f32 {
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
    let slope_penalty = slope_penalty * params.road_slope_scale;
    let water_cost = water_cost * params.road_water_scale;
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

// Road edges form a tree of grid-cell steps. They're first chained into polylines that
// break wherever roads meet or end, each polyline is relaxed and corner-cut (Chaikin) into a
// smooth curve, and then a ribbon with mitred sides is laid along it. Vertex heights come
// from the smooth, bilinearly interpolated terrain height rather than the grid vertex, and
// are raised above any water for bridges.
pub fn road_mesh(map: &TerrainMap, roads: &RoadNetwork, kind: RoadKind) -> Mesh {
    let half_width = if kind == RoadKind::Major { MAJOR_HALF_WIDTH } else { MINOR_HALF_WIDTH };
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    let n = map.grid_size();
    for chain in road_chains(roads, kind) {
        let points = smooth_polyline(chain.iter().map(|&c| idx_to_pos(map, c)).collect());
        if points.len() < 2 {
            continue;
        }
        let base = positions.len() as u32;
        for (i, &p) in points.iter().enumerate() {
            let prev = points[i.saturating_sub(1)];
            let next = points[(i + 1).min(points.len() - 1)];
            let tangent = (next - prev).normalize_or_zero();
            let perp = Vec2::new(-tangent.y, tangent.x);
            // Widen a little on bends so the ribbon keeps its width, capped so a sharp
            // kink can't spike.
            let bend = if i > 0 && i + 1 < points.len() {
                let (d0, d1) = ((p - prev).normalize_or_zero(), (next - p).normalize_or_zero());
                (1.0 / ((1.0 + d0.dot(d1)) * 0.5).max(0.01).sqrt()).min(1.5)
            } else {
                1.0
            };
            for side in [-1.0_f32, 1.0] {
                let q = p + perp * half_width * bend * side;
                let cell = nearest_cell(map, q);
                let ground = map.height_at(q).max(map.water_level(cell % n, cell / n).unwrap_or(f32::MIN));
                positions.push([q.x, ground + ROAD_LIFT, q.y]);
            }
        }
        for i in 0..points.len() as u32 - 1 {
            let (l0, r0, l1, r1) = (base + i * 2, base + i * 2 + 1, base + i * 2 + 2, base + i * 2 + 3);
            indices.extend_from_slice(&[l0, r1, l1, l0, r0, r1]);
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32(indices))
}

// Chains of cells for one road kind, each running between junctions / dead ends (a node
// where roads of any kind meet or stop). Chains are sequences of cell indices.
fn road_chains(roads: &RoadNetwork, kind: RoadKind) -> Vec<Vec<usize>> {
    let mut degree: HashMap<usize, usize> = HashMap::new();
    let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
    for &(a, b, k) in &roads.edges {
        *degree.entry(a).or_default() += 1;
        *degree.entry(b).or_default() += 1;
        if k == kind {
            adj.entry(a).or_default().push(b);
            adj.entry(b).or_default().push(a);
        }
    }
    let is_break = |c: usize| degree[&c] != 2 || adj.get(&c).map_or(0, Vec::len) != 2;
    let key = |a: usize, b: usize| (a.min(b), a.max(b));
    let mut used: HashSet<(usize, usize)> = HashSet::new();
    let mut chains = Vec::new();
    let walk = |start: usize, first: usize, used: &mut HashSet<(usize, usize)>| {
        let mut chain = vec![start, first];
        used.insert(key(start, first));
        let mut current = first;
        while !is_break(current) {
            let next = adj[&current].iter().copied().find(|&nb| !used.contains(&key(current, nb)));
            let Some(next) = next else { break };
            used.insert(key(current, next));
            chain.push(next);
            current = next;
        }
        chain
    };
    let mut nodes: Vec<usize> = adj.keys().copied().collect();
    nodes.sort_unstable();
    for &node in nodes.iter().filter(|&&c| is_break(c)) {
        for &nb in &adj[&node] {
            if !used.contains(&key(node, nb)) {
                chains.push(walk(node, nb, &mut used));
            }
        }
    }
    // Anything left is a closed loop with no break node.
    for &node in &nodes {
        for &nb in &adj[&node] {
            if !used.contains(&key(node, nb)) {
                chains.push(walk(node, nb, &mut used));
            }
        }
    }
    chains
}

// Relaxes a grid-step path so it stops zigzagging, then cuts its corners twice (Chaikin),
// which converges on a smooth curve while staying inside the original path's hull. The two
// end points never move, so chains still meet exactly at junctions.
fn smooth_polyline(mut pts: Vec<Vec2>) -> Vec<Vec2> {
    for _ in 0..2 {
        if pts.len() < 3 {
            break;
        }
        let prev = pts.clone();
        for i in 1..pts.len() - 1 {
            pts[i] = (prev[i - 1] + prev[i] * 2.0 + prev[i + 1]) * 0.25;
        }
    }
    for _ in 0..2 {
        if pts.len() < 3 {
            break;
        }
        let mut out = Vec::with_capacity(pts.len() * 2);
        out.push(pts[0]);
        for w in pts.windows(2) {
            out.push(w[0] * 0.75 + w[1] * 0.25);
            out.push(w[0] * 0.25 + w[1] * 0.75);
        }
        out.push(pts[pts.len() - 1]);
        pts = out;
    }
    pts
}

fn idx_to_pos(map: &TerrainMap, idx: usize) -> Vec2 {
    let n = map.grid_size();
    grid_pos(idx % n, idx / n)
}
