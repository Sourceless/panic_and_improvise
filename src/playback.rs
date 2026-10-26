//! Playing a recording: the game is started as it was (the same map, the same inventory, the same dice), and each
//! frame is given the length and the input it had when it was recorded, in place of the real clock and the real
//! keyboard and mouse. The game's own systems do the rest, so what is seen is the game running, not a film of it.
//!
//! A replay goes the same way as the play only if the game is deterministic, which this one is, to the extent that
//! the checks kept in a recording show: every so often the player's position is compared with the recorded one, the
//! difference is counted, and (unless `--strict`) the player is put back where they were so that one slip does not
//! spoil the rest.
//!
//! Escape pauses (and its menu can end the replay); real keys and the mouse do nothing else until the replay is
//! over and the viewer takes control.

use std::time::Duration;

use bevy::app::AppExit;
use bevy::ecs::message::Messages;
use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::time::{TimeSystems, TimeUpdateStrategy};
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use bevy_egui::{egui, EguiContexts};

use crate::controls::{ControlsSet, Keyboard, KeyboardFrame};
use crate::health::PlayerHealth;
use crate::inventory::Inventory;
use crate::inventory_ui::{DropQueue, InventoryScreen};
use crate::map::TerrainMap;
use crate::menu::Menu;
use crate::player::{spawn_player, Captured, FpsCamera};
use crate::recorder::clock;
use crate::recording::{Event, Frame, Input, Pose, Recording};
use crate::weapon::{spawn_gun, Gun};
use crate::zombie::ZombieDice;

/// How far (metres) the player can be from where they were before a replay counts as having drifted.
pub const DRIFT: f32 = 0.05;
/// How far before the player is put back where they were.
pub const RESYNC: f32 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Running,
    /// Every frame has been played; the viewer chooses what next.
    Finished,
    /// The viewer has taken over: input is real again.
    Released,
}

/// How well the replay is keeping to the recording.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Drift {
    /// Positions compared, and how many of them were off by more than `DRIFT`.
    pub checks: u32,
    pub drifted: u32,
    /// Times the player was put back.
    pub resyncs: u32,
    /// The worst difference seen, and the latest (metres).
    pub worst: f32,
    pub latest: f32,
}

#[derive(Resource)]
pub struct Playback {
    pub recording: Recording,
    /// The frame to run next, and the one running now.
    next: usize,
    current: Option<usize>,
    /// The viewer has paused (Escape).
    pub paused: bool,
    /// Escape has been pressed: the pause (or the end of it) starts with the next frame, so that no frame is given
    /// its length without its input.
    toggle_pause: bool,
    /// Never put the player back: for finding out whether a replay goes the same way.
    pub strict: bool,
    pub mode: Mode,
    pub drift: Drift,
    /// What was held at the last frame played, to be held while paused.
    held: Input,
    played: u64,
    /// A frame whose position is yet to be compared.
    check: Option<usize>,
}

impl Playback {
    pub fn new(recording: Recording, strict: bool) -> Playback {
        let finished = recording.frames.is_empty();
        Playback {
            recording,
            next: 0,
            current: None,
            paused: false,
            toggle_pause: false,
            strict,
            mode: if finished { Mode::Finished } else { Mode::Running },
            drift: Drift::default(),
            held: Input::default(),
            played: 0,
            check: None,
        }
    }

    /// Whether the recording is running the game: false once the viewer has taken over.
    pub fn driving(&self) -> bool {
        self.mode != Mode::Released
    }

    /// Whether the mouse was captured at the frame being played (nothing once the viewer has taken over).
    pub fn captured_now(&self) -> Option<bool> {
        if !self.driving() {
            return None;
        }
        Some(self.current.map_or(self.recording.header.start.captured, |i| self.recording.frames[i].captured))
    }

    /// How long the next frame is, if it is to be played.
    fn next_length(&self) -> Option<Duration> {
        if self.mode != Mode::Running || self.paused {
            return Some(Duration::ZERO);
        }
        self.recording.frames.get(self.next).map(|f| Duration::from_nanos(f.nanos))
    }

