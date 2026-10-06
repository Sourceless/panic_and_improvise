use std::cmp::Reverse;
use std::collections::BinaryHeap;

use bevy::prelude::*;

pub const MAP_SIZE: f32 = 1000.0;
pub const HALF_SIZE: f32 = MAP_SIZE / 2.0;
pub const CELL: f32 = 4.0;
pub const CELLS: usize = (MAP_SIZE / CELL) as usize;
const VERTS: usize = CELLS + 1;
const COUNT: usize = VERTS * VERTS;
const NONE: u32 = u32::MAX;

const FILL_EPSILON: f32 = 0.001;
const RIVER_FRACTION: f32 = 0.03;
const RIVER_MIN_ACCUMULATION: u32 = 25;
const WATER_SURFACE_BELOW_FILL: f32 = 0.5;
const RIVER_BED_BELOW_WATER: f32 = 1.2;
const CARVE_CORE: f32 = CELL * 0.5;
const CARVE_BANK: f32 = CELL * 4.0;
const SPAWN_CLEARING: f32 = 60.0;
const POI_MIN_SPACING: f32 = 150.0;

#[derive(Clone, Copy, Debug)]
pub struct Hill {
    pub center: Vec2,
    pub radius: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoiKind {
    Village,
    Church,
    Farm,
    Mill,
}

#[derive(Clone, Copy, Debug)]
pub struct Poi {
    pub kind: PoiKind,
    pub position: Vec2,
}

#[derive(Resource)]
pub struct TerrainMap {
    pub seed: u64,
    heights: Vec<f32>,
    water: Vec<Option<f32>>,
    river_distance: Vec<f32>,
    river_cells: usize,
    pub hills: Vec<Hill>,
    pub pois: Vec<Poi>,
}

impl TerrainMap {
    pub fn generate(seed: u64) -> Self {
        let mut rng = Rng(seed);
        let hills = make_hills(&mut rng);

        let base: Vec<f32> = (0..COUNT)
            .map(|idx| {
                let (ix, iz) = (idx % VERTS, idx / VERTS);
                base_height(grid_pos(ix, iz), seed, &hills)
            })
            .collect();

        let (filled, order, receiver) = priority_flood(&base);
        let accumulation = flow_accumulation(&order, &receiver);
        let is_river = river_mask(&accumulation);
        let river_cells = is_river.iter().filter(|&&r| r).count();

        let level: Vec<f32> = filled.iter().map(|f| f - WATER_SURFACE_BELOW_FILL).collect();
        let (river_distance, river_level) = distance_to_rivers(&is_river, &level);

        let heights: Vec<f32> = (0..COUNT)
            .map(|idx| {
                let blend = 1.0 - smoothstep(CARVE_CORE, CARVE_BANK, river_distance[idx]);
                let bed = base[idx].min(river_level[idx] - RIVER_BED_BELOW_WATER);
                base[idx] + (bed - base[idx]) * blend
            })
            .collect();

        let water = (0..COUNT)
            .map(|idx| is_river[idx].then_some(level[idx]))
            .collect();

        let mut map = TerrainMap {
            seed,
            heights,
            water,
            river_distance,
            river_cells,
            hills,
            pois: Vec::new(),
        };
        map.pois = place_pois(&mut rng, &map);
        map
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

    pub fn grid_size(&self) -> usize {
        VERTS
    }

    pub fn river_length(&self) -> f32 {
        self.river_cells as f32 * CELL
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

fn base_height(p: Vec2, seed: u64, hills: &[Hill]) -> f32 {
    let lowland = fbm(p.x / 160.0, p.y / 160.0, seed, 4) * 14.0 + 6.0;
    let mut h = lowland;
    for hill in hills {
        let d = p.distance(hill.center) / hill.radius;
        if d < 1.0 {
            h += hill.height * (1.0 - d * d).powi(2);
        }
    }
    h * smoothstep(SPAWN_CLEARING * 0.3, SPAWN_CLEARING, p.length())
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

fn is_boundary(idx: usize) -> bool {
    let (ix, iz) = (idx % VERTS, idx / VERTS);
    ix == 0 || iz == 0 || ix == VERTS - 1 || iz == VERTS - 1
}

// Priority-Flood (Barnes et al. 2014): floods inward from the map edge, which drains
// every basin. Returns the filled surface, the pop order and each cell's receiver.
fn priority_flood(base: &[f32]) -> (Vec<f32>, Vec<u32>, Vec<u32>) {
    let mut filled = base.to_vec();
    let mut receiver = vec![NONE; COUNT];
    let mut visited = vec![false; COUNT];
    let mut order = Vec::with_capacity(COUNT);
    let mut heap = BinaryHeap::new();

    for idx in (0..COUNT).filter(|&i| is_boundary(i)) {
        visited[idx] = true;
        heap.push(Reverse((base[idx].to_bits(), idx as u32)));
    }

    while let Some(Reverse((_, current))) = heap.pop() {
        let c = current as usize;
        order.push(current);
        for n in neighbours(c) {
            if visited[n] {
                continue;
            }
            visited[n] = true;
            receiver[n] = current;
            filled[n] = base[n].max(filled[c] + FILL_EPSILON);
            heap.push(Reverse((filled[n].to_bits(), n as u32)));
        }
    }
    (filled, order, receiver)
}

// D8 flow accumulation: each cell passes its catchment to its receiver, visiting
// cells from highest to lowest so every cell is complete before it drains.
fn flow_accumulation(order: &[u32], receiver: &[u32]) -> Vec<u32> {
    let mut acc = vec![1u32; COUNT];
    for &n in order.iter().rev() {
        let r = receiver[n as usize];
        if r != NONE {
            acc[r as usize] += acc[n as usize];
        }
    }
    acc
}

fn river_mask(accumulation: &[u32]) -> Vec<bool> {
    let outlet_max = (0..COUNT)
        .filter(|&i| is_boundary(i))
        .map(|i| accumulation[i])
        .max()
        .unwrap_or(0);
    let threshold = ((outlet_max as f32 * RIVER_FRACTION) as u32).max(RIVER_MIN_ACCUMULATION);
    accumulation.iter().map(|&a| a >= threshold).collect()
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

fn make_hills(rng: &mut Rng) -> Vec<Hill> {
    let mut hills = Vec::new();
    let main_angle = rng.range(0.0, std::f32::consts::TAU);
    let main_dist = rng.range(220.0, 340.0);
    hills.push(Hill {
        center: Vec2::from_angle(main_angle) * main_dist,
        radius: rng.range(180.0, 230.0),
        height: rng.range(60.0, 80.0),
    });
    for _ in 0..2 {
        let angle = rng.range(0.0, std::f32::consts::TAU);
        let dist = rng.range(150.0, 420.0);
        hills.push(Hill {
            center: Vec2::from_angle(angle) * dist,
            radius: rng.range(90.0, 130.0),
            height: rng.range(20.0, 35.0),
        });
    }
    hills
}

fn place_pois(rng: &mut Rng, map: &TerrainMap) -> Vec<Poi> {
    let plan = [
        PoiKind::Village,
        PoiKind::Church,
        PoiKind::Village,
        PoiKind::Farm,
        PoiKind::Farm,
        PoiKind::Farm,
        PoiKind::Mill,
    ];
    let mut pois: Vec<Poi> = Vec::new();
    for kind in plan {
        let position = if kind == PoiKind::Church {
            let village = pois.iter().rev().find(|p| p.kind == PoiKind::Village).copied();
            match village {
                Some(v) => v.position + Vec2::new(12.0, 0.0),
                None => continue,
            }
        } else {
            match find_site(rng, map, &pois, kind) {
                Some(p) => p,
                None => continue,
            }
        };
        pois.push(Poi { kind, position });
    }
    pois
}

fn find_site(rng: &mut Rng, map: &TerrainMap, existing: &[Poi], kind: PoiKind) -> Option<Vec2> {
    for _ in 0..2000 {
        let p = Vec2::new(
            rng.range(-HALF_SIZE + 30.0, HALF_SIZE - 30.0),
            rng.range(-HALF_SIZE + 30.0, HALF_SIZE - 30.0),
        );
        let river_dist = map.river_distance(p);
        let h = map.height_at(p);
        let river_ok = match kind {
            PoiKind::Mill => river_dist > CELL && river_dist < 12.0,
            _ => river_dist > 40.0,
        };
        let height_ok = match kind {
            PoiKind::Mill => h < 15.0,
            _ => h > 3.0 && h < 40.0,
        };
        let spacing_ok = existing
            .iter()
            .all(|other| other.position.distance(p) >= POI_MIN_SPACING);
        if river_ok && height_ok && spacing_ok && p.length() > 120.0 {
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
