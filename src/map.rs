
use bevy::prelude::*;

pub const MAP_SIZE: f32 = 5000.0;
pub const HALF_SIZE: f32 = MAP_SIZE / 2.0;
pub const CELL: f32 = 10.0;
pub const CELLS: usize = (MAP_SIZE / CELL) as usize;
const VERTS: usize = CELLS + 1;
const COUNT: usize = VERTS * VERTS;

const RIVER_MIN_SPAN: f32 = 3500.0;
const VALLEY_DEPTH: f32 = 22.0;
const VALLEY_FLAT: f32 = 30.0;
const VALLEY_WIDTH: f32 = 150.0;
const WATER_SURFACE_BELOW_FILL: f32 = 0.5;
const RIVER_BED_BELOW_WATER: f32 = 1.2;
const CARVE_CORE: f32 = CELL * 0.5;
const CARVE_BANK: f32 = CELL * 4.0;
const POI_MIN_SPACING: f32 = 400.0;
const SETTLEMENT_WATER_REACH: f32 = 350.0;

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
    river_len: f32,
    pub hills: Vec<Hill>,
    pub pois: Vec<Poi>,
}

impl TerrainMap {
    pub fn generate(seed: u64) -> Self {
        let mut rng = Rng(seed);
        let hills = make_hills(&mut rng);

        let mut base: Vec<f32> = (0..COUNT)
            .map(|idx| {
                let (ix, iz) = (idx % VERTS, idx / VERTS);
                raw_height(grid_pos(ix, iz), seed, &hills)
            })
            .collect();
        let path = river_path(&mut Rng(seed ^ 0x51DE_0001));
        carve_valley(&mut base, &path);
        for h in &mut base {
            *h /= EROSION_HEIGHT_UNIT;
        }
        erode(&mut base, &mut Rng(seed ^ 0xE20D_E000));
        for h in &mut base {
            *h *= EROSION_HEIGHT_UNIT;
        }

        let mut is_river = vec![false; COUNT];
        let mut level: Vec<f32> = base.iter().map(|h| h - WATER_SURFACE_BELOW_FILL).collect();
        let mut surface = f32::MAX;
        for &i in &path {
            is_river[i] = true;
            surface = surface.min(base[i] - WATER_SURFACE_BELOW_FILL);
            level[i] = surface;
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

        let water = (0..COUNT)
            .map(|idx| {
                if is_river[idx] {
                    Some(level[idx])
                } else if heights[idx] < 0.0 {
                    Some(0.0)
                } else {
                    None
                }
            })
            .collect();

        let mut map = TerrainMap {
            seed,
            heights,
            water,
            river_distance,
            river_len,
            hills,
            pois: Vec::new(),
        };
        map.pois = place_pois(&mut rng, &map);
        map.flatten_settlements();
        map
    }

    fn flatten_settlements(&mut self) {
        for poi in self.pois.clone() {
            let ground = self.height_at(poi.position);
            let radius = settlement_radius(poi.kind);
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
            river_len: 0.0,
            hills: Vec::new(),
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

fn raw_height(p: Vec2, seed: u64, hills: &[Hill]) -> f32 {
    let mut h = fbm(p.x / 800.0, p.y / 800.0, seed, 5) * 60.0 + 4.0;
    for hill in hills {
        let d = p.distance(hill.center) / hill.radius;
        if d < 1.0 {
            h += hill.height * (1.0 - d * d).powi(2);
        }
    }
    h
}

// The main river is a sinuous curve between two points on different edges. Its
// amplitude is chosen so the channel's length is close to pi times the straight-line
// distance, giving a strongly meandering river. The valley is then cut along it.
fn river_path(rng: &mut Rng) -> Vec<usize> {
    let max = (VERTS - 1) as f32;
    let edge_point = |side: usize, along: f32| match side {
        0 => Vec2::new(along * max, 0.0),
        1 => Vec2::new(along * max, max),
        2 => Vec2::new(0.0, along * max),
        _ => Vec2::new(max, along * max),
    };
    let (a, b) = loop {
        let side = (rng.range(0.0, 4.0) as usize).min(3);
        let target_side = (side + 1 + (rng.range(0.0, 3.0) as usize).min(2)) % 4;
        let a = edge_point(side, rng.range(0.2, 0.8));
        let b = edge_point(target_side, rng.range(0.2, 0.8));
        if a.distance(b) * CELL >= RIVER_MIN_SPAN {
            break (a, b);
        }
    };

    let chord = a.distance(b);
    let dir = (b - a) / chord;
    let normal = Vec2::new(-dir.y, dir.x);
    let cycles = 2.0 + (rng.range(0.0, 3.0)).floor();
    let point = |t: f32, amp: f32| {
        let taper = (std::f32::consts::PI * t).sin();
        let wiggle = amp * taper * (std::f32::consts::TAU * cycles * t).sin();
        a + dir * (t * chord) + normal * wiggle
    };
    let length_of = |amp: f32| {
        let steps = 2000;
        (1..=steps)
            .map(|i| point((i - 1) as f32 / steps as f32, amp).distance(point(i as f32 / steps as f32, amp)))
            .sum::<f32>()
    };
    let target = std::f32::consts::PI * chord;
    let (mut lo, mut hi) = (0.0, chord * 0.5);
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        if length_of(mid) < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let amp = (lo + hi) / 2.0;

    let samples = (chord * 8.0) as usize;
    let mut path: Vec<usize> = Vec::new();
    for i in 0..=samples {
        let p = point(i as f32 / samples as f32, amp).clamp(Vec2::ZERO, Vec2::splat(max));
        let idx = (p.y.round() as usize) * VERTS + p.x.round() as usize;
        if path.last() != Some(&idx) {
            path.push(idx);
        }
    }
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


const DROPLETS: usize = 300_000;
const DROPLET_LIFETIME: usize = 80;
const DROPLET_INERTIA: f32 = 0.1;
const DROPLET_CAPACITY: f32 = 1.5;
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
fn erode(heights: &mut [f32], rng: &mut Rng) {
    let max = (VERTS - 1) as f32;
    for _ in 0..DROPLETS {
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
                let amount = ((capacity - sediment) * DROPLET_ERODE).min(-delta);
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

fn make_hills(rng: &mut Rng) -> Vec<Hill> {
    let mut hills = Vec::new();
    let main_angle = rng.range(0.0, std::f32::consts::TAU);
    let main_dist = rng.range(900.0, 1500.0);
    hills.push(Hill {
        center: Vec2::from_angle(main_angle) * main_dist,
        radius: rng.range(600.0, 800.0),
        height: rng.range(150.0, 200.0),
    });
    for _ in 0..2 {
        let angle = rng.range(0.0, std::f32::consts::TAU);
        let dist = rng.range(500.0, 1800.0);
        hills.push(Hill {
            center: Vec2::from_angle(angle) * dist,
            radius: rng.range(300.0, 450.0),
            height: rng.range(60.0, 90.0),
        });
    }
    hills
}

pub fn settlement_radius(kind: PoiKind) -> f32 {
    match kind {
        PoiKind::Village => 30.0,
        PoiKind::Church => 12.0,
        PoiKind::Farm => 22.0,
        PoiKind::Mill => 10.0,
    }
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
                Some(v) => v.position + Vec2::new(0.0, 26.0),
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
    for _ in 0..8000 {
        let p = Vec2::new(
            rng.range(-HALF_SIZE + 30.0, HALF_SIZE - 30.0),
            rng.range(-HALF_SIZE + 30.0, HALF_SIZE - 30.0),
        );
        let river_dist = map.river_distance(p);
        let h = map.height_at(p);
        let river_ok = match kind {
            PoiKind::Mill => river_dist > CELL && river_dist < 40.0,
            _ => river_dist > 40.0 && river_dist < SETTLEMENT_WATER_REACH,
        };
        let height_ok = match kind {
            PoiKind::Mill => h > 0.0 && h < 45.0,
            _ => h > 3.0 && h < 65.0,
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