    pub fn seconds_played(&self) -> f32 {
        (self.played as f64 / 1e9) as f32
    }

    pub fn seconds_total(&self) -> f32 {
        self.recording.seconds()
    }

    /// Takes the next frame to play, if the replay is running.
    fn take_frame(&mut self) -> Option<&Frame> {
        if self.mode != Mode::Running || self.paused {
            return None;
        }
        let Some(frame) = self.recording.frames.get(self.next) else {
            self.mode = Mode::Finished;
            return None;
        };
        self.current = Some(self.next);
        self.check = frame.check.map(|_| self.next);
        self.next += 1;
        self.played += frame.nanos;
        if self.next == self.recording.frames.len() {
            self.mode = Mode::Finished;
        }
        Some(&self.recording.frames[self.next - 1])
    }

    /// Notes how far the player is from where they were, and says whether to put them back.
    fn compare(&mut self, pose: &Pose, now: Vec3) -> bool {
        let off = pose.position.distance(now);
        let drift = &mut self.drift;
        drift.checks += 1;
        drift.latest = off;
        drift.worst = drift.worst.max(off);
        if off > DRIFT {
            drift.drifted += 1;
        }
        let resync = !self.strict && off > RESYNC;
        if resync {
            drift.resyncs += 1;
        }
        resync
    }
}

pub struct PlaybackPlugin {
    pub recording: Recording,
    pub strict: bool,
}

impl Plugin for PlaybackPlugin {
    fn build(&self, app: &mut App) {
        let playback = Playback::new(self.recording.clone(), self.strict);
        let first = playback.next_length().unwrap_or(Duration::ZERO);
        let start = &self.recording.header.start;
        // Added after the plugins that make these, so these are the ones kept.
        app.insert_resource(self.recording.header.controls.clone())
            .insert_resource(start.inventory.clone())
            .insert_resource(start.health.clone())
            .insert_resource(TimeUpdateStrategy::ManualDuration(first));
        if let Some(dice) = start.dice {
            app.insert_resource(ZombieDice::from_state(dice));
        }
        app.insert_resource(playback)
            .add_systems(Startup, put_back_as_it_was.after(spawn_player).after(spawn_gun))
            .add_systems(First, set_frame_length.before(TimeSystems))
            .add_systems(PreUpdate, play_frame.after(ControlsSet))
            .add_systems(Last, (compare_position, set_cursor).chain());
    }
}

/// The player, the gun and the screens as they were when recording began.
#[allow(clippy::too_many_arguments)]
fn put_back_as_it_was(
    playback: Res<Playback>,
    map: Res<TerrainMap>,
    mut cameras: Query<(&mut Transform, &mut FpsCamera)>,
    mut guns: Query<&mut Gun>,
    mut menu: ResMut<Menu>,
    mut screen: ResMut<InventoryScreen>,
    mut captured: ResMut<Captured>,
) {
    let start = &playback.recording.header.start;
    if let Ok((mut transform, mut cam)) = cameras.single_mut() {
        transform.translation = start.pose.position;
        cam.yaw = start.pose.yaw;
        cam.pitch = start.pose.pitch;
        transform.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
        let at = Vec2::new(start.pose.position.x, start.pose.position.z);
        cam.floor = map.floor_height_at(at);
        cam.view_feet = start.pose.position.y - cam.eye_height;
    }
    if let Ok(mut gun) = guns.single_mut() {
        gun.set_rng_state(start.gun_rng);
    }
    menu.open = start.menu_open;
    screen.open = start.inventory_open;
    captured.0 = start.captured;
}

/// Gives the frame about to start the length it had.
fn set_frame_length(mut playback: ResMut<Playback>, mut strategy: ResMut<TimeUpdateStrategy>) {
    if std::mem::take(&mut playback.toggle_pause) && playback.mode == Mode::Running {
        playback.paused = !playback.paused;
    }
    *strategy = match playback.next_length() {
        Some(length) if playback.driving() => TimeUpdateStrategy::ManualDuration(length),
        _ => TimeUpdateStrategy::Automatic,
    };
}

