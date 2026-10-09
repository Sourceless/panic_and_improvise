// A frame-time overlay (F3 to toggle) and a repeatable benchmark.
//
// Run the game with FPS_BENCH=1 to fly the camera through a fixed set of spots - the summit,
// the densest forest and the biggest village - with vsync off, print frame statistics for
// each, and exit. That gives before/after numbers for any change to how the world looks.

use bevy::app::AppExit;
use bevy::diagnostic::DiagnosticsStore;
use bevy::render::diagnostic::RenderDiagnosticsPlugin;
use bevy::render::view::window::screenshot::{save_to_disk, Screenshot};
use bevy::prelude::*;
use bevy::window::{PresentMode, PrimaryWindow};

use crate::grass::{Cover, GroundCover};
use crate::map::{PoiKind, TerrainMap};
use crate::player::FpsCamera;
use crate::vegetation::VegetationPlan;

const WINDOW_FRAMES: usize = 180;
const BENCH_WARMUP: f32 = 4.0;
const BENCH_MEASURE: f32 = 8.0;

pub struct PerfPlugin;

impl Plugin for PerfPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameStats>()
            .add_systems(Startup, setup_overlay)
            .add_systems(Update, (record_frame, update_overlay, toggle_overlay).chain());
        if let Ok(path) = std::env::var("FPS_SCREENSHOT") {
            // Immediate presentation, so a blanked or locked display can't stall the frames.
            app.insert_resource(ScreenshotPath(path))
                .add_systems(Startup, uncap_frame_rate)
                .add_systems(Update, auto_screenshot);
        }
        if std::env::var("FPS_BENCH").is_ok() {
            app.add_plugins(RenderDiagnosticsPlugin)
                .insert_resource(Bench::default())
                .add_systems(Startup, uncap_frame_rate)
                .add_systems(Update, run_bench);
        }
    }
}

#[derive(Resource, Default)]
struct FrameStats {
    // Milliseconds per frame, most recent last.
    recent: Vec<f32>,
}

impl FrameStats {
    fn average(&self) -> f32 {
        self.recent.iter().sum::<f32>() / self.recent.len().max(1) as f32
    }

    fn worst(&self) -> f32 {
        self.recent.iter().copied().fold(0.0, f32::max)
    }
}

#[derive(Component)]
struct PerfText;

fn setup_overlay(mut commands: Commands) {
    commands.spawn((
        Text::new(""),
        TextFont { font_size: bevy::text::FontSize::Px(16.0), ..default() },
        TextColor(Color::srgb(0.9, 1.0, 0.7)),
        Node { position_type: PositionType::Absolute, right: Val::Px(12.0), top: Val::Px(12.0), ..default() },
        // Hidden until F3 is pressed.
        Visibility::Hidden,
        PerfText,
    ));
}

fn record_frame(time: Res<Time<Real>>, mut stats: ResMut<FrameStats>) {
    stats.recent.push(time.delta_secs() * 1000.0);
    if stats.recent.len() > WINDOW_FRAMES {
        stats.recent.remove(0);
    }
}

fn update_overlay(stats: Res<FrameStats>, entities: Query<Entity>, mut text: Query<&mut Text, With<PerfText>>) {
    let Ok(mut text) = text.single_mut() else { return };
    if stats.recent.len() < 10 {
        return;
    }
    let avg = stats.average();
    text.0 = format!(
        "{:.0} fps   {:.1} ms avg   {:.1} ms worst\n{} entities",
        1000.0 / avg.max(0.001),
        avg,
        stats.worst(),
        entities.iter().count()
    );
}

