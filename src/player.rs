use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow, WindowFocused};

use crate::map::TerrainMap;
use crate::MAP_HALF_SIZE;

const MOVE_SPEED: f32 = 6.0;
const SPRINT_MULTIPLIER: f32 = 1.8;
/// Metres per second squared. A bit stronger than real gravity, which feels snappier in a game.
const GRAVITY: f32 = 22.0;
/// Launch speed of a jump: with the gravity above, about 1.1 m high and 0.64 s in the air.
const JUMP_SPEED: f32 = 7.0;
/// How quickly (per second) the player's speed catches up with what the keys ask for, on the
/// ground and in the air. In the air momentum mostly carries on, so you can't turn on a dime.
const GROUND_RESPONSE: f32 = 14.0;
const AIR_RESPONSE: f32 = 1.6;
/// How much wider the view gets at full sprint, as a fraction of the field of view.
const SPRINT_FOV_KICK: f32 = 0.07;
const MOUSE_SENSITIVITY: f32 = 0.002;
/// Walking speed lost when fully on the sights, as a fraction.
const AIM_SLOWDOWN: f32 = 0.4;
/// How much the look sensitivity falls at full zoom, tracking the narrower field of view.
const ADS_LOOK_SCALE: f32 = 0.7;
const EYE_HEIGHT: f32 = 1.8;

/// How far the gun is raised to its sights, 0 (hip) to 1 (aimed). Written by the weapon, read by
/// the player (slower, steadier aim) and the camera (zoom).
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct AimBlend(pub f32);

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CursorIntent>()
            .init_resource::<AimBlend>()
            .add_systems(Startup, (spawn_player, grab_cursor))
            .add_systems(
                Update,
                (toggle_cursor_grab, regrab_on_focus, mouse_look, player_movement, sprint_fov).chain(),
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
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    /// Where the player is moving on the ground plane, metres per second (x, z).
    pub(crate) velocity: Vec2,
    /// Feet height above the ground, and the speed it is changing at (positive is up).
    pub(crate) air_height: f32,
    pub(crate) vertical_speed: f32,
}

impl FpsCamera {
    /// Turns the view up by `pitch` and left by `yaw` radians, as recoil does, keeping the
    /// camera's rotation in step.
    pub fn nudge(&mut self, transform: &mut Transform, pitch: f32, yaw: f32) {
        self.pitch = (self.pitch + pitch).clamp(-1.54, 1.54);
        self.yaw -= yaw;
        transform.rotation = Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0);
    }

    /// How fast the player is moving along the ground, metres per second.
    pub fn speed(&self) -> f32 {
        self.velocity.length()
    }

    /// Whether the player's feet are off the ground.
    pub fn airborne(&self) -> bool {
        self.air_height > 0.0
    }

    /// Where the view is pointing: up from level in radians.
    pub fn pitch(&self) -> f32 {
        self.pitch
    }
}

pub fn spawn_player(mut commands: Commands, map: Res<TerrainMap>) {
    let start = map.spawn_point();
    let eye = Vec3::new(start.x, map.height_at(start) + EYE_HEIGHT, start.y);
    commands.spawn((
        Camera3d::default(),
        Transform::from_translation(eye).looking_at(eye - Vec3::Z, Vec3::Y),
        FpsCamera { yaw: 0.0, pitch: 0.0, velocity: Vec2::ZERO, air_height: 0.0, vertical_speed: 0.0 },
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
    aim: Res<AimBlend>,
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
    let sensitivity = MOUSE_SENSITIVITY * (1.0 - (1.0 - ADS_LOOK_SCALE) * aim.0);
    cam.yaw -= delta.x * sensitivity;
    cam.pitch -= delta.y * sensitivity;
    cam.pitch = cam.pitch.clamp(-1.54, 1.54);

    transform.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
}

/// What the player is asking for this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct MoveIntent {
    /// Where the movement keys point, in the ground plane (unit length, or zero for none).
    pub direction: Vec2,
    pub sprint: bool,
    pub jump: bool,
    /// How far the gun is on its sights, 0 to 1: aiming is slower than walking.
    pub aim: f32,
}

/// The player's movement state, apart from where they are.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MoveState {
    pub velocity: Vec2,
    pub air_height: f32,
    pub vertical_speed: f32,
}

