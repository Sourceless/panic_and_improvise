// Builds everything that makes up the world from a generated terrain map: land-use zones,
// roads, the terrain mesh and water, settlements, fields, hedges and vegetation. The game and
// the map viewer both call this, so the viewer always previews exactly what the game builds.

use bevy::prelude::*;

use crate::collision::Colliders;
use crate::field_material::FieldMaterial;
use crate::fill::spawn_fill;
use crate::map::TerrainMap;
use crate::params::GenParams;
use crate::road_material::RoadMaterial;
use crate::roads::{spawn_roads, RoadNetwork};
use crate::settlement_plan::SettlementPlan;
use crate::sketch::Sketch;
use crate::terrain::{spawn_terrain, TerrainMaterial, TerrainTextures};
use crate::water_material::WaterMaterial;
use crate::zones::ZoneMap;

/// What a world is planned from, before any of it is built: the land use, the roads, and the settlements along them.
/// Making it takes a while and needs no renderer, so it can be done on another thread.
pub struct Plans {
    pub zones: ZoneMap,
    pub roads: RoadNetwork,
    pub plan: SettlementPlan,
}

/// Waits in the world, for the startup systems to find, when the plans were made ahead of them.
#[derive(Resource)]
pub struct PlansMade(pub Option<Plans>);

/// Where in the whole job of making a world the planning begins and ends, for the loading bar.
const PLANS_FROM: f32 = crate::map::MAP_JOB;
const PLANS_TO: f32 = 0.85;

/// Plans the world for `map`: its land use, its roads, and the villages along them. `sketch` is drawn on as it goes,
/// and says what is being done.
pub fn plan_world(map: &TerrainMap, params: &GenParams, sketch: &Sketch) -> Plans {
    let span = PLANS_TO - PLANS_FROM;
    sketch.say("Marking out fields and woods", PLANS_FROM);
    let zones = ZoneMap::generate(map, params);
    sketch.zones(&zones);
    sketch.say("Laying the roads", PLANS_FROM + span * 0.05);
    let mut roads = RoadNetwork::generate(map, params);
    sketch.roads(&crate::roads::road_ribbons(map, &roads));
    sketch.say("Planning the villages", PLANS_FROM + span * 0.9);
    // Each settlement is planned along the roads that reach it, and brings its own lanes.
    let mut plan = SettlementPlan::generate(map, &roads);
    plan.sheds = crate::fill::shed_buildings(map, &zones);
    roads.add_ribbons(plan.lanes());
    let ribbons = crate::roads::road_ribbons(map, &roads);
    sketch.roads(&ribbons);
    // Pavements beside the streets of the settlements, and a path from each door to them.
    roads.add_ribbons(crate::paving::lay(map, &plan, &ribbons));
    sketch.buildings(plan.layouts.iter().flat_map(|l| l.buildings.iter()).chain(plan.sheds.iter()));
    sketch.say("Building the world", PLANS_TO);
    Plans { zones, roads, plan }
}

/// Generates the zone map and road network for `map`, spawns all world entities, and inserts
/// the zone map and road network as resources.
pub fn build_world(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    standard: &mut Assets<StandardMaterial>,
    terrain: &mut Assets<TerrainMaterial>,
    fields: &mut Assets<FieldMaterial>,
    road_materials: &mut Assets<RoadMaterial>,
    waters: &mut Assets<WaterMaterial>,
    images: &mut Assets<Image>,
    textures: &TerrainTextures,
    map: &TerrainMap,
    params: &GenParams,
) {
    let plans = plan_world(map, params, &Sketch::default());
    build_world_from(commands, meshes, standard, terrain, fields, road_materials, waters, images, textures, map, params, plans);
}

/// As [`build_world`], for a world that has been planned already.
pub fn build_world_from(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    standard: &mut Assets<StandardMaterial>,
    terrain: &mut Assets<TerrainMaterial>,
    fields: &mut Assets<FieldMaterial>,
    road_materials: &mut Assets<RoadMaterial>,
    waters: &mut Assets<WaterMaterial>,
    images: &mut Assets<Image>,
    textures: &TerrainTextures,
    map: &TerrainMap,
    params: &GenParams,
    plans: Plans,
) {
    let Plans { zones, roads, plan } = plans;
    // Everything solid that the generators place is collected here for the player to bump into.
    let mut colliders = Colliders::default();
    spawn_terrain(commands, meshes, standard, terrain, textures, map, &zones, waters, images, &plan, &mut colliders);
    spawn_roads(commands, meshes, road_materials, textures, map, &roads);
    spawn_fill(commands, meshes, standard, fields, textures, map, &zones, &roads, params, &plan, &mut colliders);
    let ribbons = crate::roads::road_ribbons(map, &roads);
    map.set_roads(crate::roads::RoadClearance::new(&ribbons));
    commands.insert_resource(crate::footsteps::RoadSurfaces(crate::roads::RoadClearance::new(&ribbons)));
    commands.insert_resource(colliders);
    commands.insert_resource(plan);
    commands.insert_resource(zones);
    commands.insert_resource(roads);
}

