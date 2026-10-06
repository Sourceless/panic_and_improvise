use bevy::prelude::*;

pub const MAP_SIZE: f32 = 5000.0;
pub const HALF_SIZE: f32 = MAP_SIZE / 2.0;
pub const CELL: f32 = 10.0;
pub const CELLS: usize = (MAP_SIZE / CELL) as usize;
const VERTS: usize = CELLS + 1;

const RIVER_STEP: f32 = 50.0;
const RIVER_CORE: f32 = 6.0;
const RIVER_BANK: f32 = 30.0;
const RIVER_BED_DROP: f32 = 1.5;
const SPAWN_CLEARING: f32 = 200.0;
const POI_MIN_SPACING: f32 = 500.0;

#[derive(Clone, Copy, Debug)]
pub struct RiverPoint {
    pub pos: Vec2,
    pub level: f32,
}

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
    pub river: Vec<RiverPoint>,
    pub hills: Vec<Hill>,
    pub pois: Vec<Poi>,
}

impl TerrainMap {
    pub fn generate(seed: u64) -> Self {
        let mut rng = Rng(seed);
        let hills = make_hills(&mut rng);
        let river = make_river(&mut rng, &hills, seed);

        let mut heights = Vec::with_capacity(VERTS * VERTS);
        for iz in 0..VERTS {
            for ix in 0..VERTS {
                let p = grid_pos(ix, iz);
                let base = base_height(p, seed, &hills);
                heights.push(carve_river(p, base, &river));
            }
        }

        let mut map = TerrainMap {
            seed,
            heights,
            river,
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

    pub fn grid_size(&self) -> usize {
        VERTS
    }

    pub fn river_length(&self) -> f32 {
        self.river
            .windows(2)
            .map(|w| w[0].pos.distance(w[1].pos))
            .sum()
    }
}

pub fn grid_pos(ix: usize, iz: usize) -> Vec2 {
    Vec2::new(
        -HALF_SIZE + ix as f32 * CELL,
        -HALF_SIZE + iz as f32 * CELL,
    )
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
    let lowland = fbm(p.x / 700.0, p.y / 700.0, seed, 4) * 40.0 + 15.0;
    let mut h = lowland;
    for hill in hills {
        let d = p.distance(hill.center) / hill.radius;
        if d < 1.0 {
            h += hill.height * (1.0 - d * d).powi(2);
        }
    }
    let clearing = smoothstep(SPAWN_CLEARING * 0.3, SPAWN_CLEARING, p.length());
    h * clearing
}

fn carve_river(p: Vec2, base: f32, river: &[RiverPoint]) -> f32 {
    let (dist, level) = nearest_river(p, river);
    let blend = 1.0 - smoothstep(RIVER_CORE, RIVER_BANK, dist);
    base + (level - RIVER_BED_DROP - base) * blend
}

fn nearest_river(p: Vec2, river: &[RiverPoint]) -> (f32, f32) {
    let mut best = (f32::MAX, 0.0);
    for w in river.windows(2) {
        let (a, b) = (w[0], w[1]);
        let ab = b.pos - a.pos;
        let t = ((p - a.pos).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
        let d = p.distance(a.pos + ab * t);
        if d < best.0 {
            best = (d, a.level + (b.level - a.level) * t);
        }
    }
    best
}

fn make_hills(rng: &mut Rng) -> Vec<Hill> {
    let mut hills = Vec::new();
    let main_angle = rng.range(0.0, std::f32::consts::TAU);
    let main_dist = rng.range(900.0, 1500.0);
    hills.push(Hill {
        center: Vec2::from_angle(main_angle) * main_dist,
        radius: rng.range(420.0, 520.0),
        height: rng.range(90.0, 120.0),
    });
    for _ in 0..2 {
        let angle = rng.range(0.0, std::f32::consts::TAU);
        let dist = rng.range(600.0, 2000.0);
        hills.push(Hill {
            center: Vec2::from_angle(angle) * dist,
            radius: rng.range(200.0, 320.0),
            height: rng.range(25.0, 50.0),
        });
    }
    hills
}

fn make_river(rng: &mut Rng, hills: &[Hill], seed: u64) -> Vec<RiverPoint> {
    let z_start = rng.range(-1800.0, -1200.0);
    let z_end = rng.range(-1800.0, -1200.0);
    let phase_a = rng.range(0.0, std::f32::consts::TAU);
    let phase_b = rng.range(0.0, std::f32::consts::TAU);
    let steps = (MAP_SIZE / RIVER_STEP) as usize;
    let points: Vec<Vec2> = (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let x = -HALF_SIZE + t * MAP_SIZE;
            let z = z_start
                + (z_end - z_start) * t
                + 260.0 * (t * std::f32::consts::TAU * 1.5 + phase_a).sin()
                + 90.0 * (t * std::f32::consts::TAU * 5.0 + phase_b).sin();
            Vec2::new(x, z)
        })
        .collect();
    points
        .into_iter()
        .map(|pos| RiverPoint {
            pos,
            level: base_height(pos, seed, hills) - 1.0,
        })
        .collect()
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
                Some(v) => v.position + Vec2::new(30.0, 0.0),
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
            rng.range(-HALF_SIZE + 100.0, HALF_SIZE - 100.0),
            rng.range(-HALF_SIZE + 100.0, HALF_SIZE - 100.0),
        );
        let (river_dist, level) = nearest_river(p, &map.river);
        let h = map.height_at(p);
        let river_ok = match kind {
            PoiKind::Mill => (river_dist > RIVER_CORE + 4.0) && river_dist < 25.0,
            _ => river_dist > 120.0,
        };
        let height_ok = match kind {
            PoiKind::Mill => h > level,
            _ => h > 5.0 && h < 45.0,
        };
        let spacing_ok = existing
            .iter()
            .all(|other| other.position.distance(p) >= POI_MIN_SPACING);
        if river_ok && height_ok && spacing_ok && p.length() > 300.0 {
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
