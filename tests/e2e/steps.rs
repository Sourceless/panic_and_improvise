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
fn cursor_released(_world: &mut GameWorld) {
    send(Command::SetCursorCaptured(false));
    step_pause();
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

#[when("I hold fire until the target dummy is down")]
fn hold_fire_until_down(_world: &mut GameWorld) {
    step_pause();
    send(Command::Press(MouseButton::Left));
    let down = wait_for(Duration::from_secs(10), || snapshot().dummy_health <= 0.0);
    send(Command::Release(MouseButton::Left));
    assert!(down, "target dummy was not destroyed within 10 seconds");
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
