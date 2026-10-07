
use std::cmp::Reverse;
use std::collections::BinaryHeap;

use bevy::prelude::*;

use crate::params::GenParams;

pub const MAP_SIZE: f32 = 5000.0;
pub const HALF_SIZE: f32 = MAP_SIZE / 2.0;
pub const CELL: f32 = 10.0;
/// World meshes (terrain, fields, boundaries, water) are split into tiles this many cells wide,
/// so frustum culling can skip what is off-screen instead of drawing the whole map each pass.
pub const TILE_CELLS: usize = 32;
pub const CELLS: usize = (MAP_SIZE / CELL) as usize;
const VERTS: usize = CELLS + 1;
const COUNT: usize = VERTS * VERTS;

const RIVER_MIN_SPAN: f32 = 3500.0;
const NONE: u32 = u32::MAX;
const RIVER_ELEVATION_SCALE: f32 = 12.0;
const RIVER_WIGGLE: f32 = 4.0;
const VALLEY_DEPTH: f32 = 22.0;
const VALLEY_FLAT: f32 = 30.0;
const VALLEY_WIDTH: f32 = 150.0;
const WATER_SURFACE_BELOW_FILL: f32 = 0.2;
const RIVER_FILL_WIDTH: f32 = 18.0;
const RIVER_LEVEL_WINDOW: usize = 10;
const RIVER_FILL_OVERFLOW: f32 = 2.0;
const RIVER_BED_BELOW_WATER: f32 = 1.5;
const CARVE_CORE: f32 = CELL * 0.5;
const CARVE_BANK: f32 = CELL * 4.0;
const VILLAGE_CHURCH_SEARCH: f32 = 40.0;
const POI_MIN_SPACING: f32 = 250.0;
const SETTLEMENT_WATER_REACH: f32 = 500.0;
// Farms and hamlets (anything under SMALL_SETTLEMENT_RADIUS) have no river requirement at
// all, so settlements spread across the whole map instead of crowding the river's banks.
pub const SMALL_SETTLEMENT_RADIUS: f32 = 45.0;
// Settlements at least this big are only sited on level ground, since flattening a large
// footprint on a slope would carve an obvious terrace into the hillside.
const LARGE_SETTLEMENT_RADIUS: f32 = 100.0;


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoiKind {
    Village,
    Farm,
    Mill,
}

#[derive(Clone, Copy, Debug)]
pub struct Poi {
    pub kind: PoiKind,
    pub position: Vec2,
    pub landmark: Vec2,
    /// Footprint radius: the area flattened and built over. Villages vary widely in size.
    pub radius: f32,
}

#[derive(Resource)]
pub struct TerrainMap {
    pub seed: u64,
    heights: Vec<f32>,
    water: Vec<Option<f32>>,
    river_distance: Vec<f32>,
    water_distance: Vec<f32>,
    river_len: f32,
    pub pois: Vec<Poi>,
}

