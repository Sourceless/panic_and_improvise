// Renders a made-up stretch of road to PNG files, to see the road shader's work: the tarmac, the
// worn wheel paths, the white lines, a farm track joining, and a T-junction. It opens an ordinary
// window (rendering with no window at all crashes the graphics driver here) but never touches the
// mouse, and closes itself when done.
//
//     cargo run --bin road_preview -- <output directory> [view names, comma separated]
//
// (The graphics driver here sometimes drops out part way through; the second argument
// renders only the named views, to carry on from where it stopped.)

use std::path::PathBuf;

use bevy::app::AppExit;
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;
use bevy::render::view::window::screenshot::{save_to_disk, Screenshot};
use fps_prototype::map::TerrainMap;
use fps_prototype::mipmaps::{MipQueue, MipmapPlugin};
use fps_prototype::road_material::{RoadExtension, RoadMaterial};
use fps_prototype::roads::{half_width, road_mesh, RoadKind, RoadRibbon};

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

#[derive(Resource)]
struct Output(PathBuf, Vec<String>);

#[derive(Component)]
struct Eye;

struct View {
    name: &'static str,
    camera: Transform,
    fov: f32,
}

fn views() -> Vec<View> {
    let look = |name, from: Vec3, at: Vec3, fov| View { name, camera: Transform::from_translation(from).looking_at(at, Vec3::Y), fov };
    vec![
        // What a driver (or a walker) sees.
        look("driver", Vec3::new(2.0, 1.6, 40.0), Vec3::new(0.0, 0.8, -60.0), 1.05),
        look("walker_far", Vec3::new(0.0, 1.8, 20.0), Vec3::new(0.0, 1.0, -250.0), 0.9),
        look("oblique", Vec3::new(28.0, 22.0, 50.0), Vec3::new(0.0, 0.0, -70.0), 0.9),
        look("junction", Vec3::new(30.0, 9.0, -30.0), Vec3::new(0.0, 0.0, -75.0), 0.9),
        look("closeup", Vec3::new(1.2, 1.3, 14.0), Vec3::new(-0.5, 0.0, 0.0), 0.7),
        look("edge", Vec3::new(5.5, 1.0, 12.0), Vec3::new(3.6, 0.0, 0.0), 0.6),
        look("track", Vec3::new(26.0, 3.0, -42.0), Vec3::new(40.0, 0.0, -62.0), 0.8),
        View {
            name: "top",
            camera: Transform::from_translation(Vec3::new(0.0, 90.0, -70.0)).looking_at(Vec3::new(0.0, 0.0, -70.0), Vec3::NEG_Z),
            fov: 0.9,
        },
    ]
}

fn main() {
    let out = std::env::args().nth(1).map(PathBuf::from).expect("usage: road_preview <output directory>");
    std::fs::create_dir_all(&out).expect("create output directory");
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Road preview".into(),
                resolution: bevy::window::WindowResolution::new(WIDTH, HEIGHT),
                resizable: false,
                ..default()
            }),
            ..default()
        }))
        .add_plugins((MipmapPlugin, MaterialPlugin::<RoadMaterial>::default()))
        .insert_resource(Output(out, std::env::args().nth(2).map(|s| s.split(',').map(str::to_string).collect()).unwrap_or_default()))
        .insert_resource(ClearColor(Color::srgb(0.55, 0.68, 0.82)))
        .add_systems(Startup, setup)
        .add_systems(Update, step)
        .run();
}

