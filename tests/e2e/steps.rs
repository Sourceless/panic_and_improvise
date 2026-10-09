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
    let loaded = wait_for(Duration::from_secs(30), || snapshot().shot_sound_loaded);
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

#[given(regex = r"^I fire (\d+) single shots?$")]
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

#[given("I fire once")]
#[when("I fire once")]
fn fire_once(_world: &mut GameWorld) {
    step_pause();
    let before = snapshot().shots_fired;
    tap_fire();
    // Don't move on until the shot has happened, so what the next step does can't land in the same frame.
    wait_for(Duration::from_secs(3), || snapshot().shots_fired > before);
}

#[given(regex = r"^I hold fire for ([\d.]+) seconds?$")]
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
    assert!((health - (100.0 - damage)).abs() < 0.05, "health {health}, expected {}", 100.0 - damage);
}

#[then(regex = r"^the target dummy respawns within (\d+) seconds$")]
fn dummy_respawns(_world: &mut GameWorld, seconds: u64) {
    step_pause();
    let respawned = wait_for(Duration::from_secs(seconds), || snapshot().dummy_health >= 100.0);
    assert!(respawned, "target dummy did not respawn within {seconds} seconds");
}

fn key_named(name: &str) -> KeyCode {
    match name {
        "A" => KeyCode::KeyA,
        "B" => KeyCode::KeyB,
        "C" => KeyCode::KeyC,
        "D" => KeyCode::KeyD,
        "E" => KeyCode::KeyE,
        "F" => KeyCode::KeyF,
        "G" => KeyCode::KeyG,
        "H" => KeyCode::KeyH,
        "I" => KeyCode::KeyI,
        "J" => KeyCode::KeyJ,
        "K" => KeyCode::KeyK,
        "L" => KeyCode::KeyL,
        "M" => KeyCode::KeyM,
        "N" => KeyCode::KeyN,
        "O" => KeyCode::KeyO,
        "P" => KeyCode::KeyP,
        "Q" => KeyCode::KeyQ,
        "R" => KeyCode::KeyR,
        "S" => KeyCode::KeyS,
        "T" => KeyCode::KeyT,
        "U" => KeyCode::KeyU,
        "V" => KeyCode::KeyV,
        "W" => KeyCode::KeyW,
        "X" => KeyCode::KeyX,
        "Y" => KeyCode::KeyY,
        "Z" => KeyCode::KeyZ,
        "1" => KeyCode::Digit1,
        "2" => KeyCode::Digit2,
        "Tab" => KeyCode::Tab,
        "Space" => KeyCode::Space,
        "Shift" => KeyCode::ShiftLeft,
        "Ctrl" => KeyCode::ControlLeft,
        "Escape" => KeyCode::Escape,
        other => panic!("no key called {other}"),
    }
}

#[given(regex = r"^I press ([A-Za-z0-9]+)$")]
#[when(regex = r"^I press ([A-Za-z0-9]+)$")]
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

#[given(regex = r"^I am moving faster than ([\d.]+) metres per second$")]
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

#[given(regex = r"^the magazine has (\d+) rounds?$")]
#[then(regex = r"^the magazine has (\d+) rounds?$")]
fn magazine_has(_world: &mut GameWorld, rounds: u32) {
    let ok = wait_for(Duration::from_secs(8), || snapshot().ammo == rounds);
    assert!(ok, "the magazine has {} rounds, not {rounds}", snapshot().ammo);
}

#[given("the gun is reloading")]
#[then("the gun is reloading")]
fn gun_reloading(_world: &mut GameWorld) {
    let ok = wait_for(Duration::from_secs(3), || snapshot().reloading);
    assert!(ok, "the gun is not reloading");
}

#[then("the gun is not reloading")]
fn gun_not_reloading(_world: &mut GameWorld) {
    std::thread::sleep(Duration::from_millis(300));
    assert!(!snapshot().reloading, "the gun is reloading");
}

#[given("the gun has dipped off the screen")]
#[then("the gun has dipped off the screen")]
fn gun_dipped(_world: &mut GameWorld) {
    let ok = wait_for(Duration::from_secs(8), || snapshot().gun_lowered > 0.99);
    assert!(ok, "the gun only got {} of the way down", snapshot().gun_lowered);
}

