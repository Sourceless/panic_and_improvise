//! The crosshair: four short dashes in a cross, with a gap that opens to show how far a shot
//! could stray from the aim point (the gun's current hip-fire spread), and which disappears
//! completely once the gun is on its sights.
use bevy::prelude::*;

use crate::player::FpsCamera;
use crate::weapon::Gun;

const DASH_LENGTH: f32 = 9.0;
const DASH_THICKNESS: f32 = 2.0;
/// The gap never closes completely: the centre of the screen stays clear to aim by.
const MIN_GAP: f32 = 3.0;
/// How fast (per second, exponentially) the gap follows the spread.
const GAP_EASE: f32 = 18.0;
/// Past this much of the way onto the sights the crosshair is gone altogether.
const HIDDEN_AT_BLEND: f32 = 0.5;

/// Distance, in pixels, from the screen centre to the near end of each dash, for a gun whose shots
/// land within `spread` radians (half-angle) of the aim point, seen through a camera with vertical
/// field of view `fov` on a screen `screen_height` pixels tall.
pub fn gap_pixels(spread: f32, fov: f32, screen_height: f32) -> f32 {
    let on_screen = spread.tan() / (fov * 0.5).tan() * (screen_height * 0.5);
    on_screen.max(MIN_GAP)
}

/// How visible the crosshair is: fully while hip firing, fading as the gun comes up, gone on the
/// sights. `aim_blend` is 0 at the hip and 1 on the sights.
pub fn opacity(aim_blend: f32) -> f32 {
    (1.0 - aim_blend / HIDDEN_AT_BLEND).clamp(0.0, 1.0)
}

#[derive(Component)]
struct Dash {
    /// Which way from the centre this dash lies: one of the four axis directions.
    direction: Vec2,
}

pub struct CrosshairPlugin;

impl Plugin for CrosshairPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (spawn_crosshair, spawn_ammo_counter)).add_systems(Update, (update_crosshair, update_ammo_counter));
    }
}

fn spawn_crosshair(mut commands: Commands) {
    for direction in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
        let horizontal = direction.x != 0.0;
        let (width, height) = if horizontal { (DASH_LENGTH, DASH_THICKNESS) } else { (DASH_THICKNESS, DASH_LENGTH) };
        commands.spawn((
            Dash { direction },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(50.0),
                top: Val::Percent(50.0),
                width: Val::Px(width),
                height: Val::Px(height),
                ..default()
            },
            BackgroundColor(Color::WHITE),
        ));
    }
}

#[derive(Component)]
struct AmmoCounter;

fn spawn_ammo_counter(mut commands: Commands) {
    commands.spawn((
        AmmoCounter,
        Text::new(""),
        TextFont { font_size: bevy::text::FontSize::Px(30.0), ..default() },
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
        Node { position_type: PositionType::Absolute, right: Val::Px(28.0), bottom: Val::Px(22.0), ..default() },
    ));
}

/// What the ammo counter says: the rounds left in the magazine, or that a reload is going.
pub fn ammo_text(ammo: u32, reserve: u32, reloading: bool) -> String {
    if reloading {
        "RELOADING".to_string()
    } else if ammo == 0 && reserve == 0 {
        "0 / 0   NO AMMO".to_string()
    } else if ammo == 0 {
        // Nothing left to do but reload.
        format!("0 / {reserve}   R")
    } else {
        format!("{ammo} / {reserve}")
    }
}

fn update_ammo_counter(guns: Query<&Gun>, inventory: Res<crate::inventory::Inventory>, mut counters: Query<(&mut Text, &mut TextColor), With<AmmoCounter>>) {
    let Ok(gun) = guns.single() else { return };
    for (mut text, mut colour) in &mut counters {
        if gun.holstered {
            if !text.0.is_empty() {
                text.0.clear();
            }
            continue;
        }
        let reserve = inventory.rounds(gun.selected);
        // Which gun, with what, and how the trigger is set; a second line under the count.
        let load = gun.selected.def().short;
        let mode = if gun.def().modes.len() > 1 { format!("  {}", gun.mode.name()) } else { String::new() };
        let shown = format!("{}\n{}  {}{}", ammo_text(gun.ammo, reserve, gun.reloading()), gun.def().name, load, mode);
        if text.0 != shown {
            text.0 = shown;
        }
        // The last few rounds show in red.
        colour.0 = if gun.ammo <= 5 && !gun.reloading() { Color::srgb(1.0, 0.35, 0.3) } else { Color::srgba(1.0, 1.0, 1.0, 0.85) };
    }
}

