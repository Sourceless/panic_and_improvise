// Builds everything that makes up the world from a generated terrain map: land-use zones,
// roads, the terrain mesh and water, settlements, fields, hedges and vegetation. The game and
// the map viewer both call this, so the viewer always previews exactly what the game builds.

use bevy::prelude::*;

use crate::field_material::FieldMaterial;
use crate::fill::spawn_fill;
use crate::map::TerrainMap;
use crate::params::GenParams;
use crate::roads::{spawn_roads, RoadNetwork};
use crate::terrain::{spawn_terrain, TerrainMaterial, TerrainTextures};
use crate::zones::ZoneMap;

/// Generates the zone map and road network for `map`, spawns all world entities, and inserts
/// the zone map and road network as resources.
pub fn build_world(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    standard: &mut Assets<StandardMaterial>,
    terrain: &mut Assets<TerrainMaterial>,
    fields: &mut Assets<FieldMaterial>,
    textures: &TerrainTextures,
    map: &TerrainMap,
    params: &GenParams,
) {
    let zones = ZoneMap::generate(map, params);
    let roads = RoadNetwork::generate(map, params);
    spawn_terrain(commands, meshes, standard, terrain, textures, map, &zones);
    spawn_roads(commands, meshes, standard, map, &roads);
    spawn_fill(commands, meshes, standard, fields, textures, map, &zones, &roads, params);
    commands.insert_resource(zones);
    commands.insert_resource(roads);
}

/// WORLD_SKIP=trees,fields,boundaries,settlements leaves those categories out, to measure what
/// each costs. Development only.
pub fn skip(category: &str) -> bool {
    std::env::var("WORLD_SKIP").is_ok_and(|v| v.split(',').any(|c| c == category))
}