#[given("the gun is back up")]
#[then("the gun is back up")]
fn gun_back_up(_world: &mut GameWorld) {
    // It has to have gone down first: a gun that never started reloading isn't "back up".
    let started = wait_for(Duration::from_secs(3), || snapshot().reloading);
    assert!(started, "the reload never started");
    let ok = wait_for(Duration::from_secs(10), || {
        let s = snapshot();
        !s.reloading && s.gun_lowered == 0.0
    });
    assert!(ok, "the gun is still down");
}

#[given(regex = r"^the bolt is (forward|back)$")]
#[then(regex = r"^the bolt is (forward|back)$")]
fn bolt_is(_world: &mut GameWorld, place: String) {
    use fps_prototype::gun_state::Bolt;
    let want = if place == "forward" { Bolt::Forward } else { Bolt::Rear };
    let ok = wait_for(Duration::from_secs(3), || snapshot().bolt == Some(want));
    assert!(ok, "the bolt is {:?}, not {want:?}", snapshot().bolt);
}

#[given(regex = r"^the reload takes ([\d.]+) seconds$")]
#[then(regex = r"^the reload takes ([\d.]+) seconds$")]
fn reload_takes(_world: &mut GameWorld, seconds: f32) {
    let started = wait_for(Duration::from_secs(3), || snapshot().reloading);
    assert!(started, "no reload is going");
    let total = snapshot().reload_seconds;
    assert!((total - seconds).abs() < 0.05, "the reload takes {total} s, not {seconds}");
}

#[then("a bullet hole appears in the ground")]
fn hole_in_ground(_world: &mut GameWorld) {
    use fps_prototype::impact::Surface;
    let ok = wait_for(Duration::from_secs(4), || snapshot().holes_live > 0);
    let state = snapshot();
    assert!(ok, "no bullet hole appeared ({} impacts)", state.impacts.impacts);
    let (surface, normal) = state.impacts.last.expect("an impact");
    assert_eq!(surface, Surface::Ground);
    assert!(normal.y > 0.99, "the ground should face up: {normal:?}");
    let at = state.impacts.last_position.expect("a position");
    assert!(at.y.abs() < 0.001, "the bullet landed on the ground, not {} m up or down", at.y);
}

#[then("a bullet hole appears on the target")]
fn hole_on_target(_world: &mut GameWorld) {
    use fps_prototype::impact::Surface;
    let ok = wait_for(Duration::from_secs(4), || snapshot().holes_on_target > 0);
    let state = snapshot();
    assert!(ok, "no bullet hole on the target ({} impacts)", state.impacts.impacts);
    let (surface, normal) = state.impacts.last.expect("an impact");
    assert_eq!(surface, Surface::Target);
    assert!(normal.z > 0.99, "the face turned to the player points back at them: {normal:?}");
}

#[then("dirt is thrown up")]
fn dirt_thrown(_world: &mut GameWorld) {
    let ok = wait_for(Duration::from_secs(4), || snapshot().chips_live > 3);
    assert!(ok, "only {} chips in the air", snapshot().chips_live);
}

#[then("a puff of dust rises")]
fn dust_rises(_world: &mut GameWorld) {
    let ok = wait_for(Duration::from_secs(4), || snapshot().puffs_live > 0);
    assert!(ok, "no dust");
}

#[then("the impact thumps")]
fn impact_thumps(_world: &mut GameWorld) {
    // It reaches the player a moment later, at the speed of sound.
    let ok = wait_for(Duration::from_secs(4), || snapshot().thumps_playing > 0);
    let state = snapshot();
    assert!(ok, "no thump was heard (thumps started: {}, impacts: {})", state.impacts.thumps, state.impacts.impacts);
}

#[then("the debris is gone again")]
fn debris_gone(_world: &mut GameWorld) {
    let ok = wait_for(Duration::from_secs(6), || {
        let s = snapshot();
        s.chips_live == 0 && s.puffs_live == 0
    });
    let s = snapshot();
    assert!(ok, "still {} chips and {} puffs", s.chips_live, s.puffs_live);
    assert!(s.holes_live > 0, "but the hole stays");
}

