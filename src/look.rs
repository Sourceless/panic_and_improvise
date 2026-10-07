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
use bevy::light::{Atmosphere, AtmosphereEnvironmentMapLight, CascadeShadowConfigBuilder, FogVolume, VolumetricFog, VolumetricLight};
use bevy::pbr::AtmosphereSettings;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;

use crate::cloud_material::{cloud_material, CloudMaterial, DOME_RADIUS};
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection};

pub struct LookPlugin;

impl Plugin for LookPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LookSettings>()
            .insert_resource(bevy::light::DirectionalLightShadowMap { size: env_or("LOOK_SHADOW_RES", 1024) })
            .add_systems(Startup, spawn_sun_and_sky)
            .add_systems(Update, (dress_new_cameras, follow_camera));
    }
}

#[derive(Resource, Clone)]
pub struct LookSettings {
    /// The sun casts shadows (off for far-away previews, where they cost and show nothing).
    pub shadows: bool,
    /// Render the physical sky and atmosphere. Off gives a flat, clear daylight look with no
    /// distance haze, for high overhead previews of the whole map.
    pub atmosphere: bool,
    /// Volumetric clouds (they need the atmosphere's sky to sit in front of).
    pub clouds: bool,
    /// How much of the sky the clouds cover, 0 (clear) to 1 (overcast).
    pub cloud_coverage: f32,
    /// Light shafts (god rays) through the air, which need shadows to have anything to cast.
    pub god_rays: bool,
    /// Camera exposure; higher is darker.
    pub ev100: f32,
    /// Sun height above the horizon, in degrees.
    pub sun_elevation: f32,
    /// Compass bearing the sun is in, in degrees.
    pub sun_azimuth: f32,
}

impl Default for LookSettings {
    fn default() -> Self {
        LookSettings { shadows: true, atmosphere: true, clouds: true, cloud_coverage: 0.42, god_rays: true, ev100: 13.0, sun_elevation: 27.0, sun_azimuth: 130.0 }
    }
}

#[derive(Component)]
pub struct Sun;

/// The dome the clouds are drawn on, kept centred on the camera.
#[derive(Component)]
struct CloudDome;

/// The box of air that light shafts are rendered in, kept centred on the camera.
#[derive(Component)]
struct FogBox;

fn spawn_sun_and_sky(
    mut commands: Commands,
    mut media: ResMut<Assets<ScatteringMedium>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut clouds: ResMut<Assets<CloudMaterial>>,
    mut images: ResMut<Assets<Image>>,
    settings: Res<LookSettings>,
) {
    if settings.atmosphere && !disabled("atmo") {
        commands.spawn(Atmosphere::earth(media.add(ScatteringMedium::default())));
    } else {
        // Without the atmosphere there is no sky to light the shadows, so add flat fill light
        // and a plain sky colour behind everything.
        commands.insert_resource(GlobalAmbientLight { color: Color::srgb(0.85, 0.9, 1.0), brightness: 2600.0, ..default() });
        commands.insert_resource(ClearColor(Color::srgb(0.62, 0.74, 0.88)));
    }

    // The light travels along its own -Z, so tilt it down by the sun's elevation.
    let rotation = Quat::from_euler(
        EulerRot::YXZ,
        settings.sun_azimuth.to_radians(),
        -settings.sun_elevation.to_radians(),
        0.0,
    );
    let sun = commands
        .spawn((
        Sun,
        DirectionalLight {
            illuminance: lux::RAW_SUNLIGHT,
            shadow_maps_enabled: settings.shadows && !disabled("shadows"),
            ..default()
        },
        // Three cascades out to 420 m: sharp shadows around the player, softer ones to the
        // middle distance, and enough reach for light shafts to run a good way into the haze.
        // (Shadows were by far the most expensive part of the look when measured; this is as
        // far as seemed worth paying for.)
        CascadeShadowConfigBuilder {
            num_cascades: env_or("LOOK_CASCADES", 3),
            minimum_distance: 0.5,
            maximum_distance: env_or("LOOK_SHADOW_DIST", 420.0),
            first_cascade_far_bound: env_or("LOOK_SHADOW_FIRST", 22.0),
            overlap_proportion: 0.25,
        }
        .build(),
        Transform::from_rotation(rotation),
    ))
        .id();

    let shadows = settings.shadows && !disabled("shadows");
    let sky = settings.atmosphere && !disabled("atmo");
    if shadows && settings.god_rays && !disabled("rays") {
        // Light shafts: the sun lights a box of thin haze around the camera, and the shadow map
        // decides which parts of that haze are lit.
        commands.entity(sun).insert(VolumetricLight);
        commands.spawn((
            FogBox,
            FogVolume {
                density_factor: env_or("LOOK_RAY_DENSITY", 0.0019),
                absorption: 0.2,
                scattering: 0.5,
                scattering_asymmetry: 0.82,
                fog_color: Color::srgb(0.92, 0.95, 1.0),
                light_intensity: env_or("LOOK_RAY_LIGHT", 2.2),
                ..default()
            },
            Transform::from_scale(Vec3::new(1000.0, 240.0, 1000.0)),
        ));
    }
    if sky && settings.clouds && !disabled("clouds") {
        // The light travels along its own -Z, so the sun is in the opposite direction.
        let to_sun = rotation * Vec3::Z;
        if !disabled("cloudshadows") {
            // The clouds' shadows: a texture the sun shines through (see cloud_shadows.rs).
            let shadow_map = crate::cloud_shadows::CloudShadowMap::new(&mut images, rotation);
            commands.entity(sun).insert((
                shadow_map.light_texture(),
                Transform::from_translation(shadow_map.anchor())
                    .with_rotation(rotation)
                    .with_scale(Vec3::splat(crate::cloud_shadows::HALF_SIZE)),
            ));
            commands.insert_resource(shadow_map);
        }
        commands.spawn((
            CloudDome,
            bevy::light::NotShadowCaster,
            Mesh3d(meshes.add(Sphere::new(1.0).mesh().uv(48, 24))),
            MeshMaterial3d(clouds.add(cloud_material(to_sun, lux::RAW_SUNLIGHT, settings.cloud_coverage))),
            Transform::from_scale(Vec3::splat(DOME_RADIUS)),
        ));
    }
}