fn apply_input(input: &Input, keyboard: &mut Keyboard, mouse: &mut ButtonInput<MouseButton>, board: &mut ButtonInput<KeyCode>, motion: &mut Messages<MouseMotion>) {
    keyboard.set_frame(&input.keys);
    mouse.reset_all();
    for &button in &input.buttons_released {
        mouse.press(button);
        mouse.release(button);
    }
    for &button in &input.buttons_held {
        mouse.press(button);
    }
    for &button in input.buttons_held.iter().chain(&input.buttons_released) {
        if !input.buttons_pressed.contains(&button) {
            mouse.clear_just_pressed(button);
        }
    }
    board.reset_all();
    if input.escape {
        board.press(KeyCode::Escape);
    }
    motion.clear();
    if input.motion != Vec2::ZERO {
        motion.write(MouseMotion { delta: input.motion });
    }
}

/// What is held and nothing new: the frame a paused replay stands in.
fn held_only(input: &Input) -> Input {
    Input { keys: KeyboardFrame { held: input.keys.held.clone(), ..default() }, buttons_held: input.buttons_held.clone(), ..default() }
}

#[allow(clippy::too_many_arguments)]
fn play_frame(
    mut playback: ResMut<Playback>,
    mut keyboard: ResMut<Keyboard>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut board: ResMut<ButtonInput<KeyCode>>,
    mut motion: ResMut<Messages<MouseMotion>>,
    mut menu: ResMut<Menu>,
    mut screen: ResMut<InventoryScreen>,
    mut inventory: ResMut<Inventory>,
    mut health: Option<ResMut<PlayerHealth>>,
    mut thrown: ResMut<DropQueue>,
    mut cameras: Query<&mut Projection, With<FpsCamera>>,
) {
    if !playback.driving() {
        return;
    }
    // The viewer's own Escape, before the recording's replaces the keys.
    if board.just_pressed(KeyCode::Escape) && playback.mode == Mode::Running {
        playback.toggle_pause = true;
    }
    let Some(frame) = playback.take_frame().cloned() else {
        let held = held_only(&playback.held);
        apply_input(&held, &mut keyboard, &mut mouse, &mut board, &mut motion);
        return;
    };
    for event in &frame.events {
        match event {
            Event::Screens { menu: m, inventory: i } => {
                menu.open = *m;
                screen.open = *i;
            }
            Event::Inventory(now) => *inventory = now.clone(),
            Event::Health(now) => {
                if let Some(health) = health.as_deref_mut() {
                    *health = now.clone();
                }
            }
            Event::Thrown(item) => thrown.0.push(*item),
        }
    }
    // The shape of the window the play was in, whatever this one is: it decides which zombies may appear.
    if let Ok(mut projection) = cameras.single_mut() {
        if let Projection::Perspective(p) = &mut *projection {
            if p.aspect_ratio.to_bits() != frame.aspect.to_bits() {
                p.aspect_ratio = frame.aspect;
            }
        }
    }
    apply_input(&frame.input, &mut keyboard, &mut mouse, &mut board, &mut motion);
    playback.held = frame.input;
}

/// Counts how far the player is from where they were at a recorded position, and puts them back if far.
fn compare_position(mut playback: ResMut<Playback>, mut cameras: Query<(&mut Transform, &mut FpsCamera)>) {
    let Some(index) = playback.check.take() else { return };
    let Some(pose) = playback.recording.frames[index].check else { return };
    let Ok((mut transform, mut cam)) = cameras.single_mut() else { return };
    if playback.compare(&pose, transform.translation) {
        transform.translation = pose.position;
        cam.yaw = pose.yaw;
        cam.pitch = pose.pitch;
        transform.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
    }
}

/// The window's cursor: held by the game while it plays on, free for the viewer to use the replay's menu.
fn set_cursor(playback: Res<Playback>, mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>) {
    if !playback.driving() {
        return;
    }
    let Ok(mut cursor) = cursors.single_mut() else { return };
    let hold = playback.mode == Mode::Running && !playback.paused && playback.captured_now() == Some(true);
    let (grab, visible) = if hold { (CursorGrabMode::Locked, false) } else { (CursorGrabMode::None, true) };
    if cursor.grab_mode != grab || cursor.visible != visible {
        cursor.grab_mode = grab;
        cursor.visible = visible;
    }
}

