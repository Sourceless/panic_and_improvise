// Shadows cast by the clouds.
//
// The sun carries a texture (a "light cookie") that dims its direct light wherever a cloud
// is in the way, so cloud shadows fall on everything that is lit - terrain, trees, houses,
// grass - with no work in any material. The texture is a shadow map of the sky: each texel is
// one sun ray, and holds how much of the sun that ray lets through after crossing the cloud
// layer. It is baked on the CPU from the very same density function the cloud shader draws
// (a Rust port of `density` in assets/shaders/clouds.wgsl, constant for constant), so shadows
// and clouds always agree.
//
// The clouds drift with the wind, and so must their shadows. Baking is slow, so instead of
// re-baking every frame the texture is re-baked every few seconds on a background thread, and
// between bakes the whole pattern is slid along the wind by moving the light (a light cookie
// is centred on its light's position).

use bevy::asset::RenderAssetUsages;
use bevy::light::DirectionalLightTexture;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::{futures_lite::future, AsyncComputeTaskPool, Task};

use crate::cloud_material::{weather, BASE, TOP};
use crate::wind::CloudDrift;

/// Texels along each side of the shadow texture.
const SIZE: usize = 256;
/// Half the width of the area the texture covers, in metres, measured across the sun's rays.
/// Comfortably more than the map needs, even after the sun's slant stretches it on the ground.
pub const HALF_SIZE: f32 = 8_000.0;
/// How long between re-bakes, in seconds.
const REBAKE_EVERY: f32 = 12.0;
/// How far toward the sun the light entity is parked, in metres (see `CloudShadowMap::anchor`).
const ANCHOR_DISTANCE: f32 = 60_000.0;
/// How dark a fully clouded patch of ground gets: the rest is skylight scattered by the cloud.
const SHADOW_FLOOR: f32 = 0.22;

/// The texture the sun uses as its cloud-shadow cookie, and what is needed to re-bake it.
#[derive(Resource)]
pub struct CloudShadowMap {
    pub image: Handle<Image>,
    /// The sun's orientation, which decides which rays the texels stand for.
    pub rotation: Quat,
}

impl CloudShadowMap {
    pub fn new(images: &mut Assets<Image>, rotation: Quat) -> Self {
        let mut image = Image::new(
            Extent3d { width: SIZE as u32, height: SIZE as u32, depth_or_array_layers: 1 },
            TextureDimension::D2,
            vec![255; SIZE * SIZE * 4],
            // All three colour channels carry the same value: the light multiplies its colour
            // by the texture, and a single-channel texture would tint the sun red.
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
        );
        image.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
            address_mode_u: bevy::image::ImageAddressMode::Repeat,
            address_mode_v: bevy::image::ImageAddressMode::Repeat,
            ..bevy::image::ImageSamplerDescriptor::linear()
        });
        CloudShadowMap { image: images.add(image), rotation }
    }

    /// Where the sun entity sits. Bevy treats a light texture as a decal as well, painting it
    /// onto every surface inside a box around the light; and a cookie's mapping depends only
    /// on a point's position *across* the light's rays, not *along* them. So the light is
    /// parked far up its own rays, which moves that box well clear of the world while leaving
    /// the shadow pattern exactly where it was.
    pub fn anchor(&self) -> Vec3 {
        self.rotation * Vec3::Z * ANCHOR_DISTANCE
    }

    pub fn light_texture(&self) -> DirectionalLightTexture {
        DirectionalLightTexture { image: self.image.clone(), tiled: true }
    }
}

#[derive(Resource, Default)]
struct BakeState {
    task: Option<Task<Vec<u8>>>,
    /// The cloud drift the current texture was baked at.
    baked_drift: Vec2,
    /// The drift the running task is baking at (becomes `baked_drift` when it finishes).
    pending_drift: Vec2,
    last_start: f32,
    started_once: bool,
}

pub struct CloudShadowPlugin;

impl Plugin for CloudShadowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BakeState>()
            .add_systems(Update, (start_bake, collect_bake, slide_shadows).chain());
    }
}

fn start_bake(
    time: Res<Time>,
    drift: Res<CloudDrift>,
    shadows: Option<Res<CloudShadowMap>>,
    mut state: ResMut<BakeState>,
) {
    let Some(shadows) = shadows else { return };
    let now = time.elapsed_secs();
    if state.task.is_some() || (state.started_once && now - state.last_start < REBAKE_EVERY) {
        return;
    }
    state.started_once = true;
    state.last_start = now;
    state.pending_drift = drift.0;
    let (rotation, at, coverage) = (shadows.rotation, drift.0, weather(now).coverage);
    state.task = Some(AsyncComputeTaskPool::get().spawn(async move { bake(rotation, at, coverage) }));
}

