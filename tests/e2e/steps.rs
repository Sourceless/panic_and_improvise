use std::time::{Duration, Instant};

use bevy::prelude::MouseButton;
use cucumber::{given, then, when};

use crate::{send, snapshot, Command};

const PAUSE_ENV: &str = "E2E_STEP_DELAY_MS";

#[derive(Debug, Default, cucumber::World)]
pub struct GameWorld;

fn step_pause() {
    let headed = std::env::var("E2E_HEADED").is_ok();
    let default_ms = if headed { 400 } else { 0 };
    let ms = std::env::var(PAUSE_ENV)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default_ms);
    std::thread::sleep(Duration::from_millis(ms));
}

fn wait_for(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    condition()
}

fn hold_fire(duration: Duration) {
    send(Command::Press(MouseButton::Left));
    std::thread::sleep(duration);
    send(Command::Release(MouseButton::Left));
}

fn tap_fire() {
    send(Command::Tap(MouseButton::Left));
}

#[given(expr = "I am in test room {int}")]
fn in_test_room(_world: &mut GameWorld, _room: u32) {
    // Stub: there is only one room, so this resets the room state.
    send(Command::LoadRoom);
    step_pause();
}

#[given("I have the smg")]
fn have_the_smg(_world: &mut GameWorld) {
    // Stub: the pistol is the only weapon for now.
    step_pause();
}

#[given("the shot sound is loaded")]
fn shot_sound_loaded(_world: &mut GameWorld) {
    let loaded = wait_for(Duration::from_secs(5), || snapshot().shot_sound_loaded);
    assert!(loaded, "shot sound did not finish loading");
}

#[given("the cursor is released")]
#[when("the cursor is released")]
fn cursor_released(_world: &mut GameWorld) {
    send(Command::SetCursorCaptured(false));
    step_pause();
}

#[given("I right click")]
#[when("I right click")]
fn right_click(_world: &mut GameWorld) {
    step_pause();
    send(Command::Tap(MouseButton::Right));
}

#[given("the gun is on its sights")]
#[then("the gun is on its sights")]
fn gun_on_sights(_world: &mut GameWorld) {
    let raised = wait_for(Duration::from_secs(2), || snapshot().aim_blend > 0.99);
    let state = snapshot();
    assert!(raised && state.aiming, "gun not on its sights (aiming {}, blend {})", state.aiming, state.aim_blend);
}

#[then("the gun is back at the hip")]
fn gun_at_hip(_world: &mut GameWorld) {
    let lowered = wait_for(Duration::from_secs(2), || snapshot().aim_blend < 0.01);
    let state = snapshot();
    assert!(lowered && !state.aiming, "gun not at the hip (aiming {}, blend {})", state.aiming, state.aim_blend);
}

#[then("the shot started to the right of and below the view and ahead of the camera")]
fn shot_from_hip(_world: &mut GameWorld) {
    wait_for(Duration::from_secs(2), || snapshot().last_shot_origin.is_some());
    let (right, up, ahead) = snapshot().last_shot_origin.expect("no shot was fired");
    assert!(right > 0.15 && up < -0.1 && ahead > 0.4, "muzzle at right {right}, up {up}, ahead {ahead}");
}

#[then("the view has kicked upward")]
fn view_kicked(_world: &mut GameWorld) {
    let kicked = wait_for(Duration::from_secs(2), || snapshot().camera_pitch > 0.003);
    assert!(kicked, "view pitch is {}, expected recoil to lift it", snapshot().camera_pitch);
}

#[then("the view has climbed noticeably")]
fn view_climbed(_world: &mut GameWorld) {
    step_pause();
    let pitch = snapshot().camera_pitch;
    assert!(pitch > 0.02, "after a burst the view should be well up, but pitch is {pitch}");
}

#[then("the view has come most of the way back down")]
fn view_settled(_world: &mut GameWorld) {
    // After firing stops, about two thirds of the climb returns by itself.
    let peak = snapshot().camera_pitch;
    std::thread::sleep(Duration::from_secs(2));
    let settled = snapshot().camera_pitch;
    assert!(settled < peak * 0.6, "view went from {peak} to {settled}, expected it to settle");
    assert!(settled > 0.0, "but not all the way back");
}

#[then("the shot started in line with the view")]
fn shot_from_sights(_world: &mut GameWorld) {
    wait_for(Duration::from_secs(2), || snapshot().last_shot_origin.is_some());
    let (right, up, ahead) = snapshot().last_shot_origin.expect("no shot was fired");
    assert!(right.abs() < 0.01 && up.abs() < 0.12 && ahead > 0.4, "muzzle at right {right}, up {up}, ahead {ahead}");
}

#[when("I fire once")]
fn fire_once(_world: &mut GameWorld) {
    step_pause();
    tap_fire();
}

#[when(regex = r"^I hold fire for ([\d.]+) seconds?$")]
fn hold_fire_for(_world: &mut GameWorld, seconds: f32) {
    step_pause();
    hold_fire(Duration::from_secs_f32(seconds));
}

// With recoil a held burst climbs off the target (as it should), and a test can't pull the
// view back down, so the dummy is taken down the way a careful shooter would: one shot at a
// time, giving the view a moment to settle between them.
#[when("I fire single shots until the target dummy is down")]
fn fire_single_shots_until_down(_world: &mut GameWorld) {
    step_pause();
    let mut shots = 0;
    while snapshot().dummy_health > 0.0 && shots < 30 {
        tap_fire();
        shots += 1;
        std::thread::sleep(Duration::from_millis(450));
    }
    assert!(snapshot().dummy_health <= 0.0, "target dummy was not destroyed with {shots} single shots");
}

#[then("the shot sound plays on the audio device")]
fn shot_sound_plays(_world: &mut GameWorld) {
    let played = wait_for(Duration::from_secs(2), || snapshot().playing_sounds > 0);
    let state = snapshot();
    assert!(
        played,
        "no audio sink was playing after firing (shots fired: {}, cursor captured: {}) — check the audio output device",
        state.shots_fired, state.cursor_captured
    );
}

#[then("no shot is fired")]
fn no_shot_fired(_world: &mut GameWorld) {
    step_pause();
    assert_eq!(snapshot().shots_fired, 0);
}

#[then(regex = r"^the gun fired between (\d+) and (\d+) shots$")]
fn gun_fired_between(_world: &mut GameWorld, min: u32, max: u32) {
    step_pause();
    let shots = snapshot().shots_fired;
    assert!(
        (min..=max).contains(&shots),
        "expected {min}-{max} shots, got {shots}"
    );
}

#[then(regex = r"^the target dummy has taken (\d+) damage$")]
fn dummy_took_damage(_world: &mut GameWorld, damage: f32) {
    step_pause();
    let wait_for_hit = wait_for(Duration::from_secs(2), || snapshot().dummy_hits > 0);
    assert!(wait_for_hit, "no bullet hit the target dummy");
    let health = snapshot().dummy_health;
    assert_eq!(health, 100.0 - damage);
}

#[then(regex = r"^the target dummy respawns within (\d+) seconds$")]
fn dummy_respawns(_world: &mut GameWorld, seconds: u64) {
    step_pause();
    let respawned = wait_for(Duration::from_secs(seconds), || snapshot().dummy_health >= 100.0);
    assert!(respawned, "target dummy did not respawn within {seconds} seconds");
}
