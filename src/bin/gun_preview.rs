// Renders a gun (the Sterling, or the one named second: hi_power, slr, bren, mag, l42, auto5, lee_enfield) to PNG files: how it looks in first person at the hip and on the sights,
// and from a few angles around it. It opens an ordinary window (rendering with no window at all
// crashes the graphics driver here) but never touches the mouse, and closes itself when done.
//
//     cargo run --bin gun_preview -- <output directory> [gun]

use std::path::PathBuf;
use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::render::view::window::screenshot::{save_to_disk, Screenshot};
use fps_prototype::gun_models::{self, Finish};
use fps_prototype::weapon::{gun_pose, gun_transform};
use fps_prototype::weapons::WeaponKind;

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

#[derive(Resource)]
struct Output(PathBuf, WeaponKind);

fn kind_named(name: &str) -> WeaponKind {
    match name {
        "hi_power" => WeaponKind::HiPower,
        "slr" => WeaponKind::Slr,
        "bren" => WeaponKind::Bren,
        "mag" => WeaponKind::Mag,
        "l42" => WeaponKind::L42,
        "auto5" => WeaponKind::Auto5,
        "lee_enfield" => WeaponKind::LeeEnfield,
        _ => WeaponKind::Sterling,
    }
}

#[derive(Component)]
struct Gun;

#[derive(Component)]
struct Eye;

struct View {
    name: &'static str,
    camera: Transform,
    gun: Transform,
    /// Vertical field of view in radians; a very narrow one makes a near-orthographic photo-style view.
    fov: f32,
    /// Show the muzzle flash (and its light) in this view.
    flash: bool,
}

fn views(kind: WeaponKind) -> Vec<View> {
    const FOV: f32 = 0.7854;
    let (spec, handling) = (gun_models::build(kind).spec, kind.def().handling);
    // The views were framed for a gun about half a metre long; longer and shorter ones are stood back or brought in.
    let long = (-spec.muzzle.z / 0.5).max(0.45);
    let mid_z = spec.muzzle.z * 0.3;
    let first_person = |name, gun| View { name, camera: Transform::IDENTITY, gun, fov: FOV, flash: false };
    let flashing = |name, gun| View { name, camera: Transform::IDENTITY, gun, fov: FOV, flash: true };
    // Photo-style side views: a long, narrow lens from 3.5 m, looking square at the gun.
    let side = |name, x: f32| View {
        name,
        camera: Transform::from_xyz(x * long.max(1.0), 0.0, mid_z).looking_at(Vec3::new(0.0, 0.0, mid_z), Vec3::Y),
        gun: Transform::IDENTITY,
        fov: 0.2,
        flash: false,
    };
    vec![
        first_person("hip", gun_transform(&spec, &handling, 0.0)),
        first_person("sights", gun_transform(&spec, &handling, 1.0)),
        first_person("recoil", gun_pose(&spec, &handling, 0.0, 1.0)),
        flashing("flash_hip", gun_pose(&spec, &handling, 0.0, 1.0)),
        flashing("flash_sights", gun_pose(&spec, &handling, 1.0, 1.0)),
        View { name: "flash_side", camera: Transform::from_xyz(2.2 * long.max(1.0), 0.0, spec.muzzle.z).looking_at(Vec3::new(0.0, 0.0, spec.muzzle.z), Vec3::Y), gun: Transform::IDENTITY, fov: 0.35, flash: true },
        side("right", 3.5),
        side("left", -3.5),
        View {
            name: "three_quarter",
            camera: Transform::from_xyz(-0.55 * long, 0.28 * long, 0.5 * long).looking_at(Vec3::new(-0.02, -0.01, mid_z), Vec3::Y),
            gun: Transform::IDENTITY,
            fov: FOV,
            flash: false,
        },
        View {
            name: "front",
            camera: Transform::from_xyz(0.3 * long, 0.1 * long, spec.muzzle.z - 0.6 * long).looking_at(Vec3::new(0.0, 0.0, mid_z), Vec3::Y),
            gun: Transform::IDENTITY,
            fov: FOV,
            flash: false,
        },
        View {
            name: "rear_left",
            camera: Transform::from_xyz(-0.35 * long, 0.25 * long, 0.75 * long).looking_at(Vec3::new(-0.04, 0.0, mid_z), Vec3::Y),
            gun: Transform::IDENTITY,
            fov: FOV,
            flash: false,
        },
    ]
}

