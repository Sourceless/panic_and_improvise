use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::player::{spawn_player, toggle_cursor_grab, AimBlend, FpsCamera};
use crate::map::TerrainMap;
use crate::target::{dummy_aabb, TargetDummy};

const FIRE_INTERVAL: f32 = 0.12;
const BULLET_SPEED: f32 = 300.0;
const BULLET_LIFETIME: f32 = 2.0;
const BULLET_DAMAGE: f32 = 25.0;

/// Where the gun sits in camera space when carried at the hip, and when aimed down its sights.
const HIP_POSITION: Vec3 = Vec3::new(0.2, -0.2, -0.5);
/// Centred, and low enough that the tops of the two sights land on the camera's axis.
const SIGHT_TOP: f32 = 0.08;
const ADS_POSITION: Vec3 = Vec3::new(0.0, -SIGHT_TOP, -0.5);
/// The barrel's tip, in the gun's own space: the bullets start here.
pub const MUZZLE_LOCAL: Vec3 = Vec3::new(0.0, 0.02, -0.5);
/// How fast the gun moves between hip and sights (per second, exponential).
const AIM_RATE: f32 = 14.0;
/// When aimed the view zooms to this fraction of the field of view, and look speed drops.
pub const ADS_FOV_SCALE: f32 = 0.72;
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
                (aim.before(fire), fire.before(toggle_cursor_grab), move_bullets),
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
}

/// The gun's transform in camera space, `blend` of the way from the hip to the sights.
pub fn gun_transform(blend: f32) -> Transform {
    Transform::from_translation(HIP_POSITION.lerp(ADS_POSITION, blend.clamp(0.0, 1.0)))
}

const SIGHT_SIZE: Vec3 = Vec3::new(0.012, 0.035, 0.012);

/// The two sights, in the gun's space. Each is placed so its top is at height `SIGHT_TOP`,
/// which is exactly how far the gun drops when aimed, so both tops end up on the camera axis.
pub fn front_sight() -> Transform {
    let scale = Vec3::new(0.7, 1.0, 0.7);
    Transform::from_xyz(0.0, SIGHT_TOP - SIGHT_SIZE.y * scale.y * 0.5, MUZZLE_LOCAL.z + 0.01).with_scale(scale)
}

pub fn rear_sight() -> Transform {
    let scale = Vec3::new(1.4, 0.85, 1.4);
    Transform::from_xyz(0.0, SIGHT_TOP - SIGHT_SIZE.y * scale.y * 0.5, -0.12).with_scale(scale)
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

    let body_mesh = meshes.add(Cuboid::new(0.1, 0.12, 0.4));
    let barrel_mesh = meshes.add(Cuboid::new(0.05, 0.05, 0.3));
    let sight_mesh = meshes.add(Cuboid::new(SIGHT_SIZE.x, SIGHT_SIZE.y, SIGHT_SIZE.z));
    let gun_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.12, 0.14),
        ..default()
    });

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
                },
                gun_transform(0.0),
                Visibility::default(),
            ))
            .with_children(|gun| {
                gun.spawn((
                    Mesh3d(body_mesh),
                    MeshMaterial3d(gun_material.clone()),
                    Transform::default(),
                ));
                gun.spawn((
                    Mesh3d(barrel_mesh),
                    MeshMaterial3d(gun_material.clone()),
                    Transform::from_xyz(0.0, 0.02, -0.35),
                ));
                // Iron sights: a post at the muzzle and a block over the breech, whose tops
                // line up with the camera's axis when the gun is on its sights.
                let (front, rear) = (front_sight(), rear_sight());
                gun.spawn((Mesh3d(sight_mesh.clone()), MeshMaterial3d(gun_material.clone()), front));
                gun.spawn((Mesh3d(sight_mesh), MeshMaterial3d(gun_material), rear));
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
    *transform = gun_transform(gun.aim_blend);
    blend.0 = gun.aim_blend;
}

fn fire(
    mut commands: Commands,
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    cursors: Query<&CursorOptions, With<PrimaryWindow>>,
    camera: Query<&Transform, With<FpsCamera>>,
    mut guns: Query<&mut Gun>,
    dummies: Query<&Transform, (With<TargetDummy>, Without<FpsCamera>)>,
    map: Res<TerrainMap>,
    assets: Res<BulletAssets>,
) {
    let Ok(mut gun) = guns.single_mut() else {
        return;
    };
    gun.cooldown = (gun.cooldown - time.delta_secs()).max(0.0);

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
    let Ok(cam) = camera.single() else {
        return;
    };

    // The bullet leaves the end of the barrel, wherever the gun is right now (hip or sights).
    let gun_in_world = cam.mul_transform(gun_transform(gun.aim_blend));
    let muzzle = gun_in_world.transform_point(MUZZLE_LOCAL);
    let (eye, forward) = (cam.translation, *cam.forward());
    let distance = what_is_under_crosshair(eye, forward, &dummies, &map);
    let direction = launch_direction(muzzle, eye, forward, distance);

    gun.cooldown = FIRE_INTERVAL;
    gun.shots_fired += 1;
    gun.last_shot_origin = Some(muzzle);
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
        sight.transform_point(Vec3::new(0.0, SIGHT_SIZE.y * 0.5, 0.0))
    }

    #[test]
    fn on_the_sights_both_sight_tops_sit_on_the_cameras_axis() {
        for sight in [front_sight(), rear_sight()] {
            let p = in_camera(1.0, top_of(sight));
            assert!(p.x.abs() < 1e-4 && p.y.abs() < 1e-4, "sight top at ({}, {}) in camera space", p.x, p.y);
        }
        // The rear sight is nearer the eye than the front, as it must be to line up.
        assert!(in_camera(1.0, top_of(rear_sight())).z > in_camera(1.0, top_of(front_sight())).z);
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
        assert!((front.translation.z - MUZZLE_LOCAL.z).abs() < 0.02);
        // Its foot is at (or just inside) the top of the barrel, which is 0.02 + 0.025 high.
        let foot = front.transform_point(Vec3::new(0.0, -SIGHT_SIZE.y * 0.5, 0.0)).y;
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
}
