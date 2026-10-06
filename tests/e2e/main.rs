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
use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, ExitCondition, PrimaryWindow};
use bevy::winit::WinitPlugin;
use cucumber::World as _;
use cucumber::writer::Stats;
use fps_prototype::player::{spawn_player, CursorIntent, FpsCamera};
use fps_prototype::target::{spawn_dummy, TargetDummy};
use fps_prototype::weapon::{spawn_gun, Bullet, BulletAssets, Gun};
use fps_prototype::map::TerrainMap;
use fps_prototype::GamePlugin;

pub enum Command {
    Press(MouseButton),
    Release(MouseButton),
    Tap(MouseButton),
    SetCursorCaptured(bool),
    LoadRoom,
    Snapshot(Sender<Snapshot>),
    Quit { failed: bool },
}

#[derive(Debug, Clone, Copy)]
pub struct Snapshot {
    pub cursor_captured: bool,
    pub shot_sound_loaded: bool,
    pub shots_fired: u32,
    pub playing_sounds: usize,
    pub dummy_health: f32,
    pub dummy_hits: u32,
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
    reply_rx.recv_timeout(Duration::from_secs(5)).expect("no snapshot reply")
}

#[derive(Resource)]
struct Bridge {
    rx: Mutex<Receiver<Command>>,
}

#[derive(Resource, Default)]
struct PendingTaps(Vec<MouseButton>);

#[derive(Resource, Default)]
struct HeldButtons(Vec<MouseButton>);

fn main() -> ExitCode {
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
        .add_systems(PreUpdate, (isolate_input, drive).chain().after(InputSystems))
        .add_systems(Last, release_taps);
    app
}

// Real keyboard and mouse input is discarded so only test commands reach the game.
fn isolate_input(
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut motion: ResMut<Messages<MouseMotion>>,
    held: Res<HeldButtons>,
) {
    mouse.reset_all();
    keys.reset_all();
    motion.clear();
    for button in &held.0 {
        mouse.press(*button);
    }
}

fn release_taps(mut input: ResMut<ButtonInput<MouseButton>>, mut taps: ResMut<PendingTaps>) {
    for button in taps.0.drain(..) {
        input.release(button);
    }
}

fn drive(world: &mut World) {
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
            }
            Command::Release(button) => {
                world.resource_mut::<ButtonInput<MouseButton>>().release(button);
                world.resource_mut::<HeldButtons>().0.retain(|held| *held != button);
            }
            Command::Tap(button) => {
                world.resource_mut::<ButtonInput<MouseButton>>().press(button);
                world.resource_mut::<PendingTaps>().0.push(button);
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
        Some(assets) => world
            .resource::<AssetServer>()
            .is_loaded_with_dependencies(assets.shot_sound.id()),
        None => false,
    };
    Snapshot {
        cursor_captured,
        shot_sound_loaded,
        shots_fired,
        playing_sounds,
        dummy_health,
        dummy_hits,
    }
}
