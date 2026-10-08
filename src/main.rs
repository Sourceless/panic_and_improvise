use bevy::prelude::*;
use fps_prototype::terrain::TerrainPlugin;
use fps_prototype::GamePlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "FPS Prototype".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((GamePlugin, TerrainPlugin))
        .add_plugins(fps_prototype::crosshair::CrosshairPlugin)
        .run();
}
