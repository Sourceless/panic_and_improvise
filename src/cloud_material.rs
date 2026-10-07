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

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct CloudExtension {
    /// xyz: unit vector toward the sun; w: the sun's illuminance in lux.
    #[uniform(100)]
    pub sun: Vec4,
    /// x: altitude of the cloud base, y: altitude of the tops (both above sea level, metres);
    /// z: coverage, 0 (clear) to 1 (overcast); w: density.
    #[uniform(101)]
    pub layer: Vec4,
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
            layer: Vec4::new(1_700.0, 3_500.0, coverage, 1.0),
        },
    }
}