fn ribbon(kind: RoadKind, points: Vec<Vec2>, start_junction: bool, end_junction: bool) -> RoadRibbon {
    let half_widths = vec![half_width(kind); points.len()];
    RoadRibbon { kind, points, half_widths, start_junction, end_junction }
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut mips: ResMut<MipQueue>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut road_materials: ResMut<Assets<RoadMaterial>>,
) {
    commands.insert_resource(GlobalAmbientLight { color: Color::srgb(0.8, 0.88, 1.0), brightness: 2600.0, ..default() });
    commands.spawn((
        DirectionalLight { illuminance: bevy::light::light_consts::lux::RAW_SUNLIGHT, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, 2.2, -0.75, 0.0)),
    ));
    commands.spawn((
        Camera3d::default(),
        Transform::IDENTITY,
        Eye,
        bevy::camera::Exposure { ev100: 13.0 },
        bevy::core_pipeline::tonemapping::Tonemapping::TonyMcMapface,
        bevy::post_process::bloom::Bloom { intensity: 0.06, ..bevy::post_process::bloom::Bloom::NATURAL },
    ));

    // Flat ground, green.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(2000.0, 2000.0))),
        MeshMaterial3d(standard.add(StandardMaterial { base_color: Color::srgb(0.22, 0.32, 0.12), perceptual_roughness: 1.0, ..default() })),
    ));

    // The roads: a main road with a gentle bend running away down -Z, a track joining it from
    // the east, and a second main road forming a T further along.
    let bend = |z: f32| 14.0 * (z / 70.0).sin();
    let main: Vec<Vec2> = (0..=100).map(|i| { let z = 50.0 - i as f32 * 2.5; Vec2::new(bend(z), z) }).collect();
    let t_road: Vec<Vec2> = (0..=40).map(|i| Vec2::new(bend(-125.0) + i as f32 * 2.5, -125.0 - i as f32 * 0.4)).collect();
    let track: Vec<Vec2> = (0..=24).map(|i| Vec2::new(bend(-60.0) + i as f32 * 2.5, -60.0 + (i as f32 * 0.25).sin() * 3.0)).collect();
    let ribbons = vec![
        ribbon(RoadKind::Major, main, false, false),
        ribbon(RoadKind::Major, t_road, true, false),
        ribbon(RoadKind::Minor, track, true, false),
    ];
    let map = TerrainMap::flat(0.0);

    let load = |path: &'static str| -> Handle<Image> {
        asset_server
            .load_builder()
            .with_settings(|settings: &mut ImageLoaderSettings| {
                settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
                    address_mode_u: ImageAddressMode::Repeat,
                    address_mode_v: ImageAddressMode::Repeat,
                    anisotropy_clamp: 8,
                    ..ImageSamplerDescriptor::linear()
                });
            })
            .load(path)
    };
    let (asphalt, gravel) = (load("textures/pbr/asphalt.jpg"), load("textures/pbr/gravel.jpg"));
    mips.0.extend([asphalt.clone(), gravel.clone()]);
    let mut material = |surface: Handle<Image>, roughness: f32| {
        road_materials.add(RoadMaterial {
            base: StandardMaterial { base_color: Color::WHITE, perceptual_roughness: roughness, reflectance: 0.25, ..default() },
            extension: RoadExtension { surface },
        })
    };
    for (kind, surface, roughness) in [(RoadKind::Major, asphalt, 0.92), (RoadKind::Minor, gravel, 1.0)] {
        commands.spawn((Mesh3d(meshes.add(road_mesh(&map, &ribbons, kind))), MeshMaterial3d(material(surface, roughness))));
    }
}

// For each view: move the camera, let a few frames render, take the picture, go on.
fn step(
    mut commands: Commands,
    mut frame: Local<u32>,
    out: Res<Output>,
    mut eye: Query<&mut Transform, With<Eye>>,
    mut projections: Query<&mut Projection, With<Eye>>,
    mut exit: MessageWriter<AppExit>,
) {
    const PER_VIEW: u32 = 30;
    const WARMUP: u32 = 40;
    *frame += 1;
    if *frame < WARMUP {
        return;
    }
    let views: Vec<View> = views().into_iter().filter(|v| out.1.is_empty() || out.1.iter().any(|n| n == v.name)).collect();
    let index = ((*frame - WARMUP) / PER_VIEW) as usize;
    let within = (*frame - WARMUP) % PER_VIEW;
    let Some(view) = views.get(index) else {
        if *frame > WARMUP + PER_VIEW * views.len() as u32 + 60 {
            exit.write(AppExit::Success);
        }
        return;
    };
    if within == 0 {
        *eye.single_mut().unwrap() = view.camera;
        if let Projection::Perspective(p) = &mut *projections.single_mut().unwrap() {
            p.fov = view.fov;
        }
    }
    if within == PER_VIEW - 12 {
        let path = out.0.join(format!("{}.png", view.name));
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
}
