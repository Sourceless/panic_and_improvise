// Renders the controls menu to PNG files, in two states, to see how it looks. It opens an ordinary
// window (rendering with no window at all crashes the graphics driver here) but never touches the
// mouse, and closes itself when done.
//
//     cargo run --bin menu_preview -- <output directory>

use std::path::PathBuf;

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::render::view::window::screenshot::{save_to_disk, Screenshot};
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};
use fps_prototype::controls::{Action, Bind, Controls, Preset};
use fps_prototype::menu::{draw_menu, Menu};

#[derive(Resource)]
struct Output(PathBuf);

fn main() {
    let out = std::env::args().nth(1).map(PathBuf::from).expect("usage: menu_preview <output directory>");
    std::fs::create_dir_all(&out).expect("create output directory");
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Menu preview".into(),
                resolution: bevy::window::WindowResolution::new(1280, 720),
                resizable: false,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .insert_resource(Output(out))
        .insert_resource(ClearColor(Color::srgb(0.28, 0.4, 0.2)))
        .insert_resource(Controls::preset(Preset::ColemakModDh))
        .insert_resource(Menu { open: true, saved: Some(Controls::preset(Preset::ColemakModDh)), message: "Saved to ~/.config/fps_prototype/controls.txt.".into(), ..default() })
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Camera2d);
        })
        .add_systems(EguiPrimaryContextPass, draw_menu)
        .add_systems(Update, step)
        .run();
}

// Take a picture, change the menu, take another, and finish.
fn step(mut commands: Commands, mut frame: Local<u32>, out: Res<Output>, mut menu: ResMut<Menu>, mut controls: ResMut<Controls>, mut exit: MessageWriter<AppExit>) {
    *frame += 1;
    match *frame {
        60 => {
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(out.0.join("saved.png")));
        }
        90 => {
            // A key changed, and waiting for another.
            controls.set(Action::Crouch, Bind::Char('x'));
            menu.start_rebinding(Action::Reload);
        }
        150 => {
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(out.0.join("changed.png")));
        }
        240 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}
