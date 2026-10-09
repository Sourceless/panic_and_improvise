//! What the keys do: the actions the player can take, which key each is bound to, presets of those
//! bindings, and saving them.
//!
//! Keys are bound by what they type (the letter on the keycap you're looking at), not by where they sit
//! on the board. That is what makes a Colemak preset mean something: with a Colemak layout set in the
//! operating system or in the keyboard's own firmware, the key under your left middle finger types an
//! R, and a game that bound to *positions* would call it S. The names here are the letters, so a
//! saved profile works either way.
//!
//! The mouse buttons aren't rebindable (fire and aim are always the left and right buttons).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::{ButtonState, InputSystems};
use bevy::prelude::*;
use bevy::window::WindowFocused;

/// Everything the keyboard does in the game.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Action {
    Forward,
    Back,
    Left,
    Right,
    Sprint,
    Jump,
    Crouch,
    Prone,
    Reload,
    /// The inventory screen.
    Inventory,
    /// Pick up what is in front of you.
    Interact,
    /// Take up the gun in the first or the second weapon slot.
    Slot1,
    Slot2,
    /// Semi-automatic or automatic, on a gun that can do both.
    FireMode,
    /// The load that goes in at the next reload.
    CycleAmmo,
}

impl Action {
    pub const ALL: [Action; 15] = [
        Action::Forward,
        Action::Back,
        Action::Left,
        Action::Right,
        Action::Sprint,
        Action::Jump,
        Action::Crouch,
        Action::Prone,
        Action::Reload,
        Action::Inventory,
        Action::Interact,
        Action::Slot1,
        Action::Slot2,
        Action::FireMode,
        Action::CycleAmmo,
    ];

    /// What it's called on screen.
    pub fn label(self) -> &'static str {
        match self {
            Action::Forward => "Move forward",
            Action::Back => "Move back",
            Action::Left => "Move left",
            Action::Right => "Move right",
            Action::Sprint => "Sprint (with forward)",
            Action::Jump => "Jump / climb / stand up",
            Action::Crouch => "Crouch",
            Action::Prone => "Go prone",
            Action::Reload => "Reload",
            Action::Inventory => "Inventory",
            Action::Interact => "Pick up / use",
            Action::Slot1 => "First weapon",
            Action::Slot2 => "Second weapon",
            Action::FireMode => "Fire mode",
            Action::CycleAmmo => "Change ammunition",
        }
    }

    /// What it's called in a saved file.
    pub fn id(self) -> &'static str {
        match self {
            Action::Forward => "forward",
            Action::Back => "back",
            Action::Left => "left",
            Action::Right => "right",
            Action::Sprint => "sprint",
            Action::Jump => "jump",
            Action::Crouch => "crouch",
            Action::Prone => "prone",
            Action::Reload => "reload",
            Action::Inventory => "inventory",
            Action::Interact => "interact",
            Action::Slot1 => "slot1",
            Action::Slot2 => "slot2",
            Action::FireMode => "fire_mode",
            Action::CycleAmmo => "cycle_ammo",
        }
    }

    pub fn from_id(id: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.id() == id)
    }
}

/// A key, as the player knows it: a letter or symbol (always lower case), or one of the keys that
/// don't type anything.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Bind {
    Char(char),
    Shift,
    Control,
    Alt,
    Space,
    Tab,
    CapsLock,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
}

