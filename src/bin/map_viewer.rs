use std::borrow::Cow;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::window::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::pbr::{MeshMaterial3d};
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use fps_prototype::map::TerrainMap;
use fps_prototype::params::GenParams;
use fps_prototype::fill::spawn_fill;
use fps_prototype::roads::{road_mesh, RoadKind, RoadNetwork};
use fps_prototype::zones::{overlay_mesh, ZoneMap};
use fps_prototype::settlement::SettlementRoot;
use fps_prototype::terrain::{spawn_terrain, TerrainMaterial, TerrainPlugin, TerrainRoot, TerrainTextures};
use fps_prototype::MAP_SEED;

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
    zones_on: bool,
    params_open: bool,
    target: Vec2,
}

#[derive(Component)]
struct Hud;

#[derive(Component)]
struct ZoneOverlay;

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
            EguiPlugin::default(),
        ))
        .insert_resource(Viewer {
            seed,
            pending_regen: false,
            yaw: 0.8,
            pitch: 0.6,
            distance: distance.unwrap_or(4500.0),
            zones_on: std::env::var("MAP_VIEWER_ZONES").is_ok(),
            params_open: false,
            target: std::env::var("MAP_VIEWER_TARGET")
                .ok()
                .and_then(|v| {
                    let mut parts = v.split(',').filter_map(|n| n.trim().parse::<f32>().ok());
                    Some(Vec2::new(parts.next()?, parts.next()?))
                })
                .unwrap_or(Vec2::ZERO),
        })
        .insert_resource(TerrainMap::generate(seed, &GenParams::default()))
        .insert_resource(GenParams::default())
        .insert_non_send(ClipboardHandle(arboard::Clipboard::new().ok()))
        .insert_resource(GlobalAmbientLight {
            color: Color::WHITE,
            brightness: 300.0,
            ..default()
        })
        .add_systems(Startup, (setup_scene, setup_hud, build_zones, build_roads, build_fill).chain())
        .add_systems(
            Update,
            (handle_keys, copy_screenshot_key, orbit_input, regenerate, zone_overlay, update_camera, update_hud).chain(),
        )
        .add_systems(EguiPrimaryContextPass, ui_panel)
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
    if keys.just_pressed(KeyCode::KeyZ) {
        viewer.zones_on = !viewer.zones_on;
    }
    if keys.just_pressed(KeyCode::KeyP) {
        viewer.params_open = !viewer.params_open;
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

fn ui_panel(mut contexts: EguiContexts, mut params: ResMut<GenParams>, mut viewer: ResMut<Viewer>) -> Result {
    let ctx = contexts.ctx_mut()?;

    egui::Area::new("params_toggle".into())
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 8.0))
        .show(ctx, |ui| {
            let label = if viewer.params_open { "Hide parameters (P)" } else { "Show parameters (P)" };
            if ui.button(label).clicked() {
                viewer.params_open = !viewer.params_open;
            }
        });

    if !viewer.params_open {
        return Ok(());
    }

    let mut viewport_ui = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    egui::Panel::left("params_panel").default_size(260.0).show(&mut viewport_ui, |ui| {
        ui.heading("Generation parameters");
        ui.add(egui::Slider::new(&mut params.relief_scale, 0.2..=2.5).text("Relief scale"));
        ui.add(egui::Slider::new(&mut params.erosion_droplets, 0..=800_000).text("Erosion droplets"));
        ui.add(egui::Slider::new(&mut params.river_slope_weight, 1.0..=15.0).text("River slope weight"));
        ui.separator();
        ui.add(egui::Slider::new(&mut params.field_spacing, 40.0..=200.0).text("Field size"));
        ui.add(egui::Slider::new(&mut params.field_contour_weight, 0.5..=15.0).text("Field contour weight"));
        ui.add(egui::Slider::new(&mut params.field_max_slope, 0.1..=2.0).text("Field max slope change"));
        ui.add(egui::Slider::new(&mut params.max_arable_slope, 0.02..=0.5).text("Max arable slope"));
        ui.add(egui::Slider::new(&mut params.max_pasture_slope, 0.1..=1.0).text("Max pasture slope"));
        ui.separator();
        ui.add(egui::Slider::new(&mut params.road_slope_scale, 0.2..=3.0).text("Road slope cost"));
        ui.add(egui::Slider::new(&mut params.road_water_scale, 0.2..=3.0).text("Road water cost"));
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Regenerate").clicked() {
                viewer.pending_regen = true;
            }
            if ui.button("Reset to defaults").clicked() {
                *params = GenParams::default();
            }
        });
    });
    Ok(())
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
    params: Res<GenParams>,
    old_terrain: Query<Entity, Or<(With<TerrainRoot>, With<SettlementRoot>, With<ZoneOverlay>)>>,
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
    let map = TerrainMap::generate(viewer.seed, &params);
    let zones = ZoneMap::generate(&map, &params);
    let roads = RoadNetwork::generate(&map, &params);
    spawn_terrain(&mut commands, &mut meshes, &mut standard, &mut terrain, &textures, &map);
    spawn_roads(&mut commands, &mut meshes, &mut standard, &map, &roads);
    spawn_fill(&mut commands, &mut meshes, &mut standard, &map, &zones, &roads, &params);
    commands.insert_resource(zones);
    commands.insert_resource(roads);
    commands.insert_resource(map);
}

