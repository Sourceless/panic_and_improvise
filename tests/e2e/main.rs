mod steps;

use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback};
use bevy::input::InputSystems;
use bevy::ecs::message::Messages;
use bevy::ecs::system::RunSystemOnce;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input::mouse::MouseMotion;
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, ExitCondition, PrimaryWindow};
use bevy::winit::WinitPlugin;
use cucumber::World as _;
use cucumber::writer::Stats;
use fps_prototype::controls::{controls_path, Action, Controls, ControlsSet, Keyboard, Preset};
use fps_prototype::menu::Menu;
use fps_prototype::player::{spawn_player, CursorIntent, FpsCamera};
use fps_prototype::target::{spawn_dummy, TargetDummy};
use fps_prototype::weapon::{spawn_gun, Bullet, BulletAssets, Gun};
use fps_prototype::map::TerrainMap;
use fps_prototype::GamePlugin;

pub enum Command {
    /// Real keyboard events, as the keyboard sends them: the physical key, typing the letter or key
    /// its layout gives it (see `logical_key`).
    PressKey(KeyCode),
    ReleaseKey(KeyCode),
    TapKey(KeyCode),
    /// Change the controls without going through the menu.
    UsePreset(Preset),
    /// What clicking in the menu does.
    MenuChoosePreset(Preset),
    MenuStartRebind(Action),
    MenuSave,
    RemoveDummies,
    /// Put something solid in the test room.
    AddSolid(fps_prototype::collision::Solid),
    AddCanopy(fps_prototype::collision::Canopy),
    /// The wind blows toward `heading` (radians from +X toward +Z) at `speed` m/s.
    SetWind { heading: f32, speed: f32 },
    Press(MouseButton),
    Release(MouseButton),
    Tap(MouseButton),
    SetCursorCaptured(bool),
    LoadRoom,
    Snapshot(Sender<Snapshot>),
    Quit { failed: bool },
}

#[derive(Debug, Clone, Copy)]
pub struct BulletInfo {
    /// How far it has flown, metres.
    pub travelled: f32,
    /// Whether its tracer is showing.
    pub visible: bool,
    pub speed: f32,
    /// Its velocity (world axes: x east, y up, z south), m/s.
    pub velocity: Vec3,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub cursor_captured: bool,
    pub shot_sound_loaded: bool,
    pub shots_fired: u32,
    pub playing_sounds: usize,
    pub dummy_health: f32,
    pub dummy_hits: u32,
    /// Whether the player has the gun raised to its sights, and how far it has got (0 to 1).
    pub aiming: bool,
    pub aim_blend: f32,
    /// How far up the view is pointing, radians.
    pub camera_pitch: f32,
    /// How far (radians) the worst shot so far strayed from where the gun was pointed.
    pub worst_shot_error: f32,
    /// Whether the muzzle flash is showing.
    pub flash_visible: bool,
    /// Where the last bullet started, relative to the camera (right, up, forward).
    pub last_shot_origin: Option<(f32, f32, f32)>,
    pub stance: fps_prototype::player::Stance,
    /// The eye's height above the ground, metres.
    pub eye_height: f32,
    /// How fast the player is moving along the ground, m/s.
    pub ground_speed: f32,
    /// Whether the player's feet are off the ground.
    pub airborne: bool,
    /// The gun's current inaccuracy (radians, half-angle), as the crosshair shows it.
    pub spread: f32,
    /// Bullets in flight.
    pub bullets: Vec<BulletInfo>,
    /// Rounds left in the magazine, whether a reload is going, and how far the gun is lowered for it.
    pub ammo: u32,
    pub reloading: bool,
    pub gun_lowered: f32,
    /// Where the bolt rests when the gun is at rest, and how long the current reload will take.
    pub bolt: Option<fps_prototype::gun_state::Bolt>,
    pub reload_seconds: f32,
    /// What bullets landing have done so far, and what is still to be seen of it.
    pub impacts: fps_prototype::impact::ImpactStats,
    pub holes_live: usize,
    pub holes_on_target: usize,
    pub chips_live: usize,
    pub puffs_live: usize,
    /// Placed (impact) sounds playing right now.
    pub thumps_playing: usize,
    pub sound_log: fps_prototype::weapon::SoundLog,
    /// Where the player is (x, z), how high their feet are, and how many climbs they have made.
    pub player: (f32, f32),
    pub feet: f32,
    pub mantles: u32,
    pub mantling: bool,
    pub menu_open: bool,
    pub menu_rebinding: Option<Action>,
    pub menu_message: String,
    pub controls: Controls,
}

