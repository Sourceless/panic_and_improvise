//! A recording of play, as a text file that can be played again by the game itself (`fps_prototype recording.pir`).
//!
//! What is kept is what went into the game, not what came out of it: the map's seed, how the game began (where the
//! player was, what they carried, where the dice had got to), and then for every frame how long it was and what
//! the player did in it (keys, mouse buttons, the mouse's movement). Played back with the same times and the same
//! input, the game does what it did. Now and then the player's position is kept too, to tell when a replay has
//! drifted from the play it came from.
//!
//! Nothing here is about running a game: it is the file, read and written, and tested without one.
//!
//! ```text
//! panic-and-improvise-recording 1
//! game 0.1.57
//! seed 2026
//! control preset = qwerty
//! start-... (how the game began)
//! f 16666667 h=#77 d=3f800000,bf000000      one frame: its length in nanoseconds, then what changed
//! c 41200000,41f00000,c1000000 3f800000 0   where the player was at the end of it, now and then
//! ```

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use crate::controls::{Bind, Controls, KeyboardFrame};
use crate::health::PlayerHealth;
use crate::inventory::{Inventory, Item};
use crate::inventory_text::{item_text, parse_item};

pub const MAGIC: &str = "panic-and-improvise-recording";
pub const FORMAT: u32 = 1;
pub const EXTENSION: &str = "pir";

// ---- numbers and keys as text -----------------------------------------------------------------

/// A float as the eight hex digits of its bits: it reads back exactly.
pub fn bits(x: f32) -> String {
    format!("{:08x}", x.to_bits())
}

pub fn unbits(text: &str) -> Option<f32> {
    u32::from_str_radix(text, 16).ok().map(f32::from_bits)
}

fn bind_token(bind: Bind) -> String {
    match bind {
        Bind::Char(c) => format!("#{:x}", c as u32),
        other => other.name(),
    }
}

fn parse_bind(token: &str) -> Option<Bind> {
    match token.strip_prefix('#') {
        Some(code) => u32::from_str_radix(code, 16).ok().and_then(char::from_u32).map(Bind::Char),
        None => Bind::parse(token).filter(|b| !matches!(b, Bind::Char(_))),
    }
}

fn button_token(button: MouseButton) -> String {
    match button {
        MouseButton::Left => "L".into(),
        MouseButton::Right => "R".into(),
        MouseButton::Middle => "M".into(),
        MouseButton::Back => "B".into(),
        MouseButton::Forward => "F".into(),
        MouseButton::Other(n) => format!("o{n}"),
    }
}

fn parse_button(token: &str) -> Option<MouseButton> {
    Some(match token {
        "L" => MouseButton::Left,
        "R" => MouseButton::Right,
        "M" => MouseButton::Middle,
        "B" => MouseButton::Back,
        "F" => MouseButton::Forward,
        other => MouseButton::Other(other.strip_prefix('o')?.parse().ok()?),
    })
}

fn list<T: Copy>(items: &[T], token: impl Fn(T) -> String) -> String {
    if items.is_empty() {
        "-".to_string()
    } else {
        items.iter().map(|&i| token(i)).collect::<Vec<_>>().join(",")
    }
}

fn parse_list<T>(text: &str, parse: impl Fn(&str) -> Option<T>) -> Option<Vec<T>> {
    if text == "-" { Some(Vec::new()) } else { text.split(',').map(parse).collect() }
}

/// Buttons in a fixed order, so that the same buttons are always the same text.
pub fn sorted_buttons(mut buttons: Vec<MouseButton>) -> Vec<MouseButton> {
    buttons.sort_by_key(|b| button_token(*b));
    buttons.dedup();
    buttons
}

// ---- what is in a recording ---------------------------------------------------------------------

/// Where the player is and which way they look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
}

/// What the player did in one frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Input {
    pub keys: KeyboardFrame,
    pub buttons_held: Vec<MouseButton>,
    pub buttons_pressed: Vec<MouseButton>,
    pub buttons_released: Vec<MouseButton>,
    /// How far the mouse moved, in the units the window reports it.
    pub motion: Vec2,
    /// Escape went down (it is not one of the bindable keys).
    pub escape: bool,
}

