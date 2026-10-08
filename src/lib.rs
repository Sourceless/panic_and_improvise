pub mod cloud_material;
pub mod cloud_shadows;
pub mod contour;
pub mod field_material;
pub mod fill;
pub mod grass;
pub mod ground_textures;
pub mod gun_model;
pub mod look;
pub mod map;
pub mod mipmaps;
pub mod params;
pub mod perf;
pub mod player;
pub mod roads;
pub mod settlement;
pub mod target;
pub mod vegetation;
pub mod terrain;
pub mod water_material;
pub mod wind;
pub mod weapon;
pub mod wind_material;
pub mod world;
pub mod zones;

use bevy::prelude::*;

use crate::map::{TerrainMap, HALF_SIZE};

pub const MAP_HALF_SIZE: f32 = HALF_SIZE;
pub const MAP_SEED: u64 = 2026;

/// The seed for the map the game plays on: a fresh random one each run. `FPS_SEED` pins it
/// (to replay a map), and the benchmark and screenshot modes pin it to `MAP_SEED` so their
/// results stay comparable from run to run.
pub fn game_seed() -> u64 {
    if let Some(seed) = std::env::var("FPS_SEED").ok().and_then(|v| v.parse().ok()) {
        return seed;
    }
    if std::env::var("FPS_BENCH").is_ok() || std::env::var("FPS_SCREENSHOT").is_ok() {
        return MAP_SEED;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(MAP_SEED, |d| d.as_nanos() as u64);
    // Mix the bits so runs started close together still get unrelated maps.
    let mut z = nanos.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (z ^ (z >> 31)) % 1_000_000
}

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<TerrainMap>() {
            let seed = game_seed();
            info!("map seed {seed} (run with FPS_SEED={seed} to get this map again)");
            app.insert_resource(TerrainMap::generate(seed, &params::GenParams::default()));
        }
        app.add_plugins((
            player::PlayerPlugin,
            weapon::WeaponPlugin,
            target::TargetPlugin,
        ));
    }
}
