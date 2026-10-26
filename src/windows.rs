//! Window glass. A pane is solid until it breaks: a bullet breaks it and goes on, and so does a body
//! that runs into it. It goes with a crash, a spray of shards and a tinkle a moment later; once broken,
//! the window is a gap with a sill to climb through.

use bevy::audio::AudioSource;
use bevy::prelude::*;

use crate::collision::{Colliders, Material, PLAYER_RADIUS};
use crate::impact::{Impact, Surface};
use crate::settlement::Buildings;
use crate::sound::{arrival_delay, play_after};

/// How fast a body has to be pushing at a pane, m/s, to break it. (Creeping up to the glass does not.)
pub const BREAKING_SPEED: f32 = 1.5;
/// How far beyond the surface of a body a pane is still being pushed at.
const TOUCH: f32 = 0.08;
/// How high up the body the glass is looked for, above the feet.
const WAIST: f32 = 1.0;
/// How far from where a bullet landed the pane it broke is looked for, metres.
const BULLET_REACH: f32 = 0.15;
/// How fast the shards of a pane broken by a body fly (as a bullet's speed would, for the spray's strength).
const SHARD_SPEED: f32 = 150.0;

/// What the player pushed at on the latest frame without getting anywhere: set by the player's
/// movement, read by the glass.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct Bump(pub Option<Pushed>);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pushed {
    /// Where their middle was left, and how high their feet were.
    pub at: Vec2,
    pub feet: f32,
    /// The way they were pushing (a unit vector), and how fast.
    pub into: Vec2,
    pub speed: f32,
}

#[derive(Resource)]
struct GlassSounds {
    crashes: Vec<Handle<AudioSource>>,
    cracks: Vec<Handle<AudioSource>>,
    tinkle: Handle<AudioSource>,
}

pub struct WindowsPlugin;

impl Plugin for WindowsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Bump>().add_systems(Startup, load_sounds).add_systems(
            Update,
            (
                break_by_bullets.after(crate::weapon::move_bullets),
                break_by_bodies.after(crate::player::player_movement),
            ),
        );
    }
}

fn load_sounds(mut commands: Commands, assets: Option<Res<AssetServer>>) {
    let Some(assets) = assets else { return };
    let load = |name: &str, count: usize| -> Vec<Handle<AudioSource>> { (1..=count).map(|i| assets.load(format!("sounds/glass/{name}_{i}.wav"))).collect() };
    commands.insert_resource(GlassSounds { crashes: load("break", 4), cracks: load("shot", 3), tinkle: assets.load("sounds/glass/tinkle.wav") });
}

/// Which of `count` to pick, from where it happens: the same break always sounds the same, and two beside each
/// other differ.
fn pick(count: usize, at: Vec3) -> usize {
    let h = (at.x * 131.0).to_bits() ^ (at.y * 311.0).to_bits().rotate_left(9) ^ (at.z * 173.0).to_bits().rotate_left(17);
    (h.wrapping_mul(0x9E37_79B1) >> 8) as usize % count.max(1)
}

/// Breaks a pane: it is no longer there to hit, or to see, and the crash goes up from where it was. Returns where
/// the pane was and which way it faced.
#[allow(clippy::too_many_arguments)]
fn shatter(
    id: u32,
    by_a_bullet: bool,
    commands: &mut Commands,
    colliders: &mut Colliders,
    buildings: &mut Buildings,
    meshes: &mut Assets<Mesh>,
    sounds: Option<&GlassSounds>,
    ear: Option<Vec3>,
) -> Option<(Vec3, Vec3)> {
    let (centre, normal) = buildings.break_pane(id, meshes, commands)?;
    colliders.remove(id);
    if let Some(sounds) = sounds {
        let delay = arrival_delay(ear.map_or(0.0, |e| e.distance(centre)));
        let (pool, volume) = if by_a_bullet { (&sounds.cracks, 0.9) } else { (&sounds.crashes, 1.0) };
        play_after(commands, delay, pool[pick(pool.len(), centre)].clone(), 1.0, volume, Some(centre));
        // The last of it falls a moment later.
        play_after(commands, delay + 0.3 + 0.2 * (pick(4, centre) as f32 / 3.0), sounds.tinkle.clone(), 0.9 + 0.1 * (pick(3, centre) as f32), 0.5, Some(centre));
    }
    Some((centre, normal))
}

/// A bullet that hit a pane breaks it (the bullet goes on; see the weapon's flight).
#[allow(clippy::too_many_arguments)]
fn break_by_bullets(
    mut commands: Commands,
    mut impacts: MessageReader<Impact>,
    colliders: Option<ResMut<Colliders>>,
    buildings: Option<ResMut<Buildings>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    sounds: Option<Res<GlassSounds>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
) {
    let (Some(mut colliders), Some(mut buildings), Some(mut meshes)) = (colliders, buildings, meshes) else { return };
    let ear = camera.single().ok().map(|c| c.translation());
    for impact in impacts.read() {
        if impact.surface != Surface::Solid(Material::Glass) {
            continue;
        }
        if let Some(id) = colliders.glass_near(impact.position, BULLET_REACH, None) {
            shatter(id, true, &mut commands, &mut colliders, &mut buildings, &mut meshes, sounds.as_deref(), ear);
        }
    }
}

