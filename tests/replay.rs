//! Recording a session and playing it back: the whole game, with its zombies, run twice, once from a script and once
//! from the recording that made, must end up in the same place.
//!
//! Each run is its own headless app (no window, no sound output), stepped by hand with a fixed length of frame so that
//! nothing depends on how fast the machine is.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::ecs::message::Messages;
use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::time::{TimeSystems, TimeUpdateStrategy};
use bevy::window::{CursorGrabMode, CursorOptions, ExitCondition, PrimaryWindow};
use bevy::winit::WinitPlugin;
use fps_prototype::controls::{Bind, ControlsSet, Keyboard};
use fps_prototype::health::{HealthPlugin, PlayerHealth};
use fps_prototype::inventory::Inventory;
use fps_prototype::inventory_ui::{DropQueue, InventoryScreen};
use fps_prototype::menu::Menu;
use fps_prototype::map::TerrainMap;
use fps_prototype::playback::{Mode, Playback, PlaybackPlugin};
use fps_prototype::player::FpsCamera;
use fps_prototype::recorder::{Recorder, RecordSet};
use fps_prototype::recording::Recording;
use fps_prototype::weapon::Gun;
use fps_prototype::zombie::{Zombie, ZombiePlugin};
use fps_prototype::GamePlugin;

/// What the player does in one frame of the script.
#[derive(Clone, Default)]
struct Step {
    escape: bool,
    seconds: f32,
    keys: Vec<Bind>,
    left: bool,
    right: bool,
    motion: Vec2,
}

/// Something the player does on a screen with the mouse, which the recording does not see as a click.
#[derive(Clone, Copy)]
enum OnScreen {
    /// Throws the first stack in the pockets out, and starts a dressing healing.
    ThrowAndHeal,
    /// Clicks Resume in the menu.
    Resume,
}

#[derive(Resource, Default)]
struct Script {
    steps: Vec<Step>,
    next: usize,
    on_screen: Vec<(usize, OnScreen)>,
}

/// A long play: it walks and runs about, looks around, shoots, reloads, aims and jumps, with zombies after it.
fn script(frames: usize) -> Vec<Step> {
    let forward = Bind::Char('w');
    (0..frames)
        .map(|i| {
            let phase = i / 60;
            let mut keys = Vec::new();
            if phase % 4 != 3 {
                keys.push(forward);
            }
            if phase % 3 == 1 {
                keys.push(Bind::Shift);
            }
            if phase % 5 == 2 {
                keys.push(Bind::Char('d'));
            }
            if i % 97 == 5 {
                keys.push(Bind::Space);
            }
            if i % 211 == 100 {
                keys.push(Bind::Char('r'));
            }
            // Frames of different lengths, as real ones are.
            let seconds = [0.0166, 0.0171, 0.0158, 0.033, 0.0167][i % 5];
            Step {
                escape: false,
                seconds,
                keys,
                left: (i / 7) % 3 == 0,
                right: i % 150 > 110,
                motion: Vec2::new(((i * 37) % 23) as f32 - 11.0, ((i * 11) % 9) as f32 - 4.0) * 0.8,
            }
        })
        .collect()
}

fn headless() -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin { primary_window: None, exit_condition: ExitCondition::DontExit, close_when_requested: false, ..default() })
            .disable::<WinitPlugin>(),
    );
    app.world_mut().spawn((Window::default(), PrimaryWindow, CursorOptions { grab_mode: CursorGrabMode::Locked, visible: false, ..default() }));
    app.insert_resource(TerrainMap::flat(0.0))
        .add_plugins((GamePlugin, HealthPlugin, ZombiePlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(16)));
    app
}

/// The script's input, in place of a keyboard and a mouse; the recorder reads it as it would theirs.
fn scripted(mut script: ResMut<Script>, mut keyboard: ResMut<Keyboard>, mut mouse: ResMut<ButtonInput<MouseButton>>, mut board: ResMut<ButtonInput<KeyCode>>, mut motion: ResMut<Messages<MouseMotion>>) {
    let Some(step) = script.steps.get(script.next).cloned() else { return };
    script.next += 1;
    let held: Vec<Bind> = keyboard.frame().held;
    for key in &held {
        if !step.keys.contains(key) {
            keyboard.release(*key);
        }
    }
    for key in &step.keys {
        if !held.contains(key) {
            keyboard.press(*key);
        }
    }
    for (button, wanted) in [(MouseButton::Left, step.left), (MouseButton::Right, step.right)] {
        match (mouse.pressed(button), wanted) {
            (false, true) => mouse.press(button),
            (true, false) => mouse.release(button),
            _ => {}
        }
    }
    if step.escape {
        board.press(KeyCode::Escape);
    }
    if step.motion != Vec2::ZERO {
        motion.write(MouseMotion { delta: step.motion });
    }
}