impl TerrainMap {
    pub fn generate(seed: u64, params: &GenParams) -> Self {
        let mut rng = Rng(seed);

        let mut base: Vec<f32> = (0..COUNT)
            .map(|idx| {
                let (ix, iz) = (idx % VERTS, idx / VERTS);
                let p = grid_pos(ix, iz);
                coastal_height(base_height(p, seed, params), p, seed, params)
            })
            .collect();
        let path = river_path(&base, seed, params, &mut Rng(seed ^ 0x51DE_0001));
        carve_valley(&mut base, &path);
        for h in &mut base {
            *h /= EROSION_HEIGHT_UNIT;
        }
        erode(&mut base, params.erosion_droplets, &mut Rng(seed ^ 0xE20D_E000));
        for h in &mut base {
            *h *= EROSION_HEIGHT_UNIT;
        }

        let mut is_river = vec![false; COUNT];
        let mut level: Vec<f32> = base.iter().map(|h| h - WATER_SURFACE_BELOW_FILL).collect();
        let raw: Vec<f32> = path
            .iter()
            .map(|&i| if base[i] < 0.0 { 0.0 } else { base[i] - WATER_SURFACE_BELOW_FILL })
            .collect();
        for (k, &i) in path.iter().enumerate() {
            is_river[i] = true;
            let window = k.saturating_sub(RIVER_LEVEL_WINDOW)..(k + RIVER_LEVEL_WINDOW + 1).min(path.len());
            level[i] = raw[window].iter().cloned().fold(f32::MAX, f32::min);
        }
        let river_len = path.windows(2).map(|w| grid_pos_of(w[0]).distance(grid_pos_of(w[1]))).sum();
        let (river_distance, river_level) = distance_to_rivers(&is_river, &level);

        let heights: Vec<f32> = (0..COUNT)
            .map(|idx| {
                let blend = 1.0 - smoothstep(CARVE_CORE, CARVE_BANK, river_distance[idx]);
                let bed = base[idx].min(river_level[idx] - RIVER_BED_BELOW_WATER);
                base[idx] + (bed - base[idx]) * blend
            })
            .collect();

        let water: Vec<Option<f32>> = (0..COUNT)
            .map(|idx| {
                if is_river[idx] {
                    Some(level[idx])
                } else if river_distance[idx] < RIVER_FILL_WIDTH && heights[idx] < river_level[idx] + RIVER_FILL_OVERFLOW {
                    Some(river_level[idx])
                } else if heights[idx] < 0.0 {
                    Some(0.0)
                } else {
                    None
                }
            })
            .collect();

        let is_water: Vec<bool> = water.iter().map(Option::is_some).collect();
        let (water_distance, _) = distance_to_rivers(&is_water, &vec![0.0; COUNT]);

        let mut map = TerrainMap {
            seed,
            heights,
            water,
            river_distance,
            water_distance,
            river_len,
            pois: Vec::new(),
        };
        map.pois = place_pois(&mut rng, &map);
        map.flatten_settlements();
        map
    }

    fn flatten_settlements(&mut self) {
        for poi in self.pois.clone() {
            let ground = self.height_at(poi.position);
            let radius = poi.radius;
            for iz in 0..VERTS {
                for ix in 0..VERTS {
                    let d = grid_pos(ix, iz).distance(poi.position);
                    if d < radius {
                        let t = smoothstep(radius * 0.6, radius, d);
                        let idx = iz * VERTS + ix;
                        if self.water[idx].is_none() {
                            self.heights[idx] = ground + (self.heights[idx] - ground) * t;
                        }
                    }
                }
            }
        }
    }

    pub fn flat(height: f32) -> Self {
        TerrainMap {
            seed: 0,
            heights: vec![height; COUNT],
            water: vec![None; COUNT],
            river_distance: vec![f32::MAX; COUNT],
            water_distance: vec![f32::MAX; COUNT],
            river_len: 0.0,
            pois: Vec::new(),
        }
    }

    pub fn height_at(&self, p: Vec2) -> f32 {
        let gx = ((p.x + HALF_SIZE) / CELL).clamp(0.0, CELLS as f32 - 0.001);
        let gz = ((p.y + HALF_SIZE) / CELL).clamp(0.0, CELLS as f32 - 0.001);
        let (ix, iz) = (gx.floor() as usize, gz.floor() as usize);
        let (fx, fz) = (gx - ix as f32, gz - iz as f32);
        let h = |x: usize, z: usize| self.heights[z * VERTS + x];
        let top = h(ix, iz) + (h(ix + 1, iz) - h(ix, iz)) * fx;
        let bottom = h(ix, iz + 1) + (h(ix + 1, iz + 1) - h(ix, iz + 1)) * fx;
        top + (bottom - top) * fz
    }

    pub fn vertex_height(&self, ix: usize, iz: usize) -> f32 {
        self.heights[iz * VERTS + ix]
    }

    pub fn water_level(&self, ix: usize, iz: usize) -> Option<f32> {
        self.water[iz * VERTS + ix]
    }

    pub fn river_distance(&self, p: Vec2) -> f32 {
        let (ix, iz) = nearest_vertex(p);
        self.river_distance[iz * VERTS + ix]
    }

    // Distance to the nearest water of any kind (river or lake), unlike river_distance
    // which only measures distance to the flowing river channel.
    pub fn water_distance(&self, p: Vec2) -> f32 {
        let (ix, iz) = nearest_vertex(p);
        self.water_distance[iz * VERTS + ix]
    }

