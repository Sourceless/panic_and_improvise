//! The menu that Escape opens: resume, start or stop a recording, choose a keyboard layout preset, change single
//! keys and save them, and leave the game.
//!
//! What happens in it is plain logic on [`Menu`] and [`Controls`], tested without a screen; the
//! window itself ([`draw_menu`], in egui) only shows that state and turns clicks into calls on it.
//! While it is open the mouse is free and the game's controls are ignored.

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::controls::{controls_path, Action, Controls, Keyboard, Preset};
use crate::playback::Playback;
use crate::recorder::{clock, Recorder};

#[derive(Resource, Default, Debug)]
pub struct Menu {
    pub open: bool,
    /// The action that is waiting for a key to be pressed to go on.
    pub rebinding: Option<Action>,
    /// What the controls were last saved as, to tell whether there are changes that aren't.
    pub saved: Option<Controls>,
    /// A line at the bottom of the menu: what was just saved, or what went wrong.
    pub message: String,
}

impl Menu {
    pub fn open(&mut self) {
        self.open = true;
        self.rebinding = None;
        self.message.clear();
        // What is on disk, so unsaved changes can be told from saved ones.
        self.saved = Some(Controls::load(&controls_path()));
    }

    pub fn close(&mut self) {
        self.open = false;
        self.rebinding = None;
    }

    /// Escape: closes the menu, or opens it, or if a key is being waited for, gives up waiting.
    pub fn escape(&mut self) {
        if self.rebinding.is_some() {
            self.rebinding = None;
            self.message = "Cancelled.".to_string();
        } else if self.open {
            self.close();
        } else {
            self.open();
        }
    }

    pub fn start_rebinding(&mut self, action: Action) {
        self.rebinding = Some(action);
        self.message = format!("Press the key for \"{}\" (Escape to cancel).", action.label());
    }

    /// A key has been pressed: if one is being waited for, that's it.
    pub fn key_pressed(&mut self, key: crate::controls::Bind, controls: &mut Controls) {
        if let Some(action) = self.rebinding.take() {
            controls.set(action, key);
            self.message = format!("{} is now {}.", action.label(), key.label());
        }
    }

    pub fn choose_preset(&mut self, preset: Preset, controls: &mut Controls) {
        controls.use_preset(preset);
        self.rebinding = None;
        self.message = format!("Using {}.", preset.label());
    }

    /// Writes the controls where they'll be loaded from next time.
    pub fn save(&mut self, controls: &Controls, path: &std::path::Path) {
        match controls.save(path) {
            Ok(()) => {
                self.saved = Some(controls.clone());
                self.message = format!("Saved to {}.", path.display());
            }
            Err(error) => self.message = format!("Could not save: {error}"),
        }
    }

    /// Whether the controls differ from what was last saved.
    pub fn has_unsaved_changes(&self, controls: &Controls) -> bool {
        self.saved.as_ref() != Some(controls)
    }
}

/// Escape toggles the menu, and while it's open a key press goes to whatever is being rebound.
pub fn menu_keys(
    keys: Res<ButtonInput<KeyCode>>,
    keyboard: Res<Keyboard>,
    mut menu: ResMut<Menu>,
    mut controls: ResMut<Controls>,
    mut screen: ResMut<crate::inventory_ui::InventoryScreen>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        // Escape shuts the inventory first.
        if screen.open && !menu.open {
            screen.toggle();
        } else {
            menu.escape();
        }
    } else if menu.open {
        if let Some(key) = keyboard.last_press() {
            menu.key_pressed(key, &mut controls);
        }
    }
}

/// The menu window.
pub fn draw_menu(
    mut contexts: EguiContexts,
    mut menu: ResMut<Menu>,
    mut controls: ResMut<Controls>,
    mut recorder: ResMut<Recorder>,
    replay: Option<Res<Playback>>,
    mut exit: MessageWriter<AppExit>,
) -> Result {
    // A replay has its own menu.
    if !menu.open || replay.is_some_and(|r| r.driving()) {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let mut state = (menu.rebinding, menu.message.clone());
    let (mut choose, mut rebind, mut save, mut reset, mut resume, mut quit, mut to_title) = (None, None, false, false, false, false, false);
    let unsaved = menu.has_unsaved_changes(&controls);
    egui::Window::new(format!("Panic and Improvise v{}", crate::VERSION))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            if ui.add_sized([ui.available_width(), 28.0], egui::Button::new("Resume")).clicked() {
                resume = true;
            }
            ui.separator();
            recording_section(ui, &mut recorder);
            ui.separator();
            egui::CollapsingHeader::new("Controls").default_open(state.0.is_some()).show(ui, |ui| {
            ui.label("Keyboard layout");
            ui.horizontal(|ui| {
                for preset in Preset::ALL {
                    let current = controls.base() == preset;
                    if ui.selectable_label(current, preset.label()).clicked() {
                        choose = Some(preset);
                    }
                }
            });
            ui.label("Keys are named by the letter they type, so this works however your layout is set.");
            ui.separator();
            egui::Grid::new("bindings").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                for action in Action::ALL {
                    ui.label(action.label());
                    let text = if state.0 == Some(action) { "press a key…".to_string() } else { controls.bind(action).label() };
                    if ui.add_sized([110.0, 22.0], egui::Button::new(text)).clicked() {
                        rebind = Some(action);
                    }
                    ui.end_row();
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Save as my controls").clicked() {
                    save = true;
                }
                if ui.add_enabled(controls.is_customised(), egui::Button::new("Reset keys to preset")).clicked() {
                    reset = true;
                }
            });
            if unsaved {
                ui.colored_label(egui::Color32::from_rgb(230, 180, 60), "Changes not saved: they apply now, but won't be here next time.");
            } else {
                ui.label("Saved.");
            }
            if !state.1.is_empty() {
                ui.label(&state.1);
            }
            });
            ui.separator();
            if ui.add_sized([ui.available_width(), 28.0], egui::Button::new("Main menu")).on_hover_text("Leaves this game and starts the program again at the title").clicked() {
                to_title = true;
            }
            if ui.add_sized([ui.available_width(), 28.0], egui::Button::new("Exit game")).clicked() {
                quit = true;
            }
            ui.label("Escape closes this menu.");
        });
    if to_title {
        // The title is where the program starts, so it is started again; this one leaves as it would for Exit
        // (a game that was joined says goodbye, a recording is closed).
        match std::env::current_exe().and_then(|exe| std::process::Command::new(exe).spawn()) {
            Ok(_) => quit = true,
            Err(error) => menu.message = format!("Could not go back to the title: {error}"),
        }
    }
    if quit {
        // A recording being made is closed properly as the game leaves.
        exit.write(AppExit::Success);
    }
    if let Some(preset) = choose {
        menu.choose_preset(preset, &mut controls);
    }
    if let Some(action) = rebind {
        menu.start_rebinding(action);
    }
    if reset {
        let base = controls.base();
        menu.choose_preset(base, &mut controls);
    }
    if save {
        let path = controls_path();
        menu.save(&controls, &path);
    }
    if resume {
        menu.close();
    }
    state.0 = menu.rebinding;
    Ok(())
}