fn follow_camera(
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut dome: Query<&mut Transform, (With<CloudDome>, Without<FogBox>)>,
    mut fog: Query<&mut Transform, (With<FogBox>, Without<CloudDome>)>,
) {
    let Some(camera) = cameras.iter().next() else { return };
    let at = camera.translation();
    for mut t in &mut dome {
        t.translation = at;
    }
    for mut t in &mut fog {
        // Mostly above the camera, since the ground below it is solid anyway.
        t.translation = at + Vec3::new(0.0, 90.0, 0.0);
    }
}

// Tunables can be overridden from the environment while experimenting.
fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

// LOOK_OFF=atmo,env,bloom,grading turns individual effects off, to find out what each costs.
fn disabled(effect: &str) -> bool {
    std::env::var("LOOK_OFF").is_ok_and(|v| v.split(',').any(|e| e == effect))
}

fn dress_new_cameras(mut commands: Commands, new: Query<(Entity, Option<&Projection>), Added<Camera3d>>, settings: Res<LookSettings>) {
    for (camera, projection) in &new {
        let mut entity = commands.entity(camera);
        if std::env::var("LOOK_MSAA").is_ok_and(|v| v == "off") {
            entity.insert(Msaa::Off);
        }
        // The default far plane is 1 km; the world (and the sea beyond it) reaches much further.
        let far_enough = matches!(projection, Some(Projection::Perspective(p)) if p.far > 5_000.0);
        if !far_enough {
            entity.insert(Projection::Perspective(PerspectiveProjection { far: 80_000.0, ..default() }));
        }
        entity.insert((Exposure { ev100: settings.ev100 }, Tonemapping::TonyMcMapface));
        let sky = settings.atmosphere && !disabled("atmo");
        if sky {
            entity.insert(AtmosphereSettings::default());
        }
        if sky && !disabled("env") {
            let size = std::env::var("LOOK_ENV_SIZE").ok().and_then(|v| v.parse().ok()).unwrap_or(64);
            entity.insert(AtmosphereEnvironmentMapLight { size: UVec2::splat(size), ..default() });
        }
        if !disabled("bloom") {
            entity.insert(Bloom { intensity: 0.06, ..Bloom::NATURAL });
        }
        if settings.shadows && settings.god_rays && !disabled("shadows") && !disabled("rays") {
            entity.insert(VolumetricFog {
                ambient_intensity: 0.0,
                step_count: env_or("LOOK_RAY_STEPS", 40),
                // Fewer steps over a longer reach would band, so the start of each ray is
                // jittered (and the grain is hidden by the haze's softness).
                jitter: 1.5,
                ..default()
            });
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
