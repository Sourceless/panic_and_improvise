// Volumetric clouds, drawn on a big dome that follows the camera.
//
// Each pixel of the dome marches a ray from the camera through a slab of sky (see
// assets/shaders/clouds.wgsl), sampling a 3D noise density, lighting it from the sun and
// accumulating colour and opacity. The result is blended over the sky, behind any terrain.
// Bevy has no cloud renderer of its own, so this is entirely ours.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

pub type CloudMaterial = ExtendedMaterial<StandardMaterial, CloudExtension>;

/// Radius of the dome, in metres: beyond the far reaches of the map and inside the far plane.
pub const DOME_RADIUS: f32 = 45_000.0;

/// Altitudes (above sea level, metres) of the bottom and top of the main cloud layer.
pub const BASE: f32 = 1_700.0;
pub const TOP: f32 = 3_500.0;

/// How cloudy the sky is at a moment in time.
#[derive(Clone, Copy, Debug)]
pub struct Weather {
    /// Cover of the main (cumulus / stratocumulus) layer, 0 clear to 1 overcast.
    pub coverage: f32,
    /// Cover of the high, wispy cirrus.
    pub cirrus: f32,
}

/// Slowly changing weather: clearer and cloudier spells, over minutes, from a few incommensurate
/// sine waves so it never repeats exactly. `seconds` is time since the game started.
pub fn weather(seconds: f32) -> Weather {
    Weather {
        coverage: (0.36 + 0.15 * (seconds * 0.0113).sin() + 0.07 * (seconds * 0.0297 + 2.0).sin()).clamp(0.1, 0.72),
        cirrus: (0.5 + 0.35 * (seconds * 0.0071 + 1.0).sin() + 0.1 * (seconds * 0.021).sin()).clamp(0.0, 1.0),
    }
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct CloudExtension {
    /// xyz: unit vector toward the sun; w: the sun's illuminance in lux.
    #[uniform(100)]
    pub sun: Vec4,
    /// x: altitude of the cloud base, y: altitude of the tops (both above sea level, metres);
    /// z: coverage, 0 (clear) to 1 (overcast); w: density.
    #[uniform(101)]
    pub layer: Vec4,
    /// xy: how far the whole cloud field has drifted on the wind (metres); z: cirrus cover.
    #[uniform(102)]
    pub drift: Vec4,
}

impl MaterialExtension for CloudExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/clouds.wgsl".into()
    }
}

pub fn cloud_material(sun_direction: Vec3, sun_lux: f32, coverage: f32) -> CloudMaterial {
    CloudMaterial {
        base: StandardMaterial {
            // The shader outputs finished, premultiplied colour; the PBR path isn't used.
            unlit: true,
            alpha_mode: AlphaMode::Premultiplied,
            cull_mode: None,
            ..default()
        },
        extension: CloudExtension {
            sun: sun_direction.extend(sun_lux),
            layer: Vec4::new(BASE, TOP, coverage, 1.0),
            drift: Vec4::new(0.0, 0.0, 0.4, 0.0),
        },
    }
}
