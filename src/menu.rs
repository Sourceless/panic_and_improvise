//! The menu that Escape opens: choose a keyboard layout preset, change single keys, and save.
//!
//! What happens in it is plain logic on [`Menu`] and [`Controls`], tested without a screen; the
//! window itself ([`draw_menu`], in egui) only shows that state and turns clicks into calls on it.
//! While it is open the mouse is free and the game's controls are ignored.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::controls::{controls_path, Action, Controls, Keyboard, Preset};

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
pub fn menu_keys(keys: Res<ButtonInput<KeyCode>>, keyboard: Res<Keyboard>, mut menu: ResMut<Menu>, mut controls: ResMut<Controls>) {
    if keys.just_pressed(KeyCode::Escape) {
        menu.escape();
    } else if menu.open {
        if let Some(key) = keyboard.last_press() {
            menu.key_pressed(key, &mut controls);
        }
    }
}

/// The menu window.
pub fn draw_menu(mut contexts: EguiContexts, mut menu: ResMut<Menu>, mut controls: ResMut<Controls>) -> Result {
    if !menu.open {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let mut state = (menu.rebinding, menu.message.clone());
    let (mut choose, mut rebind, mut save, mut reset, mut resume) = (None, None, false, false, false);
    let unsaved = menu.has_unsaved_changes(&controls);
    egui::Window::new("Controls")
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
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
                if ui.button("Resume").clicked() {
                    resume = true;
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
            ui.label("Escape closes this menu.");
        });
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