/// A change to the game that the keys and mouse do not explain, made at the start of a frame: what a click on a
/// screen did (the inventory, the menu), which is not recorded as clicks but as the state they left.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Screens { menu: bool, inventory: bool },
    Inventory(Inventory),
    Health(PlayerHealth),
    Thrown(Item),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// How long the frame was, nanoseconds.
    pub nanos: u64,
    pub input: Input,
    /// Whether the mouse was captured by the game when the frame ran.
    pub captured: bool,
    /// The camera's width over height as the game saw it. Whether a zombie is in view, and so may appear, depends on
    /// it, so a replay in a window of another shape has to be told it.
    pub aspect: f32,
    /// What changes at the start of the frame, before it runs.
    pub events: Vec<Event>,
    /// Where the player was at the end of the frame, kept every so often.
    pub check: Option<Pose>,
}

impl Frame {
    pub fn seconds(&self) -> f32 {
        std::time::Duration::from_nanos(self.nanos).as_secs_f32()
    }
}

/// How the game was when recording began.
#[derive(Clone, Debug, PartialEq)]
pub struct Start {
    pub pose: Pose,
    pub health: PlayerHealth,
    pub inventory: Inventory,
    /// Where the zombies' dice had got to, if there are zombies.
    pub dice: Option<(u64, u32)>,
    /// Where the generator that scatters shots had got to.
    pub gun_rng: u32,
    pub menu_open: bool,
    pub inventory_open: bool,
    pub captured: bool,
    pub aspect: f32,
    /// Recording began as the game did, so a replay has everything as it was. If not, the world around the player
    /// (the zombies, what has been picked up) starts as a new game's, and may go differently.
    pub fresh: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Header {
    /// The release of the game that made it.
    pub game: String,
    /// The map's seed.
    pub seed: u64,
    pub controls: Controls,
    pub start: Start,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    pub header: Header,
    pub frames: Vec<Frame>,
}

impl Recording {
    /// How long it plays for, seconds.
    pub fn seconds(&self) -> f32 {
        self.frames.iter().map(|f| f.nanos as f64).sum::<f64>() as f32 / 1e9
    }

    pub fn read(path: &Path) -> Result<Recording, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
        Recording::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn to_text(&self) -> String {
        let mut encoder = Encoder::new(&self.header);
        let mut text = encoder.header(&self.header);
        for frame in &self.frames {
            text += &encoder.frame(frame);
        }
        text
    }

