use bevy::prelude::*;
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};
use fps_prototype::controls::{controls_path, Controls, Preset};
use fps_prototype::terrain::TerrainPlugin;
use fps_prototype::GamePlugin;

fn main() {
    if let Some(code) = command_line() {
        std::process::exit(code);
    }
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "FPS Prototype".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((GamePlugin, TerrainPlugin, EguiPlugin::default()))
        .add_plugins(fps_prototype::crosshair::CrosshairPlugin)
        // The menu Escape opens.
        .add_systems(EguiPrimaryContextPass, fps_prototype::menu::draw_menu)
        .run();
}

/// The flags that do something and exit, instead of starting the game. Returns the exit code if
/// there was one.
fn command_line() -> Option<i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--set-preset") => {
            let Some(preset) = args.get(1).and_then(|id| Preset::from_id(id)) else {
                let names: Vec<&str> = Preset::ALL.iter().map(|p| p.id()).collect();
                eprintln!("usage: fps_prototype --set-preset <{}>", names.join(" | "));
                return Some(2);
            };
            let path = controls_path();
            match Controls::preset(preset).save(&path) {
                Ok(()) => {
                    println!("Saved {} as your controls: {}", preset.label(), path.display());
                    Some(0)
                }
                Err(error) => {
                    eprintln!("could not save {}: {error}", path.display());
                    Some(1)
                }
            }
        }
        Some("--help") | Some("-h") => {
            println!("fps_prototype                        play");
            println!("fps_prototype --set-preset <name>    save a keyboard layout (qwerty, colemak_mod_dh) as your controls, and quit");
            println!("In the game, Escape opens the menu, where the layout can be chosen, keys changed and saved.");
            println!("Saved controls live in {}", controls_path().display());
            Some(0)
        }
        _ => None,
    }
}
