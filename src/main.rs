// A release build on Windows is a windowed program: no console opens behind the game. (Its `--help` has nowhere to
// print, then; the bot is a console program and does.)
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

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
                title: fps_prototype::window_title(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((GamePlugin, TerrainPlugin, EguiPlugin::default()))
        .add_plugins((fps_prototype::crosshair::CrosshairPlugin, fps_prototype::health::HealthPlugin, fps_prototype::zombie::ZombiePlugin, fps_prototype::net_host::HostPlugin::from_env()))
        // The menu Escape opens.
        .add_systems(EguiPrimaryContextPass, (fps_prototype::menu::draw_menu, fps_prototype::inventory_ui::draw_inventory, fps_prototype::hud::draw_hud))
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
        Some("--range") => {
            // The shooting range is switched on by the environment, so the game proper can see it.
            // Safe here: nothing else is running yet.
            unsafe { std::env::set_var("FPS_RANGE", "1") };
            None
        }
        Some("--host") => {
            // Hosting is switched on by the environment too, for the plugin to see.
            let address = args.get(1).cloned().unwrap_or_else(|| "0.0.0.0".to_string());
            unsafe { std::env::set_var("FPS_HOST", address) };
            None
        }
        Some("--help") | Some("-h") => {
            println!("fps_prototype                        play");
            println!("fps_prototype --range                  play on a shooting range: every gun and load lying out, dummies to 300 m");
            println!("fps_prototype --host [address[:port]]  play, and let others join over UDP (default 0.0.0.0:7777); `cargo run --bin bot` joins one");
            println!("fps_prototype --set-preset <name>    save a keyboard layout (qwerty, colemak_mod_dh) as your controls, and quit");
            println!("In the game, Escape opens the menu, where the layout can be chosen, keys changed and saved.");
            println!("Saved controls live in {}", controls_path().display());
            Some(0)
        }
        _ => None,
    }
}
