// A release build on Windows is a windowed program: no console opens behind the game. (Its `--help` has nowhere to
// print, then; the bot is a console program and does.)
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use bevy::prelude::*;
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};
use fps_prototype::controls::{controls_path, Controls, Preset};
use fps_prototype::playback::PlaybackPlugin;
use fps_prototype::recording::{recordings_dir, Recording, EXTENSION};
use fps_prototype::terrain::TerrainPlugin;
use fps_prototype::GamePlugin;

/// A recording to play instead of playing, and whether to keep the player exactly to what it recorded.
struct Replay {
    recording: Recording,
    strict: bool,
}

fn main() {
    let replay = match command_line() {
        Ok(replay) => replay,
        Err(code) => std::process::exit(code),
    };
    let mut app = App::new();
    app
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
        .add_systems(EguiPrimaryContextPass, (fps_prototype::menu::draw_menu, fps_prototype::inventory_ui::draw_inventory, fps_prototype::hud::draw_hud, fps_prototype::playback::draw_playback));
    // Last, to have the last word on what the game starts with.
    if let Some(replay) = replay {
        app.add_plugins(PlaybackPlugin { recording: replay.recording, strict: replay.strict });
    }
    app.run();
}

/// The flags that do something and exit, instead of starting the game, return the exit code. A recording to play is
/// returned for the game to be started with.
fn command_line() -> Result<Option<Replay>, i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut replay_file = None;
    let mut strict = false;
    let mut record = None;
    let mut rest = args.iter().peekable();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--set-preset" => {
                let Some(preset) = rest.next().and_then(|id| Preset::from_id(id)) else {
                    let names: Vec<&str> = Preset::ALL.iter().map(|p| p.id()).collect();
                    eprintln!("usage: fps_prototype --set-preset <{}>", names.join(" | "));
                    return Err(2);
                };
                let path = controls_path();
                return match Controls::preset(preset).save(&path) {
                    Ok(()) => {
                        println!("Saved {} as your controls: {}", preset.label(), path.display());
                        Err(0)
                    }
                    Err(error) => {
                        eprintln!("could not save {}: {error}", path.display());
                        Err(1)
                    }
                };
            }
            "--range" => {
                // The shooting range is switched on by the environment, so the game proper can see it.
                // Safe here: nothing else is running yet.
                unsafe { std::env::set_var("FPS_RANGE", "1") };
            }
            "--host" => {
                // Hosting is switched on by the environment too, for the plugin to see.
                let address = rest.next_if(|a| !a.starts_with("--") && !a.ends_with(&format!(".{EXTENSION}"))).cloned().unwrap_or_else(|| "0.0.0.0".to_string());
                unsafe { std::env::set_var("FPS_HOST", address) };
            }
            "--record" => {
                // Where to put it is optional.
                record = Some(rest.next_if(|a| !a.starts_with("--")).cloned().unwrap_or_default());
            }
            "--replay" => match rest.next() {
                Some(file) => replay_file = Some(file.clone()),
                None => {
                    eprintln!("usage: fps_prototype --replay <recording.{EXTENSION}>");
                    return Err(2);
                }
            },
            "--strict" => strict = true,
            "--help" | "-h" => {
                println!("fps_prototype                        play");
                println!("fps_prototype <recording.{EXTENSION}>        play a recording again (also --replay <file>); --strict never puts the player back on course");
                println!("fps_prototype --record [file]         play, recording from the start (the menu can also start and stop one)");
                println!("fps_prototype --range                  play on a shooting range: every gun and load lying out, dummies to 300 m");
                println!("fps_prototype --host [address[:port]]  play, and let others join over UDP (default 0.0.0.0:7777); `cargo run --bin bot` joins one");
                println!("fps_prototype --set-preset <name>    save a keyboard layout (qwerty, colemak_mod_dh) as your controls, and quit");
                println!("In the game, Escape opens the menu: resume, record, choose the layout and change keys, exit. Tab is the inventory.");
                println!("Saved controls live in {}", controls_path().display());
                println!("Recordings are kept in {}", recordings_dir().display());
                return Err(0);
            }
            // A recording given as it is, as when it is opened from a folder.
            file if !file.starts_with('-') && replay_file.is_none() => replay_file = Some(file.to_string()),
            other => {
                eprintln!("unknown argument `{other}` (try --help)");
                return Err(2);
            }
        }
    }
    if let Some(file) = replay_file {
        let recording = Recording::read(std::path::Path::new(&file)).map_err(|error| {
            eprintln!("could not play it: {error}");
            1
        })?;
        // The map and what the game starts with are the recording's.
        unsafe { std::env::set_var("FPS_SEED", recording.header.seed.to_string()) };
        return Ok(Some(Replay { recording, strict }));
    }
    if let Some(file) = record {
        unsafe { std::env::set_var("FPS_RECORD", if file.is_empty() { "1".to_string() } else { file }) };
    }
    Ok(None)
}
