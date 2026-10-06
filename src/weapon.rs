use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::player::{spawn_player, toggle_cursor_grab, FpsCamera};
use crate::map::TerrainMap;
use crate::target::{dummy_aabb, TargetDummy};

const FIRE_INTERVAL: f32 = 0.12;
const BULLET_SPEED: f32 = 300.0;
const BULLET_LIFETIME: f32 = 2.0;
const BULLET_DAMAGE: f32 = 25.0;
// Muzzle position in camera space.
const MUZZLE_OFFSET: Vec3 = Vec3::new(0.2, -0.18, -1.0);

pub struct WeaponPlugin;

impl Plugin for WeaponPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_gun.after(spawn_player))
            .add_systems(Update, (fire.before(toggle_cursor_grab), move_bullets));
    }
}

#[derive(Component)]
pub struct Gun {
    cooldown: f32,
    trigger_blocked: bool,
    pub shots_fired: u32,
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
                },
                Transform::from_xyz(0.2, -0.2, -0.5),
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
                    MeshMaterial3d(gun_material),
                    Transform::from_xyz(0.0, 0.02, -0.35),
                ));
            });
    });
}

fn fire(
    mut commands: Commands,
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    cursors: Query<&CursorOptions, With<PrimaryWindow>>,
    camera: Query<&Transform, With<FpsCamera>>,
    mut guns: Query<&mut Gun>,
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

    gun.cooldown = FIRE_INTERVAL;
    gun.shots_fired += 1;
    commands.spawn((AudioPlayer(assets.shot_sound.clone()), PlaybackSettings::DESPAWN));
    commands.spawn((
        Bullet {
            velocity: *cam.forward() * BULLET_SPEED,
            age: 0.0,
        },
        Mesh3d(assets.mesh.clone()),
        MeshMaterial3d(assets.material.clone()),
        Transform::from_translation(cam.transform_point(MUZZLE_OFFSET)),
    ));
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
