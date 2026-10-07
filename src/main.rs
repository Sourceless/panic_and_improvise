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
        .add_systems(Startup, (setup_scene, setup_hud))
        .run();
}

fn setup_scene(mut commands: Commands) {
    commands.spawn((
        DirectionalLight {
            illuminance: 10_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.0, 0.5, 0.0)),
    ));

    commands.insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: 300.0,
        ..default()
    });
}

fn setup_hud(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            top: Val::Percent(50.0),
            width: Val::Px(4.0),
            height: Val::Px(4.0),
            margin: UiRect::all(Val::Px(-2.0)),
            ..default()
        },
        BackgroundColor(Color::WHITE),
    ));
}