/// What the screens' own systems do after the game has run, when the mouse has clicked on them.
fn on_screen(script: Res<Script>, mut menu: ResMut<Menu>, mut inventory: ResMut<Inventory>, mut thrown: ResMut<DropQueue>, mut health: ResMut<PlayerHealth>) {
    let frame = script.next.saturating_sub(1);
    for &(at, what) in &script.on_screen {
        if at != frame {
            continue;
        }
        match what {
            OnScreen::ThrowAndHeal => {
                let id = inventory.pockets.items[0].item.id;
                let item = inventory.take(id).expect("something is in the pockets");
                thrown.0.push(item);
                health.hurt(10.0);
                assert!(health.heal_over_time(30.0, 12.0));
            }
            OnScreen::Resume => menu.close(),
        }
    }
}

fn scripted_time(script: Res<Script>, mut strategy: ResMut<TimeUpdateStrategy>) {
    if let Some(step) = script.steps.get(script.next) {
        *strategy = TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(step.seconds));
    }
}

/// Where everything ended up, to the bit.
#[derive(Debug, PartialEq)]
struct Outcome {
    player: [u32; 7],
    health: String,
    ammo: u32,
    shots: u32,
    inventory: String,
    zombies: Vec<[u32; 3]>,
    /// Things lying about to be picked up.
    lying: usize,
    screens: (bool, bool),
}

