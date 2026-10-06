use std::time::{SystemTime, UNIX_EPOCH};

use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use fps_prototype::map::TerrainMap;
use fps_prototype::terrain::{spawn_terrain, TerrainRoot};
use fps_prototype::MAP_SEED;

const ORBIT_TARGET: Vec3 = Vec3::ZERO;
const MIN_DISTANCE: f32 = 300.0;
const MAX_DISTANCE: f32 = 12000.0;
const ORBIT_SENSITIVITY: f32 = 0.005;
const ZOOM_STEP: f32 = 0.12;

#[derive(Resource)]
struct Viewer {
    seed: u64,
    pending_regen: bool,
    yaw: f32,
    pitch: f32,
    distance: f32,
}

#[derive(Component)]
struct Hud;

fn main() {
    let seed = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(MAP_SEED);

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Map Viewer".into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(Viewer {
            seed,
            pending_regen: false,
            yaw: 0.8,
            pitch: 0.6,
            distance: 4500.0,
        })
        .insert_resource(TerrainMap::generate(seed))
        .insert_resource(GlobalAmbientLight {
            color: Color::WHITE,
            brightness: 300.0,
            ..default()
        })
        .add_systems(Startup, (setup_scene, setup_hud, build_terrain).chain())
        .add_systems(
            Update,
            (handle_keys, orbit_input, regenerate, update_camera, update_hud).chain(),
        )
        .run();
}

fn setup_scene(mut commands: Commands) {
    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.0, 0.6, 0.0)),
    ));
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            far: 30_000.0,
            ..default()
        }),
        Transform::default(),
    ));
}

fn build_terrain(
    mut commands: Commands,
    map: Res<TerrainMap>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    spawn_terrain(&mut commands, &mut meshes, &mut materials, &map);
}

fn setup_hud(mut commands: Commands) {
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: bevy::text::FontSize::Px(18.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(12.0),
            top: Val::Px(12.0),
            ..default()
        },
        Hud,
    ));
}

fn handle_keys(keys: Res<ButtonInput<KeyCode>>, mut viewer: ResMut<Viewer>) {
    if keys.just_pressed(KeyCode::KeyN) {
        viewer.seed = new_random_seed();
        viewer.pending_regen = true;
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        viewer.seed = viewer.seed.wrapping_add(1);
        viewer.pending_regen = true;
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        viewer.seed = viewer.seed.wrapping_sub(1);
        viewer.pending_regen = true;
    }
}

fn new_random_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(MAP_SEED)
}

fn regenerate(
    mut commands: Commands,
    mut viewer: ResMut<Viewer>,
    old_terrain: Query<Entity, With<TerrainRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !viewer.pending_regen {
        return;
    }
    viewer.pending_regen = false;
    for entity in &old_terrain {
        commands.entity(entity).despawn();
    }
    let map = TerrainMap::generate(viewer.seed);
    spawn_terrain(&mut commands, &mut meshes, &mut materials, &map);
    commands.insert_resource(map);
}

fn orbit_input(
    buttons: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    mut viewer: ResMut<Viewer>,
) {
    if buttons.pressed(MouseButton::Left) {
        for event in motion.read() {
            viewer.yaw -= event.delta.x * ORBIT_SENSITIVITY;
            viewer.pitch = (viewer.pitch + event.delta.y * ORBIT_SENSITIVITY).clamp(0.05, 1.5);
        }
    } else {
        motion.clear();
    }
    for event in wheel.read() {
        let factor = (1.0 - event.y * ZOOM_STEP).max(0.1);
        viewer.distance = (viewer.distance * factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }
}

fn update_camera(viewer: Res<Viewer>, mut cameras: Query<&mut Transform, With<Camera3d>>) {
    let Ok(mut transform) = cameras.single_mut() else {
        return;
    };
    let offset = Vec3::new(
        viewer.pitch.cos() * viewer.yaw.sin(),
        viewer.pitch.sin(),
        viewer.pitch.cos() * viewer.yaw.cos(),
    ) * viewer.distance;
    *transform = Transform::from_translation(ORBIT_TARGET + offset).looking_at(ORBIT_TARGET, Vec3::Y);
}

fn update_hud(viewer: Res<Viewer>, map: Res<TerrainMap>, mut hud: Query<&mut Text, With<Hud>>) {
    let Ok(mut text) = hud.single_mut() else {
        return;
    };
    let tallest = map.hills.iter().map(|h| h.height).fold(0.0, f32::max);
    text.0 = format!(
        "Seed {}\nHills: {} (tallest {:.0} m)   River: {:.1} km   POIs: {}\n\n\
         N new seed   [ / ] previous / next seed\n\
         Drag: orbit   Wheel: zoom",
        viewer.seed,
        map.hills.len(),
        tallest,
        map.river_length() / 1000.0,
        map.pois.len(),
    );
}