    pub fn height_range(&self) -> (f32, f32) {
        self.heights
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), &h| (lo.min(h), hi.max(h)))
    }

    /// Where the player starts: the top of the tallest peak, or the origin on a flat map.
    pub fn spawn_point(&self) -> Vec2 {
        let (lo, hi) = self.height_range();
        if hi - lo < 1e-3 {
            return Vec2::ZERO;
        }
        let top = self.heights.iter().enumerate().fold((0, f32::MIN), |best, (i, &h)| if h > best.1 { (i, h) } else { best });
        grid_pos(top.0 % VERTS, top.0 / VERTS)
    }

    pub fn grid_size(&self) -> usize {
        VERTS
    }

    pub fn river_length(&self) -> f32 {
        self.river_len
    }
}

pub fn grid_pos(ix: usize, iz: usize) -> Vec2 {
    Vec2::new(-HALF_SIZE + ix as f32 * CELL, -HALF_SIZE + iz as f32 * CELL)
}

fn nearest_vertex(p: Vec2) -> (usize, usize) {
    let ix = ((p.x + HALF_SIZE) / CELL).round().clamp(0.0, CELLS as f32) as usize;
    let iz = ((p.y + HALF_SIZE) / CELL).round().clamp(0.0, CELLS as f32) as usize;
    (ix, iz)
}

