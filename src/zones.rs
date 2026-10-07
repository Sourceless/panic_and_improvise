use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::map::{fbm, grid_pos, PoiKind, TerrainMap, CELL};
use crate::params::GenParams;

const OVERLAY_LIFT: f32 = 0.6;
const OVERLAY_ALPHA: f32 = 0.55;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Zone {
    Water,
    Urban,
    Industrial,
    Military,
    Orchard,
    Arable,
    Pasture,
    Woodland,
    Conifer,
    Wetland,
    Moorland,
    Quarry,
    Open,
}

impl Zone {
    pub fn color(self) -> [f32; 3] {
        match self {
            Zone::Water => [0.20, 0.42, 0.72],
            Zone::Urban => [0.80, 0.42, 0.38],
            Zone::Industrial => [0.36, 0.36, 0.40],
            Zone::Military => [0.40, 0.44, 0.24],
            Zone::Orchard => [0.62, 0.78, 0.30],
            Zone::Arable => [0.92, 0.80, 0.30],
            Zone::Pasture => [0.56, 0.84, 0.44],
            Zone::Woodland => [0.16, 0.42, 0.18],
            Zone::Conifer => [0.10, 0.28, 0.24],
            Zone::Wetland => [0.22, 0.60, 0.56],
            Zone::Moorland => [0.56, 0.42, 0.56],
            Zone::Quarry => [0.52, 0.46, 0.38],
            Zone::Open => [0.74, 0.70, 0.46],
        }
    }
}

// Zones are tried in this order; the first one whose rule matches a cell wins.
const PRIORITY: [Zone; 11] = [
    Zone::Urban,
    Zone::Industrial,
    Zone::Military,
    Zone::Orchard,
    Zone::Wetland,
    Zone::Quarry,
    Zone::Conifer,
    Zone::Woodland,
    Zone::Arable,
    Zone::Moorland,
    Zone::Pasture,
];

struct Site {
    elevation: f32,
    slope: f32,
    river_distance: f32,
    village_distance: f32,
    farm_distance: f32,
}

// Each zone's rule: physical limits, plus a noise threshold so zones form patches.
struct Rule {
    zone: Zone,
    matches: Box<dyn Fn(&Site) -> bool>,
    patch_scale: f32,
    min_noise: f32,
}

fn rule_for(zone: Zone, params: &GenParams) -> Rule {
    let max_arable_slope = params.max_arable_slope;
    let max_pasture_slope = params.max_pasture_slope;
    let (matches, patch_scale, min_noise): (Box<dyn Fn(&Site) -> bool>, f32, f32) = match zone {
        Zone::Urban => (Box::new(|s| s.village_distance < 140.0 && s.slope < 0.15), 1.0, 0.0),
        Zone::Industrial => (
            Box::new(|s| (250.0..1200.0).contains(&s.village_distance) && s.slope < 0.06 && s.elevation < 110.0 && s.river_distance > 80.0),
            400.0,
            0.68,
        ),
        Zone::Military => (
            Box::new(|s| s.village_distance > 200.0 && s.slope < 0.10 && s.elevation < 150.0),
            900.0,
            0.74,
        ),
        Zone::Orchard => (
            Box::new(|s| s.farm_distance < 300.0 && s.elevation < 70.0 && s.slope < 0.12),
            200.0,
            0.50,
        ),
        Zone::Wetland => (
            Box::new(|s| s.river_distance < 120.0 && s.elevation < 25.0 && s.slope < 0.05),
            150.0,
            0.45,
        ),
        Zone::Quarry => (Box::new(|s| s.slope > 0.5 && s.elevation > 100.0), 120.0, 0.62),
        Zone::Conifer => (Box::new(|s| s.elevation > 140.0 && s.slope < 0.7), 250.0, 0.62),
        Zone::Woodland => (Box::new(|s| s.elevation < 170.0 && s.slope < 0.5), 300.0, 0.70),
        Zone::Arable => (
            Box::new(move |s| s.elevation < 130.0 && s.slope < max_arable_slope && s.river_distance > 30.0),
            350.0,
            0.38,
        ),
        Zone::Moorland => (Box::new(|s| s.elevation > 150.0 && s.slope < 0.5), 500.0, 0.60),
        Zone::Pasture => (Box::new(move |s| s.elevation < 230.0 && s.slope < max_pasture_slope), 1.0, 0.0),
        Zone::Water | Zone::Open => (Box::new(|_| false), 1.0, 0.0),
    };
    Rule {
        zone,
        matches,
        patch_scale,
        min_noise,
    }
}

