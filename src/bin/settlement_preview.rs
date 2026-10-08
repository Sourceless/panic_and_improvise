// Renders a made-up settlement scene to PNG files, to see the buildings: a large village with its
// church, pubs, shops, school, hall and petrol station, a farm, and a mill. A flat ground and the
// real layout and mesh code. It opens an ordinary window (rendering with no window at all crashes the
// graphics driver here) but never touches the mouse, and closes itself when done.
//
//     cargo run --bin settlement_preview -- <output directory> [view names, comma separated]

use std::path::PathBuf;

use bevy::app::AppExit;
use bevy::image::{ImageAddressMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;
use bevy::render::view::window::screenshot::{save_to_disk, Screenshot};
use fps_prototype::collision::Colliders;
use fps_prototype::map::{Poi, PoiKind, TerrainMap};
use fps_prototype::mipmaps::{MipQueue, MipmapPlugin};
use fps_prototype::road_material::{RoadExtension, RoadMaterial};
use fps_prototype::roads::{half_width, road_mesh, RoadKind, RoadRibbon};
use fps_prototype::settlement::spawn_settlements;
use fps_prototype::settlement_plan::{BuildingKind, Layout, SettlementPlan};

#[derive(Resource)]
struct Output(PathBuf, Vec<String>);

#[derive(Component)]
struct Eye;

#[derive(Resource)]
struct Views(Vec<View>);

struct View {
    name: &'static str,
    camera: Transform,
    fov: f32,
}

fn main() {
    let out = std::env::args().nth(1).map(PathBuf::from).expect("usage: settlement_preview <output directory> [views]");
    std::fs::create_dir_all(&out).expect("create output directory");
    let only: Vec<String> = std::env::args().nth(2).map(|s| s.split(',').map(str::to_string).collect()).unwrap_or_default();
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Settlement preview".into(),
                resolution: bevy::window::WindowResolution::new(1280, 720),
                resizable: false,
                ..default()
            }),
            ..default()
        }))
        .add_plugins((MipmapPlugin, MaterialPlugin::<RoadMaterial>::default()))
        .insert_resource(Output(out, only))
        .insert_resource(ClearColor(Color::srgb(0.55, 0.68, 0.82)))
        .add_systems(Startup, setup)
        .add_systems(Update, step)
        .run();
}

fn ribbon(kind: RoadKind, points: Vec<Vec2>, start_junction: bool, end_junction: bool) -> RoadRibbon {
    let half_widths = vec![half_width(kind); points.len()];
    RoadRibbon { kind, points, half_widths, start_junction, end_junction }
}