fn update_crosshair(
    time: Res<Time>,
    windows: Query<&Window>,
    guns: Query<&Gun>,
    cameras: Query<&Projection, With<FpsCamera>>,
    mut dashes: Query<(&Dash, &mut Node, &mut BackgroundColor, &mut Visibility)>,
    mut gap: Local<f32>,
) {
    let (Ok(window), Ok(gun), Ok(Projection::Perspective(perspective))) = (windows.single(), guns.single(), cameras.single()) else {
        return;
    };
    // The gap is measured against the hip-fire field of view: the crosshair is only seen at the hip
    // (it's gone before the sights' zoom matters), but the fov still eases with sprinting.
    let target = gap_pixels(gun.current_spread, perspective.fov, window.height());
    *gap = if *gap == 0.0 { target } else { *gap + (target - *gap) * (1.0 - (-GAP_EASE * time.delta_secs()).exp()) };
    let alpha = opacity(gun.aim_blend);

    for (dash, mut node, mut colour, mut visibility) in &mut dashes {
        *visibility = if alpha <= 0.0 { Visibility::Hidden } else { Visibility::Inherited };
        colour.0 = Color::srgba(1.0, 1.0, 1.0, 0.9 * alpha);
        // Each dash is placed by its margins, from the centre: the near end `gap` away, the dash
        // extending outwards, and centred on its own axis across.
        let (width, height) = (DASH_LENGTH, DASH_THICKNESS);
        let along = |positive: bool, length: f32| Val::Px(if positive { *gap } else { -(*gap + length) });
        if dash.direction.x != 0.0 {
            node.margin = UiRect { left: along(dash.direction.x > 0.0, width), top: Val::Px(-height * 0.5), ..default() };
        } else {
            // Screen y points down, so "up" is negative.
            node.margin = UiRect { top: along(dash.direction.y < 0.0, width), left: Val::Px(-height * 0.5), ..default() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOV: f32 = std::f32::consts::FRAC_PI_2 * 0.8;

    #[test]
    fn the_ammo_counter_reads_rounds_or_reloading() {
        assert_eq!(ammo_text(30, 120, false), "30 / 120");
        assert_eq!(ammo_text(7, 0, false), "7 / 0");
        assert_eq!(ammo_text(7, 30, true), "RELOADING");
        assert!(ammo_text(0, 0, false).contains("NO AMMO"));
        assert!(ammo_text(0, 30, false).ends_with('R'), "an empty magazine says how to reload");
    }

    #[test]
    fn the_gap_grows_with_the_spread() {
        let tight = gap_pixels(0.005, FOV, 1080.0);
        let loose = gap_pixels(0.03, FOV, 1080.0);
        let wild = gap_pixels(0.06, FOV, 1080.0);
        assert!(tight < loose && loose < wild, "{tight} {loose} {wild}");
    }

    #[test]
    fn the_gap_matches_where_the_shots_land_on_screen() {
        // A shot at the edge of the cone appears this far from the centre: tan(spread) of the way
        // to the screen's half-height, per tan(fov/2).
        let gap = gap_pixels(0.03, FOV, 1000.0);
        let expected = 0.03_f32.tan() / (FOV / 2.0).tan() * 500.0;
        assert!((gap - expected).abs() < 0.01, "{gap} vs {expected}");
    }

    #[test]
    fn the_centre_always_stays_clear() {
        assert_eq!(gap_pixels(0.0, FOV, 1080.0), MIN_GAP);
    }

    #[test]
    fn a_bigger_screen_scales_the_gap() {
        assert!(gap_pixels(0.03, FOV, 2160.0) > 1.9 * gap_pixels(0.03, FOV, 1080.0));
    }

    #[test]
    fn the_crosshair_disappears_on_the_sights() {
        assert_eq!(opacity(0.0), 1.0);
        assert!(opacity(0.25) > 0.0 && opacity(0.25) < 1.0);
        assert_eq!(opacity(HIDDEN_AT_BLEND), 0.0);
        assert_eq!(opacity(1.0), 0.0);
    }
}