static GAME: OnceLock<Sender<Command>> = OnceLock::new();

pub fn send(command: Command) {
    GAME.get()
        .expect("game not started")
        .send(command)
        .expect("game thread stopped");
}

pub fn snapshot() -> Snapshot {
    let (reply_tx, reply_rx) = mpsc::channel();
    send(Command::Snapshot(reply_tx));
    reply_rx.recv_timeout(Duration::from_secs(30)).expect("no snapshot reply")
}

#[derive(Resource)]
struct Bridge {
    rx: Mutex<Receiver<Command>>,
}

#[derive(Resource, Default)]
struct PendingTaps(Vec<MouseButton>);

#[derive(Resource, Default)]
struct HeldButtons(Vec<MouseButton>);

/// Keys tapped (pressed for one frame): their release goes out at the start of the next frame.
#[derive(Resource, Default)]
struct PendingKeyTaps(Vec<KeyCode>);

/// What was already held down on the previous frame: a button held for many frames is pressed once.
#[derive(Resource, Default)]
struct AlreadyHeld {
    buttons: Vec<MouseButton>,
}

fn main() -> ExitCode {
    // The tests keep their settings in a folder of their own, so your own saved controls can't change what they do.
    let config = std::env::temp_dir().join(format!("fps_e2e_config_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&config);
    // SAFETY: nothing else is running yet: this is before any thread is started.
    unsafe { std::env::set_var("FPS_CONFIG_DIR", &config) };
    let headed = std::env::var("E2E_HEADED").is_ok();
    let (tx, rx) = mpsc::channel();
    GAME.set(tx).expect("game already started");

    std::thread::spawn(move || {
        let failed = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
            .block_on(async {
                let writer = steps::GameWorld::cucumber()
                    .max_concurrent_scenarios(1)
                    .run("tests/e2e/features")
                    .await;
                writer.execution_has_failed()
            });
        send(Command::Quit { failed });
    });

    match build_app(headed, rx).run() {
        AppExit::Success => ExitCode::SUCCESS,
        AppExit::Error(code) => ExitCode::from(code.get()),
    }
}

fn build_app(headed: bool, rx: Receiver<Command>) -> App {
    let mut app = App::new();
    if headed {
        app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "FPS E2E".into(),
                ..default()
            }),
            ..default()
        }));
    } else {
        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    close_when_requested: false,
                    ..default()
                })
                .disable::<WinitPlugin>(),
        )
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::from_millis(1)));
        app.world_mut().spawn((
            Window::default(),
            PrimaryWindow,
            CursorOptions {
                grab_mode: CursorGrabMode::Locked,
                visible: false,
                ..default()
            },
        ));
    }

    app.insert_resource(TerrainMap::flat(0.0))
        .add_plugins(GamePlugin)
        .insert_resource(Bridge { rx: Mutex::new(rx) })
        .init_resource::<PendingTaps>()
        .init_resource::<HeldButtons>()
        .init_resource::<PendingKeyTaps>()
        .init_resource::<AlreadyHeld>()
        // The test's key events go out before the game reads the keyboard (`ControlsSet`).
        .add_systems(PreUpdate, (isolate_input, drive).chain().after(InputSystems).before(ControlsSet))
        .add_systems(Last, release_taps);
    app
}

// Real keyboard and mouse input is discarded so only test commands reach the game.
fn isolate_input(
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut key_events: ResMut<Messages<KeyboardInput>>,
    mut motion: ResMut<Messages<MouseMotion>>,
    held: Res<HeldButtons>,
    mut already: ResMut<AlreadyHeld>,
) {
    mouse.reset_all();
    keys.reset_all();
    // Whatever the real keyboard sent doesn't get through: only the test's own events do.
    key_events.clear();
    motion.clear();
    // Held buttons stay pressed, but only the first frame counts as a fresh press (as with real input).
    for button in &held.0 {
        mouse.press(*button);
        if already.buttons.contains(button) {
            mouse.clear_just_pressed(*button);
        }
    }
    already.buttons = held.0.clone();
}