impl Bind {
    const NAMED: [(Bind, &'static str); 10] = [
        (Bind::Shift, "Shift"),
        (Bind::Control, "Ctrl"),
        (Bind::Alt, "Alt"),
        (Bind::Space, "Space"),
        (Bind::Tab, "Tab"),
        (Bind::CapsLock, "CapsLock"),
        (Bind::ArrowUp, "Up"),
        (Bind::ArrowDown, "Down"),
        (Bind::ArrowLeft, "Left"),
        (Bind::ArrowRight, "Right"),
    ];

    /// The bindable key a logical key is, if it is one.
    pub fn from_key(key: &Key) -> Option<Bind> {
        match key {
            Key::Character(text) => {
                let mut chars = text.chars();
                let c = chars.next()?;
                // One character only (not a dead key's composed pair); shifted is the same key.
                chars.next().is_none().then(|| Bind::Char(c.to_lowercase().next().unwrap_or(c)))
            }
            Key::Shift => Some(Bind::Shift),
            Key::Control => Some(Bind::Control),
            Key::Alt => Some(Bind::Alt),
            Key::Space => Some(Bind::Space),
            Key::Tab => Some(Bind::Tab),
            Key::CapsLock => Some(Bind::CapsLock),
            Key::ArrowUp => Some(Bind::ArrowUp),
            Key::ArrowDown => Some(Bind::ArrowDown),
            Key::ArrowLeft => Some(Bind::ArrowLeft),
            Key::ArrowRight => Some(Bind::ArrowRight),
            _ => None,
        }
    }

    /// How it is shown and saved: the character, or the key's name.
    pub fn name(self) -> String {
        match self {
            Bind::Char(c) => c.to_string(),
            other => Bind::NAMED.iter().find(|(b, _)| *b == other).map_or_else(String::new, |(_, n)| (*n).to_string()),
        }
    }

    /// What is shown on screen: letters in capitals.
    pub fn label(self) -> String {
        match self {
            Bind::Char(c) => c.to_uppercase().to_string(),
            other => other.name(),
        }
    }

    pub fn parse(text: &str) -> Option<Bind> {
        let text = text.trim();
        let mut chars = text.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            return Some(Bind::Char(c.to_lowercase().next().unwrap_or(c)));
        }
        Bind::NAMED.iter().find(|(_, n)| n.eq_ignore_ascii_case(text)).map(|(b, _)| *b)
    }
}

/// A named set of bindings.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Preset {
    #[default]
    Qwerty,
    ColemakModDh,
}

impl Preset {
    pub const ALL: [Preset; 2] = [Preset::Qwerty, Preset::ColemakModDh];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Qwerty => "QWERTY",
            Preset::ColemakModDh => "Colemak Mod-DH",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Preset::Qwerty => "qwerty",
            Preset::ColemakModDh => "colemak_mod_dh",
        }
    }

    pub fn from_id(id: &str) -> Option<Preset> {
        let id = id.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        Preset::ALL.into_iter().find(|p| p.id() == id)
    }

    /// The key an action is on, in this preset.
    ///
    /// QWERTY is W A S D to move, shift to sprint, space to jump, C, Z and R. Colemak Mod-DH keeps every
    /// key where it is on the board: the letters that sit where QWERTY's W, A, S, D, C, Z and R do are
    /// W, A, R, S, C, Z and P.
    pub fn bind(self, action: Action) -> Bind {
        let c = |key: char| Bind::Char(key);
        match (self, action) {
            (_, Action::Sprint) => Bind::Shift,
            (_, Action::Jump) => Bind::Space,
            (_, Action::Inventory) => Bind::Tab,
            (_, Action::Slot1) => c('1'),
            (_, Action::Slot2) => c('2'),
            (Preset::Qwerty, Action::Interact) => c('e'),
            (Preset::Qwerty, Action::FireMode) => c('v'),
            (Preset::Qwerty, Action::CycleAmmo) => c('b'),
            (Preset::ColemakModDh, Action::Interact) => c('f'),
            (Preset::ColemakModDh, Action::FireMode) => c('d'),
            (Preset::ColemakModDh, Action::CycleAmmo) => c('v'),
            (Preset::Qwerty, Action::Forward) => c('w'),
            (Preset::Qwerty, Action::Back) => c('s'),
            (Preset::Qwerty, Action::Left) => c('a'),
            (Preset::Qwerty, Action::Right) => c('d'),
            (Preset::Qwerty, Action::Crouch) => c('c'),
            (Preset::Qwerty, Action::Prone) => c('z'),
            (Preset::Qwerty, Action::Reload) => c('r'),
            (Preset::ColemakModDh, Action::Forward) => c('w'),
            (Preset::ColemakModDh, Action::Back) => c('r'),
            (Preset::ColemakModDh, Action::Left) => c('a'),
            (Preset::ColemakModDh, Action::Right) => c('s'),
            (Preset::ColemakModDh, Action::Crouch) => c('c'),
            (Preset::ColemakModDh, Action::Prone) => c('z'),
            (Preset::ColemakModDh, Action::Reload) => c('p'),
        }
    }
}