fn collect_bake(shadows: Option<Res<CloudShadowMap>>, mut state: ResMut<BakeState>, mut images: ResMut<Assets<Image>>) {
    let Some(shadows) = shadows else { return };
    let Some(task) = state.task.as_mut() else { return };
    let Some(pixels) = future::block_on(future::poll_once(task)) else { return };
    state.task = None;
    state.baked_drift = state.pending_drift;
    if let Some(mut image) = images.get_mut(&shadows.image) {
        image.data = Some(pixels);
    }
}

// The texture holds the clouds as they were when baked; the clouds have drifted since, and the
// pattern is slid by the same amount (the cookie is centred on the light, so moving the light
// moves it). The texture swap and this slide land in the same frame, so there is no jump.
fn slide_shadows(
    drift: Res<CloudDrift>,
    state: Res<BakeState>,
    shadows: Option<Res<CloudShadowMap>>,
    mut sun: Query<&mut Transform, With<DirectionalLightTexture>>,
) {
    let Some(shadows) = shadows else { return };
    let moved = drift.0 - state.baked_drift;
    for mut transform in &mut sun {
        transform.translation = shadows.anchor() + Vec3::new(moved.x, 0.0, moved.y);
    }
}

// ---------------------------------------------------------------------------------------
// The bake

fn bake(rotation: Quat, drift: Vec2, coverage: f32) -> Vec<u8> {
    let (right, up, to_sun) = (rotation * Vec3::X, rotation * Vec3::Y, rotation * Vec3::Z);
    let mut pixels = vec![255u8; SIZE * SIZE * 4];
    // A sun ray that never rises above the horizon can't be shadowed meaningfully.
    if to_sun.y < 0.05 {
        return pixels;
    }
    for j in 0..SIZE {
        for i in 0..SIZE {
            // The renderer maps a point's light-plane position (lx, ly) to texture coordinates
            // (u, v) = (0.5 - lx / 2, 0.5 + ly / 2), in units of the light's scale.
            let (u, v) = ((i as f32 + 0.5) / SIZE as f32, (j as f32 + 0.5) / SIZE as f32);
            let (lx, ly) = (1.0 - 2.0 * u, 2.0 * v - 1.0);
            let on_plane = right * (lx * HALF_SIZE) + up * (ly * HALF_SIZE);
            // Slide along the sun ray down to sea level, then follow it up through the clouds.
            let ground = on_plane + to_sun * (-on_plane.y / to_sun.y);
            let (t0, t1) = ((BASE - ground.y) / to_sun.y, (TOP - ground.y) / to_sun.y);
            const STEPS: usize = 12;
            let dt = (t1 - t0) / STEPS as f32;
            let mut optical_depth = 0.0;
            for k in 0..STEPS {
                let p = ground + to_sun * (t0 + (k as f32 + 0.5) * dt);
                optical_depth += density(p, drift, coverage) * dt;
            }
            let through = (-optical_depth * 0.0075 * 0.8).exp();
            let light = SHADOW_FLOOR + (1.0 - SHADOW_FLOOR) * through;
            let value = (light * 255.0 + 0.5) as u8;
            pixels[(j * SIZE + i) * 4..(j * SIZE + i) * 4 + 4].copy_from_slice(&[value, value, value, 255]);
        }
    }
    pixels
}

// ---------------------------------------------------------------------------------------
// A port of the cloud density function in assets/shaders/clouds.wgsl (without the fine
// "detail" erosion, which shadows are too soft to show). Keep the two in step.

fn fract3(v: Vec3) -> Vec3 {
    v - v.floor()
}

fn hash31(p: Vec3) -> f32 {
    let mut q = fract3(p * Vec3::new(0.1031, 0.1030, 0.0973));
    q += Vec3::splat(q.dot(q.yxz() + Vec3::splat(33.33)));
    let r = (q.x + q.y) * q.z;
    r - r.floor()
}

fn noise3(p: Vec3) -> f32 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * (Vec3::splat(3.0) - 2.0 * f);
    let h = |dx: f32, dy: f32, dz: f32| hash31(i + Vec3::new(dx, dy, dz));
    let x00 = h(0.0, 0.0, 0.0) + (h(1.0, 0.0, 0.0) - h(0.0, 0.0, 0.0)) * u.x;
    let x10 = h(0.0, 1.0, 0.0) + (h(1.0, 1.0, 0.0) - h(0.0, 1.0, 0.0)) * u.x;
    let x01 = h(0.0, 0.0, 1.0) + (h(1.0, 0.0, 1.0) - h(0.0, 0.0, 1.0)) * u.x;
    let x11 = h(0.0, 1.0, 1.0) + (h(1.0, 1.0, 1.0) - h(0.0, 1.0, 1.0)) * u.x;
    let y0 = x00 + (x10 - x00) * u.y;
    let y1 = x01 + (x11 - x01) * u.y;
    y0 + (y1 - y0) * u.z
}

