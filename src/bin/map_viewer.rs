use std::borrow::Cow;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::window::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured};
use bevy::pbr::{DistanceFog, FogFalloff};
use fps_prototype::map::TerrainMap;
use fps_prototype::settlement::SettlementRoot;
use fps_prototype::terrain::{spawn_terrain, TerrainMaterial, TerrainPlugin, TerrainRoot, TerrainTextures};
use fps_prototype::MAP_SEED;

const ORBIT_TARGET: Vec3 = Vec3::ZERO;
const MIN_DISTANCE: f32 = 300.0;
const MAX_DISTANCE: f32 = 20000.0;
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

#[derive(Resource)]
struct ScreenshotRequest(String);

struct ClipboardHandle(Option<arboard::Clipboard>);

fn main() {
    let seed = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(MAP_SEED);

    let mut app = App::new();
    if let Ok(path) = std::env::var("MAP_VIEWER_SCREENSHOT") {
        app.insert_resource(ScreenshotRequest(path))
            .add_systems(Update, auto_screenshot);
    }
    let distance = std::env::var("MAP_VIEWER_DISTANCE")
        .ok()
        .and_then(|value| value.parse().ok());
    app.add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Map Viewer".into(),
                    ..default()
                }),
                ..default()
            }),
            TerrainPlugin,
        ))
        .insert_resource(Viewer {
            seed,
            pending_regen: false,
            yaw: 0.8,
            pitch: 0.6,
            distance: distance.unwrap_or(4500.0),
        })
        .insert_resource(TerrainMap::generate(seed))
        .insert_non_send(ClipboardHandle(arboard::Clipboard::new().ok()))
        .insert_resource(GlobalAmbientLight {
            color: Color::WHITE,
            brightness: 300.0,
            ..default()
        })
        .add_systems(Startup, (setup_scene, setup_hud))
        .add_systems(
            Update,
            (handle_keys, copy_screenshot_key, orbit_input, regenerate, update_camera, update_hud).chain(),
        )
        .run();
}

fn auto_screenshot(
    mut commands: Commands,
    mut frame: Local<u32>,
    request: Res<ScreenshotRequest>,
    mut exit: MessageWriter<AppExit>,
) {
    *frame += 1;
    if *frame == 180 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(request.0.clone()));
    }
    if *frame == 240 {
        exit.write(AppExit::Success);
    }
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
        DistanceFog {
            color: Color::srgb(0.74, 0.82, 0.88),
            falloff: FogFalloff::Linear {
                start: 6000.0,
                end: 30000.0,
            },
            ..default()
        },
        Transform::default(),
    ));
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

fn copy_screenshot_key(keys: Res<ButtonInput<KeyCode>>, mut commands: Commands) {
    if keys.just_pressed(KeyCode::KeyC) {
        commands
            .spawn(Screenshot::primary_window())
            .observe(copy_screenshot_to_clipboard);
    }
}

fn copy_screenshot_to_clipboard(
    captured: On<ScreenshotCaptured>,
    mut clipboard: NonSendMut<ClipboardHandle>,
) {
    let image = &captured.image;
    let Some(data) = image.data.as_ref() else {
        warn!("screenshot has no pixel data");
        return;
    };
    let mut rgba = data.clone();
    if matches!(
        image.texture_descriptor.format,
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb
    ) {
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
    }
    let Some(board) = clipboard.0.as_mut() else {
        warn!("no clipboard available");
        return;
    };
    let size = image.texture_descriptor.size;
    let result = board.set_image(arboard::ImageData {
        width: size.width as usize,
        height: size.height as usize,
        bytes: Cow::Owned(rgba),
    });
    match result {
        Ok(()) => info!("screenshot copied to clipboard"),
        Err(err) => warn!("could not copy screenshot: {err}"),
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
    old_terrain: Query<Entity, Or<(With<TerrainRoot>, With<SettlementRoot>)>>,
    textures: Res<TerrainTextures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut terrain: ResMut<Assets<TerrainMaterial>>,
) {
    if !viewer.pending_regen {
        return;
    }
    viewer.pending_regen = false;
    for entity in &old_terrain {
        commands.entity(entity).despawn();
    }
    let map = TerrainMap::generate(viewer.seed);
    spawn_terrain(&mut commands, &mut meshes, &mut standard, &mut terrain, &textures, &map);
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
    let (low, high) = map.height_range();
    text.0 = format!(
        "Seed {}\nRelief: {:.0} to {:.0} m   River: {:.1} km   POIs: {}\n\n\
         N new seed   [ / ] previous / next seed\n\
         Drag: orbit   Wheel: zoom   C: copy screenshot",
        viewer.seed,
        low,
        high,
        map.river_length() / 1000.0,
        map.pois.len(),
    );
}
