pub mod ammo;
pub mod ballistics;
pub mod body;
pub mod bot;
pub mod building;
pub mod cloud_material;
pub mod collision;
pub mod controls;
pub mod cloud_shadows;
pub mod contour;
pub mod crosshair;
pub mod field_material;
pub mod figure;
pub mod fill;
pub mod footsteps;
pub mod grass;
pub mod impact;
pub mod inventory;
pub mod inventory_text;
pub mod inventory_ui;
pub mod item_models;
pub mod gun_state;
pub mod ground_textures;
pub mod gun_model;
pub mod health;
pub mod hud;
pub mod gun_models;
pub mod look;
pub mod loot;
pub mod map;
pub mod meshbake;
pub mod menu;
pub mod mipmaps;
pub mod muzzle_flash;
pub mod net_host;
pub mod net;
pub mod navigation;
pub mod params;
pub mod pickup;
pub mod range;
pub mod recorder;
pub mod recording;
pub mod perf;
pub mod player;
pub mod playback;
pub mod road_material;
pub mod roads;
pub mod sound;
pub mod settlement;
pub mod settlement_plan;
pub mod target;
pub mod vegetation;
pub mod terrain;
pub mod water_material;
pub mod wind;
pub mod weapon;
pub mod walking;
pub mod weapons;
pub mod windows;
pub mod wind_material;
pub mod world;
pub mod zombie;
pub mod zones;

use bevy::prelude::*;

use crate::map::{TerrainMap, HALF_SIZE};

pub const MAP_HALF_SIZE: f32 = HALF_SIZE;
pub const MAP_SEED: u64 = 2026;

/// The release this was built as (set by the build of a release, `0.1.<number>`); the crate's own version when built
/// any other way.
pub const VERSION: &str = match option_env!("PANIC_AND_IMPROVISE_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

/// What the game's window is called.
pub fn window_title() -> String {
    format!("Panic and Improvise v{VERSION}")
}

#[cfg(test)]
mod version_tests {
    #[test]
    fn the_window_is_named_for_the_game_and_its_version() {
        let title = super::window_title();
        assert!(title.starts_with("Panic and Improvise v"), "{title}");
        assert!(title.ends_with(super::VERSION) && super::VERSION.chars().next().unwrap().is_ascii_digit(), "{title}");
    }
}

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
            footsteps::FootstepsPlugin,
            loot::LootPlugin,
            pickup::PickupPlugin,
            range::RangePlugin,
            weapon::WeaponPlugin,
            collision::CollisionPlugin,
            controls::ControlsPlugin::default(),
            sound::SoundPlugin,
            impact::ImpactPlugin,
            target::TargetPlugin,
            figure::FigurePlugin,
            body::BodyPlugin,
            recorder::RecorderPlugin,
        ));
    }
}
