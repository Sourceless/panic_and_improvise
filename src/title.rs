//! The title screen, and the loading screen that follows it: what comes before the world.
//!
//! The game starts on the title, with nothing built. Singleplayer, hosting and joining all begin there; each
//! says what kind of game it is and the title makes the world, showing how far it has got, then puts the player
//! in it. (Joining first knocks on the host's door, which is "Connecting"; the seed the map is made from comes
//! back in the welcome.)
//!
//! How the world is kept out of the way until then: the game's own systems all live in `Startup` and `Update`.
//! `TitlePlugin` takes those two schedules (and `PreStartup`, `PostStartup`) out of the app as it is built, so
//! that none of them runs; Bevy skips a schedule that is not there. What is left is the title, the renderer and
//! the few systems that wait in `PreUpdate`, `PostUpdate` and the rest. To begin the world the schedules are
//! put back, the startup ones are run by hand, and from then on it is the ordinary game. So the plugin must be
//! added after every other that puts systems in those schedules.
//!
//! The map is made on another thread, so that the screen stays alive; the rest (the buildings, the roads, the
//! meshes) is made by the startup systems on this one, and the screen is drawn once before they begin, so that
//! it says what it is doing while they hold it up.
//!
//! What happens is logic on [`Title`], and the screens ([`draw_title`], in egui) only show it. Nothing here is
//! used by the tests of the game, which make their own worlds; this is for the program that is played.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bevy::app::AppExit;
use bevy::ecs::schedule::{ScheduleLabel, Schedules};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::map::TerrainMap;
use crate::net::DEFAULT_PORT;
use crate::net_client::{Connection, KeepAlive, NetClient};
use crate::net_host::{with_port, Host};
use crate::params::GenParams;

/// How long, seconds, the host is given to answer.
const PATIENCE: Duration = Duration::from_secs(8);

/// How many frames the loading screen is drawn, saying what comes next, before the world is begun.
const FRAMES_TO_SETTLE: u32 = 4;

/// How long, seconds, making the map is expected to take; the bar creeps toward its share of the way over about
/// this long, and never reaches it before the map is made.
const MAP_SECONDS: f32 = 6.0;

/// How much of the bar the map is, and what is left for the building of the world.
const MAP_SHARE: f32 = 0.45;

/// Where in the title, or past it, the program is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Title,
    /// Waiting for a host to let the player in.
    Connecting,
    /// Making the world.
    Loading,
    /// In it: the game is on, and none of this has anything to do any more.
    Playing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Main,
    Multiplayer,
    Replays,
}

/// What the player has chosen on the title.
#[derive(Debug, PartialEq)]
pub enum Request {
    Single,
    /// Host a game on this address.
    Host(String),
    /// Join the game at this address, under this name.
    Join(String, String),
    /// Give up on a connection that has not been answered.
    Cancel,
    /// Play the recording in this file (by starting the game again with it).
    Replay(PathBuf),
    Quit,
}

/// What kind of game is being made ready.
enum Plan {
    Single,
    Host(String),
    Join(Connection),
}

/// What the loading screen is waiting for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    /// The map is being made on another thread.
    Map,
    /// The screen says "building" for a few frames, then the startup systems hold it up.
    Building(u32),
}

#[derive(Resource)]
pub struct Title {
    pub phase: Phase,
    pub screen: Screen,
    pub name: String,
    pub address: String,
    pub port: String,
    /// A line on the title: what went wrong, usually.
    pub message: String,
    /// What the loading screen is doing, and how far it has got (0 to 1).
    pub detail: String,
    pub progress: f32,
    pub request: Option<Request>,
    /// Recordings, newest first, for the replays screen.
    pub recordings: Vec<PathBuf>,
    /// How the map is made: the real thing, unless a test says otherwise.
    pub generate: fn(u64) -> TerrainMap,
    plan: Option<Plan>,
    seed: u64,
    making: Option<JoinHandle<TerrainMap>>,
    asking: Option<(Mutex<Receiver<Result<Connection, String>>>, Arc<AtomicBool>)>,
    alive: Option<KeepAlive>,
    stage: Stage,
    since: Instant,
    /// The schedules taken out of the app until the world is begun.
    stashed: Vec<Schedule>,
    camera: Option<Entity>,
}