#[then(regex = r"^the gun has played (\d+) reload sounds?, (\d+) of them with the bolt charged$")]
fn reload_sounds(_world: &mut GameWorld, total: u32, charged: u32) {
    let ok = wait_for(Duration::from_secs(3), || snapshot().sound_log.reloads == total);
    let log = snapshot().sound_log;
    assert!(ok && log.charged_reloads == charged, "reload sounds: {log:?}");
}

#[then(regex = r"^the gun has clicked (\d+) times?$")]
fn dry_clicks(_world: &mut GameWorld, clicks: u32) {
    let ok = wait_for(Duration::from_secs(3), || snapshot().sound_log.dry_clicks == clicks);
    assert!(ok, "clicks: {:?}", snapshot().sound_log);
}

#[when(regex = r"^I pull the trigger (\d+) times quickly$")]
fn pull_quickly(_world: &mut GameWorld, times: u32) {
    step_pause();
    for _ in 0..times {
        tap_fire();
        std::thread::sleep(Duration::from_millis(120));
    }
}

// ---- collision ------------------------------------------------------------------------------

use fps_prototype::collision::{Material, Solid};

/// A wall across the way, 3 m ahead of where the player starts, running 40 m across.
fn wall_ahead(half_thickness: f32, height: f32, material: Material) {
    let across = bevy::prelude::Vec2::new(20.0, 0.0);
    let at = bevy::prelude::Vec2::new(0.0, -3.0);
    send(Command::AddSolid(Solid::wall(at - across, at + across, half_thickness, height, height).of(material)));
    step_pause();
}

#[given(regex = r"^there is a ([\d.]+) metre high wall ahead$")]
fn stone_wall_ahead(_world: &mut GameWorld, height: f32) {
    wall_ahead(0.3, height, Material::Stone);
}

#[given(regex = r"^there is a ([\d.]+) metre high hedge ahead$")]
fn hedge_ahead(_world: &mut GameWorld, height: f32) {
    wall_ahead(0.6, height, Material::Leaves);
}

#[given(regex = r"^there is a ([\d.]+) metre high fence ahead$")]
fn fence_ahead(_world: &mut GameWorld, height: f32) {
    wall_ahead(0.12, height, Material::Wood);
}

#[given("there is a tree trunk ahead")]
fn trunk_ahead(_world: &mut GameWorld) {
    send(Command::AddSolid(Solid::circle(bevy::prelude::Vec2::new(0.0, -3.0), 0.5, 8.0).of(Material::Wood)));
    step_pause();
}

#[given(regex = r"^there is a platform ([\d.]+) metres high ahead$")]
fn platform_ahead(_world: &mut GameWorld, height: f32) {
    send(Command::AddSolid(Solid::rect(bevy::prelude::Vec2::new(0.0, -5.0), bevy::prelude::Vec2::new(3.0, 3.0), 0.0, height)));
    step_pause();
}

#[when(regex = r"^I wait ([\d.]+) seconds?$")]
#[given(regex = r"^I wait ([\d.]+) seconds?$")]
fn wait_seconds(_world: &mut GameWorld, seconds: f32) {
    std::thread::sleep(Duration::from_secs_f32(seconds));
}

#[then("I am still on this side of it")]
fn this_side(_world: &mut GameWorld) {
    // The obstacles are 3 m ahead; being stopped by one is being short of its middle line.
    let (_, z) = snapshot().player;
    assert!(z > -3.0, "got through: now at z = {z}");
    assert!(z < 0.0, "never moved? z = {z}");
}

#[then(regex = r"^I stopped ([\d.]+) metres short of its middle$")]
fn stopped_short(_world: &mut GameWorld, metres: f32) {
    let (_, z) = snapshot().player;
    let gap = z - -3.0;
    assert!((gap - metres).abs() < 0.12, "stopped {gap} m from the line the obstacle is on, not {metres}");
}

#[then("I am on the far side of it")]
fn far_side(_world: &mut GameWorld) {
    let across = wait_for(Duration::from_secs(4), || snapshot().player.1 < -3.9);
    assert!(across, "still on this side: z = {}", snapshot().player.1);
}

