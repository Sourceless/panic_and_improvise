//! Making a recording: while it is on, each frame's length and what the player did in it are written to a file
//! (see `recording` for what is in one and why). It is started and stopped from the menu, or from the start with
//! `--record`.
//!
//! The input is taken where the game takes it from (the keyboard as the game reads it, the mouse's buttons and
//! movement), early in the frame; what the screens did that the input does not explain (the inventory, the menu)
//! is taken at the end of the frame, as the state they left, to be put back at the start of the next.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::diagnostic::FrameCount;
use bevy::ecs::schedule::{MultiThreadedExecutor, Schedules, SingleThreadedExecutor};
use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;

use crate::controls::{Controls, ControlsSet, Keyboard};
use crate::health::PlayerHealth;
use crate::inventory::Inventory;
use crate::inventory_ui::{DropQueue, InventoryScreen};
use crate::map::TerrainMap;
use crate::menu::Menu;
use crate::player::{Captured, FpsCamera};
use crate::recording::{recordings_dir, recording_name, sorted_buttons, Event, Frame, Header, Input, Pose, Start, Writer};
use crate::weapon::Gun;
use crate::zombie::ZombieDice;

/// How many frames between the positions kept to tell a replay from drifting.
pub const CHECK_EVERY: u32 = 30;

/// A game that is no more than this many frames old is new enough for a replay to have everything as it was.
const FRESH_FRAMES: u32 = 8;

/// Systems that read input for a recording run in this set: whatever supplies the input (a test) goes before it.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub struct RecordSet;

/// What the screens were last time they were looked at, to tell when they have changed.
struct Watch {
    menu: bool,
    inventory_open: bool,
    inventory: Inventory,
    health: PlayerHealth,
    /// The items in the queue of things thrown out, as of the last frame.
    thrown: Vec<u32>,
}

struct Session {
    writer: Writer,
    /// This frame's length, input and the camera's shape, taken early in it, written when it has finished.
    sample: Option<(u64, Input, f32)>,
    /// What changes at the start of the next frame.
    carry: Vec<Event>,
    watch: Watch,
    nanos: u64,
    until_check: u32,
}

#[derive(Resource, Default)]
pub struct Recorder {
    /// Recording is to start with the next frame, to this file (or to a new one in the usual place).
    start: Option<Option<PathBuf>>,
    stopping: bool,
    session: Option<Session>,
    /// What happened last, in words, for the menu: where a recording was saved, or why it could not be made.
    pub message: String,
}

impl Recorder {
    /// Records from the next frame, into a new file in the usual place.
    pub fn start(&mut self) {
        if !self.wanted() {
            self.start = Some(None);
        }
    }

    /// Records from the next frame, into `path`.
    pub fn start_to(&mut self, path: PathBuf) {
        if !self.wanted() {
            self.start = Some(Some(path));
        }
    }

    /// Stops at the end of this frame.
    pub fn stop(&mut self) {
        if self.session.is_some() {
            self.stopping = true;
        }
        self.start = None;
    }

    /// Whether it is recording, or about to.
    pub fn wanted(&self) -> bool {
        self.session.is_some() || self.start.is_some()
    }

    pub fn is_recording(&self) -> bool {
        self.session.is_some()
    }

    /// How long the recording being made has gone on for, seconds.
    pub fn seconds(&self) -> f32 {
        self.session.as_ref().map_or(0.0, |s| (s.nanos as f64 / 1e9) as f32)
    }

    pub fn path(&self) -> Option<&std::path::Path> {
        self.session.as_ref().map(|s| s.writer.path.as_path())
    }
}