/// The player's bindings: a preset, with any keys changed from it.
#[derive(Resource, Clone, Debug, PartialEq, Default)]
pub struct Controls {
    preset: Preset,
    changes: HashMap<Action, Bind>,
}

impl Controls {
    pub fn preset(preset: Preset) -> Controls {
        Controls { preset, changes: HashMap::new() }
    }

    /// The preset these bindings started from.
    pub fn base(&self) -> Preset {
        self.preset
    }

    pub fn bind(&self, action: Action) -> Bind {
        self.changes.get(&action).copied().unwrap_or_else(|| self.preset.bind(action))
    }

    /// Whether any key has been changed from the preset.
    pub fn is_customised(&self) -> bool {
        !self.changes.is_empty()
    }

    /// Start again from a preset, dropping any changes.
    pub fn use_preset(&mut self, preset: Preset) {
        *self = Controls::preset(preset);
    }

    /// Puts `action` on `key`. If another action is on that key it takes the one `action` had, so no
    /// key is ever left doing two things.
    pub fn set(&mut self, action: Action, key: Bind) {
        let old = self.bind(action);
        if old == key {
            return;
        }
        if let Some(other) = Action::ALL.into_iter().find(|&a| a != action && self.bind(a) == key) {
            self.change(other, old);
        }
        self.change(action, key);
    }

    fn change(&mut self, action: Action, key: Bind) {
        if self.preset.bind(action) == key {
            self.changes.remove(&action);
        } else {
            self.changes.insert(action, key);
        }
    }

    pub fn pressed(&self, action: Action, keys: &Keyboard) -> bool {
        keys.pressed(self.bind(action))
    }

    pub fn just_pressed(&self, action: Action, keys: &Keyboard) -> bool {
        keys.just_pressed(self.bind(action))
    }

    // ---- saving and loading -----------------------------------------------------------------

    /// The text of a settings file: the preset, then a line for each key changed from it.
    pub fn to_text(&self) -> String {
        let mut text = String::from("# Controls. `preset` is the base layout; later lines change single keys.\n");
        text += &format!("preset = {}\n", self.preset.id());
        for action in Action::ALL {
            if let Some(key) = self.changes.get(&action) {
                text += &format!("{} = {}\n", action.id(), key.name());
            }
        }
        text
    }

    /// Reads the text of a settings file. Anything it can't make sense of is skipped and reported,
    /// not fatal, so a hand-edited file with a typo still loads.
    pub fn from_text(text: &str) -> (Controls, Vec<String>) {
        let mut controls = Controls::default();
        let mut problems = Vec::new();
        let mut lines = Vec::new();
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((name, value)) = line.split_once('=') else {
                problems.push(format!("line {}: expected `name = value`", number + 1));
                continue;
            };
            lines.push((number + 1, name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
        // The preset first, whichever order they're written in, then the changes on top of it.
        for (number, name, value) in &lines {
            if name == "preset" {
                match Preset::from_id(value) {
                    Some(preset) => controls = Controls::preset(preset),
                    None => problems.push(format!("line {number}: no preset called `{value}`")),
                }
            }
        }
        for (number, name, value) in &lines {
            if name == "preset" {
                continue;
            }
            match (Action::from_id(name), Bind::parse(value)) {
                (Some(action), Some(key)) => controls.set(action, key),
                (None, _) => problems.push(format!("line {number}: no action called `{name}`")),
                (_, None) => problems.push(format!("line {number}: `{value}` isn't a key")),
            }
        }
        (controls, problems)
    }

    /// The saved controls, or the default (QWERTY) if there is no file or it can't be read.
    pub fn load(path: &Path) -> Controls {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let (controls, problems) = Controls::from_text(&text);
                for problem in problems {
                    warn!("{}: {problem}", path.display());
                }
                controls
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Controls::default(),
            Err(error) => {
                warn!("could not read {}: {error}", path.display());
                Controls::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.to_text())
    }
}

/// Where settings are kept: `$FPS_CONFIG_DIR` if set (tests use that), else the user's config
/// directory (`$XDG_CONFIG_HOME` or `~/.config`) under `fps_prototype`.
pub fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("FPS_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("fps_prototype")
}

pub fn controls_path() -> PathBuf {
    config_dir().join("controls.txt")
}

// ---- the keyboard, by what the keys type --------------------------------------------------------

/// Which keys are held down, by what they type.
///
/// A key's meaning is fixed when it goes down (it is looked up on the board's own physical key, so
/// pressing shift while holding W doesn't turn the held W into a different, never-released key).
#[derive(Resource, Default)]
pub struct Keyboard {
    held: HashMap<Held, Bind>,
    pressed_now: HashSet<Bind>,
    released_now: HashSet<Bind>,
    last_press: Option<Bind>,
}

/// What a held key is tracked by: its place on the board, or (for tests and scripts) just its name.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Held {
    Physical(KeyCode),
    Named(Bind),
}

impl Keyboard {
    pub fn pressed(&self, key: Bind) -> bool {
        self.held.values().any(|&held| held == key)
    }