/// What a physical key types, on the layout the tests pretend to have (the letter the key is named for).
fn logical_key(code: KeyCode) -> Key {
    match code {
        KeyCode::Space => Key::Space,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => Key::Shift,
        KeyCode::ControlLeft | KeyCode::ControlRight => Key::Control,
        KeyCode::Escape => Key::Escape,
        other => match format!("{other:?}").strip_prefix("Key") {
            Some(letter) if letter.len() == 1 => Key::Character(letter.to_lowercase().into()),
            _ => Key::Unidentified(NativeKey::Unidentified),
        },
    }
}

fn key_event(world: &mut World, code: KeyCode, state: ButtonState) {
    let window = world.query_filtered::<Entity, With<PrimaryWindow>>().iter(world).next().unwrap_or(Entity::PLACEHOLDER);
    world.write_message(KeyboardInput { key_code: code, logical_key: logical_key(code), state, text: None, repeat: false, window });
    let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
    match state {
        ButtonState::Pressed => keys.press(code),
        ButtonState::Released => keys.release(code),
    }
}

fn release_taps(mut input: ResMut<ButtonInput<MouseButton>>, mut taps: ResMut<PendingTaps>) {
    for button in taps.0.drain(..) {
        input.release(button);
    }
}

fn drive(world: &mut World) {
    for code in std::mem::take(&mut world.resource_mut::<PendingKeyTaps>().0) {
        key_event(world, code, ButtonState::Released);
    }
    let commands: Vec<Command> = world
        .resource::<Bridge>()
        .rx
        .lock()
        .expect("bridge lock")
        .try_iter()
        .collect();
    for command in commands {
        match command {
            Command::Press(button) => {
                world.resource_mut::<ButtonInput<MouseButton>>().press(button);
                let mut held = world.resource_mut::<HeldButtons>();
                if !held.0.contains(&button) {
                    held.0.push(button);
                }
                world.resource_mut::<AlreadyHeld>().buttons.push(button);
            }
            Command::Release(button) => {
                world.resource_mut::<ButtonInput<MouseButton>>().release(button);
                world.resource_mut::<HeldButtons>().0.retain(|held| *held != button);
            }
            Command::Tap(button) => {
                world.resource_mut::<ButtonInput<MouseButton>>().press(button);
                world.resource_mut::<PendingTaps>().0.push(button);
            }
            Command::PressKey(code) => key_event(world, code, ButtonState::Pressed),
            Command::ReleaseKey(code) => key_event(world, code, ButtonState::Released),
            // A tap lasts one frame: it is let go of at the start of the next.
            Command::TapKey(code) => {
                key_event(world, code, ButtonState::Pressed);
                world.resource_mut::<PendingKeyTaps>().0.push(code);
            }
            Command::UsePreset(preset) => world.resource_mut::<Controls>().use_preset(preset),
            Command::MenuChoosePreset(preset) => {
                let mut controls = world.resource::<Controls>().clone();
                world.resource_mut::<Menu>().choose_preset(preset, &mut controls);
                *world.resource_mut::<Controls>() = controls;
            }
            Command::MenuStartRebind(action) => world.resource_mut::<Menu>().start_rebinding(action),
            Command::MenuSave => {
                let controls = world.resource::<Controls>().clone();
                world.resource_mut::<Menu>().save(&controls, &controls_path());
            }
            Command::SetWind { heading, speed } => {
                *world.resource_mut::<fps_prototype::wind::Wind>() = fps_prototype::wind::Wind { heading, speed };
            }
            Command::AddSolid(solid) => world.resource_mut::<fps_prototype::collision::Colliders>().add(solid),
            Command::AddCanopy(canopy) => world.resource_mut::<fps_prototype::collision::Colliders>().add_canopy(canopy),
            Command::RemoveDummies => {
                let dummies: Vec<Entity> = world.query_filtered::<Entity, With<TargetDummy>>().iter(world).collect();
                for dummy in dummies {
                    world.despawn(dummy);
                }
            }
            Command::SetCursorCaptured(captured) => set_cursor(world, captured),
            Command::LoadRoom => load_room(world),
            Command::Snapshot(reply) => {
                let _ = reply.send(take_snapshot(world));
            }
            Command::Quit { failed } => {
                world.write_message(if failed { AppExit::error() } else { AppExit::Success });
            }
        }
    }
}