impl Default for Title {
    fn default() -> Self {
        Title {
            phase: Phase::Title,
            screen: Screen::Main,
            name: default_name(),
            address: format!("127.0.0.1:{DEFAULT_PORT}"),
            port: DEFAULT_PORT.to_string(),
            message: String::new(),
            detail: String::new(),
            progress: 0.0,
            request: None,
            recordings: Vec::new(),
            generate: |seed| TerrainMap::generate(seed, &GenParams::default()),
            plan: None,
            seed: 0,
            making: None,
            asking: None,
            alive: None,
            stage: Stage::Map,
            since: Instant::now(),
            stashed: Vec::new(),
            camera: None,
        }
    }
}

/// A name to join under unless the player says another: the account's, if the system says what it is.
fn default_name() -> String {
    let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
    crate::net::clean_name(&user)
}

impl Title {
    /// The address to host on, from what is in the port field: `0.0.0.0` and that port, or an error to say.
    pub fn host_address(&self) -> Result<String, String> {
        match self.port.trim().parse::<u16>() {
            Ok(port) if port > 0 => Ok(format!("0.0.0.0:{port}")),
            _ => Err("The port is a number from 1 to 65535.".to_string()),
        }
    }

    /// The things the screen is showing as it goes: for the bar.
    pub fn fraction(&self) -> f32 {
        self.progress.clamp(0.0, 1.0)
    }

    pub fn showing_title(&self) -> bool {
        self.phase == Phase::Title
    }

    /// Back to the title with a line to say why.
    fn fall_back(&mut self, message: impl Into<String>) {
        self.phase = Phase::Title;
        self.message = message.into();
        self.plan = None;
        self.asking = None;
        self.making = None;
        self.alive = None;
    }

    fn begin_loading(&mut self, plan: Plan, seed: u64) {
        self.seed = seed;
        self.phase = Phase::Loading;
        self.stage = Stage::Map;
        self.since = Instant::now();
        self.progress = 0.0;
        self.detail = "Shaping the land".to_string();
        self.message.clear();
        // A joined game is kept alive while the world is made: the host would not wait.
        if let Plan::Join(connection) = &plan {
            self.alive = Some(connection.keep_alive());
        }
        self.plan = Some(plan);
        let make = self.generate;
        self.making = Some(std::thread::spawn(move || make(seed)));
    }
}

/// Whether the game is on, or there is no title to wait for (as in the tests): for what is only done in the game.
pub fn playing(title: Option<Res<Title>>) -> bool {
    title.is_none_or(|t| t.phase == Phase::Playing)
}

pub struct TitlePlugin;

impl Plugin for TitlePlugin {
    fn build(&self, app: &mut App) {
        let mut title = app.world_mut().remove_resource::<Title>().unwrap_or_default();
        // Out of the way until the world is begun.
        {
            let mut schedules = app.world_mut().resource_mut::<Schedules>();
            title.stashed = [PreStartup.intern(), Startup.intern(), Update.intern(), PostStartup.intern()].into_iter().filter_map(|label| schedules.remove(label)).collect();
        }
        // Something for the title to be drawn on.
        title.camera = Some(app.world_mut().spawn(Camera2d).id());
        title.recordings = recordings();
        app.insert_resource(title).add_systems(PreUpdate, drive.run_if(not(playing)));
    }
}

/// The recordings there are, newest first.
fn recordings() -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(crate::recording::recordings_dir()) else { return Vec::new() };
    let mut found: Vec<PathBuf> = read.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == crate::recording::EXTENSION)).collect();
    found.sort();
    found.reverse();
    found
}