fn fbm3_lo(p: Vec3) -> f32 {
    0.6 * noise3(p) + 0.28 * noise3(p * 2.03 + Vec3::splat(11.7)) + 0.12 * noise3(p * 4.1 + Vec3::splat(3.9))
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How dense the cloud is at `world`, for a field that has drifted by `drift`.
pub fn density(world: Vec3, drift: Vec2, coverage: f32) -> f32 {
    let p = world - Vec3::new(drift.x, 0.0, drift.y);
    let h = ((world.y - BASE) / (TOP - BASE)).clamp(0.0, 1.0);

    let typ = fbm3_lo(Vec3::new(p.x, 0.0, p.z) / 26000.0 + Vec3::new(5.2, 0.0, 1.3));
    let flat_k = smoothstep(0.53, 0.68, typ);
    let profile_cumulus = smoothstep(0.0, 0.1, h) * (1.0 - smoothstep(0.5, 1.0, h));
    let profile_strato = smoothstep(0.0, 0.14, h) * (1.0 - smoothstep(0.3, 0.5, h));
    let profile = profile_cumulus + (profile_strato - profile_cumulus) * flat_k;

    let cover = fbm3_lo(Vec3::new(p.x, p.y * 0.2, p.z) / 9500.0);
    let coarse = (cover - (1.0 - coverage - flat_k * 0.03) * 0.9) * 4.5;
    if profile <= 0.0 || coarse + 0.9 <= 0.0 {
        return 0.0;
    }
    let squash = 1.0 + 1.6 * flat_k;
    let scale = 1500.0 + (650.0 - 1500.0) * flat_k;
    let shape = fbm3_lo(Vec3::new(p.x, p.y * squash, p.z) / scale);
    let d = coarse + (shape - 0.5) * (1.8 + 0.9 * flat_k);
    let dc = d.clamp(0.0, 1.0);
    dc * dc * (3.0 - 2.0 * dc) * profile * (1.0 + (0.45 - 1.0) * flat_k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_is_zero_outside_the_cloud_layer_and_never_negative() {
        for i in 0..400 {
            let p = Vec3::new(i as f32 * 137.0 - 20_000.0, BASE - 50.0, i as f32 * 91.0);
            assert_eq!(density(p, Vec2::ZERO, 0.5), 0.0);
        }
        for i in 0..2000 {
            let p = Vec3::new(i as f32 * 53.0, BASE + (i % 17) as f32 * 100.0, i as f32 * 29.0);
            let d = density(p, Vec2::new(300.0, 80.0), 0.5);
            assert!((0.0..=1.0).contains(&d), "density {d}");
        }
    }

    #[test]
    fn more_coverage_means_more_cloud() {
        let total = |coverage: f32| -> f32 {
            (0..4000)
                .map(|i| density(Vec3::new(i as f32 * 211.0 % 30_000.0, 2_000.0, i as f32 * 97.0 % 30_000.0), Vec2::ZERO, coverage))
                .sum()
        };
        assert!(total(0.7) > total(0.4) && total(0.4) > total(0.15), "cloud should grow with coverage");
    }

    #[test]
    fn the_shadow_texture_has_clear_sky_and_shadow_and_is_deterministic() {
        // A sun 30 degrees above the horizon.
        let rotation = Quat::from_euler(EulerRot::YXZ, 2.0, -0.52, 0.0);
        let a = bake(rotation, Vec2::new(500.0, 200.0), 0.5);
        let b = bake(rotation, Vec2::new(500.0, 200.0), 0.5);
        assert_eq!(a, b);
        assert_eq!(a.len(), SIZE * SIZE * 4);
        let reds: Vec<u8> = a.chunks(4).map(|p| p[0]).collect();
        assert!(a.chunks(4).all(|p| p[0] == p[1] && p[1] == p[2]), "grey, so the sun isn't tinted");
        assert!(reds.iter().any(|&v| v > 240), "some sun should get through");
        assert!(reds.iter().any(|&v| v < 160), "some shadow should be cast");
        assert!(reds.iter().all(|&v| v as f32 >= SHADOW_FLOOR * 255.0 - 1.0), "never darker than the floor");
    }

    #[test]
    fn drifting_the_clouds_moves_the_shadows() {
        let rotation = Quat::from_euler(EulerRot::YXZ, 2.0, -0.52, 0.0);
        let before = bake(rotation, Vec2::ZERO, 0.5);
        let after = bake(rotation, Vec2::new(2_000.0, 0.0), 0.5);
        assert_ne!(before, after);
    }
}