/// What the viewer sees of the replay: where it has got to and how it is doing, and its menu when paused or done.
pub fn draw_playback(mut contexts: EguiContexts, playback: Option<ResMut<Playback>>, mut exit: MessageWriter<AppExit>, menu: Res<Menu>, screen: Res<InventoryScreen>) -> Result {
    let Some(mut playback) = playback else { return Ok(()) };
    if !playback.driving() {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let header = &playback.recording.header;
    egui::Area::new(egui::Id::new("replay status")).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 8.0)).interactable(false).show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.set_min_width(480.0);
            ui.label(egui::RichText::new(format!("REPLAY  {} / {}", clock(playback.seconds_played()), clock(playback.seconds_total()))).strong().size(16.0).color(egui::Color32::WHITE));
            let drift = playback.drift;
            if drift.checks == 0 {
            } else if drift.resyncs > 0 {
                ui.colored_label(egui::Color32::from_rgb(230, 180, 60), format!("drifted from the recording {} time(s), worst {:.1} m", drift.resyncs, drift.worst));
            } else if drift.drifted > 0 {
                ui.colored_label(egui::Color32::from_rgb(230, 180, 60), format!("{:.2} m off the recording at worst", drift.worst));
            } else {
                ui.colored_label(egui::Color32::from_rgb(120, 200, 120), "in step with the recording");
            }
            if playback.seconds_played() < 12.0 {
                if !header.start.fresh {
                    ui.colored_label(egui::Color32::from_rgb(230, 180, 60), "Recorded part-way through a game: the zombies and loot start as in a new game.");
                }
                if header.game != crate::VERSION {
                    ui.colored_label(egui::Color32::from_rgb(230, 180, 60), format!("Recorded on {}, played on {}: it may not go the same way.", header.game, crate::VERSION));
                }
            }
            if menu.open {
                ui.label("(the player has the menu open)");
            } else if screen.open {
                ui.label("(the player has the inventory open)");
            }
        });
    });
    let (mut resume, mut take_over, mut quit) = (false, false, false);
    let title = if playback.mode == Mode::Finished { "Replay finished" } else { "Replay paused" };
    if playback.mode == Mode::Finished || playback.paused {
        egui::Window::new(title).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).collapsible(false).resizable(false).show(ctx, |ui| {
            let drift = playback.drift;
            ui.label(format!("{} of {}", clock(playback.seconds_played()), clock(playback.seconds_total())));
            if drift.checks > 0 {
                ui.label(format!("{} of {} checks off the recording; worst {:.2} m; put back {} time(s).", drift.drifted, drift.checks, drift.worst, drift.resyncs));
            }
            ui.separator();
            if playback.mode == Mode::Running && ui.button("Resume").clicked() {
                resume = true;
            }
            if ui.button("Take control from here").clicked() {
                take_over = true;
            }
            if ui.button("Exit the game").clicked() {
                quit = true;
            }
            if playback.mode == Mode::Running {
                ui.label("Escape resumes.");
            }
        });
    }
    if resume {
        playback.toggle_pause = true;
    }
    if take_over {
        playback.mode = Mode::Released;
        playback.paused = false;
    }
    if quit {
        exit.write(AppExit::Success);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::Controls;
    use crate::health::PlayerHealth;
    use crate::recording::{Header, Start};

    fn recording(frames: usize) -> Recording {
        let start = Start {
            pose: Pose { position: Vec3::ZERO, yaw: 0.0, pitch: 0.0 },
            health: PlayerHealth::default(),
            inventory: Inventory::starting(),
            dice: None,
            gun_rng: 1,
            menu_open: false,
            inventory_open: false,
            captured: true,
            aspect: 16.0 / 9.0,
            fresh: true,
        };
        let frame = |i: usize| Frame {
            nanos: 10_000_000 * (i as u64 + 1),
            input: Input::default(),
            captured: i % 2 == 0,
            aspect: 16.0 / 9.0,
            events: Vec::new(),
            check: (i == 1).then_some(Pose { position: Vec3::new(1.0, 0.0, 0.0), yaw: 0.0, pitch: 0.0 }),
        };
        Recording { header: Header { game: "t".into(), seed: 1, controls: Controls::default(), start }, frames: (0..frames).map(frame).collect() }
    }

    #[test]
    fn frames_are_played_in_order_with_the_length_they_had() {
        let mut playback = Playback::new(recording(3), false);
        assert_eq!(playback.next_length(), Some(Duration::from_millis(10)));
        assert_eq!(playback.take_frame().map(|f| f.nanos), Some(10_000_000));
        assert_eq!(playback.next_length(), Some(Duration::from_millis(20)));
        assert_eq!(playback.take_frame().map(|f| f.nanos), Some(20_000_000));
        assert_eq!(playback.mode, Mode::Running);
        assert_eq!(playback.take_frame().map(|f| f.nanos), Some(30_000_000));
        assert_eq!(playback.mode, Mode::Finished, "the last frame has been given out");
        assert!(playback.take_frame().is_none());
        assert_eq!(playback.next_length(), Some(Duration::ZERO), "and time stands still after it");
        assert!((playback.seconds_played() - 0.06).abs() < 1e-6);
    }

    #[test]
    fn a_paused_replay_gives_out_no_frames_and_no_time() {
        let mut playback = Playback::new(recording(3), false);
        playback.take_frame();
        playback.paused = true;
        assert_eq!(playback.next_length(), Some(Duration::ZERO));
        assert!(playback.take_frame().is_none());
        assert_eq!(playback.mode, Mode::Running, "it is not finished, only stopped");
        playback.paused = false;
        assert_eq!(playback.take_frame().map(|f| f.nanos), Some(20_000_000));
    }

    #[test]
    fn the_mouse_is_as_captured_as_the_frame_playing_had_it() {
        let mut playback = Playback::new(recording(3), false);
        assert_eq!(playback.captured_now(), Some(true), "as the recording began");
        playback.take_frame();
        assert_eq!(playback.captured_now(), Some(true));
        playback.take_frame();
        assert_eq!(playback.captured_now(), Some(false));
        playback.mode = Mode::Released;
        assert_eq!(playback.captured_now(), None, "once the viewer has taken over it is the window's");
    }

    #[test]
    fn an_empty_recording_is_over_at_once() {
        let mut playback = Playback::new(recording(0), false);
        assert_eq!(playback.mode, Mode::Finished);
        assert!(playback.take_frame().is_none());
    }

    #[test]
    fn a_position_that_is_off_is_counted_and_a_long_way_off_is_put_back() {
        let mut playback = Playback::new(recording(3), false);
        let pose = Pose { position: Vec3::new(10.0, 0.0, 0.0), yaw: 0.0, pitch: 0.0 };
        assert!(!playback.compare(&pose, Vec3::new(10.0, 0.0, 0.0)));
        assert!(!playback.compare(&pose, Vec3::new(10.1, 0.0, 0.0)), "a little off is counted and left");
        assert!(playback.compare(&pose, Vec3::new(11.0, 0.0, 0.0)), "a metre off is put back");
        let drift = playback.drift;
        assert_eq!((drift.checks, drift.drifted, drift.resyncs), (3, 2, 1));
        assert!((drift.worst - 1.0).abs() < 1e-5 && (drift.latest - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_strict_replay_is_never_put_back() {
        let mut playback = Playback::new(recording(3), true);
        let pose = Pose { position: Vec3::ZERO, yaw: 0.0, pitch: 0.0 };
        assert!(!playback.compare(&pose, Vec3::new(50.0, 0.0, 0.0)));
        assert_eq!((playback.drift.drifted, playback.drift.resyncs), (1, 0));
    }

    #[test]
    fn only_a_frame_with_a_position_is_compared() {
        let mut playback = Playback::new(recording(3), false);
        playback.take_frame();
        assert_eq!(playback.check, None);
        playback.take_frame();
        assert_eq!(playback.check, Some(1));
    }
}