/// Does what has been asked, and sees the world made.
fn drive(world: &mut World) {
    world.resource_scope(|world, mut title: Mut<Title>| {
        if let Some(request) = title.request.take() {
            deal_with(&mut title, request, world);
        }
        match title.phase {
            Phase::Title | Phase::Playing => {}
            Phase::Connecting => {
                let answer = title.asking.as_ref().and_then(|(receiver, _)| receiver.lock().ok().and_then(|r| r.try_recv().ok()));
                match answer {
                    Some(Ok(connection)) => {
                        let seed = connection.seed;
                        title.asking = None;
                        title.begin_loading(Plan::Join(connection), seed);
                    }
                    Some(Err(error)) => title.fall_back(error),
                    None => {}
                }
            }
            Phase::Loading => {
                let elapsed = title.since.elapsed().as_secs_f32();
                match title.stage {
                    Stage::Map => {
                        // It creeps toward its share, slower as it goes, and is there when the map is.
                        title.progress = MAP_SHARE * (1.0 - (-elapsed / MAP_SECONDS).exp()) * 0.95;
                        if title.making.as_ref().is_some_and(|m| m.is_finished()) {
                            let made = title.making.take().map(|m| m.join());
                            match made {
                                Some(Ok(map)) => {
                                    info!("map seed {} made in {:.1} s", map.seed, elapsed);
                                    world.insert_resource(map);
                                    title.stage = Stage::Building(0);
                                    title.progress = MAP_SHARE;
                                    title.detail = "Building the world".to_string();
                                    title.since = Instant::now();
                                }
                                _ => title.fall_back("The map could not be made."),
                            }
                        }
                    }
                    Stage::Building(frames) => {
                        title.stage = Stage::Building(frames + 1);
                        title.progress = MAP_SHARE + 0.4 * (frames as f32 / FRAMES_TO_SETTLE as f32);
                        if frames >= FRAMES_TO_SETTLE {
                            begin_world(world, &mut title);
                        }
                    }
                }
            }
        }
    });
}

fn deal_with(title: &mut Title, request: Request, world: &mut World) {
    match (title.phase, request) {
        (Phase::Title, Request::Single) => title.begin_loading(Plan::Single, crate::game_seed()),
        (Phase::Title, Request::Host(address)) => title.begin_loading(Plan::Host(address), crate::game_seed()),
        (Phase::Title, Request::Join(address, name)) => {
            let name = crate::net::clean_name(&name);
            let address = with_port(address.trim());
            title.phase = Phase::Connecting;
            title.message = format!("Asking {address} to let {name} in...");
            let cancel = Arc::new(AtomicBool::new(false));
            let (send, receive) = mpsc::channel();
            let flag = cancel.clone();
            std::thread::spawn(move || {
                let _ = send.send(Connection::join(address.as_str(), &name, PATIENCE, &flag));
            });
            title.asking = Some((Mutex::new(receive), cancel));
        }
        (Phase::Connecting, Request::Cancel) => {
            if let Some((_, cancel)) = &title.asking {
                cancel.store(true, Ordering::Relaxed);
            }
            title.fall_back("");
        }
        (Phase::Title, Request::Quit) => {
            world.write_message(AppExit::Success);
        }
        (Phase::Title, Request::Replay(file)) => {
            // A recording is played by a program started with it: the map and everything it starts with are its.
            match std::env::current_exe().and_then(|exe| std::process::Command::new(exe).arg(&file).spawn()) {
                Ok(_) => {
                    world.write_message(AppExit::Success);
                }
                Err(error) => title.message = format!("Could not play it: {error}"),
            }
        }
        _ => {}
    }
}

/// The world is made: the schedules are put back, the startup systems run, and the game is on.
fn begin_world(world: &mut World, title: &mut Title) {
    // Hosting begins when the world is there to be joined; a joined game keeps its own word.
    let plan = title.plan.take();
    let host = match &plan {
        Some(Plan::Host(address)) => match Host::bind(address) {
            Ok(host) => Some(host),
            Err(error) => {
                title.fall_back(error);
                return;
            }
        },
        _ => None,
    };
    let started = Instant::now();
    {
        let mut schedules = world.resource_mut::<Schedules>();
        for schedule in title.stashed.drain(..) {
            schedules.insert(schedule);
        }
    }
    for label in [PreStartup.intern(), Startup.intern(), PostStartup.intern()] {
        let _ = world.try_run_schedule(label);
    }
    info!("world begun in {:.1} s", started.elapsed().as_secs_f32());
    if let Some(camera) = title.camera.take() {
        world.despawn(camera);
    }
    if let Some(host) = host {
        world.insert_resource(host);
    }
    if let Some(Plan::Join(connection)) = plan {
        // What kept it alive stops: from here the game says where the player is.
        title.alive = None;
        world.insert_resource(NetClient::new(connection));
    }
    title.phase = Phase::Playing;
    title.progress = 1.0;
}

