use bevy::prelude::*;
use fps_prototype::target::TargetDummy;
use fps_prototype::terrain::TerrainPlugin;
use fps_prototype::map::TerrainMap;
use fps_prototype::GamePlugin;

#[derive(Component)]
struct HudText;

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
        .add_systems(Update, update_hud)
        .run();
}

fn setup_scene(
    mut commands: Commands,
    map: Res<TerrainMap>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let box_mesh = meshes.add(Cuboid::new(2.0, 2.0, 2.0));
    let box_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.6, 0.65),
        ..default()
    });
    let positions = [
        Vec3::new(5.0, 1.0, -5.0),
        Vec3::new(-8.0, 1.0, 4.0),
        Vec3::new(10.0, 1.0, 10.0),
        Vec3::new(-4.0, 1.0, -12.0),
        Vec3::new(0.0, 1.0, -20.0),
    ];
    for pos in positions {
        let ground = map.height_at(Vec2::new(pos.x, pos.z));
        commands.spawn((
            Mesh3d(box_mesh.clone()),
            MeshMaterial3d(box_material.clone()),
            Transform::from_xyz(pos.x, ground + pos.y, pos.z),
        ));
    }

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
        Text::new(""),
        TextFont {
            font_size: bevy::text::FontSize::Px(20.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(12.0),
            top: Val::Px(12.0),
            ..default()
        },
        HudText,
    ));
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

fn update_hud(dummies: Query<&TargetDummy>, mut texts: Query<&mut Text, With<HudText>>) {
    let Ok(mut text) = texts.single_mut() else {
        return;
    };
    let Ok(dummy) = dummies.single() else {
        return;
    };
    text.0 = if dummy.health <= 0.0 {
        format!("Target down! Hits: {}", dummy.hits)
    } else {
        format!("Target HP: {:.0}   Hits: {}", dummy.health, dummy.hits)
    };
}
