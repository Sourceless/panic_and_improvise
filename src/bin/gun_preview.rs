// Renders the gun to PNG files: how it looks in first person at the hip and on the sights,
// and from a few angles around it. It opens an ordinary window (rendering with no window at all
// crashes the graphics driver here) but never touches the mouse, and closes itself when done.
//
//     cargo run --bin gun_preview -- <output directory>

use std::path::PathBuf;
use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::render::view::window::screenshot::{save_to_disk, Screenshot};
use fps_prototype::gun_model;
use fps_prototype::weapon::{gun_pose, gun_transform};

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

#[derive(Resource)]
struct Output(PathBuf);

#[derive(Component)]
struct Gun;

#[derive(Component)]
struct Eye;

struct View {
    name: &'static str,
    camera: Transform,
    gun: Transform,
}

fn views() -> Vec<View> {
    let first_person = |name, gun| View { name, camera: Transform::IDENTITY, gun };
    let orbit = |name, at: Vec3| View {
        name,
        camera: Transform::from_translation(at + Vec3::new(0.0, 0.0, -0.15)).looking_at(Vec3::new(-0.02, 0.0, -0.15), Vec3::Y),
        gun: Transform::IDENTITY,
    };
    vec![
        first_person("hip", gun_transform(0.0)),
        first_person("sights", gun_transform(1.0)),
        first_person("recoil", gun_pose(0.0, 1.0)),
        orbit("right", Vec3::new(0.85, 0.06, 0.0)),
        orbit("left", Vec3::new(-0.85, 0.06, 0.0)),
        View {
            name: "three_quarter",
            camera: Transform::from_xyz(-0.55, 0.28, 0.5).looking_at(Vec3::new(-0.02, -0.01, -0.15), Vec3::Y),
            gun: Transform::IDENTITY,
        },
        View {
            name: "front",
            camera: Transform::from_xyz(0.25, 0.12, -1.0).looking_at(Vec3::new(0.0, 0.0, -0.3), Vec3::Y),
            gun: Transform::IDENTITY,
        },
    ]
}

fn main() {
    let out = std::env::args().nth(1).map(PathBuf::from).expect("usage: gun_preview <output directory>");
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
        .insert_resource(Output(out))
        .insert_resource(ClearColor(Color::srgb(0.55, 0.68, 0.82)))
        .add_systems(Startup, setup)
        .add_systems(Update, step)
        .run();
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(GlobalAmbientLight { color: Color::srgb(0.8, 0.88, 1.0), brightness: 350.0, ..default() });
    commands.spawn((
        DirectionalLight { illuminance: 11_000.0, ..default() },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, 2.2, -0.75, 0.0)),
    ));
    commands.spawn((Camera3d::default(), Transform::IDENTITY, Eye));

    let (metal, plastic, dark) = gun_model::build();
    let finish = |c: Color, rough: f32, metallic: f32| StandardMaterial { base_color: c, perceptual_roughness: rough, metallic, ..default() };
    commands
        .spawn((Gun, Transform::IDENTITY, Visibility::default()))
        .with_children(|gun| {
            for (parts, material) in [
                (metal, finish(Color::srgb(0.13, 0.135, 0.145), 0.45, 0.75)),
                (plastic, finish(Color::srgb(0.035, 0.035, 0.04), 0.35, 0.0)),
                (dark, finish(Color::srgb(0.008, 0.008, 0.01), 0.9, 0.0)),
            ] {
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
    mut gun: Query<&mut Transform, (With<Gun>, Without<Eye>)>,
    mut exit: MessageWriter<AppExit>,
) {
    const PER_VIEW: u32 = 30;
    const WARMUP: u32 = 30;
    *frame += 1;
    if *frame < WARMUP {
        return;
    }
    let views = views();
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
        *gun.single_mut().unwrap() = view.gun;
    }
    if within == PER_VIEW - 12 {
        let path = out.0.join(format!("{}.png", view.name));
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
}