// ---- The screens ----

const SKY_TOP: egui::Color32 = egui::Color32::from_rgb(18, 24, 30);
const SKY_BOTTOM: egui::Color32 = egui::Color32::from_rgb(70, 52, 40);
const GROUND: egui::Color32 = egui::Color32::from_rgb(9, 10, 12);
const TEXT: egui::Color32 = egui::Color32::from_rgb(232, 226, 214);
const DIM: egui::Color32 = egui::Color32::from_rgb(150, 144, 132);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(214, 150, 60);

/// Draws the screen the title is on: the title, the multiplayer choices, the recordings, "Connecting" or "Loading".
pub fn draw_title(mut contexts: EguiContexts, mut title: ResMut<Title>, time: Res<Time>) -> Result {
    if title.phase == Phase::Playing {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let request = show_title(ctx, &mut title, time.elapsed_secs());
    if request.is_some() {
        title.request = request;
    }
    Ok(())
}

/// Draws the screen on `ctx`, and says what was chosen on it, if anything was.
pub fn show_title(ctx: &egui::Context, title: &mut Title, t: f32) -> Option<Request> {
    let mut request = None;
    let rect = ctx.content_rect();
    egui::Area::new(egui::Id::new("title")).order(egui::Order::Background).fixed_pos(rect.min).constrain(false).show(ctx, |ui| {
        ui.set_clip_rect(rect);
        paint_backdrop(ui.painter(), rect, t);
        // The column of what is chosen, a little above the middle.
        let width = 360.0_f32.min(rect.width() - 32.0);
        let column = egui::Rect::from_center_size(egui::pos2(rect.center().x, rect.top() + rect.height() * 0.5), egui::vec2(width, rect.height() * 0.8));
        ui.scope_builder(egui::UiBuilder::new().max_rect(column), |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(rect.height() * 0.1);
                ui.label(egui::RichText::new("PANIC").size(76.0).strong().color(TEXT));
                ui.label(egui::RichText::new("AND IMPROVISE").size(34.0).strong().color(ACCENT));
                ui.add_space(36.0);
                match title.phase {
                    Phase::Title => match title.screen {
                        Screen::Main => main_screen(ui, title, &mut request),
                        Screen::Multiplayer => multiplayer_screen(ui, title, &mut request),
                        Screen::Replays => replays_screen(ui, title, &mut request),
                    },
                    Phase::Connecting => {
                        ui.label(egui::RichText::new(&title.message).size(18.0).color(TEXT));
                        ui.add_space(14.0);
                        ui.add(egui::Spinner::new().size(28.0).color(ACCENT));
                        ui.add_space(18.0);
                        if button(ui, "Cancel") {
                            request = Some(Request::Cancel);
                        }
                    }
                    Phase::Loading => loading_screen(ui, title, t),
                    Phase::Playing => {}
                }
                if !title.message.is_empty() && title.phase == Phase::Title {
                    ui.add_space(14.0);
                    ui.label(egui::RichText::new(&title.message).size(15.0).color(egui::Color32::from_rgb(230, 120, 100)));
                }
            });
        });
        ui.painter().text(egui::pos2(rect.left() + 16.0, rect.bottom() - 14.0), egui::Align2::LEFT_BOTTOM, crate::version_label(), egui::FontId::proportional(14.0), DIM);
    });
    request
}

/// A button as wide as the column.
fn button(ui: &mut egui::Ui, label: &str) -> bool {
    ui.add_sized([ui.available_width(), 44.0], egui::Button::new(egui::RichText::new(label).size(18.0))).clicked()
}