impl MoveState {
    pub fn grounded(&self) -> bool {
        self.air_height <= 0.0 && self.vertical_speed <= 0.0
    }

    /// 0 when walking or standing, 1 at full sprint speed.
    pub fn sprint_fraction(&self) -> f32 {
        ((self.velocity.length() - MOVE_SPEED) / (MOVE_SPEED * (SPRINT_MULTIPLIER - 1.0))).clamp(0.0, 1.0)
    }
}

/// Advances the player by one frame of `dt` seconds. Pure, so it can be tested without a game.
pub fn step_movement(mut state: MoveState, intent: MoveIntent, dt: f32) -> MoveState {
    let grounded = state.grounded();
    // Sprinting needs somewhere to go: it only applies while moving.
    let sprinting = intent.sprint && intent.direction != Vec2::ZERO;
    let speed = if sprinting { MOVE_SPEED * SPRINT_MULTIPLIER } else { MOVE_SPEED * (1.0 - AIM_SLOWDOWN * intent.aim) };
    let target = intent.direction * speed;
    let response = if grounded { GROUND_RESPONSE } else { AIR_RESPONSE };
    state.velocity = state.velocity.lerp(target, 1.0 - (-response * dt).exp());
    // Settle fully rather than creeping toward zero forever.
    if intent.direction == Vec2::ZERO && state.velocity.length() < 0.02 {
        state.velocity = Vec2::ZERO;
    }

    if grounded && intent.jump {
        state.vertical_speed = JUMP_SPEED;
    }
    if !grounded || state.vertical_speed > 0.0 {
        state.vertical_speed -= GRAVITY * dt;
        state.air_height += state.vertical_speed * dt;
        if state.air_height <= 0.0 {
            state.air_height = 0.0;
            state.vertical_speed = 0.0;
        }
    }
    state
}

fn player_movement(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    terrain: Res<TerrainMap>,
    aim: Res<AimBlend>,
    mut query: Query<(&mut Transform, &mut FpsCamera)>,
) {
    let Ok((mut transform, mut cam)) = query.single_mut() else {
        return;
    };

    let forward = Vec2::new(-cam.yaw.sin(), -cam.yaw.cos());
    let right = Vec2::new(cam.yaw.cos(), -cam.yaw.sin());

    let mut direction = Vec2::ZERO;
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
    let intent = MoveIntent {
        direction: direction.normalize_or_zero(),
        // Sprinting is for running forward, not backpedalling.
        sprint: keys.pressed(KeyCode::ShiftLeft) && keys.pressed(KeyCode::KeyW) && !keys.pressed(KeyCode::KeyS),
        jump: keys.pressed(KeyCode::Space),
        aim: aim.0,
    };
    let state = step_movement(
        MoveState { velocity: cam.velocity, air_height: cam.air_height, vertical_speed: cam.vertical_speed },
        intent,
        time.delta_secs(),
    );
    cam.velocity = state.velocity;
    cam.air_height = state.air_height;
    cam.vertical_speed = state.vertical_speed;

    let mut pos = Vec2::new(transform.translation.x, transform.translation.z) + state.velocity * time.delta_secs();
    pos = pos.clamp(Vec2::splat(-MAP_HALF_SIZE), Vec2::splat(MAP_HALF_SIZE));
    transform.translation.x = pos.x;
    transform.translation.z = pos.y;

    let ground = terrain.height_at(pos);
    transform.translation.y = ground + cam.air_height + EYE_HEIGHT;
}

