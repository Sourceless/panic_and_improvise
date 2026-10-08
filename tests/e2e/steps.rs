use std::time::{Duration, Instant};

use bevy::prelude::{KeyCode, MouseButton};
use fps_prototype::player::Stance;
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
    // Game time runs slower than real time if the machine is busy, so allow plenty.
    let raised = wait_for(Duration::from_secs(8), || snapshot().aim_blend > 0.99);
    let state = snapshot();
    assert!(raised && state.aiming, "gun not on its sights (aiming {}, blend {})", state.aiming, state.aim_blend);
}

#[then("the gun is back at the hip")]
fn gun_at_hip(_world: &mut GameWorld) {
    let lowered = wait_for(Duration::from_secs(8), || snapshot().aim_blend < 0.01);
    let state = snapshot();
    assert!(lowered && !state.aiming, "gun not at the hip (aiming {}, blend {})", state.aiming, state.aim_blend);
}

#[then("the shot started to the right of and below the view and ahead of the camera")]
fn shot_from_hip(_world: &mut GameWorld) {
    wait_for(Duration::from_secs(2), || snapshot().last_shot_origin.is_some());
    let (right, up, ahead) = snapshot().last_shot_origin.expect("no shot was fired");
    assert!(right > 0.15 && up < -0.1 && ahead > 0.4, "muzzle at right {right}, up {up}, ahead {ahead}");
}

#[when(regex = r"^I fire (\d+) single shots?$")]
fn fire_n_single_shots(_world: &mut GameWorld, count: u32) {
    step_pause();
    for _ in 0..count {
        tap_fire();
        // Long enough between shots for the burst penalty to fade and the view to settle.
        std::thread::sleep(Duration::from_millis(700));
    }
}

#[then(regex = r"^the worst shot strayed more than ([\d.]+) degrees$")]
fn worst_shot_more_than(_world: &mut GameWorld, degrees: f32) {
    let worst = snapshot().worst_shot_error.to_degrees();
    assert!(worst > degrees, "the worst of the shots was only {worst:.2} degrees off");
}

#[then(regex = r"^no shot strayed more than ([\d.]+) degrees$")]
fn no_shot_more_than(_world: &mut GameWorld, degrees: f32) {
    let worst = snapshot().worst_shot_error.to_degrees();
    assert!(worst < degrees, "a shot strayed {worst:.2} degrees");
    assert!(snapshot().shots_fired > 0, "no shots were fired");
}

#[then("the muzzle flash shows")]
fn flash_shows(_world: &mut GameWorld) {
    let seen = wait_for(Duration::from_millis(400), || snapshot().flash_visible);
    assert!(seen, "no muzzle flash appeared after the shot");
}