#[then("I am on the ground")]
fn on_the_ground(_world: &mut GameWorld) {
    let down = wait_for(Duration::from_secs(4), || snapshot().feet.abs() < 0.03);
    assert!(down, "feet at {} m", snapshot().feet);
}

#[then(regex = r"^I have climbed (at least|exactly) (\d+) times?$")]
fn climbed(_world: &mut GameWorld, how: String, times: u32) {
    let got = snapshot().mantles;
    let ok = if how == "exactly" { got == times } else { got >= times };
    assert!(ok, "climbed {got} times");
}

#[then(regex = r"^my feet are at least ([\d.]+) metres up$")]
fn feet_up(_world: &mut GameWorld, metres: f32) {
    let up = wait_for(Duration::from_secs(4), || snapshot().feet >= metres);
    assert!(up, "feet only {} m up", snapshot().feet);
}

#[then(regex = r"^my feet are ([\d.]+) metres up$")]
fn feet_exactly(_world: &mut GameWorld, metres: f32) {
    let feet = snapshot().feet;
    assert!((feet - metres).abs() < 0.05, "feet are {feet} m up, not {metres}");
}

#[then(regex = r"^I am past z of (-?[\d.]+)$")]
fn past_z(_world: &mut GameWorld, z: f32) {
    let ok = wait_for(Duration::from_secs(5), || snapshot().player.1 < z);
    assert!(ok, "only got to z = {}", snapshot().player.1);
}

#[given("there is a metal shed ahead")]
fn shed_ahead(_world: &mut GameWorld) {
    send(Command::AddSolid(Solid::rect(bevy::prelude::Vec2::new(0.0, -6.0), bevy::prelude::Vec2::new(4.0, 3.0), 0.0, 5.0).of(Material::Metal)));
    step_pause();
}

// ---- what bullets do to solid things ---------------------------------------------------------

fn material_named(name: &str) -> Material {
    match name {
        "stone" => Material::Stone,
        "wood" => Material::Wood,
        "leaves" => Material::Leaves,
        "metal" => Material::Metal,
        other => panic!("no material called {other}"),
    }
}

#[then(regex = r"^a bullet hole appears in the (stone|wood|leaves|metal)$")]
fn hole_in_solid(_world: &mut GameWorld, name: String) {
    use fps_prototype::impact::Surface;
    let ok = wait_for(Duration::from_secs(4), || snapshot().holes_live > 0);
    let state = snapshot();
    assert!(ok, "no bullet hole appeared ({} impacts)", state.impacts.impacts);
    let (surface, normal) = state.impacts.last.expect("an impact");
    assert_eq!(surface, Surface::Solid(material_named(&name)));
    assert!(normal.z > 0.9, "the face shot at looks back at the shooter: {normal:?}");
}

#[then(regex = r"^the bullet landed ([\d.]+) metres short of the middle line$")]
fn landed_short(_world: &mut GameWorld, metres: f32) {
    let at = snapshot().impacts.last_position.expect("an impact");
    let gap = at.z - -3.0;
    assert!((gap - metres).abs() < 0.08, "landed {gap} m from the middle line, not {metres}");
}

#[then("the bullet landed on the shed")]
fn landed_on_shed(_world: &mut GameWorld) {
    let at = snapshot().impacts.last_position.expect("an impact");
    assert!((at.z - -3.0).abs() < 0.05, "front of the shed is at z = -3, not {}", at.z);
}

#[then("nothing was hit beyond it")]
fn nothing_beyond(_world: &mut GameWorld) {
    std::thread::sleep(Duration::from_millis(600));
    let state = snapshot();
    assert_eq!(state.dummy_hits, 0, "the target dummy behind it was hit");
    assert_eq!(state.impacts.impacts, 1, "{} impacts", state.impacts.impacts);
}

#[given(regex = r"^there is a (thin|thick) tree crown ahead$")]
fn crown_ahead(_world: &mut GameWorld, size: String) {
    // 4 m across and 8 m across, both centred at the height of a standing shot, 5 m ahead.
    let radius = if size == "thin" { 2.0 } else { 4.0 };
    send(Command::AddCanopy(fps_prototype::collision::Canopy {
        centre: bevy::prelude::Vec3::new(0.0, 1.8, -5.0),
        radii: bevy::prelude::Vec3::splat(radius),
    }));
    step_pause();
}

