pub mod contour;
pub mod fill;
pub mod map;
pub mod params;
pub mod player;
pub mod roads;
pub mod settlement;
pub mod target;
pub mod terrain;
pub mod weapon;
pub mod zones;

use bevy::prelude::*;

use crate::map::{TerrainMap, HALF_SIZE};

pub const MAP_HALF_SIZE: f32 = HALF_SIZE;
pub const MAP_SEED: u64 = 2026;

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<TerrainMap>() {
            app.insert_resource(TerrainMap::generate(MAP_SEED, &params::GenParams::default()));
        }
        app.add_plugins((
            player::PlayerPlugin,
            weapon::WeaponPlugin,
            target::TargetPlugin,
        ));
    }
}