fn main_screen(ui: &mut egui::Ui, title: &mut Title, request: &mut Option<Request>) {
    ui.spacing_mut().item_spacing.y = 10.0;
    if button(ui, "Singleplayer") {
        *request = Some(Request::Single);
    }
    if button(ui, "Multiplayer") {
        title.screen = Screen::Multiplayer;
        title.message.clear();
    }
    let any = !title.recordings.is_empty();
    ui.add_enabled_ui(any, |ui| {
        if button(ui, "Watch a replay") {
            title.screen = Screen::Replays;
            title.message.clear();
        }
    });
    if button(ui, "Quit") {
        *request = Some(Request::Quit);
    }
}

fn multiplayer_screen(ui: &mut egui::Ui, title: &mut Title, request: &mut Option<Request>) {
    ui.spacing_mut().item_spacing.y = 10.0;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Your name").color(DIM));
        ui.add(egui::TextEdit::singleline(&mut title.name).desired_width(f32::INFINITY).char_limit(crate::net::NAME_BYTES));
    });
    ui.add_space(8.0);
    ui.label(egui::RichText::new("Host").size(16.0).strong().color(TEXT));
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Port").color(DIM));
        ui.add(egui::TextEdit::singleline(&mut title.port).desired_width(80.0).char_limit(5));
    });
    if button(ui, "Host a game") {
        match title.host_address() {
            Ok(address) => *request = Some(Request::Host(address)),
            Err(error) => title.message = error,
        }
    }
    ui.add_space(8.0);
    ui.label(egui::RichText::new("Join").size(16.0).strong().color(TEXT));
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Address").color(DIM));
        ui.add(egui::TextEdit::singleline(&mut title.address).desired_width(f32::INFINITY).hint_text("192.168.1.20:7777"));
    });
    if button(ui, "Join a game") {
        if title.address.trim().is_empty() {
            title.message = "Say where the game is: an address like 192.168.1.20.".to_string();
        } else {
            *request = Some(Request::Join(title.address.clone(), title.name.clone()));
        }
    }
    ui.add_space(8.0);
    if button(ui, "Back") {
        title.screen = Screen::Main;
        title.message.clear();
    }
}

fn replays_screen(ui: &mut egui::Ui, title: &mut Title, request: &mut Option<Request>) {
    ui.spacing_mut().item_spacing.y = 8.0;
    egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
        for file in title.recordings.iter().take(40) {
            let name = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if ui.add_sized([ui.available_width(), 34.0], egui::Button::new(name)).clicked() {
                *request = Some(Request::Replay(file.clone()));
            }
        }
    });
    ui.add_space(8.0);
    if button(ui, "Back") {
        title.screen = Screen::Main;
    }
}

fn loading_screen(ui: &mut egui::Ui, title: &Title, t: f32) {
    ui.label(egui::RichText::new("Loading").size(20.0).color(TEXT));
    ui.add_space(14.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 14.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 7.0, egui::Color32::from_rgba_unmultiplied(0, 0, 0, 170));
    let filled = egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + rect.width() * title.fraction(), rect.bottom()));
    painter.rect_filled(filled.shrink(2.0), 5.0, ACCENT);
    ui.add_space(14.0);
    let dots = ".".repeat(1 + (t * 2.0) as usize % 3);
    ui.label(egui::RichText::new(format!("{}{dots}", title.detail)).size(16.0).color(DIM));
}