fn line(from: Vec2, to: Vec2, step: f32) -> Vec<Vec2> {
    let n = (from.distance(to) / step).ceil().max(1.0) as usize;
    (0..=n).map(|i| from.lerp(to, i as f32 / n as f32)).collect()
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
    commands.spawn((DirectionalLight { illuminance: bevy::light::light_consts::lux::RAW_SUNLIGHT, shadow_maps_enabled: true, ..default() }, Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, 2.2, -0.75, 0.0))));
    commands.spawn((
        Camera3d::default(),
        Transform::IDENTITY,
        Eye,
        bevy::camera::Exposure { ev100: 13.0 },
        bevy::core_pipeline::tonemapping::Tonemapping::TonyMcMapface,
        bevy::post_process::bloom::Bloom { intensity: 0.06, ..bevy::post_process::bloom::Bloom::NATURAL },
    ));
    // The ground, at the height the (flat) map says it is.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(4000.0, 4000.0))),
        MeshMaterial3d(standard.add(StandardMaterial { base_color: Color::srgb(0.22, 0.32, 0.12), perceptual_roughness: 1.0, ..default() })),
        Transform::from_xyz(0.0, 20.0, 0.0),
    ));

    // A flat map with three settlements on it: a large village at the middle, a farm and a mill.
    let mut map = TerrainMap::flat(20.0);
    map.pois = vec![
        Poi { kind: PoiKind::Village, position: Vec2::ZERO, landmark: Vec2::new(-40.0, 25.0), radius: 200.0 },
        Poi { kind: PoiKind::Farm, position: Vec2::new(800.0, 0.0), landmark: Vec2::new(800.0, 0.0), radius: 34.0 },
        Poi { kind: PoiKind::Mill, position: Vec2::new(-800.0, 0.0), landmark: Vec2::new(-800.0, 0.0), radius: 10.0 },
    ];
    // Roads cross at the middle of the town, one running east and west and one north and south.
    let main = ribbon(RoadKind::Major, line(Vec2::new(-260.0, 0.0), Vec2::new(260.0, 0.0), 5.0), false, false);
    let north_south = ribbon(RoadKind::Major, line(Vec2::new(0.0, -260.0), Vec2::new(0.0, 260.0), 5.0), false, false);
    let track = ribbon(RoadKind::Minor, line(Vec2::new(700.0, 0.0), Vec2::new(802.0, 0.0), 5.0), false, false);
    let mill_road = ribbon(RoadKind::Major, line(Vec2::new(-900.0, 0.0), Vec2::new(-800.0, 0.0), 5.0), false, false);
    let mut ribbons = vec![main, north_south, track, mill_road];
    let plan = SettlementPlan { layouts: map.pois.iter().map(|poi| Layout::generate(&map, poi, &ribbons)).collect() };
    ribbons.extend(plan.lanes());
    eprintln!(
        "{} buildings: {:?}",
        plan.layouts.iter().map(|l| l.buildings.len()).sum::<usize>(),
        [BuildingKind::Church, BuildingKind::Pub, BuildingKind::Shop, BuildingKind::School, BuildingKind::Hall, BuildingKind::PetrolStation]
            .map(|k| plan.layouts[0].buildings.iter().filter(|b| b.kind == k).count())
    );

    let load = |path: &'static str| -> Handle<Image> {
        asset_server
            .load_builder()
            .with_settings(|settings: &mut ImageLoaderSettings| {
                settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor { address_mode_u: ImageAddressMode::Repeat, address_mode_v: ImageAddressMode::Repeat, anisotropy_clamp: 8, ..ImageSamplerDescriptor::linear() });
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
    for (kind, surface, roughness) in [(RoadKind::Major, asphalt.clone(), 0.92), (RoadKind::Lane, asphalt, 0.92), (RoadKind::Minor, gravel, 1.0)] {
        commands.spawn((Mesh3d(meshes.add(road_mesh(&map, &ribbons, kind))), MeshMaterial3d(material(surface, roughness))));
    }
    let mut colliders = Colliders::default();
    spawn_settlements(&mut commands, &mut meshes, &mut standard, &map, &plan, &mut colliders);

    // Views: the whole town from the air, along the main street, and each kind of building close up.
    let at = |name, from: Vec3, to: Vec3, fov| View { name, camera: Transform::from_translation(from).looking_at(to, Vec3::Y), fov };
    let mut views = vec![
        at("aerial", Vec3::new(0.0, 330.0, 330.0), Vec3::new(0.0, 20.0, 0.0), 0.9),
        at("street", Vec3::new(-90.0, 22.0, 6.0), Vec3::new(40.0, 22.0, -2.0), 1.0),
        at("farm", Vec3::new(800.0, 38.0, 70.0), Vec3::new(800.0, 22.0, 0.0), 0.95),
        at("mill", Vec3::new(-800.0, 30.0, 50.0), Vec3::new(-800.0, 24.0, 0.0), 0.95),
    ];
    let buildings = &plan.layouts[0].buildings;
    for (name, kind) in [("pub", BuildingKind::Pub), ("shop", BuildingKind::Shop), ("church", BuildingKind::Church), ("school", BuildingKind::School), ("petrol", BuildingKind::PetrolStation)] {
        let Some(b) = buildings.iter().find(|b| b.kind == kind) else { continue };
        let c = Vec3::new(b.centre.x, 20.0, b.centre.y);
        // A spot in front of it and a bit to one side, 30 m out, that is not inside anything.
        let candidate = [0.0f32, 0.5, -0.5, 1.0, -1.0].iter().flat_map(|&across| [30.0f32, 38.0, 46.0].map(|out| (across, out))).find_map(|(across, out)| {
            let p = b.centre + b.front * out + Vec2::new(-b.front.y, b.front.x) * across * out * 0.5;
            buildings.iter().all(|o| o.shape().separation(p).0 > 6.0).then_some(p)
        });
        if let Some(p) = candidate {
            views.push(at(name, Vec3::new(p.x, 27.0, p.y), c + Vec3::Y * 2.5, 0.85));
        }
    }
    commands.insert_resource(Views(views));
}

fn step(
    mut commands: Commands,
    mut frame: Local<u32>,
    out: Res<Output>,
    views: Res<Views>,
    mut eye: Query<&mut Transform, With<Eye>>,
    mut projections: Query<&mut Projection, With<Eye>>,
    mut exit: MessageWriter<AppExit>,
) {
    const PER_VIEW: u32 = 30;
    const WARMUP: u32 = 60;
    *frame += 1;
    if *frame < WARMUP {
        return;
    }
    let shown: Vec<&View> = views.0.iter().filter(|v| out.1.is_empty() || out.1.iter().any(|n| n == v.name)).collect();
    let index = ((*frame - WARMUP) / PER_VIEW) as usize;
    let within = (*frame - WARMUP) % PER_VIEW;
    let Some(view) = shown.get(index) else {
        if *frame > WARMUP + PER_VIEW * shown.len() as u32 + 60 {
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
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(out.0.join(format!("{}.png", view.name))));
    }
}
