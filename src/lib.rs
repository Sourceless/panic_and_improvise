pub mod map;
pub mod player;
pub mod target;
pub mod terrain;
pub mod weapon;

use bevy::prelude::*;

use crate::map::{TerrainMap, HALF_SIZE};

pub const MAP_HALF_SIZE: f32 = HALF_SIZE;
pub const MAP_SEED: u64 = 2026;

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(TerrainMap::generate(MAP_SEED))
            .add_plugins((
                player::PlayerPlugin,
                weapon::WeaponPlugin,
                target::TargetPlugin,
            ));
    }
}
