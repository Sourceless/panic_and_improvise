use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::gun_model::{self, BORE_Y, MUZZLE_Z, SIGHT_TOP};
use crate::player::{spawn_player, toggle_cursor_grab, AimBlend, FpsCamera};
pub use crate::gun_model::{front_sight, rear_sights};
use crate::map::TerrainMap;
use crate::target::{dummy_aabb, TargetDummy};

const FIRE_INTERVAL: f32 = 0.12;
const BULLET_SPEED: f32 = 300.0;
const BULLET_LIFETIME: f32 = 2.0;
const BULLET_DAMAGE: f32 = 25.0;

/// Where the gun sits in camera space when carried at the hip, and when aimed down its sights.
const HIP_POSITION: Vec3 = Vec3::new(0.2, -0.2, -0.5);
/// Centred, and low enough that the tops of the two sights land on the camera's axis.
const ADS_POSITION: Vec3 = Vec3::new(0.0, -SIGHT_TOP, -0.5);
/// The barrel's tip, in the gun's own space: the bullets start here.
pub const MUZZLE_LOCAL: Vec3 = Vec3::new(0.0, BORE_Y, MUZZLE_Z);
/// How fast the gun moves between hip and sights (per second, exponential).
const AIM_RATE: f32 = 14.0;
/// When aimed the view zooms to this fraction of the field of view, and look speed drops.
pub const ADS_FOV_SCALE: f32 = 0.72;
/// Recoil. Each shot kicks the view up (and a little sideways, at random), by this much in
/// radians, less when braced on the sights...
const KICK_PITCH: f32 = 0.0085;
const KICK_YAW: f32 = 0.0028;
const ADS_KICK_SCALE: f32 = 0.6;
/// ...and once the gun has been quiet this long, a good part of the climb comes back down:
/// this fraction of it, at this rate (per second). The rest is yours to pull down.
const KICK_RECOVERY_DELAY: f32 = 0.14;
const KICK_RECOVERY_FRACTION: f32 = 0.65;
const KICK_RECOVERY_RATE: f32 = 6.0;
/// The gun model itself jolts back and tips up with each shot, then settles quickly.
const GUN_KICK_DECAY: f32 = 16.0;
const GUN_KICK_BACK: f32 = 0.032;
const GUN_KICK_TIP: f32 = 0.045;
/// A shot is aimed at whatever is under the crosshair, or this far off if nothing is.
const FAR_AIM: f32 = 300.0;
/// ...but never closer than this, so a muzzle just past a wall can't flip the aim around.
const NEAR_AIM: f32 = 4.0;

pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_gun.after(spawn_player))
            .add_systems(
                Update,
                (aim.before(fire), fire.before(toggle_cursor_grab), recover_view, move_bullets),
            );
    }
}

#[derive(Component)]
pub struct Gun {
    cooldown: f32,
    trigger_blocked: bool,
    pub shots_fired: u32,
    /// Whether the player has toggled aiming down the sights.
    pub aiming: bool,
    /// Where the most recent bullet started, in world space.
    pub last_shot_origin: Option<Vec3>,
    /// 0 at the hip, 1 fully on the sights, easing between.
    pub aim_blend: f32,
    /// How far the view has been kicked up and across by recoil and not yet recovered (radians).
    pub view_kick: Vec2,
    /// Seconds since the last shot.
    pub since_shot: f32,
    /// The gun model's jolt, 1 right after a shot, decaying to 0.
    pub gun_kick: f32,
}

/// The gun's transform in camera space, `blend` of the way from the hip to the sights.
pub fn gun_transform(blend: f32) -> Transform {
    Transform::from_translation(HIP_POSITION.lerp(ADS_POSITION, blend.clamp(0.0, 1.0)))
}

