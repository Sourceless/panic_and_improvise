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

/// WORLD_SKIP=trees,fields,boundaries,settlements leaves those categories out, to measure what
/// each costs. Development only.
pub fn skip(category: &str) -> bool {
    std::env::var("WORLD_SKIP").is_ok_and(|v| v.split(',').any(|c| c == category))
}