/// Start or stop recording, and what happened the last time.
fn recording_section(ui: &mut egui::Ui, recorder: &mut Recorder) {
    if recorder.is_recording() {
        ui.horizontal(|ui| {
            crate::hud::recording_dot(ui);
            ui.colored_label(egui::Color32::from_rgb(230, 70, 60), format!("Recording  {}", clock(recorder.seconds())));
        });
        if let Some(path) = recorder.path() {
            ui.label(egui::RichText::new(path.display().to_string()).small());
        }
        if ui.button("Stop recording").clicked() {
            recorder.stop();
        }
    } else if recorder.wanted() {
        ui.label("Starting to record…");
    } else {
        if ui.button("Start recording").clicked() {
            recorder.start();
        }
        ui.label(egui::RichText::new(format!("Kept in {} as a .pir file, to play again by opening it with the game.", crate::recording::recordings_dir().display())).small());
    }
    if !recorder.message.is_empty() && !recorder.is_recording() {
        ui.label(&recorder.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::Bind;

    #[test]
    fn escape_opens_and_closes_the_menu() {
        let mut menu = Menu::default();
        assert!(!menu.open);
        menu.escape();
        assert!(menu.open);
        menu.escape();
        assert!(!menu.open);
    }

    #[test]
    fn escape_while_waiting_for_a_key_only_cancels_that() {
        let mut menu = Menu::default();
        menu.escape();
        menu.start_rebinding(Action::Reload);
        menu.escape();
        assert!(menu.open, "the menu stays open");
        assert_eq!(menu.rebinding, None);
        assert!(menu.message.contains("Cancelled"));
    }

    #[test]
    fn the_next_key_pressed_goes_to_the_action_being_rebound() {
        let mut menu = Menu::default();
        let mut controls = Controls::default();
        menu.key_pressed(Bind::Char('g'), &mut controls);
        assert_eq!(controls, Controls::default(), "nothing is waiting, so nothing changes");
        menu.start_rebinding(Action::Reload);
        menu.key_pressed(Bind::Char('g'), &mut controls);
        assert_eq!(controls.bind(Action::Reload), Bind::Char('g'));
        assert_eq!(menu.rebinding, None, "and it is done");
        assert!(menu.message.contains("G"));
    }

    #[test]
    fn choosing_a_preset_replaces_the_bindings_and_stops_any_rebinding() {
        let mut menu = Menu::default();
        let mut controls = Controls::default();
        controls.set(Action::Jump, Bind::Char('x'));
        menu.start_rebinding(Action::Reload);
        menu.choose_preset(Preset::ColemakModDh, &mut controls);
        assert_eq!(controls, Controls::preset(Preset::ColemakModDh));
        assert_eq!(menu.rebinding, None);
    }

    #[test]
    fn saving_records_what_was_saved_and_says_where() {
        let dir = std::env::temp_dir().join(format!("fps_menu_save_{}", std::process::id()));
        let path = dir.join("controls.txt");
        let mut menu = Menu::default();
        let mut controls = Controls::default();
        assert!(menu.has_unsaved_changes(&controls), "nothing has been saved yet");
        menu.choose_preset(Preset::ColemakModDh, &mut controls);
        menu.save(&controls, &path);
        assert!(menu.message.contains("Saved"), "{}", menu.message);
        assert!(!menu.has_unsaved_changes(&controls));
        assert_eq!(Controls::load(&path), controls, "it really is on disk");
        controls.set(Action::Reload, Bind::Char('g'));
        assert!(menu.has_unsaved_changes(&controls), "a change since is unsaved");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_save_that_fails_says_so_and_keeps_the_old_state() {
        // A file where a folder is needed.
        let blocker = std::env::temp_dir().join(format!("fps_menu_blocker_{}", std::process::id()));
        std::fs::write(&blocker, "x").unwrap();
        let mut menu = Menu::default();
        let controls = Controls::default();
        menu.save(&controls, &blocker.join("sub").join("controls.txt"));
        assert!(menu.message.starts_with("Could not save"), "{}", menu.message);
        assert!(menu.saved.is_none());
        let _ = std::fs::remove_file(&blocker);
    }
}