/// `1:05` for sixty-five seconds.
pub fn clock(seconds: f32) -> String {
    let whole = seconds.max(0.0) as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// The schedules the game's own systems run in. Where two systems want the same thing and nothing says which is
/// first, a game on several threads picks one differently from frame to frame; that would be a different game each
/// time, and no replay of it. One thread runs them in the one order, always. (A recording is made, and played, that
/// way; ordinary play keeps the threads.)
pub fn run_in_order(schedules: &mut Schedules, in_order: bool) {
    use bevy::ecs::schedule::ScheduleLabel;
    for label in [PreUpdate.intern(), Update.intern(), Last.intern()] {
        if let Some(schedule) = schedules.get_mut(label) {
            if in_order {
                schedule.set_executor(SingleThreadedExecutor::new());
            } else {
                schedule.set_executor(MultiThreadedExecutor::new());
            }
        }
    }
}

pub struct RecorderPlugin;

impl Plugin for RecorderPlugin {
    fn build(&self, app: &mut App) {
        let mut recorder = Recorder::default();
        // `--record` (which says so by the environment, like the other flags) records from the start.
        if let Some(value) = std::env::var_os("FPS_RECORD") {
            match value.to_str() {
                Some("1") | Some("") | None => recorder.start(),
                Some(_) => recorder.start_to(PathBuf::from(value)),
            }
        }
        app.insert_resource(recorder)
            .configure_sets(PreUpdate, RecordSet.after(ControlsSet))
            .add_systems(First, choose_executor)
            .add_systems(PreUpdate, (begin_recording, sample_input).chain().in_set(RecordSet))
            .add_systems(Last, finish_frame);
    }
}

/// At the start of a frame, before any of the schedules it runs: one thread while a recording is made or played (a
/// schedule cannot be changed while it is running, so this is done before them; and a recording starts only once it
/// has been, in the same frame), the threads back after.
fn choose_executor(recorder: Res<Recorder>, replay: Option<Res<crate::playback::Playback>>, mut schedules: ResMut<Schedules>, mut in_order: Local<bool>) {
    let wanted = recorder.wanted() || replay.is_some_and(|r| r.driving());
    if wanted != *in_order {
        run_in_order(&mut schedules, wanted);
        *in_order = wanted;
    }
}

/// How wide the camera's view is for its height; a view that is not a perspective one is taken as the usual shape.
pub fn aspect_of(projection: Option<&Projection>) -> f32 {
    match projection {
        Some(Projection::Perspective(p)) => p.aspect_ratio,
        _ => 16.0 / 9.0,
    }
}

fn camera_projection(world: &mut World) -> Option<Projection> {
    world.query_filtered::<&Projection, With<FpsCamera>>().iter(world).next().cloned()
}

fn pose_of(world: &mut World) -> Option<Pose> {
    let mut query = world.query::<(&Transform, &FpsCamera)>();
    query.iter(world).next().map(|(t, cam)| Pose { position: t.translation, yaw: cam.yaw, pitch: cam.pitch })
}

/// Opens the file and writes how the game is now, if a recording was asked for.
fn begin_recording(world: &mut World) {
    let Some(request) = world.resource_mut::<Recorder>().start.take() else { return };
    // A replay is not recorded again: it would only be the same recording.
    if world.get_resource::<crate::playback::Playback>().is_some() {
        world.resource_mut::<Recorder>().message = "A replay cannot be recorded.".to_string();
        return;
    }
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let path = request.unwrap_or_else(|| recordings_dir().join(recording_name(seconds)));
    let Some(pose) = pose_of(world) else {
        world.resource_mut::<Recorder>().message = "Could not start: there is no player yet.".to_string();
        return;
    };
    let health = world.get_resource::<PlayerHealth>().cloned().unwrap_or_default();
    let inventory = world.resource::<Inventory>().clone();
    let mut guns = world.query::<&Gun>();
    let gun_rng = guns.iter(world).next().map_or(0, |g| g.rng_state());
    let frame = world.get_resource::<FrameCount>().map_or(0, |f| f.0);
    let menu_open = world.resource::<Menu>().open;
    let inventory_open = world.resource::<InventoryScreen>().open;
    let header = Header {
        game: crate::VERSION.to_string(),
        seed: world.resource::<TerrainMap>().seed,
        controls: world.resource::<Controls>().clone(),
        start: Start {
            pose,
            health: health.clone(),
            inventory: inventory.clone(),
            dice: world.get_resource::<ZombieDice>().map(ZombieDice::state),
            gun_rng,
            menu_open,
            inventory_open,
            captured: world.resource::<Captured>().0,
            aspect: aspect_of(camera_projection(world).as_ref()),
            fresh: frame <= FRESH_FRAMES,
        },
    };
    match Writer::create(&path, &header) {
        Ok(writer) => {
            let thrown = world.get_resource::<DropQueue>().map(|q| q.0.iter().map(|i| i.id).collect()).unwrap_or_default();
            let watch = Watch { menu: menu_open, inventory_open, inventory, health, thrown };
            let mut recorder = world.resource_mut::<Recorder>();
            recorder.message = format!("Recording to {}", path.display());
            recorder.session = Some(Session { writer, sample: None, carry: Vec::new(), watch, nanos: 0, until_check: 0 });
            recorder.stopping = false;
        }
        Err(error) => {
            world.resource_mut::<Recorder>().message = format!("Could not record to {}: {error}", path.display());
        }
    }
}

/// Takes the frame's length and what the player is doing, as the game is about to read them.
fn sample_input(
    mut recorder: ResMut<Recorder>,
    time: Res<Time>,
    keys: Res<Keyboard>,
    mouse: Res<ButtonInput<MouseButton>>,
    board: Res<ButtonInput<KeyCode>>,
    mut motion: MessageReader<MouseMotion>,
    camera: Query<&Projection, With<FpsCamera>>,
) {
    // Read every frame, so that a recording that starts later does not begin with a backlog.
    let mut moved = Vec2::ZERO;
    for event in motion.read() {
        moved += event.delta;
    }
    let Some(session) = recorder.session.as_mut() else { return };
    let input = Input {
        keys: keys.frame(),
        buttons_held: sorted_buttons(mouse.get_pressed().copied().collect()),
        buttons_pressed: sorted_buttons(mouse.get_just_pressed().copied().collect()),
        buttons_released: sorted_buttons(mouse.get_just_released().copied().collect()),
        motion: moved,
        escape: board.just_pressed(KeyCode::Escape),
    };
    session.sample = Some((time.delta().as_nanos() as u64, input, aspect_of(camera.single().ok())));
}

/// Writes the frame that has just run, and notes what the screens changed for the start of the next.
#[allow(clippy::too_many_arguments)]
fn finish_frame(
    mut recorder: ResMut<Recorder>,
    captured: Res<Captured>,
    menu: Res<Menu>,
    screen: Res<InventoryScreen>,
    inventory: Res<Inventory>,
    health: Option<Res<PlayerHealth>>,
    queue: Option<Res<DropQueue>>,
    camera: Query<(&Transform, &FpsCamera)>,
    mut exits: MessageReader<AppExit>,
) {
    let leaving = exits.read().count() > 0;
    let stopping = std::mem::take(&mut recorder.stopping) || leaving;
    let Some(session) = recorder.session.as_mut() else { return };
    let mut failed = None;
    if let Some((nanos, input, aspect)) = session.sample.take() {
        session.nanos += nanos;
        let check = if session.until_check == 0 {
            session.until_check = CHECK_EVERY;
            camera.single().ok().map(|(t, cam)| Pose { position: t.translation, yaw: cam.yaw, pitch: cam.pitch })
        } else {
            None
        };
        session.until_check = session.until_check.saturating_sub(1);
        let frame = Frame { nanos, input, captured: captured.0, aspect, events: std::mem::take(&mut session.carry), check };
        if let Err(error) = session.writer.frame(&frame) {
            failed = Some(error);
        }

        // What the screens left behind, for the start of the next frame.
        let watch = &mut session.watch;
        let ui_busy = screen.open || watch.inventory_open;
        if (menu.open, screen.open) != (watch.menu, watch.inventory_open) {
            session.carry.push(Event::Screens { menu: menu.open, inventory: screen.open });
            (watch.menu, watch.inventory_open) = (menu.open, screen.open);
        }
        if *inventory != watch.inventory {
            session.carry.push(Event::Inventory(inventory.clone()));
            watch.inventory = inventory.clone();
        }
        if let Some(health) = health.as_deref() {
            // Health is changed by the inventory screen (using a dressing) as well as by the game; it is only
            // noted while the screen is in use, which is the only time the game cannot work the change out.
            if ui_busy && *health != watch.health {
                session.carry.push(Event::Health(health.clone()));
            }
            if *health != watch.health {
                watch.health = health.clone();
            }
        }
        if let Some(queue) = queue.as_deref() {
            for item in &queue.0 {
                if !watch.thrown.contains(&item.id) {
                    session.carry.push(Event::Thrown(*item));
                }
            }
            watch.thrown = queue.0.iter().map(|i| i.id).collect();
        }
    }
    if failed.is_some() || stopping {
        let Some(session) = recorder.session.take() else { return };
        let (path, seconds, frames) = (session.writer.path.clone(), (session.nanos as f64 / 1e9) as f32, session.writer.frames);
        recorder.message = match failed.map_or_else(|| session.writer.finish(), Err) {
            Ok(()) => format!("Saved {} of play ({frames} frames) to {}", clock(seconds), path.display()),
            Err(error) => format!("Could not save the recording to {}: {error}", path.display()),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_shown_in_minutes_and_seconds() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(5.9), "0:05");
        assert_eq!(clock(65.0), "1:05");
        assert_eq!(clock(3600.0), "60:00");
        assert_eq!(clock(-3.0), "0:00");
    }

    #[test]
    fn asking_to_record_is_wanted_until_it_is_withdrawn_and_only_once() {
        let mut recorder = Recorder::default();
        assert!(!recorder.wanted() && !recorder.is_recording());
        recorder.start();
        assert!(recorder.wanted(), "it is to start with the next frame");
        assert!(!recorder.is_recording(), "but is not yet");
        recorder.start_to(PathBuf::from("elsewhere.pir"));
        assert_eq!(recorder.start, Some(None), "a second asking does not change where the first goes");
        recorder.stop();
        assert!(!recorder.wanted(), "stopping before it began withdraws the asking");
    }

    #[test]
    fn what_a_camera_that_is_not_a_perspective_one_is_taken_as_is_the_usual_shape() {
        assert_eq!(aspect_of(None), 16.0 / 9.0);
        let view = Projection::Perspective(PerspectiveProjection { aspect_ratio: 2.0, ..default() });
        assert_eq!(aspect_of(Some(&view)), 2.0);
        assert_eq!(aspect_of(Some(&Projection::Orthographic(OrthographicProjection::default_3d()))), 16.0 / 9.0);
    }
}