/// The gun with its recoil jolt applied: shoved back toward the shoulder and tipped muzzle-up,
/// about half as much when braced on the sights.
pub fn gun_pose(blend: f32, kick: f32) -> Transform {
    let k = kick * (1.0 - 0.5 * blend.clamp(0.0, 1.0));
    let mut t = gun_transform(blend);
    t.translation.z += GUN_KICK_BACK * k;
    t.rotation = Quat::from_rotation_x(GUN_KICK_TIP * k);
    t
}

/// How much one shot kicks the view: (pitch up, yaw), in radians. The sideways part is a fixed
/// pseudo-random wander per shot number, so it is repeatable.
pub fn recoil_kick(shot: u32, aiming: bool) -> Vec2 {
    let scale = if aiming { ADS_KICK_SCALE } else { 1.0 };
    let mut h = shot.wrapping_mul(0x9E37_79B1) ^ 0x85EB_CA6B;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    let wander = ((h >> 8) as f32 / (1u32 << 24) as f32) * 2.0 - 1.0;
    // A little variation in the climb too.
    let climb = 0.85 + 0.3 * (((h >> 3) & 0xFF) as f32 / 255.0);
    Vec2::new(KICK_PITCH * climb, KICK_YAW * wander) * scale
}

/// How much of the unrecovered kick comes back in `dt` seconds, once the gun has been quiet.
/// Returns (the part of the kick to remove from the bookkeeping, the part to bring the view back by).
pub fn recover_kick(kick: Vec2, since_shot: f32, dt: f32) -> (Vec2, Vec2) {
    if since_shot < KICK_RECOVERY_DELAY {
        return (Vec2::ZERO, Vec2::ZERO);
    }
    let settled = kick * (1.0 - (-KICK_RECOVERY_RATE * dt).exp());
    (settled, settled * KICK_RECOVERY_FRACTION)
}

/// Which way a bullet leaves the muzzle: straight at the point the crosshair is on, `distance`
/// metres along the view. (The muzzle is off to one side, so firing parallel to the view
/// would land the shot to the side of where you aim.)
pub fn launch_direction(muzzle: Vec3, eye: Vec3, view_forward: Vec3, distance: f32) -> Vec3 {
    let target = eye + view_forward * distance.max(NEAR_AIM);
    (target - muzzle).normalize_or(view_forward)
}

/// How far along a ray the box is first entered, if it is.
pub fn ray_hits_aabb(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let (mut t_min, mut t_max) = (0.0f32, f32::MAX);
    for i in 0..3 {
        if dir[i].abs() < 1e-8 {
            if origin[i] < min[i] || origin[i] > max[i] {
                return None;
            }
        } else {
            let (a, b) = ((min[i] - origin[i]) / dir[i], (max[i] - origin[i]) / dir[i]);
            t_min = t_min.max(a.min(b));
            t_max = t_max.min(a.max(b));
            if t_min > t_max {
                return None;
            }
        }
    }
    Some(t_min)
}

#[derive(Component)]
pub struct Bullet {
    velocity: Vec3,
    age: f32,
}

#[derive(Resource)]
pub struct BulletAssets {
    pub mesh: Handle<Mesh>,
    pub material: Handle<StandardMaterial>,
    pub shot_sound: Handle<AudioSource>,
}