fn build_zones(mut commands: Commands, map: Res<TerrainMap>, params: Res<GenParams>) {
    commands.insert_resource(ZoneMap::generate(&map, &params));
}

fn build_fill(
    mut commands: Commands,
    map: Res<TerrainMap>,
    zones: Res<ZoneMap>,
    roads: Res<RoadNetwork>,
    params: Res<GenParams>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
) {
    spawn_fill(&mut commands, &mut meshes, &mut standard, &map, &zones, &roads, &params);
}

fn build_roads(
    mut commands: Commands,
    map: Res<TerrainMap>,
    params: Res<GenParams>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
) {
    let roads = RoadNetwork::generate(&map, &params);
    spawn_roads(&mut commands, &mut meshes, &mut standard, &map, &roads);
    commands.insert_resource(roads);
}

fn spawn_roads(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    standard: &mut Assets<StandardMaterial>,
    map: &TerrainMap,
    roads: &RoadNetwork,
) {
    let major = standard.add(StandardMaterial {
        base_color: Color::srgb(0.30, 0.29, 0.28),
        perceptual_roughness: 0.95,
        ..default()
    });
    let minor = standard.add(StandardMaterial {
        base_color: Color::srgb(0.52, 0.42, 0.30),
        perceptual_roughness: 1.0,
        ..default()
    });
    for (kind, material) in [(RoadKind::Major, major), (RoadKind::Minor, minor)] {
        commands.spawn((
            TerrainRoot,
            Mesh3d(meshes.add(road_mesh(map, roads, kind))),
            MeshMaterial3d(material),
        ));
    }
}

fn zone_overlay(
    mut commands: Commands,
    viewer: Res<Viewer>,
    map: Res<TerrainMap>,
    zones: Res<ZoneMap>,
    existing: Query<Entity, With<ZoneOverlay>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !viewer.zones_on {
        for entity in &existing {
            commands.entity(entity).despawn();
        }
        return;
    }
    if existing.is_empty() {
        commands.spawn((
            ZoneOverlay,
            Mesh3d(meshes.add(overlay_mesh(&map, &zones))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::WHITE,
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                ..default()
            })),
        ));
    }
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
    let orbit_target = Vec3::new(viewer.target.x, 0.0, viewer.target.y);
    let Ok(mut transform) = cameras.single_mut() else {
        return;
    };
    let offset = Vec3::new(
        viewer.pitch.cos() * viewer.yaw.sin(),
        viewer.pitch.sin(),
        viewer.pitch.cos() * viewer.yaw.cos(),
    ) * viewer.distance;
    *transform = Transform::from_translation(orbit_target + offset).looking_at(orbit_target, Vec3::Y);
}

fn update_hud(
    viewer: Res<Viewer>,
    map: Res<TerrainMap>,
    roads: Res<RoadNetwork>,
    mut hud: Query<&mut Text, With<Hud>>,
) {
    let Ok(mut text) = hud.single_mut() else {
        return;
    };
    let (low, high) = map.height_range();
    text.0 = format!(
        "Seed {}\nRelief: {:.0} to {:.0} m   River: {:.1} km   POIs: {}\n\
         Roads: {:.1} km major, {:.1} km minor\n\n\
         N new seed   [ / ] previous / next seed\n\
         Drag: orbit   Wheel: zoom   C: copy screenshot   Z: zones   P: parameters",
        viewer.seed,
        low,
        high,
        map.river_length() / 1000.0,
        map.pois.len(),
        roads.length(RoadKind::Major) / 1000.0,
        roads.length(RoadKind::Minor) / 1000.0,
    );
}
