//! Puts the settlements in the world: every building of every settlement as one mesh with its
//! collision, and its insides, which are only built while the player is near.

use std::sync::Arc;

use bevy::prelude::*;

use crate::building::geom::{Layer, Model};
use crate::building::{self, Site};
use crate::collision::Colliders;
use crate::loot::{LootAssets, LootBox, Rarity};
use crate::map::TerrainMap;
use crate::settlement_plan::{footprint_points, Building, Rng, SettlementPlan};

/// Everything the settlements spawn carries this, so that a regenerated map can clear it.
#[derive(Component)]
pub struct SettlementRoot;

/// How near the player has to be for a building's insides to be there, and how far they must go
/// before they are taken away again.
const INTERIOR_SHOW: f32 = 55.0;
const INTERIOR_HIDE: f32 = 80.0;
/// Lamps are lit in at most this many of the nearest buildings, none further than `LAMP_RANGE`.
const LAMPED_BUILDINGS: usize = 12;
const LAMP_RANGE: f32 = 32.0;
/// How bright a lamp is, in lumens. The camera's exposure is set for full daylight.
const LAMP_LUMENS: f32 = 450_000.0;
/// How far above the highest ground under a building its floor is.
const PLINTH: f32 = 0.12;

struct Entry {
    origin: Vec3,
    yaw: f32,
    model: Arc<Model>,
    interior: Option<Entity>,
    /// The lamps lit inside, while the player is near.
    lamps: Vec<Entity>,
}

/// Every building's model, for building and removing the insides as the player comes and goes.
#[derive(Resource)]
pub struct Buildings {
    entries: Vec<Entry>,
    material: Handle<StandardMaterial>,
    since_look: f32,
}

/// Window glass: see-through, a little shiny.
fn glass_material(materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial { base_color: Color::WHITE, alpha_mode: AlphaMode::Blend, perceptual_roughness: 0.08, reflectance: 0.6, ..default() })
}

pub struct SettlementPlugin;

impl Plugin for SettlementPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, stream_interiors);
    }
}

/// How the ground lies under a building, and so where its floor goes.
pub struct Ground {
    pub floor: f32,
    pub site: Site,
}

pub fn ground_under(map: &TerrainMap, b: &Building) -> Ground {
    let heights: Vec<f32> = footprint_points(&b.shape()).iter().map(|&p| map.height_at(p)).collect();
    let (high, low) = (heights.iter().cloned().fold(f32::MIN, f32::max), heights.iter().cloned().fold(f32::MAX, f32::min));
    let floor = high + PLINTH;
    let outside = b.centre + b.front * (b.depth * 0.5 + 0.8);
    Ground { floor, site: Site { skirt: floor - low + 0.3, door_rise: (floor - map.height_at(outside)).max(0.0) } }
}

pub fn spawn_settlements(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    map: &TerrainMap,
    plan: &SettlementPlan,
    colliders: &mut Colliders,
) {
    // Every part is coloured in its own vertices, so one plain material serves the lot.
    let material = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.9, ..default() });
    let glass = glass_material(materials);
    let mut entries = Vec::new();
    let all = map
        .pois
        .iter()
        .zip(&plan.layouts)
        .flat_map(|(poi, layout)| layout.buildings.iter().enumerate().map(move |(i, b)| (b, Vec2::splat(i as f32 * 0.37) + poi.position * 0.001)))
        .chain(plan.sheds.iter().map(|b| (b, Vec2::ZERO)));
    // There is only one unique thing in the whole map: the second and later rolls become legendary.
    let mut unique_found = false;
    for (b, jitter) in all {
        let ground = ground_under(map, b);
        let origin = Vec3::new(b.centre.x, ground.floor, b.centre.y);
        let mut rng = Rng::from_position(b.centre + jitter);
        let mut model = building::build(b, &ground.site, &mut rng);
        for (_, rarity) in &mut model.loot {
            if *rarity == Rarity::Unique {
                if unique_found {
                    *rarity = Rarity::Legendary;
                }
                unique_found = true;
            }
        }
        for solid in model.solids(origin, b.yaw) {
            colliders.add(solid);
        }
        let at = Transform::from_translation(origin).with_rotation(Quat::from_rotation_y(b.yaw));
        commands.spawn((SettlementRoot, Mesh3d(meshes.add(model.bake(Layer::Shell))), MeshMaterial3d(material.clone()), at));
        if model.has(Layer::Glass) {
            commands.spawn((SettlementRoot, Mesh3d(meshes.add(model.bake(Layer::Glass))), MeshMaterial3d(glass.clone()), at, bevy::light::NotShadowCaster));
        }
        entries.push(Entry { origin, yaw: b.yaw, model: Arc::new(model), interior: None, lamps: Vec::new() });
    }
    commands.insert_resource(Buildings { entries, material, since_look: 1.0 });
}