#[then("the muzzle flash is gone")]
fn flash_gone(_world: &mut GameWorld) {
    let gone = wait_for(Duration::from_secs(3), || !snapshot().flash_visible);
    assert!(gone, "the muzzle flash stayed on");
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

fn key_named(name: &str) -> KeyCode {
    match name {
        "C" => KeyCode::KeyC,
        "Z" => KeyCode::KeyZ,
        "W" => KeyCode::KeyW,
        "Space" => KeyCode::Space,
        "Shift" => KeyCode::ShiftLeft,
        other => panic!("no key called {other}"),
    }
}

#[given(regex = r"^I press ([A-Za-z]+)$")]
#[when(regex = r"^I press ([A-Za-z]+)$")]
fn press_key(_world: &mut GameWorld, name: String) {
    step_pause();
    send(Command::TapKey(key_named(&name)));
    // Let the game see the press before the next step.
    std::thread::sleep(Duration::from_millis(100));
}

#[given(regex = r"^I hold ([A-Za-z]+)$")]
#[when(regex = r"^I hold ([A-Za-z]+)$")]
fn hold_key(_world: &mut GameWorld, name: String) {
    step_pause();
    send(Command::PressKey(key_named(&name)));
}

#[when(regex = r"^I release ([A-Za-z]+)$")]
fn release_key(_world: &mut GameWorld, name: String) {
    send(Command::ReleaseKey(key_named(&name)));
}

#[given("the target dummy is out of the way")]
fn dummy_out_of_the_way(_world: &mut GameWorld) {
    send(Command::RemoveDummies);
    step_pause();
}

#[given(regex = r"^I am (standing|crouching|prone)$")]
#[then(regex = r"^I am (standing|crouching|prone)$")]
fn i_am_in_stance(_world: &mut GameWorld, name: String) {
    let want = match name.as_str() {
        "standing" => Stance::Stand,
        "crouching" => Stance::Crouch,
        _ => Stance::Prone,
    };
    let reached = wait_for(Duration::from_secs(8), || snapshot().stance == want);
    assert!(reached, "wanted to be {want:?} but am {:?}", snapshot().stance);
}

#[then(regex = r"^my eyes are (?:below|under) ([\d.]+) metres$")]
fn eyes_below(_world: &mut GameWorld, metres: f32) {
    let low = wait_for(Duration::from_secs(8), || snapshot().eye_height < metres);
    assert!(low, "eyes still at {} m", snapshot().eye_height);
}

#[then(regex = r"^my eyes are above ([\d.]+) metres$")]
fn eyes_above(_world: &mut GameWorld, metres: f32) {
    let high = wait_for(Duration::from_secs(8), || snapshot().eye_height > metres);
    assert!(high, "eyes only at {} m", snapshot().eye_height);
}

#[then(regex = r"^I am moving slower than ([\d.]+) metres per second$")]
fn moving_slower_than(_world: &mut GameWorld, limit: f32) {
    // Give the movement time to build up to its top speed first.
    std::thread::sleep(Duration::from_secs(2));
    let speed = snapshot().ground_speed;
    assert!(speed < limit && speed > 0.2, "moving at {speed} m/s");
}

#[then(regex = r"^I am moving faster than ([\d.]+) metres per second$")]
fn moving_faster_than(_world: &mut GameWorld, limit: f32) {
    let fast = wait_for(Duration::from_secs(8), || snapshot().ground_speed > limit);
    assert!(fast, "only moving at {} m/s", snapshot().ground_speed);
}

#[given(regex = r"^the crosshair is less than ([\d.]+) degrees wide$")]
#[then(regex = r"^the crosshair is less than ([\d.]+) degrees wide$")]
fn spread_less_than(_world: &mut GameWorld, degrees: f32) {
    let ok = wait_for(Duration::from_secs(8), || snapshot().spread.to_degrees() < degrees);
    assert!(ok, "spread is {:.2} degrees", snapshot().spread.to_degrees());
}

#[given(regex = r"^the crosshair is more than ([\d.]+) degrees wide$")]
#[then(regex = r"^the crosshair is more than ([\d.]+) degrees wide$")]
fn spread_more_than(_world: &mut GameWorld, degrees: f32) {
    let ok = wait_for(Duration::from_secs(8), || snapshot().spread.to_degrees() > degrees);
    assert!(ok, "spread is only {:.2} degrees", snapshot().spread.to_degrees());
}

#[then("every tracer showing has flown at least 15 metres")]
fn tracers_only_far_out(_world: &mut GameWorld) {
    // Look many times while the bullet travels: a bullet is only ever shown once it is well out.
    let mut looked = 0;
    let mut seen_bullet = false;
    let deadline = Instant::now() + Duration::from_millis(800);
    while Instant::now() < deadline {
        for bullet in snapshot().bullets {
            seen_bullet = true;
            assert!(!bullet.visible || bullet.travelled >= 15.0, "a tracer showed after only {} m", bullet.travelled);
        }
        looked += 1;
    }
    assert!(seen_bullet, "never saw a bullet in flight ({looked} looks)");
}

#[then("a tracer shows once the bullet is well out")]
fn tracer_shows(_world: &mut GameWorld) {
    let shown = wait_for(Duration::from_secs(3), || snapshot().bullets.iter().any(|b| b.visible));
    assert!(shown, "no tracer ever appeared");
}

#[given(regex = r"^the wind blows (east|west) at ([\d.]+) metres per second$")]
fn wind_blows(_world: &mut GameWorld, toward: String, speed: f32) {
    // Heading counts from +X (east) toward +Z (south).
    let heading = if toward == "east" { 0.0 } else { std::f32::consts::PI };
    send(Command::SetWind { heading, speed });
    step_pause();
}

/// A bullet in flight that has gone at least `metres`, read before it lands.
fn bullet_past(metres: f32) -> Option<crate::BulletInfo> {
    let mut found = None;
    wait_for(Duration::from_secs(3), || {
        found = snapshot().bullets.into_iter().find(|b| b.travelled >= metres);
        found.is_some()
    });
    found
}

#[then(regex = r"^the bullet leaves at between (\d+) and (\d+) metres per second$")]
fn muzzle_velocity(_world: &mut GameWorld, low: f32, high: f32) {
    let mut first = None;
    wait_for(Duration::from_secs(3), || {
        first = snapshot().bullets.into_iter().next();
        first.is_some()
    });
    let bullet = first.expect("no bullet in flight");
    assert!(bullet.speed > low && bullet.speed < high, "first seen at {} m/s ({} m out)", bullet.speed, bullet.travelled);
}

#[then(regex = r"^the bullet has slowed to between (\d+) and (\d+) metres per second after (\d+) metres$")]
fn slowed(_world: &mut GameWorld, low: f32, high: f32, metres: f32) {
    let bullet = bullet_past(metres).expect("the bullet never got that far");
    assert!(bullet.speed > low && bullet.speed < high, "{} m/s after {} m", bullet.speed, bullet.travelled);
}

#[then(regex = r"^the bullet is falling after (\d+) metres$")]
fn falling(_world: &mut GameWorld, metres: f32) {
    let bullet = bullet_past(metres).expect("the bullet never got that far");
    assert!(bullet.velocity.y < -1.0, "still moving up/level at {} m/s after {} m", bullet.velocity.y, bullet.travelled);
}

#[then(regex = r"^the bullet is being blown (east|west) after (\d+) metres$")]
fn blown(_world: &mut GameWorld, toward: String, metres: f32) {
    let bullet = bullet_past(metres).expect("the bullet never got that far");
    let sideways = if toward == "east" { bullet.velocity.x } else { -bullet.velocity.x };
    assert!(sideways > 1.5, "only {sideways} m/s {toward}ward after {} m", bullet.travelled);
}

#[then(regex = r"^the bullet is not being blown sideways after (\d+) metres$")]
fn not_blown(_world: &mut GameWorld, metres: f32) {
    let bullet = bullet_past(metres).expect("the bullet never got that far");
    assert!(bullet.velocity.x.abs() < 1.5, "moving sideways at {} m/s after {} m", bullet.velocity.x, bullet.travelled);
}
