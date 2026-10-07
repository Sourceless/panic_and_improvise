use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow, WindowFocused};

use crate::map::TerrainMap;
use crate::MAP_HALF_SIZE;

const MOVE_SPEED: f32 = 6.0;
const SPRINT_MULTIPLIER: f32 = 1.8;
const MOUSE_SENSITIVITY: f32 = 0.002;
const EYE_HEIGHT: f32 = 1.8;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CursorIntent>()
            .add_systems(Startup, (spawn_player, grab_cursor))
            .add_systems(
                Update,
                (toggle_cursor_grab, regrab_on_focus, mouse_look, player_movement),
            );
    }
}

// The OS can refuse a grab and winit reverts CursorOptions, so the player's wish is kept here.
#[derive(Resource, Default)]
pub struct CursorIntent {
    pub captured: bool,
}

#[derive(Component)]
pub struct FpsCamera {
    yaw: f32,
    pitch: f32,
}

pub fn spawn_player(mut commands: Commands, map: Res<TerrainMap>) {
    let start = map.spawn_point();
    let eye = Vec3::new(start.x, map.height_at(start) + EYE_HEIGHT, start.y);
    commands.spawn((
        Camera3d::default(),
        Transform::from_translation(eye).looking_at(eye - Vec3::Z, Vec3::Y),
        FpsCamera { yaw: 0.0, pitch: 0.0 },
    ));
}

fn grab_cursor(
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut intent: ResMut<CursorIntent>,
) {
    intent.captured = true;
    let Ok(mut cursor) = cursors.single_mut() else {
        return;
    };
    cursor.grab_mode = CursorGrabMode::Locked;
    cursor.visible = false;
}

fn regrab_on_focus(
    mut focus_events: MessageReader<WindowFocused>,
    intent: Res<CursorIntent>,
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let regained = focus_events.read().any(|event| event.focused);
    if !regained || !intent.captured {
        return;
    }
    let Ok(mut cursor) = cursors.single_mut() else {
        return;
    };
    cursor.grab_mode = CursorGrabMode::Locked;
    cursor.visible = false;
}

pub fn toggle_cursor_grab(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut intent: ResMut<CursorIntent>,
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let Ok(mut cursor) = cursors.single_mut() else {
        return;
    };
    if keys.just_pressed(KeyCode::Escape) {
        intent.captured = false;
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    } else if mouse.just_pressed(MouseButton::Left) {
        intent.captured = true;
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    }
}

fn mouse_look(
    cursors: Query<&CursorOptions, With<PrimaryWindow>>,
    mut mouse_motion: MessageReader<MouseMotion>,
    mut query: Query<(&mut Transform, &mut FpsCamera)>,
) {
    let Ok(cursor) = cursors.single() else {
        return;
    };
    if cursor.grab_mode == CursorGrabMode::None {
        mouse_motion.clear();
        return;
    }

    let mut delta = Vec2::ZERO;
    for motion in mouse_motion.read() {
        delta += motion.delta;
    }
    if delta == Vec2::ZERO {
        return;
    }

    let Ok((mut transform, mut cam)) = query.single_mut() else {
        return;
    };
    cam.yaw -= delta.x * MOUSE_SENSITIVITY;
    cam.pitch -= delta.y * MOUSE_SENSITIVITY;
    cam.pitch = cam.pitch.clamp(-1.54, 1.54);

    transform.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
}

fn player_movement(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    terrain: Res<TerrainMap>,
    mut query: Query<(&mut Transform, &FpsCamera)>,
) {
    let Ok((mut transform, cam)) = query.single_mut() else {
        return;
    };

    let forward = Vec3::new(-cam.yaw.sin(), 0.0, -cam.yaw.cos());
    let right = Vec3::new(cam.yaw.cos(), 0.0, -cam.yaw.sin());

    let mut direction = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        direction += forward;
    }
    if keys.pressed(KeyCode::KeyS) {
        direction -= forward;
    }
    if keys.pressed(KeyCode::KeyD) {
        direction += right;
    }
    if keys.pressed(KeyCode::KeyA) {
        direction -= right;
    }

    if direction.length_squared() > 0.0 {
        direction = direction.normalize();
        let speed = if keys.pressed(KeyCode::ShiftLeft) {
            MOVE_SPEED * SPRINT_MULTIPLIER
        } else {
            MOVE_SPEED
        };
        let mut new_pos = transform.translation + direction * speed * time.delta_secs();
        new_pos.x = new_pos.x.clamp(-MAP_HALF_SIZE, MAP_HALF_SIZE);
        new_pos.z = new_pos.z.clamp(-MAP_HALF_SIZE, MAP_HALF_SIZE);
        transform.translation.x = new_pos.x;
        transform.translation.z = new_pos.z;
    }

    let ground = terrain.height_at(Vec2::new(transform.translation.x, transform.translation.z));
    transform.translation.y = ground + EYE_HEIGHT;
}