/// Builds the insides of the buildings the player is near, and takes them away from those they have left.
fn stream_interiors(
    mut commands: Commands,
    time: Res<Time>,
    buildings: Option<ResMut<Buildings>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut meshes: ResMut<Assets<Mesh>>,
    loot: Option<Res<LootAssets>>,
) {
    let (Some(mut buildings), Ok(eye)) = (buildings, camera.single()) else { return };
    buildings.since_look += time.delta_secs();
    if buildings.since_look < 0.25 {
        return;
    }
    buildings.since_look = 0.0;
    let here = eye.translation();
    let material = buildings.material.clone();
    // Which buildings have their lamps lit: the nearest few.
    let mut near: Vec<(f32, usize)> = buildings
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.model.lights.is_empty())
        .map(|(i, e)| (Vec2::new(e.origin.x - here.x, e.origin.z - here.z).length(), i))
        .filter(|(d, _)| *d < LAMP_RANGE)
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    let lit: std::collections::HashSet<usize> = near.iter().take(LAMPED_BUILDINGS).map(|&(_, i)| i).collect();
    for (i, entry) in buildings.entries.iter_mut().enumerate() {
        if lit.contains(&i) && entry.lamps.is_empty() {
            let turn = Quat::from_rotation_y(entry.yaw);
            for &(at, range) in &entry.model.lights {
                let id = commands
                    .spawn((
                        SettlementRoot,
                        PointLight { intensity: LAMP_LUMENS, range, radius: 0.25, color: Color::srgb(1.0, 0.88, 0.7), shadow_maps_enabled: false, ..default() },
                        Transform::from_translation(entry.origin + turn * at),
                    ))
                    .id();
                entry.lamps.push(id);
            }
        } else if !lit.contains(&i) && !entry.lamps.is_empty() {
            for id in entry.lamps.drain(..) {
                commands.entity(id).despawn();
            }
        }
    }
    for entry in &mut buildings.entries {
        let distance = Vec2::new(entry.origin.x - here.x, entry.origin.z - here.z).length();
        match entry.interior {
            None if distance < INTERIOR_SHOW && entry.model.has(Layer::Interior) => {
                let id = commands
                    .spawn((
                        SettlementRoot,
                        Mesh3d(meshes.add(entry.model.bake(Layer::Interior))),
                        MeshMaterial3d(material.clone()),
                        Transform::from_translation(entry.origin).with_rotation(Quat::from_rotation_y(entry.yaw)),
                    ))
                    .id();
                // The loot lying about inside, in the building's own frame.
                if let Some(loot) = &loot {
                    commands.entity(id).with_children(|parent| {
                        for &(at, rarity) in &entry.model.loot {
                            let size = rarity.size();
                            parent.spawn((
                                LootBox { rarity },
                                Mesh3d(loot.mesh.clone()),
                                MeshMaterial3d(loot.material(rarity)),
                                Transform::from_translation(at + Vec3::Y * (size * 0.5 + 0.03)).with_scale(Vec3::splat(size)),
                            ));
                        }
                    });
                }
                entry.interior = Some(id);
            }
            Some(id) if distance > INTERIOR_HIDE => {
                commands.entity(id).despawn();
                entry.interior = None;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A diagnostic, not an assertion: builds every settlement on a real map and measures how well the
    /// buildings sit among the roads, each other, water and slopes.
    ///   cargo test --lib how_well_do_settlements -- --ignored --nocapture
    #[test]
    #[ignore]
    fn how_well_do_settlements_fit_their_surroundings() {
        use crate::collision::Shape;
        use crate::params::GenParams;
        use crate::roads::{road_ribbons, RoadClearance, RoadNetwork};
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let roads = RoadNetwork::generate(&map, &params);
        let plan = SettlementPlan::generate(&map, &roads);
        let mut streets = road_ribbons(&map, &roads);
        streets.extend(plan.lanes());
        let clearance = RoadClearance::new(&streets);
        #[derive(Default, Debug)]
        struct Tally {
            settlements: usize,
            buildings: usize,
            on_road: usize,
            by_road: usize,
            overlapping: usize,
            on_water: usize,
            on_slope: usize,
        }
        let mut tiers: std::collections::BTreeMap<&str, Tally> = Default::default();
        for (poi, layout) in map.pois.iter().zip(&plan.layouts) {
            let shapes: Vec<Shape> = layout.buildings.iter().map(|b| b.shape()).collect();
            let tier = match (poi.kind, poi.radius) {
                (crate::map::PoiKind::Mill, _) => "mill",
                (crate::map::PoiKind::Farm, _) => "farm",
                (_, r) if r >= 150.0 => "town",
                (_, r) if r >= 90.0 => "large village",
                (_, r) if r >= 45.0 => "village",
                _ => "hamlet",
            };
            let tally = tiers.entry(tier).or_default();
            tally.settlements += 1;
            for (i, shape) in shapes.iter().enumerate() {
                tally.buildings += 1;
                let pts = footprint_points(shape);
                let worst = pts.iter().map(|&p| clearance.clearance(p)).fold(f32::MAX, f32::min);
                if worst < 0.0 {
                    tally.on_road += 1;
                } else if worst < 3.0 {
                    tally.by_road += 1;
                }
                if shapes.iter().enumerate().any(|(j, other)| j != i && pts.iter().any(|&p| other.separation(p).0 < -0.01)) {
                    tally.overlapping += 1;
                }
                if pts.iter().any(|&p| map.water_distance(p) < 3.0 || map.water_surface_at(p).is_some()) {
                    tally.on_water += 1;
                }
                let heights: Vec<f32> = pts.iter().map(|&p| map.height_at(p)).collect();
                let range = heights.iter().cloned().fold(f32::MIN, f32::max) - heights.iter().cloned().fold(f32::MAX, f32::min);
                if range > 1.5 {
                    tally.on_slope += 1;
                }
            }
        }
        println!("{:14} {:>5} {:>9} {:>8} {:>9} {:>11} {:>8} {:>8}", "tier", "count", "buildings", "on road", "by road", "overlapping", "in water", ">1.5m slope");
        for (name, t) in &tiers {
            let pct = |n: usize| if t.buildings == 0 { 0.0 } else { 100.0 * n as f32 / t.buildings as f32 };
            println!("{name:14} {:>5} {:>9} {:>7.0}% {:>8.0}% {:>10.0}% {:>7.0}% {:>10.0}%", t.settlements, t.buildings, pct(t.on_road), pct(t.by_road), pct(t.overlapping), pct(t.on_water), pct(t.on_slope));
        }
    }
}
