// One wind for the whole world: a single steady direction that the trees and grass bend
// along, the clouds drift with, and the water's waves run with.
//
// Gusts aren't stored here: the vegetation shader already rolls a slow gust field across the
// landscape (so a gust reaches trees one after another instead of all at once), which needs
// no per-frame upload. What is stored is what changes rarely (the direction and the mean
// speed) and the cloud drift, which accumulates every frame.

use bevy::prelude::*;

use crate::cloud_material::CloudMaterial;
use crate::water_material::WaterMaterial;
use crate::wind_material::WindMaterial;

/// Clouds travel faster than the wind at the ground, as the real wind does with height.
const CLOUD_SPEED_RATIO: f32 = 1.6;
/// The direction the water's waves were authored for; the wave field is turned by the
/// difference between this and the actual wind.
pub const WAVE_REFERENCE_HEADING: f32 = 0.35;
/// The wind speed the tree and grass sway amounts were tuned at.
pub const REFERENCE_SPEED: f32 = 7.0;

#[derive(Resource, Clone, Copy, Debug)]
pub struct Wind {
    /// The direction the wind blows toward, as an angle in the ground plane from +X toward +Z.
    pub heading: f32,
    /// Mean speed in metres per second at the ground.
    pub speed: f32,
}

impl Default for Wind {
    fn default() -> Self {
        Wind { heading: WAVE_REFERENCE_HEADING, speed: REFERENCE_SPEED }
    }
}

impl Wind {
    /// Unit vector in the ground plane (x, z) the wind blows along.
    pub fn direction(&self) -> Vec2 {
        Vec2::from_angle(self.heading)
    }

    /// How hard the wind is blowing compared with the breeze the sway was tuned for.
    pub fn strength(&self) -> f32 {
        self.speed / REFERENCE_SPEED
    }
}

/// How far the whole cloud field has drifted from where it started, in metres.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct CloudDrift(pub Vec2);

pub struct WindPlugin;

impl Plugin for WindPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Wind>()
            .init_resource::<CloudDrift>()
            .add_systems(Update, (advance_cloud_drift, push_wind_to_materials, push_drift_to_clouds).chain());
    }
}

fn advance_cloud_drift(time: Res<Time>, wind: Res<Wind>, mut drift: ResMut<CloudDrift>) {
    drift.0 += wind.direction() * wind.speed * CLOUD_SPEED_RATIO * time.delta_secs();
}

// Wind changes rarely, so the materials only hear about it when it does (and once at the
// start, when materials are first created).
fn push_wind_to_materials(
    wind: Res<Wind>,
    mut winds: ResMut<Assets<WindMaterial>>,
    mut waters: ResMut<Assets<WaterMaterial>>,
    mut last: Local<Option<(f32, f32, usize, usize)>>,
) {
    let key = (wind.heading, wind.speed, winds.len(), waters.len());
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    let d = wind.direction();
    for (_, material) in winds.iter_mut() {
        material.extension.wind = Vec4::new(d.x, d.y, wind.strength(), 0.0);
    }
    for (_, material) in waters.iter_mut() {
        // How far to turn the waves from their authored direction, and how rough the sea is.
        material.extension.wind = Vec4::new(
            wind.heading - WAVE_REFERENCE_HEADING,
            wind.strength().clamp(0.4, 1.7),
            0.0,
            0.0,
        );
    }
}

fn push_drift_to_clouds(drift: Res<CloudDrift>, time: Res<Time>, mut clouds: ResMut<Assets<CloudMaterial>>) {
    let weather = crate::cloud_material::weather(time.elapsed_secs());
    for (_, material) in clouds.iter_mut() {
        material.extension.drift = Vec4::new(drift.0.x, drift.0.y, weather.cirrus, 0.0);
        material.extension.layer.z = weather.coverage;
    }
}