/// Everything solid in the world for `map`, built without a window, a renderer or any textures: the walls,
/// fences, trees and the rest that the player bumps into, exactly as `build_world` places them. For
/// something that has to move through the world without being shown it, such as a bot.
///
/// The world is built into a throwaway `World` with plain asset stores, and everything but the colliders is
/// thrown away.
pub fn headless_colliders(map: &TerrainMap, params: &GenParams) -> Colliders {
    use bevy::ecs::world::CommandQueue;
    use crate::terrain::TerrainTextures;
    let mut world = World::new();
    let mut queue = CommandQueue::default();
    {
        let mut commands = Commands::new(&mut queue, &world);
        let textures = TerrainTextures { ground: crate::ground_textures::GroundArrays { diffuse: Handle::default(), normal: Handle::default() }, hedge: Handle::default(), wood: Handle::default(), wall_stone: Handle::default(), asphalt: Handle::default(), track: Handle::default() };
        build_world(
            &mut commands,
            &mut Assets::default(),
            &mut Assets::default(),
            &mut Assets::default(),
            &mut Assets::default(),
            &mut Assets::default(),
            &mut Assets::default(),
            &mut Assets::default(),
            &textures,
            map,
            params,
        );
    }
    queue.apply(&mut world);
    world.remove_resource::<Colliders>().unwrap_or_default()
}

/// WORLD_SKIP=trees,fields,boundaries,settlements leaves those categories out, to measure what
/// each costs. Development only.
pub fn skip(category: &str) -> bool {
    std::env::var("WORLD_SKIP").is_ok_and(|v| v.split(',').any(|c| c == category))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prints where to stand, and which way to face, to have a wall half a metre ahead: `FPS_AT=x,z FPS_YAW=yaw`
    /// (for looking at how things are drawn against a wall). `cargo test --lib where_to_stand_at_a_wall -- --ignored --nocapture`
    #[test]
    #[ignore = "prints a place to look at"]
    fn where_to_stand_at_a_wall() {
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let colliders = headless_colliders(&map, &params);
        let start = map.spawn_point();
        let mut best: Option<(f32, Vec2, Vec2)> = None;
        for i in 0..64 {
            let angle = i as f32 / 64.0 * std::f32::consts::TAU;
            let direction = Vec2::from_angle(angle);
            let from = Vec3::new(start.x, map.height_at(start) + 1.8, start.y);
            let to = from + Vec3::new(direction.x, 0.0, direction.y) * 40.0;
            if let Some(hit) = colliders.segment_hit(from, to) {
                let distance = hit.t * 40.0;
                if best.is_none_or(|(d, _, _)| distance < d) && hit.normal.y.abs() < 0.2 {
                    best = Some((distance, Vec2::new(hit.point.x, hit.point.z), Vec2::new(hit.normal.x, hit.normal.z)));
                }
            }
        }
        let (distance, point, normal) = best.expect("a wall near the start");
        let stand = point + normal.normalize() * 0.5;
        let yaw = (normal.x).atan2(normal.y);
        eprintln!("nearest wall {distance:.1} m from the start: FPS_AT={:.2},{:.2} FPS_YAW={yaw:.3}", stand.x, stand.y);
        // And what a gun held out there would be in.
        let eye = Transform::from_xyz(stand.x, map.height_at(stand) + 1.8, stand.y).with_rotation(Quat::from_rotation_y(yaw));
        let cam = GlobalTransform::from(eye);
        let model = crate::gun_models::build(crate::weapons::WeaponKind::Sterling);
        let pose = crate::weapon::gun_pose_held(&model.spec, &crate::weapons::WeaponKind::Sterling.def().handling, 0.0, 0.0, 0.0, 0.0);
        let muzzle = cam.mul_transform(pose).transform_point(model.spec.muzzle);
        let facing = cam.forward();
        eprintln!("eye {:?} looking {:?} muzzle {muzzle:?} wall at {point:?}: {:?}", eye.translation, *facing, colliders.segment_hit(eye.translation, muzzle));
        eprintln!("intrusion {}", crate::weapon::barrel_intrusion(eye.translation, muzzle, &colliders, &map));
    }

    #[test]
    fn the_solids_of_a_world_can_be_built_with_no_window_and_are_the_same_every_time() {
        let params = GenParams::default();
        let map = TerrainMap::generate(crate::MAP_SEED, &params);
        let started = std::time::Instant::now();
        let first = headless_colliders(&map, &params);
        eprintln!("built {} solids in {:?}", first.len(), started.elapsed());
        assert!(first.len() > 1000, "{} solids", first.len());
        assert_eq!(first.len(), headless_colliders(&map, &params).len(), "the same map gives the same world");
    }
}
