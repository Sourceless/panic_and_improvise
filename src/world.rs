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
use crate::terrain::{spawn_terrain, TerrainMaterial, TerrainTextures};
use crate::water_material::WaterMaterial;
use crate::zones::ZoneMap;

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
    let zones = ZoneMap::generate(map, params);
    let mut roads = RoadNetwork::generate(map, params);
    // Each settlement is planned along the roads that reach it, and brings its own lanes.
    let mut plan = SettlementPlan::generate(map, &roads);
    plan.sheds = crate::fill::shed_buildings(map, &zones);
    roads.add_ribbons(plan.lanes());
    // Everything solid that the generators place is collected here for the player to bump into.
    let mut colliders = Colliders::default();
    spawn_terrain(commands, meshes, standard, terrain, textures, map, &zones, waters, images, &plan, &mut colliders);
    spawn_roads(commands, meshes, road_materials, textures, map, &roads);
    spawn_fill(commands, meshes, standard, fields, textures, map, &zones, &roads, params, &plan, &mut colliders);
    commands.insert_resource(crate::footsteps::RoadSurfaces(crate::roads::RoadClearance::new(&crate::roads::road_ribbons(map, &roads))));
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