#[then(regex = r"^the target dummy has taken between ([\d.]+) and ([\d.]+) damage$")]
fn dummy_damage_between(_world: &mut GameWorld, low: f32, high: f32) {
    let ok = wait_for(Duration::from_secs(4), || {
        let taken = 100.0 - snapshot().dummy_health;
        taken >= low && taken <= high
    });
    assert!(ok, "the target dummy has taken {} damage", 100.0 - snapshot().dummy_health);
}

#[then("the target dummy is unhurt")]
fn dummy_unhurt(_world: &mut GameWorld) {
    // Give a bullet time to get there, if it is going to.
    std::thread::sleep(Duration::from_millis(900));
    let state = snapshot();
    assert_eq!(state.dummy_hits, 0, "the target dummy was hit");
    assert_eq!(state.dummy_health, 100.0);
}

// ---- controls and the menu ------------------------------------------------------------------

use fps_prototype::controls::{Action, Bind, Preset};

#[given("my controls are Colemak Mod-DH")]
fn controls_are_colemak(_world: &mut GameWorld) {
    send(Command::UsePreset(Preset::ColemakModDh));
    step_pause();
}

#[then("the menu is open")]
fn menu_is_open(_world: &mut GameWorld) {
    let ok = wait_for(Duration::from_secs(4), || snapshot().menu_open);
    assert!(ok, "the menu is not open");
}

#[then("the menu is closed")]
fn menu_is_closed(_world: &mut GameWorld) {
    let ok = wait_for(Duration::from_secs(4), || !snapshot().menu_open);
    assert!(ok, "the menu is open");
}

#[given("the menu is open")]
fn open_the_menu(_world: &mut GameWorld) {
    step_pause();
    send(Command::TapKey(KeyCode::Escape));
    let ok = wait_for(Duration::from_secs(4), || snapshot().menu_open);
    assert!(ok, "the menu did not open");
}

#[then("the mouse is free")]
fn mouse_is_free(_world: &mut GameWorld) {
    let ok = wait_for(Duration::from_secs(4), || !snapshot().cursor_captured);
    assert!(ok, "the mouse is still captured");
}

#[then("the mouse is captured")]
fn mouse_is_captured(_world: &mut GameWorld) {
    let ok = wait_for(Duration::from_secs(4), || snapshot().cursor_captured);
    assert!(ok, "the mouse is free");
}

#[when(regex = r"^I choose (QWERTY|Colemak Mod-DH) in the menu$")]
fn choose_preset(_world: &mut GameWorld, name: String) {
    let preset = if name == "QWERTY" { Preset::Qwerty } else { Preset::ColemakModDh };
    send(Command::MenuChoosePreset(preset));
    step_pause();
}

#[when(regex = r"^I choose to rebind (\w+)$")]
fn choose_rebind(_world: &mut GameWorld, action: String) {
    send(Command::MenuStartRebind(Action::from_id(&action).unwrap_or_else(|| panic!("no action {action}"))));
    step_pause();
}

#[when("I save my controls")]
fn save_controls(_world: &mut GameWorld) {
    send(Command::MenuSave);
    step_pause();
}

#[then(regex = r"^the menu is waiting for a key for (\w+)$")]
fn menu_waiting(_world: &mut GameWorld, action: String) {
    let want = Action::from_id(&action).unwrap_or_else(|| panic!("no action {action}"));
    let ok = wait_for(Duration::from_secs(3), || snapshot().menu_rebinding == Some(want));
    assert!(ok, "waiting for {:?}", snapshot().menu_rebinding);
}

#[then("the menu is not waiting for a key")]
fn menu_not_waiting(_world: &mut GameWorld) {
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(snapshot().menu_rebinding, None);
}

#[then(regex = r"^(\w+) is bound to (\w+)$")]
fn is_bound_to(_world: &mut GameWorld, action: String, key: String) {
    let action = Action::from_id(&action).unwrap_or_else(|| panic!("no action {action}"));
    let want = Bind::parse(&key).unwrap_or_else(|| panic!("no key {key}"));
    let ok = wait_for(Duration::from_secs(3), || snapshot().controls.bind(action) == want);
    assert!(ok, "{action:?} is on {:?}, not {want:?}", snapshot().controls.bind(action));
}