    /// Reads the text of a recording. A last line cut short (the game stopped in the middle of writing it) is
    /// dropped; any other line that makes no sense is an error, with its number.
    pub fn parse(text: &str) -> Result<Recording, String> {
        let lines: Vec<(usize, &str)> = text.lines().enumerate().map(|(i, l)| (i + 1, l.trim())).filter(|(_, l)| !l.is_empty()).collect();
        let Some(&(_, first)) = lines.first() else {
            return Err("it is empty".into());
        };
        let version = first.strip_prefix(MAGIC).map(str::trim);
        match version.and_then(|v| v.parse::<u32>().ok()) {
            Some(FORMAT) => {}
            Some(other) => return Err(format!("it is format {other}, and this game reads format {FORMAT}")),
            None => return Err("it is not a recording".into()),
        }

        let mut game = String::new();
        let mut seed = None;
        let mut control_lines = Vec::new();
        let mut pose = None;
        let mut health = None;
        let mut inventory = None;
        let mut dice = None;
        let mut gun_rng = None;
        let mut screens = None;
        let mut aspect = 16.0 / 9.0;
        let mut fresh = false;

        let mut frames: Vec<Frame> = Vec::new();
        let mut events: Vec<Event> = Vec::new();
        // What carries from frame to frame until a frame says otherwise.
        let mut keys_held: Vec<Bind> = Vec::new();
        let mut buttons_held: Vec<MouseButton> = Vec::new();
        let mut captured = false;
        let mut frame_aspect = aspect;
        let mut in_frames = false;

        for (index, &(number, line)) in lines.iter().enumerate().skip(1) {
            let last = index + 1 == lines.len();
            let (word, rest) = line.split_once(' ').unwrap_or((line, ""));
            let outcome: Result<(), String> = (|| {
                match word {
                    "f" => {
                        if !in_frames {
                            in_frames = true;
                            captured = screens.map_or(false, |(_, _, c)| c);
                            frame_aspect = aspect;
                        }
                        let mut fields = rest.split_whitespace();
                        let nanos = fields.next().and_then(|n| n.parse().ok()).ok_or("a frame has no length")?;
                        let mut frame = Frame { nanos, input: Input::default(), captured, aspect: frame_aspect, events: std::mem::take(&mut events), check: None };
                        for field in fields {
                            let bad = || format!("`{field}` is not part of a frame");
                            if field == "esc" {
                                frame.input.escape = true;
                                continue;
                            }
                            let (name, value) = field.split_once('=').ok_or_else(bad)?;
                            match name {
                                "h" => keys_held = parse_list(value, parse_bind).ok_or_else(bad)?,
                                "m" => buttons_held = parse_list(value, parse_button).ok_or_else(bad)?,
                                "p" => frame.input.keys.pressed = parse_list(value, parse_bind).ok_or_else(bad)?,
                                "r" => frame.input.keys.released = parse_list(value, parse_bind).ok_or_else(bad)?,
                                "mp" => frame.input.buttons_pressed = parse_list(value, parse_button).ok_or_else(bad)?,
                                "mr" => frame.input.buttons_released = parse_list(value, parse_button).ok_or_else(bad)?,
                                "d" => {
                                    let (x, y) = value.split_once(',').ok_or_else(bad)?;
                                    frame.input.motion = Vec2::new(unbits(x).ok_or_else(bad)?, unbits(y).ok_or_else(bad)?);
                                }
                                "cap" => captured = value == "1",
                                "asp" => frame_aspect = unbits(value).ok_or_else(bad)?,
                                _ => return Err(bad()),
                            }
                        }
                        frame.captured = captured;
                        frame.aspect = frame_aspect;
                        frame.input.keys.held = keys_held.clone();
                        frame.input.buttons_held = buttons_held.clone();
                        frames.push(frame);
                    }
                    "c" => {
                        let frame = frames.last_mut().ok_or("a position with no frame before it")?;
                        frame.check = Some(parse_pose(rest).ok_or("a position that is not one")?);
                    }
                    "e" => {
                        let (kind, value) = rest.split_once(' ').unwrap_or((rest, ""));
                        events.push(match kind {
                            "screens" => {
                                let mut flags = value.split_whitespace().map(|v| v == "1");
                                Event::Screens { menu: flags.next().ok_or("screens with no flags")?, inventory: flags.next().ok_or("screens with no flags")? }
                            }
                            "inventory" => Event::Inventory(Inventory::from_text(value).ok_or("an inventory that is not one")?),
                            "health" => Event::Health(PlayerHealth::from_text(value).ok_or("health that is not")?),
                            "thrown" => Event::Thrown(parse_item(value).ok_or("an item that is not one")?),
                            other => return Err(format!("no event called `{other}`")),
                        });
                    }
                    _ if in_frames => return Err(format!("`{word}` is not a line of a recording")),
                    "game" => game = rest.to_string(),
                    "seed" => seed = Some(rest.parse::<u64>().map_err(|_| "a seed that is not a number")?),
                    "control" => control_lines.push(rest.to_string()),
                    "start-pose" => pose = Some(parse_pose(rest).ok_or("a pose that is not one")?),
                    "start-health" => health = Some(PlayerHealth::from_text(rest).ok_or("health that is not")?),
                    "start-inventory" => inventory = Some(Inventory::from_text(rest).ok_or("an inventory that is not one")?),
                    "start-dice" => {
                        let (a, b) = rest.split_once(' ').ok_or("dice with no count")?;
                        dice = Some((u64::from_str_radix(a, 16).map_err(|_| "dice that are not")?, b.parse::<u32>().map_err(|_| "dice that are not")?));
                    }
                    "start-gun-rng" => gun_rng = Some(u32::from_str_radix(rest, 16).map_err(|_| "a generator that is not one")?),
                    "start-screens" => {
                        let flags: Vec<bool> = rest.split_whitespace().map(|v| v == "1").collect();
                        if flags.len() != 3 {
                            return Err("screens need three flags".into());
                        }
                        screens = Some((flags[0], flags[1], flags[2]));
                    }
                    "start-aspect" => aspect = unbits(rest).ok_or("an aspect that is not one")?,
                    "start-fresh" => fresh = rest == "1",
                    // A line from a newer game is skipped, not fatal.
                    _ => {}
                }
                Ok(())
            })();
            if let Err(message) = outcome {
                if last {
                    break;
                }
                return Err(format!("line {number}: {message}"));
            }
        }

        let (controls, _) = Controls::from_text(&control_lines.join("\n"));
        let (menu_open, inventory_open, start_captured) = screens.ok_or("it has no start-screens line")?;
        let header = Header {
            game,
            seed: seed.ok_or("it has no seed")?,
            controls,
            start: Start {
                pose: pose.ok_or("it has no start-pose line")?,
                health: health.ok_or("it has no start-health line")?,
                inventory: inventory.ok_or("it has no start-inventory line")?,
                dice,
                gun_rng: gun_rng.ok_or("it has no start-gun-rng line")?,
                menu_open,
                inventory_open,
                captured: start_captured,
                aspect,
                fresh,
            },
        };
        Ok(Recording { header, frames })
    }
}

fn pose_text(pose: &Pose) -> String {
    format!("{},{},{} {} {}", bits(pose.position.x), bits(pose.position.y), bits(pose.position.z), bits(pose.yaw), bits(pose.pitch))
}

fn parse_pose(text: &str) -> Option<Pose> {
    let mut parts = text.split_whitespace();
    let mut at = parts.next()?.split(',');
    let position = Vec3::new(unbits(at.next()?)?, unbits(at.next()?)?, unbits(at.next()?)?);
    let yaw = unbits(parts.next()?)?;
    let pitch = unbits(parts.next()?)?;
    (parts.next().is_none() && at.next().is_none()).then_some(Pose { position, yaw, pitch })
}

// ---- writing --------------------------------------------------------------------------------------

/// Turns the header and then frames into text, leaving out what has not changed since the frame before.
pub struct Encoder {
    keys_held: Option<Vec<Bind>>,
    buttons_held: Option<Vec<MouseButton>>,
    captured: bool,
    aspect: f32,
}

impl Encoder {
    pub fn new(header: &Header) -> Encoder {
        Encoder { keys_held: None, buttons_held: None, captured: header.start.captured, aspect: header.start.aspect }
    }

