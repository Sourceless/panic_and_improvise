//! The sound of the player's feet (recordings: see `tools/cut_footsteps.py`): a step every stride on whatever they are walking on, softer
//! crouched and crawling, louder at a run, and a thud on landing.

use std::collections::HashMap;

use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings, Volume};
use bevy::prelude::*;

use crate::collision::{Colliders, Material, STEP_UP};
use crate::map::{TerrainMap, CELL, HALF_SIZE};
use crate::player::{FpsCamera, Stance};
use crate::roads::{RoadClearance, RoadKind};
use crate::zones::{Zone, ZoneMap};

/// What a foot lands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Surface {
    Grass,
    Dirt,
    Gravel,
    Tarmac,
    Stone,
    Wood,
    Water,
}

impl Surface {
    pub const ALL: [Surface; 7] = [Surface::Grass, Surface::Dirt, Surface::Gravel, Surface::Tarmac, Surface::Stone, Surface::Wood, Surface::Water];

    /// How many recordings of a step on it there are (`sounds/footsteps/<name>_1.wav` and up).
    fn takes(self) -> usize {
        match self {
            Surface::Grass | Surface::Tarmac => 4,
            Surface::Dirt | Surface::Gravel | Surface::Stone | Surface::Wood => 3,
            Surface::Water => 2,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Surface::Grass => "grass",
            Surface::Dirt => "dirt",
            Surface::Gravel => "gravel",
            Surface::Tarmac => "tarmac",
            Surface::Stone => "stone",
            Surface::Wood => "wood",
            Surface::Water => "water",
        }
    }
}

/// What the ground is, from the solid under the feet if there is one, else from the roads and the land.
pub fn surface_under(p: Vec2, feet: f32, map: &TerrainMap, zones: Option<&ZoneMap>, colliders: &Colliders, roads: Option<&RoadClearance>) -> Surface {
    if let Some((top, material)) = colliders.support_material(p, feet, STEP_UP) {
        if feet - top < 0.3 {
            return match material {
                Material::Wood => Surface::Wood,
                Material::Leaves => Surface::Grass,
                Material::Stone | Material::Metal => Surface::Stone,
            };
        }
    }
    if map.water_surface_at(p).is_some_and(|level| feet < level + 0.1) {
        return Surface::Water;
    }
    match roads.and_then(|r| r.kind_at(p)) {
        Some(RoadKind::Major | RoadKind::Lane) => return Surface::Tarmac,
        Some(RoadKind::Minor) => return Surface::Gravel,
        Some(RoadKind::Path) => return Surface::Dirt,
        None => {}
    }
    let Some(zones) = zones else { return Surface::Grass };
    let n = map.grid_size();
    let ix = (((p.x + HALF_SIZE) / CELL).round().max(0.0) as usize).min(n - 1);
    let iz = (((p.y + HALF_SIZE) / CELL).round().max(0.0) as usize).min(n - 1);
    match zones.zone_at(ix, iz) {
        Zone::Arable | Zone::Woodland | Zone::Conifer => Surface::Dirt,
        Zone::Quarry | Zone::Military => Surface::Gravel,
        Zone::Industrial => Surface::Tarmac,
        _ => Surface::Grass,
    }
}

/// How far the player goes between one footfall and the next, and how loud each is.
pub fn stride(stance: Stance, speed: f32) -> (f32, f32) {
    match stance {
        Stance::Prone => (0.45, 0.1),
        Stance::Crouch => (0.7, 0.25),
        Stance::Stand if speed > 8.0 => (1.45, 0.85),
        Stance::Stand => (1.0, 0.5),
    }
}

/// The roads to tell tarmac from track from path underfoot.
#[derive(Resource)]
pub struct RoadSurfaces(pub RoadClearance);

#[derive(Resource)]
struct StepSounds {
    by_surface: HashMap<Surface, Vec<Handle<AudioSource>>>,
}

#[derive(Resource, Default)]
struct Walked {
    /// Metres since the last footfall.
    distance: f32,
    /// Whether the last frame was spent in the air, and how fast it was falling.
    airborne: bool,
    falling_speed: f32,
    last_variant: usize,
    luck: u32,
}

impl Walked {
    /// A small random number in 0..1 (a cheap generator: this only chooses between takes).
    fn random(&mut self) -> f32 {
        self.luck ^= self.luck << 13;
        self.luck ^= self.luck >> 17;
        self.luck ^= self.luck << 5;
        (self.luck >> 8) as f32 / (1u32 << 24) as f32
    }
}

pub struct FootstepsPlugin;

impl Plugin for FootstepsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Walked { luck: 0x9E37_79B9, ..default() }).add_systems(Startup, load_steps).add_systems(Update, footsteps);
    }
}