#[derive(Resource)]
pub struct ZoneMap {
    zones: Vec<Zone>,
    verts: usize,
}

impl ZoneMap {
    pub fn generate(map: &TerrainMap, params: &GenParams) -> Self {
        let n = map.grid_size();
        let mut zones = Vec::with_capacity(n * n);
        // Each village's centre and how much bigger than the standard 55m village it is, so
        // distance-based rules (the Urban zone especially) grow with a large settlement.
        let villages: Vec<(Vec2, f32)> = map
            .pois
            .iter()
            .filter(|p| p.kind == PoiKind::Village)
            .map(|p| (p.position, p.radius - 55.0))
            .collect();
        let farms: Vec<Vec2> = map
            .pois
            .iter()
            .filter(|p| p.kind == PoiKind::Farm)
            .map(|p| p.position)
            .collect();
        let rules: Vec<Rule> = PRIORITY.iter().map(|&z| rule_for(z, params)).collect();
        let seed = map.seed;

        for iz in 0..n {
            for ix in 0..n {
                let p = grid_pos(ix, iz);
                if map.water_level(ix, iz).is_some() {
                    zones.push(Zone::Water);
                    continue;
                }
                let site = Site {
                    elevation: map.vertex_height(ix, iz),
                    slope: slope_at(map, ix, iz),
                    river_distance: map.river_distance(p),
                    village_distance: villages
                        .iter()
                        .map(|&(q, extra)| (q.distance(p) - extra).max(0.0))
                        .fold(f32::MAX, f32::min),
                    farm_distance: nearest(&farms, p),
                };
                let zone = rules
                    .iter()
                    .enumerate()
                    .find(|(i, rule)| {
                        (rule.matches)(&site) && {
                            let patch = fbm(
                                p.x / rule.patch_scale,
                                p.y / rule.patch_scale,
                                seed ^ (0x20E5 + *i as u64),
                                3,
                            );
                            patch >= rule.min_noise
                        }
                    })
                    .map(|(_, rule)| rule.zone)
                    .unwrap_or(Zone::Open);
                zones.push(zone);
            }
        }
        ZoneMap { zones, verts: n }
    }

    pub fn zone_at(&self, ix: usize, iz: usize) -> Zone {
        self.zones[iz * self.verts + ix]
    }

    pub fn count(&self, zone: Zone) -> usize {
        self.zones.iter().filter(|&&z| z == zone).count()
    }

    pub fn distinct_zones(&self) -> usize {
        let mut seen = std::collections::HashSet::new();
        for &z in &self.zones {
            seen.insert(z);
        }
        seen.len()
    }
}

fn slope_at(map: &TerrainMap, ix: usize, iz: usize) -> f32 {
    let n = map.grid_size();
    let dx = map.vertex_height((ix + 1).min(n - 1), iz) - map.vertex_height(ix.saturating_sub(1), iz);
    let dz = map.vertex_height(ix, (iz + 1).min(n - 1)) - map.vertex_height(ix, iz.saturating_sub(1));
    (dx * dx + dz * dz).sqrt() / (2.0 * CELL)
}

fn nearest(points: &[Vec2], p: Vec2) -> f32 {
    points
        .iter()
        .map(|q| q.distance(p))
        .fold(f32::MAX, f32::min)
}

// A translucent quad per cell, lifted just above the terrain, coloured by zone.
pub fn overlay_mesh(map: &TerrainMap, zones: &ZoneMap) -> Mesh {
    let n = map.grid_size();
    let half = CELL * 0.5;
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    for iz in 0..n {
        for ix in 0..n {
            let p = grid_pos(ix, iz);
            let y = map.vertex_height(ix, iz) + OVERLAY_LIFT;
            let c = zones.zone_at(ix, iz).color();
            let color = [c[0], c[1], c[2], OVERLAY_ALPHA];
            let base = positions.len() as u32;
            for (dx, dz) in [(-half, -half), (half, -half), (half, half), (-half, half)] {
                positions.push([p.x + dx, y, p.y + dz]);
                colors.push(color);
            }
            indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices))
}