fn toggle_overlay(keys: Res<ButtonInput<KeyCode>>, mut text: Query<&mut Visibility, With<PerfText>>) {
    if keys.just_pressed(KeyCode::F3) {
        if let Ok(mut v) = text.single_mut() {
            *v = if *v == Visibility::Hidden { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
}

fn uncap_frame_rate(mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    for mut window in &mut windows {
        window.present_mode = PresentMode::Immediate;
    }
}

// ---------------------------------------------------------------------------------------
// Benchmark

#[derive(Resource, Default)]
struct Bench {
    spots: Vec<(&'static str, Vec2, f32)>,
    step: usize,
    elapsed: f32,
    teleported: bool,
    samples: Vec<f32>,
    report: Vec<String>,
}

// Named places to stand: the summit, the densest forest and the biggest village.
fn camera_spots(map: &TerrainMap, plan: &VegetationPlan, cover: Option<&GroundCover>) -> Vec<(&'static str, Vec2, f32)> {
    let mut spots = vec![("summit", map.spawn_point(), 0.0)];
    if let Some(at) = cover.and_then(|c| c.find(Cover::Meadow).or_else(|| c.find(Cover::Pasture))) {
        spots.push(("meadow", at, 0.3));
    }
    if let Some(c) = plan.densest_chunk_centre() {
        spots.push(("forest", c, 0.6));
    }
    // A beach: low dry land beside the sea, near the edge of the map, facing out to sea.
    let n = map.grid_size();
    let mut beach: Option<(f32, Vec2)> = None;
    for iz in (0..n).step_by(2) {
        for ix in (0..n).step_by(2) {
            let q = crate::map::grid_pos(ix, iz);
            let edge = crate::map::HALF_SIZE - q.x.abs().max(q.y.abs());
            let h = map.vertex_height(ix, iz);
            if edge < 900.0 && (1.0..3.0).contains(&h) && map.water_level(ix, iz).is_none() && map.water_distance(q) < 14.0 && q.x > 0.0 && q.x.abs() > q.y.abs() {
                if beach.map_or(true, |(e, _)| edge < e) {
                    beach = Some((edge, q));
                }
            }
        }
    }
    if let Some((_, at)) = beach {
        // Facing +X, toward the east edge and the open sea.
        spots.push(("coast", at, -std::f32::consts::FRAC_PI_2));
    }
    if let Some(town) = map.pois.iter().filter(|p| p.kind == PoiKind::Village).max_by(|a, b| a.radius.total_cmp(&b.radius)) {
        // Stand on the edge of the village looking toward its centre.
        spots.push(("town", town.position + Vec2::new(0.0, town.radius * 0.8), 0.0));
    }
    spots
}

fn run_bench(
    time: Res<Time<Real>>,
    mut bench: ResMut<Bench>,
    map: Res<TerrainMap>,
    plan: Option<Res<VegetationPlan>>,
    cover: Option<Res<GroundCover>>,
    mut camera: Query<(&mut Transform, &mut FpsCamera)>,
    diagnostics: Res<DiagnosticsStore>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(plan) = plan else { return };
    let Ok((mut transform, mut cam)) = camera.single_mut() else { return };
    if bench.spots.is_empty() {
        let mut spots = camera_spots(&map, &plan, cover.as_deref());
        // FPS_BENCH_SPOTS=summit,forest limits the run to those spots.
        if let Ok(only) = std::env::var("FPS_BENCH_SPOTS") {
            spots.retain(|s| only.split(',').any(|o| o == s.0));
        }
        bench.spots = spots;
    }

    let dt = time.delta_secs();
    let (name, at, yaw) = bench.spots[bench.step];
    if !bench.teleported {
        let y = map.height_at(at) + 1.8;
        cam.yaw = yaw;
        cam.pitch = -0.04;
        transform.translation = Vec3::new(at.x, y, at.y);
        transform.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
        bench.teleported = true;
        bench.elapsed = 0.0;
        bench.samples.clear();
        return;
    }
    bench.elapsed += dt;
    if bench.elapsed > BENCH_WARMUP {
        bench.samples.push(dt * 1000.0);
    }
    if bench.elapsed < BENCH_WARMUP + BENCH_MEASURE {
        return;
    }
    let mut sorted = bench.samples.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let avg = sorted.iter().sum::<f32>() / sorted.len().max(1) as f32;
    let p95 = sorted.get(sorted.len() * 95 / 100).copied().unwrap_or(0.0);
    let worst = sorted.last().copied().unwrap_or(0.0);
    let line = format!(
        "BENCH {name:7} {:6.1} fps   avg {avg:5.1} ms   p95 {p95:5.1} ms   worst {worst:5.1} ms   ({} frames)",
        1000.0 / avg.max(0.001),
        sorted.len()
    );
    println!("{line}");
    // The heaviest GPU passes over the last couple of seconds, which is where the time goes.
    let mut gpu: Vec<(String, f64)> = diagnostics
        .iter()
        .filter(|d| d.path().as_str().ends_with("elapsed_gpu"))
        .filter_map(|d| d.average().map(|a| (d.path().as_str().to_string(), a)))
        .collect();
    gpu.sort_by(|a, b| b.1.total_cmp(&a.1));
    for (path, ms) in gpu.iter().take(5) {
        println!("BENCH   gpu {ms:6.2} ms  {}", path.trim_start_matches("render/").trim_end_matches("/elapsed_gpu"));
    }
    bench.report.push(line);
    bench.step += 1;
    bench.teleported = false;
    if bench.step >= bench.spots.len() {
        println!("BENCH done");
        exit.write(AppExit::Success);
    }
}

// ---------------------------------------------------------------------------------------
// Screenshot: FPS_SCREENSHOT=path.png saves the first-person view once the world has settled,
// then exits. Works without a display being visible.

#[derive(Resource)]
struct ScreenshotPath(String);

fn auto_screenshot(
    mut commands: Commands,
    mut frame: Local<u32>,
    path: Res<ScreenshotPath>,
    map: Res<TerrainMap>,
    plan: Option<Res<VegetationPlan>>,
    cover: Option<Res<GroundCover>>,
    mut camera: Query<(&mut Transform, &mut FpsCamera)>,
    mut exit: MessageWriter<AppExit>,
) {
    *frame += 1;
    // FPS_SPOT=forest|town|summit picks where to stand (default: the summit spawn).
    if *frame == 300 {
        if let (Ok(spot), Some(plan), Ok((mut transform, mut cam))) = (std::env::var("FPS_SPOT"), plan, camera.single_mut()) {
            if let Some(&(_, at, yaw)) = camera_spots(&map, &plan, cover.as_deref()).iter().find(|s| s.0 == spot) {
                cam.yaw = yaw + std::env::var("FPS_YAW").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                cam.pitch = -0.04;
                transform.translation = Vec3::new(at.x, map.height_at(at) + 1.8, at.y);
                transform.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
            }
        }
    }
    // FPS_AT=x,z stands anywhere (FPS_YAW turns, FPS_PITCH tips).
    if *frame == 300 {
        if let (Ok(at), Ok((mut transform, mut cam))) = (std::env::var("FPS_AT"), camera.single_mut()) {
            let mut parts = at.split(',').filter_map(|v| v.trim().parse::<f32>().ok());
            if let (Some(x), Some(z)) = (parts.next(), parts.next()) {
                cam.yaw = std::env::var("FPS_YAW").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
                cam.pitch = std::env::var("FPS_PITCH").ok().and_then(|v| v.parse().ok()).unwrap_or(-0.04);
                let ground = map.height_at(Vec2::new(x, z));
                transform.translation = Vec3::new(x, ground + std::env::var("FPS_HEIGHT").ok().and_then(|v| v.parse().ok()).unwrap_or(1.8), z);
                transform.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
            }
        }
    }
    // The world takes a while to build and stream in, so shoot well after startup.
    let first: u32 = std::env::var("FPS_SHOT_FRAME").ok().and_then(|v| v.parse().ok()).unwrap_or(900);
    if *frame == first {
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path.0.clone()));
    }
    // A second shot a little later (FPS_SCREENSHOT_2), to see things that move.
    if *frame == first + 40 {
        if let Ok(second) = std::env::var("FPS_SCREENSHOT_2") {
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(second));
        }
    }
    if *frame == first + 100 {
        exit.write(AppExit::Success);
    }
}
