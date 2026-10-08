// The muzzle flash: for a few hundredths of a second after each shot, a bright star of flame
// at the end of the barrel (a forward spike for the jet of gas, and a ring of side spikes),
// plus a short pulse of orange light that falls on the gun, the grass and anything near.
//
// The flash is unlit and additive, so it just adds light to the picture; the vertex colours
// run from a white-hot centre to orange tips that fade to nothing, and the values are above 1
// so it blooms. A new random twist and size each shot stops it looking like one repeated sprite.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// How long a flash lasts, in seconds: two or three frames at 60 fps.
pub const FLASH_TIME: f32 = 0.05;
/// Peak brightness of the light pulse, in lumens. The camera's exposure is set for full
/// daylight, so a flash light has to be a very powerful one to register at all.
const LIGHT_PEAK: f32 = 3_000_000.0;
const SIDE_SPIKES: usize = 7;

#[derive(Component)]
pub struct MuzzleFlash;

#[derive(Component)]
pub struct MuzzleFlashLight;

/// A diamond-shaped spike from the origin out to `tip`, `width` across at its widest, `side`
/// giving the direction it's widest in. Pushes four vertices (centre-hot, tip-faded).
fn spike(positions: &mut Vec<[f32; 3]>, colors: &mut Vec<[f32; 4]>, indices: &mut Vec<u32>, tip: Vec3, side: Vec3, width: f32, hot: [f32; 4], cool: [f32; 4]) {
    let base = positions.len() as u32;
    let middle = tip * 0.3;
    positions.extend([Vec3::ZERO, middle + side * width, tip, middle - side * width].map(|p| p.to_array()));
    colors.extend([hot, hot, cool, hot]);
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// The flash's geometry: facing -Z (the way the barrel points), about 16 cm long.
pub fn flash_mesh() -> Mesh {
    let (mut positions, mut colors, mut indices) = (Vec::new(), Vec::new(), Vec::new());
    let hot = [1.0, 0.92, 0.65, 1.0];
    let tip = [1.0, 0.38, 0.06, 0.0];
    // Forward jet: two crossed spikes, so it has volume from any angle, and shorter ragged ones
    // either side of them at slight angles, which make the outline flame-like rather than a diamond.
    for side in [Vec3::X, Vec3::Y] {
        spike(&mut positions, &mut colors, &mut indices, Vec3::new(0.0, 0.0, -0.16), side, 0.019, hot, tip);
    }
    for (k, (dx, dy, length)) in [(0.045, 0.01, 0.105), (-0.05, -0.012, 0.09), (0.012, 0.05, 0.095), (-0.008, -0.045, 0.1)].into_iter().enumerate() {
        let tip_at = Vec3::new(dx, dy, -length);
        let side = if k % 2 == 0 { Vec3::Y } else { Vec3::X };
        spike(&mut positions, &mut colors, &mut indices, tip_at, side, 0.009, hot, tip);
    }
    // A short stub back the other way, where the gas leaks around the muzzle.
    spike(&mut positions, &mut colors, &mut indices, Vec3::new(0.0, 0.0, 0.03), Vec3::X, 0.014, hot, tip);
    // The star: spikes fanned around the barrel, alternately long and short.
    for k in 0..SIDE_SPIKES {
        let a = k as f32 / SIDE_SPIKES as f32 * std::f32::consts::TAU + 0.3;
        let length = if k % 2 == 0 { 0.085 } else { 0.055 };
        let dir = Vec3::new(a.cos(), a.sin(), -0.25);
        let side = Vec3::new(-a.sin(), a.cos(), 0.0);
        spike(&mut positions, &mut colors, &mut indices, dir.normalize() * length, side, 0.011, hot, tip);
    }
    let normals = vec![[0.0, 0.0, 1.0]; positions.len()];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices))
}

pub fn flash_material() -> StandardMaterial {
    StandardMaterial {
        // Above 1 so the flash is HDR-bright and blooms.
        base_color: Color::linear_rgb(3.4, 2.2, 1.1),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    }
}

/// Spawns the flash (and its light) as children of the gun, at the muzzle. Both start hidden /
/// dark; `visible` forces the flash on, for the preview tool.
pub fn spawn(gun: &mut ChildSpawnerCommands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, muzzle: Vec3, visible: bool) {
    gun.spawn((
        MuzzleFlash,
        Mesh3d(meshes.add(flash_mesh())),
        MeshMaterial3d(materials.add(flash_material())),
        Transform::from_translation(muzzle),
        if visible { Visibility::Inherited } else { Visibility::Hidden },
        bevy::light::NotShadowCaster,
    ));
    gun.spawn((
        MuzzleFlashLight,
        PointLight {
            color: Color::srgb(1.0, 0.62, 0.28),
            intensity: if visible { LIGHT_PEAK } else { 0.0 },
            range: 10.0,
            radius: 0.03,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_translation(muzzle + Vec3::new(0.0, 0.0, -0.1)),
    ));
}

/// Where in its life a flash is: `remaining` seconds left of `FLASH_TIME`.
/// Returns (scale of the flash, light intensity). Brightest at the start, gone by the end.
pub fn flash_state(remaining: f32) -> (f32, f32) {
    if remaining <= 0.0 {
        return (0.0, 0.0);
    }
    let life = (remaining / FLASH_TIME).clamp(0.0, 1.0);
    (0.55 + 0.45 * life, LIGHT_PEAK * life * life)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flash_is_a_well_formed_star_pointing_down_the_barrel() {
        let mesh = flash_mesh();
        let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION).and_then(|a| a.as_float3()).unwrap().to_vec();
        assert!(positions.iter().flatten().all(|v| v.is_finite()));
        let front = positions.iter().map(|p| p[2]).fold(f32::MAX, f32::min);
        let back = positions.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
        assert!((-0.17..-0.15).contains(&front), "jet reaches {front} m ahead");
        assert!(back < 0.05, "little goes backward ({back})");
        // Spikes fan out sideways too, and are bright in the middle and fade at the tips.
        let wide = positions.iter().map(|p| p[0].abs().max(p[1].abs())).fold(0.0, f32::max);
        assert!(wide > 0.04, "star spikes reach only {wide} m to the side");
        let colors = mesh.attribute(Mesh::ATTRIBUTE_COLOR).unwrap();
        let alphas: Vec<f32> = match colors {
            bevy::mesh::VertexAttributeValues::Float32x4(c) => c.iter().map(|c| c[3]).collect(),
            _ => panic!("colours should be rgba floats"),
        };
        assert!(alphas.iter().any(|&a| a == 1.0) && alphas.iter().any(|&a| a == 0.0));
    }

    #[test]
    fn a_flash_is_brightest_first_and_gone_by_its_end() {
        let (start_scale, start_light) = flash_state(FLASH_TIME);
        let (mid_scale, mid_light) = flash_state(FLASH_TIME * 0.5);
        assert!(start_scale > mid_scale && start_light > mid_light);
        assert_eq!(flash_state(0.0), (0.0, 0.0));
        assert_eq!(flash_state(-1.0), (0.0, 0.0));
        assert!(start_scale <= 1.0 + 1e-6 && (start_light - LIGHT_PEAK).abs() < 1.0);
    }
}