pub fn fbm(x: f32, z: f32, seed: u64, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut norm, mut freq) = (0.0, 0.5, 0.0, 1.0);
    for octave in 0..octaves {
        sum += amp * value_noise(x * freq, z * freq, seed.wrapping_add(octave as u64));
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

// Layered noise: a domain-warped broad field, then rolling and small undulations.
fn base_height(p: Vec2, seed: u64, params: &GenParams) -> f32 {
    let warp = Vec2::new(
        fbm(p.x / 1200.0, p.y / 1200.0, seed ^ 0x1, 3) - 0.5,
        fbm(p.x / 1200.0 + 7.3, p.y / 1200.0 + 1.9, seed ^ 0x2, 3) - 0.5,
    ) * 900.0;
    let q = p + warp;
    let broad = fbm(q.x / 1400.0, q.y / 1400.0, seed, 3) - 0.5;
    let rolling = fbm(q.x / 500.0, q.y / 500.0, seed ^ 0x3, 4) - 0.5;
    let small = fbm(q.x / 140.0, q.y / 140.0, seed ^ 0x4, 4) - 0.5;
    (broad * 320.0 + rolling * 120.0 + small * 35.0) * params.relief_scale + 50.0
}

// How far below sea level the sea floor drops at the very edge of the map.
const SEA_FLOOR: f32 = -55.0;

// Slopes the land down into the sea toward the map edge, so the world ends in a coastline
// (with beaches where the land crosses sea level) instead of a hard cut. The band's width is
// varied by noise so the coast wanders rather than following a straight, square line.
fn coastal_height(h: f32, p: Vec2, seed: u64, params: &GenParams) -> f32 {
    let width = params.coast_width;
    if width <= 0.0 {
        return h;
    }
    let edge_distance = (HALF_SIZE - p.x.abs()).min(HALF_SIZE - p.y.abs());
    let wobble = (fbm(p.x / 380.0, p.y / 380.0, seed ^ 0xC0A5, 3) - 0.5) * 2.0 * width * 0.5;
    let t = ((edge_distance + wobble) / width).clamp(0.0, 1.0);
    let sunk = 1.0 - smoothstep(0.0, 1.0, t);
    h + (SEA_FLOOR - h) * sunk.powf(1.3)
}

fn neighbours(idx: usize) -> impl Iterator<Item = usize> {
    let (ix, iz) = ((idx % VERTS) as isize, (idx / VERTS) as isize);
    (-1..=1isize)
        .flat_map(move |dz| (-1..=1isize).map(move |dx| (dx, dz)))
        .filter(|&(dx, dz)| (dx, dz) != (0, 0))
        .filter_map(move |(dx, dz)| {
            let (x, z) = (ix + dx, iz + dz);
            let inside = (0..VERTS as isize).contains(&x) && (0..VERTS as isize).contains(&z);
            inside.then(|| z as usize * VERTS + x as usize)
        })
}

fn on_edge(idx: usize, side: usize) -> bool {
    let (ix, iz) = (idx % VERTS, idx / VERTS);
    match side {
        0 => iz == 0,
        1 => iz == VERTS - 1,
        2 => ix == 0,
        _ => ix == VERTS - 1,
    }
}

// The main river enters at a random point on one edge and crosses to the opposite edge
// along the cheapest path, where climbing costs more than flowing, so it follows low
// ground and the valleys that erosion carved.
fn river_path(heights: &[f32], seed: u64, params: &GenParams, rng: &mut Rng) -> Vec<usize> {
    let side = (rng.range(0.0, 4.0) as usize).min(3);
    let along = (rng.range(0.2, 0.8) * (VERTS - 1) as f32) as usize;
    let source = match side {
        0 => along,
        1 => (VERTS - 1) * VERTS + along,
        2 => along * VERTS,
        _ => along * VERTS + VERTS - 1,
    };
    let (sx, sz) = ((source % VERTS) as f32, (source / VERTS) as f32);
    let is_target = |c: usize| {
        let (dx, dz) = ((c % VERTS) as f32 - sx, (c / VERTS) as f32 - sz);
        (0..4).any(|s| s != side && on_edge(c, s)) && (dx * dx + dz * dz).sqrt() * CELL >= RIVER_MIN_SPAN
    };

    let mut cost = vec![f32::MAX; COUNT];
    let mut previous = vec![NONE; COUNT];
    let mut heap = BinaryHeap::new();
    cost[source] = 0.0;
    heap.push(Reverse((0u32, source as u32)));
    let mut end = None;
    while let Some(Reverse((_, current))) = heap.pop() {
        let c = current as usize;
        if is_target(c) {
            end = Some(c);
            break;
        }
        for n in neighbours(c) {
            let diagonal = (n % VERTS != c % VERTS) && (n / VERTS != c / VERTS);
            let step = if diagonal { CELL * std::f32::consts::SQRT_2 } else { CELL };
            let climb = (heights[n] - heights[c]).max(0.0) / CELL;
            let wiggle = (1.0 + RIVER_WIGGLE * (value_noise((n % VERTS) as f32 / 6.0, (n / VERTS) as f32 / 6.0, seed) - 0.5)).max(0.1);
            let next_cost = cost[c] + step * wiggle * (1.0 + params.river_slope_weight * climb) * (1.0 + heights[n].max(0.0) / RIVER_ELEVATION_SCALE);
            if next_cost < cost[n] {
                cost[n] = next_cost;
                previous[n] = current;
                heap.push(Reverse((next_cost.to_bits(), n as u32)));
            }
        }
    }

    let mut path = Vec::new();
    let mut current = end.expect("river reaches the opposite edge");
    loop {
        path.push(current);
        if current == source {
            break;
        }
        current = previous[current] as usize;
    }
    path.reverse();
    path
}

// Cuts a broad valley floor along the river so erosion refines an existing valley
// rather than having to create one.
fn carve_valley(heights: &mut [f32], path: &[usize]) {
    let mut is_path = vec![false; COUNT];
    let mut path_heights = vec![0.0; COUNT];
    for &i in path {
        is_path[i] = true;
        path_heights[i] = heights[i];
    }
    let (dist, floor_source) = distance_to_rivers(&is_path, &path_heights);
    for idx in 0..COUNT {
        let target = floor_source[idx] - VALLEY_DEPTH;
        let t = 1.0 - smoothstep(VALLEY_FLAT, VALLEY_WIDTH, dist[idx]);
        if heights[idx] > target {
            heights[idx] += (target - heights[idx]) * t;
        }
    }
}

fn grid_pos_of(idx: usize) -> Vec2 {
    grid_pos(idx % VERTS, idx / VERTS)
}


const DROPLET_LIFETIME: usize = 80;
const DROPLET_INERTIA: f32 = 0.1;
const DROPLET_CAPACITY: f32 = 1.0;
const DROPLET_STEP_LIMIT: f32 = 0.2;
const DROPLET_MIN_SLOPE: f32 = 0.01;
const DROPLET_ERODE: f32 = 0.4;
const DROPLET_DEPOSIT: f32 = 0.2;
const DROPLET_EVAPORATE: f32 = 0.02;
const DROPLET_GRAVITY: f32 = 4.0;
const EROSION_RADIUS: isize = 2;
// Droplet erosion is tuned for heights of order one, so it runs in units of this many metres.
const EROSION_HEIGHT_UNIT: f32 = 50.0;

// Particle-based hydraulic erosion (after Lague, and Mei et al. 2007): water droplets roll
// downhill, carry sediment up to a capacity set by speed and slope, erode when under
// capacity and deposit when over it.
fn erode(heights: &mut [f32], droplets: u32, rng: &mut Rng) {
    let max = (VERTS - 1) as f32;
    for _ in 0..droplets {
        let mut pos = Vec2::new(rng.range(0.0, max), rng.range(0.0, max));
        let mut dir = Vec2::ZERO;
        let mut speed = 1.0_f32;
        let mut water = 1.0_f32;
        let mut sediment = 0.0_f32;

        for _ in 0..DROPLET_LIFETIME {
            let (grad, h) = gradient(heights, pos);
            dir = (dir * DROPLET_INERTIA - grad * (1.0 - DROPLET_INERTIA)).normalize_or_zero();
            if dir == Vec2::ZERO {
                break;
            }
            let next = pos + dir;
            if next.x < 0.0 || next.y < 0.0 || next.x >= max || next.y >= max {
                break;
            }
            let delta = sample(heights, next) - h;

            let capacity = (-delta).max(DROPLET_MIN_SLOPE) * speed * water * DROPLET_CAPACITY;
            if sediment > capacity || delta > 0.0 {
                let amount = if delta > 0.0 {
                    delta.min(sediment)
                } else {
                    (sediment - capacity) * DROPLET_DEPOSIT
                };
                sediment -= amount;
                deposit(heights, pos, amount);
            } else {
                let amount = ((capacity - sediment) * DROPLET_ERODE).min(-delta).min(DROPLET_STEP_LIMIT);
                sediment += amount;
                erode_brush(heights, pos, amount);
            }

            speed = (speed * speed - delta * DROPLET_GRAVITY).max(0.0).sqrt();
            water *= 1.0 - DROPLET_EVAPORATE;
            pos = next;
        }
    }
}

fn cell(heights: &[f32], ix: usize, iz: usize) -> f32 {
    heights[iz * VERTS + ix]
}

fn sample(heights: &[f32], p: Vec2) -> f32 {
    gradient(heights, p).1
}

// Bilinear height and its gradient (per grid cell) at a grid-space position.
fn gradient(heights: &[f32], p: Vec2) -> (Vec2, f32) {
    let (ix, iz) = (p.x.floor() as usize, p.y.floor() as usize);
    let (fx, fz) = (p.x - ix as f32, p.y - iz as f32);
    let (h00, h10) = (cell(heights, ix, iz), cell(heights, ix + 1, iz));
    let (h01, h11) = (cell(heights, ix, iz + 1), cell(heights, ix + 1, iz + 1));
    let gx = (h10 - h00) * (1.0 - fz) + (h11 - h01) * fz;
    let gz = (h01 - h00) * (1.0 - fx) + (h11 - h10) * fx;
    let h = h00 * (1.0 - fx) * (1.0 - fz) + h10 * fx * (1.0 - fz) + h01 * (1.0 - fx) * fz + h11 * fx * fz;
    (Vec2::new(gx, gz), h)
}

fn deposit(heights: &mut [f32], p: Vec2, amount: f32) {
    let (ix, iz) = (p.x.floor() as usize, p.y.floor() as usize);
    let (fx, fz) = (p.x - ix as f32, p.y - iz as f32);
    heights[iz * VERTS + ix] += amount * (1.0 - fx) * (1.0 - fz);
    heights[iz * VERTS + ix + 1] += amount * fx * (1.0 - fz);
    heights[(iz + 1) * VERTS + ix] += amount * (1.0 - fx) * fz;
    heights[(iz + 1) * VERTS + ix + 1] += amount * fx * fz;
}

fn erode_brush(heights: &mut [f32], p: Vec2, amount: f32) {
    let (cx, cz) = (p.x.round() as isize, p.y.round() as isize);
    let mut weights = Vec::new();
    for dz in -EROSION_RADIUS..=EROSION_RADIUS {
        for dx in -EROSION_RADIUS..=EROSION_RADIUS {
            let (x, z) = (cx + dx, cz + dz);
            let d = ((dx * dx + dz * dz) as f32).sqrt();
            if d < EROSION_RADIUS as f32 && x >= 0 && z >= 0 && (x as usize) < VERTS && (z as usize) < VERTS {
                weights.push((z as usize * VERTS + x as usize, EROSION_RADIUS as f32 - d));
            }
        }
    }
    let total: f32 = weights.iter().map(|w| w.1).sum();
    for (idx, w) in weights {
        heights[idx] -= amount * w / total;
    }
}

// Two-pass chamfer distance transform that also carries each cell's nearest river level.
fn distance_to_rivers(is_river: &[bool], level: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let mut dist = vec![f32::MAX; COUNT];
    let mut carried = vec![0.0; COUNT];
    for idx in 0..COUNT {
        if is_river[idx] {
            dist[idx] = 0.0;
            carried[idx] = level[idx];
        }
    }
    let step = |dx: isize, dz: isize| if dx != 0 && dz != 0 { CELL * 1.414 } else { CELL };
    let relax = |dist: &mut Vec<f32>, carried: &mut Vec<f32>, idx: usize, nb: usize, dx: isize, dz: isize| {
        let candidate = dist[nb] + step(dx, dz);
        if candidate < dist[idx] {
            dist[idx] = candidate;
            carried[idx] = carried[nb];
        }
    };
    for iz in 0..VERTS {
        for ix in 0..VERTS {
            let idx = iz * VERTS + ix;
            for (dx, dz) in [(-1isize, -1isize), (0, -1), (1, -1), (-1, 0)] {
                let (x, z) = (ix as isize + dx, iz as isize + dz);
                if (0..VERTS as isize).contains(&x) && (0..VERTS as isize).contains(&z) {
                    relax(&mut dist, &mut carried, idx, z as usize * VERTS + x as usize, dx, dz);
                }
            }
        }
    }
    for iz in (0..VERTS).rev() {
        for ix in (0..VERTS).rev() {
            let idx = iz * VERTS + ix;
            for (dx, dz) in [(1isize, 1isize), (0, 1), (-1, 1), (1, 0)] {
                let (x, z) = (ix as isize + dx, iz as isize + dz);
                if (0..VERTS as isize).contains(&x) && (0..VERTS as isize).contains(&z) {
                    relax(&mut dist, &mut carried, idx, z as usize * VERTS + x as usize, dx, dz);
                }
            }
        }
    }
    (dist, carried)
}


pub fn settlement_radius(kind: PoiKind) -> f32 {
    match kind {
        PoiKind::Village => 55.0,
        PoiKind::Farm => 34.0,
        PoiKind::Mill => 10.0,
    }
}

fn place_pois(rng: &mut Rng, map: &TerrainMap) -> Vec<Poi> {
    // Biggest first, so the one town and the large villages get first pick of level ground
    // and the roads (which root at the first village) grow out from the town.
    // Mills need a specific stretch of riverbank, which the coast now leaves short, so they
    // are placed before anything else can take it.
    let mut plan: Vec<(PoiKind, f32)> = (0..3).map(|_| (PoiKind::Mill, settlement_radius(PoiKind::Mill))).collect();
    plan.push((PoiKind::Village, 230.0));
    plan.extend((0..4).map(|_| (PoiKind::Village, rng.range(90.0, 120.0))));
    plan.extend((0..12).map(|_| (PoiKind::Village, rng.range(58.0, 80.0))));
    plan.extend((0..15).map(|_| (PoiKind::Village, rng.range(28.0, 42.0))));
    plan.extend((0..60).map(|_| (PoiKind::Farm, settlement_radius(PoiKind::Farm))));

    let mut pois: Vec<Poi> = Vec::new();
    for (kind, wanted) in plan {
        // If there's no room for the full size (the town especially), settle for smaller.
        let mut radius = wanted;
        let position = loop {
            if let Some(p) = find_site(rng, map, &pois, kind, radius) {
                break Some(p);
            }
            radius *= 0.85;
            if kind != PoiKind::Village || radius < wanted * 0.5 || radius < 28.0 {
                break None;
            }
        };
        let Some(position) = position else { continue };
        let landmark = if kind == PoiKind::Village && radius >= SMALL_SETTLEMENT_RADIUS {
            highest_point(map, position, VILLAGE_CHURCH_SEARCH.max(radius * 0.4))
        } else {
            position
        };
        pois.push(Poi { kind, position, landmark, radius });
    }
    pois
}

// The village church stands on the highest ground near the village centre.
fn highest_point(map: &TerrainMap, centre: Vec2, radius: f32) -> Vec2 {
    let n = map.grid_size();
    let mut best = (f32::MIN, centre);
    for iz in 0..n {
        for ix in 0..n {
            let p = grid_pos(ix, iz);
            if p.distance(centre) <= radius {
                let h = map.vertex_height(ix, iz);
                if h > best.0 {
                    best = (h, p);
                }
            }
        }
    }
    best.1
}

fn find_site(rng: &mut Rng, map: &TerrainMap, existing: &[Poi], kind: PoiKind, radius: f32) -> Option<Vec2> {
    for _ in 0..8000 {
        let p = Vec2::new(
            rng.range(-HALF_SIZE + 30.0, HALF_SIZE - 30.0),
            rng.range(-HALF_SIZE + 30.0, HALF_SIZE - 30.0),
        );
        let river_dist = map.river_distance(p);
        let h = map.height_at(p);
        let river_ok = match kind {
            // Mills stand on a shore (a river bank, or the edge of a lake or the sea), kept
            // within reach of the river so they still sit in a settled part of the map.
            PoiKind::Mill => river_dist > CELL && river_dist < SETTLEMENT_WATER_REACH,
            _ if radius < SMALL_SETTLEMENT_RADIUS => river_dist > 25.0,
            _ => river_dist > 25.0 && river_dist < SETTLEMENT_WATER_REACH,
        };
        // The whole flattened footprint must stay clear of water, not just the centre
        // point - otherwise a village or farm can end up overlapping a lake.
        let water_ok = match kind {
            PoiKind::Mill => (10.0..32.0).contains(&map.water_distance(p)),
            _ if radius >= LARGE_SETTLEMENT_RADIUS => map.water_distance(p) > 40.0,
            _ => map.water_distance(p) > radius + 20.0,
        };
        let height_ok = match kind {
            PoiKind::Mill => h > 0.0 && h < 90.0,
            _ => h > 0.0 && h < 90.0,
        };
        let spacing_ok = existing
            .iter()
            .all(|other| other.position.distance(p) >= POI_MIN_SPACING.max(radius + other.radius + 80.0));
        let level_ok = radius < LARGE_SETTLEMENT_RADIUS || {
            let limit = radius * 0.1;
            (0..8).all(|i| {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                (map.height_at(p + Vec2::new(a.cos(), a.sin()) * radius * 0.8) - h).abs() < limit
            })
        };
        if river_ok && water_ok && height_ok && spacing_ok && level_ok && p.length() > 120.0 {
            return Some(p);
        }
    }
    None
}

fn value_noise(x: f32, z: f32, seed: u64) -> f32 {
    let (x0, z0) = (x.floor(), z.floor());
    let (fx, fz) = (x - x0, z - z0);
    let (sx, sz) = (smooth(fx), smooth(fz));
    let (ix, iz) = (x0 as i32, z0 as i32);
    let a = lattice(ix, iz, seed);
    let b = lattice(ix + 1, iz, seed);
    let c = lattice(ix, iz + 1, seed);
    let d = lattice(ix + 1, iz + 1, seed);
    let top = a + (b - a) * sx;
    let bottom = c + (d - c) * sx;
    top + (bottom - top) * sz
}

fn lattice(ix: i32, iz: i32, seed: u64) -> f32 {
    let mut h = seed
        ^ (ix as u32 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (iz as u32 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    (h >> 40) as f32 / (1u64 << 24) as f32
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    smooth(((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0))
}

struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        let unit = (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32;
        lo + (hi - lo) * unit
    }
}