#[then(regex = r#"^the saved controls say "(.*)"$"#)]
fn saved_say(_world: &mut GameWorld, text: String) {
    // The game writes the file on its next frame.
    let read = || std::fs::read_to_string(fps_prototype::controls::controls_path()).unwrap_or_default();
    let ok = wait_for(Duration::from_secs(4), || read().contains(&text));
    assert!(ok, "the saved controls are:\n{}", read());
}

#[then(regex = r"^I have moved (forward|back|left|right)$")]
fn have_moved(_world: &mut GameWorld, way: String) {
    let ok = wait_for(Duration::from_secs(4), || {
        let (x, z) = snapshot().player;
        match way.as_str() {
            "forward" => z < -1.0,
            "back" => z > 1.0,
            "right" => x > 1.0,
            _ => x < -1.0,
        }
    });
    assert!(ok, "at {:?}", snapshot().player);
}

#[then("I have not moved")]
fn have_not_moved(_world: &mut GameWorld) {
    std::thread::sleep(Duration::from_secs(1));
    let (x, z) = snapshot().player;
    assert!(x.abs() < 0.05 && z.abs() < 0.05, "moved to ({x}, {z})");
}

#[then(regex = r"^the gun in hand is the (.+)$")]
fn gun_in_hand(_world: &mut GameWorld, name: String) {
    let ok = wait_for(Duration::from_secs(3), || snapshot().gun_name == name);
    assert!(ok, "the gun in hand is {:?}, not {name}", snapshot().gun_name);
}

#[then("my hands are empty")]
fn hands_empty(_world: &mut GameWorld) {
    let ok = wait_for(Duration::from_secs(3), || snapshot().gun_name.is_empty());
    assert!(ok, "something is in hand: {}", snapshot().gun_name);
}

#[given(regex = r"^I have (\d+) spare rounds$")]
#[then(regex = r"^I have (\d+) spare rounds$")]
fn spare_rounds(_world: &mut GameWorld, rounds: u32) {
    let ok = wait_for(Duration::from_secs(3), || snapshot().reserve == rounds);
    assert!(ok, "there are {} spare rounds, not {rounds}", snapshot().reserve);
}

#[given("I have no spare rounds")]
fn no_spare_rounds(_world: &mut GameWorld) {
    send(Command::EmptyPockets);
    let ok = wait_for(Duration::from_secs(3), || snapshot().reserve == 0);
    assert!(ok, "still {} spare rounds", snapshot().reserve);
}

#[then(regex = r"^the next reload puts in (.+)$")]
fn next_load_is(_world: &mut GameWorld, name: String) {
    let ok = wait_for(Duration::from_secs(3), || snapshot().next_load == name);
    assert!(ok, "the next load is {:?}, not {name}", snapshot().next_load);
}

#[then(regex = r"^the gun is loaded with (.+)$")]
fn gun_loaded_with(_world: &mut GameWorld, name: String) {
    let ok = wait_for(Duration::from_secs(3), || snapshot().loaded_with == name);
    assert!(ok, "loaded with {:?}, not {name}", snapshot().loaded_with);
}

#[then(regex = r"^the trigger is set to (semi|auto)$")]
fn trigger_set(_world: &mut GameWorld, mode: String) {
    let ok = wait_for(Duration::from_secs(3), || snapshot().fire_mode == mode);
    assert!(ok, "the trigger is set to {}, not {mode}", snapshot().fire_mode);
}

#[then("the inventory is open and the mouse is free")]
fn inventory_open(_world: &mut GameWorld) {
    let ok = wait_for(Duration::from_secs(3), || snapshot().inventory_open && !snapshot().cursor_captured);
    let s = snapshot();
    assert!(ok, "inventory open: {}, cursor captured: {}", s.inventory_open, s.cursor_captured);
}

#[then("the inventory is closed")]
fn inventory_closed(_world: &mut GameWorld) {
    let ok = wait_for(Duration::from_secs(3), || !snapshot().inventory_open);
    assert!(ok, "the inventory is still open");
}