/// A body that runs into a pane breaks it, and goes through if it keeps on.
#[allow(clippy::too_many_arguments)]
fn break_by_bodies(
    mut commands: Commands,
    mut bump: ResMut<Bump>,
    mut impacts: MessageWriter<Impact>,
    colliders: Option<ResMut<Colliders>>,
    buildings: Option<ResMut<Buildings>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    sounds: Option<Res<GlassSounds>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
) {
    let Some(pushed) = bump.0.take() else { return };
    let (Some(mut colliders), Some(mut buildings), Some(mut meshes)) = (colliders, buildings, meshes) else { return };
    if pushed.speed < BREAKING_SPEED {
        return;
    }
    let waist = Vec3::new(pushed.at.x, pushed.feet + WAIST, pushed.at.y);
    let Some(id) = colliders.glass_near(waist, PLAYER_RADIUS + TOUCH, Some(pushed.into)) else { return };
    let ear = camera.single().ok().map(|c| c.translation());
    let Some((centre, _)) = shatter(id, false, &mut commands, &mut colliders, &mut buildings, &mut meshes, sounds.as_deref(), ear) else { return };
    // The shards go on the way the body was going.
    impacts.write(Impact { position: centre, normal: Vec3::new(pushed.into.x, 0.0, pushed.into.y), surface: Surface::Solid(Material::Glass), speed: SHARD_SPEED, target: None });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::Colliders;

    fn app() -> (App, u32) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default())).init_asset::<Mesh>().init_asset::<AudioSource>().init_resource::<Colliders>().add_message::<Impact>().add_plugins(WindowsPlugin);
        let pane = Buildings::add_one_window(app.world_mut());
        (app, pane)
    }

    fn push(app: &mut App, speed: f32, into: Vec2) {
        app.world_mut().resource_mut::<Bump>().0 = Some(Pushed { at: Vec2::new(0.0, 0.46), feet: 0.0, into, speed });
        app.update();
    }

    fn whole(app: &App, pane: u32) -> bool {
        app.world().resource::<Colliders>().glass_near(Vec3::new(0.0, 1.5, 0.05), 0.3, None) == Some(pane)
    }

    #[test]
    fn running_into_a_pane_breaks_it_and_creeping_up_to_it_does_not() {
        let (mut app, pane) = app();
        push(&mut app, BREAKING_SPEED * 0.5, Vec2::NEG_Y);
        assert!(whole(&app, pane), "a slow push leaves it");
        push(&mut app, 4.0, Vec2::Y);
        assert!(whole(&app, pane), "pushing away from it does nothing");
        push(&mut app, 4.0, Vec2::NEG_Y);
        assert!(!whole(&app, pane), "a run at it breaks it");
        let sprays = app.world().resource::<Messages<Impact>>().iter_current_update_messages().filter(|i| i.surface == Surface::Solid(Material::Glass)).count();
        assert_eq!(sprays, 1, "with a spray of shards");
        // And the player is no longer stopped there.
        let colliders = app.world().resource::<Colliders>();
        let was = Vec2::new(0.0, 0.1);
        assert_eq!(colliders.resolve(was, PLAYER_RADIUS, 0.0, crate::collision::STEP_UP), was);
    }

    #[test]
    fn a_bullet_that_hits_a_pane_breaks_it() {
        let (mut app, pane) = app();
        app.world_mut().write_message(Impact { position: Vec3::new(0.3, 1.4, 0.05), normal: Vec3::Z, surface: Surface::Solid(Material::Stone), speed: 400.0, target: None });
        app.update();
        assert!(whole(&app, pane), "a hit on stone leaves the glass");
        app.world_mut().write_message(Impact { position: Vec3::new(0.3, 1.4, 0.05), normal: Vec3::Z, surface: Surface::Solid(Material::Glass), speed: 400.0, target: None });
        app.update();
        assert!(!whole(&app, pane));
    }

    #[test]
    fn the_same_break_sounds_the_same_and_the_sounds_vary() {
        let a = Vec3::new(10.0, 1.4, -3.0);
        assert_eq!(pick(4, a), pick(4, a));
        let kinds: std::collections::HashSet<usize> = (0..40).map(|i| pick(4, a + Vec3::new(i as f32 * 3.7, 0.0, 0.0))).collect();
        assert!(kinds.len() >= 3, "{kinds:?}");
        assert_eq!(pick(0, a), 0);
    }
}