    pub fn header(&self, header: &Header) -> String {
        let start = &header.start;
        let mut text = format!("{MAGIC} {FORMAT}\ngame {}\nseed {}\n", header.game, header.seed);
        for line in header.controls.to_text().lines().filter(|l| !l.starts_with('#')) {
            text += &format!("control {line}\n");
        }
        text += &format!("start-pose {}\n", pose_text(&start.pose));
        text += &format!("start-health {}\n", start.health.to_text());
        text += &format!("start-inventory {}\n", start.inventory.to_text());
        if let Some((state, count)) = start.dice {
            text += &format!("start-dice {state:x} {count}\n");
        }
        text += &format!("start-gun-rng {:x}\n", start.gun_rng);
        text += &format!("start-screens {} {} {}\n", u8::from(start.menu_open), u8::from(start.inventory_open), u8::from(start.captured));
        text += &format!("start-aspect {}\n", bits(start.aspect));
        text += &format!("start-fresh {}\n", u8::from(start.fresh));
        text
    }

    pub fn frame(&mut self, frame: &Frame) -> String {
        let mut text = String::new();
        for event in &frame.events {
            text += &match event {
                Event::Screens { menu, inventory } => format!("e screens {} {}\n", u8::from(*menu), u8::from(*inventory)),
                Event::Inventory(inventory) => format!("e inventory {}\n", inventory.to_text()),
                Event::Health(health) => format!("e health {}\n", health.to_text()),
                Event::Thrown(item) => format!("e thrown {}\n", item_text(item)),
            };
        }
        let input = &frame.input;
        text += &format!("f {}", frame.nanos);
        if self.keys_held.as_ref() != Some(&input.keys.held) {
            text += &format!(" h={}", list(&input.keys.held, bind_token));
            self.keys_held = Some(input.keys.held.clone());
        }
        if self.buttons_held.as_ref() != Some(&input.buttons_held) {
            text += &format!(" m={}", list(&input.buttons_held, button_token));
            self.buttons_held = Some(input.buttons_held.clone());
        }
        if !input.keys.pressed.is_empty() {
            text += &format!(" p={}", list(&input.keys.pressed, bind_token));
        }
        if !input.keys.released.is_empty() {
            text += &format!(" r={}", list(&input.keys.released, bind_token));
        }
        if !input.buttons_pressed.is_empty() {
            text += &format!(" mp={}", list(&input.buttons_pressed, button_token));
        }
        if !input.buttons_released.is_empty() {
            text += &format!(" mr={}", list(&input.buttons_released, button_token));
        }
        if input.motion != Vec2::ZERO {
            text += &format!(" d={},{}", bits(input.motion.x), bits(input.motion.y));
        }
        if input.escape {
            text += " esc";
        }
        if frame.captured != self.captured {
            self.captured = frame.captured;
            text += &format!(" cap={}", u8::from(frame.captured));
        }
        if frame.aspect.to_bits() != self.aspect.to_bits() {
            self.aspect = frame.aspect;
            text += &format!(" asp={}", bits(frame.aspect));
        }
        text.push('\n');
        if let Some(pose) = &frame.check {
            text += &format!("c {}\n", pose_text(pose));
        }
        text
    }
}

/// A recording being written to a file as it is made, so that what was recorded is there if the game stops.
pub struct Writer {
    out: BufWriter<File>,
    encoder: Encoder,
    pub path: PathBuf,
    pub frames: u64,
}

impl Writer {
    pub fn create(path: &Path, header: &Header) -> io::Result<Writer> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut out = BufWriter::new(File::create(path)?);
        let encoder = Encoder::new(header);
        out.write_all(encoder.header(header).as_bytes())?;
        Ok(Writer { out, encoder, path: path.to_path_buf(), frames: 0 })
    }