fn set_cursor(world: &mut World, captured: bool) {
    world.resource_mut::<CursorIntent>().captured = captured;
    let mut query = world.query_filtered::<&mut CursorOptions, With<PrimaryWindow>>();
    for mut cursor in query.iter_mut(world) {
        cursor.grab_mode = if captured {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
    }
}

// Test room stub: there is only one room, so loading it resets the player, gun, dummy and sounds.
fn load_room(world: &mut World) {
    let entities: Vec<Entity> = world
        .query_filtered::<Entity, Or<(With<FpsCamera>, With<Gun>, With<TargetDummy>, With<Bullet>, With<AudioPlayer>)>>()
        .iter(world)
        .collect();
    for entity in entities {
        world.despawn(entity);
    }
    world.resource_mut::<ButtonInput<MouseButton>>().reset_all();
    world.resource_mut::<HeldButtons>().0.clear();
    world.resource_mut::<PendingKeyTaps>().0.clear();
    world.resource_mut::<ButtonInput<KeyCode>>().reset_all();
    // Controls, the menu, and anything saved: back to a fresh install, for every scenario.
    world.resource_mut::<Keyboard>().clear();
    *world.resource_mut::<Controls>() = Controls::default();
    *world.resource_mut::<Menu>() = Menu::default();
    let _ = std::fs::remove_file(controls_path());
    *world.resource_mut::<fps_prototype::wind::Wind>() = Default::default();
    world.resource_mut::<fps_prototype::collision::Colliders>().clear();
    *world.resource_mut::<fps_prototype::impact::ImpactStats>() = Default::default();
    // What earlier bullets left behind goes too: holes, flying chips, dust.
    let leftovers: Vec<Entity> = world
        .query_filtered::<Entity, Or<(With<fps_prototype::impact::BulletHole>, With<fps_prototype::impact::Chipping>, With<fps_prototype::impact::Dust>)>>()
        .iter(world)
        .collect();
    for entity in leftovers {
        world.despawn(entity);
    }
    set_cursor(world, true);
    world.run_system_once(spawn_player).expect("spawn player");
    world.run_system_once(spawn_gun).expect("spawn gun");
    world.run_system_once(spawn_dummy).expect("spawn dummy");
}

fn take_snapshot(world: &mut World) -> Snapshot {
    let cursor_captured = world
        .query_filtered::<&CursorOptions, With<PrimaryWindow>>()
        .iter(world)
        .any(|cursor| cursor.grab_mode == CursorGrabMode::Locked);
    let shots_fired = world.query::<&Gun>().iter(world).map(|gun| gun.shots_fired).sum();
    let playing_sounds = world
        .query::<&AudioSink>()
        .iter(world)
        .filter(|sink| !sink.empty() && !sink.is_paused())
        .count();
    let (dummy_health, dummy_hits) = world
        .query::<&TargetDummy>()
        .iter(world)
        .next()
        .map(|dummy| (dummy.health, dummy.hits))
        .unwrap_or((0.0, 0));
    let shot_sound_loaded = match world.get_resource::<BulletAssets>() {
        Some(assets) => {
            let server = world.resource::<AssetServer>();
            let mut sounds: Vec<_> = assets.shot_sounds.iter().collect();
            if let Some(gun) = world.get_resource::<fps_prototype::weapon::GunSounds>() {
                sounds.extend(gun.all());
            }
            if let Some(impact) = world.get_resource::<fps_prototype::impact::ImpactAssets>() {
                sounds.extend(impact.sounds());
            }
            sounds.iter().all(|sound| server.is_loaded_with_dependencies(sound.id()))
        }
        None => false,
    };
    let aiming = world.query::<&Gun>().iter(world).any(|gun| gun.aiming);
    let aim_blend = world.query::<&Gun>().iter(world).map(|gun| gun.aim_blend).fold(0.0, f32::max);
    let camera = world.query_filtered::<&Transform, With<FpsCamera>>().iter(world).next().copied();
    let origin = world.query::<&Gun>().iter(world).find_map(|gun| gun.last_shot_origin);
    let last_shot_origin = match (camera, origin) {
        (Some(cam), Some(world_origin)) => {
            let local = cam.compute_affine().inverse().transform_point3(world_origin);
            Some((local.x, local.y, -local.z))
        }
        _ => None,
    };
    let camera_pitch = world.query::<&FpsCamera>().iter(world).next().map_or(0.0, |c| c.pitch());
    let worst_shot_error = world.query::<&Gun>().iter(world).map(|gun| gun.worst_shot_error).fold(0.0, f32::max);
    let flash_visible = world
        .query_filtered::<&Visibility, With<fps_prototype::muzzle_flash::MuzzleFlash>>()
        .iter(world)
        .any(|v| *v != Visibility::Hidden);
    let (stance, eye_height, ground_speed, airborne) = world
        .query::<&FpsCamera>()
        .iter(world)
        .next()
        .map_or((Default::default(), 0.0, 0.0, false), |c| (c.stance(), c.eye_height(), c.speed(), c.airborne()));
    let spread = world.query::<&Gun>().iter(world).map(|gun| gun.current_spread).next().unwrap_or(0.0);
    let bullets = world
        .query::<(&Bullet, &Visibility)>()
        .iter(world)
        .map(|(bullet, visibility)| BulletInfo {
            travelled: bullet.travelled(),
            visible: *visibility != Visibility::Hidden,
            speed: bullet.speed(),
            velocity: bullet.velocity(),
        })
        .collect();
    let (ammo, reloading, gun_lowered, bolt, reload_seconds) = world
        .query::<&Gun>()
        .iter(world)
        .next()
        .map_or((0, false, 0.0, None, 0.0), |g| (g.ammo, g.reloading(), g.lowered, g.bolt(), g.reload_seconds()));
    let impacts = world.get_resource::<fps_prototype::impact::ImpactStats>().copied().unwrap_or_default();
    let holes_live = world.query::<&fps_prototype::impact::BulletHole>().iter(world).count();
    let holes_on_target = {
        let mut query = world.query_filtered::<&ChildOf, With<fps_prototype::impact::BulletHole>>();
        let parents: Vec<Entity> = query.iter(world).map(|c| c.parent()).collect();
        parents.into_iter().filter(|&p| world.get::<TargetDummy>(p).is_some()).count()
    };
    let chips_live = world.query::<&fps_prototype::impact::Chipping>().iter(world).count();
    let puffs_live = world.query::<&fps_prototype::impact::Dust>().iter(world).count();
    let thumps_playing = world
        .query::<&bevy::audio::SpatialAudioSink>()
        .iter(world)
        .filter(|sink| !sink.empty() && !sink.is_paused())
        .count();
    let sound_log = world.query::<&Gun>().iter(world).next().map(|g| g.sound_log).unwrap_or_default();
    let (player, feet, mantles, mantling) = world
        .query::<(&Transform, &FpsCamera)>()
        .iter(world)
        .next()
        .map_or(((0.0, 0.0), 0.0, 0, false), |(t, c)| ((t.translation.x, t.translation.z), c.feet(), c.mantles(), c.mantling()));
    let menu = world.resource::<Menu>();
    let (menu_open, menu_rebinding, menu_message) = (menu.open, menu.rebinding, menu.message.clone());
    let controls = world.resource::<Controls>().clone();
    Snapshot {
        menu_open,
        menu_rebinding,
        menu_message,
        controls,
        player,
        feet,
        mantles,
        mantling,
        impacts,
        holes_live,
        holes_on_target,
        chips_live,
        puffs_live,
        thumps_playing,
        sound_log,
        bolt,
        reload_seconds,
        ammo,
        reloading,
        gun_lowered,
        stance,
        eye_height,
        ground_speed,
        airborne,
        spread,
        bullets,
        worst_shot_error,
        flash_visible,
        camera_pitch,
        aiming,
        aim_blend,
        last_shot_origin,
        cursor_captured,
        shot_sound_loaded,
        shots_fired,
        playing_sounds,
        dummy_health,
        dummy_hits,
    }
}