fn load_steps(mut commands: Commands, assets: Option<Res<AssetServer>>) {
    let Some(assets) = assets else { return };
    let by_surface = Surface::ALL.iter().map(|&s| (s, (1..=s.takes()).map(|i| assets.load(format!("sounds/footsteps/{}_{i}.wav", s.name()))).collect())).collect();
    commands.insert_resource(StepSounds { by_surface });
}

#[allow(clippy::too_many_arguments)]
fn footsteps(
    mut commands: Commands,
    time: Res<Time>,
    sounds: Option<Res<StepSounds>>,
    mut walked: ResMut<Walked>,
    player: Query<(&Transform, &FpsCamera)>,
    map: Option<Res<TerrainMap>>,
    zones: Option<Res<ZoneMap>>,
    colliders: Option<Res<Colliders>>,
    roads: Option<Res<RoadSurfaces>>,
) {
    let (Some(sounds), Some(map), Ok((transform, cam))) = (sounds, map, player.single()) else { return };
    let nothing = Colliders::default();
    let colliders: &Colliders = colliders.as_deref().unwrap_or(&nothing);
    let grounded = cam.air_height <= 0.001 && cam.vertical_speed() <= 0.0 && !cam.climbing();
    let dt = time.delta_secs();
    let p = Vec2::new(transform.translation.x, transform.translation.z);
    let feet = cam.floor + cam.air_height;
    let play = |commands: &mut Commands, walked: &mut Walked, volume: f32| {
        let surface = surface_under(p, feet, &map, zones.as_deref(), colliders, roads.as_deref().map(|r| &r.0));
        let Some(takes) = sounds.by_surface.get(&surface).filter(|t| !t.is_empty()) else { return };
        // Not the same take twice running.
        let mut k = (walked.random() * takes.len() as f32) as usize % takes.len();
        if k == walked.last_variant && takes.len() > 1 {
            k = (k + 1) % takes.len();
        }
        walked.last_variant = k;
        let speed = 0.93 + 0.14 * walked.random();
        commands.spawn((AudioPlayer(takes[k].clone()), PlaybackSettings { speed, volume: Volume::Linear(volume), ..PlaybackSettings::DESPAWN }));
    };
    if !grounded {
        walked.airborne = true;
        walked.falling_speed = walked.falling_speed.max(-cam.vertical_speed());
        return;
    }
    // Landing: a thud as loud as the fall was fast.
    if walked.airborne {
        let loudness = (walked.falling_speed / 9.0).clamp(0.0, 1.0);
        if loudness > 0.15 {
            play(&mut commands, &mut walked, 0.35 + 0.65 * loudness);
        }
        walked.airborne = false;
        walked.falling_speed = 0.0;
        walked.distance = 0.0;
        return;
    }
    // A slide scrapes rather than steps.
    if cam.sliding() {
        return;
    }
    let speed = cam.velocity.length();
    if speed < 0.4 {
        // Standing: the next step starts a short way into a stride.
        walked.distance = walked.distance.min(0.3);
        return;
    }
    let (length, volume) = stride(cam.stance, speed);
    walked.distance += speed * dt;
    if walked.distance >= length {
        walked.distance -= length;
        play(&mut commands, &mut walked, volume);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_covers_more_ground_between_steps_and_is_louder_than_a_walk() {
        let (walk_len, walk_vol) = stride(Stance::Stand, 6.0);
        let (run_len, run_vol) = stride(Stance::Stand, 10.8);
        assert!(run_len > walk_len && run_vol > walk_vol);
    }

    #[test]
    fn crouching_and_crawling_are_quieter_and_slower_paced() {
        let (_, stand) = stride(Stance::Stand, 6.0);
        let (crouch_len, crouch) = stride(Stance::Crouch, 3.3);
        let (prone_len, prone) = stride(Stance::Prone, 1.5);
        assert!(crouch < stand && prone < crouch);
        assert!(prone_len < crouch_len);
    }

    #[test]
    fn what_is_underfoot_comes_from_the_floor_first_then_roads_then_land() {
        let map = TerrainMap::flat(0.0);
        let mut colliders = Colliders::default();
        colliders.add(crate::collision::Solid::rect(Vec2::new(20.0, 0.0), Vec2::splat(3.0), 0.0, 0.2).of(Material::Wood));
        assert_eq!(surface_under(Vec2::new(20.0, 0.0), 0.2, &map, None, &colliders, None), Surface::Wood);
        assert_eq!(surface_under(Vec2::new(0.0, 0.0), 0.0, &map, None, &colliders, None), Surface::Grass);
    }
}
