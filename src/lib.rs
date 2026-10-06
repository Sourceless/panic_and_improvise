pub mod player;
pub mod target;
pub mod weapon;

use bevy::prelude::*;

pub const MAP_HALF_SIZE: f32 = 50.0;

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            player::PlayerPlugin,
            weapon::WeaponPlugin,
            target::TargetPlugin,
        ));
    }
}