fn main() {
    let out = std::env::args().nth(1).map(PathBuf::from).expect("usage: gun_preview <output directory> [gun]");
    let kind = std::env::args().nth(2).map_or(WeaponKind::Sterling, |n| kind_named(&n));
    std::fs::create_dir_all(&out).expect("create output directory");
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Gun preview".into(),
                resolution: bevy::window::WindowResolution::new(WIDTH, HEIGHT),
                resizable: false,
                ..default()
            }),
            ..default()
        }))
        .insert_resource(Output(out, kind))
        .insert_resource(ClearColor(Color::srgb(0.55, 0.68, 0.82)))
        .add_systems(Startup, setup)
        .add_systems(Update, step)
        .run();
}

fn setup(mut commands: Commands, out: Res<Output>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(GlobalAmbientLight { color: Color::srgb(0.8, 0.88, 1.0), brightness: 2600.0, ..default() });
    commands.spawn((
        DirectionalLight { illuminance: bevy::light::light_consts::lux::RAW_SUNLIGHT, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, 2.2, -0.75, 0.0)),
    ));
    // The same exposure, tonemapping and bloom as the game's camera, so the flash is judged as it
    // will look there.
    commands.spawn((
        Camera3d::default(),
        Transform::IDENTITY,
        Eye,
        bevy::camera::Exposure { ev100: 13.0 },
        bevy::core_pipeline::tonemapping::Tonemapping::TonyMcMapface,
        bevy::post_process::bloom::Bloom { intensity: 0.06, ..bevy::post_process::bloom::Bloom::NATURAL },
    ));

    let model = gun_models::build(out.1);
    let finish = |c: Color, rough: f32, metallic: f32| StandardMaterial { base_color: c, perceptual_roughness: rough, metallic, ..default() };
    commands
        .spawn((Gun, Transform::IDENTITY, Visibility::default()))
        .with_children(|gun| {
            fps_prototype::muzzle_flash::spawn(gun, &mut meshes, &mut materials, model.spec.muzzle, false);
            for (f, parts) in model.parts {
                let material = match f {
                    Finish::Metal => finish(Color::srgb(0.13, 0.135, 0.145), 0.45, 0.75),
                    Finish::Black => finish(Color::srgb(0.035, 0.035, 0.04), 0.35, 0.0),
                    Finish::Dark => finish(Color::srgb(0.008, 0.008, 0.01), 0.9, 0.0),
                    Finish::Wood => finish(Color::srgb(0.2, 0.1, 0.05), 0.6, 0.0),
                    Finish::Olive => finish(Color::srgb(0.14, 0.17, 0.08), 0.6, 0.3),
                    Finish::Glass => finish(Color::srgb(0.05, 0.08, 0.12), 0.08, 0.3),
                };
                gun.spawn((Mesh3d(meshes.add(parts.into_mesh())), MeshMaterial3d(materials.add(material))));
            }
        });
}

// For each view: move the camera and gun, let a few frames render, take the picture, go on.
fn step(
    mut commands: Commands,
    mut frame: Local<u32>,
    out: Res<Output>,
    mut eye: Query<&mut Transform, (With<Eye>, Without<Gun>)>,
    mut projections: Query<&mut Projection, With<Eye>>,
    mut flash: Query<&mut Visibility, With<fps_prototype::muzzle_flash::MuzzleFlash>>,
    mut flash_light: Query<&mut PointLight, With<fps_prototype::muzzle_flash::MuzzleFlashLight>>,
    mut gun: Query<&mut Transform, (With<Gun>, Without<Eye>)>,
    mut exit: MessageWriter<AppExit>,
) {
    const PER_VIEW: u32 = 30;
    const WARMUP: u32 = 30;
    *frame += 1;
    if *frame < WARMUP {
        return;
    }
    let views = views(out.1);
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
        *gun.single_mut().unwrap() = view.gun;
        for mut v in &mut flash {
            *v = if view.flash { Visibility::Inherited } else { Visibility::Hidden };
        }
        for mut l in &mut flash_light {
            l.intensity = if view.flash { 3_000_000.0 } else { 0.0 };
        }
    }
    if within == PER_VIEW - 12 {
        let path = out.0.join(format!("{}.png", view.name));
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
}