pub fn spawn_gun(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    cameras: Query<Entity, With<FpsCamera>>,
) {
    let Ok(cam) = cameras.single() else {
        return;
    };

    // The Sterling, merged into three meshes by material (see gun_model.rs).
    let (metal, plastic, dark) = gun_model::build();
    let finish = |color: Color, roughness: f32, metallic: f32| StandardMaterial {
        base_color: color,
        perceptual_roughness: roughness,
        metallic,
        ..default()
    };
    let parts = [
        (meshes.add(metal.into_mesh()), materials.add(finish(Color::srgb(0.13, 0.135, 0.145), 0.45, 0.75))),
        (meshes.add(plastic.into_mesh()), materials.add(finish(Color::srgb(0.035, 0.035, 0.04), 0.35, 0.0))),
        (meshes.add(dark.into_mesh()), materials.add(finish(Color::srgb(0.008, 0.008, 0.01), 0.9, 0.0))),
    ];

    commands.insert_resource(BulletAssets {
        mesh: meshes.add(Sphere::new(0.03)),
        material: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.85, 0.3),
            emissive: LinearRgba::rgb(12.0, 9.0, 2.0),
            ..default()
        }),
        shot_sound: asset_server.load("sounds/gunshots/pistol_shot.wav"),
    });

    commands.entity(cam).with_children(|cam_children| {
        cam_children
            .spawn((
                Gun {
                    cooldown: 0.0,
                    trigger_blocked: false,
                    shots_fired: 0,
                    aiming: false,
                    last_shot_origin: None,
                    aim_blend: 0.0,
                    view_kick: Vec2::ZERO,
                    since_shot: 10.0,
                    gun_kick: 0.0,
                },
                gun_transform(0.0),
                Visibility::default(),
            ))
            .with_children(|gun| {
                for (mesh, material) in parts {
                    gun.spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::default()));
                }
            });
    });
}

// Right click toggles aiming down the sights (only while the mouse is captured, so the click
// that recaptures it doesn't also raise the gun). Sprinting drops the gun back to the hip. The
// gun then eases toward its target, and the blend is shared (as AimBlend) so the camera can
// zoom and the player slow down in step with it.
fn aim(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    cursors: Query<&CursorOptions, With<PrimaryWindow>>,
    mut guns: Query<(&mut Gun, &mut Transform)>,
    mut blend: ResMut<AimBlend>,
) {
    let Ok((mut gun, mut transform)) = guns.single_mut() else {
        return;
    };
    let captured = cursors.single().is_ok_and(|c| c.grab_mode == CursorGrabMode::Locked);
    if captured && mouse.just_pressed(MouseButton::Right) {
        gun.aiming = !gun.aiming;
    }
    if keys.pressed(KeyCode::ShiftLeft) && keys.pressed(KeyCode::KeyW) {
        gun.aiming = false;
    }
    if !captured {
        gun.aiming = false;
    }
    let target = if gun.aiming { 1.0 } else { 0.0 };
    gun.aim_blend += (target - gun.aim_blend) * (1.0 - (-AIM_RATE * time.delta_secs()).exp());
    if (gun.aim_blend - target).abs() < 0.002 {
        gun.aim_blend = target;
    }
    gun.gun_kick *= (-GUN_KICK_DECAY * time.delta_secs()).exp();
    if gun.gun_kick < 0.002 {
        gun.gun_kick = 0.0;
    }
    *transform = gun_pose(gun.aim_blend, gun.gun_kick);
    blend.0 = gun.aim_blend;
}

