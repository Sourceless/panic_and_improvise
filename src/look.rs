// The overall look of the world: sun, sky, atmosphere, exposure and grading. Added once, it
// dresses every 3D camera that appears, so the game and the map viewer share it exactly.
//
// The atmosphere is physically based (Hillaire 2020): it renders the sky, the sun's colour
// changing with its height, and distance haze that tints far terrain toward the sky colour.
// That replaces both a sky box and distance fog. Because it works in physical units the
// sun is a real-world 100k+ lux and the camera exposure is set to match.

use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::atmosphere::ScatteringMedium;
use bevy::light::light_consts::lux;
use bevy::light::{Atmosphere, AtmosphereEnvironmentMapLight, CascadeShadowConfigBuilder};
use bevy::pbr::AtmosphereSettings;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};

pub struct LookPlugin;

impl Plugin for LookPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LookSettings>()
            .insert_resource(bevy::light::DirectionalLightShadowMap { size: env_or("LOOK_SHADOW_RES", 1024) })
            .add_systems(Startup, spawn_sun_and_sky)
            .add_systems(Update, dress_new_cameras);
    }
}

#[derive(Resource, Clone)]
pub struct LookSettings {
    /// The sun casts shadows (off for far-away previews, where they cost and show nothing).
    pub shadows: bool,
    /// Camera exposure; higher is darker.
    pub ev100: f32,
    /// Sun height above the horizon, in degrees.
    pub sun_elevation: f32,
    /// Compass bearing the sun is in, in degrees.
    pub sun_azimuth: f32,
}

impl Default for LookSettings {
    fn default() -> Self {
        LookSettings { shadows: true, ev100: 13.0, sun_elevation: 27.0, sun_azimuth: 130.0 }
    }
}

#[derive(Component)]
pub struct Sun;

fn spawn_sun_and_sky(mut commands: Commands, mut media: ResMut<Assets<ScatteringMedium>>, settings: Res<LookSettings>) {
    if !disabled("atmo") {
        commands.spawn(Atmosphere::earth(media.add(ScatteringMedium::default())));
    }

    // The light travels along its own -Z, so tilt it down by the sun's elevation.
    let rotation = Quat::from_euler(
        EulerRot::YXZ,
        settings.sun_azimuth.to_radians(),
        -settings.sun_elevation.to_radians(),
        0.0,
    );
    commands.spawn((
        Sun,
        DirectionalLight {
            illuminance: lux::RAW_SUNLIGHT,
            shadow_maps_enabled: settings.shadows && !disabled("shadows"),
            ..default()
        },
        // Two cascades out to 120 m: sharp shadows around the player, and nothing farther away is
        // big enough on screen to be worth the cost of rendering them (shadows were by far the
        // most expensive part of the look when measured).
        CascadeShadowConfigBuilder {
            num_cascades: env_or("LOOK_CASCADES", 2),
            minimum_distance: 0.5,
            maximum_distance: env_or("LOOK_SHADOW_DIST", 120.0),
            first_cascade_far_bound: env_or("LOOK_SHADOW_FIRST", 25.0),
            overlap_proportion: 0.25,
        }
        .build(),
        Transform::from_rotation(rotation),
    ));
}

// Tunables can be overridden from the environment while experimenting.
fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

// LOOK_OFF=atmo,env,bloom,grading turns individual effects off, to find out what each costs.
fn disabled(effect: &str) -> bool {
    std::env::var("LOOK_OFF").is_ok_and(|v| v.split(',').any(|e| e == effect))
}

fn dress_new_cameras(mut commands: Commands, new: Query<Entity, Added<Camera3d>>, settings: Res<LookSettings>) {
    for camera in &new {
        let mut entity = commands.entity(camera);
        entity.insert((Exposure { ev100: settings.ev100 }, Tonemapping::TonyMcMapface));
        if !disabled("atmo") {
            entity.insert(AtmosphereSettings::default());
        }
        if !disabled("env") && !disabled("atmo") {
            let size = std::env::var("LOOK_ENV_SIZE").ok().and_then(|v| v.parse().ok()).unwrap_or(64);
            entity.insert(AtmosphereEnvironmentMapLight { size: UVec2::splat(size), ..default() });
        }
        if !disabled("bloom") {
            entity.insert(Bloom { intensity: 0.06, ..Bloom::NATURAL });
        }
        if !disabled("grading") {
            entity.insert(ColorGrading {
                global: ColorGradingGlobal { temperature: 0.015, post_saturation: 1.12, ..default() },
                midtones: ColorGradingSection { contrast: 1.12, ..default() },
                ..default()
            });
        }
    }
}