/// The backdrop: a dusk sky going from cold to warm over a row of terraced roofs, and a few stars of embers.
fn paint_backdrop(painter: &egui::Painter, rect: egui::Rect, t: f32) {
    let bands = 48;
    for i in 0..bands {
        let (a, b) = (i as f32 / bands as f32, (i + 1) as f32 / bands as f32);
        let mix = |from: u8, to: u8| (from as f32 + (to as f32 - from as f32) * (a * a)) as u8;
        let colour = egui::Color32::from_rgb(mix(SKY_TOP.r(), SKY_BOTTOM.r()), mix(SKY_TOP.g(), SKY_BOTTOM.g()), mix(SKY_TOP.b(), SKY_BOTTOM.b()));
        painter.rect_filled(egui::Rect::from_min_max(egui::pos2(rect.left(), rect.top() + rect.height() * a), egui::pos2(rect.right(), rect.top() + rect.height() * b + 1.0)), 0.0, colour);
    }
    // The roofs, as a skyline of gables and chimneys along the bottom.
    let base = rect.bottom() - rect.height() * 0.12;
    let unit = rect.width() / 24.0;
    let mut x = rect.left() - unit * 0.5;
    let mut n = 0u32;
    while x < rect.right() {
        // A cheap run of numbers: the same skyline every time.
        let r = (n.wrapping_mul(2_654_435_761) >> 12) % 100;
        let width = unit * (1.2 + (r % 7) as f32 * 0.18);
        let wall = rect.height() * (0.07 + (r % 5) as f32 * 0.012);
        let peak = wall + width * 0.32;
        painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x, base - wall), egui::pos2(x + width, rect.bottom())), 0.0, GROUND);
        painter.add(egui::Shape::convex_polygon(vec![egui::pos2(x - 4.0, base - wall), egui::pos2(x + width * 0.5, base - peak), egui::pos2(x + width + 4.0, base - wall)], GROUND, egui::Stroke::NONE));
        if r % 3 == 0 {
            painter.rect_filled(egui::Rect::from_min_size(egui::pos2(x + width * 0.62, base - peak), egui::vec2(width * 0.1, wall * 0.5)), 0.0, GROUND);
        }
        // A lit window here and there, flickering.
        if r % 4 == 0 {
            let flicker = 0.6 + 0.4 * (t * 1.7 + n as f32).sin();
            let glow = egui::Color32::from_rgba_unmultiplied(235, 170, 70, (150.0 * flicker) as u8);
            painter.rect_filled(egui::Rect::from_min_size(egui::pos2(x + width * 0.3, base - wall * 0.6), egui::vec2(width * 0.14, wall * 0.3)), 1.0, glow);
        }
        x += width;
        n += 1;
    }
    painter.rect_filled(egui::Rect::from_min_max(egui::pos2(rect.left(), base), rect.max), 0.0, GROUND);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An app with a title and a game that is only what shows whether it has begun.
    #[derive(Resource, Default)]
    struct Begun(u32);

    #[derive(Resource, Default)]
    struct Frames(u32);

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(Title { generate: |_| TerrainMap::flat(0.0), ..default() })
            .init_resource::<Begun>()
            .init_resource::<Frames>()
            .add_systems(Startup, |mut begun: ResMut<Begun>| begun.0 += 1)
            .add_systems(Update, |mut frames: ResMut<Frames>| frames.0 += 1)
            .add_plugins(TitlePlugin);
        app
    }

    fn run(app: &mut App, until: impl Fn(&Title) -> bool) {
        for _ in 0..400 {
            app.update();
            std::thread::sleep(Duration::from_millis(2));
            if until(app.world().resource::<Title>()) {
                return;
            }
        }
        panic!("it did not get there: {:?}", app.world().resource::<Title>().phase);
    }

    #[test]
    fn nothing_of_the_game_runs_on_the_title() {
        let mut app = app();
        for _ in 0..5 {
            app.update();
        }
        assert_eq!((app.world().resource::<Begun>().0, app.world().resource::<Frames>().0), (0, 0));
        assert_eq!(app.world().resource::<Title>().phase, Phase::Title);
    }

    #[test]
    fn singleplayer_makes_the_map_begins_the_world_once_and_then_the_game_runs() {
        let mut app = app();
        app.update();
        app.world_mut().resource_mut::<Title>().request = Some(Request::Single);
        run(&mut app, |t| t.phase == Phase::Playing);
        // The startup systems ran once, and the map is the one that was made.
        assert_eq!(app.world().resource::<Begun>().0, 1);
        assert!(app.world().get_resource::<TerrainMap>().is_some());
        assert!(app.world().get_resource::<Host>().is_none() && app.world().get_resource::<NetClient>().is_none());
        let before = app.world().resource::<Frames>().0;
        for _ in 0..3 {
            app.update();
        }
        assert!(app.world().resource::<Frames>().0 >= before + 3, "the game's own systems run each frame from now on");
        assert_eq!(app.world().resource::<Begun>().0, 1, "and the startup ones do not run again");
    }

    #[test]
    fn hosting_opens_the_door_only_once_the_world_is_there() {
        let mut app = app();
        app.update();
        app.world_mut().resource_mut::<Title>().request = Some(Request::Host("127.0.0.1:0".to_string()));
        let mut at_the_start = None;
        for _ in 0..400 {
            app.update();
            std::thread::sleep(Duration::from_millis(2));
            let title = app.world().resource::<Title>();
            if title.phase == Phase::Loading && at_the_start.is_none() {
                at_the_start = Some(app.world().get_resource::<Host>().is_some());
            }
            if title.phase == Phase::Playing {
                break;
            }
        }
        assert_eq!(at_the_start, Some(false), "nobody can join a game that has no world yet");
        assert!(app.world().get_resource::<Host>().is_some());
    }

    #[test]
    fn a_host_that_does_not_answer_sends_the_player_back_to_the_title_with_the_reason() {
        let mut app = app();
        app.update();
        // A port with nothing on it.
        let nothing = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = nothing.local_addr().unwrap().to_string();
        app.world_mut().resource_mut::<Title>().request = Some(Request::Join(address, "Tester".into()));
        app.update();
        assert_eq!(app.world().resource::<Title>().phase, Phase::Connecting);
        // Cancelled, rather than waiting out the patience.
        app.world_mut().resource_mut::<Title>().request = Some(Request::Cancel);
        app.update();
        let title = app.world().resource::<Title>();
        assert_eq!(title.phase, Phase::Title);
        assert!(title.asking.is_none());
    }

    /// Runs the title for a frame with `events` arriving, and says what was chosen on it.
    fn frame(ctx: &egui::Context, title: &mut Title, events: Vec<egui::Event>) -> Option<Request> {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));
        ctx.begin_pass(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() });
        let chosen = show_title(ctx, title, 0.0);
        ctx.end_pass().textures_delta.clear();
        chosen
    }

    /// Clicks at `at`, a frame to move there, one to press and one to let go.
    fn click(ctx: &egui::Context, title: &mut Title, at: egui::Pos2) -> Option<Request> {
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        frame(ctx, title, vec![egui::Event::PointerMoved(at)]);
        let pressed = frame(ctx, title, vec![button(true)]);
        let released = frame(ctx, title, vec![button(false)]);
        pressed.or(released)
    }

    #[test]
    fn a_click_on_the_singleplayer_button_chooses_singleplayer_and_one_on_multiplayer_opens_that_screen() {
        let ctx = egui::Context::default();
        let mut title = Title::default();
        // The first frames lay it out.
        for _ in 0..3 {
            frame(&ctx, &mut title, vec![]);
        }
        // Down the middle of the screen, until something is clicked: the first button is the first thing there.
        let mut got = None;
        for y in (150..700).step_by(5) {
            if let Some(request) = click(&ctx, &mut title, egui::pos2(640.0, y as f32)) {
                got = Some((y, request));
                break;
            }
        }
        let (y, request) = got.expect("some button took a click");
        assert_eq!(request, Request::Single, "at {y}");
        // The second one opens the multiplayer screen.
        let mut opened = false;
        for y in (150..700).step_by(5) {
            click(&ctx, &mut title, egui::pos2(640.0, y as f32));
            if title.screen == Screen::Multiplayer {
                opened = true;
                break;
            }
        }
        assert!(opened);
    }

    #[test]
    fn the_port_to_host_on_is_a_number_in_range() {
        let mut t = Title::default();
        assert_eq!(t.host_address().unwrap(), format!("0.0.0.0:{DEFAULT_PORT}"));
        for bad in ["", "0", "70000", "abc", "-1"] {
            t.port = bad.to_string();
            assert!(t.host_address().is_err(), "{bad:?}");
        }
        t.port = " 9000 ".to_string();
        assert_eq!(t.host_address().unwrap(), "0.0.0.0:9000");
    }
}