fn fire(
    mut commands: Commands,
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    cursors: Query<&CursorOptions, With<PrimaryWindow>>,
    mut camera: Query<(&mut Transform, &mut FpsCamera)>,
    mut guns: Query<&mut Gun>,
    dummies: Query<&Transform, (With<TargetDummy>, Without<FpsCamera>)>,
    map: Res<TerrainMap>,
    assets: Res<BulletAssets>,
) {
    let Ok(mut gun) = guns.single_mut() else {
        return;
    };
    gun.cooldown = (gun.cooldown - time.delta_secs()).max(0.0);
    gun.since_shot += time.delta_secs();

    if !mouse.pressed(MouseButton::Left) {
        gun.trigger_blocked = false;
        return;
    }
    let Ok(cursor) = cursors.single() else {
        return;
    };
    if cursor.grab_mode != CursorGrabMode::Locked {
        // The click that recaptures the mouse must not fire.
        gun.trigger_blocked = true;
        return;
    }
    if gun.trigger_blocked || gun.cooldown > 0.0 {
        return;
    }
    let Ok((mut cam, mut view)) = camera.single_mut() else {
        return;
    };

    // The bullet leaves the end of the barrel, wherever the gun is right now (hip or sights).
    let gun_in_world = cam.mul_transform(gun_pose(gun.aim_blend, gun.gun_kick));
    let muzzle = gun_in_world.transform_point(MUZZLE_LOCAL);
    let (eye, forward) = (cam.translation, *cam.forward());
    let distance = what_is_under_crosshair(eye, forward, &dummies, &map);
    let direction = launch_direction(muzzle, eye, forward, distance);

    gun.cooldown = FIRE_INTERVAL;
    gun.shots_fired += 1;
    gun.last_shot_origin = Some(muzzle);
    gun.since_shot = 0.0;
    commands.spawn((AudioPlayer(assets.shot_sound.clone()), PlaybackSettings::DESPAWN));
    commands.spawn((
        Bullet {
            velocity: direction * BULLET_SPEED,
            age: 0.0,
        },
        Mesh3d(assets.mesh.clone()),
        MeshMaterial3d(assets.material.clone()),
        Transform::from_translation(muzzle),
    ));

    // Recoil: the shot has left; now the gun jolts and the view climbs.
    let kick = recoil_kick(gun.shots_fired, gun.aiming);
    gun.gun_kick = (gun.gun_kick + 1.0).min(1.6);
    gun.view_kick += kick;
    view.nudge(&mut cam, kick.x, kick.y);
}

// Once the gun has been quiet a moment, most of the climb settles back by itself, like the
// muzzle coming back down as the shooter recovers; what's left is for the player to pull down.
fn recover_view(time: Res<Time>, mut guns: Query<&mut Gun>, mut camera: Query<(&mut Transform, &mut FpsCamera)>) {
    let (Ok(mut gun), Ok((mut cam, mut view))) = (guns.single_mut(), camera.single_mut()) else {
        return;
    };
    let (settled, bring_back) = recover_kick(gun.view_kick, gun.since_shot, time.delta_secs());
    gun.view_kick -= settled;
    if bring_back != Vec2::ZERO {
        view.nudge(&mut cam, -bring_back.x, -bring_back.y);
    }
}

// How far along the view the first thing is: a target dummy or the ground, else FAR_AIM.
fn what_is_under_crosshair(eye: Vec3, forward: Vec3, dummies: &Query<&Transform, (With<TargetDummy>, Without<FpsCamera>)>, map: &TerrainMap) -> f32 {
    let mut nearest = FAR_AIM;
    for transform in dummies {
        let (min, max) = dummy_aabb(transform.translation);
        if let Some(t) = ray_hits_aabb(eye, forward, min, max) {
            nearest = nearest.min(t);
        }
    }
    // March the terrain in steps, finer than its 10 m cells.
    let mut t = 2.0;
    while t < nearest {
        let p = eye + forward * t;
        if p.y < map.height_at(Vec2::new(p.x, p.z)) {
            nearest = t;
            break;
        }
        t += 2.0;
    }
    nearest
}

fn move_bullets(
    mut commands: Commands,
    time: Res<Time>,
    mut bullets: Query<(Entity, &mut Transform, &mut Bullet)>,
    mut dummies: Query<(&Transform, &mut TargetDummy), Without<Bullet>>,
    map: Res<TerrainMap>,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut bullet) in &mut bullets {
        let start = transform.translation;
        let end = start + bullet.velocity * dt;
        bullet.age += dt;

        let mut hit = false;
        for (dummy_transform, mut dummy) in &mut dummies {
            if dummy.health <= 0.0 {
                continue;
            }
            let (min, max) = dummy_aabb(dummy_transform.translation);
            if segment_hits_aabb(start, end, min, max) {
                dummy.take_hit(BULLET_DAMAGE);
                hit = true;
                break;
            }
        }

        transform.translation = end;
        let ground = map.height_at(Vec2::new(end.x, end.z));
        if hit || bullet.age > BULLET_LIFETIME || end.y < ground {
            commands.entity(entity).despawn();
        }
    }
}