    pub fn frame(&mut self, frame: &Frame) -> io::Result<()> {
        self.out.write_all(self.encoder.frame(frame).as_bytes())?;
        self.frames += 1;
        // Often enough that a crash loses a moment, not the session.
        if self.frames % 120 == 0 {
            self.out.flush()?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> io::Result<()> {
        self.out.flush()
    }
}

// ---- where recordings are kept -------------------------------------------------------------------

/// Where new recordings go: `$FPS_RECORDINGS_DIR` if set; else `recordings` in the game's own folder (the launcher
/// says which with `PANIC_AND_IMPROVISE_HOME`); else in the user's data directory.
pub fn recordings_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FPS_RECORDINGS_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(home) = std::env::var_os("PANIC_AND_IMPROVISE_HOME").filter(|v| !v.is_empty()) {
        return PathBuf::from(home).join("recordings");
    }
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        if let Some(base) = var("LOCALAPPDATA") {
            return base.join("PanicAndImprovise").join("recordings");
        }
    } else if let Some(base) = var("XDG_DATA_HOME") {
        return base.join("panic_and_improvise").join("recordings");
    } else if let Some(home) = var("HOME") {
        return home.join(".local/share/panic_and_improvise/recordings");
    }
    PathBuf::from("recordings")
}

/// A name for a recording made at `unix_seconds` (UTC): `recording-20261025-153012.pir`.
pub fn recording_name(unix_seconds: u64) -> String {
    let days = (unix_seconds / 86_400) as i64;
    let in_day = unix_seconds % 86_400;
    // Civil date from a day count (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("recording-{year:04}{month:02}{day:02}-{:02}{:02}{:02}.{EXTENSION}", in_day / 3600, in_day % 3600 / 60, in_day % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::Action;
    use crate::weapons::WeaponKind;

    fn start() -> Start {
        Start {
            pose: Pose { position: Vec3::new(-332.9, 12.5, 2084.8), yaw: 3.14, pitch: -0.25 },
            health: PlayerHealth::default(),
            inventory: Inventory::starting(),
            dice: Some((0xDEAD_BEEF_1234, 17)),
            gun_rng: 0x9E37_79B9,
            menu_open: false,
            inventory_open: false,
            captured: true,
            aspect: 16.0 / 9.0,
            fresh: true,
        }
    }

    fn header() -> Header {
        let mut controls = Controls::preset(crate::controls::Preset::ColemakModDh);
        controls.set(Action::Reload, Bind::Char('g'));
        Header { game: "0.1.57".into(), seed: 2026, controls, start: start() }
    }

    fn frame(nanos: u64, held: &[Bind]) -> Frame {
        Frame {
            nanos,
            input: Input { keys: KeyboardFrame { held: held.to_vec(), ..default() }, ..default() },
            captured: true,
            aspect: 16.0 / 9.0,
            events: Vec::new(),
            check: None,
        }
    }

    fn busy() -> Recording {
        let mut recording = Recording { header: header(), frames: vec![frame(0, &[]), frame(16_666_667, &[Bind::Char('w')])] };
        let mut third = frame(16_000_001, &[Bind::Char('w'), Bind::Shift]);
        third.input.keys.pressed = vec![Bind::Shift];
        third.input.motion = Vec2::new(3.5, -1.25);
        third.input.buttons_held = vec![MouseButton::Left];
        third.input.buttons_pressed = vec![MouseButton::Left];
        third.check = Some(Pose { position: Vec3::new(1.0, 2.0, 3.0), yaw: 0.5, pitch: -0.5 });
        recording.frames.push(third);
        let mut fourth = frame(17_000_000, &[Bind::Char(',')]);
        fourth.input.keys.released = vec![Bind::Char('w'), Bind::Shift];
        fourth.input.buttons_released = vec![MouseButton::Left];
        fourth.input.escape = true;
        fourth.captured = false;
        fourth.aspect = 1.0;
        fourth.events = vec![
            Event::Screens { menu: true, inventory: false },
            Event::Inventory(Inventory::default()),
            Event::Health({
                let mut h = PlayerHealth::default();
                h.hurt(20.0);
                h
            }),
            Event::Thrown({
                let mut inv = Inventory::default();
                inv.make_gun(WeaponKind::Mag, crate::ammo::AmmoKind::NatoBall, 5)
            }),
        ];
        recording.frames.push(fourth);
        recording
    }

    #[test]
    fn a_recording_reads_back_exactly_as_it_was_written() {
        let recording = busy();
        let text = recording.to_text();
        assert_eq!(Recording::parse(&text), Ok(recording));
    }

    #[test]
    fn floats_and_keys_that_are_awkward_as_text_come_back_exactly() {
        let mut recording = busy();
        recording.frames[1].input.motion = Vec2::new(f32::MIN_POSITIVE, -0.1);
        recording.frames[1].input.keys.held = vec![Bind::Char(','), Bind::Char('#'), Bind::Char('é'), Bind::Space, Bind::Tab];
        recording.frames[1].input.keys.held.sort_by_key(|k| k.name());
        assert_eq!(Recording::parse(&recording.to_text()), Ok(recording));
    }

    #[test]
    fn the_shape_of_the_window_comes_back_frame_by_frame() {
        let mut recording = busy();
        recording.frames[1].aspect = 2.5;
        recording.frames[2].aspect = 2.5;
        let read = Recording::parse(&recording.to_text()).unwrap();
        let aspects: Vec<f32> = read.frames.iter().map(|f| f.aspect).collect();
        assert_eq!(aspects, [16.0 / 9.0, 2.5, 2.5, 1.0, ]);
        assert_eq!(read, recording);
    }

    #[test]
    fn what_has_not_changed_since_the_frame_before_is_not_written_again() {
        let text = busy().to_text();
        let frames: Vec<&str> = text.lines().filter(|l| l.starts_with("f ")).collect();
        assert!(frames[0].contains("h=-"), "the first frame says what is held: {}", frames[0]);
        assert!(frames[1].contains("h=#77"), "{}", frames[1]);
        let steady = Recording { header: header(), frames: vec![frame(1, &[Bind::Char('w')]), frame(2, &[Bind::Char('w')]), frame(3, &[Bind::Char('w')])] };
        let text = steady.to_text();
        let lines: Vec<&str> = text.lines().filter(|l| l.starts_with("f ")).collect();
        assert_eq!(lines[1], "f 2");
        assert_eq!(lines[2], "f 3");
        assert_eq!(Recording::parse(&text), Ok(steady));
    }

    #[test]
    fn a_last_line_cut_short_is_dropped_and_the_rest_is_kept() {
        let recording = busy();
        let text = recording.to_text();
        let cut = &text[..text.rfind('\n').unwrap() - 4];
        let read = Recording::parse(cut).expect("what was written before the cut is there");
        assert!(read.frames.len() >= 3 && read.frames.len() <= recording.frames.len());
        assert_eq!(read.frames[..3], recording.frames[..3]);
    }

    #[test]
    fn a_line_in_the_middle_that_makes_no_sense_is_an_error_with_its_number() {
        let text = busy().to_text().replace("f 16666667", "f sixteen");
        let error = Recording::parse(&text).unwrap_err();
        assert!(error.starts_with("line "), "{error}");
        assert!(error.contains("no length"), "{error}");
    }

    #[test]
    fn something_that_is_not_a_recording_is_refused_plainly() {
        assert!(Recording::parse("").unwrap_err().contains("empty"));
        assert!(Recording::parse("hello\nworld").unwrap_err().contains("not a recording"));
        let newer = busy().to_text().replacen(&format!("{MAGIC} {FORMAT}"), &format!("{MAGIC} 9"), 1);
        assert!(Recording::parse(&newer).unwrap_err().contains("format 9"));
        let headless = format!("{MAGIC} {FORMAT}\nseed 1\n");
        assert!(Recording::parse(&headless).is_err());
    }

    #[test]
    fn the_controls_it_was_played_on_come_with_it() {
        let read = Recording::parse(&busy().to_text()).unwrap();
        assert_eq!(read.header.controls.bind(Action::Reload), Bind::Char('g'));
        assert_eq!(read.header.controls.base(), crate::controls::Preset::ColemakModDh);
    }

    #[test]
    fn it_is_as_long_as_its_frames() {
        let recording = Recording { header: header(), frames: vec![frame(500_000_000, &[]), frame(250_000_000, &[])] };
        assert!((recording.seconds() - 0.75).abs() < 1e-6);
    }

    #[test]
    fn a_recording_goes_to_a_file_as_it_is_made_and_comes_back_from_it() {
        let dir = std::env::temp_dir().join(format!("recording-test-{}", std::process::id()));
        let path = dir.join("nested").join("run.pir");
        let recording = busy();
        let mut writer = Writer::create(&path, &recording.header).unwrap();
        for f in &recording.frames {
            writer.frame(f).unwrap();
        }
        writer.finish().unwrap();
        assert_eq!(Recording::read(&path), Ok(recording));
        assert!(Recording::read(&dir.join("missing.pir")).unwrap_err().contains("could not read"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_recording_is_named_for_the_moment_it_was_made() {
        assert_eq!(recording_name(0), "recording-19700101-000000.pir");
        // 2026-10-25 15:30:12 UTC
        assert_eq!(recording_name(1_792_942_212), "recording-20261025-153012.pir");
        // A leap day.
        assert_eq!(recording_name(1_709_209_496), "recording-20240229-122456.pir");
    }

    #[test]
    fn recordings_go_where_they_are_told() {
        // Only this test touches the variable.
        unsafe { std::env::set_var("FPS_RECORDINGS_DIR", "/somewhere/else") };
        assert_eq!(recordings_dir(), PathBuf::from("/somewhere/else"));
        unsafe { std::env::remove_var("FPS_RECORDINGS_DIR") };
    }
}