fn outcome(app: &mut App) -> Outcome {
    let world = app.world_mut();
    let transform = world.query_filtered::<&Transform, With<FpsCamera>>().single(world).unwrap();
    let (t, r) = (transform.translation, transform.rotation);
    let player = [t.x.to_bits(), t.y.to_bits(), t.z.to_bits(), r.x.to_bits(), r.y.to_bits(), r.z.to_bits(), r.w.to_bits()];
    let gun = world.query::<&Gun>().single(world).unwrap();
    let (ammo, shots) = (gun.ammo, gun.shots_fired);
    let mut zombies: Vec<[u32; 3]> = world.query_filtered::<&Transform, With<Zombie>>().iter(world).map(|t| [t.translation.x.to_bits(), t.translation.y.to_bits(), t.translation.z.to_bits()]).collect();
    zombies.sort();
    let lying = world.query::<&fps_prototype::loot::Pickup>().iter(world).count();
    let screens = (world.resource::<Menu>().open, world.resource::<InventoryScreen>().open);
    Outcome { player, health: world.resource::<PlayerHealth>().to_text(), ammo, shots, inventory: world.resource::<Inventory>().to_text(), zombies, lying, screens }
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("replay-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Plays `frames` frames of the script, recording them to `file`.
fn play_and_record(frames: usize, file: &Path) -> Outcome {
    play_script_and_record(script(frames), Vec::new(), file)
}

fn play_script_and_record(steps: Vec<Step>, on_screen: Vec<(usize, OnScreen)>, file: &Path) -> Outcome {
    let frames = steps.len();
    let mut app = headless();
    app.init_resource::<Script>()
        .add_systems(First, scripted_time.before(TimeSystems))
        .add_systems(PreUpdate, scripted.after(ControlsSet).before(RecordSet))
        .add_systems(PostUpdate, self::on_screen);
    app.world_mut().resource_mut::<Script>().steps = steps;
    app.world_mut().resource_mut::<Script>().on_screen = on_screen;
    app.world_mut().resource_mut::<Recorder>().start_to(file.to_path_buf());
    app.finish();
    app.cleanup();
    for _ in 0..frames {
        app.update();
    }
    app.world_mut().resource_mut::<Recorder>().stop();
    app.update();
    assert!(!app.world().resource::<Recorder>().is_recording(), "{}", app.world().resource::<Recorder>().message);
    outcome(&mut app)
}

/// Plays `recording` through, and says where it ended and how far it was from the recording on the way.
fn replay(recording: Recording, strict: bool) -> (Outcome, fps_prototype::playback::Drift) {
    let frames = recording.frames.len();
    let mut app = headless();
    app.add_plugins(PlaybackPlugin { recording, strict });
    app.finish();
    app.cleanup();
    for _ in 0..frames {
        app.update();
    }
    let playback = app.world().resource::<Playback>();
    assert_eq!(playback.mode, Mode::Finished, "every frame has been played");
    let drift = playback.drift;
    (outcome(&mut app), drift)
}

#[test]
fn a_replay_ends_where_the_play_did_with_every_check_along_the_way_exact() {
    let dir = temp("exact");
    let file = dir.join("play.pir");
    let frames = 1800;
    let played = play_and_record(frames, &file);

    let recording = Recording::read(&file).expect("what was recorded can be read");
    assert_eq!(recording.frames.len(), frames + 1, "every frame was kept, and the one the stop was asked in");
    assert!(recording.header.start.fresh, "recording from the first frame is a fresh game");
    assert!(recording.frames.iter().filter(|f| f.check.is_some()).count() >= frames / 31 - 1, "positions were kept along the way");

    let (replayed, drift) = replay(recording, true);
    assert_eq!(drift.drifted, 0, "no position was off the recording: {drift:?}");
    assert!(drift.checks > 50);
    assert_eq!(replayed, played, "and it ends in the same place");
    assert!(!played.zombies.is_empty(), "with zombies, which did the same");
    assert!(played.shots > 5, "and a gun that was fired: {} shots", played.shots);
    assert_ne!(played.health, PlayerHealth::default().to_text(), "and a player the zombies got to");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_zombies_dice_are_part_of_what_is_recorded() {
    // The same play with other dice in the zombies' hands goes differently: so the agreement above is the dice
    // being recorded, not the zombies having nothing to decide.
    let dir = temp("dice");
    let file = dir.join("play.pir");
    let played = play_and_record(1800, &file);
    let mut recording = Recording::read(&file).unwrap();
    let (state, count) = recording.header.start.dice.expect("the zombies' dice are kept");
    recording.header.start.dice = Some((state ^ 0x5555_5555_5555, count));
    let (replayed, _) = replay(recording, true);
    assert_ne!(replayed, played);
    let _ = std::fs::remove_dir_all(&dir);
}

fn press_escape(app: &mut App) {
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::input::ButtonState;
    let window = app.world_mut().query_filtered::<Entity, With<PrimaryWindow>>().single(app.world()).unwrap();
    app.world_mut().write_message(KeyboardInput { key_code: KeyCode::Escape, logical_key: Key::Escape, state: ButtonState::Pressed, text: None, repeat: false, window });
}

#[test]
fn what_was_done_on_the_screens_is_done_again_in_the_replay() {
    let dir = temp("screens");
    let file = dir.join("play.pir");
    let mut steps = script(900);
    // The inventory opens (Tab), things are moved about on it with the mouse, and it is closed (Tab).
    steps[200].keys.push(Bind::Tab);
    steps[350].keys.push(Bind::Tab);
    // The menu opens (Escape) and is closed by a click.
    steps[500].escape = true;
    let on_screen = vec![(260, OnScreen::ThrowAndHeal), (560, OnScreen::Resume)];
    let played = play_script_and_record(steps, on_screen, &file);
    assert!(played.lying >= 1, "what was thrown is on the ground");
    assert_eq!(played.screens, (false, false), "and both screens were closed again");

    let recording = Recording::read(&file).unwrap();
    use fps_prototype::recording::Event;
    let events: Vec<&Event> = recording.frames.iter().flat_map(|f| &f.events).collect();
    assert!(events.iter().any(|e| matches!(e, Event::Inventory(_))), "the inventory screen's work was kept");
    assert!(events.iter().any(|e| matches!(e, Event::Thrown(_))));
    assert!(events.iter().any(|e| matches!(e, Event::Health(_))));
    assert!(events.iter().any(|e| matches!(e, Event::Screens { menu: false, .. })), "and the menu being closed by a click");
    assert!(recording.frames.iter().any(|f| f.input.escape));
    let (replayed, drift) = replay(recording, true);
    assert_eq!(drift.drifted, 0, "{drift:?}");
    assert_eq!(replayed, played);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_paused_replay_stands_still_and_goes_on_exactly_as_if_it_had_not_stopped() {
    let dir = temp("pause");
    let file = dir.join("play.pir");
    let frames = 900;
    play_and_record(frames, &file);
    let recording = Recording::read(&file).unwrap();
    let (straight, _) = replay(recording.clone(), true);

    let mut app = headless();
    app.add_plugins(PlaybackPlugin { recording, strict: true });
    app.finish();
    app.cleanup();
    for _ in 0..300 {
        app.update();
    }
    press_escape(&mut app);
    // The frame it is pressed in plays; the next ones are held.
    app.update();
    app.update();
    assert!(app.world().resource::<Playback>().paused);
    let held = (outcome(&mut app), app.world().resource::<Playback>().seconds_played());
    for _ in 0..40 {
        app.update();
    }
    let still = (outcome(&mut app), app.world().resource::<Playback>().seconds_played());
    assert_eq!(held, still, "nothing moves while it is paused");
    press_escape(&mut app);
    app.update();
    app.update();
    assert!(!app.world().resource::<Playback>().paused);
    while app.world().resource::<Playback>().mode == Mode::Running {
        app.update();
    }
    assert_eq!(outcome(&mut app), straight, "and it ends where an uninterrupted replay does");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_recording_made_part_way_through_a_game_says_so() {
    let dir = temp("midway");
    let file = dir.join("play.pir");
    let mut app = headless();
    app.init_resource::<Script>().add_systems(First, scripted_time.before(TimeSystems)).add_systems(PreUpdate, scripted.after(ControlsSet).before(RecordSet));
    app.world_mut().resource_mut::<Script>().steps = script(200);
    app.finish();
    app.cleanup();
    for _ in 0..100 {
        app.update();
    }
    app.world_mut().resource_mut::<Recorder>().start_to(file.clone());
    for _ in 0..50 {
        app.update();
    }
    app.world_mut().resource_mut::<Recorder>().stop();
    app.update();
    let recording = Recording::read(&file).unwrap();
    assert!(!recording.header.start.fresh);
    assert_eq!(recording.frames.len(), 51, "the frame the stop was asked in is the last");
    // Where the player was when recording began is where the replay begins.
    let first = recording.header.start.pose;
    let (replayed, _) = replay(recording, false);
    assert_ne!(replayed.player[0], first.position.x.to_bits(), "and the replay goes on from there");
    let _ = std::fs::remove_dir_all(&dir);
}

#[derive(Resource, Default)]
struct SeenAspects(Vec<f32>);

fn watch_aspect(camera: Query<&Projection, With<FpsCamera>>, mut seen: ResMut<SeenAspects>) {
    seen.0.push(fps_prototype::recorder::aspect_of(camera.single().ok()));
}

#[test]
fn a_replay_is_told_the_shape_of_the_window_the_play_was_in() {
    // Which zombies may appear depends on the camera's shape, so a replay in a window of another shape has to see
    // the play's.
    let dir = temp("aspect");
    let file = dir.join("play.pir");
    play_and_record(300, &file);
    let mut recording = Recording::read(&file).unwrap();
    for frame in recording.frames.iter_mut().skip(5) {
        frame.aspect = 2.35;
    }
    let frames = recording.frames.len();
    let mut app = headless();
    app.add_plugins(PlaybackPlugin { recording, strict: true }).init_resource::<SeenAspects>().add_systems(Update, watch_aspect);
    app.finish();
    app.cleanup();
    for _ in 0..frames {
        app.update();
    }
    let seen = &app.world().resource::<SeenAspects>().0;
    assert_eq!(seen.len(), frames);
    assert!(seen[6..].iter().all(|&a| a == 2.35), "{:?}", &seen[6..12]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_replay_that_has_been_put_off_course_is_put_back() {
    let dir = temp("drift");
    let file = dir.join("play.pir");
    play_and_record(400, &file);
    let mut recording = Recording::read(&file).unwrap();
    // A recording that says the player was somewhere else at the first check.
    let at = recording.frames.iter().position(|f| f.check.is_some() && f.check.unwrap().position != Vec3::ZERO).unwrap();
    let mut pose = recording.frames[at].check.unwrap();
    pose.position += Vec3::new(5.0, 0.0, 0.0);
    recording.frames[at].check = Some(pose);
    let (_, drift) = replay(recording, false);
    assert!(drift.resyncs >= 1 && drift.worst >= 4.9, "{drift:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