    /// Went down this frame.
    pub fn just_pressed(&self, key: Bind) -> bool {
        self.pressed_now.contains(&key)
    }

    pub fn just_released(&self, key: Bind) -> bool {
        self.released_now.contains(&key)
    }

    /// The key that went down this frame, if any (for rebinding).
    pub fn last_press(&self) -> Option<Bind> {
        self.last_press
    }

    /// Everything let go of.
    pub fn release_all(&mut self) {
        self.held.clear();
    }

    /// Everything let go of, and nothing pressed or released this frame either.
    pub fn clear(&mut self) {
        *self = Keyboard::default();
    }

    /// A key goes down, for tests and scripts that have no physical keyboard.
    pub fn press(&mut self, key: Bind) {
        if self.held.insert(Held::Named(key), key).is_none() {
            self.pressed_now.insert(key);
            self.last_press = Some(key);
        }
    }

    pub fn release(&mut self, key: Bind) {
        if self.held.remove(&Held::Named(key)).is_some() {
            self.released_now.insert(key);
        }
    }

    /// Forgets what happened last frame.
    pub fn new_frame(&mut self) {
        self.pressed_now.clear();
        self.released_now.clear();
        self.last_press = None;
    }

    pub fn apply(&mut self, event: &KeyboardInput) {
        match event.state {
            ButtonState::Pressed if !event.repeat => {
                if let Some(key) = Bind::from_key(&event.logical_key) {
                    self.held.insert(Held::Physical(event.key_code), key);
                    self.pressed_now.insert(key);
                    self.last_press = Some(key);
                }
            }
            ButtonState::Released => {
                if let Some(key) = self.held.remove(&Held::Physical(event.key_code)) {
                    self.released_now.insert(key);
                }
            }
            _ => {}
        }
    }
}

#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ControlsSet;

fn track_keyboard(mut events: MessageReader<KeyboardInput>, mut focus: MessageReader<WindowFocused>, mut keyboard: ResMut<Keyboard>) {
    keyboard.new_frame();
    // Keys held when the window loses focus never see their release, so they'd stay "down".
    if focus.read().any(|event| !event.focused) {
        keyboard.release_all();
    }
    for event in events.read() {
        keyboard.apply(event);
    }
}

/// Loads the saved controls (or QWERTY, if there are none) when the game starts.
pub struct ControlsPlugin {
    pub path: PathBuf,
}

impl Default for ControlsPlugin {
    fn default() -> Self {
        ControlsPlugin { path: controls_path() }
    }
}

impl Plugin for ControlsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Controls::load(&self.path))
            .init_resource::<Keyboard>()
            .add_systems(PreUpdate, track_keyboard.in_set(ControlsSet).after(InputSystems));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qwerty_is_the_default_and_is_wasd() {
        let c = Controls::default();
        assert_eq!(c.base(), Preset::Qwerty);
        let keys: Vec<String> = [Action::Forward, Action::Left, Action::Back, Action::Right].iter().map(|&a| c.bind(a).name()).collect();
        assert_eq!(keys, ["w", "a", "s", "d"]);
        assert_eq!(c.bind(Action::Sprint), Bind::Shift);
        assert_eq!(c.bind(Action::Jump), Bind::Space);
        assert_eq!((c.bind(Action::Crouch), c.bind(Action::Prone), c.bind(Action::Reload)), (Bind::Char('c'), Bind::Char('z'), Bind::Char('r')));
        assert!(!c.is_customised());
    }

    /// Colemak Mod-DH, ANSI: the letters on each row, left to right.
    const COLEMAK_DH_ROWS: [&str; 3] = ["qwfpbjluy;", "arstgmneio", "zxcdvkh,./"];
    const QWERTY_ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl;", "zxcvbnm,./"];

    /// The letter Colemak Mod-DH puts where QWERTY puts `letter`.
    fn colemak_dh_at(letter: char) -> char {
        for (qwerty, colemak) in QWERTY_ROWS.iter().zip(COLEMAK_DH_ROWS) {
            if let Some(i) = qwerty.chars().position(|c| c == letter) {
                return colemak.chars().nth(i).unwrap();
            }
        }
        panic!("{letter} isn't on the board");
    }

    #[test]
    fn colemak_mod_dh_keeps_every_key_where_the_qwerty_default_is() {
        // Each QWERTY letter binding, mapped through the layout, is the Colemak Mod-DH binding.
        for action in [Action::Forward, Action::Back, Action::Left, Action::Right, Action::Crouch, Action::Prone, Action::Reload] {
            let Bind::Char(qwerty) = Preset::Qwerty.bind(action) else { panic!("{action:?} is a letter") };
            assert_eq!(Preset::ColemakModDh.bind(action), Bind::Char(colemak_dh_at(qwerty)), "{action:?}");
        }
        // The keys that aren't letters are the same.
        assert_eq!(Preset::ColemakModDh.bind(Action::Sprint), Bind::Shift);
        assert_eq!(Preset::ColemakModDh.bind(Action::Jump), Bind::Space);
    }

    #[test]
    fn colemak_mod_dh_moves_on_w_a_r_s() {
        let c = Controls::preset(Preset::ColemakModDh);
        let keys: Vec<String> = [Action::Forward, Action::Left, Action::Back, Action::Right].iter().map(|&a| c.bind(a).name()).collect();
        assert_eq!(keys, ["w", "a", "r", "s"], "the keys under the same fingers as W A S D");
        assert_eq!(c.bind(Action::Reload), Bind::Char('p'), "where QWERTY's R is");
    }

    #[test]
    fn no_preset_puts_two_actions_on_one_key() {
        for preset in Preset::ALL {
            let keys: HashSet<Bind> = Action::ALL.iter().map(|&a| preset.bind(a)).collect();
            assert_eq!(keys.len(), Action::ALL.len(), "{preset:?}");
        }
    }

    #[test]
    fn rebinding_to_a_key_in_use_swaps_the_two() {
        let mut c = Controls::default();
        c.set(Action::Reload, Bind::Char('c'));
        assert_eq!(c.bind(Action::Reload), Bind::Char('c'));
        assert_eq!(c.bind(Action::Crouch), Bind::Char('r'), "crouch gets the key reload had");
        assert!(c.is_customised());
        let keys: HashSet<Bind> = Action::ALL.iter().map(|&a| c.bind(a)).collect();
        assert_eq!(keys.len(), Action::ALL.len());
    }

    #[test]
    fn putting_a_key_back_is_no_change() {
        let mut c = Controls::default();
        c.set(Action::Reload, Bind::Char('g'));
        assert!(c.is_customised());
        c.set(Action::Reload, Bind::Char('r'));
        assert!(!c.is_customised(), "back where the preset has it");
        assert_eq!(c, Controls::default());
    }

    #[test]
    fn choosing_a_preset_drops_changes() {
        let mut c = Controls::default();
        c.set(Action::Jump, Bind::Char('x'));
        c.use_preset(Preset::ColemakModDh);
        assert_eq!(c, Controls::preset(Preset::ColemakModDh));
    }

    #[test]
    fn settings_survive_saving_and_loading() {
        for preset in Preset::ALL {
            let mut c = Controls::preset(preset);
            assert_eq!(Controls::from_text(&c.to_text()), (c.clone(), vec![]), "{preset:?} as it is");
            c.set(Action::Reload, Bind::Char('g'));
            c.set(Action::Sprint, Bind::Control);
            c.set(Action::Jump, Bind::Char('='));
            let (loaded, problems) = Controls::from_text(&c.to_text());
            assert!(problems.is_empty(), "{problems:?}");
            assert_eq!(loaded, c, "{preset:?} with changes: {}", c.to_text());
        }
    }

    #[test]
    fn the_saved_file_reads_the_way_it_looks() {
        let text = Controls::preset(Preset::ColemakModDh).to_text();
        assert!(text.contains("preset = colemak_mod_dh"), "{text}");
        let mut changed = Controls::preset(Preset::Qwerty);
        changed.set(Action::Reload, Bind::Char('g'));
        assert!(changed.to_text().contains("reload = g"));
    }

    #[test]
    fn a_hand_written_file_is_read_leniently() {
        let (c, problems) = Controls::from_text("  # my keys\n\nforward = Z\nPreset = Colemak Mod-DH\nsprint = ctrl\njump = nonsense key\nflying = f\nbad line\npreset = elf\n");
        // The preset applies whatever order it is in, and the changes go on top.
        assert_eq!(c.base(), Preset::ColemakModDh);
        assert_eq!(c.bind(Action::Forward), Bind::Char('z'));
        assert_eq!(c.bind(Action::Sprint), Bind::Control);
        assert_eq!(c.bind(Action::Jump), Bind::Space, "a key it couldn't read leaves the default");
        assert_eq!(problems.len(), 4, "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("nonsense key")) && problems.iter().any(|p| p.contains("flying")) && problems.iter().any(|p| p.contains("elf")));
    }

    #[test]
    fn an_empty_or_missing_file_means_qwerty() {
        assert_eq!(Controls::from_text("").0, Controls::default());
        let dir = std::env::temp_dir().join(format!("fps_controls_missing_{}", std::process::id()));
        assert_eq!(Controls::load(&dir.join("controls.txt")), Controls::default());
    }

    #[test]
    fn controls_are_written_and_read_back_from_disk() {
        let dir = std::env::temp_dir().join(format!("fps_controls_disk_{}", std::process::id()));
        let path = dir.join("nested").join("controls.txt");
        let mut c = Controls::preset(Preset::ColemakModDh);
        c.set(Action::Crouch, Bind::Control);
        c.save(&path).expect("save creates the folder and writes the file");
        assert_eq!(Controls::load(&path), c);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keys_are_named_by_what_they_type() {
        assert_eq!(Bind::from_key(&Key::Character("W".into())), Some(Bind::Char('w')), "shifted is the same key");
        assert_eq!(Bind::from_key(&Key::Character("r".into())), Some(Bind::Char('r')));
        assert_eq!(Bind::from_key(&Key::Space), Some(Bind::Space));
        assert_eq!(Bind::from_key(&Key::Shift), Some(Bind::Shift));
        assert_eq!(Bind::from_key(&Key::Escape), None, "escape isn't bindable");
        assert_eq!(Bind::from_key(&Key::F3), None);
        assert_eq!(Bind::from_key(&Key::Character("ab".into())), None, "composed text isn't a key");
        assert_eq!(Bind::parse("Space"), Some(Bind::Space));
        assert_eq!(Bind::parse("R"), Some(Bind::Char('r')));
        assert_eq!(Bind::parse("shift"), Some(Bind::Shift));
        assert_eq!(Bind::parse("no such"), None);
        assert_eq!(Bind::Char('w').label(), "W");
        assert_eq!(Bind::Shift.label(), "Shift");
    }

    fn key_event(code: KeyCode, key: Key, state: ButtonState) -> KeyboardInput {
        KeyboardInput { key_code: code, logical_key: key, state, text: None, repeat: false, window: Entity::PLACEHOLDER }
    }

    #[test]
    fn a_held_key_stays_held_when_shift_changes_what_it_types() {
        // Press W, press shift, then let go of W while shift is down: the release is of a capital W,
        // but it is the same key and it must be released.
        let mut kb = Keyboard::default();
        kb.apply(&key_event(KeyCode::KeyW, Key::Character("w".into()), ButtonState::Pressed));
        kb.apply(&key_event(KeyCode::ShiftLeft, Key::Shift, ButtonState::Pressed));
        assert!(kb.pressed(Bind::Char('w')) && kb.pressed(Bind::Shift));
        kb.apply(&key_event(KeyCode::KeyW, Key::Character("W".into()), ButtonState::Released));
        assert!(!kb.pressed(Bind::Char('w')), "W is up");
        assert!(kb.pressed(Bind::Shift));
    }

    #[test]
    fn the_same_physical_key_is_a_different_letter_under_a_different_layout() {
        // The key at QWERTY's S position, on a Colemak layout, types R.
        let mut kb = Keyboard::default();
        kb.apply(&key_event(KeyCode::KeyS, Key::Character("r".into()), ButtonState::Pressed));
        let colemak = Controls::preset(Preset::ColemakModDh);
        let qwerty = Controls::default();
        assert!(colemak.pressed(Action::Back, &kb), "that is back for a Colemak player");
        assert!(qwerty.pressed(Action::Reload, &kb) && !qwerty.pressed(Action::Back, &kb), "and Reload for a QWERTY one");
    }

    #[test]
    fn just_pressed_lasts_one_frame_and_key_repeat_is_ignored() {
        let mut kb = Keyboard::default();
        kb.apply(&key_event(KeyCode::KeyR, Key::Character("r".into()), ButtonState::Pressed));
        assert!(kb.just_pressed(Bind::Char('r')) && kb.last_press() == Some(Bind::Char('r')));
        kb.new_frame();
        assert!(!kb.just_pressed(Bind::Char('r')) && kb.pressed(Bind::Char('r')) && kb.last_press().is_none());
        let mut repeat = key_event(KeyCode::KeyR, Key::Character("r".into()), ButtonState::Pressed);
        repeat.repeat = true;
        kb.apply(&repeat);
        assert!(!kb.just_pressed(Bind::Char('r')), "holding a key doesn't press it again");
        kb.apply(&key_event(KeyCode::KeyR, Key::Character("r".into()), ButtonState::Released));
        assert!(kb.just_released(Bind::Char('r')) && !kb.pressed(Bind::Char('r')));
    }

    #[test]
    fn keys_pressed_by_name_work_without_a_keyboard() {
        let mut kb = Keyboard::default();
        kb.press(Bind::Space);
        kb.press(Bind::Space);
        assert!(kb.pressed(Bind::Space) && kb.just_pressed(Bind::Space));
        kb.new_frame();
        kb.release(Bind::Space);
        assert!(!kb.pressed(Bind::Space) && kb.just_released(Bind::Space));
        kb.press(Bind::Shift);
        kb.release_all();
        assert!(!kb.pressed(Bind::Shift));
    }

    #[test]
    fn settings_go_where_the_config_variable_says() {
        // (Only the shape of it: the environment is shared between tests, so this doesn't set it.)
        assert!(controls_path().ends_with("controls.txt"));
        assert!(config_dir().ends_with("fps_prototype") || std::env::var_os("FPS_CONFIG_DIR").is_some());
    }

    #[test]
    fn the_game_starts_with_whatever_was_saved() {
        let dir = std::env::temp_dir().join(format!("fps_controls_launch_{}", std::process::id()));
        let path = dir.join("controls.txt");
        let mut saved = Controls::preset(Preset::ColemakModDh);
        saved.set(Action::Reload, Bind::Char('g'));
        saved.save(&path).unwrap();
        let mut app = App::new();
        app.add_plugins(ControlsPlugin { path: path.clone() });
        assert_eq!(*app.world().resource::<Controls>(), saved, "loaded on launch");
        // And with nothing saved, it's QWERTY.
        let mut fresh = App::new();
        fresh.add_plugins(ControlsPlugin { path: dir.join("nothing_here.txt") });
        assert_eq!(*fresh.world().resource::<Controls>(), Controls::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_file_the_command_line_writes_loads_as_colemak_mod_dh() {
        // Exactly what `fps_prototype --set-preset colemak_mod_dh` leaves on disk.
        let text = "# Controls. `preset` is the base layout; later lines change single keys.\npreset = colemak_mod_dh\n";
        let (controls, problems) = Controls::from_text(text);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(controls, Controls::preset(Preset::ColemakModDh));
        assert_eq!(controls.to_text(), text, "and saving it again changes nothing");
        assert_eq!(controls.bind(Action::Back), Bind::Char('r'));
    }
}