fn segment_hits_aabb(p0: Vec3, p1: Vec3, min: Vec3, max: Vec3) -> bool {
    let d = p1 - p0;
    let mut t_min = 0.0_f32;
    let mut t_max = 1.0_f32;
    for i in 0..3 {
        if d[i].abs() < f32::EPSILON {
            if p0[i] < min[i] || p0[i] > max[i] {
                return false;
            }
        } else {
            let inv = 1.0 / d[i];
            let mut t1 = (min[i] - p0[i]) * inv;
            let mut t2 = (max[i] - p0[i]) * inv;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            t_min = t_min.max(t1);
            t_max = t_max.min(t2);
            if t_min > t_max {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    // A point in the gun's space, as seen in the camera's space at a given aim blend.
    fn in_camera(blend: f32, gun_space: Vec3) -> Vec3 {
        gun_transform(blend).transform_point(gun_space)
    }

    fn top_of(sight: Transform) -> Vec3 {
        sight.transform_point(Vec3::new(0.0, 0.5, 0.0))
    }

    #[test]
    fn on_the_sights_both_sight_tops_sit_on_the_cameras_axis() {
        let front = in_camera(1.0, top_of(front_sight()));
        assert!(front.x.abs() < 1e-4 && front.y.abs() < 1e-4, "front post top at ({}, {}) in camera space", front.x, front.y);
        for upright in rear_sights() {
            let p = in_camera(1.0, top_of(upright));
            assert!(p.y.abs() < 1e-4, "rear upright top is {} off the line of sight", p.y);
            assert!(p.x.abs() < 0.02, "rear upright is {} to the side", p.x);
            // The rear sight is nearer the eye than the front, as it must be to line up.
            assert!(p.z > front.z);
        }
    }

    #[test]
    fn at_the_hip_the_gun_is_off_to_the_right_and_below() {
        let p = in_camera(0.0, MUZZLE_LOCAL);
        assert!(p.x > 0.1 && p.y < -0.1 && p.z < -0.3, "muzzle at {p:?}");
        let aimed = in_camera(1.0, MUZZLE_LOCAL);
        assert!(aimed.x.abs() < 1e-4 && aimed.y > -0.1, "aimed muzzle at {aimed:?}");
    }

    #[test]
    fn the_front_sight_sits_on_the_barrel_at_the_muzzle() {
        let front = front_sight();
        assert!((front.translation.z - MUZZLE_LOCAL.z).abs() < 0.03, "the post stands just behind the muzzle");
        // Its foot is on the top of the barrel jacket, which is 0.02 + 0.026 high.
        let foot = front.transform_point(Vec3::new(0.0, -0.5, 0.0)).y;
        assert!((0.04..0.05).contains(&foot), "foot at {foot}");
    }

    #[test]
    fn shots_converge_on_the_crosshair_from_the_off_centre_muzzle() {
        let (eye, forward) = (Vec3::new(10.0, 5.0, 3.0), Vec3::NEG_Z);
        let muzzle = eye + Vec3::new(0.2, -0.18, -0.5);
        for distance in [5.0, 20.0, 120.0] {
            let dir = launch_direction(muzzle, eye, forward, distance);
            let target = eye + forward * distance;
            // Flying along `dir` from the muzzle gets to the point under the crosshair.
            let along = (target - muzzle).dot(dir);
            let miss = (muzzle + dir * along - target).length();
            assert!(miss < 1e-3, "missed by {miss} at {distance} m");
        }
    }

    #[test]
    fn a_target_right_in_front_of_the_muzzle_cannot_flip_the_shot_backwards() {
        let (eye, forward) = (Vec3::ZERO, Vec3::NEG_Z);
        let muzzle = Vec3::new(0.2, -0.2, -1.0);
        let dir = launch_direction(muzzle, eye, forward, 0.1);
        assert!(dir.dot(forward) > 0.9, "{dir:?}");
    }

    #[test]
    fn rays_find_boxes_and_miss_them() {
        let (min, max) = (Vec3::new(-1.0, 0.0, -11.0), Vec3::new(1.0, 2.0, -9.0));
        assert_eq!(ray_hits_aabb(Vec3::new(0.0, 1.0, 0.0), Vec3::NEG_Z, min, max).map(|t| t.round()), Some(9.0));
        assert!(ray_hits_aabb(Vec3::new(5.0, 1.0, 0.0), Vec3::NEG_Z, min, max).is_none());
        assert!(ray_hits_aabb(Vec3::new(0.0, 1.0, 0.0), Vec3::Z, min, max).is_none(), "behind the ray");
    }

    #[test]
    fn every_shot_kicks_the_view_up_and_less_on_the_sights() {
        for shot in 1..200 {
            let hip = recoil_kick(shot, false);
            let ads = recoil_kick(shot, true);
            assert!(hip.x > 0.006 && hip.x < 0.012, "pitch kick {}", hip.x);
            assert!(hip.y.abs() <= KICK_YAW + 1e-6);
            assert!((ads.x - hip.x * ADS_KICK_SCALE).abs() < 1e-6 && ads.x < hip.x);
            assert_eq!(recoil_kick(shot, false), hip, "repeatable");
        }
        // Sideways wander goes both ways and averages out near nothing.
        let sum: f32 = (1..400).map(|s| recoil_kick(s, false).y).sum();
        assert!(sum.abs() < 0.2, "yaw drifts one way: {sum}");
        assert!((1..50).any(|s| recoil_kick(s, false).y > 0.0) && (1..50).any(|s| recoil_kick(s, false).y < 0.0));
    }

    #[test]
    fn the_climb_only_recovers_once_the_gun_has_gone_quiet_and_never_completely() {
        let kick = Vec2::new(0.05, 0.01);
        assert_eq!(recover_kick(kick, 0.05, 0.016), (Vec2::ZERO, Vec2::ZERO), "no recovery mid-burst");
        // Run recovery for a few seconds, as the weapon does.
        let (mut left, mut returned) = (kick, Vec2::ZERO);
        for _ in 0..600 {
            let (settled, back) = recover_kick(left, 1.0, 1.0 / 120.0);
            left -= settled;
            returned += back;
        }
        assert!(left.length() < 1e-3, "the bookkeeping settles to zero");
        let fraction = returned.x / kick.x;
        assert!((fraction - KICK_RECOVERY_FRACTION).abs() < 0.01, "{fraction} of the climb came back");
        assert!(kick.x - returned.x > 0.01, "some climb is left for the player to pull down");
    }

    #[test]
    fn the_gun_jolts_back_and_up_on_a_shot_and_less_on_the_sights() {
        let (rest, hip, ads) = (gun_pose(0.0, 0.0), gun_pose(0.0, 1.0), gun_pose(1.0, 1.0));
        assert_eq!(rest.translation, gun_transform(0.0).translation);
        assert!(hip.translation.z > rest.translation.z, "shoved back toward the shoulder");
        // Tipped muzzle-up: the muzzle (at -Z) rises.
        assert!(hip.transform_point(MUZZLE_LOCAL).y > rest.transform_point(MUZZLE_LOCAL).y);
        let hip_shove = hip.translation.z - rest.translation.z;
        let ads_shove = ads.translation.z - gun_transform(1.0).translation.z;
        assert!(ads_shove < hip_shove);
        // Sight tops stay close to the line of sight even in the jolt.
        let top = ads.transform_point(Vec3::new(0.0, SIGHT_TOP, MUZZLE_Z));
        assert!(top.y.abs() < 0.03, "sight top {} off the line of sight under recoil", top.y);
    }
}