/// Widens the view a little while sprinting, which makes speed read on screen.
fn sprint_fov(time: Res<Time>, aim: Res<AimBlend>, mut cameras: Query<(&FpsCamera, &mut Projection)>, mut base: Local<Option<f32>>) {
    for (cam, mut projection) in &mut cameras {
        let Projection::Perspective(p) = &mut *projection else { continue };
        let base_fov = *base.get_or_insert(p.fov);
        let sprint = MoveState { velocity: cam.velocity, ..default() }.sprint_fraction();
        // Sprinting widens the view; aiming down the sights narrows it.
        let zoom = 1.0 + (crate::weapon::ADS_FOV_SCALE - 1.0) * aim.0;
        let target = base_fov * (1.0 + SPRINT_FOV_KICK * sprint) * zoom;
        p.fov += (target - p.fov) * (1.0 - (-8.0 * time.delta_secs()).exp());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 120.0;

    fn run(mut state: MoveState, intent: MoveIntent, seconds: f32) -> MoveState {
        for _ in 0..(seconds / DT) as usize {
            state = step_movement(state, intent, DT);
        }
        state
    }

    fn forward() -> MoveIntent {
        MoveIntent { direction: Vec2::X, ..default() }
    }

    #[test]
    fn standing_still_stays_still() {
        let s = run(MoveState::default(), MoveIntent::default(), 1.0);
        assert_eq!(s, MoveState::default());
    }

    #[test]
    fn walking_reaches_walking_speed_and_sprinting_reaches_sprint_speed() {
        let walk = run(MoveState::default(), forward(), 1.0);
        assert!((walk.velocity.length() - MOVE_SPEED).abs() < 0.05);
        let sprint = run(MoveState::default(), MoveIntent { sprint: true, ..forward() }, 1.0);
        assert!((sprint.velocity.length() - MOVE_SPEED * SPRINT_MULTIPLIER).abs() < 0.05);
        assert!(sprint.sprint_fraction() > 0.99 && walk.sprint_fraction() < 0.01);
    }

    #[test]
    fn sprinting_without_moving_does_nothing() {
        let s = run(MoveState::default(), MoveIntent { sprint: true, ..default() }, 1.0);
        assert_eq!(s.velocity, Vec2::ZERO);
    }

    #[test]
    fn a_jump_goes_up_about_a_metre_and_comes_back_down() {
        let mut s = step_movement(MoveState::default(), MoveIntent { jump: true, ..default() }, DT);
        let (mut apex, mut frames) = (0.0f32, 0);
        while !s.grounded() && frames < 2000 {
            s = step_movement(s, MoveIntent::default(), DT);
            apex = apex.max(s.air_height);
            frames += 1;
        }
        assert!(s.grounded(), "must land");
        assert!((0.95..1.25).contains(&apex), "apex {apex}");
        let airtime = frames as f32 * DT;
        assert!((0.55..0.75).contains(&airtime), "airtime {airtime}");
    }

    #[test]
    fn you_cannot_jump_again_in_the_air() {
        let mut s = step_movement(MoveState::default(), MoveIntent { jump: true, ..default() }, DT);
        s = run(s, MoveIntent::default(), 0.2);
        let before = s.vertical_speed;
        let after = step_movement(s, MoveIntent { jump: true, ..default() }, DT);
        assert!(after.vertical_speed < before, "still falling, no second boost");
    }

    #[test]
    fn holding_jump_hops_again_on_landing() {
        let hold = MoveIntent { jump: true, ..default() };
        let mut s = MoveState::default();
        let mut landings = 0;
        let mut was_air = false;
        for _ in 0..(3.0 / DT) as usize {
            s = step_movement(s, hold, DT);
            if !s.grounded() {
                was_air = true;
            } else if was_air {
                landings += 1;
                was_air = false;
            }
        }
        assert!(landings >= 3, "{landings} landings in 3 s");
    }

    #[test]
    fn momentum_carries_through_the_air_but_you_can_steer_a_little() {
        let sprint = MoveIntent { sprint: true, ..forward() };
        let mut s = run(MoveState::default(), sprint, 1.0);
        s = step_movement(s, MoveIntent { jump: true, ..sprint }, DT);
        // Let go of everything mid-air: most of the speed is still there a quarter second on.
        let drifting = run(s, MoveIntent::default(), 0.25);
        assert!(drifting.velocity.length() > MOVE_SPEED * SPRINT_MULTIPLIER * 0.55, "{:?}", drifting.velocity);
        // And on the ground it stops almost at once.
        let grounded = run(run(MoveState::default(), sprint, 1.0), MoveIntent::default(), 0.5);
        assert!(grounded.velocity.length() < 0.1);
    }

    // The same thing through the real system: keys, terrain and camera wired together.
    fn app_with_player(ground: f32) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TerrainMap::flat(ground))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<AimBlend>()
            .add_systems(Update, player_movement);
        app.world_mut().spawn((
            Transform::from_xyz(0.0, ground + EYE_HEIGHT, 0.0),
            FpsCamera { yaw: 0.0, pitch: 0.0, velocity: Vec2::ZERO, air_height: 0.0, vertical_speed: 0.0 },
        ));
        // Every update advances the clock by exactly 1/60 s, whatever the real time taken.
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(std::time::Duration::from_secs_f32(1.0 / 60.0)));
        app.update(); // the first update only sets the clock's starting point
        app
    }

    fn frame(app: &mut App, _seconds: f32) {
        app.update();
    }

    fn eye_height(app: &mut App) -> f32 {
        app.world_mut().query::<&Transform>().iter(app.world()).next().unwrap().translation.y
    }

    #[test]
    fn pressing_space_in_the_game_lifts_the_camera_and_it_lands_again() {
        let mut app = app_with_player(10.0);
        let standing = 10.0 + EYE_HEIGHT;
        frame(&mut app, 1.0 / 60.0);
        assert!((eye_height(&mut app) - standing).abs() < 1e-3);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Space);
        let mut peak = 0.0f32;
        for _ in 0..30 {
            frame(&mut app, 1.0 / 60.0);
            peak = peak.max(eye_height(&mut app) - standing);
        }
        assert!(peak > 0.8, "peak {peak}");
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::Space);
        for _ in 0..90 {
            frame(&mut app, 1.0 / 60.0);
        }
        assert!((eye_height(&mut app) - standing).abs() < 1e-3, "back on the ground");
    }

    #[test]
    fn shift_w_covers_more_ground_than_w_alone() {
        let travelled = |sprint: bool| {
            let mut app = app_with_player(0.0);
            {
                let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keys.press(KeyCode::KeyW);
                if sprint {
                    keys.press(KeyCode::ShiftLeft);
                }
            }
            for _ in 0..120 {
                frame(&mut app, 1.0 / 60.0);
            }
            app.world_mut().query::<&Transform>().iter(app.world()).next().unwrap().translation.z.abs()
        };
        let (walk, run) = (travelled(false), travelled(true));
        // Two seconds from standing: about 12 m walking (less the ramp-up), about 21 m sprinting.
        assert!((10.0..12.5).contains(&walk), "walked {walk} m");
        assert!((18.0..22.0).contains(&run), "sprinted {run} m");
    }

    #[test]
    fn aiming_is_slower_than_walking_and_sprinting_ignores_it() {
        let walk = run(MoveState::default(), forward(), 1.0).velocity.length();
        let aimed = run(MoveState::default(), MoveIntent { aim: 1.0, ..forward() }, 1.0).velocity.length();
        assert!((aimed - walk * (1.0 - AIM_SLOWDOWN)).abs() < 0.1, "walk {walk}, aimed {aimed}");
        let sprint = run(MoveState::default(), MoveIntent { sprint: true, aim: 1.0, ..forward() }, 1.0).velocity.length();
        assert!(sprint > walk * 1.7, "sprinting is not slowed by aim: {sprint}");
    }
}
